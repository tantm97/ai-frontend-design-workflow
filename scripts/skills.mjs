#!/usr/bin/env node
// External skill version management.
//   status                      pinned commits
//   check [--json]              compare pins with upstream (no changes)
//   propose <id> [--to <sha>]   build the candidate in isolation and write a compatibility/security report
//   apply <id> [--to <sha>] [--force]   apply a reviewed, non-blocking proposal (updates the lock, rebuilds, validates)
//   rollback <id> [--to <sha>]  restore the previous (or given) pin and rebuild
//   history [id]                update history
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { REPO_ROOT, readJson, writeJson, loadRegistry, loadLock, buildPlugin, buildAll, listFiles } from './lib/build.mjs';
import { resolveRef, commitCount } from './lib/git.mjs';
import { xrefCheck, hooksCheck, conflictLint, executableFiles } from './lib/checks.mjs';
import { getKey } from './lib/frontmatter.mjs';

const LOCK = path.join(REPO_ROOT, 'registry', 'skills.lock.json');
const HISTORY = path.join(REPO_ROOT, 'registry', 'history.jsonl');
const REPORTS = path.join(REPO_ROOT, 'reports', 'proposals');

const args = process.argv.slice(2);
const cmd = args[0];
const opt = (name) => (args.includes(`--${name}`) ? args[args.indexOf(`--${name}`) + 1] : undefined);
const flag = (name) => args.includes(`--${name}`);
const today = () => new Date().toISOString().slice(0, 10);

function skillById(id) {
  const s = loadRegistry().skills.find((x) => x.id === id);
  if (!s) throw new Error(`Unknown skill "${id}". Known: ${loadRegistry().skills.map((x) => x.id).join(', ')}`);
  return s;
}

function history() {
  if (!fs.existsSync(HISTORY)) return [];
  return fs.readFileSync(HISTORY, 'utf8').split('\n').filter(Boolean).map((l) => JSON.parse(l));
}

function appendHistory(entry) {
  fs.appendFileSync(HISTORY, JSON.stringify({ at: new Date().toISOString(), ...entry }) + '\n');
}

function setLock(id, sha) {
  const lock = loadLock();
  const s = skillById(id);
  lock.skills[id] = { repo: s.repo, sha, lockedAt: today() };
  writeJson(LOCK, lock);
}

function descriptions(dir) {
  const out = {};
  for (const rel of listFiles(dir).filter((r) => r.endsWith('SKILL.md'))) {
    out[rel] = getKey(fs.readFileSync(path.join(dir, rel), 'utf8'), 'description') || '';
  }
  return out;
}

function cmdStatus() {
  const lock = loadLock();
  const rows = loadRegistry().skills.map((s) => `${s.id.padEnd(17)} ${s.repo.padEnd(38)} ${lock.skills[s.id]?.sha.slice(0, 12) || 'UNLOCKED'}  ${lock.skills[s.id]?.lockedAt || ''}${s.mode === 'reference' ? '  (reference mode)' : ''}`);
  console.log(rows.join('\n'));
}

function cmdCheck() {
  const lock = loadLock();
  const out = [];
  for (const s of loadRegistry().skills) {
    const pinned = lock.skills[s.id]?.sha;
    let latest;
    try {
      latest = resolveRef(s.repo, s.track || 'HEAD');
    } catch (e) {
      out.push({ id: s.id, error: e.message });
      continue;
    }
    const behind = pinned === latest ? 0 : commitCount(s.repo, pinned, latest);
    out.push({ id: s.id, repo: s.repo, track: s.track, pinned, latest, upToDate: pinned === latest, commits: behind });
  }
  if (flag('json')) console.log(JSON.stringify(out, null, 2));
  else {
    for (const r of out) {
      if (r.error) console.log(`${r.id}: ERROR ${r.error}`);
      else console.log(`${r.id.padEnd(17)} ${r.upToDate ? 'up to date' : `update available ${r.pinned.slice(0, 7)} -> ${r.latest.slice(0, 7)}${r.commits != null ? ` (${r.commits} commits)` : ''}`}`);
    }
    const n = out.filter((r) => r.upToDate === false).length;
    console.log(n ? `\n${n} update(s) available. Next: node scripts/skills.mjs propose <id>` : '\nAll pinned skills are current.');
  }
  return out;
}

