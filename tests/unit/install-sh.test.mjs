// install.sh with a fake curl serving a fake release from a folder: nothing is downloaded and
// the "binary" it installs and hands over to only prints its arguments. HOME and DFW_HOME are
// temporary folders.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const posix = { skip: process.platform === 'win32' ? 'install.sh runs on macOS and Linux' : false };
const OS = { darwin: 'darwin', linux: 'linux' }[process.platform];
const ARCH = { arm64: 'arm64', x64: 'x64' }[process.arch];

function fixture() {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-install-sh-')));
  const release = path.join(root, 'release');
  const fakebin = path.join(root, 'fakebin');
  const home = path.join(root, 'home');
  for (const d of [release, fakebin, home]) fs.mkdirSync(d, { recursive: true });
  const name = `dfw-9.9.9-${OS}-${ARCH}`;
  const binary = '#!/bin/sh\necho "fake dfw $*"\n';
  fs.writeFileSync(path.join(release, name), binary);
  fs.writeFileSync(path.join(release, 'SHA256SUMS'), `${crypto.createHash('sha256').update(binary).digest('hex')}  ${name}\n`);
  // curl -fsSL <url>: the file of that name from the release folder.
  fs.writeFileSync(path.join(fakebin, 'curl'), `#!/bin/sh\nfor a; do u=$a; done\ncat "${release}/$(basename "$u")"\n`, { mode: 0o755 });
  const dfwHome = path.join(home, '.config', 'dfw');
  const link = path.join(home, '.local', 'bin', 'dfw');
  const dest = path.join(dfwHome, 'bin', '9.9.9', 'dfw');
  const run = () =>
    spawnSync('/bin/sh', [path.join(REPO, 'install.sh'), '--no-runtime'], {
      encoding: 'utf8',
      env: { PATH: `${fakebin}:/usr/bin:/bin`, HOME: home, DFW_VERSION: '9.9.9' },
    });
  return { home, dfwHome, link, dest, run };
}

test('install.sh: links ~/.local/bin/dfw when it is missing, then hands over to dfw install', posix, () => {
  const { link, dest, run } = fixture();
  const r = run();
  assert.equal(r.status, 0, r.stderr);
  assert.equal(fs.readlinkSync(link), dest);
  assert.match(r.stdout, /installed .*\/bin\/9\.9\.9\/dfw\n/);
  assert.match(r.stdout, /fake dfw install --no-runtime\n$/);
});

test('install.sh: replaces its own link (into the dfw home, or broken), never a dfw of the user', posix, () => {
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
