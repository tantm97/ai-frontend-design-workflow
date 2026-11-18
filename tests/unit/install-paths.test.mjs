// Install/packaging paths that run without network: the sh launcher's fallback, Windows spawn
// quoting, the marketplace re-add decision, the upstream bot's guards and the "leftover" check.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { spawnPlan, marketplaceAction } from '../../core/lib/install.mjs';
import { leftoverCheck } from '../../scripts/lib/checks.mjs';
import { blockedTitle, proposeOutcome, issueExists, findReport } from '../../scripts/ci-upstream.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const tmp = (p) => fs.mkdtempSync(path.join(os.tmpdir(), p));

/** A plugin dir holding the real launcher at version 9.9.9, plus a DFW_HOME with fake binaries. */
function launcherFixture(versions) {
  const root = tmp('dfw-launch-');
  const plugin = path.join(root, 'plugin');
  fs.mkdirSync(path.join(plugin, 'bin'), { recursive: true });
  fs.mkdirSync(path.join(plugin, '.claude-plugin'));
  fs.copyFileSync(path.join(REPO, 'core', 'bin', 'dfw'), path.join(plugin, 'bin', 'dfw'));
  fs.chmodSync(path.join(plugin, 'bin', 'dfw'), 0o755);
  fs.writeFileSync(path.join(plugin, '.claude-plugin', 'plugin.json'), '{\n  "name": "dfw",\n  "version": "9.9.9"\n}\n');
  const home = path.join(root, 'home');
  fs.mkdirSync(path.join(home, 'bin'), { recursive: true });
  for (const v of versions) {
    fs.mkdirSync(path.join(home, 'bin', v));
    fs.writeFileSync(path.join(home, 'bin', v, 'dfw'), `#!/bin/sh\necho "fake ${v} root=$DFW_PLUGIN_ROOT args=$*"\n`, { mode: 0o755 });
  }
  // PATH without node: only the tools the launcher itself needs.
  const run = (env = {}) => spawnSync('/bin/sh', [path.join(plugin, 'bin', 'dfw'), 'version'], { encoding: 'utf8', env: { PATH: '/usr/bin:/bin', HOME: root, DFW_HOME: home, ...env } });
  return { plugin, run };
}

const posix = { skip: process.platform === 'win32' || spawnSync('/bin/sh', ['-c', 'PATH=/usr/bin:/bin command -v node']).status === 0 };

test('launcher: exact-version binary runs silently', posix, () => {
  const { plugin, run } = launcherFixture(['0.5.1', '9.9.9']);
  const r = run();
  assert.equal(r.status, 0);
  const m = /^fake 9\.9\.9 root=(.+) args=version$/.exec(r.stdout.trim());
  assert.ok(m, r.stdout);
  assert.equal(fs.realpathSync(m[1]), fs.realpathSync(plugin));
  assert.equal(r.stderr, '');
});

test('launcher: no node and no exact binary -> newest installed binary, one stderr line', posix, () => {
  const { run } = launcherFixture(['0.5.1', '0.10.0', '0.9.3']);
  const r = run();
  assert.equal(r.status, 0);
  assert.match(r.stdout, /^fake 0\.10\.0 /); // numeric order, not 0.9.3
  assert.equal(r.stderr.trim().split('\n').length, 1);
  assert.match(r.stderr, /no binary for plugin version 9\.9\.9 .* running binary 0\.10\.0/);
});

test('launcher: nothing to run -> 127; DFW_NO_BINARY never falls back', posix, () => {
  assert.equal(launcherFixture([]).run().status, 127);
  const r = launcherFixture(['0.5.1']).run({ DFW_NO_BINARY: '1' });
  assert.equal(r.status, 127);
  assert.equal(r.stdout, '');
});

test('install: Windows spawn plan quotes shell arguments and runs absolute executables directly', () => {
  assert.deepEqual(spawnPlan('C:\\Program Files\\nodejs\\node.exe', ['C:\\p\\dfw.mjs', 'doctor'], 'win32'), { bin: 'C:\\Program Files\\nodejs\\node.exe', args: ['C:\\p\\dfw.mjs', 'doctor'], shell: false });
  const p = spawnPlan('claude', ['plugin', 'marketplace', 'add', 'C:\\Users\\John Doe\\repo', '--scope', 'user'], 'win32');
  assert.equal(p.shell, true);
  assert.deepEqual(p.args, ['plugin', 'marketplace', 'add', '"C:\\Users\\John Doe\\repo"', '--scope', 'user']);
  assert.deepEqual(spawnPlan('claude', ['a&calc'], 'win32').args, ['"a&calc"']);
  assert.deepEqual(spawnPlan('claude', ['a b'], 'linux'), { bin: 'claude', args: ['a b'], shell: false });
});

test('install: marketplace is re-added only when registered from another source', () => {
  const list = JSON.stringify([{ name: 'other', source: 'github', repo: 'a/b' }, { name: 'dfw-marketplace', source: 'github', repo: 'tantm97/ai-frontend-design-workflow' }]);
  assert.equal(marketplaceAction(list, 'tantm97/ai-frontend-design-workflow'), 'update');
  assert.equal(marketplaceAction(list, 'https://github.com/tantm97/ai-frontend-design-workflow.git'), 'update');
  assert.equal(marketplaceAction(list, './my-clone'), 'readd');
  assert.equal(marketplaceAction('[]', './my-clone'), 'add');
  const local = JSON.stringify([{ name: 'dfw-marketplace', source: 'directory', path: path.resolve('my-clone') }]);
  assert.equal(marketplaceAction(local, './my-clone'), 'update');
  assert.equal(marketplaceAction(local, 'tantm97/ai-frontend-design-workflow'), 'readd');
  // Older Claude Code without --json: the text listing.
  assert.equal(marketplaceAction('Configured marketplaces:\n  dfw-marketplace\n', 'x'), 'update');
  assert.equal(marketplaceAction('Configured marketplaces:\n', 'x'), 'add');
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

test('adapters: every host plugin is assembled from the one shared core copy list', () => {
  const core = JSON.parse(fs.readFileSync(path.join(REPO, 'adapters', 'core-copy.json'), 'utf8')).copy;
  assert.ok(core.some((c) => c.from === 'core/lib') && core.every((c) => c.from.startsWith('core/')));
  for (const a of ['claude-code', 'copilot']) {
    const spec = JSON.parse(fs.readFileSync(path.join(REPO, 'adapters', a, 'assemble.json'), 'utf8'));
    assert.deepEqual(spec.copy, [{ from: `adapters/${a}/plugin`, to: '.' }]);
  }
});
