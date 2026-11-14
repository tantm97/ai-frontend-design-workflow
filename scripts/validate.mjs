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

// 1. Marketplaces (one per host format) <-> plugins.
for (const file of [path.join('.claude-plugin', 'marketplace.json'), path.join('.github', 'plugin', 'marketplace.json')]) {
  const m = readJson(path.join(REPO_ROOT, file));
  for (const p of m.plugins) {
    const pj = [path.join(p.source, '.claude-plugin', 'plugin.json'), path.join(p.source, 'plugin.json')].map((f) => path.join(REPO_ROOT, f)).find((f) => fs.existsSync(f));
    if (!pj) errors.push(`${file} lists ${p.name} but ${p.source} has no plugin manifest`);
    else if (readJson(pj).name !== p.name) errors.push(`${p.source}: plugin manifest name differs from ${file} entry ${p.name}`);
  }
  const names = m.plugins.map((p) => p.name);
  if (new Set(names).size !== names.length) errors.push(`duplicate plugin names in ${file}`);
}
// Host plugins must not leak unrendered placeholders or another host's variables.
for (const l of registry.local || []) {
  const spec = readJson(path.join(REPO_ROOT, 'adapters', l.adapter, 'assemble.json'));
  const dir = path.join(REPO_ROOT, 'plugins', spec.plugin);
  for (const sub of ['skills', 'agents']) {
    const walk = (d) => (fs.existsSync(d) ? fs.readdirSync(d, { withFileTypes: true }).flatMap((e) => (e.isDirectory() ? walk(path.join(d, e.name)) : [path.join(d, e.name)])) : []);
    for (const f of walk(path.join(dir, sub)).filter((f) => f.endsWith('.md'))) {
      const t = fs.readFileSync(f, 'utf8');
      const left = t.match(/\{\{(SKILL_DIR|ASK_TOOL|BACKGROUND|HOST_NAME|HOST_SETTINGS|DFW_CLI|INVOKE_SKILL)\}\}/);
      if (left) errors.push(`${path.relative(REPO_ROOT, f)}: unrendered ${left[0]}`);
      if (spec.host !== 'claude' && /CLAUDE_SKILL_DIR|AskUserQuestion/.test(t)) errors.push(`${path.relative(REPO_ROOT, f)}: Claude Code-only wording in the ${spec.host} plugin`);
    }
  }
}

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
