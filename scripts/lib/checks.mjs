// Compatibility checks for built plugins: cross-references used by dfw,
// "no upstream hooks" invariant, and conflict lint of upstream instructions.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { REPO_ROOT, RETIRED_PLUGINS, readJson, listFiles, rootDefaults } from './build.mjs';
import { split, topLevelKeys } from './frontmatter.mjs';

/** Skills and agents a plugin directory provides, where its Claude Code manifest says they are. */
export function provided(pluginDir) {
  const pj = fs.existsSync(path.join(pluginDir, '.claude-plugin', 'plugin.json'))
    ? readJson(path.join(pluginDir, '.claude-plugin', 'plugin.json'))
    : {};
  const skillsRoot = path.join(pluginDir, pj.skills || 'skills');
  const skills = fs.existsSync(skillsRoot)
    ? fs.readdirSync(skillsRoot).filter((d) => fs.existsSync(path.join(skillsRoot, d, 'SKILL.md')))
    : [];
  // `agents`: a folder, or a list of .md files (the dfw plugin lists its files one by one).
  const agentsDir = path.join(pluginDir, typeof pj.agents === 'string' ? pj.agents : 'agents');
  const files = Array.isArray(pj.agents)
    ? pj.agents.map((f) => path.join(pluginDir, f)).filter((f) => fs.existsSync(f))
    : fs.existsSync(agentsDir)
      ? fs.readdirSync(agentsDir).map((f) => path.join(agentsDir, f))
      : [];
  const agents = files
    .filter((f) => f.endsWith('.md'))
    .map((f) => {
      const m = fs.readFileSync(f, 'utf8').match(/^name:\s*(\S+)/m);
      return m ? m[1] : path.basename(f).replace(/\.md$/, '');
    });
  return { skills, agents };
}

function dfwTextFiles(pluginsRoot) {
  const dfw = path.join(pluginsRoot, 'dfw');
  return listFiles(dfw)
    .filter((r) => /\.(md|mjs|json)$/.test(r) && !r.startsWith('templates/'))
    .map((r) => ({ rel: `plugins/dfw/${r}`, text: fs.readFileSync(path.join(dfw, r), 'utf8') }));
}

/**
 * Cross-reference check: every `plugin:skill` named by dfw must exist, and every
 * file listed in registry/xref.json must exist in the built plugin.
 */
export function xrefCheck({ pluginsRoot = path.join(REPO_ROOT, 'plugins'), root = REPO_ROOT, overrides = {} } = {}) {
  const registry = readJson(path.join(root, 'registry', 'skills.json'));
  const names = new Set([...registry.skills.map((s) => s.plugin), 'dfw']);
  const dirOf = (plugin) => overrides[plugin] || path.join(pluginsRoot, plugin);
  const errors = [];
  const cache = {};
  const prov = (p) => (cache[p] ||= provided(dirOf(p)));
  const re = /\b([a-z][a-z0-9-]+):([a-z][a-z0-9-]+)\b/g;
  for (const { rel, text } of dfwTextFiles(pluginsRoot)) {
    for (const m of text.matchAll(re)) {
      const [, plugin, item] = m;
      if (!names.has(plugin)) continue;
      const p = prov(plugin);
      if (!p.skills.includes(item) && !p.agents.includes(item)) {
        errors.push(
          `${rel}: references ${plugin}:${item}, which ${plugin} does not provide (skills: ${p.skills.join(', ') || 'none'}; agents: ${p.agents.join(', ') || 'none'}).`,
        );
      }
    }
  }
  const xref = readJson(path.join(root, 'registry', 'xref.json'));
  for (const [plugin, files] of Object.entries(xref.requiredFiles)) {
    for (const f of files) {
      if (!fs.existsSync(path.join(dirOf(plugin), f))) errors.push(`registry/xref.json: ${plugin}/${f} is required by dfw but missing.`);
    }
  }
  // Reference-mode skills: every references/*.md named in its SKILL.md must exist.
  for (const s of registry.skills.filter((x) => x.mode === 'reference')) {
    const skillMd = path.join(dirOf(s.plugin), 'skills', s.plugin, 'SKILL.md');
    if (!fs.existsSync(skillMd)) continue;
    for (const m of fs.readFileSync(skillMd, 'utf8').matchAll(/`((?:references|scripts)\/[\w./-]+)`/g)) {
      if (!fs.existsSync(path.join(path.dirname(skillMd), m[1]))) errors.push(`${s.plugin}/SKILL.md names ${m[1]}, which is missing.`);
    }
  }
  return errors;
}

