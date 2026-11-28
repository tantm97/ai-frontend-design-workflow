// Build vendored plugins from registry/skills.json + registry/skills.lock.json + overlays/.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { fetchAt } from './git.mjs';
import { loadOverlay, applyOverlay } from './overlay.mjs';
import { renderText, getHost } from './host.mjs';

export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const CACHE = path.join(REPO_ROOT, '.cache', 'upstream');
/** The notices file at the repo root, written by `npm run build` (writeNotices). */
export const NOTICES = 'THIRD_PARTY_NOTICES.md';

export function readJson(f) {
  return JSON.parse(fs.readFileSync(f, 'utf8'));
}
export function writeJson(f, data) {
  fs.mkdirSync(path.dirname(f), { recursive: true });
  fs.writeFileSync(f, JSON.stringify(data, null, 2) + '\n');
}
export function loadRegistry(root = REPO_ROOT) {
  return readJson(path.join(root, 'registry', 'skills.json'));
}
export function loadLock(root = REPO_ROOT) {
  return readJson(path.join(root, 'registry', 'skills.lock.json'));
}

// Runtime artifacts that tools create next to vendored scripts; never part of a build.
const IGNORED = new Set(['__pycache__', '.DS_Store', 'node_modules']);

function listFiles(dir, base = dir) {
  const out = [];
  if (!fs.existsSync(dir)) return out;
  for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
    if (IGNORED.has(e.name) || e.name.endsWith('.pyc')) continue;
    const abs = path.join(dir, e.name);
    if (e.isDirectory()) out.push(...listFiles(abs, base));
    else if (e.isFile()) out.push(path.relative(base, abs).split(path.sep).join('/'));
  }
  return out.sort();
}
export { listFiles };

function copyEntry(src, dest, exclude = []) {
  const st = fs.statSync(src);
  if (st.isFile()) {
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(src, dest);
    fs.chmodSync(dest, st.mode);
    return;
  }
  for (const rel of listFiles(src)) {
    if (exclude.some((x) => rel === x || rel.startsWith(`${x}/`))) continue;
    const to = path.join(dest, rel);
    fs.mkdirSync(path.dirname(to), { recursive: true });
    fs.copyFileSync(path.join(src, rel), to);
    fs.chmodSync(to, fs.statSync(path.join(src, rel)).mode);
  }
}

/** Build one vendored plugin into outRoot/<plugin>. Returns build metadata. */
export function buildPlugin(skill, sha, { outRoot = path.join(REPO_ROOT, 'plugins'), root = REPO_ROOT } = {}) {
  if (skill.plugin === 'dfw') throw new Error('Refusing to build over the local dfw plugin.');
  const src = fetchAt(skill.repo, sha, CACHE, [...skill.include.map((i) => i.from), ...(skill.licenseFiles || [])]);
  const dest = path.join(outRoot, skill.plugin);
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  for (const inc of skill.include) {
    const from = path.join(src, inc.from);
    if (!fs.existsSync(from))
      throw new Error(
        `${skill.id}: "${inc.from}" does not exist upstream at ${sha.slice(0, 7)}. Upstream layout changed; update registry/skills.json.`,
      );
    copyEntry(from, path.join(dest, inc.to), inc.exclude || []);
  }
  for (const lf of skill.licenseFiles || []) {
    const from = path.join(src, lf);
    if (!fs.existsSync(from)) throw new Error(`${skill.id}: license file "${lf}" missing upstream.`);
    fs.copyFileSync(from, path.join(dest, path.basename(lf)));
  }
  const overlayLog = applyOverlay(dest, loadOverlay(root, skill.id), root);
  const manifest = {
    name: skill.plugin,
    version: `0.0.0-${sha.slice(0, 7)}`,
    description: skill.description,
    author: { name: skill.author },
    license: skill.license,
    homepage: skill.homepage,
    repository: `https://github.com/${skill.repo}`,
  };
  if (skill.skillsPath) manifest.skills = skill.skillsPath;
  writeJson(path.join(dest, '.claude-plugin', 'plugin.json'), manifest);
  const upstream = {
    id: skill.id,
    repo: skill.repo,
    sha,
    track: skill.track,
    include: skill.include,
    overlay: overlayLog,
    mode: skill.mode || 'plugin',
  };
  writeJson(path.join(dest, 'UPSTREAM.json'), upstream);
  return upstream;
}

