// How dfw runs the vendored Impeccable skill: project config cannot loosen it, only a
// checksum-verified engine runs, hooks cannot be re-enabled from shipped text, and a tool that
// did not run is visible to machines. No test here starts the real engine or uses the network.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { loadConfig, allowUnverifiedEngine } from '../../core/lib/config.mjs';
import { resolveEngine, verifyEngine, engineAssets, launcherCommand } from '../../core/lib/locate.mjs';
import { runImpeccable, setupEngine, formatReport, checkExitCode } from '../../core/lib/check.mjs';
import { leftoverCheck } from '../../scripts/lib/checks.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../..');
const DFW = path.join(REPO, 'core/bin/dfw.mjs');
const tmp = (p = 'dfw-imp-') => fs.mkdtempSync(path.join(os.tmpdir(), p));
const write = (file, text, mode) => {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, text, mode ? { mode } : undefined);
  return file;
};
const sha = (text) => crypto.createHash('sha256').update(text).digest('hex');

// ---- 1. project config env ----

test('config: project layers cannot set engine or loader variables, or loosen the privacy defaults', () => {
  const root = tmp();
  const globalFile = write(path.join(root, 'home', 'config.json'), '{}');
  const project = write(
    path.join(root, '.dfw', 'config.json'),
    JSON.stringify({
      env: {
        IMPECCABLE_BIN: '/repo/evil',
        IMPECCABLE_API_URL: 'http://evil.example',
        IMPECCABLE_UPDATE_HOST: 'evil.example',
        IMPECCABLE_HOME: '/repo/cache',
        DO_NOT_TRACK: '0',
        IMPECCABLE_NO_UPDATE_CHECK: null,
        IMPECCABLE_NO_TELEMETRY: '0',
        PATH: '/repo/bin',
        NODE_OPTIONS: '--require /repo/x.js',
        LD_PRELOAD: '/repo/x.so',
        DYLD_INSERT_LIBRARIES: '/repo/x.dylib',
        NO_COLOR: '1',
      },
    }),
  );
  const local = write(path.join(root, '.dfw', 'config.local.json'), JSON.stringify({ env: { IMPECCABLE_HOOK_DISABLED: '0', IMPECCABLE_NO_TELEMETRY: '1' } }));
  const warnings = [];
  const config = loadConfig(root, { warn: (m) => warnings.push(m), globalFile });
  assert.deepEqual(config.env, { DO_NOT_TRACK: '1', IMPECCABLE_NO_UPDATE_CHECK: '1', IMPECCABLE_HOOK_DISABLED: '1', NO_COLOR: '1', IMPECCABLE_NO_TELEMETRY: '1' });
  const dropped = ['IMPECCABLE_BIN', 'IMPECCABLE_API_URL', 'IMPECCABLE_UPDATE_HOST', 'IMPECCABLE_HOME', 'DO_NOT_TRACK', 'IMPECCABLE_NO_UPDATE_CHECK', 'IMPECCABLE_NO_TELEMETRY', 'PATH', 'NODE_OPTIONS', 'LD_PRELOAD', 'DYLD_INSERT_LIBRARIES'];
  for (const k of dropped) {
    assert.equal(warnings.filter((w) => w.includes(`env.${k} `) && w.includes(project)).length, 1, `one warning for ${k} naming the file`);
  }
  assert.equal(warnings.filter((w) => w.includes('env.IMPECCABLE_HOOK_DISABLED ') && w.includes(local)).length, 1);
  assert.equal(warnings.length, dropped.length + 1, 'allowed keys do not warn');
});

test('config: a project "env": null cannot erase the defaults; the global config may set anything', () => {
  const root = tmp();
  write(path.join(root, '.dfw', 'config.json'), '{"env":null}');
  const globalFile = write(path.join(root, 'home', 'config.json'), JSON.stringify({ env: { DO_NOT_TRACK: '0', IMPECCABLE_API_URL: 'http://mirror.internal' } }));
  const warnings = [];
  const config = loadConfig(root, { warn: (m) => warnings.push(m), globalFile });
  assert.equal(config.env.IMPECCABLE_NO_UPDATE_CHECK, '1');
  assert.equal(config.env.DO_NOT_TRACK, '0', 'the user may change a default in the global config');
  assert.equal(config.env.IMPECCABLE_API_URL, 'http://mirror.internal');
  assert.equal(warnings.length, 1);
});