/** Vendored plugins must not ship hooks (plugin hooks.json or SKILL.md frontmatter hooks). */
export function hooksCheck({ pluginsRoot = path.join(REPO_ROOT, 'plugins'), root = REPO_ROOT, only } = {}) {
  const registry = readJson(path.join(root, 'registry', 'skills.json'));
  const errors = [];
  for (const s of registry.skills) {
    if (only && !only.includes(s.plugin)) continue;
    const dir = path.join(pluginsRoot, s.plugin);
    if (!fs.existsSync(dir)) continue;
    if (fs.existsSync(path.join(dir, 'hooks'))) errors.push(`${s.plugin}: ships a hooks/ directory (hooks must be stripped).`);
    const pj = path.join(dir, '.claude-plugin', 'plugin.json');
    if (fs.existsSync(pj) && readJson(pj).hooks) errors.push(`${s.plugin}: plugin.json declares hooks.`);
    for (const rel of listFiles(dir).filter((r) => r.endsWith('SKILL.md') || r.startsWith('agents/'))) {
      const text = fs.readFileSync(path.join(dir, rel), 'utf8');
      if (split(text).has && topLevelKeys(text).includes('hooks')) errors.push(`${s.plugin}/${rel}: frontmatter declares hooks.`);
    }
  }
  return errors;
}

/** Lint upstream instruction text for stack-imposing / risky instructions. Returns warnings. */
export function conflictLint({ pluginsRoot = path.join(REPO_ROOT, 'plugins'), root = REPO_ROOT, only } = {}) {
  const rules = readJson(path.join(root, 'registry', 'stack-lint.json')).rules;
  const registry = readJson(path.join(root, 'registry', 'skills.json'));
  const hits = [];
  for (const s of registry.skills) {
    if (only && !only.includes(s.plugin)) continue;
    const dir = path.join(pluginsRoot, s.plugin);
    for (const rel of listFiles(dir).filter((r) => r.endsWith('.md'))) {
      const lines = fs.readFileSync(path.join(dir, rel), 'utf8').split('\n');
      lines.forEach((line, i) => {
        for (const r of rules) {
          if (new RegExp(r.pattern, r.flags || 'i').test(line)) {
            hits.push({ rule: r.id, plugin: s.plugin, file: rel, line: i + 1, text: line.trim().slice(0, 200), note: r.note });
          }
        }
      });
    }
  }
  return hits;
}

export function executableFiles(dir) {
  return listFiles(dir).filter((r) => {
    if (/\.(sh|mjs|cjs|js|py|ts|cmd|ps1|rb)$/.test(r)) return true;
    try {
      const st = fs.statSync(path.join(dir, r));
      if (st.mode & 0o111) return true;
      const head = fs.readFileSync(path.join(dir, r)).subarray(0, 2).toString();
      return head === '#!';
    } catch {
      return false;
    }
  });
}

/**
 * Built plugins must not ship an unresolved `<skill-dir>` placeholder (pasted into a shell it is a
 * redirect, not a path), an upstream hook installer script (it writes into the host's settings), or
 * Impeccable text that tells the model how to turn its hooks on (`hooks on` writes host settings).
 */
const IMPECCABLE_HOOKS_ON = /\bhooks\s+on\b|scripts\/impeccable(\.cmd)?"?\s+hooks\b/;

