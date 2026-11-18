#!/usr/bin/env node
// Weekly upstream check (GitHub Actions). For each skill with a new upstream commit:
// - build a proposal report;
// - non-blocking -> branch with the lock bump + rebuilt plugin + report, and a PR (merging = approval);
// - blocking -> an issue with the report (needs an overlay/registry fix first), once per upstream commit;
// - the proposal itself failed (network, crash) -> logged, the other skills still run, exit 1 at the end.
// Never pushes to the default branch.
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { REPO_ROOT } from './lib/build.mjs';

const sh = (cmd, args, opts = {}) => execFileSync(cmd, args, { cwd: REPO_ROOT, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'], ...opts }).trim();
const node = (...a) => sh(process.execPath, a);

export const blockedTitle = (u) => `Upstream update blocked: ${u.id} ${u.pinned.slice(0, 7)} -> ${u.latest.slice(0, 7)}`;

/** "skills.mjs propose" result: exit 0 = ready, exit 2 = blocking findings (report written), anything else = the run failed. */
export function proposeOutcome(err) {
  if (!err) return 'ready';
  return err.status === 2 ? 'blocked' : 'failed';
}

/** True when an open issue already has exactly this title (the search is fuzzy, so compare). */
export function issueExists(title, run = sh) {
  const out = run('gh', ['issue', 'list', '--state', 'open', '--search', `"${title}" in:title`, '--json', 'title', '--jq', '.[].title']);
  return out.split('\n').some((l) => l.trim() === title);
}

/** Newest proposal report (.md, relative to the repo) for this skill and commit, or null. */
export function findReport(id, sha, root = REPO_ROOT) {
  const dir = path.join(root, 'reports', 'proposals');
  if (!fs.existsSync(dir)) return null;
  const f = fs.readdirSync(dir).filter((x) => x.endsWith(`-${id}-${sha.slice(0, 7)}.md`)).sort().at(-1);
  return f ? path.join('reports', 'proposals', f) : null;
}

function main() {
  const dry = process.argv.includes('--dry-run');
  const base = sh('git', ['rev-parse', '--abbrev-ref', 'HEAD']);
  const updates = JSON.parse(node('scripts/skills.mjs', 'check', '--json')).filter((u) => u.upToDate === false);
  if (!updates.length) {
    console.log('All pinned skills are current.');
    return;
  }
  const failed = [];
  for (const u of updates) {
    const branch = `upstream/${u.id}-${u.latest.slice(0, 7)}`;
    const exists = sh('git', ['ls-remote', '--heads', 'origin', branch]);
    if (exists) {
      console.log(`${u.id}: ${branch} already proposed; skipping.`);
      continue;
    }
    sh('git', ['checkout', '-q', base]);
    let err = null;
    try {
      node('scripts/skills.mjs', 'propose', u.id, '--to', u.latest);
    } catch (e) {
      err = e;
    }
    const outcome = proposeOutcome(err);
    const reportPath = findReport(u.id, u.latest);
    if (outcome === 'failed' || !reportPath) {
      console.error(`${u.id}: proposal failed, skipped: ${String(err?.stderr || err?.message || 'no report written').trim().split('\n').slice(-3).join(' | ')}`);
      failed.push(u.id);
      continue;
    }
    const blocked = outcome === 'blocked';
    if (dry) {
      console.log(`${u.id}: ${blocked ? 'BLOCKED' : 'ready'} (${reportPath})`);
      continue;
    }
    if (blocked) {
      const title = blockedTitle(u);
      if (issueExists(title)) console.log(`${u.id}: blocked; an open issue already reports it.`);
      else {
        sh('gh', ['issue', 'create', '--title', title, '--body-file', path.join(REPO_ROOT, reportPath)]);
        console.log(`${u.id}: blocked -> issue opened.`);
      }
      fs.rmSync(path.join(REPO_ROOT, reportPath));
      fs.rmSync(path.join(REPO_ROOT, reportPath.replace(/\.md$/, '.json')), { force: true });
      continue;
    }
    sh('git', ['checkout', '-q', '-b', branch]);
    node('scripts/skills.mjs', 'apply', u.id, '--to', u.latest);
    sh('git', ['add', '-A']);
    sh('git', ['commit', '-q', '-m', `chore(skills): update ${u.id} to ${u.latest.slice(0, 7)}\n\nProposal report: ${reportPath}`]);
    sh('git', ['push', '-q', 'origin', branch]);
    sh('gh', ['pr', 'create', '--base', base, '--head', branch, '--title', `Update ${u.id} to ${u.latest.slice(0, 7)}`, '--body-file', path.join(REPO_ROOT, reportPath)]);
    // A PR opened with GITHUB_TOKEN does not fire pull_request workflows: start validate on the branch.
    try {
      sh('gh', ['workflow', 'run', 'validate.yml', '--ref', branch]);
    } catch (e) {
      console.error(`${u.id}: could not start validate.yml on ${branch} (the workflow token needs "actions: write"): ${String(e.stderr || e.message).trim().split('\n')[0]}`);
    }
    console.log(`${u.id}: PR opened from ${branch}. Merging it is the approval.`);
  }
  sh('git', ['checkout', '-q', base]);
  if (failed.length) {
    console.error(`Proposal failed for: ${failed.join(', ')}`);
    process.exitCode = 1;
  }
}

if (process.argv[1] === fileURLToPath(import.meta.url)) main();