function cmdPropose(id) {
  const s = skillById(id);
  const lock = loadLock();
  const from = lock.skills[id]?.sha;
  const to = opt('to') || resolveRef(s.repo, s.track || 'HEAD');
  const report = { id, plugin: s.plugin, repo: s.repo, from, to, createdAt: new Date().toISOString(), blocking: [], warnings: [] };
  if (from === to) {
    console.log(`${id} is already pinned to ${to.slice(0, 7)}.`);
    return;
  }
  report.commits = from ? commitCount(s.repo, from, to) : null;
  const tmp = fs.mkdtempSync(path.join(os.tmpdir(), `dfw-propose-${id}-`));
  const current = path.join(REPO_ROOT, 'plugins', s.plugin);
  const cand = path.join(tmp, s.plugin);
  try {
    buildPlugin(s, to, { outRoot: tmp });
  } catch (e) {
    report.blocking.push(`Build/overlay failed: ${e.message}`);
  }
  if (fs.existsSync(cand)) {
    // File-level diff (ignoring generated metadata).
    const ignore = new Set(['UPSTREAM.json', '.claude-plugin/plugin.json']);
    const a = new Set(listFiles(current).filter((f) => !ignore.has(f)));
    const b = new Set(listFiles(cand).filter((f) => !ignore.has(f)));
    report.added = [...b].filter((f) => !a.has(f));
    report.removed = [...a].filter((f) => !b.has(f));
    report.changed = [...a].filter((f) => b.has(f) && !fs.readFileSync(path.join(current, f)).equals(fs.readFileSync(path.join(cand, f))));
    // Trigger scope: description changes.
    const da = descriptions(current);
    const db = descriptions(cand);
    report.descriptionChanges = Object.keys({ ...da, ...db })
      .filter((k) => da[k] !== db[k])
      .map((k) => ({ file: k, before: da[k] ?? null, after: db[k] ?? null }));
    // Security-relevant: executable code.
    const ea = new Set(executableFiles(current));
    const eb = executableFiles(cand);
    report.executables = {
      added: eb.filter((f) => !ea.has(f)),
      changed: eb.filter((f) => ea.has(f) && report.changed.includes(f)),
      removed: [...ea].filter((f) => !eb.includes(f)),
    };
    if (report.executables.added.length || report.executables.changed.length) {
      report.warnings.push(`Executable code added/changed (${report.executables.added.length + report.executables.changed.length} file(s)). Review before applying.`);
    }
    // Hooks invariant.
    const hooks = hooksCheck({ pluginsRoot: tmp, only: [s.plugin] });
    report.blocking.push(...hooks);
    // Cross references used by dfw, evaluated against the candidate.
    const xref = xrefCheck({ overrides: { [s.plugin]: cand } });
    report.blocking.push(...xref);
    // Conflict lint: new hits only.
    const key = (h) => `${h.rule}|${h.file}|${h.text}`;
    const before = new Set(conflictLint({ only: [s.plugin] }).map(key));
    report.newConflictHits = conflictLint({ pluginsRoot: tmp, only: [s.plugin] }).filter((h) => !before.has(key(h)));
    if (report.newConflictHits.length) report.warnings.push(`${report.newConflictHits.length} new conflict-lint hit(s); check against policies/conflicts.md.`);
    // Size of loaded instructions.
    const size = (dir) => listFiles(dir).filter((f) => f.endsWith('SKILL.md')).reduce((n, f) => n + fs.statSync(path.join(dir, f)).size, 0);
    report.skillMdBytes = { before: size(current), after: size(cand) };
    if (report.skillMdBytes.after > report.skillMdBytes.before * 1.3) report.warnings.push(`SKILL.md text grew ${Math.round((report.skillMdBytes.after / report.skillMdBytes.before - 1) * 100)}% (context cost).`);
  }
  fs.rmSync(tmp, { recursive: true, force: true });
  fs.mkdirSync(REPORTS, { recursive: true });
  const base = path.join(REPORTS, `${today()}-${id}-${to.slice(0, 7)}`);
  writeJson(`${base}.json`, report);
  fs.writeFileSync(`${base}.md`, renderReport(report));
  console.log(renderReport(report));
  console.log(`\nReport: ${path.relative(REPO_ROOT, base)}.md`);
  console.log(report.blocking.length ? 'BLOCKED: fix overlays/registry first (see above).' : `Ready for user review. Apply after approval: node scripts/skills.mjs apply ${id} --to ${to}`);
  if (report.blocking.length) process.exitCode = 2;
}

function renderReport(r) {
  const list = (a, n = 30) => (a?.length ? a.slice(0, n).map((x) => `- ${x}`).join('\n') + (a.length > n ? `\n- ... ${a.length - n} more` : '') : '- none');
  return `# Update proposal: ${r.id}

- Upstream: https://github.com/${r.repo}
- Pinned: \`${r.from?.slice(0, 12) || 'none'}\` -> proposed \`${r.to.slice(0, 12)}\`${r.commits != null ? ` (${r.commits} upstream commits)` : ''}
- Compare: https://github.com/${r.repo}/compare/${r.from}...${r.to}

## Verdict
${r.blocking.length ? `**BLOCKED** (${r.blocking.length})\n${list(r.blocking)}` : '**No blocking issues.** Apply only after the user approves.'}

## Warnings
${list(r.warnings)}

## Trigger scope (SKILL.md descriptions)
${r.descriptionChanges?.length ? r.descriptionChanges.map((d) => `- \`${d.file}\`\n  - before: ${d.before}\n  - after: ${d.after}`).join('\n') : '- unchanged (after overlays)'}