export function leftoverCheck({ pluginsRoot = path.join(REPO_ROOT, 'plugins'), only } = {}) {
  const errors = [];
  if (!fs.existsSync(pluginsRoot)) return errors;
  for (const plugin of fs.readdirSync(pluginsRoot).sort()) {
    if (only && !only.includes(plugin)) continue;
    const dir = path.join(pluginsRoot, plugin);
    if (!fs.statSync(dir).isDirectory()) continue;
    for (const rel of listFiles(dir)) {
      if (/(^|\/)install-hooks?\.[cm]?js$/.test(rel))
        errors.push(`${plugin}/${rel}: upstream hook installer shipped (delete it in overlays/).`);
      if (!/\.(md|txt|json)$/.test(rel) || rel === 'UPSTREAM.json') continue; // UPSTREAM.json quotes the overlay's own "why" text
      const lines = fs.readFileSync(path.join(dir, rel), 'utf8').split('\n');
      const i = lines.findIndex((l) => l.includes('<skill-dir>'));
      if (i >= 0) errors.push(`${plugin}/${rel}:${i + 1}: unresolved <skill-dir> placeholder (add a replace op in overlays/).`);
      if (plugin !== 'impeccable') continue;
      // A line written by a dfw overlay names dfw and forbids the command; any other mention is upstream's how-to.
      const h = lines.findIndex((l) => IMPECCABLE_HOOKS_ON.test(l) && !/\bdfw\b/.test(l));
      if (h >= 0)
        errors.push(
          `${plugin}/${rel}:${h + 1}: instructions to enable Impeccable hooks shipped (replace them in overlays/impeccable.json).`,
        );
    }
  }
  return errors;
}

/**
 * One release version everywhere (contracts.md C14): the plugin manifests come from
 * core/manifest.json, the binary's from native/Cargo.toml, the npx installer's from package.json.
 * Returns the error text, or null when they agree.
 */
export function versionMismatch(root = REPO_ROOT) {
  const read = (rel) => fs.readFileSync(path.join(root, rel), 'utf8');
  const lock = JSON.parse(read('package-lock.json'));
  const cargoLock = /\[\[package\]\]\nname = "dfw"\nversion = "([^"]+)"/.exec(read('native/Cargo.lock'));
  const versions = {
    'core/manifest.json': JSON.parse(read('core/manifest.json')).version,
    'package.json': JSON.parse(read('package.json')).version,
    'package-lock.json': lock.version,
    'package-lock.json packages[""]': lock.packages?.['']?.version,
    'native/Cargo.toml': /^version = "([^"]+)"$/m.exec(read('native/Cargo.toml'))?.[1],
    'native/Cargo.lock (dfw)': cargoLock?.[1],
  };
  const distinct = new Set(Object.values(versions));
  return distinct.size === 1
    ? null
    : `versions differ: ${Object.entries(versions)
        .map(([f, v]) => `${f} ${v ?? '(none)'}`)
        .join(', ')}. Bump them together.`;
}

/** Component paths a manifest names (`skills`, `agents`, `hooks`), as a flat list. */
function componentPaths(manifest) {
  return ['skills', 'agents', 'hooks'].flatMap((k) => (manifest[k] === undefined ? [] : [manifest[k]].flat()));
}

/** Every hook command of a hooks file: Claude Code's `command`, Copilot's `bash` and `powershell`. */
function hookCommands(hooks) {
  const out = [];
  const walk = (v) => {
    if (Array.isArray(v)) v.forEach(walk);
    else if (v && typeof v === 'object')
      for (const [k, x] of Object.entries(v)) {
        if (['command', 'bash', 'powershell'].includes(k) && typeof x === 'string') out.push(x);
        else walk(x);
      }
  };
  walk(hooks.hooks);
  return out;
}

/**
 * The one dfw plugin folder (plugin-packaging spec): each host's skill, agents and hooks under
 * hosts/<host>/ named only by that host's manifest, nothing at a root location a host reads by
 * default, shared files once, no retired second folder, both marketplaces on ./plugins/dfw at the
 * core version. Returns error lines.
 */
