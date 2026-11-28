// install.sh with a fake curl serving a fake release from a folder: nothing is downloaded and
// the "binary" it installs and hands over to only prints its arguments (a shell script; on Windows
// Git's own echo.exe, since Git Bash runs a file named .exe only when it is a real program). HOME
// and DFW_HOME are temporary folders. The Windows path (Git Bash) runs twice: on Windows with the real uname, and
// on macOS and Linux with a fake uname that answers as Git Bash does. CI runs this file on
// windows-latest under Git Bash (.github/workflows/validate.yml, job native).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const WIN32 = 'win32';
const onWindows = process.platform === WIN32;
const VERSION = '9.9.9';
/** What Git Bash answers to `uname -s` and `uname -m` on 64-bit Windows. */
const GIT_BASH_UNAME = { s: 'MINGW64_NT-10.0-26100', m: 'x86_64' };
/** The second line of the script install.sh writes on Windows (install.sh MARK). */
const MARK = '# dfw: written by install.sh, replaced by the next install';
const FAKE_SCRIPT = '#!/bin/sh\necho "fake dfw $*"\n';

/**
 * Git Bash's sh.exe on Windows, from the folder that also holds the tools install.sh calls (Git's
 * usr\bin, on PATH in a CI step with `shell: bash`), else /bin/sh.
 */
function findSh() {
  if (!onWindows) return '/bin/sh';
  for (const dir of (process.env.PATH || '').split(path.delimiter)) {
    const sh = path.join(dir, 'sh.exe');
    if (dir && ['sh.exe', 'uname.exe', 'echo.exe'].every((f) => fs.existsSync(path.join(dir, f)))) return sh;
  }
  return null;
}
const SH = findSh();
// A skip says why; CI (which sets CI) never skips (testing.md T7).
const runnable = { skip: SH || process.env.CI ? false : 'no sh.exe on PATH: run this file from Git Bash' };
/** The fake release binary, and what it prints when run with `args`. */
const FAKE_BINARY = onWindows && SH ? fs.readFileSync(path.join(path.dirname(SH), 'echo.exe')) : Buffer.from(FAKE_SCRIPT);
const printed = (args) => (onWindows ? `${args}\n` : `fake dfw ${args}\n`);
const posixOnly = { skip: onWindows ? 'symbolic links: install.sh writes a script on Windows instead (tested below)' : false };

/** A path the shell scripts can hold as text: forward slashes (Git Bash reads C:/x as C:\x). */
const shPath = (p) => p.replaceAll('\\', '/');

/**
 * @param {{ os?: string }} [opts] `win32` answers `uname` as Git Bash does on macOS and Linux; on
 *   Windows the real uname runs.
 */
function fixture({ os: target = process.platform } = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-install-sh-')));
  const release = path.join(root, 'release');
  const fakebin = path.join(root, 'fakebin');
  const home = path.join(root, 'home');
  for (const d of [release, fakebin, home]) fs.mkdirSync(d, { recursive: true });
  const win = target === WIN32;
  const arch = win && !onWindows ? 'x64' : { arm64: 'arm64', x64: 'x64' }[process.arch];
  const ext = win ? '.exe' : '';
  const name = `dfw-${VERSION}-${target}-${arch}${ext}`;
  fs.writeFileSync(path.join(release, name), FAKE_BINARY);
  const sums = `${crypto.createHash('sha256').update(FAKE_BINARY).digest('hex')}  ${name}\n`;
  fs.writeFileSync(path.join(release, 'SHA256SUMS'), sums);
  const script = (file, body) => fs.writeFileSync(path.join(fakebin, file), `#!/bin/sh\n${body}`, { mode: 0o755 });
  // curl -fsSL <url>: the file of that name from the release folder.
  script('curl', `for a; do u=$a; done\ncat "${shPath(release)}/$(basename "$u")"\n`);
  if (win && !onWindows) script('uname', `case "$1" in -s) echo ${GIT_BASH_UNAME.s} ;; -m) echo ${GIT_BASH_UNAME.m} ;; esac\n`);
  const dfwHome = path.join(home, '.config', 'dfw');
  const link = path.join(home, '.local', 'bin', 'dfw');
  const dest = path.join(dfwHome, 'bin', VERSION, `dfw${ext}`);
  const sysPath = onWindows ? path.dirname(/** @type {string} */ (SH)) : '/usr/bin:/bin';
  const env = {
    PATH: `${fakebin}${path.delimiter}${sysPath}`,
    HOME: home,
    DFW_VERSION: VERSION,
    ...(onWindows ? { SYSTEMROOT: process.env.SYSTEMROOT || '', TEMP: os.tmpdir(), TMP: os.tmpdir() } : {}),
  };
  const sh = (args) => spawnSync(/** @type {string} */ (SH), args, { encoding: 'utf8', env });
  const run = () => sh([path.join(REPO, 'install.sh'), '--no-runtime']);
  return { root, home, release, name, dfwHome, link, dest, run, sh };
}

const hashOf = (file) => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex');

