// Build vendored plugins from registry/skills.json + registry/skills.lock.json + overlays/.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { fetchAt } from './git.mjs';
import { loadOverlay, applyOverlay } from './overlay.mjs';

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

/** Assemble an agent adapter's plugin (e.g. plugins/dfw) from core/ + adapters/<agent>/. */
export function assembleAdapter(agent = 'claude-code', { root = REPO_ROOT, outRoot = path.join(root, 'plugins') } = {}) {
  const spec = readJson(path.join(root, 'adapters', agent, 'assemble.json'));
  const dest = path.join(outRoot, spec.plugin);
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  for (const c of spec.copy) {
    const from = path.join(root, c.from);
    if (!fs.existsSync(from)) throw new Error(`assemble ${agent}: ${c.from} does not exist`);
    copyEntry(from, path.join(dest, c.to));
  }
  fs.writeFileSync(
    path.join(dest, 'GENERATED.md'),
    `# Generated directory\n\nThis plugin is assembled by \`npm run build\` from \`core/\` and \`adapters/${agent}/\` (see \`adapters/${agent}/assemble.json\`). Do not edit files here; edit the sources and rebuild.\n`,
  );
  return { plugin: spec.plugin, agent };
}

export function writeMarketplace(registry, lock, { root = REPO_ROOT } = {}) {
  const plugins = [];
  for (const l of registry.local || []) {
    const pj = readJson(path.join(root, 'plugins', l.plugin, '.claude-plugin', 'plugin.json'));
    plugins.push({ name: l.plugin, source: `./plugins/${l.plugin}`, description: l.description, version: pj.version, category: l.category });
  }
  for (const s of registry.skills) {
    const sha = lock.skills[s.id]?.sha;
    if (!sha) throw new Error(`No lock entry for ${s.id}`);
    plugins.push({ name: s.plugin, source: `./plugins/${s.plugin}`, description: s.description, version: `0.0.0-${sha.slice(0, 7)}`, category: 'design', license: s.license });
  }
  const mp = { name: registry.marketplace.name, owner: registry.marketplace.owner, metadata: { description: registry.marketplace.description }, plugins };
  writeJson(path.join(root, '.claude-plugin', 'marketplace.json'), mp);
  return mp;
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
  lines.push('', 'The `dfw` plugin, scripts, overlays and policies are original work under the MIT license (see LICENSE).', '');
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