export function layoutCheck({ pluginsRoot = path.join(REPO_ROOT, 'plugins'), root = REPO_ROOT } = {}) {
  const errors = [];
  const dir = path.join(pluginsRoot, 'dfw');
  const rel = (f) => `plugins/dfw/${f}`;
  if (!fs.existsSync(dir)) return ['plugins/dfw: not built (run npm run build)'];
  for (const old of RETIRED_PLUGINS)
    if (fs.existsSync(path.join(pluginsRoot, old)))
      errors.push(`plugins/${old}: retired (one plugins/dfw serves every host); delete it and run npm run build`);
  errors.push(...rootDefaults(dir, 'plugins/dfw'));
  const version = readJson(path.join(root, 'core', 'manifest.json')).version;
  const hosts = {
    claude: { manifest: '.claude-plugin/plugin.json', prefix: './hosts/claude/', agentExt: '.md' },
    copilot: { manifest: 'plugin.json', prefix: 'hosts/copilot/', agentExt: '.agent.md' },
  };
  for (const [id, h] of Object.entries(hosts)) {
    const file = path.join(dir, h.manifest);
    if (!fs.existsSync(file)) {
      errors.push(`${rel(h.manifest)}: missing (the ${id} manifest)`);
      continue;
    }
    const pj = readJson(file);
    if (pj.name !== 'dfw') errors.push(`${rel(h.manifest)}: name must be "dfw"`);
    if (pj.version !== version) errors.push(`${rel(h.manifest)}: version ${pj.version}, core/manifest.json says ${version}`);
    if (id === 'copilot' && pj.$schema) errors.push(`${rel(h.manifest)}: has a $schema (must stay a legacy manifest, not Agent Plugins)`);
    for (const p of componentPaths(pj)) {
      if (typeof p !== 'string' || !p.startsWith(h.prefix)) errors.push(`${rel(h.manifest)}: names ${p}, outside ${h.prefix}`);
      else if (!fs.existsSync(path.join(dir, p))) errors.push(`${rel(h.manifest)}: names ${p}, which does not exist`);
    }
    if (id === 'claude' && !Array.isArray(pj.agents))
      errors.push(`${rel(h.manifest)}: agents must list each .md file (Claude Code takes files, not a folder)`);
    const hostDir = path.join(dir, 'hosts', id);
    const files = listFiles(hostDir);
    const agents = files.filter((f) => f.startsWith('agents/'));
    if (!agents.length) errors.push(`${rel(`hosts/${id}/agents`)}: no agents`);
    for (const a of agents)
      if (!a.endsWith(h.agentExt) || (id === 'claude' && a.endsWith('.agent.md')))
        errors.push(`${rel(`hosts/${id}/${a}`)}: a ${id} agent file ends in ${h.agentExt}`);
    if (id === 'claude' && Array.isArray(pj.agents)) {
      const listed = new Set(pj.agents.map((a) => a.replace(h.prefix, '')));
      for (const a of agents) if (!listed.has(a)) errors.push(`${rel(h.manifest)}: does not list ${a} (Claude Code would not load it)`);
    }
    const hooksFile = path.join(hostDir, 'hooks.json');
    if (fs.existsSync(hooksFile)) {
      const hooks = readJson(hooksFile);
      const cmds = hookCommands(hooks);
      if (!cmds.length) errors.push(`${rel(`hosts/${id}/hooks.json`)}: no hook commands`);
      if (id === 'copilot') {
        if (hooks.version !== 1) errors.push(`${rel('hosts/copilot/hooks.json')}: version must be 1`);
        for (const c of cmds)
          if (!/--host copilot\b/.test(c))
            errors.push(`${rel('hosts/copilot/hooks.json')}: a command without --host copilot: ${c.slice(0, 80)}`);
      } else
        for (const c of cmds)
          if (/--host\b/.test(c)) errors.push(`${rel(`hosts/${id}/hooks.json`)}: a Claude Code hook with --host: ${c.slice(0, 80)}`);
    }
    for (const f of files.filter((x) => x.endsWith('.md'))) {
      const t = fs.readFileSync(path.join(hostDir, f), 'utf8');
      const left = t.match(/\{\{(SKILL_DIR|ASK_TOOL|BACKGROUND|HOST_NAME|HOST_SETTINGS|DFW_CLI|INVOKE_SKILL)\}\}/);
      if (left) errors.push(`${rel(`hosts/${id}/${f}`)}: unrendered ${left[0]}`);
      if (id !== 'claude' && /CLAUDE_SKILL_DIR|AskUserQuestion/.test(t))
        errors.push(`${rel(`hosts/${id}/${f}`)}: Claude Code-only wording in the ${id} text`);
      if (id === 'claude' && /\bask_user\b/.test(t)) errors.push(`${rel(`hosts/${id}/${f}`)}: Copilot-only wording in the claude text`);
    }
    // Shared files exist once: nothing under hosts/<id>/ repeats a root file.
    for (const f of files) {
      const twin = path.join(dir, f);
      if (fs.existsSync(twin) && fs.statSync(twin).isFile() && fs.readFileSync(twin).equals(fs.readFileSync(path.join(hostDir, f))))
        errors.push(`${rel(`hosts/${id}/${f}`)}: the same file is shared at ${rel(f)}; keep shared files once, at the root`);
    }
  }
  for (const file of [path.join('.claude-plugin', 'marketplace.json'), path.join('.github', 'plugin', 'marketplace.json')]) {
    const f = path.join(root, file);
    if (!fs.existsSync(f)) {
      errors.push(`${file}: missing`);
      continue;
    }
    const e = readJson(f).plugins.find((p) => p.name === 'dfw');
    if (!e) errors.push(`${file}: does not list dfw`);
    else {
      if (e.source !== './plugins/dfw') errors.push(`${file}: dfw source is ${e.source}, must be ./plugins/dfw`);
      if (e.version !== version) errors.push(`${file}: dfw version ${e.version}, core/manifest.json says ${version}`);
    }
  }
  return errors;
}