test('config: the unverified-engine opt-in is read from the global file only', () => {
  const root = tmp();
  const globalFile = write(path.join(root, 'home', 'config.json'), '{}');
  write(path.join(root, '.dfw', 'config.json'), JSON.stringify({ impeccable: { allowUnverifiedEngine: true } }));
  assert.equal(allowUnverifiedEngine(globalFile), false);
  fs.writeFileSync(globalFile, JSON.stringify({ impeccable: { allowUnverifiedEngine: true } }));
  assert.equal(allowUnverifiedEngine(globalFile), true);
  fs.writeFileSync(globalFile, JSON.stringify({ impeccable: { allowUnverifiedEngine: 'true' } }));
  assert.equal(allowUnverifiedEngine(globalFile), false, 'only the boolean true opts in');
});

// ---- 2 and 3. engine lookup ----

const ENGINE = 'fake engine bytes';
/** A skill folder pinned at 9.9.9 whose checksum file lists the digest of ENGINE for every platform. */
function skill(root) {
  const skillDir = path.join(root, 'plugin', 'skills', 'impeccable');
  write(path.join(skillDir, 'scripts', 'impeccable'), '#!/bin/sh\nexit 99\n', 0o755);
  write(path.join(skillDir, 'scripts', 'impeccable.cmd'), '@echo off\r\n');
  write(path.join(skillDir, 'scripts', 'VERSION'), '9.9.9\n');
  const assets = ['impeccable-darwin-arm64', 'impeccable-darwin-x64', 'impeccable-linux-x64', 'impeccable-linux-arm64', 'impeccable-windows-x64.exe'];
  write(path.join(skillDir, 'scripts', 'engine-checksums.txt'), `# comment\n9.9.8 impeccable-linux-x64 ${sha('old')}\n${assets.map((a) => `9.9.9 ${a} ${sha(ENGINE)}`).join('\r\n')}\n`);
  const home = path.join(root, 'home');
  return { skillDir, home, stampFile: path.join(root, 'dfwhome', 'engine-verified.json'), env: {}, platform: 'linux', arch: 'x64' };
}

test('engine: nothing runs until the version cache holds a binary with the pinned sha256', () => {
  const o = skill(tmp());
  const cached = path.join(o.home, '.impeccable', 'bin', '9.9.9', 'impeccable');
  let e = resolveEngine(o);
  assert.equal(e.ready, false);
  assert.equal(e.cmd, null);
  assert.match(e.detail, /dfw check --setup/);

  write(cached, 'tampered', 0o755);
  e = resolveEngine(o);
  assert.equal(e.ready, false, 'a cached file with another digest is not run');
  assert.match(e.detail, /does not match the pinned checksum/);

  write(cached, ENGINE, 0o755);
  e = resolveEngine(o);
  assert.deepEqual([e.ready, e.verified, e.cmd], [true, true, cached], 'dfw runs the engine itself, not the sh launcher');
  assert.deepEqual(e.env, { IMPECCABLE_SKILL_DIR: o.skillDir, IMPECCABLE_SELF: path.join(o.skillDir, 'scripts', 'impeccable') });
});

test('engine: the verification is cached by path + size + mtime', () => {
  const o = skill(tmp());
  const cached = write(path.join(o.home, '.impeccable', 'bin', '9.9.9', 'impeccable'), ENGINE, 0o755);
  const digests = [sha(ENGINE)];
  fs.utimesSync(cached, new Date(1700000000000), new Date(1700000000000)); // whole milliseconds, so utimes can restore it
  assert.equal(verifyEngine(cached, digests, o.stampFile), true);
  const st = fs.statSync(cached);
  assert.deepEqual(JSON.parse(fs.readFileSync(o.stampFile, 'utf8'))[cached], { size: st.size, mtimeMs: st.mtimeMs, sha256: sha(ENGINE) });
  // Same size and mtime: the stamp answers and the file is not hashed again.
  fs.writeFileSync(cached, ENGINE.replace('fake', 'FAKE'));
  fs.utimesSync(cached, st.atime, st.mtime);
  assert.equal(verifyEngine(cached, digests, o.stampFile), true);
  // A new mtime or size invalidates the stamp, and the changed file fails the hash.
  fs.utimesSync(cached, st.atime, new Date(st.mtimeMs + 5000));
  assert.equal(verifyEngine(cached, digests, o.stampFile), false);
  fs.writeFileSync(cached, `${ENGINE}!`);
  assert.equal(verifyEngine(cached, digests, o.stampFile), false);
  // A stamp for a digest that is no longer pinned (skill update) does not count.
  fs.writeFileSync(cached, ENGINE);
  assert.equal(verifyEngine(cached, digests, o.stampFile), true);
  assert.equal(verifyEngine(cached, [sha('next release')], o.stampFile), false);
  assert.equal(verifyEngine(cached, [], o.stampFile), false);
});

