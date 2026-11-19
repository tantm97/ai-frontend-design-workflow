// Build vendored plugins from registry/skills.json + registry/skills.lock.json + overlays/.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { fetchAt } from './git.mjs';
import { loadOverlay, applyOverlay } from './overlay.mjs';
import { renderText, getHost } from './host.mjs';

export const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
export const CACHE = path.join(REPO_ROOT, '.cache', 'upstream');

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
    if (!fs.existsSync(from)) throw new Error(`${skill.id}: "${inc.from}" does not exist upstream at ${sha.slice(0, 7)}. Upstream layout changed; update registry/skills.json.`);
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
  const upstream = { id: skill.id, repo: skill.repo, sha, track: skill.track, include: skill.include, overlay: overlayLog, mode: skill.mode || 'plugin' };
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

/** Assemble a host adapter's plugin (e.g. plugins/dfw, plugins/dfw-copilot) from core/ + adapters/<adapter>/. */
export function assembleAdapter(agent = 'claude-code', { root = REPO_ROOT, outRoot = path.join(root, 'plugins') } = {}) {
  const spec = readJson(path.join(root, 'adapters', agent, 'assemble.json'));
  const host = getHost(spec.host || 'claude');
  const dest = path.join(outRoot, spec.plugin);
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  const coreCopy = readJson(path.join(root, 'adapters', 'core-copy.json')).copy;
  for (const c of [...coreCopy, ...spec.copy]) {
    const from = path.join(root, c.from);
    if (!fs.existsSync(from)) throw new Error(`assemble ${agent}: ${c.from} does not exist`);
    copyEntry(from, path.join(dest, c.to));
    if (c.render) renderTree(path.join(dest, c.to), c.render, c.host ? getHost(c.host) : host);
  }
  // One manifest source (core/manifest.json) rendered in each host's format and location.
  const core = readJson(path.join(root, 'core', 'manifest.json'));
  const manifest = { ...core, name: spec.name || core.name, ...(spec.manifestExtra || {}) };
  if (spec.manifest !== '.claude-plugin/plugin.json') delete manifest.dependencies; // kept in dfw.json
  writeJson(path.join(dest, spec.manifest), manifest);
  writeJson(path.join(dest, 'dfw.json'), { host: host.id, version: core.version, dependencies: core.dependencies || [] });
  fs.writeFileSync(
    path.join(dest, 'GENERATED.md'),
    `# Generated directory\n\nThis ${host.name} plugin is assembled by \`npm run build\` from \`core/\` and \`adapters/${agent}/\` (see \`adapters/${agent}/assemble.json\`). Do not edit files here; edit the sources and rebuild.\n`,
  );
  return { plugin: spec.plugin, agent, host: host.id, manifest: spec.manifest };
}

/**
 * One marketplace per host format, same name ("dfw-marketplace") and same vendored skills:
 * - .claude-plugin/marketplace.json (Claude Code): dfw -> ./plugins/dfw
 * - .github/plugin/marketplace.json (GitHub Copilot): dfw -> ./plugins/dfw-copilot
 */
export function writeMarketplace(registry, lock, { root = REPO_ROOT } = {}) {
  const skills = registry.skills.map((s) => {
    const sha = lock.skills[s.id]?.sha;
    if (!sha) throw new Error(`No lock entry for ${s.id}`);
    return { name: s.plugin, source: `./plugins/${s.plugin}`, description: s.description, version: `0.0.0-${sha.slice(0, 7)}`, category: 'design', license: s.license };
  });
  const out = {};
  for (const l of registry.local || []) {
    const spec = readJson(path.join(root, 'adapters', l.adapter, 'assemble.json'));
    const pj = readJson(path.join(root, 'plugins', spec.plugin, spec.manifest));
    const entry = { name: spec.name || spec.plugin, source: `./plugins/${spec.plugin}`, description: l.description, version: pj.version, category: l.category };
    const file = l.marketplace || path.join('.claude-plugin', 'marketplace.json');
    const mp = { name: registry.marketplace.name, owner: registry.marketplace.owner, metadata: { description: registry.marketplace.description }, plugins: [entry, ...skills] };
    writeJson(path.join(root, file), mp);
    out[file] = mp;
  }
  return out[path.join('.claude-plugin', 'marketplace.json')] || Object.values(out)[0];
}

/** The Rust crates linked into the binary, from native/crate-licenses.json (scripts/crate-licenses.mjs). */
function crateNotices(root) {
  const file = path.join(root, 'native', 'crate-licenses.json');
  if (!fs.existsSync(file)) return [];
  const { crates, unresolvedTargets } = readJson(file);
  const linkedOn = (c) => (Array.isArray(c.linked) ? (c.linked.length === 5 ? 'all release targets' : c.linked.join(', ')) : `not on the resolved targets (${unresolvedTargets.length ? `${unresolvedTargets.join(', ')} not resolved offline; ` : ''}a test-only, build-time, Windows-only or other-platform crate)`);
  return [
    '## Rust crates in the dfw binary',
    '',
    `Every crate of \`native/Cargo.lock\` (${crates.length}), with the licence its own manifest declares (\`cargo metadata\`, or the crate's Cargo.toml in the local cargo registry). Direct dependencies are marked. A licence that could not be read is listed as "not recorded"; it is never guessed. Regenerate with \`node scripts/crate-licenses.mjs\`.`,
    '',
    '| Crate | Version | License | Direct | Linked on |',
    '|---|---|---|---|---|',
    ...crates.map((c) => `| ${c.name} | ${c.version} | ${c.license} | ${c.direct ? 'yes' : ''} | ${linkedOn(c)} |`),
    '',
  ];
}

export function writeNotices(registry, lock, { root = REPO_ROOT } = {}) {
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
    'Downloaded on the user\'s machine, not shipped: axe-core 4.14.0 (https://github.com/dequelabs/axe-core, MPL-2.0), fetched by `dfw setup --a11y` (or `dfw install`) from the npm registry with its integrity hash checked and injected unmodified into rendered pages by `dfw check`; Google\'s chrome-headless-shell (Chrome for Testing, BSD-3-Clause and its own notices), fetched only by `dfw setup --browser`, or by `dfw install` when no Chromium-based browser is found (`--no-runtime` skips it). dfw otherwise drives an already-installed Chromium-based browser.',
    '',
    ...crateNotices(root),
  );
  fs.writeFileSync(path.join(root, 'THIRD_PARTY_NOTICES.md'), lines.join('\n'));
}

export function buildAll({ only, root = REPO_ROOT, outRoot } = {}) {
  const registry = loadRegistry(root);
  const lock = loadLock(root);
  const results = [];
  if (!only) for (const l of registry.local || []) assembleAdapter(l.adapter || 'claude-code', { root, outRoot: outRoot || path.join(root, 'plugins') });
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
