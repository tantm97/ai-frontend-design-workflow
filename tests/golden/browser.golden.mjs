// Browser and tools groups of the binary: inspector, doctor, install, uninstall, setup, diff and
// shoot, compared with the results the Node CLI of 0.5.2 recorded in tests/golden/browser/.
// Run: node --test tests/golden/
//
// Safety: install, uninstall and doctor only ever see fake `claude` / `copilot` programs (both
// always present, first on a PATH that holds nothing else but /usr/bin:/bin) and a temporary
// HOME and DFW_HOME. Nothing is downloaded: install runs with --no-runtime --no-binary.
// The tests that need a real browser run only with DFW_TEST_CHROME=1.
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import crypto from 'node:crypto';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import zlib from 'node:zlib';
import net from 'node:net';
import { spawn, spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import {
  REPO,
  NOW,
  MINIMAL_STATE,
  nativeBinary,
  skipReason,
  assertSame,
  run as runGolden,
  maskVersion,
  suite,
  expected,
} from './harness.mjs';

const test = suite('browser');
const FIX = path.join(REPO, 'tests', 'fixtures', 'inspector');
const CHROME = process.env.DFW_TEST_CHROME === '1' ? false : 'needs Chrome: set DFW_TEST_CHROME=1';
const posix = process.platform === 'win32' ? 'fake host CLIs are sh scripts' : false;

/** Every file under dir as { relative path: Buffer }; symlinks as "-> target". */
function tree(dir) {
  const files = new Map();
  const walk = (abs) => {
    if (!fs.existsSync(abs)) return;
    for (const ent of fs.readdirSync(abs, { withFileTypes: true })) {
      const p = path.join(abs, ent.name);
      if (ent.isSymbolicLink()) files.set(path.relative(dir, p).split(path.sep).join('/'), Buffer.from(`-> ${fs.readlinkSync(p)}`));
      else if (ent.isDirectory()) walk(p);
      else if (ent.isFile()) files.set(path.relative(dir, p).split(path.sep).join('/'), fs.readFileSync(p));
    }
  };
  walk(dir);
  return files;
}

/**
 * Like harness.run, for commands that write outside .dfw/ and design/: every file of the project
 * copy and of the temporary home is collected, and `after(ctx)` can collect more (its return
 * value lands in `.extra`). args: a list, or (root, tmp) => list. opts: project, state (null =
 * none), setup(root, tmp), env(tmp), normalize, after({ root, tmp, normalize }), images (true: a
 * written PNG is kept by name only; the recorded bytes were written by another encoder).
 */
function run(args, opts = {}) {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-browser-')));
  const root = path.join(tmp, 'project');
  const home = path.join(tmp, 'home');
  try {
    if (opts.project) fs.cpSync(opts.project, root, { recursive: true });
    else fs.mkdirSync(root, { recursive: true });
    fs.mkdirSync(path.join(tmp, 'tmp'), { recursive: true });
    fs.mkdirSync(home, { recursive: true });
    if (opts.state !== null && opts.state !== undefined) {
      fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
      fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify(opts.state, null, 2) + '\n');
    }
    opts.setup?.(root, tmp);
    const before = new Map([...tree(root)].map(([k, v]) => [`project/${k}`, v]).concat([...tree(home)].map(([k, v]) => [`home/${k}`, v])));
    const env = { ...process.env };
    for (const k of [
      'DFW_NO_BINARY',
      'DFW_VERSION',
      'DFW_HOST',
      'XDG_CONFIG_HOME',
      'CLAUDE_CONFIG_DIR',
      'COPILOT_HOME',
      'CODEX_HOME',
      'IMPECCABLE_BIN',
      'IMPECCABLE_HOME',
    ])
      delete env[k];
    Object.assign(env, {
      HOME: home,
      DFW_HOME: path.join(home, 'dfw-home'),
      DFW_NOW: NOW,
      DFW_PROJECT_DIR: root,
      CLAUDE_PROJECT_DIR: root,
      DFW_PLUGIN_ROOT: path.join(REPO, 'core'),
      TMPDIR: path.join(tmp, 'tmp'),
      // Where a browser on Linux keeps its own caches and settings (fontconfig, crash reports,
      // the NSS database): outside the recorded home, so only what dfw writes is compared.
      XDG_CACHE_HOME: path.join(tmp, 'xdg', 'cache'),
      XDG_DATA_HOME: path.join(tmp, 'xdg', 'data'),
      ...(opts.env ? opts.env(tmp) : {}),
    });
    const list = typeof args === 'function' ? args(root, tmp) : args;
    const r = spawnSync(nativeBinary(), list, { cwd: root, env, input: '', encoding: 'utf8', timeout: opts.timeout ?? 120_000 });
    if (r.error) throw r.error;
    const escaped = (p) => JSON.stringify(p).slice(1, -1);
    const normalize = (text) => {
      // How dfw starts itself again ("  $ <dfw> doctor").
      let t = text.split(nativeBinary()).join('<dfw>');
      for (const [from, to] of [
        [root, '<root>'],
        [tmp, '<tmp>'],
        [REPO, '<repo>'],
      ])
        t = t.split(escaped(from)).join(to).split(from).join(to);
      t = maskVersion(t);
      return opts.normalize ? opts.normalize(t) : t;
    };
    const printable = (buf) =>
      Buffer.from(buf.toString('utf8'), 'utf8').equals(buf) ? normalize(buf.toString('utf8')) : `base64:${buf.toString('base64')}`;
    const now = new Map([...tree(root)].map(([k, v]) => [`project/${k}`, v]).concat([...tree(home)].map(([k, v]) => [`home/${k}`, v])));
    const files = {};
    for (const [rel, buf] of now)
      if (!before.get(rel)?.equals(buf)) files[rel] = opts.images && rel.endsWith('.png') ? '(image)' : printable(buf);
    for (const rel of before.keys()) if (!now.has(rel)) files[rel] = null;
    const extra = opts.after ? opts.after({ root, tmp, normalize }) : null;
    const leftovers = fs.readdirSync(path.join(tmp, 'tmp')).filter((f) => f.startsWith('dfw-browser-'));
    return {
      code: r.status,
      stdout: normalize(r.stdout),
      stderr: normalize(r.stderr),
      files: Object.fromEntries(Object.entries(files).sort(([a], [b]) => (a < b ? -1 : 1))),
      extra,
      leftovers,
    };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

// The doctor of the binary (also printed by install) has one row the recorded Node doctor has
// not: telemetry, right after the global config dir. It is checked here and removed, so
// everything else compares as is. Returns { native }.
const TELEMETRY_ROW = /^( *)(ok +global config dir - [^\n]*\n)\1ok +telemetry - [^\n]*\n/m;
function run2(args, opts) {
  const native = run(args, opts);
  if (/ok +global config dir - /.test(native.stdout)) {
    assert.match(native.stdout, TELEMETRY_ROW, 'the doctor has the telemetry row');
    native.stdout = native.stdout.replace(TELEMETRY_ROW, '$1$2');
  }
  return { native };
}

/**
 * What a case compares of one run, as plain JSON: exit code, stdout, the files written, the
 * collected extras and, when asked, stderr (true: all of it; a function: the part compared).
 */
function seen(r, { stderr = false } = {}) {
  return {
    code: r.code,
    stdout: r.stdout,
    files: r.files,
    extra: r.extra,
    ...(stderr ? { stderr: stderr === true ? r.stderr : stderr(r.stderr) } : {}),
  };
}

/** The run is the recorded one, as `seen` shows it. Returns the run. */
function same(r, o = {}) {
  const now = seen(r.native, o);
  assert.deepEqual(now, expected(now));
  return r.native;
}

/** The first line of stderr: the recorded Node CLI printed a stack after it for a plain Error. */
const firstLine = (s) => s.split('\n')[0];
const firstTwoLines = (s) => s.split('\n').slice(0, 2).join('\n');

const G3 = {
  ...MINIMAL_STATE,
  mode: 'existing-project',
  phase: 'implement',
  gates: { ...MINIMAL_STATE.gates, G3: { at: NOW, note: 'ok' } },
};
const approvedStack = (root) => {
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(
    path.join(root, '.dfw', 'stack.json'),
    JSON.stringify({ kind: 'javascript', dependencies: [{ name: 'code-inspector-plugin', version: '2.0.9', dev: true }] }, null, 2) + '\n',
  );
};

describe('tools: inspector', { skip: skipReason() }, () => {
  const SHAPES = fs.readdirSync(FIX).sort();

  test('inspector setup (dry): the plan as text and as JSON, for every bundler shape', () => {
    assert.deepEqual(SHAPES, ['next-webpack', 'next16', 'nuxt', 'vite', 'vite-noplugins', 'webpack']);
    for (const shape of SHAPES) {
      for (const args of [['inspector', 'setup'], ['inspector'], ['inspector', 'setup', '--json']]) {
        const r = same(run2(args, { project: path.join(FIX, shape), state: null }), { stderr: true });
        assert.equal(r.code, 0, shape);
        assert.deepEqual(r.files, {}, `${shape}: a dry run writes nothing`);
      }
    }
  });

  test('inspector setup (dry): other project shapes', () => {
    const shapes = {
      'no package.json': () => {},
      'static html': (root) => fs.writeFileSync(path.join(root, 'index.html'), '<p>hi</p>'),
      'another language': (root) => fs.writeFileSync(path.join(root, 'go.mod'), 'module x\n'),
      'config file only (rspack)': (root) => fs.writeFileSync(path.join(root, 'rspack.config.ts'), 'export default {};\n'),
      'vite without a config file, pnpm': (root) => {
        fs.writeFileSync(path.join(root, 'package.json'), '{"devDependencies":{"vite":"7.1.0","vue":"3.5.0","typescript":"5.9.3"}}');
        fs.writeFileSync(path.join(root, 'pnpm-lock.yaml'), '');
      },
      'next 15.2 with --turbo, yarn': (root) => {
        fs.writeFileSync(
          path.join(root, 'package.json'),
          '{"scripts":{"dev":"next dev --turbo"},"dependencies":{"next":"^15.2.4","react":"19.0.0"}}',
        );
        fs.writeFileSync(path.join(root, 'next.config.mjs'), 'export default {};\n');
        fs.writeFileSync(path.join(root, 'yarn.lock'), '');
      },
      'next 15.3 with --turbopack and a turbopack key': (root) => {
        fs.writeFileSync(path.join(root, 'package.json'), '{"scripts":{"dev":"next dev --turbopack"},"dependencies":{"next":"15.3.1"}}');
        fs.writeFileSync(path.join(root, 'next.config.ts'), 'export default { turbopack: { root: "." } };\n');
      },
      'next without a config file': (root) => fs.writeFileSync(path.join(root, 'package.json'), '{"dependencies":{"next":"14.0.0"}}'),
      'esbuild, bun': (root) => {
        fs.writeFileSync(path.join(root, 'package.json'), '{"devDependencies":{"esbuild":"0.25.0","preact":"10.0.0"}}');
        fs.writeFileSync(path.join(root, 'bun.lock'), '');
      },
      'unknown bundler': (root) => fs.writeFileSync(path.join(root, 'package.json'), '{"dependencies":{"lit":"3.0.0"}}'),
      'vite already configured': (root) => {
        fs.writeFileSync(path.join(root, 'package.json'), '{"devDependencies":{"vite":"7.1.0","code-inspector-plugin":"2.0.9"}}');
        fs.writeFileSync(
          path.join(root, 'vite.config.mjs'),
          "import { codeInspectorPlugin } from 'code-inspector-plugin';\nexport default { plugins: [codeInspectorPlugin({ bundler: 'vite' })] };\n",
        );
      },
      'vite config function with a body': (root) => {
        fs.writeFileSync(path.join(root, 'package.json'), '{"devDependencies":{"vite":"7.1.0"}}');
        fs.writeFileSync(
          path.join(root, 'vite.config.ts'),
          "import { defineConfig } from 'vite';\nexport default defineConfig(({ mode }) => {\n  return { plugins: [] };\n});\n",
        );
      },
      'webpack CommonJS with one plugin per line, CRLF': (root) => {
        fs.writeFileSync(path.join(root, 'package.json'), '{"devDependencies":{"webpack":"5.0.0"}}');
        fs.writeFileSync(
          path.join(root, 'webpack.config.js'),
          '"use strict"\r\nconst A = require("a"); // plugins: [x]\r\nmodule.exports = {\r\n  resolve: { plugins: [new B()] },\r\n  plugins: [\r\n    new A(),\r\n  ],\r\n};\r\n',
        );
      },
    };
    for (const [name, setup] of Object.entries(shapes)) {
      for (const args of [
        ['inspector', 'setup'],
        ['inspector', 'setup', '--json'],
      ]) {
        const r = same(run2(args, { state: null, setup }), { stderr: true });
        assert.equal(r.code, 0, name);
      }
    }
  });

  test('inspector setup --write: refused without G3 and the approved dependency', () => {
    const vite = path.join(FIX, 'vite');
    for (const opts of [{ state: null }, { state: MINIMAL_STATE }, { state: G3 }, { state: MINIMAL_STATE, setup: approvedStack }]) {
      const r = same(run2(['inspector', 'setup', '--write'], { project: vite, ...opts }), { stderr: true });
      assert.equal(r.code, 1);
      assert.match(r.stderr, /^dfw inspector setup --write refused: /);
      assert.deepEqual(r.files, {});
    }
    const usage = same(run2(['inspector', 'remove'], { project: vite, state: null }), { stderr: true });
    assert.equal(usage.stderr, 'Usage: dfw inspector setup [--write] [--json]\n');
  });

  test('inspector setup --write: config, backup and package.json are written the same, for every shape', () => {
    const written = {};
    for (const shape of SHAPES) {
      for (const json of [[], ['--json']]) {
        const r = same(
          run2(['inspector', 'setup', '--write', ...json], { project: path.join(FIX, shape), state: G3, setup: approvedStack }),
          { stderr: true },
        );
        written[shape] = Object.keys(r.files);
        assert.equal(r.code, shape === 'nuxt' ? 1 : 0, shape);
      }
    }
    assert.deepEqual(written.vite, ['project/package.json', 'project/vite.config.ts', 'project/vite.config.ts.dfw-backup']);
    assert.deepEqual(written['next16'], ['project/next.config.ts', 'project/next.config.ts.dfw-backup', 'project/package.json']);
    assert.deepEqual(written.nuxt, [], 'a config dfw does not edit: nothing is written');
  });

  test('inspector setup --write: second run, tab indentation, an existing backup', () => {
    const again = (root) => {
      approvedStack(root);
      fs.writeFileSync(
        path.join(root, 'package.json'),
        '{\n\t"name": "x",\n\t"version": "1.0.0",\n\t"devDependencies": {\n\t\t"vite": "7.1.0",\n\t\t"code-inspector-plugin": "1.0.0"\n\t},\n\t"count": 1e21\n}',
      );
      fs.writeFileSync(path.join(root, 'vite.config.ts.dfw-backup'), 'the first original\n');
    };
    const r = same(run2(['inspector', 'setup', '--write'], { project: path.join(FIX, 'vite'), state: G3, setup: again }), { stderr: true });
    assert.deepEqual(Object.keys(r.files), ['project/package.json', 'project/vite.config.ts']);
    assert.match(r.files['project/package.json'], /^\{\n\t"name": "x",/);
    const done = (root) => {
      approvedStack(root);
      fs.writeFileSync(path.join(root, 'package.json'), '{"devDependencies":{"vite":"7.1.0","code-inspector-plugin":"2.0.9"}}');
      fs.writeFileSync(
        path.join(root, 'vite.config.ts'),
        "import { codeInspectorPlugin } from 'code-inspector-plugin';\nexport default { plugins: [codeInspectorPlugin({ bundler: 'vite' })] };\n",
      );
    };
    const second = same(run2(['inspector', 'setup', '--write'], { project: path.join(FIX, 'vite'), state: G3, setup: done }), {
      stderr: true,
    });
    assert.equal(second.stdout, 'code-inspector-plugin@2.0.9 is already set up.\n');
    assert.deepEqual(second.files, {});
  });

  test('parseInspPath (through __debug insp-path)', () => {
    const cases = [
      ['src/components/Foo.tsx:42:7:div'],
      ['src/App.vue:3:5:el-button'],
      ['src/App.tsx:3:5'],
      ['src/App.tsx:3:Header'],
      ['./src/a.jsx:1:1:p'],
      ['C:\\work\\shop\\src\\components\\Foo.tsx:42:7:div'],
      ['C:\\work\\shop\\src\\components\\Foo.tsx:42:7:div', 'C:\\work\\shop'],
      ['C:/work/shop/src/Foo.tsx:9:1:span', 'C:\\work\\shop'],
      ['c:/Work/shop/src/Foo.tsx:9:1:span', 'C:/work/SHOP/'],
      ['D:/work/shop/src/Foo.tsx:9:1:span', 'C:\\work\\shop'],
      ['/home/me/shop/src/Foo.tsx:42:7:div', '/home/me/shop'],
      ['/home/me/shop/src/Foo.tsx:42:7:div', '/home/me/shop/'],
      ['/home/me/shop/../other/x.tsx:1:2:a', '/home/me/shop'],
      ['/elsewhere/lib/x.tsx:1:2:a', '/home/me/shop'],
      ['/home/me/shop:1:2:a', '/home/me/shop'],
      ['/home/me/shop/..x/y.ts:1', '/home/me/shop'],
      ['  src/a.ts:12:3:a  '],
      ['src/a.ts:007:08:b'],
      ['a:b:c:1:2:3:x'],
      ['src/a.ts:1:2:3'],
      ['src/Foo.tsx'],
      ['div'],
      [':1:2'],
      ['   '],
      ['x:99999999999999999999999:1:p'],
    ];
    for (const [value, root] of cases) {
      const r = spawnSync(nativeBinary(), ['__debug', 'insp-path', value, ...(root ? [root] : [])], { encoding: 'utf8' });
      assert.equal(r.status, 0, r.stderr);
      assert.equal(r.stdout, expected(r.stdout), `${value} (root ${root})`);
    }
  });
});

/* ---------------------------------------------------------------- fake host CLIs */

const FAKE_CLI = `#!/bin/sh
name=$(basename "$0")
echo "$name $*" >> "$FAKE_LOG"
if [ "$*" = "--version" ]; then
  [ "$FAKE_MISSING" = "$name" ] && exit 127
  echo "9.9.9 (fake $name)"
  echo "second line"
  exit 0
fi
if [ "$*" = "plugin marketplace list --json" ]; then
  [ -n "$FAKE_NO_JSON" ] && { echo "unknown option --json" >&2; exit 1; }
  printf '%s\\n' "$FAKE_LIST"
  exit 0
fi
if [ "$*" = "plugin list --json" ] && [ -n "$FAKE_PLUGINS" ]; then
  printf '%s\\n' "$FAKE_PLUGINS"
  exit 0
fi
if [ "$*" = "plugin marketplace list" ]; then
  printf '%s\\n' "$FAKE_LIST"
  exit 0
fi
if [ -n "$FAKE_FAIL" ]; then
  case "$*" in *"$FAKE_FAIL"*) echo "boom: $*" >&2; exit 1 ;; esac
fi
echo "done: $*"
`;

/**
 * The machine's own tools the doctor reports, made the same on every machine: python3 prints the
 * version the recording was made with, and the browser is a file named by DFW_BROWSER (the doctor
 * only looks for it), so neither the runner's Python nor where its Chrome lives reaches the output.
 */
const FAKE_PYTHON = '#!/bin/sh\necho "Python 3.9.6"\n';

/** Options for run2: the fake CLIs first on a PATH without anything else of the user's. */
function withFakes({ env = {}, setup, ...rest } = {}) {
  for (const dir of ['/usr/bin', '/bin'])
    for (const cli of ['claude', 'copilot'])
      assert.ok(!fs.existsSync(path.join(dir, cli)), `a real ${cli} in ${dir} could be started: refusing to run`);
  return {
    state: null,
    ...rest,
    setup(root, tmp) {
      const bin = path.join(tmp, 'fakebin');
      fs.mkdirSync(bin);
      for (const cli of ['claude', 'copilot']) fs.writeFileSync(path.join(bin, cli), FAKE_CLI, { mode: 0o755 });
      fs.writeFileSync(path.join(bin, 'python3'), FAKE_PYTHON, { mode: 0o755 });
      fs.writeFileSync(path.join(bin, 'chrome'), '');
      setup?.(root, tmp);
    },
    env: (tmp) => ({
      PATH: `${path.join(tmp, 'fakebin')}:/usr/bin:/bin`,
      DFW_BROWSER: path.join(tmp, 'fakebin', 'chrome'),
      CHROME_PATH: '',
      FAKE_LOG: path.join(tmp, 'calls.log'),
      FAKE_LIST: '[]',
      ...env,
    }),
    // What the binary says about itself (the recorded Node CLI said "on Node <version>").
    normalize: (t) => t.replace(/· dfw binary/, '· dfw <runtime>').replace(/^(\s*ok {3}dfw runtime) - .*$/m, '$1 - <runtime>'),
    after: ({ tmp, normalize }) => ({
      calls: fs.existsSync(path.join(tmp, 'calls.log')) ? normalize(fs.readFileSync(path.join(tmp, 'calls.log'), 'utf8')) : '',
    }),
  };
}

const INSTALL = ['install', '--no-runtime', '--no-binary'];

describe('tools: install, uninstall, doctor (fake host CLIs)', { skip: skipReason() || posix }, () => {
  test('install: both hosts, the default source', () => {
    const r = same(run2(INSTALL, withFakes()));
    assert.equal(r.code, 0, r.stderr);
    const calls = r.extra.calls.trim().split('\n');
    assert.deepEqual(calls.slice(0, 5), [
      'claude --version',
      'copilot --version',
      'claude plugin marketplace list --json',
      'claude plugin marketplace add tantm97/ai-frontend-design-workflow --scope user',
      'claude plugin install dfw@dfw-marketplace --scope user',
    ]);
    assert.ok(calls.includes('copilot plugin install dfw@dfw-marketplace'));
    assert.ok(!calls.some((c) => c.startsWith('copilot plugin marketplace remove')), 'the default source is not removed first');
    assert.deepEqual(calls.slice(-2), ['claude --version', 'copilot --version'], 'doctor ran last');
    assert.match(r.stdout, /Hosts: Claude Code, GitHub Copilot · dfw <runtime>/);
    assert.match(r.stdout, /\n {2}ok {3}dfw runtime - <runtime>\n/);
    // eslint-disable-next-line no-control-regex -- the installer colours its output (ESC [0m)
    assert.match(r.stdout, /Done\.\x1b\[0m Restart Claude Code .* Restart GitHub Copilot /);
    // Copilot does not put the plugin's bin/ on PATH: the launcher is linked into ~/.local/bin.
    assert.equal(r.files['home/.local/bin/dfw'], '-> <tmp>/home/.copilot/installed-plugins/dfw-marketplace/dfw/bin/dfw');
  });

  test('install: marketplace already registered (update), from another source (re-add), old CLI without --json', () => {
    const repo = '[{"name":"dfw-marketplace","source":"github","repo":"tantm97/ai-frontend-design-workflow"}]';
    const update = same(run2([...INSTALL, '--host', 'claude'], withFakes({ env: { FAKE_LIST: repo } })));
    assert.match(update.extra.calls, /claude plugin marketplace update dfw-marketplace\n/);
    assert.doesNotMatch(update.extra.calls, /marketplace add|copilot plugin/);

    const readd = same(
      run2(
        [...INSTALL, '--host', 'all', '--source', './my clone', '--scope', 'project', '--quiet'],
        withFakes({ env: { FAKE_LIST: repo } }),
      ),
    );
    assert.match(
      readd.extra.calls,
      /claude plugin marketplace remove dfw-marketplace\nclaude plugin marketplace add \.\/my clone --scope project\n/,
    );
    assert.match(readd.extra.calls, /copilot plugin marketplace remove dfw-marketplace\ncopilot plugin marketplace add \.\/my clone\n/);
    assert.doesNotMatch(readd.stdout, /\$ claude/, '--quiet hides the commands');

    const old = same(
      run2(
        [...INSTALL, '--host', 'claude'],
        withFakes({ env: { FAKE_NO_JSON: '1', FAKE_LIST: 'Configured marketplaces:\n  dfw-marketplace\n' } }),
      ),
    );
    assert.match(old.extra.calls, /list --json\nclaude plugin marketplace list\nclaude plugin marketplace update dfw-marketplace\n/);
  });

  test('install: a host that is not installed, a failing skill, a failing dfw plugin', () => {
    const auto = same(run2(INSTALL, withFakes({ env: { FAKE_MISSING: 'copilot' } })));
    assert.equal(auto.code, 0);
    assert.match(auto.stdout, /Hosts: Claude Code · dfw <runtime>/);
    assert.match(auto.stdout, /host GitHub Copilot - "copilot" not found \(skip, or install it and run "dfw install --host copilot"\)/);

    for (const args of [
      [...INSTALL, '--host', 'copilot'],
      [...INSTALL, '--host', 'all'],
      [...INSTALL, '--host', 'cursor'],
      [...INSTALL, '--host', 'nope'],
      ['uninstall', '--host', 'copilot'],
    ]) {
      const r = run2(args, withFakes({ env: { FAKE_MISSING: 'copilot' } }));
      same(r, { stderr: firstLine });
      assert.equal(r.native.code, 1);
    }
    const none = run2(
      INSTALL,
      withFakes({
        env: { FAKE_MISSING: 'copilot', FAKE_LIST: '[]' },
        setup: (root, tmp) =>
          fs.writeFileSync(path.join(tmp, 'fakebin', 'claude'), '#!/bin/sh\necho "claude $*" >> "$FAKE_LOG"\nexit 1\n', { mode: 0o755 }),
      }),
    );
    same(none, { stderr: firstLine });
    assert.equal(
      firstLine(none.native.stderr),
      'No supported AI coding tool found on PATH (claude, copilot). Install Claude Code or GitHub Copilot CLI first, or pass --host.',
    );

    // A skill that does not install is skipped; dfw itself failing stops the install.
    const skill = same(run2(INSTALL, withFakes({ env: { FAKE_FAIL: 'install impeccable@' } })));
    assert.equal(skill.code, 0);
    assert.match(skill.extra.calls, /copilot plugin install impeccable@dfw-marketplace\ncopilot plugin update impeccable\n/);
    const dfw = run2([...INSTALL, '--host', 'claude'], withFakes({ env: { FAKE_FAIL: 'install dfw@' } }));
    same(dfw, { stderr: firstTwoLines });
    assert.equal(dfw.native.code, 1);
    assert.equal(
      dfw.native.stderr.split('\n').slice(0, 2).join('\n'),
      'Error: claude plugin install dfw@dfw-marketplace --scope user failed:\nboom: plugin install dfw@dfw-marketplace --scope user',
    );
  });

  test('install: ~/.local/bin/dfw is left alone when it is a file or links to another working dfw', () => {
    const link = (make) =>
      same(
        run2(
          [...INSTALL, '--host', 'copilot', '--quiet'],
          withFakes({
            setup: (root, tmp) => {
              fs.mkdirSync(path.join(tmp, 'home', '.local', 'bin'), { recursive: true });
              make(path.join(tmp, 'home', '.local', 'bin', 'dfw'), tmp);
            },
          }),
        ),
      ).files['home/.local/bin/dfw'];
    assert.equal(
      link((file) => fs.writeFileSync(file, 'a real file')),
      undefined,
    );
    assert.equal(
      link((file, tmp) => {
        fs.writeFileSync(path.join(tmp, 'other-dfw'), '');
        fs.symlinkSync(path.join(tmp, 'other-dfw'), file);
      }),
      undefined,
    );
    // A Node bug the binary keeps for now: fs.rmSync(force) does not remove a link whose target is gone, so it is not replaced either.
    assert.equal(
      link((file, tmp) => fs.symlinkSync(path.join(tmp, 'gone'), file)),
      undefined,
    );
    // A link to the launcher itself is written again (same target: no change to see).
    assert.equal(
      link((file, tmp) => {
        const launcher = path.join(tmp, 'home', '.copilot', 'installed-plugins', 'dfw-marketplace', 'dfw', 'bin', 'dfw');
        fs.mkdirSync(path.dirname(launcher), { recursive: true });
        fs.writeFileSync(launcher, '');
        fs.symlinkSync(launcher, file);
      }),
      undefined,
    );
  });

  test('install: a Copilot dfw of the two-folder layout is uninstalled, then installed, and the user is told', () => {
    const dfwDir = (tmp) => path.join(tmp, 'home', '.copilot', 'installed-plugins', 'dfw-marketplace', 'dfw');
    const layout = (entries) => (root, tmp) => {
      for (const e of entries) {
        const f = path.join(dfwDir(tmp), e);
        fs.mkdirSync(path.dirname(f), { recursive: true });
        if (e.endsWith('/')) fs.mkdirSync(f, { recursive: true });
        else fs.writeFileSync(f, '{}');
      }
    };
    const copilot = [...INSTALL, '--host', 'copilot'];
    // The installed folder still has the root hooks.json and agents/ of plugins/dfw-copilot.
    const old = same(run2(copilot, withFakes({ setup: layout(['plugin.json', 'hooks.json', 'agents/', 'skills/dfw/', 'bin/dfw']) })));
    assert.equal(old.code, 0, old.stderr);
    assert.match(
      old.extra.calls,
      /^copilot --version\ncopilot plugin list --json\ncopilot plugin uninstall dfw\ncopilot plugin marketplace add tantm97\/ai-frontend-design-workflow\n/,
    );
    assert.match(old.extra.calls, /\ncopilot plugin install dfw@dfw-marketplace\n/);
    assert.match(
      old.stdout,
      /\nReplaced the old dfw for GitHub Copilot \(<tmp>\/home\/\.copilot\/installed-plugins\/dfw-marketplace\/dfw\): two-folder layout\.\n/,
    );
    // The doctor at the end still sees the fake (unchanged) folder and names the fix.
    assert.match(
      old.stdout,
      /WARN host GitHub Copilot - 9\.9\.9 \(fake copilot\); dfw plugin installed with the old two-folder layout \(run dfw install --host copilot to replace it\)/,
    );

    // Installed from a dfw-copilot folder of a local clone: found in the plugin listing.
    const listed = JSON.stringify([
      { name: 'dfw', path: '/clone/plugins/dfw-copilot' },
      { name: 'impeccable', path: '/x/_direct/y' },
    ]);
    const byPath = same(run2(copilot, withFakes({ env: { FAKE_PLUGINS: listed }, setup: layout(['plugin.json', 'hosts/', 'bin/dfw']) })));
    assert.match(byPath.extra.calls, /copilot plugin list --json\ncopilot plugin uninstall dfw\n/);
    assert.match(byPath.stdout, /\nReplaced the old dfw for GitHub Copilot \(\/clone\/plugins\/dfw-copilot\): two-folder layout\.\n/);
    assert.match(byPath.stdout, /ok {3}host GitHub Copilot - 9\.9\.9 \(fake copilot\); dfw plugin installed\n/);

    // Already on the one folder: installed as before, nothing replaced.
    const current = JSON.stringify([{ name: 'dfw', path: '<home>/.copilot/installed-plugins/dfw-marketplace/dfw' }]);
    const fine = same(run2(copilot, withFakes({ env: { FAKE_PLUGINS: current }, setup: layout(['plugin.json', 'hosts/', 'bin/dfw']) })));
    assert.doesNotMatch(fine.extra.calls, /uninstall/);
    assert.doesNotMatch(fine.stdout, /Replaced/);

    // The uninstall fails: the run stops with the host's message, nothing is installed.
    const failed = run2(copilot, withFakes({ env: { FAKE_FAIL: 'uninstall dfw' }, setup: layout(['hooks.json']) }));
    same(failed, { stderr: firstTwoLines });
    assert.equal(failed.native.code, 1);
    assert.equal(
      failed.native.stderr.split('\n').slice(0, 2).join('\n'),
      'Error: copilot plugin uninstall dfw failed:\nboom: plugin uninstall dfw',
    );
    assert.doesNotMatch(failed.native.stdout, /Replaced|plugin install/);
  });

  test('uninstall: every plugin and the marketplace, per host', () => {
    const r = same(run2(['uninstall'], withFakes()));
    assert.equal(r.code, 0);
    assert.match(r.extra.calls, /claude plugin uninstall dfw@dfw-marketplace --scope user\n/);
    assert.match(r.extra.calls, /copilot plugin uninstall dfw\n/);
    assert.match(r.extra.calls, /copilot plugin marketplace remove dfw-marketplace\n$/);
    // eslint-disable-next-line no-control-regex -- the installer colours its output (ESC [0m)
    assert.match(r.stdout, /GitHub Copilot: removed dfw and its skills\.\x1b\[0m\nProject files \(\.dfw\/, design\/\) are untouched\.\n$/);
    same(run2(['uninstall', '--host', 'claude', '--scope', 'local', '--quiet'], withFakes({ env: { FAKE_FAIL: 'uninstall' } })));
  });

  test('doctor: a bare machine, and one with plugins, an approved snapshot and MCP configs', () => {
    const bare = same(run2(['doctor'], withFakes()), { stderr: true });
    assert.equal(bare.code, 0);
    assert.match(bare.stdout, /^ok {3}dfw runtime - <runtime>\n/);
    assert.match(bare.stdout, /\nok {3}project workflow - not initialized here\n/);
    assert.match(bare.stdout, /\nWARN host Claude Code - 9\.9\.9 \(fake claude\); dfw plugin missing \(dfw install --host claude\)\n/);
    assert.match(bare.stdout, /\nok {3}global config dir - <tmp>\/home\/dfw-home\n$/);

    const state = { ...MINIMAL_STATE, design: { version: 2, status: 'draft', locked: false, approved: [{ version: 'v1', at: NOW }] } };
    const full = same(
      run2(
        ['doctor'],
        withFakes({
          state,
          setup: (root, tmp) => {
            const home = path.join(tmp, 'home');
            const write = (file, text) => (fs.mkdirSync(path.dirname(file), { recursive: true }), fs.writeFileSync(file, text));
            write(
              path.join(home, '.claude', 'plugins', 'installed_plugins.json'),
              JSON.stringify({
                plugins: {
                  'dfw@dfw-marketplace': [{}],
                  'impeccable@elsewhere': [{}],
                  'hallmark@dfw-marketplace': [{}],
                  'taste@other': [{}],
                  'unrelated@x': [{}],
                },
              }),
            );
            write(path.join(home, '.copilot', 'installed-plugins', 'dfw-marketplace', 'dfw', 'bin', 'dfw'), '');
            write(path.join(home, '.codex', 'config.toml'), 'model = "x"\r\n\r\n[mcp_servers.dfw]\r\ncommand = "dfw"\r\n');
            write(path.join(home, '.cursor', 'mcp.json'), '{"mcpServers":{"dfw":{"command":"dfw"}}}');
            write(path.join(home, '.config', 'opencode', 'opencode.json'), '{"mcp":{}}');
            const approved = path.join(root, 'design', 'approved', 'v1');
            write(path.join(approved, 'kept.html'), 'kept');
            write(path.join(approved, 'changed.html'), 'now');
            write(path.join(approved, 'added.css'), 'x');
            const sha = (s) => crypto.createHash('sha256').update(s).digest('hex');
            write(
              path.join(approved, 'approval.lock.json'),
              JSON.stringify({
                files: [
                  { path: 'kept.html', sha256: sha('kept') },
                  { path: 'changed.html', sha256: sha('before') },
                  { path: 'gone.html', sha256: sha('gone') },
                ],
              }),
            );
          },
        }),
      ),
      { stderr: true },
    );
    assert.match(
      full.stdout,
      /\nWARN no duplicate upstream installs - also installed outside dfw-marketplace: impeccable@elsewhere, taste@other \(skills would load twice; consider disabling them\)\n/,
    );
    assert.match(
      full.stdout,
      /\nok {3}project workflow - new-product\/discovery\nWARN approved snapshot v1 intact - modified: changed\.html; missing: gone\.html; added: added\.css\n/,
    );
    assert.match(
      full.stdout,
      /\nok {3}host Claude Code - 9\.9\.9 \(fake claude\); dfw plugin installed\nok {3}host GitHub Copilot - 9\.9\.9 \(fake copilot\); dfw plugin installed\n/,
    );
    assert.match(
      full.stdout,
      /\nok {3}host OpenCode - plugin planned; use "dfw mcp install opencode"\nok {3}host OpenAI Codex - plugin planned; dfw MCP server configured\nok {3}host Cursor - plugin planned; dfw MCP server configured\n/,
    );

    // A snapshot without its lock stops doctor.
    const noLock = run2(['doctor'], withFakes({ state }));
    same(noLock, { stderr: true });
    assert.equal(noLock.native.code, 1);
    assert.match(noLock.native.stderr, /^No approval\.lock\.json in <root>\/design\/approved\/v1\n$/);
  });

  test('setup: the marketplace repo is recorded; no flag is a usage error', () => {
    const r = same(run2(['setup', '--marketplace-repo', 'some/../clone'], withFakes()), { stderr: true });
    assert.equal(r.stdout, 'Recorded marketplace repo in <tmp>/home/dfw-home/config.json\n');
    assert.equal(r.files['home/dfw-home/config.json'], '{\n  "marketplaceRepo": "<root>/clone"\n}\n');
    const usage = same(run2(['setup'], withFakes()), { stderr: true });
    assert.equal(usage.stderr, 'Usage: dfw setup --browser | --a11y | --binary | --marketplace-repo <path>\n');
    assert.equal(usage.code, 1);
  });
});

/* ---------------------------------------------------------------- diff and shoot */

/** A PNG file from RGBA bytes (8-bit, no filter), written with Node's zlib: no image library. */
function png(w, h, rgba) {
  const table = Array.from({ length: 256 }, (_, n) => {
    let c = n;
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
    return c >>> 0;
  });
  const crc = (buf) => (buf.reduce((c, b) => table[(c ^ b) & 0xff] ^ (c >>> 8), 0xffffffff) ^ 0xffffffff) >>> 0;
  const chunk = (type, data) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const out = Buffer.alloc(body.length + 8);
    out.writeUInt32BE(data.length, 0);
    body.copy(out, 4);
    out.writeUInt32BE(crc(body), body.length + 4);
    return out;
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(w, 0);
  head.writeUInt32BE(h, 4);
  head.set([8, 6, 0, 0, 0], 8);
  const rows = Buffer.alloc(h * (w * 4 + 1));
  for (let y = 0; y < h; y++) Buffer.from(rgba.buffer, rgba.byteOffset + y * w * 4, w * 4).copy(rows, y * (w * 4 + 1) + 1);
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', head),
    chunk('IDAT', zlib.deflateSync(rows)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

/** An image from a pixel function (x, y) -> [r, g, b, a]. */
function picture(w, h, pixel) {
  const rgba = new Uint8Array(w * h * 4);
  for (let y = 0; y < h; y++) for (let x = 0; x < w; x++) rgba.set(pixel(x, y), (y * w + x) * 4);
  return png(w, h, rgba);
}

/** Deterministic noise in 0..255. */
const noise = (x, y, seed) => (Math.imul((x * 73856093) ^ (y * 19349663) ^ (seed * 83492791), 0x9e3779b1) >>> 24) & 255;

/** Fixture folders a/ and b/ under the project: shapes with anti-aliased edges, colour shifts around the threshold, alpha, noise, sizes. */
function diffFixtures(root) {
  const edge = (shift) => (x, y) => {
    // A soft diagonal edge (three ramp pixels), moved by `shift`.
    const d = x + shift - y;
    const v = d < -1 ? 20 : d > 1 ? 240 : [70, 130, 190][d + 1];
    return [v, v, v, 255];
  };
  const pictures = {
    'same.png': [picture(40, 30, edge(0)), picture(40, 30, edge(0))],
    'edge.png': [picture(40, 30, edge(0)), picture(40, 30, edge(1))],
    'tint.png': [picture(64, 16, () => [200, 180, 160, 255]), picture(64, 16, (x) => [200 - x * 2, 180, 160 + x, 255])],
    'dark.png': [picture(64, 16, (x) => [x, x, x, 255]), picture(64, 16, (x, y) => [x + y * 3, x, x + (x >> 2), 255])],
    'noise.png': [
      picture(48, 48, (x, y) => [noise(x, y, 1), noise(x, y, 2), noise(x, y, 3), 255]),
      picture(48, 48, (x, y) => [noise(x, y, 1) ^ (x > 24 ? 9 : 0), noise(x, y, 2), noise(x, y, 3) ^ (y > 30 ? 40 : 0), 255]),
    ],
    'alpha.png': [
      picture(32, 32, (x, y) => [x * 8, y * 8, 128, 255]),
      picture(32, 32, (x, y) => [x * 8, y * 8, 128, 255 - ((x * y) & 255)]),
    ],
    'alpha-both.png': [picture(32, 8, (x) => [255, 0, 0, x * 8]), picture(32, 8, (x, y) => [0, 0, 255, x * 8 + y])],
    'size.png': [
      picture(20, 10, () => [0, 0, 0, 255]),
      picture(24, 14, (x, y) => (x < 20 && y < 10 ? [0, 0, 0, 255] : [200, 60, 60, 255])),
    ],
    'text.png': [
      picture(60, 20, (x, y) => (noise(x >> 1, y >> 1, 7) > 150 ? [30, 30, 30, 255] : [255, 255, 255, 255])),
      picture(60, 20, (x, y) => (noise((x + 1) >> 1, y >> 1, 7) > 150 ? [30, 30, 30, 255] : [255, 255, 255, 255])),
    ],
  };
  for (const [name, [a, b]] of Object.entries(pictures)) {
    for (const [dir, buf] of [
      ['a', a],
      ['b', b],
    ]) {
      fs.mkdirSync(path.join(root, dir), { recursive: true });
      fs.writeFileSync(path.join(root, dir, name), buf);
    }
  }
  fs.writeFileSync(path.join(root, 'a', 'only-a.png'), pictures['same.png'][0]);
  fs.writeFileSync(path.join(root, 'b', 'notes.txt'), 'not an image');
  return Object.keys(pictures);
}

/** Size and decoded-pixel hash of every PNG under dir (decoded by the binary). */
function pixels(dir) {
  const out = {};
  for (const [rel, buf] of tree(dir)) {
    if (!rel.endsWith('.png')) continue;
    const file = path.join(dir, rel);
    const r = spawnSync(nativeBinary(), ['__debug', 'png-info', file], { encoding: 'utf8' });
    out[rel] = r.status === 0 ? JSON.parse(r.stdout) : { error: r.stderr.trim(), bytes: buf.length };
  }
  return out;
}

describe('browser: diff', { skip: skipReason() }, () => {
  test('usage error', () => {
    for (const args of [['diff'], ['diff', 'a.png']]) {
      const r = runGolden(args);
      assertSame(r, { stderr: true });
      assert.equal(r.native.code, 1);
    }
  });

  test('pixel counts, ratios and diff images equal pixelmatch in the browser', { skip: CHROME }, () => {
    let names;
    const setup = (root) => {
      names = diffFixtures(root);
      fs.mkdirSync(path.join(root, 'empty'));
    };
    const opts = { state: null, images: true, setup, after: ({ root }) => pixels(path.join(root, 'out')) };
    // --json keeps the order the folder was read in: compare by file.
    const byFile = (r) =>
      Object.fromEntries(
        JSON.parse(r.stdout)
          .map((x) => [path.basename(x.b), x])
          .sort(([a], [b]) => (a < b ? -1 : 1)),
      );
    const dirs = run2(['diff', 'a', 'b', '--out', 'out', '--json'], opts);
    assert.equal(dirs.native.code, 0, dirs.native.stderr);
    const dirsSeen = { code: dirs.native.code, byFile: byFile(dirs.native), extra: dirs.native.extra };
    assert.deepEqual(dirsSeen, expected(dirsSeen), 'diff, w, h, ratio, sizeChanged per pair, and the diff images');
    assert.deepEqual(Object.keys(byFile(dirs.native)).sort(), [...names].sort());
    // The diff images: the recorded size and decoded pixels (the PNG bytes are another encoder's).
    assert.equal(Object.keys(dirs.native.extra).length, names.length);
    assert.deepEqual(dirs.native.leftovers, []);
    const nums = byFile(dirs.native);
    assert.equal(nums['same.png'].diff, 0);
    // The fixtures must exercise the comparison: differences found, and some of them only as anti-aliasing or below the threshold.
    console.log(
      `      diff pixels: ${Object.entries(nums)
        .map(([f, n]) => `${f.replace('.png', '')} ${n.diff}`)
        .join(', ')}`,
    );
    for (const changed of ['tint.png', 'dark.png', 'noise.png', 'alpha.png', 'alpha-both.png', 'size.png', 'text.png'])
      assert.ok(nums[changed].diff > 0, `${changed}: ${JSON.stringify(nums[changed])}`);
    assert.equal(nums['size.png'].sizeChanged, '20x10 -> 24x14');

    // Text output: most changed first, default output folder b/_diff.
    const text = run2(['diff', 'a', 'b'], { ...opts, after: ({ root }) => pixels(path.join(root, 'b', '_diff')) });
    const lines = (r) => r.stdout.trim().split('\n');
    // Equal ratios keep the read order: the lines compare as a sorted list, the ratios in order.
    const textSeen = { lines: [...lines(text.native)].sort(), ratios: lines(text.native).map(parseFloat), extra: text.native.extra };
    assert.deepEqual(textSeen, expected(textSeen));
    assert.match(text.native.stdout, /^ +\d+\.\d\d% {2}size\.png {2}\(size 20x10 -> 24x14\) {2}-> b\/_diff\/size\.png$/m);

    // Two files, other thresholds, the default output name, and what cannot be compared.
    for (const args of [
      ['diff', 'a/tint.png', 'b/tint.png', '--threshold', '0.02', '--json'],
      ['diff', 'a/tint.png', 'b/tint.png', '--threshold', '0.3'],
      ['diff', 'a/dark.png', 'b/dark.png', '--threshold', '0.005'],
      ['diff', 'a/noise.png', 'b/noise.png', '--threshold', '0'],
      ['diff', 'a/alpha-both.png', 'b/alpha-both.png', '--out', 'x/y/d.png'],
      ['diff', 'a/edge.png', 'b/missing.png'],
      ['diff', 'a/edge.png', 'b/notes.txt'],
      ['diff', 'a', 'empty'],
      ['diff', 'a', 'nowhere'],
    ]) {
      // The recorded Node CLI printed the page's stack line after "Error: cannot decode image".
      const normalize = (t) => t.replace(/\n {4}at img\.onerror \(<anonymous>:\d+:\d+\)/g, '');
      const r = run2(args, {
        ...opts,
        normalize,
        after: ({ root }) => ({ ...pixels(path.join(root, 'x')), ...pixels(path.join(root, 'b')) }),
      });
      same(r, { stderr: true });
    }
  });
});

describe('browser: shoot', { skip: skipReason() || CHROME }, () => {
  // The browser's settings folder too (crash reports on Linux); dfw's own is DFW_HOME.
  const browserConfig = (tmp) => ({ XDG_CONFIG_HOME: path.join(tmp, 'xdg', 'config') });
  const PAGE = `<!doctype html><html><head><meta charset="utf-8"><style>body{margin:0;background:#fff;font:16px sans-serif}#card{width:200px;height:80px;background:#d03030;margin:20px}.tall{height:1500px;background:linear-gradient(#fff,#ccd)}</style></head>
<body><div id="card">Card</div><div class="tall"></div></body></html>`;
  /** `dfw shoot --url <the local page> ...flags`; .extra is the decoded size of every PNG written. */
  const shoot = (flags, target) => {
    const r = run2((root) => ['shoot', '--url', target || pathToFileURL(path.join(root, 'page.html')).href, ...flags], {
      state: null,
      images: true,
      timeout: 180_000,
      env: browserConfig,
      setup: (root) => fs.writeFileSync(path.join(root, 'page.html'), PAGE),
      after: ({ root }) =>
        Object.fromEntries(Object.entries(pixels(root)).map(([file, info]) => [file, info.error ? info : `${info.width}x${info.height}`])),
    });
    // Compared: exit code, stdout, the names of the files written, and that every PNG decodes
    // to the recorded width and height. Not compared: the pixels (two runs of a browser may
    // differ in anti-aliasing) and the PNG bytes.
    same(r, { stderr: true });
    assert.deepEqual(r.native.leftovers, [], 'no browser profile left behind');
    return r.native;
  };

  test('one file per breakpoint; full page, viewport, element, missing element', () => {
    const all = shoot(['--out', 'shots/page.png']);
    assert.equal(all.code, 0, all.stderr);
    assert.equal(all.stdout, 'ok   shots/page__desktop.png\nok   shots/page__tablet.png\nok   shots/page__mobile.png\n');
    assert.deepEqual(all.extra, {
      'shots/page__desktop.png': '1440x1620',
      'shots/page__tablet.png': '834x1620',
      'shots/page__mobile.png': '390x1620',
    });

    const viewport = shoot(['--bp', 'desktop', '--viewport', '--out', 'view-{bp}.png']);
    assert.deepEqual(viewport.extra, { 'view-desktop.png': '1440x900' });

    const card = shoot(['--bp', 'mobile,desktop', '--selector', '#card', '--out', 'card']);
    assert.deepEqual(card.extra, { 'card__mobile.png': '200x80', 'card__desktop.png': '200x80' });

    const missing = shoot(['--bp', 'desktop', '--selector', '#missing', '--out', 'none.png']);
    assert.equal(missing.code, 2);
    assert.equal(missing.stdout, 'FAIL none.png (selector not found: #missing)\n');
    assert.deepEqual(missing.extra, {});

    const nothing = shoot(['--bp', 'nope']);
    assert.equal(nothing.stderr, 'Nothing to screenshot (check --screen/--variant/--bp).\n');
  });

  test('the default file name comes from the URL path; a page that cannot be opened fails that job', () => {
    const named = shoot(['--bp', 'desktop'], 'data:text/html,<p>hi</p>');
    assert.equal(named.stdout, 'ok   .dfw/shots/url/text_html_p_hi_p___desktop.png\n');
    assert.deepEqual(named.extra, { '.dfw/shots/url/text_html_p_hi_p___desktop.png': '1440x900' });

    const refused = shoot(['--bp', 'desktop', '--out', 'refused.png'], 'http://127.0.0.1:9/nothing-listens-here');
    assert.equal(refused.code, 2);
    assert.match(refused.stdout, /^FAIL refused\.png \(cannot open http:\/\/127\.0\.0\.1:9\/nothing-listens-here: net::ERR_/);
  });

  test('native only: the browser and its profile go when dfw is killed mid-run (SIGTERM, SIGKILL)', async () => {
    // A server that accepts and never answers: the screenshot waits on it until dfw is killed.
    const stall = net.createServer(() => {});
    await new Promise((r) => stall.listen(0, '127.0.0.1', r));
    const until = async (done, ms = 15_000) => {
      for (const end = Date.now() + ms; Date.now() < end;) {
        if (done()) return true;
        await new Promise((r) => setTimeout(r, 50));
      }
      return done();
    };
    try {
      for (const signal of ['SIGTERM', 'SIGKILL']) {
        const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-kill-')));
        const profiles = () => fs.readdirSync(tmp).filter((f) => f.startsWith('dfw-browser-'));
        const browsers = () => spawnSync('pgrep', ['-f', `user-data-dir=${tmp}/dfw-browser-`], { encoding: 'utf8' }).stdout.trim();
        const dfw = spawn(
          nativeBinary(),
          [
            'shoot',
            '--root',
            tmp,
            '--url',
            `http://127.0.0.1:${stall.address().port}/`,
            '--bp',
            'desktop',
            '--out',
            path.join(tmp, 'never.png'),
          ],
          {
            env: { ...process.env, TMPDIR: tmp, DFW_HOME: path.join(tmp, 'home'), DFW_PLUGIN_ROOT: path.join(REPO, 'core') },
            stdio: 'ignore',
          },
        );
        try {
          assert.ok(await until(() => profiles().length === 1 && browsers() !== ''), 'the browser started');
          dfw.kill(signal);
          assert.ok(await until(() => browsers() === ''), `${signal}: browser processes gone`);
          assert.ok(await until(() => profiles().length === 0), `${signal}: profile removed`);
        } finally {
          dfw.kill('SIGKILL');
          await until(() => browsers() === '' && profiles().length === 0);
          fs.rmSync(tmp, { recursive: true, force: true, maxRetries: 5 });
        }
      }
    } finally {
      stall.close();
    }
  });

  test('screens of the design, served by a running Studio', () => {
    const project = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');
    const studio = (root, tmp, sub) => {
      const env = {
        ...process.env,
        DFW_HOME: path.join(tmp, 'home', 'dfw-home'),
        DFW_PLUGIN_ROOT: path.join(REPO, 'core'),
        CLAUDE_PROJECT_DIR: root,
        DFW_NOW: NOW,
      };
      const r = spawnSync(nativeBinary(), ['studio', sub], { cwd: root, env, encoding: 'utf8', timeout: 30_000 });
      assert.equal(r.status, 0, r.stderr);
    };
    for (const flags of [
      [],
      ['--page', 'stock', '--bp', 'mobile', '--viewport'],
      ['--screen', 'items', '--variant', 'a', '--states', '--themes', '--bp', 'desktop'],
      ['--system'],
      ['--selector', 'body', '--bp', 'mobile'],
      ['--screen', 'nope'],
      ['--page', 'nope'],
    ]) {
      const r = run2(['shoot', ...flags], {
        project,
        state: MINIMAL_STATE,
        timeout: 180_000,
        env: browserConfig,
        setup: (root, tmp) => studio(root, tmp, 'start'),
        after: ({ root, tmp }) => {
          studio(root, tmp, 'stop');
          return Object.fromEntries(
            Object.entries(pixels(path.join(root, '.dfw', 'shots'))).map(([file, info]) => [
              file,
              info.error ? info : `${info.width}x${info.height}`,
            ]),
          );
        },
      });
      // Compared: exit code, stdout, stderr, the files written and their decoded sizes.
      const now = { code: r.native.code, stdout: r.native.stdout, stderr: r.native.stderr, extra: r.native.extra };
      const want = expected(now);
      // The Design System page is long text: its full-page height follows the platform's fonts
      // (macOS and Linux differ by a pixel). Its width and every other size compare exactly.
      for (const [file, size] of Object.entries(now.extra)) {
        const [w, h] = String(size).split('x').map(Number);
        const [ww, wh] = String(want.extra?.[file]).split('x').map(Number);
        if (file.startsWith('v1/_system__') && w === ww && Math.abs(h - wh) <= 8) now.extra[file] = want.extra[file];
      }
      assert.deepEqual(now, want, flags.join(' '));
      assert.deepEqual(r.native.leftovers, []);
      if (!flags.length) {
        assert.equal(r.native.stdout, 'ok   .dfw/shots/v1/items__a__desktop.png\nok   .dfw/shots/v1/items__a__mobile.png\n');
        assert.deepEqual(Object.keys(r.native.extra).sort(), ['v1/items__a__desktop.png', 'v1/items__a__mobile.png']);
      }
      if (flags[0] === '--system') assert.equal(r.native.stdout, 'ok   .dfw/shots/v1/_system__desktop.png\n');
      if (flags.includes('nope')) assert.equal(r.native.stderr, 'Nothing to screenshot (check --screen/--variant/--bp).\n');
    }
  });

  test('without the Studio running, screens need --url', () => {
    const r = run2(['shoot'], { project: path.join(REPO, 'tests', 'fixtures', 'penpot', 'project'), state: MINIMAL_STATE });
    same(r, { stderr: true });
    assert.equal(r.native.stderr, 'Start the studio first (dfw studio start) or pass --url.\n');
  });
});