test('engine: unverified locations are used only with the global opt-in', () => {
  const root = tmp();
  const o = skill(root);
  const envBin = write(path.join(root, 'own', 'impeccable'), 'own build', 0o755);
  const sibling = write(path.join(o.skillDir, 'scripts', 'bin', 'linux-x64', 'impeccable'), 'sibling', 0o755);
  const homeBin = write(path.join(o.home, '.impeccable', 'bin', 'impeccable'), 'home', 0o755);
  const pathBin = write(path.join(root, 'pathdir', 'impeccable'), 'path', 0o755);
  const env = { IMPECCABLE_BIN: envBin, PATH: `${path.join(root, 'empty')}:${path.join(root, 'pathdir')}` };

  let e = resolveEngine({ ...o, env });
  assert.equal(e.ready, false);
  assert.equal(e.cmd, null);
  assert.deepEqual(e.unverified, [envBin, sibling, homeBin, pathBin], 'reported so setup can refuse to start the launcher');

  const cached = write(path.join(o.home, '.impeccable', 'bin', '9.9.9', 'impeccable'), ENGINE, 0o755);
  e = resolveEngine({ ...o, env });
  assert.deepEqual([e.cmd, e.verified], [cached, true], 'without the opt-in the verified cache wins over IMPECCABLE_BIN');

  e = resolveEngine({ ...o, env, allowUnverified: true });
  assert.deepEqual([e.cmd, e.ready, e.verified], [envBin, true, false]);
  e = resolveEngine({ ...o, env: { PATH: env.PATH }, allowUnverified: true });
  assert.equal(e.cmd, sibling, 'launcher order: sibling build before ~/.impeccable/bin/impeccable');
  fs.rmSync(sibling);
  fs.rmSync(homeBin);
  fs.rmSync(cached);
  e = resolveEngine({ ...o, env: { PATH: env.PATH }, allowUnverified: true });
  assert.deepEqual([e.cmd, e.verified], [pathBin, false], 'PATH is the last resort');
});

test('engine on Windows: .exe in the cache, the .cmd launcher, and no sh', () => {
  const o = { ...skill(tmp()), platform: 'win32', arch: 'x64' };
  const dir = path.join(o.home, '.impeccable', 'bin', '9.9.9');
  write(path.join(dir, 'impeccable'), ENGINE, 0o755);
  let e = resolveEngine(o);
  assert.equal(e.ready, false, 'the POSIX file name is not the Windows engine');
  assert.equal(path.basename(e.cached), 'impeccable.exe');
  assert.equal(path.basename(e.launcher), 'impeccable.cmd');

  write(path.join(dir, 'impeccable.exe'), ENGINE);
  e = resolveEngine(o);
  assert.deepEqual([e.ready, e.verified, path.basename(e.cmd)], [true, true, 'impeccable.exe']);
  assert.equal(resolveEngine({ ...o, arch: 'arm64' }).ready, true, 'Windows on ARM accepts the x64 build, as the upstream launcher does');

  const env = { PATH: `${path.join(o.home, 'a')};${path.join(o.home, 'b')}` };
  const shim = write(path.join(o.home, 'b', 'impeccable.cmd'), '');
  fs.rmSync(path.join(dir, 'impeccable.exe'));
  assert.deepEqual(resolveEngine({ ...o, env }).unverified, [shim], 'PATH is split on ";" and runnable extensions are tried');

  const c = launcherCommand(e.launcher, ['detect', '--json', 'C:\\p\\setup.html'], { platform: 'win32', env: { ComSpec: 'C:\\Windows\\system32\\cmd.exe' } });
  assert.equal(c.cmd, 'C:\\Windows\\system32\\cmd.exe');
  assert.deepEqual(c.args.slice(0, 3), ['/d', '/s', '/c']);
  assert.equal(c.args[3], `""${e.launcher}" "detect" "--json" "C:\\p\\setup.html""`);
  assert.equal(c.options.windowsVerbatimArguments, true);
  assert.ok(![c.cmd, ...c.args].some((x) => /(^|[\\/])sh(\.exe)?$/.test(x)));
  assert.deepEqual(launcherCommand('/x/impeccable', ['a'], { platform: 'linux' }), { cmd: '/x/impeccable', args: ['a'], options: {} });

  assert.deepEqual(engineAssets('win32', 'x64'), ['impeccable-windows-x64.exe']);
  assert.deepEqual(engineAssets('darwin', 'arm64'), ['impeccable-darwin-arm64']);
  assert.deepEqual(engineAssets('freebsd', 'x64'), []);
});

