#!/usr/bin/env node
// Weekly upstream check (GitHub Actions). For each skill with a new upstream commit:
// - build a proposal report;
// - non-blocking -> branch with the lock bump + rebuilt plugin + report, and a PR (merging = approval);
// - blocking -> an issue with the report (needs an overlay/registry fix first).
// Never pushes to the default branch.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { REPO_ROOT } from './lib/build.mjs';

const sh = (cmd, args, opts = {}) => execFileSync(cmd, args, { cwd: REPO_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...opts }).trim();
const node = (...a) => sh(process.execPath, a);
const dry = process.argv.includes('--dry-run');

const base = sh('git', ['rev-parse', '--abbrev-ref', 'HEAD']);
const updates = JSON.parse(node('scripts/skills.mjs', 'check', '--json')).filter((u) => u.upToDate === false);
if (!updates.length) {
  console.log('All pinned skills are current.');
  process.exit(0);
}

for (const u of updates) {
  const branch = `upstream/${u.id}-${u.latest.slice(0, 7)}`;
  const exists = sh('git', ['ls-remote', '--heads', 'origin', branch]);
  if (exists) {
    console.log(`${u.id}: ${branch} already proposed; skipping.`);
    continue;
  }
  sh('git', ['checkout', '-q', base]);
  let blocked = false;
  try {
    node('scripts/skills.mjs', 'propose', u.id, '--to', u.latest);
  } catch {
    blocked = true;
  }
  const report = fs.readdirSync(path.join(REPO_ROOT, 'reports/proposals')).filter((f) => f.endsWith(`-${u.id}-${u.latest.slice(0, 7)}.md`)).sort().at(-1);
  const reportPath = path.join('reports/proposals', report);
  if (dry) {
    console.log(`${u.id}: ${blocked ? 'BLOCKED' : 'ready'} (${reportPath})`);
    continue;
  }
  if (blocked) {
    sh('gh', ['issue', 'create', '--title', `Upstream update blocked: ${u.id} ${u.pinned.slice(0, 7)} -> ${u.latest.slice(0, 7)}`, '--body-file', path.join(REPO_ROOT, reportPath)]);
    fs.rmSync(path.join(REPO_ROOT, reportPath));
    fs.rmSync(path.join(REPO_ROOT, reportPath.replace(/\.md$/, '.json')));
    console.log(`${u.id}: blocked -> issue opened.`);
    continue;
  }
  sh('git', ['checkout', '-q', '-b', branch]);
  node('scripts/skills.mjs', 'apply', u.id, '--to', u.latest);
  sh('git', ['add', '-A']);
  sh('git', ['commit', '-q', '-m', `chore(skills): update ${u.id} to ${u.latest.slice(0, 7)}\n\nProposal report: ${reportPath}`]);
  sh('git', ['push', '-q', 'origin', branch]);
  sh('gh', ['pr', 'create', '--base', base, '--head', branch, '--title', `Update ${u.id} to ${u.latest.slice(0, 7)}`, '--body-file', path.join(REPO_ROOT, reportPath)]);
  console.log(`${u.id}: PR opened from ${branch}. Merging it is the approval.`);
}
sh('git', ['checkout', '-q', base]);
