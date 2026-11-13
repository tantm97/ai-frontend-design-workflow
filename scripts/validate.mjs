#!/usr/bin/env node
// Validate the marketplace: structure, lock/build consistency, cross-references,
// no upstream hooks, conflict-lint warnings, and `claude plugin validate` when available.
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { REPO_ROOT, readJson, loadRegistry, loadLock } from './lib/build.mjs';
import { xrefCheck, hooksCheck, conflictLint } from './lib/checks.mjs';

const errors = [];
const warnings = [];
const registry = loadRegistry();
const lock = loadLock();
const mp = readJson(path.join(REPO_ROOT, '.claude-plugin', 'marketplace.json'));

// 1. Marketplace <-> plugins.
for (const p of mp.plugins) {
  const pj = path.join(REPO_ROOT, p.source, '.claude-plugin', 'plugin.json');
  if (!fs.existsSync(pj)) errors.push(`marketplace lists ${p.name} but ${p.source}/.claude-plugin/plugin.json is missing`);
  else if (readJson(pj).name !== p.name) errors.push(`${p.source}: plugin.json name differs from marketplace entry ${p.name}`);
}
const names = mp.plugins.map((p) => p.name);
if (new Set(names).size !== names.length) errors.push('duplicate plugin names in marketplace.json');

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
  else if (readJson(up).sha !== entry.sha) errors.push(`${s.plugin}: built from ${readJson(up).sha.slice(0, 7)} but lock says ${entry.sha.slice(0, 7)} (run npm run build)`);
}
for (const id of Object.keys(lock.skills)) if (!registry.skills.some((s) => s.id === id)) warnings.push(`lock has "${id}" which is not in the registry`);

// 3. dfw's references to other plugins and files.
errors.push(...xrefCheck());
// 4. Hooks invariant.
errors.push(...hooksCheck());
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
    if (/\ndisable-model-invocation:\s*true/.test(text)) warnings.push(`${p.name}/${d}: disable-model-invocation is set; dfw cannot delegate to it`);
    const kb = Buffer.byteLength(text) / 1024;
    if (kb > 48) warnings.push(`${p.name}/${d}/SKILL.md is ${kb.toFixed(0)} KB (~${Math.round((kb * 1024) / 4 / 1000)}k tokens) when loaded`);
  }
}
// 6. Conflict lint (warnings).
const lint = conflictLint();
const byRule = {};
for (const h of lint) (byRule[h.rule] ||= []).push(h);
for (const [rule, list] of Object.entries(byRule)) warnings.push(`conflict-lint ${rule}: ${list.length} line(s), e.g. ${list.slice(0, 2).map((h) => `${h.plugin}/${h.file}:${h.line}`).join(', ')} (${list[0].note})`);

// 7. Claude Code's own validator when installed.
const claude = spawnSync('claude', ['plugin', 'validate', REPO_ROOT], { encoding: 'utf8' });
if (claude.error) warnings.push('claude CLI not found: skipped `claude plugin validate`');
else if (claude.status !== 0) errors.push(`claude plugin validate failed:\n${(claude.stdout + claude.stderr).trim().slice(0, 2000)}`);

for (const w of warnings) console.log(`warn  ${w}`);
for (const e of errors) console.log(`ERROR ${e}`);
console.log(`\nvalidate: ${errors.length} error(s), ${warnings.length} warning(s)`);
process.exit(errors.length ? 1 : 0);
