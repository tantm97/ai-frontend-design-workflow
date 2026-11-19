// The behaviour changes of release 0.5.2 (the node-stable unit tests and the config part of the
// impeccable-hardening unit tests of that release), recorded from the Node CLI of 0.5.2.
// The letters match the defect list of that release.
// Run: node --test tests/golden/
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { run, assertSame, skipReason, MINIMAL_STATE, NOW, suite } from './harness.mjs';

const test = suite('align');

// A new review session is named after the real time to the second, so its id differs per run.
const maskSessions = (text) => text.replace(/rv_\d{8}T\d{6}/g, 'rv_<now>');

/** The binary gives the recorded exit code, stdout, stderr and files. Returns its result. */
function same(args, opts = {}) {
  const r = run(args, { normalize: maskSessions, ...opts });
  assertSame(r, { stderr: true });
  return r.native;
}

const write = (root, rel, data) => {
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof data === 'string' ? data : JSON.stringify(data, null, 2) + '\n');
};
const st = (over = {}) => structuredClone({ ...MINIMAL_STATE, ...over });
const GATE = { approvedAt: NOW, note: 'ok' };
const FROZEN = st({
  phase: 'stack',
  design: { version: 1, status: 'approved', locked: true, approved: [{ version: 'v1', hash: 'abc', at: NOW }] },
  gates: { G1: GATE, DS: GATE, G2: { ...GATE, version: 'v1', hash: 'abc' }, G3: null, G4: null },
});
const session = (comments) => (root) =>
  write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments });
const comment = (id, status) => ({
  id,
  createdAt: NOW,
  designVersion: 'v1',
  screen: null,
  variant: null,
  breakpoint: null,
  state: null,
  theme: null,
  file: null,
  target: {},
  comment: 'x',
  screenshot: null,
  status,
  thread: [],
  resolution: null,
  provider: 'dfw-studio',
});