/** Copilot (and similar hosts) custom agents: <name>.agent.md with name + description frontmatter. */
function renderAgent(text, host) {
  const m = text.match(/^---\n([\s\S]*?)\n---\n?([\s\S]*)$/);
  if (!m || host.id === 'claude') return renderText(text, host.id);
  const keep = m[1].split('\n').filter((l) => /^(name|description):/.test(l));
  return `---\n${keep.join('\n')}\n---\n${renderText(m[2], host.id)}`;
}

function renderTree(dest, kind, host) {
  const files = fs.statSync(dest).isFile() ? [dest] : listFiles(dest).map((r) => path.join(dest, r));
  for (const f of files) {
    if (!f.endsWith('.md')) continue;
    const text = fs.readFileSync(f, 'utf8');
    if (kind === 'agent') {
      fs.writeFileSync(f, renderAgent(text, host));
      if (host.id !== 'claude' && !f.endsWith('.agent.md')) fs.renameSync(f, f.replace(/\.md$/, '.agent.md'));
    } else fs.writeFileSync(f, renderText(text, host.id));
  }
}

/**
 * Locations a host reads by default at a plugin root. The one dfw folder serves every host, so none
 * may exist there: Claude Code always loads a root skills/ (and a root SKILL.md, agents/,
 * hooks/hooks.json, .mcp.json), Copilot reads skills/, agents/, hooks.json and hooks/hooks.json
 * when its manifest names nothing, and .plugin/ or .github/plugin/plugin.json would be read as a
 * Copilot manifest before the root plugin.json. Each host's text lives under hosts/<host>/.
 */
export const ROOT_DEFAULTS = ['skills', 'agents', 'hooks', 'hooks.json', 'SKILL.md', '.mcp.json', '.plugin', '.github/plugin/plugin.json'];

/** Plugin folders earlier versions generated; the build deletes them and the check reports them. */
export const RETIRED_PLUGINS = ['dfw-copilot'];

/** Folders in plugins/ a clean build does not make (a retired plugins/dfw-copilot, a dropped plugin). */
export function strayPlugins(pluginsDir, built) {
  if (!fs.existsSync(pluginsDir)) return [];
  return fs
    .readdirSync(pluginsDir, { withFileTypes: true })
    .filter((e) => e.isDirectory() && !built.includes(e.name))
    .map((e) => `plugins/${e.name}: not part of a clean build (delete it, or run npm run build)`)
    .sort();
}

/** The default locations present at a plugin root, as error lines naming the path and the rule. */
export function rootDefaults(dir, label = path.basename(dir)) {
  return ROOT_DEFAULTS.filter((rel) => fs.existsSync(path.join(dir, rel))).map(
    (rel) =>
      `${label}/${rel}: a host reads this location by default; the dfw plugin keeps each host's skill, agents and hooks under hosts/<host>/ (adapters/core-copy.json).`,
  );
}

/** The adapters of the dfw plugin, one per host (registry/skills.json `local`), each read once. */
function dfwAdapters(root) {
  const seen = new Set();
  const out = [];
  for (const l of loadRegistry(root).local || []) {
    const adapter = l.adapter || 'claude-code';
    if (seen.has(adapter)) continue;
    seen.add(adapter);
    out.push({ adapter, spec: readJson(path.join(root, 'adapters', adapter, 'assemble.json')) });
  }
  return out;
}

/**
 * Assemble the one dfw plugin folder (plugins/dfw) for every host adapter: the shared core files
 * once at the root, then for each host its rendered skill and agents and its adapter files under
 * hosts/<host>/, and its manifest. Built in a staging folder that replaces the old one only when
 * no host default location appeared at the root.
 */
