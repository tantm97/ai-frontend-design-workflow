// The build, validation and maintenance scripts (scripts/): the leftover check, vendored
// knowledge, overlays applied to Impeccable, the upstream bot, and the workflow and README texts
// they guard. The dfw plugin folder and its marketplaces: plugin-layout.test.mjs.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';
import { leftoverCheck, versionMismatch, trackedToolingCheck, workflowPipeCheck, WORKFLOWS_DIR } from '../../scripts/lib/checks.mjs';
import { checkSteps, WINDOWS_TARGET, NO_OP_CC } from '../../scripts/lib/check-steps.mjs';
import { outputOf } from '../../scripts/lib/no-op-cc.mjs';
import { blockedTitle, proposeOutcome, issueExists, findReport } from '../../scripts/ci-upstream.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const tmp = (p) => fs.mkdtempSync(path.join(os.tmpdir(), p));
const write = (file, text, mode) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text, mode ? { mode } : undefined);
  return file;
};

test('knowledge: vendored file keeps the upstream text byte for byte between markers', async () => {
  const { wrap, unwrap } = await import('../../scripts/knowledge.mjs');
  const body = '# Rules\n- MUST: x ≥ 24px …\n';
  const text = wrap(
    { repo: 'o/r', commit: 'a'.repeat(40), path: 'AGENTS.md', license: 'MIT', title: 'T', usage: 'U' },
    body,
    'MIT License\n...',
  );
  assert.equal(unwrap(text), body);
  assert.match(text, /## License \(MIT/);
  const reg = JSON.parse(fs.readFileSync(path.join(REPO, 'registry', 'skills.json'), 'utf8'));
  for (const k of reg.knowledge) assert.match(k.commit, /^[0-9a-f]{40}$/);
});

test('overlay: the hooks reference is a short "not available" note and context is bounded', () => {
  const skillDir = path.join(REPO, 'plugins/impeccable/skills/impeccable');
  const hooks = fs.readFileSync(path.join(skillDir, 'reference/hooks.md'), 'utf8');
  assert.ok(hooks.length < 1500, 'short note, not the upstream guide');
  assert.match(hooks, /not available under dfw-marketplace and must not be enabled/);
  assert.doesNotMatch(hooks, /hooks on\b|settings\.local\.json|<skill-base-dir>/);
  const skillMd = fs.readFileSync(path.join(skillDir, 'SKILL.md'), 'utf8');
  assert.match(skillMd, /^\*\*Hooks:\*\* not available in dfw-marketplace and must not be enabled\./m);
  assert.match(skillMd, /^\*\*Pin \/ Unpin:\*\* not available in dfw-marketplace and must not be run/m);
  const setup = skillMd.split('\n').find((l) => l.startsWith('1. Run `"${CLAUDE_SKILL_DIR}/scripts/impeccable" context`'));
  assert.match(
    setup,
    /run `context` only through this launcher with its defaults.*never run it with that variable unset or changed, or with hooks enabled/,
  );
  assert.match(
    fs.readFileSync(path.join(skillDir, 'reference/doctor.md'), 'utf8'),
    /any finding about the design hook: report it and do not repair it/,
  );
});

test('checks: Impeccable text that tells how to enable hooks fails validation; the repo has none', () => {
  const root = tmp('dfw-left-');
  const ref = path.join(root, 'impeccable', 'skills', 'impeccable', 'reference');
  write(
    path.join(ref, 'ok.md'),
    '**Hooks:** not available in dfw-marketplace. Never run `hooks on`.\nWhen the design hook is active it already enforces the checks.\n',
  );
  assert.deepEqual(leftoverCheck({ pluginsRoot: root }), []);
  // Lines quoted from the upstream hooks reference.
  write(path.join(ref, 'hooks.md'), '# /impeccable hooks\n\nstatus says so and gives `impeccable hooks on` as the repair command.\n');
  write(path.join(ref, 'admin.md'), 'Invoke the admin script:\n\n   "<skill-base-dir>/scripts/impeccable" hooks <action> [args...]\n');
  const errs = leftoverCheck({ pluginsRoot: root });
  assert.deepEqual(errs, [
    'impeccable/skills/impeccable/reference/admin.md:3: instructions to enable Impeccable hooks shipped (replace them in overlays/impeccable.json).',
    'impeccable/skills/impeccable/reference/hooks.md:3: instructions to enable Impeccable hooks shipped (replace them in overlays/impeccable.json).',
  ]);
  // Other plugins may talk about hooks of their own (React hooks on a component, ...).
  write(path.join(root, 'other', 'skills', 's', 'a.md'), 'Use hooks on the client only.\n');
  assert.equal(leftoverCheck({ pluginsRoot: root }).length, 2);
  assert.deepEqual(leftoverCheck(), []);
});

test('texts: R9 names generate-image, shape is planning only, the reviewer says what it writes, README lists what remains', () => {
  const read = (rel) => fs.readFileSync(path.join(REPO, rel), 'utf8');
  const conflicts = read('core/workflow/policies/conflicts.md');
  const r9 = conflicts.split('\n').find((l) => l.includes('no paid APIs by default'));
  assert.match(r9, /Impeccable `generate-image`/);
  assert.doesNotMatch(r9, /Impeccable `generate` /);
  const r3 = conflicts.slice(conflicts.indexOf('## R3.'), conflicts.indexOf('## R4.'));
  assert.match(r3, /`shape` is allowed for UX planning only/);
  assert.doesNotMatch(r3, /`\/impeccable shape` \/ concept seed/);
  assert.match(
    read('core/workflow/references/phase-discovery.md'),
    /`shape` command for UX planning of a single feature \(planning only: stop before its design-direction phase/,
  );

  const reviewer = read('core/workflow/agents/visual-reviewer.md');
  assert.doesNotMatch(reviewer, /read-only/i);
  assert.match(reviewer, /never edits design or code/);
  assert.match(reviewer, /records Impeccable `critique`\/`audit` save under `\.impeccable\/`/);
  assert.doesNotMatch(read('core/workflow/SKILL.md'), /visual-reviewer.*\(read-only\)/);

  const readme = read('README.md');
  assert.doesNotMatch(readme, /Two calls remain/);
  for (const host of [
    'api/roll',
    'api/commands',
    'api/version',
    'api/chosen',
    'api.openai.com',
    'fonts.googleapis.com',
    'api.impeccable.style',
  ])
    assert.ok(readme.includes(host), `README lists ${host}`);
  assert.match(readme, /"Observed" means seen at runtime.*"read" means/);
  assert.match(readme, /impeccable\.allowUnverifiedEngine true --global/);
  assert.doesNotMatch(readme, /\]\(\.?\/?docs\//, 'README has no links into docs/');
});

test('checks: built plugins with <skill-dir> or a hook installer are errors; the repo has none', () => {
  const root = tmp('dfw-left-');
  fs.mkdirSync(path.join(root, 'p', 'skills', 's', 'scripts'), { recursive: true });
  fs.writeFileSync(path.join(root, 'p', 'skills', 's', 'ok.md'), 'Run node ${CLAUDE_SKILL_DIR}/scripts/x.mjs\n');
  assert.deepEqual(leftoverCheck({ pluginsRoot: root }), []);
  fs.writeFileSync(path.join(root, 'p', 'skills', 's', 'ref.md'), 'a\nnode <skill-dir>/scripts/x.mjs\n');
  fs.writeFileSync(path.join(root, 'p', 'skills', 's', 'scripts', 'install-hook.mjs'), '');
  const errs = leftoverCheck({ pluginsRoot: root });
  assert.equal(errs.length, 2);
  assert.ok(errs.some((e) => e.startsWith('p/skills/s/ref.md:2: unresolved <skill-dir>')));
  assert.ok(errs.some((e) => e.startsWith('p/skills/s/scripts/install-hook.mjs: upstream hook installer')));
  assert.deepEqual(leftoverCheck(), []);
});

test('upstream bot: propose outcomes, one blocked issue per commit, missing report', () => {
  assert.equal(proposeOutcome(null), 'ready');
  assert.equal(proposeOutcome({ status: 2 }), 'blocked');
  assert.equal(proposeOutcome({ status: 1 }), 'failed');
  assert.equal(proposeOutcome(new Error('spawn ENOENT')), 'failed');
  const title = blockedTitle({ id: 'hallmark', pinned: 'a'.repeat(40), latest: 'b'.repeat(40) });
  assert.equal(title, 'Upstream update blocked: hallmark aaaaaaa -> bbbbbbb');
  let seen;
  const gh = (out) => (cmd, args) => ((seen = [cmd, ...args]), out);
  assert.equal(issueExists(title, gh(`${title}\n`)), true);
  assert.deepEqual(seen.slice(0, 3), ['gh', 'issue', 'list']);
  assert.ok(seen.includes('open'));
  assert.equal(issueExists(title, gh('Upstream update blocked: hallmark aaaaaaa -> ccccccc')), false); // fuzzy search hit
  assert.equal(issueExists(title, gh('')), false);
  const root = tmp('dfw-rep-');
  assert.equal(findReport('hallmark', 'b'.repeat(40), root), null);
  fs.mkdirSync(path.join(root, 'reports', 'proposals'), { recursive: true });
  for (const d of ['2026-11-01', '2026-11-08']) fs.writeFileSync(path.join(root, 'reports', 'proposals', `${d}-hallmark-bbbbbbb.md`), '');
  assert.equal(findReport('hallmark', 'b'.repeat(40), root), path.join('reports', 'proposals', '2026-11-08-hallmark-bbbbbbb.md'));
});

test('versions: core/manifest.json, package.json, package-lock.json and native/Cargo.toml must agree; the repo does', () => {
  const root = tmp('dfw-versions-');
  const files = {
    'core/manifest.json': '{ "name": "dfw", "version": "1.2.3" }',
    'package.json': '{ "version": "1.2.3" }',
    'package-lock.json': '{ "version": "1.2.3", "packages": { "": { "version": "1.2.3" } } }',
    'native/Cargo.toml': '[package]\nname = "dfw"\nversion = "1.2.3"\n',
    'native/Cargo.lock': 'version = 4\n\n[[package]]\nname = "dfw"\nversion = "1.2.3"\n',
  };
  for (const [rel, text] of Object.entries(files)) write(path.join(root, rel), text);
  assert.equal(versionMismatch(root), null);
  write(path.join(root, 'native/Cargo.toml'), '[package]\nname = "dfw"\nversion = "1.2.4"\n');
  assert.match(versionMismatch(root), /^versions differ: .*native\/Cargo\.toml 1\.2\.4.*Bump them together\.$/);
  assert.equal(versionMismatch(), null);
});

test('checks: tracked local tooling (AGENTS.md, docs/) is an error; vendored .claude/ folders are not; the repo has none', () => {
  const repoWith = (files) => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-tooling-'));
    execFileSync('git', ['init', '-q', dir]);
    for (const f of files) {
      fs.mkdirSync(path.dirname(path.join(dir, f)), { recursive: true });
      fs.writeFileSync(path.join(dir, f), 'x');
    }
    execFileSync('git', ['-C', dir, 'add', '--', ...files]);
    return dir;
  };
  assert.equal(trackedToolingCheck(repoWith(['AGENTS.md'])).length, 1);
  assert.equal(trackedToolingCheck(repoWith(['docs/x.md'])).length, 1);
  assert.deepEqual(trackedToolingCheck(repoWith(['plugins/x/.claude/skills/s/SKILL.md', '.claude-plugin/marketplace.json'])), []);
  assert.deepEqual(trackedToolingCheck(), []);
});

test('checks: a workflow line that pipes into grep -q is an error; a comment is not; the repo has none', () => {
  const root = tmp('dfw-workflows-');
  const yml = (lines) => write(path.join(root, WORKFLOWS_DIR, 'ci.yml'), `jobs:\n  x:\n    steps:\n${lines.join('\n')}\n`);
  yml(['      - run: rustc -vV | grep -q "host: x"']);
  assert.match(workflowPipeCheck(root).join('\n'), /ci\.yml:4: a pipe into grep -q/);
  yml(['      - run: dfw --version |grep --quiet 0.6', '      - run: ls | grep -c x']);
  assert.equal(workflowPipeCheck(root).length, 1);
  yml(['      # `cmd | grep -q` fails under pipefail', '      - run: out=$(rustc -vV); case "$out" in *host*) ;; esac']);
  assert.deepEqual(workflowPipeCheck(root), []);
  assert.deepEqual(workflowPipeCheck(), []);
});

test('check: macOS and Linux also run clippy for the Windows target with a no-op C compiler; Windows does not', () => {
  const windowsClippy = (platform) => checkSteps({ platform }).filter((s) => s.args.includes(WINDOWS_TARGET));
  for (const platform of ['darwin', 'linux']) {
    const [step, ...more] = windowsClippy(platform);
    assert.equal(more.length, 0);
    assert.equal(step.args[0], 'clippy');
    assert.ok(step.args.includes('--all-targets') && step.args.includes('warnings'));
    assert.equal(step.target, WINDOWS_TARGET);
    assert.deepEqual(Object.values(step.env), [NO_OP_CC, NO_OP_CC]);
    assert.ok(Object.keys(step.env).every((k) => k.endsWith(WINDOWS_TARGET.replaceAll('-', '_'))));
    const names = checkSteps({ platform }).map((s) => s.name);
    assert.ok(names.indexOf(step.name) > names.indexOf('cargo clippy') && names.indexOf(step.name) < names.indexOf('cargo test'));
  }
  assert.deepEqual(windowsClippy('win32'), []);
  if (process.platform !== 'win32') assert.ok(fs.statSync(NO_OP_CC).mode & 0o111, 'the no-op compiler is executable');
});

test('no-op compiler: writes the -o file and the archive empty, and nothing for a probe', () => {
  assert.equal(outputOf(['-O0', '-c', 'a.c', '-o', 'out/a.o']), 'out/a.o');
  assert.equal(outputOf(['cq', 'libring.a', 'a.o', 'b.o']), 'libring.a');
  assert.equal(outputOf(['s', 'libring.a']), 'libring.a');
  assert.equal(outputOf(['-E', 'detect_compiler_family.c']), undefined);
  const dir = tmp('dfw-no-op-cc-');
  const src = write(path.join(dir, 'probe.c'), 'int x;');
  execFileSync(process.execPath, [NO_OP_CC, '-c', src, '-o', path.join(dir, 'probe.o')]);
  execFileSync(process.execPath, [NO_OP_CC, '-E', src]);
  assert.equal(fs.readFileSync(path.join(dir, 'probe.o'), 'utf8'), '');
  assert.equal(fs.readFileSync(src, 'utf8'), 'int x;');
});