describe('align: node 0.5.2 behaviour', { skip: skipReason() }, () => {
  test('b, m: a flag that takes a value is refused without one, before anything runs', () => {
    const cases = [
      ['decide', 'use', 'tabs', '--level'],
      ['comments', 'wait', '--timeout'],
      ['status', '--root'],
      ['studio', 'status', '--port'],
      ['approve', 'G1', '--note'],
      ['reopen', '--reason'],
      ['comments', 'reply', 'fb_001', '--message'],
      ['lang', 'set', 'vi', '--session'],
      ['comments', 'list', '--status'],
      ['review', 'start', '--url'],
      ['install', '--host'],
      ['nope', '--root', '--level'],
      ['help', '--max'],
    ];
    for (const args of cases) {
      const r = same(args);
      assert.equal(r.code, 1, args.join(' '));
      assert.match(r.stderr, /^--[\w-]+ needs a value\.\n$/, args.join(' '));
      assert.deepEqual(r.files, {});
    }
    assert.equal(same(['decide', '--root', '--level']).stderr, '--root needs a value.\n', 'the first one of the list wins');
    assert.equal(same(['status', '--json', '--root', '.']).code, 0, 'a value, and flags without one, still work');
    assert.equal(same(['comments', 'list', '--status=open']).code, 0);
  });

  test('l: a hook checks its flags inside, and fails open', () => {
    for (const args of [
      ['hook', 'session-start', '--host'],
      ['hook', 'pre-tool-use', '--root'],
    ]) {
      const r = same(args, { stdin: '{}' });
      assert.equal(r.code, 0);
      assert.equal(r.stdout, '');
      assert.match(r.stderr, /^dfw hook [a-z-]+ error: --\w+ needs a value\.\n$/);
    }
    assert.equal(same(['hook', 'nope', '--host']).code, 1, 'an unknown hook is still an error');
  });

  test('l, o: a payload that is not an object is an empty one; only a string names a tool', () => {
    for (const stdin of ['null', '[]', '"text"', '5']) assert.match(same(['hook', 'session-start'], { stdin }).stdout, /phase=discovery/);
    for (const tool of [['Write'], { name: 'Write' }, 5, true]) {
      for (const key of ['tool_name', 'toolName']) {
        same(['hook', 'pre-tool-use'], { stdin: JSON.stringify({ [key]: tool, tool_input: { file_path: 'src/a.ts' } }) });
      }
    }
  });

  test('c: phase --force never writes a missing or unknown phase', () => {
    for (const args of [
      ['phase', '--force'],
      ['phase', 'bogus', '--force'],
      ['phase', 'nowhere', '--force', '--reason', 'x'],
    ]) {
      const r = same(args);
      assert.equal(r.code, 1);
      assert.match(r.stderr, /Unknown phase "(undefined|bogus|nowhere)"/);
      assert.deepEqual(r.files, {});
    }
    assert.equal(same(['phase', 'concepts', '--force', '--reason', 'test']).code, 0, '--force still overrides gate blockers');
  });

  test('d: the write gate applies to a file whose name starts with two dots', () => {
    for (const file of ['..x.ts', '..x/y.ts', '...', '../x.ts', '..']) {
      same(['hook', 'pre-tool-use'], { stdin: JSON.stringify({ tool_name: 'Write', tool_input: { file_path: file } }) });
    }
    const denied = same(['hook', 'pre-tool-use'], { stdin: JSON.stringify({ tool_name: 'Write', tool_input: { file_path: '..x.ts' } }) });
    assert.match(denied.stdout, /"permissionDecision":"deny"/);
  });

  test('e: a comment file or snapshot version from input never leaves design/', () => {
    for (const file of [
      '../outside.html',
      'screens/../../outside.html',
      'screens\\..\\..\\outside.html',
      '/etc/hosts',
      'C:\\outside.html',
      { a: 1 },
      5,
    ]) {
      const r = same(['comments', 'add', '--json', JSON.stringify({ comment: 'x', file, target: { dfwId: 'items.title' } })]);
      assert.equal(r.code, 0);
      const c = JSON.parse(r.stdout);
      assert.deepEqual([c.file, c.target.source], [null, null], JSON.stringify(file));
    }
    const snap = (root) => write(root, 'snap/approval.lock.json', { files: [] });
    for (const v of ['../../snap', 'v', '1', 'v1/../x']) {
      const r = same(['verify-snapshot', v], { setup: snap });
      assert.equal(r.code, 1);
      assert.match(r.stderr, /^Unknown snapshot ".*"\. A snapshot is named v<number>, for example v1\.\n$/);
    }
  });

  test('g, j, n: config set needs a value, refuses reserved names and keys through a list', () => {
    const list = (root) => write(root, '.dfw/config.json', { gate: { allow: ['src/**'] } });
    for (const args of [
      ['config', 'set'],
      ['config', 'set', 'a.b'],
      ['__proto__.x', '1'],
      ['a.constructor', '1'],
      ['a.prototype.b', '1'],
      ['gate.allow.0.b', '5'],
      ['gate.allow.extra', '5'],
    ]) {
      const argv = args[0] === 'config' ? args : ['config', 'set', ...args];
      const r = same(argv, { setup: list });
      assert.equal(r.code, 1, argv.join(' '));
      assert.deepEqual(r.files, {});
    }
    for (const key of ['toString', 'studio.constructor', 'studio.breakpoints.length', 'studio.breakpoints.0.id', 'studio.breakpoints.01'])
      same(['config', 'get', key]);
    same(['config', 'get'], {
      setup: (root) =>
        write(root, '.dfw/config.local.json', '{"__proto__":{"polluted":"yes"},"constructor":{"x":1},"studio":{"__proto__":{"port":1}}}'),
    });
  });

  test('k: a config file that is not a JSON object is a clear error', () => {
    for (const content of ['[1,2]', '"text"', '5', 'null', 'true']) {
      for (const args of [['config', 'get', 'studio.port'], ['config', 'set', 'x', '1'], ['status']]) {
        const r = same(args, { setup: (root) => write(root, '.dfw/config.json', content) });
        assert.equal(r.code, args[0] === 'status' ? 0 : 1, `${args.join(' ')} ${content}`);
      }
    }
  });

  test('config: project layers cannot set engine or loader variables; one warning per dropped key', () => {
    const env = {
      IMPECCABLE_BIN: '/repo/evil',
      DO_NOT_TRACK: '0',
      IMPECCABLE_NO_UPDATE_CHECK: null,
      PATH: '/repo/bin',
      NO_COLOR: '1',
      TZ: 'UTC',
      IMPECCABLE_NO_TELEMETRY: '1',
    };
    const r = same(['config', 'get', 'env'], {
      setup: (root) => (
        write(root, '.dfw/config.json', { env }),
        write(root, '.dfw/config.local.json', { env: { IMPECCABLE_HOOK_DISABLED: '0', LANG: 'vi_VN' } })
      ),
    });
    assert.deepEqual(JSON.parse(r.stdout), {
      DO_NOT_TRACK: '1',
      IMPECCABLE_NO_UPDATE_CHECK: '1',
      IMPECCABLE_HOOK_DISABLED: '1',
      NO_COLOR: '1',
      TZ: 'UTC',
      IMPECCABLE_NO_TELEMETRY: '1',
      LANG: 'vi_VN',
    });
    assert.equal(r.stderr.split('\n').filter(Boolean).length, 5);
    for (const value of ['null', '[1]', '"x"'])
      same(['config', 'get', 'env'], { setup: (root) => write(root, '.dfw/config.json', `{"env":${value}}`) });
    // The global config may set anything.
    same(['config', 'get', 'env'], {
      env: { DFW_HOME: '.dfw/home' },
      setup: (root) => write(root, '.dfw/home/config.json', { env: { DO_NOT_TRACK: '0', PATH: '/x' } }),
    });
  });

  test('h: approve G3 refuses a stack.json that is not an object', () => {
    for (const content of ['[1,2]', '"next"', '5', 'null']) {
      const r = same(['approve', 'G3', '--note', 'ok'], { state: FROZEN, setup: (root) => write(root, '.dfw/stack.json', content) });
      assert.equal(r.code, 1);
      assert.deepEqual(r.files, {});
    }
    assert.equal(
      same(['approve', 'G3', '--note', 'ok'], {
        state: FROZEN,
        setup: (root) => write(root, '.dfw/stack.json', { framework: { name: 'x' } }),
      }).code,
      0,
    );
  });

  test('i: setting the status a comment already has is refused', () => {
    const setup = session([comment('fb_001', 'open'), comment('fb_002', 'acknowledged'), comment('fb_003', 'resolved')]);
    for (const args of [
      ['ack', 'fb_002'],
      ['ack', 'fb_002', '--message', 'again'],
      ['resolve', 'fb_003', '--by', 'user'],
      ['reply', 'fb_002', '--message', 'a reply still works'],
    ]) {
      same(['comments', ...args], { setup });
    }
  });

  test('j: names that are prototype keys are not ids', () => {
    for (const name of ['constructor', 'toString', '__proto__', 'hasOwnProperty']) {
      assert.equal(same(['mode', name]).code, 1);
      assert.equal(same(['approve', name, '--note', 'x']).code, 1);
      assert.equal(same(['hook', name], { stdin: '{"block":"x"}' }).code, 1);
      assert.equal(same(['init', '--mode', name], { state: null }).code, 1);
    }
    // A user's mistake: the message alone (a plain Error, with "Error: ", in Node 0.5.2).
    const r = run(['mcp', 'install', 'constructor', '--print']);
    assertSame(r);
    assert.match(r.native.stderr, /^Unknown tool "constructor"\. MCP install targets: /);
    assert.match(r.expected.stderr, /^Unknown tool "constructor"\. MCP install targets: /);
  });

  test('p: a Studio pid that is not a positive integer is never signalled', () => {
    for (const pid of [-1, 0, -5, 1.5, '12', true, null]) {
      const r = same(['status', '--json'], { setup: (root) => write(root, '.dfw/studio.json', { pid, url: 'http://127.0.0.1:4600/' }) });
      assert.equal(JSON.parse(r.stdout).studio, null, String(pid));
    }
  });

  test('presence: .dfw/presence.json is ignored by git in a new project; ids in comments are skipped', () => {
    const r = same(['init'], { state: null });
    assert.match(r.files['.dfw/.gitignore'], /^presence\.json$/m);
    const html = `<main data-dfw-id="page">\n<!-- <section data-dfw-id="old.hero"> removed -->\n<header data-dfw-id='nav'><a>Home</a></header>\n<section data-dfw-id='hero'><h1>Title</h1></section>\n<!-- <p data-dfw-id="never.closed">\n</main>\n`;
    const file = 'design/screens/items/presence.html';
    for (const old of ["<section data-dfw-id='hero'><h1>Title</h1>", '<a>Home</a>', '</main>']) {
      const out = same(['hook', 'pre-tool-use'], {
        state: st({ phase: 'concepts', gates: { ...MINIMAL_STATE.gates, G1: GATE, DS: GATE } }),
        setup: (root) => write(root, file, html),
        get stdin() {
          return JSON.stringify({ tool_name: 'Edit', tool_input: { file_path: file, old_string: old, new_string: 'x' } });
        },
      });
      assert.match(out.files['.dfw/presence.json'], /"dfwId": "(hero|nav)"/);
    }
  });
});
