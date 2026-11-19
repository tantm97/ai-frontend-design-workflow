// The plugin launchers (core/bin/dfw for sh, core/bin/dfw.cmd for Windows): they run the dfw binary
// installed for the plugin's version, else the newest installed one, else say how to install it.
// Nothing here needs Node.js on PATH or the network.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const tmp = (p) => fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), p)));
const posix = { skip: process.platform === 'win32' ? 'the sh launcher runs on macOS and Linux' : false };

/** A plugin dir holding the real launcher at version 9.9.9, plus a DFW_HOME with fake binaries. */
function launcherFixture(versions, { manifest = '.claude-plugin/plugin.json' } = {}) {
  const root = tmp('dfw-launch-');
  const plugin = path.join(root, 'plugin');
  fs.mkdirSync(path.join(plugin, 'bin'), { recursive: true });
  fs.mkdirSync(path.dirname(path.join(plugin, manifest)), { recursive: true });
  fs.copyFileSync(path.join(REPO, 'core', 'bin', 'dfw'), path.join(plugin, 'bin', 'dfw'));
  fs.chmodSync(path.join(plugin, 'bin', 'dfw'), 0o755);
  fs.writeFileSync(path.join(plugin, manifest), '{\n  "name": "dfw",\n  "version": "9.9.9"\n}\n');
  const home = path.join(root, 'home');
  fs.mkdirSync(path.join(home, 'bin'), { recursive: true });
  for (const v of versions) {
    fs.mkdirSync(path.join(home, 'bin', v));
    fs.writeFileSync(path.join(home, 'bin', v, 'dfw'), `#!/bin/sh\necho "fake ${v} root=$DFW_PLUGIN_ROOT args=$*"\n`, { mode: 0o755 });
  }
  // Only the system tools on PATH: no Node.js.
  const run = (env = {}, args = ['version']) => spawnSync('/bin/sh', [path.join(plugin, 'bin', 'dfw'), ...args], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: root, DFW_HOME: home, ...env } });
  return { root, plugin, home, run };
}

test('launcher: exact-version binary runs silently', posix, () => {
  const { plugin, run } = launcherFixture(['0.5.1', '9.9.9']);
  const r = run({}, ['status', '--json']);
  assert.equal(r.status, 0);
  const m = /^fake 9\.9\.9 root=(.+) args=status --json$/.exec(r.stdout.trim());
  assert.ok(m, r.stdout);
  assert.equal(fs.realpathSync(m[1]), fs.realpathSync(plugin));
  assert.equal(r.stderr, '');
});

test('launcher: the version comes from the root plugin.json of other hosts too', posix, () => {
  const r = launcherFixture(['9.9.9'], { manifest: 'plugin.json' }).run();
  assert.match(r.stdout, /^fake 9\.9\.9 /);
});

test('launcher: no exact binary -> newest installed binary, one stderr line', posix, () => {
  const { run } = launcherFixture(['0.5.1', '0.10.0', '0.9.3']);
  const r = run();
  assert.equal(r.status, 0);
  assert.match(r.stdout, /^fake 0\.10\.0 /); // numeric order, not 0.9.3
  assert.equal(r.stderr.trim().split('\n').length, 1);
  assert.match(r.stderr, /^dfw: no binary for plugin version 9\.9\.9; running 0\.10\.0\. Update: curl .*install\.sh \| sh\n$/);
});