test('check: an unverified engine is "not-installed"; a verified one is run directly with the skill dir', { skip: process.platform === 'win32' }, () => {
  const root = tmp();
  const none = runImpeccable(root, ['a.html'], { env: {}, timeoutMs: 5000, engine: { ready: false, cmd: null, detail: 'Engine not downloaded yet; run "dfw check --setup" once.' } });
  assert.deepEqual([none.status, none.findings.length], ['not-installed', 0]);
  assert.match(none.detail, /dfw check --setup/);
  assert.equal(runImpeccable(root, ['a.html'], { env: {}, timeoutMs: 5000, engine: null }).status, 'unavailable');

  // A stand-in script, not the Impeccable engine: it reports the environment it was started with.
  const stub = write(path.join(root, 'stub'), '#!/bin/sh\nprintf \'[{"antipattern":"env","name":"%s","description":"%s","severity":"warning","file":"a.html"}]\' "$IMPECCABLE_SKILL_DIR" "$DO_NOT_TRACK"\n', 0o755);
  const ran = runImpeccable(root, ['a.html'], { env: { DO_NOT_TRACK: '1' }, timeoutMs: 5000, engine: { ready: true, cmd: stub, env: { IMPECCABLE_SKILL_DIR: '/skill' } } });
  assert.equal(ran.status, 'ran');
  assert.equal(ran.findings[0].message, '/skill: 1');
});

test('check --setup: the launcher only downloads, and the result must verify', () => {
  const calls = [];
  const launch = (...a) => (calls.push(a), { status: 0, stderr: 'curl: no network' });
  const notReady = { ready: false, unverified: [], launcher: '/s/impeccable', detail: 'Engine not downloaded yet; run "dfw check --setup" once.' };

  assert.deepEqual(setupEngine({ resolve: () => ({ ready: true }), launch }), { ok: true, text: 'Impeccable engine ready.' });
  assert.equal(calls.length, 0, 'a verified engine needs no launcher run');

  const stopped = setupEngine({ resolve: () => ({ ...notReady, unverified: ['/usr/local/bin/impeccable'] }), launch });
  assert.equal(stopped.ok, false);
  assert.match(stopped.text, /cannot verify \(\/usr\/local\/bin\/impeccable\).*allowUnverifiedEngine true --global/);
  assert.equal(calls.length, 0, 'the launcher would run that binary, so it is not started');

  const states = [notReady, { ready: true }];
  assert.equal(setupEngine({ resolve: () => states.shift(), launch, target: '/p/setup.html', env: { DO_NOT_TRACK: '1' } }).ok, true);
  assert.equal(calls.length, 1);
  if (process.platform !== 'win32') assert.deepEqual(calls[0].slice(0, 2), ['/s/impeccable', ['detect', '--json', '/p/setup.html']]);
  assert.equal(calls[0][2].env.DO_NOT_TRACK, '1');

  const failed = setupEngine({ resolve: () => notReady, launch, target: '/p/setup.html' });
  assert.deepEqual(failed, { ok: false, text: 'Impeccable setup failed: curl: no network' }, 'exit 0 from the launcher is not enough');
  assert.equal(setupEngine({ resolve: () => null, launch }).ok, false);
});

// ---- 5. machines see a tool that did not run ----

test('check: unverified tools are listed, and --strict turns them into exit 3', () => {
  const results = [
    { tool: 'dfw-lint', status: 'ran', findings: [] },
    { tool: 'impeccable', status: 'not-installed', detail: 'run "dfw check --setup" once', findings: [] },
    { tool: 'sloplint', status: 'skipped', findings: [] },
  ];
  const rep = formatReport(results);
  assert.deepEqual(rep.unverified, ['impeccable']);
  assert.equal(checkExitCode(rep), 0, 'default exit code does not change');
  assert.equal(checkExitCode(rep, { strict: true }), 3);
  const must = formatReport([...results, { tool: 'x', status: 'ran', findings: [{ level: 'error', file: 'a', line: 1, rule: 'r', source: 'x', message: 'm' }] }]);
  assert.equal(checkExitCode(must), 2);
  assert.equal(checkExitCode(must, { strict: true }), 2, 'MUST findings keep exit 2');
  assert.equal(checkExitCode(formatReport([results[0], results[2]]), { strict: true }), 0, 'skipped is not "did not run"');
});

