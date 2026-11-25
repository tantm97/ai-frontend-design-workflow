#!/usr/bin/env node
// Validate the marketplace: structure, lock/build consistency, cross-references,
// no upstream hooks, conflict-lint warnings, and `claude plugin validate` when available.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { REPO_ROOT, readJson, loadRegistry, loadLock } from './lib/build.mjs';
import { xrefCheck, hooksCheck, leftoverCheck, layoutCheck, trackedToolingCheck, conflictLint, versionMismatch } from './lib/checks.mjs';
import { createHash } from 'node:crypto';
import { unwrap } from './knowledge.mjs';
import { lockPackages } from './crate-licenses.mjs';

const errors = [];
const warnings = [];
const registry = loadRegistry();
const lock = loadLock();
const mp = readJson(path.join(REPO_ROOT, '.claude-plugin', 'marketplace.json'));

// 1. Marketplaces (one per host format) <-> plugins.
for (const file of [path.join('.claude-plugin', 'marketplace.json'), path.join('.github', 'plugin', 'marketplace.json')]) {
  const m = readJson(path.join(REPO_ROOT, file));
  for (const p of m.plugins) {
    const pj = [path.join(p.source, '.claude-plugin', 'plugin.json'), path.join(p.source, 'plugin.json')]
      .map((f) => path.join(REPO_ROOT, f))
      .find((f) => fs.existsSync(f));
    if (!pj) errors.push(`${file} lists ${p.name} but ${p.source} has no plugin manifest`);
    else if (readJson(pj).name !== p.name) errors.push(`${p.source}: plugin manifest name differs from ${file} entry ${p.name}`);
  }
  const names = m.plugins.map((p) => p.name);
  if (new Set(names).size !== names.length) errors.push(`duplicate plugin names in ${file}`);
}
// The one dfw plugin folder for every host: layout, manifests, host text, marketplace entries.
errors.push(...layoutCheck());
// Local tooling (AGENTS.md, openspec/, docs/, .claude/) is never tracked.
errors.push(...trackedToolingCheck());

// 2. Lock <-> built copies.
for (const s of registry.skills) {
  const entry = lock.skills[s.id];
  if (!entry) {
    errors.push(`${s.id}: no lock entry`);
    continue;
  }
  if (!/^[0-9a-f]{40}$/.test(entry.sha)) errors.push(`${s.id}: lock sha must be a full commit sha`);
  const up = path.join(REPO_ROOT, 'plugins', s.plugin, 'UPSTREAM.json');
  if (!fs.existsSync(up)) errors.push(`${s.plugin}: not built (run npm run build)`);
  else if (readJson(up).sha !== entry.sha)
    errors.push(`${s.plugin}: built from ${readJson(up).sha.slice(0, 7)} but lock says ${entry.sha.slice(0, 7)} (run npm run build)`);
}
for (const id of Object.keys(lock.skills))
  if (!registry.skills.some((s) => s.id === id)) warnings.push(`lock has "${id}" which is not in the registry`);

// 3. dfw's references to other plugins and files.
// Vendored knowledge files: pinned, unmodified upstream text plus its license.
for (const k of registry.knowledge || []) {
  const f = path.join(REPO_ROOT, k.dest);
  if (!/^[0-9a-f]{40}$/.test(k.commit || '')) errors.push(`knowledge ${k.id}: commit must be a full sha`);
  if (!fs.existsSync(f)) {
    errors.push(`knowledge ${k.id}: ${k.dest} missing (npm run knowledge -- sync)`);
    continue;
  }
  const text = fs.readFileSync(f, 'utf8');
  const body = unwrap(text);
  if (body == null || createHash('sha256').update(body).digest('hex') !== k.sha256)
    errors.push(`knowledge ${k.id}: ${k.dest} differs from the pinned upstream text (do not edit it; npm run knowledge -- sync)`);
  if (!/## License/.test(text)) errors.push(`knowledge ${k.id}: license section missing`);
}

// 3b. One release version everywhere (contracts.md C14).
const versions = versionMismatch();
if (versions) errors.push(versions);
// The crate list of THIRD_PARTY_NOTICES.md covers exactly native/Cargo.lock.
{
  const lock = lockPackages(fs.readFileSync(path.join(REPO_ROOT, 'native', 'Cargo.lock'), 'utf8'))
    .map((p) => `${p.name} ${p.version}`)
    .sort();
  const listed = readJson(path.join(REPO_ROOT, 'native', 'crate-licenses.json'))
    .crates.map((c) => `${c.name} ${c.version}`)
    .sort();
  if (JSON.stringify(lock) !== JSON.stringify(listed))
    errors.push(
      'native/crate-licenses.json does not list exactly the crates of native/Cargo.lock: run node scripts/crate-licenses.mjs, then npm run build',
    );
}

errors.push(...xrefCheck());
// 4. Hooks invariant.
errors.push(...hooksCheck());
errors.push(...leftoverCheck());
// 5. Skill frontmatter sanity.
for (const p of mp.plugins) {
  const dir = path.join(REPO_ROOT, p.source);
  const pj = readJson(path.join(dir, '.claude-plugin', 'plugin.json'));
  const skillsRoot = path.join(dir, pj.skills || 'skills');
  if (!fs.existsSync(skillsRoot)) continue;
  for (const d of fs.readdirSync(skillsRoot)) {
    const f = path.join(skillsRoot, d, 'SKILL.md');
    if (!fs.existsSync(f)) continue;
    const text = fs.readFileSync(f, 'utf8');
    if (!/^---\r?\n(?:[\s\S]*?\n)?name:\s*\S+/.test(text)) errors.push(`${p.name}/${d}/SKILL.md: missing name`);
    if (!/\ndescription:\s*\S/.test(text)) errors.push(`${p.name}/${d}/SKILL.md: missing description`);
    if (/\ndisable-model-invocation:\s*true/.test(text))
      warnings.push(`${p.name}/${d}: disable-model-invocation is set; dfw cannot delegate to it`);
    const kb = Buffer.byteLength(text) / 1024;
    if (kb > 48)
      warnings.push(`${p.name}/${d}/SKILL.md is ${kb.toFixed(0)} KB (~${Math.round((kb * 1024) / 4 / 1000)}k tokens) when loaded`);
  }
}
// 6. Conflict lint (warnings).
const lint = conflictLint();
const byRule = {};
for (const h of lint) (byRule[h.rule] ||= []).push(h);
for (const [rule, list] of Object.entries(byRule))
  warnings.push(
    `conflict-lint ${rule}: ${list.length} line(s), e.g. ${list
      .slice(0, 2)
      .map((h) => `${h.plugin}/${h.file}:${h.line}`)
      .join(', ')} (${list[0].note})`,
  );

// 7. Claude Code's own validator when installed.
const claude = spawnSync('claude', ['plugin', 'validate', REPO_ROOT], { encoding: 'utf8' });
if (claude.error) warnings.push('claude CLI not found: skipped `claude plugin validate`');
else if (claude.status !== 0) errors.push(`claude plugin validate failed:\n${(claude.stdout + claude.stderr).trim().slice(0, 2000)}`);

for (const w of warnings) console.log(`warn  ${w}`);
for (const e of errors) console.log(`ERROR ${e}`);
console.log(`\nvalidate: ${errors.length} error(s), ${warnings.length} warning(s)`);
process.exit(errors.length ? 1 : 0);