## Executable code (security-relevant)
### Added
${list(r.executables?.added)}
### Changed
${list(r.executables?.changed)}
### Removed
${list(r.executables?.removed)}

## Files
### Added (${r.added?.length ?? 0})
${list(r.added, 20)}
### Removed (${r.removed?.length ?? 0})
${list(r.removed, 20)}
### Changed (${r.changed?.length ?? 0})
${list(r.changed, 20)}

## New conflict-lint hits
${r.newConflictHits?.length ? r.newConflictHits.slice(0, 20).map((h) => `- [${h.rule}] ${h.file}:${h.line} ${h.text} (${h.note})`).join('\n') : '- none'}

## Instruction size
- SKILL.md bytes: ${r.skillMdBytes?.before ?? '?'} -> ${r.skillMdBytes?.after ?? '?'}
`;
}

function validate() {
  const r = spawnSync(process.execPath, [path.join(REPO_ROOT, 'scripts', 'validate.mjs')], { encoding: 'utf8' });
  return { ok: r.status === 0, out: (r.stdout + r.stderr).trim() };
}

function cmdApply(id) {
  const s = skillById(id);
  const from = loadLock().skills[id]?.sha;
  const to = opt('to');
  if (!to) throw new Error('apply needs --to <sha> (from the proposal report the user approved).');
  const reports = fs.existsSync(REPORTS) ? fs.readdirSync(REPORTS).filter((f) => f.endsWith(`-${id}-${to.slice(0, 7)}.json`)).sort() : [];
  if (!reports.length && !flag('force')) throw new Error(`No proposal report for ${id} @ ${to.slice(0, 7)}. Run propose first and get the user's approval.`);
  const report = reports.length ? readJson(path.join(REPORTS, reports.at(-1))) : null;
  if (report?.blocking.length && !flag('force')) throw new Error(`The proposal for ${id} @ ${to.slice(0, 7)} has blocking issues; fix them and propose again.`);
  setLock(id, to);
  try {
    buildAll({ only: [id] });
  } catch (e) {
    setLock(id, from);
    buildAll({ only: [id] });
    throw new Error(`Build failed; lock restored to ${from.slice(0, 7)}: ${e.message}`);
  }
  const v = validate();
  if (!v.ok) {
    setLock(id, from);
    buildAll({ only: [id] });
    throw new Error(`Validation failed; lock restored to ${from.slice(0, 7)}.\n${v.out}`);
  }
  appendHistory({ action: 'apply', id, plugin: s.plugin, from, to, report: report ? `reports/proposals/${reports.at(-1).replace(/\.json$/, '.md')}` : null, forced: flag('force') || undefined });
  console.log(`Applied ${id}: ${from?.slice(0, 7)} -> ${to.slice(0, 7)}. Validation passed.\nNext: commit (with the user's approval), then in Claude Code: claude plugin marketplace update dfw-marketplace && /reload-plugins`);
}

function cmdRollback(id) {
  const s = skillById(id);
  const current = loadLock().skills[id]?.sha;
  let to = opt('to');
  if (!to) {
    const prev = history().filter((h) => h.id === id && ['apply', 'rollback'].includes(h.action) && h.to === current).at(-1);
    if (!prev?.from) throw new Error(`No previous pin recorded for ${id}; pass --to <sha>.`);
    to = prev.from;
  }
  setLock(id, to);
  buildAll({ only: [id] });
  const v = validate();
  appendHistory({ action: 'rollback', id, plugin: s.plugin, from: current, to, validation: v.ok ? 'passed' : 'failed' });
  console.log(`Rolled back ${id}: ${current.slice(0, 7)} -> ${to.slice(0, 7)}. Validation ${v.ok ? 'passed' : 'FAILED'}.`);
  if (!v.ok) {
    console.log(v.out);
    process.exitCode = 1;
  }
}

function cmdHistory(id) {
  const h = history().filter((x) => !id || x.id === id);
  if (!h.length) return console.log('No history yet.');
  for (const e of h) console.log(`${e.at.slice(0, 16)} ${e.action.padEnd(8)} ${e.id.padEnd(17)} ${e.from?.slice(0, 7) || '-------'} -> ${e.to.slice(0, 7)}${e.report ? `  ${e.report}` : ''}`);
}

try {
  switch (cmd) {
    case 'status':
      cmdStatus();
      break;
    case 'check':
      cmdCheck();
      break;
    case 'propose':
      cmdPropose(args[1]);
      break;
    case 'apply':
      cmdApply(args[1]);
      break;
    case 'rollback':
      cmdRollback(args[1]);
      break;
    case 'history':
      cmdHistory(args[1]);
      break;
    default:
      console.log('Usage: node scripts/skills.mjs status | check [--json] | propose <id> [--to <sha>] | apply <id> --to <sha> [--force] | rollback <id> [--to <sha>] | history [id]');
      if (cmd) process.exitCode = 1;
  }
} catch (e) {
  console.error(e.message);
  process.exitCode = 1;
}