test('cli: "dfw check --json" has an unverified array and --strict exits 3 (no engine in an empty cache)', () => {
  const root = tmp();
  write(path.join(root, '.dfw', 'config.json'), JSON.stringify({ checks: { sloplint: false } }));
  write(path.join(root, 'page.html'), '<!doctype html><html lang="en"><title>x</title><p>x</p></html>');
  // An empty engine cache: the engine is reported missing and nothing is executed or downloaded.
  const env = { ...process.env, DFW_HOME: path.join(root, 'dfwhome'), IMPECCABLE_HOME: path.join(root, 'no-engine'), IMPECCABLE_BIN: '', CLAUDE_PROJECT_DIR: root };
  const run = (...args) => spawnSync(process.execPath, [DFW, 'check', 'page.html', ...args], { cwd: root, env, encoding: 'utf8' });
  const plain = run('--json');
  assert.equal(plain.status, 0, plain.stderr);
  const data = JSON.parse(plain.stdout);
  assert.deepEqual(data.unverified, ['impeccable']);
  assert.equal(data.results.find((r) => r.tool === 'impeccable').status, 'not-installed');
  assert.equal(run('--json', '--strict').status, 3);
  assert.equal(run('--strict').status, 3);
  assert.equal(spawnSync(process.execPath, [DFW, 'check', '--strict', 'page.html', '--json'], { cwd: root, env, encoding: 'utf8' }).status, 3, '--strict before the file keeps the file as a target');
});

// ---- 4. hooks cannot be re-enabled from shipped text ----

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
  assert.match(setup, /run `context` only through this launcher with its defaults.*never run it with that variable unset or changed, or with hooks enabled/);
  assert.match(fs.readFileSync(path.join(skillDir, 'reference/doctor.md'), 'utf8'), /any finding about the design hook: report it and do not repair it/);
});

test('checks: Impeccable text that tells how to enable hooks fails validation; the repo has none', () => {
  const root = tmp('dfw-left-');
  const ref = path.join(root, 'impeccable', 'skills', 'impeccable', 'reference');
  write(path.join(ref, 'ok.md'), '**Hooks:** not available in dfw-marketplace. Never run `hooks on`.\nWhen the design hook is active it already enforces the checks.\n');
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

// ---- 6. texts ----

test('texts: R9 names generate-image, shape is planning only, the reviewer says what it writes, README lists what remains', () => {
  const read = (rel) => fs.readFileSync(path.join(REPO, rel), 'utf8');
  const conflicts = read('core/workflow/policies/conflicts.md');
  const r9 = conflicts.split('\n').find((l) => l.includes('no paid APIs by default'));
  assert.match(r9, /Impeccable `generate-image`/);
  assert.doesNotMatch(r9, /Impeccable `generate` /);
  const r3 = conflicts.slice(conflicts.indexOf('## R3.'), conflicts.indexOf('## R4.'));
  assert.match(r3, /`shape` is allowed for UX planning only/);
  assert.doesNotMatch(r3, /`\/impeccable shape` \/ concept seed/);
  assert.match(read('core/workflow/references/phase-discovery.md'), /`shape` command for UX planning of a single feature \(planning only: stop before its design-direction phase/);

  const reviewer = read('core/workflow/agents/visual-reviewer.md');
  assert.doesNotMatch(reviewer, /read-only/i);
  assert.match(reviewer, /never edits design or code/);
  assert.match(reviewer, /records Impeccable `critique`\/`audit` save under `\.impeccable\/`/);
  assert.doesNotMatch(read('core/workflow/SKILL.md'), /visual-reviewer.*\(read-only\)/);

  const readme = read('README.md');
  assert.doesNotMatch(readme, /Two calls remain/);
  for (const host of ['api/roll', 'api/commands', 'api/version', 'api/chosen', 'api.openai.com', 'fonts.googleapis.com', 'api.impeccable.style']) assert.ok(readme.includes(host), `README lists ${host}`);
  assert.match(readme, /"Observed" means seen at runtime.*"read" means/);
  assert.match(readme, /impeccable\.allowUnverifiedEngine true --global/);
  assert.doesNotMatch(readme, /\]\(\.?\/?docs\//, 'README has no links into docs/');
});