/** Local tooling (AGENTS.md, G6): never tracked in this repository. Vendored plugin folders may hold `.claude/`. */
export const LOCAL_TOOLING_FILES = ['AGENTS.md', 'CLAUDE.md'];
export const LOCAL_TOOLING_DIRS = ['openspec/', 'docs/', '.claude/', '.agents/'];

/** Tracked files that are local tooling: `git ls-files` must list none of them. */
export function trackedToolingCheck(root = REPO_ROOT) {
  const tracked = execFileSync('git', ['-C', root, 'ls-files', '-z'], { encoding: 'utf8' }).split('\0').filter(Boolean);
  return tracked
    .filter((f) => LOCAL_TOOLING_FILES.includes(f) || LOCAL_TOOLING_DIRS.some((d) => f.startsWith(d)))
    .map((f) => `${f}: local tooling is tracked; remove it with git rm --cached and keep it in .git/info/exclude (G6)`);
}

/** The CI workflows: every `run:` step is a bash script with `set -eo pipefail`. */
export const WORKFLOWS_DIR = path.join('.github', 'workflows');
/** A pipe into `grep -q` (or `--quiet`): grep exits at the first match and the writer's SIGPIPE fails the step. */
const PIPE_INTO_QUIET_GREP = /\|\s*grep\s+(?:-\w*q\w*|--quiet)\b/;
const YAML_COMMENT = /^\s*#/;

/**
 * Workflow lines that pipe into `grep -q`. Under pipefail the step fails whenever the writer is still
 * writing when grep exits, so it passes or fails by timing: the macOS release builds of v0.6.0 failed
 * the host check `rustc -vV | grep -q` this way and no release was made (#22).
 */
export function workflowPipeCheck(root = REPO_ROOT) {
  const dir = path.join(root, WORKFLOWS_DIR);
  if (!fs.existsSync(dir)) return [];
  const errors = [];
  for (const name of fs
    .readdirSync(dir)
    .filter((f) => /\.ya?ml$/.test(f))
    .sort()) {
    const rel = path.join(WORKFLOWS_DIR, name);
    fs.readFileSync(path.join(dir, name), 'utf8')
      .split('\n')
      .forEach((line, i) => {
        if (YAML_COMMENT.test(line) || !PIPE_INTO_QUIET_GREP.test(line)) return;
        errors.push(
          `${rel}:${i + 1}: a pipe into grep -q fails under pipefail when grep exits first; read the output whole (out=$(cmd)), then match it with case`,
        );
      });
  }
  return errors;
}
