// Compatibility checks for built plugins: cross-references used by dfw,
// "no upstream hooks" invariant, and conflict lint of upstream instructions.
import fs from 'node:fs';
import path from 'node:path';
import { REPO_ROOT, readJson, listFiles } from './build.mjs';
import { split, topLevelKeys } from './frontmatter.mjs';

/** Skills and agents a plugin directory provides. */
export function provided(pluginDir) {
  const pj = fs.existsSync(path.join(pluginDir, '.claude-plugin', 'plugin.json')) ? readJson(path.join(pluginDir, '.claude-plugin', 'plugin.json')) : {};
  const skillsRoot = path.join(pluginDir, pj.skills || 'skills');
  const skills = fs.existsSync(skillsRoot)
    ? fs.readdirSync(skillsRoot).filter((d) => fs.existsSync(path.join(skillsRoot, d, 'SKILL.md')))
    : [];
  const agentsDir = path.join(pluginDir, 'agents');
  const agents = fs.existsSync(agentsDir)
    ? fs.readdirSync(agentsDir).filter((f) => f.endsWith('.md')).map((f) => {
        const m = fs.readFileSync(path.join(agentsDir, f), 'utf8').match(/^name:\s*(\S+)/m);
        return m ? m[1] : f.replace(/\.md$/, '');
      })
    : [];
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
        errors.push(`${rel}: references ${plugin}:${item}, which ${plugin} does not provide (skills: ${p.skills.join(', ') || 'none'}; agents: ${p.agents.join(', ') || 'none'}).`);
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