export function assembleDfw({ root = REPO_ROOT, outRoot = path.join(root, 'plugins') } = {}) {
  const adapters = dfwAdapters(root);
  const plugins = [...new Set(adapters.map((a) => a.spec.plugin))];
  if (plugins.length !== 1) throw new Error(`assemble dfw: every adapter must build the same plugin folder (got ${plugins.join(', ')})`);
  const plugin = plugins[0];
  const coreCopy = readJson(path.join(root, 'adapters', 'core-copy.json'));
  fs.mkdirSync(outRoot, { recursive: true });
  const stage = fs.mkdtempSync(path.join(outRoot, `.${plugin}-build-`));
  try {
    const copy = (c, base, host, label) => {
      const from = path.join(root, c.from);
      if (!fs.existsSync(from)) throw new Error(`assemble ${label}: ${c.from} does not exist`);
      copyEntry(from, path.join(base, c.to));
      if (c.render) renderTree(path.join(base, c.to), c.render, c.host ? getHost(c.host) : host);
    };
    for (const c of coreCopy.shared) copy(c, stage, null, 'shared');
    const core = readJson(path.join(root, 'core', 'manifest.json'));
    for (const { adapter, spec } of adapters) {
      const host = getHost(spec.host || 'claude');
      const hostDir = path.join(stage, 'hosts', host.id);
      for (const c of [...coreCopy.hostText, ...spec.copy]) copy(c, hostDir, host, adapter);
      // One manifest source (core/manifest.json) rendered in each host's format and location.
      const manifest = { ...core, name: spec.name || core.name, ...(spec.manifestExtra || {}) };
      if (spec.agentFiles) {
        // Claude Code: `agents` replaces the agents/ scan and takes .md files, one entry each.
        const dir = manifest.agents.replace(/^\.\//, '');
        manifest.agents = listFiles(path.join(stage, dir))
          .filter((f) => f.endsWith('.md'))
          .map((f) => `./${path.posix.join(dir, f)}`);
      }
      if (spec.manifest !== '.claude-plugin/plugin.json') delete manifest.dependencies; // kept in dfw.json
      writeJson(path.join(stage, spec.manifest), manifest);
    }
    writeJson(path.join(stage, 'dfw.json'), { version: core.version, dependencies: core.dependencies || [] });
    const names = adapters.map((a) => getHost(a.spec.host).name).join(' and ');
    fs.writeFileSync(
      path.join(stage, 'GENERATED.md'),
      `# Generated directory\n\nThis plugin folder serves ${names}. It is assembled by \`npm run build\` from \`core/\` (\`adapters/core-copy.json\`: shared files at the root) and \`adapters/<host>/\` (each host's skill, agents and hooks under \`hosts/<host>/\`, named only by that host's manifest). Do not edit files here; edit the sources and rebuild.\n`,
    );
    const refused = rootDefaults(stage, plugin);
    if (refused.length) throw new Error(`assemble dfw: refused, nothing written:\n${refused.join('\n')}`);
    const dest = path.join(outRoot, plugin);
    fs.rmSync(dest, { recursive: true, force: true });
    fs.renameSync(stage, dest);
  } finally {
    fs.rmSync(stage, { recursive: true, force: true });
  }
  for (const old of RETIRED_PLUGINS) fs.rmSync(path.join(outRoot, old), { recursive: true, force: true });
  return { plugin, hosts: adapters.map((a) => a.spec.host) };
}

/**
 * One marketplace per host format, same name ("dfw-marketplace") and same vendored skills, both
 * listing dfw from the one folder ./plugins/dfw at the core version:
 * - .claude-plugin/marketplace.json (Claude Code)
 * - .github/plugin/marketplace.json (GitHub Copilot, read before the Claude Code file)
 */
export function writeMarketplace(registry, lock, { root = REPO_ROOT } = {}) {
  const skills = registry.skills.map((s) => {
    const sha = lock.skills[s.id]?.sha;
    if (!sha) throw new Error(`No lock entry for ${s.id}`);
    return {
      name: s.plugin,
      source: `./plugins/${s.plugin}`,
      description: s.description,
      version: `0.0.0-${sha.slice(0, 7)}`,
      category: 'design',
      license: s.license,
    };
  });
  const core = readJson(path.join(root, 'core', 'manifest.json'));
  const out = {};
  for (const l of registry.local || []) {
    const spec = readJson(path.join(root, 'adapters', l.adapter, 'assemble.json'));
    const entry = {
      name: spec.name || core.name,
      source: `./plugins/${spec.plugin}`,
      description: l.description,
      version: core.version,
      category: l.category,
    };
    const file = l.marketplace || path.join('.claude-plugin', 'marketplace.json');
    const mp = {
      name: registry.marketplace.name,
      owner: registry.marketplace.owner,
      metadata: { description: registry.marketplace.description },
      plugins: [entry, ...skills],
    };
    writeJson(path.join(root, file), mp);
    out[file] = mp;
  }
  return out[path.join('.claude-plugin', 'marketplace.json')] || Object.values(out)[0];
}

/** The Rust crates linked into the binary, from native/crate-licenses.json (scripts/crate-licenses.mjs). */
function crateNotices(root) {
  const file = path.join(root, 'native', 'crate-licenses.json');
  if (!fs.existsSync(file)) return [];
  const { crates, releaseTargets } = readJson(file);
  const linkedOn = (c) =>
    c.linked.length === releaseTargets.length
      ? 'all release targets'
      : c.linked.length
        ? c.linked.join(', ')
        : 'no release target (a test-only, build-time or other-platform crate)';
  return [
    '## Rust crates in the dfw binary',
    '',
    `Every crate of \`native/Cargo.lock\` (${crates.length}), with the licence its own manifest declares (\`cargo metadata\` for each release target: ${releaseTargets.join(', ')}). Direct dependencies are marked. Regenerate with \`node scripts/crate-licenses.mjs\`.`,
    '',
    '| Crate | Version | License | Direct | Linked on |',
    '|---|---|---|---|---|',
    ...crates.map((c) => `| ${c.name} | ${c.version} | ${c.license} | ${c.direct ? 'yes' : ''} | ${linkedOn(c)} |`),
    '',
  ];
}

/** The text of THIRD_PARTY_NOTICES.md, from the registry, the lock and native/crate-licenses.json. */
export function noticesText(registry, lock, { root = REPO_ROOT } = {}) {
  const lines = [
    '# Third-party notices',
    '',
    'This repository vendors unmodified copies of the following projects (adapted only by the declarative overlays in `overlays/`, applied at build time). Each plugin directory contains the upstream license file(s) and an `UPSTREAM.json` with the exact commit.',
    '',
    '| Plugin | Upstream | Commit | License | Author |',
    '|---|---|---|---|---|',
  ];
  for (const s of registry.skills) {
    const sha = lock.skills[s.id].sha;
    lines.push(`| ${s.plugin} | https://github.com/${s.repo} | \`${sha.slice(0, 12)}\` | ${s.license} | ${s.author} |`);
  }
  lines.push(
    '',
    'The `dfw` plugin, the `dfw` binary (the Rust crate in `native/`), scripts, overlays and policies are original work under the MIT license (see LICENSE).',
    '',
    '## Code and data in the dfw binary',
    '',
    '| Item | Upstream | License | How it is used |',
    '|---|---|---|---|',
    '| pixelmatch 8.0.0 | https://github.com/mapbox/pixelmatch (Copyright (c) 2025, Mapbox) | ISC | Ported to Rust for `dfw diff` (`native/src/browser/pixeldiff.rs`); the original is kept in `core/assets/vendor/pixelmatch.js` with its licence |',
    '| Lucide icons | https://github.com/lucide-icons/lucide (Copyright (c) Lucide Icons and Contributors) | ISC | The icon set of `dfw icons`, vendored at tag 1.64.0 in `core/assets/icons/` (licence in `LICENSE-lucide`) and embedded in the binary |',
    '',
    "Downloaded on the user's machine, not shipped: axe-core 4.14.0 (https://github.com/dequelabs/axe-core, MPL-2.0), fetched by `dfw setup --a11y` (or `dfw install`) from the npm registry with its integrity hash checked and injected unmodified into rendered pages by `dfw check`; Google's chrome-headless-shell (Chrome for Testing, BSD-3-Clause and its own notices), fetched only by `dfw setup --browser`, or by `dfw install` when no Chromium-based browser is found (`--no-runtime` skips it). dfw otherwise drives an already-installed Chromium-based browser.",
    '',
    ...crateNotices(root),
  );
  return lines.join('\n');
}

export function writeNotices(registry, lock, { root = REPO_ROOT } = {}) {
  fs.writeFileSync(path.join(root, NOTICES), noticesText(registry, lock, { root }));
}

export function buildAll({ only, root = REPO_ROOT, outRoot } = {}) {
  const registry = loadRegistry(root);
  const lock = loadLock(root);
  const results = [];
  if (!only) assembleDfw({ root, outRoot: outRoot || path.join(root, 'plugins') });
  for (const s of registry.skills) {
    if (only && !only.includes(s.id)) continue;
    const sha = lock.skills[s.id]?.sha;
    if (!sha) throw new Error(`registry has "${s.id}" but the lock has no entry for it. Run: node scripts/skills.mjs propose ${s.id}`);
    results.push(buildPlugin(s, sha, { outRoot: outRoot || path.join(root, 'plugins'), root }));
  }
  if (!outRoot) {
    writeMarketplace(registry, lock, { root });
    writeNotices(registry, lock, { root });
  }
  return results;
}