for (const target of onWindows ? [WIN32] : [process.platform, WIN32]) {
  const label = target === WIN32 ? `Windows (Git Bash${onWindows ? '' : ', faked uname'})` : target;

  test(`install.sh on ${label}: picks the release asset, checks it, installs it, hands over to dfw install`, runnable, () => {
    const { dest, name, run } = fixture({ os: target });
    const r = run();
    assert.equal(r.status, 0, r.stderr);
    if (target === WIN32) assert.match(name, /^dfw-9\.9\.9-win32-x64\.exe$/);
    assert.match(r.stdout, new RegExp(`› dfw 9\\.9\\.9 for ${target}-(x64|arm64)\\n`));
    assert.match(r.stdout, target === WIN32 ? /installed .*\/bin\/9\.9\.9\/dfw\.exe\n/ : /installed .*\/bin\/9\.9\.9\/dfw\n/);
    assert.equal(hashOf(dest), crypto.createHash('sha256').update(FAKE_BINARY).digest('hex'));
    assert.ok(r.stdout.endsWith(printed('install --no-runtime')), r.stdout);
  });

  test(`install.sh on ${label}: a checksum that does not match installs nothing`, runnable, () => {
    const { release, name, dest, run } = fixture({ os: target });
    fs.appendFileSync(path.join(release, name), 'tampered\n');
    const r = run();
    assert.equal(r.status, 1);
    assert.match(r.stderr, new RegExp(`checksum mismatch for ${name.replaceAll('.', '\\.')}; nothing installed`));
    assert.equal(fs.existsSync(dest), false);
  });

  test(`install.sh on ${label}: a release without this platform's binary installs nothing`, runnable, () => {
    const { release, dest, run } = fixture({ os: target });
    fs.writeFileSync(path.join(release, 'SHA256SUMS'), `${'0'.repeat(64)}  dfw-${VERSION}-other-x64\n`);
    const r = run();
    assert.equal(r.status, 1);
    assert.match(r.stderr, /release v9\.9\.9 has no binary for /);
    assert.equal(fs.existsSync(dest), false);
  });
}

test('install.sh: links ~/.local/bin/dfw when it is missing, then hands over to dfw install', posixOnly, () => {
  const { link, dest, run } = fixture();
  const r = run();
  assert.equal(r.status, 0, r.stderr);
  assert.equal(fs.readlinkSync(link), dest);
  assert.match(r.stdout, /installed .*\/bin\/9\.9\.9\/dfw\n/);
  assert.match(r.stdout, /fake dfw install --no-runtime\n$/);
});

test('install.sh: replaces its own link (into the dfw home, or broken), never a dfw of the user', posixOnly, () => {
  for (const kind of ['old-version', 'broken']) {
    const { dfwHome, link, dest, run } = fixture();
    fs.mkdirSync(path.dirname(link), { recursive: true });
    const old = kind === 'broken' ? path.join(path.dirname(link), 'gone') : path.join(dfwHome, 'bin', '0.5.2', 'dfw');
    if (kind !== 'broken') {
      fs.mkdirSync(path.dirname(old), { recursive: true });
      fs.writeFileSync(old, '#!/bin/sh\n', { mode: 0o755 });
    }
    fs.symlinkSync(old, link);
    assert.equal(run().status, 0, kind);
    assert.equal(fs.readlinkSync(link), dest, kind);
  }
  for (const kind of ['file', 'link-elsewhere']) {
    const { home, link, run } = fixture();
    fs.mkdirSync(path.dirname(link), { recursive: true });
    const mine = path.join(home, 'my-dfw');
    fs.writeFileSync(mine, '#!/bin/sh\necho mine\n', { mode: 0o755 });
    if (kind === 'file') fs.copyFileSync(mine, link);
    else fs.symlinkSync(mine, link);
    const r = run();
    assert.equal(r.status, 0, kind);
    assert.match(r.stdout, /\.local\/bin\/dfw is not a dfw link; left as it is\. To use this dfw from a shell: ln -sf /, kind);
    if (kind === 'file') assert.equal(fs.readFileSync(link, 'utf8'), '#!/bin/sh\necho mine\n');
    else assert.equal(fs.readlinkSync(link), mine);
  }
});

test('install.sh on Windows: ~/.local/bin/dfw is a script of its own that runs the installed binary', runnable, () => {
  const { link, sh, run } = fixture({ os: WIN32 });
  assert.equal(run().status, 0);
  const text = fs.readFileSync(link, 'utf8');
  assert.equal(text.split('\n')[1], MARK);
  assert.match(text, /exec '.*\/bin\/9\.9\.9\/dfw\.exe' "\$@"\n$/);
  const r = sh([link, 'version', 'a b']);
  assert.equal(r.status, 0, r.stderr);
  assert.equal(r.stdout, printed('version a b'));
});

test('install.sh on Windows: replaces its own script on an upgrade, never a dfw of the user', runnable, () => {
  const { link, run } = fixture({ os: WIN32 });
  fs.mkdirSync(path.dirname(link), { recursive: true });
  fs.writeFileSync(link, `#!/bin/sh\n${MARK}\nexec '/old/bin/0.5.2/dfw.exe' "$@"\n`);
  assert.equal(run().status, 0);
  assert.match(fs.readFileSync(link, 'utf8'), /\/bin\/9\.9\.9\/dfw\.exe/);

  const user = fixture({ os: WIN32 });
  fs.mkdirSync(path.dirname(user.link), { recursive: true });
  fs.writeFileSync(user.link, '#!/bin/sh\necho mine\n');
  const r = user.run();
  assert.equal(r.status, 0);
  assert.match(r.stdout, /\.local\/bin\/dfw is not dfw's; left as it is\. To use this dfw from a shell: add .*\/bin\/9\.9\.9 to PATH/);
  assert.equal(fs.readFileSync(user.link, 'utf8'), '#!/bin/sh\necho mine\n');
});