test('launcher: nothing installed -> 127 and how to install, never Node', posix, () => {
  const { home, run } = launcherFixture([]);
  // A node on PATH is not used: the binary is the only runtime.
  const fakeBin = path.join(home, 'path');
  fs.mkdirSync(fakeBin);
  fs.writeFileSync(path.join(fakeBin, 'node'), '#!/bin/sh\necho node ran\n', { mode: 0o755 });
  const r = run({ PATH: `${fakeBin}:/usr/bin:/bin` });
  assert.equal(r.status, 127);
  assert.equal(r.stdout, '');
  assert.match(r.stderr, /^dfw: the dfw binary is not installed \(looked in .*\/home\/bin\/9\.9\.9\/\)\.\n/);
  assert.match(r.stderr, /Install it: curl -fsSL https:\/\/raw\.githubusercontent\.com\/tantm97\/ai-frontend-design-workflow\/main\/install\.sh \| sh/);
  // A version folder without an executable is not a binary.
  fs.mkdirSync(path.join(home, 'bin', '9.9.9'));
  fs.writeFileSync(path.join(home, 'bin', '9.9.9', 'dfw'), 'not executable');
  assert.equal(run().status, 127);
});

test('launcher: DFW_BIN names an executable -> it runs, before any installed binary', posix, () => {
  const { plugin, run } = launcherFixture(['9.9.9']);
  const bin = path.join(path.dirname(plugin), 'native-dfw');
  fs.writeFileSync(bin, '#!/bin/sh\necho "native root=$DFW_PLUGIN_ROOT args=$*"\n', { mode: 0o755 });
  const r = run({ DFW_BIN: bin });
  assert.equal(r.status, 0);
  const m = /^native root=(.+) args=version$/.exec(r.stdout.trim());
  assert.ok(m, r.stdout);
  assert.equal(fs.realpathSync(m[1]), fs.realpathSync(plugin));
  assert.equal(r.stderr, '');
  // Not an executable file (missing, a directory, no x bit): the launcher behaves as without DFW_BIN.
  fs.chmodSync(bin, 0o644);
  for (const DFW_BIN of [bin, path.join(plugin, 'missing'), plugin]) assert.match(run({ DFW_BIN }).stdout, /^fake 9\.9\.9 /);
});

test('launcher: a symlink to the launcher (~/.local/bin/dfw) finds its plugin folder', posix, () => {
  const { root, plugin } = launcherFixture(['9.9.9']);
  const link = path.join(root, 'link', 'dfw');
  fs.mkdirSync(path.dirname(link));
  fs.symlinkSync(path.join(plugin, 'bin', 'dfw'), link);
  const r = spawnSync('/bin/sh', [link, 'version'], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: root, DFW_HOME: path.join(root, 'home') } });
  assert.match(r.stdout, /^fake 9\.9\.9 root=.*\/plugin args=version$/m);
});

// cmd.exe cannot run here: the Windows launcher is checked for what it must contain, and the
// release workflow runs it on the Windows runner (`plugins\dfw\bin\dfw.cmd version`).
test('launcher (Windows): dfw.cmd has CRLF lines and the same order: DFW_BIN, this version, newest, 127', () => {
  const text = fs.readFileSync(path.join(REPO, 'core', 'bin', 'dfw.cmd'), 'utf8');
  assert.ok(!/[^\r]\n/.test(text), 'every line ends with CRLF (labels and calls break on LF in cmd.exe)');
  const at = (s) => {
    const i = text.indexOf(s);
    assert.ok(i >= 0, s);
    return i;
  };
  assert.ok(at('if defined DFW_BIN') < at('%HOME_DIR%\\bin\\%VER%\\dfw.exe') && at('%HOME_DIR%\\bin\\%VER%\\dfw.exe') < at('call :newer') && at('call :newer') < at('exit /b 127'));
  assert.match(text, /set "DFW_PLUGIN_ROOT=%ROOT%"/);
  assert.match(text, /\.config\\dfw/);
  assert.doesNotMatch(text, /node /i, 'no Node fallback');
  // Delayed expansion would drop every "!" from the arguments (%*): it stays off, and no
  // !VAR! is read anywhere.
  assert.match(text, /setlocal EnableExtensions DisableDelayedExpansion/);
  assert.doesNotMatch(text, /EnableDelayedExpansion/);
  assert.doesNotMatch(text, /!\w+!/);
});
