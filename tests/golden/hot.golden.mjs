// Golden tests of the hot path: workflow state and gates (state, gate, freeze, stack, lang),
// review comments (comments, inbox) and the hook handlers, through init, status, mode, phase,
// approve, reopen, pause, resume, decide, decisions, config, lang, verify-snapshot,
// comments <sub> and hook <name>, plus the freeze, stack detection and reviews lock modules through
// the hidden `__debug` command. Only the binary runs; tests/golden/hot/ holds the recorded results.
// Run: node --test tests/golden/
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, spawnSync } from 'node:child_process';
import { run as runBinary, runWith, assertSame, skipReason, nativeBinary, expected, suite, NOW, MINIMAL_STATE, REPO } from './harness.mjs';

const test = suite('hot');

// A new review session is named after the real time to the second, so its id differs per run.
const STAMP = /rv_\d{8}T\d{6}/g;
const maskSessions = (text) => text.replace(STAMP, 'rv_<now>');

/** Run the binary; session ids made by the run are masked, also in file names. */
function run(args, opts = {}) {
  opts.normalize ??= maskSessions; // set on the object itself: `stdin` may be a getter (see hookOpts)
  const r = runBinary(args, opts);
  for (const side of [r.expected, r.native]) side.files = Object.fromEntries(Object.entries(side.files).map(([k, v]) => [maskSessions(k), v]));
  return r;
}

/** The binary does what was recorded: exit code, stdout, stderr and files. Returns its result. */
function same(args, opts) {
  const r = run(args, opts);
  assertSame(r, { stderr: true });
  return r.native;
}

/** What a recorded result holds instead of the stderr a case does not compare. */
const STDERR_NOT_COMPARED = '<not compared>';

/**
 * Same, without stderr: for errors the reference reported with a stack trace or an engine message.
 * The recorded result leaves the stderr out, as the comparison does.
 */
function sameButStderr(args, opts = {}) {
  opts.normalize ??= maskSessions;
  const native = runWith(nativeBinary(), [], args, opts);
  const want = expected({ ...native, stderr: STDERR_NOT_COMPARED });
  assertSame({ expected: want, native });
  return native;
}

// A deep copy, so no case changes a shared state object.
const st = (over = {}) => structuredClone({ ...MINIMAL_STATE, ...over });
const write = (root, rel, data) => {
  const file = path.join(root, rel);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, typeof data === 'string' ? data : JSON.stringify(data, null, 2) + '\n');
};
const all = (...setups) => (root) => setups.forEach((s) => s(root));
/** Prepare a project with one of the binary's hidden `__debug` helpers, at the recorded clock. */
const helper = (args) => {
  const r = spawnSync(nativeBinary(), ['__debug', ...args], { env: { ...process.env, DFW_NOW: NOW, DFW_HOME: path.join(os.tmpdir(), 'dfw-golden-no-home') }, encoding: 'utf8' });
  assert.equal(r.status, 0, `dfw __debug ${args[0]} failed: ${r.stderr}`);
};

const GATE = { approvedAt: NOW, note: 'ok' };
const APPROVED_DESIGN = { version: 1, status: 'approved', locked: true, approved: [{ version: 'v1', hash: 'abc', at: NOW }] };
const FROZEN = st({ phase: 'stack', design: APPROVED_DESIGN, gates: { G1: GATE, DS: GATE, G2: { ...GATE, version: 'v1', hash: 'abc' }, G3: null, G4: null } });
const STACK = { framework: { name: 'Vue' }, status: 'proposed', proposedAt: NOW };

const comment = (id, status = 'open', extra = {}) => ({
  id,
  createdAt: '2026-01-01T00:00:00.000Z',
  designVersion: 'v1',
  screen: 'items',
  variant: 'a',
  breakpoint: null,
  state: null,
  theme: null,
  file: 'design/screens/items/a.html',
  target: { dfwId: 'items.title', source: 'design/screens/items/a.html:18' },
  comment: `Comment ${id} "quoted" tiếng Việt`,
  screenshot: null,
  status,
  thread: [],
  resolution: null,
  provider: 'dfw-studio',
  ...extra,
});
const line = (by, message, status) => ({ at: '2026-01-01T01:00:00.000Z', by, message, ...(status ? { status } : {}) });
/** An open review session `rv_1` (sorts before every real session id) with the given comments. */
const withSession = (comments, extra = {}) => (root) => write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments, ...extra });
const MIXED = [
  comment('fb_001'),
  comment('fb_002', 'acknowledged', { thread: [line('claude', 'On it', 'acknowledged')], updatedAt: '2026-01-01T01:00:00.000Z' }),
  comment('fb_003', 'fixed', { screen: null, variant: null, target: { selector: 'main > p' }, thread: [line('claude', 'Wider now', 'fixed')], resolution: { at: NOW, by: 'claude', note: 'Wider now', evidence: null } }),
  comment('fb_004', 'resolved', { breakpoint: 'mobile', theme: 'dark', state: 'empty' }),
  comment('fb_005', 'reopened', { thread: [line('claude', 'Done', 'fixed'), line('user', 'Still too small', 'reopened')], updatedAt: '2026-01-01T02:00:00.000Z' }),
  comment('fb_006', 'dismissed', { target: {}, provider: 'dfw-live', url: 'http://localhost:3000/items' }),
];

describe('hot: workflow commands', { skip: skipReason() }, () => {
  test('init: modes, product, errors, existing documents', () => {
    const fresh = { state: null };
    const first = same(['init', '--product', 'Fixture'], fresh);
    assert.equal(first.code, 0);
    assert.deepEqual(JSON.parse(first.files['.dfw/state.json']), MINIMAL_STATE);
    assert.deepEqual(Object.keys(first.files), ['.dfw/.gitignore', '.dfw/config.json', '.dfw/decisions.jsonl', '.dfw/state.json']);
    assert.match(first.stdout, /^Initialized dfw \(new-product\) in <root>\nCreated: PRODUCT\.md, DESIGN\.md\n/);
    for (const args of [[], ['--mode', 'quick'], ['--mode', 'design-only', '--product', 'Tên "sản phẩm"'], ['--mode', 'review'], ['--product'], ['--mode=existing', '--product=']]) same(['init', ...args], fresh);
    // Existing documents and config are kept.
    const kept = same(['init'], { state: null, setup: (root) => (write(root, 'PRODUCT.md', '# Mine\n'), write(root, '.dfw/config.json', { schema: 1, studio: { port: 4700 } }), write(root, '.dfw/decisions.jsonl', '{"at":"x"}\n')) });
    assert.match(kept.stdout, /\nCreated: DESIGN\.md\n/);
    assert.ok(!('.dfw/config.json' in kept.files) && !('.dfw/decisions.jsonl' in kept.files));
    for (const args of [['--mode', 'implement'], ['--mode', 'nope'], ['--mode']]) assert.equal(same(['init', ...args], fresh).code, 1);
    const again = same(['init']);
    assert.equal(again.code, 1);
    assert.match(again.stderr, /^dfw is already initialized here \(<root>.*state\.json\)\. Use "dfw status"\.\n$/);
    assert.deepEqual(again.files, {});
  });

  test('init without design/: the design template and the token build', () => {
    const r = same(['init', '--product', 'New'], { state: null, setup: (root) => fs.rmSync(path.join(root, 'design'), { recursive: true }) });
    assert.equal(r.code, 0);
    assert.ok('design/tokens/tokens.css' in r.files);
  });

  test('status: text and JSON in every phase', () => {
    const states = {
      none: null,
      discovery: st(),
      direction: st({ phase: 'direction' }),
      directionApproved: st({ phase: 'direction', gates: { ...MINIMAL_STATE.gates, G1: GATE } }),
      systemApproved: st({ phase: 'system', gates: { ...MINIMAL_STATE.gates, G1: GATE, DS: GATE } }),
      concepts: st({ phase: 'concepts', mode: 'quick-change', waived: { G1: 'waived by mode quick-change', DS: 'waived by mode quick-change' } }),
      conceptsApproved: { ...FROZEN, phase: 'concepts' },
      stack: FROZEN,
      stackApproved: { ...FROZEN, gates: { ...FROZEN.gates, G3: GATE } },
      implement: { ...FROZEN, phase: 'implement', gates: { ...FROZEN.gates, G3: GATE } },
      verify: { ...FROZEN, phase: 'verify' },
      audit: st({ mode: 'review-only', phase: 'audit' }),
      done: { ...FROZEN, phase: 'done', product: '' },
      odd: st({ phase: 'somewhere', product: 0, design: { version: 2.5, status: 'draft', locked: 1, approved: [] } }),
    };
    for (const [name, state] of Object.entries(states)) {
      for (const args of [['status'], ['status', '--json']]) {
        const r = same(args, { state });
        assert.equal(r.code, 0, name);
        assert.deepEqual(r.files, {}, name);
      }
    }
    assert.match(same(['status'], { state: states.directionApproved }).stdout, /\nnext: "dfw phase system"\n$/);
    assert.equal(same(['status'], { state: null }).stdout, 'dfw: no workflow in this project (run /dfw to start).\n');
  });

  test('status: comments, stack and studio', () => {
    const concepts = st({ phase: 'concepts' });
    assert.match(same(['status'], { state: concepts, setup: withSession(MIXED) }).stdout, /comments: 3 active, 1 awaiting user verification, 1 resolved\n[^]*next: apply 3 open review comment\(s\)\n$/);
    assert.match(same(['status'], { state: concepts, setup: withSession([MIXED[2], MIXED[3]]) }).stdout, /next: ask the user to verify fixed comments in the studio\n$/);
    same(['status', '--json'], { state: concepts, setup: withSession(MIXED) });
    for (const stack of [STACK, { status: 'proposed' }, { framework: {} }, '"text"', 0]) {
      for (const state of [FROZEN, { ...FROZEN, gates: { ...FROZEN.gates, G3: GATE } }]) {
        for (const args of [['status'], ['status', '--json']]) same(args, { state, setup: (root) => write(root, '.dfw/stack.json', stack) });
      }
    }
    // A recorded Studio counts only while its process lives (this test process does; pid 0 never).
    const live = { pid: process.pid, port: 4600, url: 'http://127.0.0.1:4600/' };
    assert.match(same(['status'], { setup: (root) => write(root, '.dfw/studio.json', live) }).stdout, /\nstudio: http:\/\/127\.0\.0\.1:4600\/\n/);
    for (const studio of [{ ...live, pid: 0 }, { url: 'x' }, { ...live, pid: 'abc' }, null]) {
      for (const args of [['status'], ['status', '--json']]) assert.match(same(args, { setup: (root) => write(root, '.dfw/studio.json', studio) }).stdout, /studio: not running|"studio": null/);
    }
  });

  test('status: the system phase before DS counts the masters; a broken state fails in both', () => {
    const system = st({ phase: 'system', gates: { ...MINIMAL_STATE.gates, G1: GATE } });
    assert.match(same(['status'], { state: system }).stdout, /\nnext: review the Design System page in the studio/);
    same(['status', '--json'], { state: system });
    for (const text of ['{ nope', '']) assert.equal(sameButStderr(['status'], { setup: (root) => write(root, '.dfw/state.json', text) }).code, 1);
    for (const state of [{}, [], { gates: {} }]) {
      for (const args of [['status'], ['status', '--json']]) sameButStderr(args, { state });
    }
  });

  test('mode: aliases, errors, the waived gates follow the mode', () => {
    for (const mode of ['new', 'existing', 'design', 'refine', 'refinement', 'quick', 'review', 'quick-change', 'new-product']) {
      const r = same(['mode', mode]);
      assert.equal(r.code, 0, mode);
      assert.match(r.files['.dfw/state.json'], /"event": "mode"/);
    }
    assert.equal(JSON.parse(same(['mode', 'quick']).files['.dfw/state.json']).waived.DS, 'waived by mode quick-change');
    assert.deepEqual(JSON.parse(same(['mode', 'new'], { state: st({ mode: 'quick-change', waived: { G1: 'x', DS: 'x' } }) }).files['.dfw/state.json']).waived, {});
    assert.equal(same(['mode', 'implement'], { state: FROZEN }).code, 0);
    for (const args of [['mode', 'implement'], ['mode', 'nope'], ['mode'], ['mode', '']]) {
      const r = same(args);
      assert.equal(r.code, 1);
      assert.deepEqual(r.files, {});
    }
    assert.equal(same(['mode', 'quick'], { state: null }).stderr, 'No dfw workflow in <root>. Run "dfw init" first.\n');
  });

  test('phase: allowed moves, every blocker, --force, unknown and missing phase', () => {
    const g = (gates) => ({ ...MINIMAL_STATE.gates, ...gates });
    const cases = [
      [st(), 'direction'],
      [st(), 'system'],
      [st(), 'concepts'],
      [st({ gates: g({ G1: GATE }) }), 'system'],
      [st({ gates: g({ G1: GATE }) }), 'concepts'],
      [st({ gates: g({ G1: GATE, DS: GATE }) }), 'concepts'],
      [st({ mode: 'quick-change', waived: { G1: 'w', DS: 'w' } }), 'concepts'],
      [st(), 'stack'],
      [st(), 'implement'],
      [FROZEN, 'implement'],
      [{ ...FROZEN, gates: { ...FROZEN.gates, G3: GATE } }, 'implement'],
      [st(), 'verify'],
      [{ ...FROZEN, phase: 'implement' }, 'verify'],
      [{ ...FROZEN, mode: 'implement-approved' }, 'verify'],
      [st(), 'done'],
      [{ ...FROZEN, phase: 'verify', gates: { ...FROZEN.gates, G4: GATE } }, 'done'],
      [st({ mode: 'design-only' }), 'done'],
      [st({ mode: 'design-only' }), 'implement'],
      [{ ...FROZEN, mode: 'design-only' }, 'done'],
      [st({ mode: 'review-only', phase: 'audit' }), 'verify'],
      [st({ mode: 'review-only', phase: 'audit' }), 'done'],
      [st(), 'audit'],
      [st(), 'discovery'],
      [st(), 'nowhere'],
      [st(), undefined],
    ];
    for (const [state, to] of cases) {
      const args = ['phase', ...(to === undefined ? [] : [to])];
      const plain = same(args, { state });
      const forced = same([...args, '--force', '--reason', 'the user said so'], { state });
      // 0.5.2: --force never writes a missing or unknown phase.
      const known = to !== undefined && to !== 'nowhere';
      assert.equal(forced.code, known ? 0 : 1, `${state.mode} -> ${to}`);
      if (!known) assert.deepEqual(forced.files, {});
      if (plain.code === 1) {
        assert.match(plain.stderr, /^Cannot move to ".*":\n- /);
        assert.deepEqual(plain.files, {});
        if (known) assert.match(forced.files['.dfw/state.json'], /"forced": \[/);
      }
    }
    same(['phase', 'direction', '--reason'], {});
    same(['phase', 'direction', '--force'], {});
    assert.equal(same(['phase', 'direction'], { state: null }).code, 1);
    // A mode the table does not know is an engine error (stderr differs from the reference).
    assert.equal(sameButStderr(['phase', 'system'], { state: st({ mode: 'custom' }) }).code, 1);
  });

  test('approve: G1, G3, G4, unknown gate, missing note, no workflow', () => {
    const verify = { ...FROZEN, phase: 'verify', gates: { ...FROZEN.gates, G3: GATE } };
    const stack = (root) => write(root, '.dfw/stack.json', STACK);
    const ok = [
      [['approve', 'G1', '--note', 'Direction A'], {}],
      [['approve', 'g1', '--note=Direction "B"'], { state: st({ phase: 'direction' }) }],
      [['approve', 'G3', '--note', 'Vue 3 + plain CSS'], { state: FROZEN, setup: stack }],
      [['approve', 'G4', '--note', 'Matches the design'], { state: verify }],
    ];
    for (const [args, opts] of ok) {
      const r = same(args, opts);
      assert.equal(r.code, 0, args.join(' '));
      assert.match(r.files['.dfw/decisions.jsonl'], /"level":1,"topic":"gate:G\d"/);
      assert.match(r.files['.dfw/state.json'], /"event": "approve"/);
    }
    assert.match(same(...ok[2]).files['.dfw/stack.json'], /"status": "approved",\n {2}"proposedAt": ".*",\n {2}"approvedAt": /);
    const bad = [
      [['approve', 'G1'], {}],
      [['approve', 'G1', '--note', ''], {}],
      [['approve', 'XX', '--note', 'x'], {}],
      [['approve', '--note', 'x'], {}],
      [['approve', 'G3', '--note', 'x'], { state: FROZEN }],
      [['approve', 'G4', '--note', 'x'], { state: FROZEN }],
      [['approve', 'G1', '--note', 'x'], { state: null }],
      [['approve'], { state: null }],
      // 0.5.2: a flag that takes a value needs one; G3 refuses a stack.json that is not an object.
      [['approve', 'G1', '--note'], {}],
      [['approve', 'G3', '--note', 'x'], { state: FROZEN, setup: (root) => write(root, '.dfw/stack.json', 'null') }],
      [['approve', 'G3', '--note', 'x'], { state: FROZEN, setup: (root) => write(root, '.dfw/stack.json', [1, 2]) }],
    ];
    for (const [args, opts] of bad) {
      const r = same(args, opts);
      assert.equal(r.code, 1, args.join(' '));
      assert.deepEqual(r.files, {}, args.join(' '));
    }
    assert.equal(same(['approve', 'XX', '--note', 'x']).stderr, 'Unknown gate "XX". Gates: G1, DS, G2, G3, G4\n');
    assert.match(same(['approve', 'G1']).stderr, /^Approvals record what the user approved/);
  });

  test('approve DS and G2 run their gate checks first', () => {
    for (const gate of ['DS', 'G2', 'ds']) {
      for (const state of [st(), null]) {
        const r = same(['approve', gate], { state });
        assert.equal(r.code, 1);
        assert.deepEqual(r.files, {});
      }
    }
    // DS on a project without a rationale is refused by the design-system audit in both.
    const r = same(['approve', 'DS', '--note', 'x'], { state: st({ phase: 'system', gates: { ...MINIMAL_STATE.gates, G1: GATE } }) });
    assert.equal(r.code, 1);
    assert.match(r.stderr, /^DS blocked: /);
    assert.ok(!('.dfw/state.json' in r.files));
  });

  test('reopen, pause and resume', () => {
    for (const phase of ['stack', 'implement', 'verify', 'done', 'concepts', 'audit']) {
      const r = same(['reopen', '--reason', 'Client feedback'], { state: { ...FROZEN, phase, gates: { ...FROZEN.gates, G4: GATE } } });
      assert.equal(r.code, 0);
      const s = JSON.parse(r.files['.dfw/state.json']);
      assert.deepEqual([s.design.version, s.design.status, s.design.locked, s.gates.G2, s.gates.G4], [2, 'draft', false, null, null]);
      assert.equal(s.phase, ['concepts', 'audit'].includes(phase) ? phase : 'concepts');
    }
    same(['reopen', '--reason'], { state: FROZEN });
    same(['reopen', '--reason', 'x'], { state: { ...FROZEN, design: { ...APPROVED_DESIGN, version: 2.5 } } });
    for (const [args, opts] of [[['reopen'], { state: FROZEN }], [['reopen', '--reason='], { state: FROZEN }], [['reopen', '--reason', 'x'], {}], [['reopen', '--reason', 'x'], { state: null }]]) {
      const r = same(args, opts);
      assert.equal(r.code, 1);
      assert.deepEqual(r.files, {});
    }
    for (const cmd of ['pause', 'resume']) {
      for (const args of [[], ['--reason', 'Hotfix first'], ['--reason=']]) {
        const r = same([cmd, ...args], { state: st({ active: cmd === 'pause' }) });
        assert.equal(JSON.parse(r.files['.dfw/state.json']).active, cmd === 'resume');
      }
      // 0.5.2: --reason alone is refused ("--reason needs a value.").
      assert.deepEqual(same([cmd, '--reason'], { state: st({ active: cmd === 'pause' }) }).files, {});
      assert.equal(same([cmd], { state: null }).code, 1);
    }
    assert.equal(same(['pause']).stdout, 'dfw paused (gates and hooks off) for <root>\n');
  });

  test('decide and decisions', () => {
    const ok = [
      ['--level', '1', 'Blue', 'accent'],
      ['--level', '3', '--topic', 'type', '--overrides', 'd1', 'Inter for UI text'],
      ['--level', '2.5', '--text', 'From a flag'],
      ['--level=5', 'x'],
      ['--level', '0x2', 'hex'],
      ['--level', ' 4 ', 'padded'],
      ['words', 'first', '--level', '2'],
    ];
    for (const args of ok) {
      const r = same(['decide', ...args]);
      assert.equal(r.code, 0, args.join(' '));
      assert.equal(r.files['.dfw/decisions.jsonl'], `${JSON.stringify(JSON.parse(r.stdout))}\n`);
    }
    assert.equal(same(['decide', '--level', '2', 'x'], { state: null }).code, 0, 'decide needs no workflow');
    // 0.5.2: --level and --text alone are refused, no longer read as level 1 and an empty text.
    for (const args of [['x'], ['--level', '0', 'x'], ['--level', '6', 'x'], ['--level', 'two', 'x'], ['--level', '', 'x'], ['--level', '2'], ['--level', '2', '--text', ''], ['--level', '--topic', 't', 'x'], ['--level', ' 4 ', '--text']]) {
      const r = same(['decide', ...args]);
      assert.equal(r.code, 1, args.join(' '));
      assert.deepEqual(r.files, {});
    }
    const log = (root) => write(root, '.dfw/decisions.jsonl', `${JSON.stringify({ at: NOW, level: 1, topic: 'gate:G1', decision: 'Design direction approved - A', by: 'user' })}\n\n${JSON.stringify({ at: '2026-02-03T04:05:06.007Z', level: 2.5, topic: 'type', decision: 'Inter', by: 'orchestrator', overrides: 'd1' })}\n`);
    assert.equal(same(['decisions']).stdout, 'No decisions recorded.\n');
    assert.equal(same(['decisions', '--json']).stdout, '[]\n');
    assert.equal(same(['decisions'], { setup: log }).stdout, '2026-01-02T03:04 L1 [gate:G1] Design direction approved - A\n2026-02-03T04:05 L2.5 [type] Inter\n');
    same(['decisions', '--json'], { setup: log });
    for (const text of ['{ nope\n', '{"level":1}\n']) assert.equal(sameButStderr(['decisions'], { setup: (root) => write(root, '.dfw/decisions.jsonl', text) }).code, 1);
  });

  test('config get and set in every layer', () => {
    // A relative DFW_HOME puts the global config under .dfw/, where the harness compares files.
    const env = { DFW_HOME: '.dfw/home' };
    const layers = (root) => (write(root, '.dfw/config.json', { schema: 1, gate: { allow: ['docs/**'] }, checks: { onEdit: false } }), write(root, '.dfw/config.local.json', { studio: { port: 4800 } }), write(root, '.dfw/home/config.json', { language: { lastUsed: 'vi' }, gate: { allow: ['global/**'] } }));
    for (const args of [[], ['gate.allow'], ['studio.port'], ['studio.breakpoints'], ['checks'], ['language.lastUsed'], ['no.such.key'], ['studio.breakpoints.1.id'], ['']]) {
      same(['config', 'get', ...args], { env });
      same(['config', 'get', ...args], { env, setup: layers });
    }
    assert.equal(same(['config', 'get', 'no.such.key']).stdout, 'undefined\n');
    assert.equal(same(['config', 'get', 'studio.port'], { env, setup: layers }).stdout, '4800\n');
    const sets = [
      [['studio.port', '4700'], '.dfw/config.json'],
      [['gate.allow', '["docs/**","notes/**"]'], '.dfw/config.json'],
      [['checks.onEdit', 'false'], '.dfw/config.json'],
      [['review.shortcut', 'ctrl+shift+k'], '.dfw/config.json'],
      [['custom.deep.key', '{"a":[1,2.5,null]}'], '.dfw/config.json'],
      [['studio.port', '4900', '--local'], '.dfw/config.local.json'],
      [['language.default', 'vi', '--global'], '.dfw/home/config.json'],
      [['studio.port', '4900', '--project'], '.dfw/config.json'],
    ];
    for (const [args, file] of sets) {
      for (const setup of [undefined, layers]) {
        const r = same(['config', 'set', ...args], { env, setup });
        assert.equal(r.code, 0, args.join(' '));
        assert.match(r.stdout, new RegExp(`^Set ${args[0].replaceAll('.', '\\.')} in .*${file.replaceAll('.', '\\.')}\n$`));
      }
    }
    assert.equal(same(['config', 'set', 'studio.port', '4700']).files['.dfw/config.json'], '{\n  "studio": {\n    "port": 4700\n  }\n}\n');
    assert.equal(sameButStderr(['config', 'set']).code, 1);
    // 0.5.2: set without a value is a usage error and writes nothing.
    for (const key of ['a.b', 'gate.allow', 'schema']) {
      for (const setup of [undefined, layers]) {
        const r = same(['config', 'set', key], { env, setup });
        assert.deepEqual([r.code, r.files], [1, {}], key);
      }
    }
    for (const args of [['config'], ['config', 'unset', 'a']]) assert.equal(same(args).stderr, 'Usage: dfw config get [key] | set <key> <value> [--global|--project|--local]\n');
    assert.equal(sameButStderr(['config', 'get'], { setup: (root) => write(root, '.dfw/config.json', '{ nope') }).code, 1);
  });

  test('lang status and set', () => {
    const env = { DFW_HOME: '.dfw/home', DFW_SESSION_ID: '', CLAUDE_SESSION_ID: '' };
    const chosen = (root) => (write(root, '.dfw/home/sessions/abc.json', { language: 'vi', setAt: NOW }), write(root, '.dfw/home/config.json', { marketplaceRepo: '/r', language: { askPolicy: 'session', lastUsed: 'vi' } }));
    assert.deepEqual(JSON.parse(same(['lang'], { env }).stdout), { session: null, chosen: false, language: null, lastUsed: 'en' });
    for (const args of [['status'], ['status', '--session', 'abc'], ['status', '--session', 'def'], ['status', '--session=']]) {
      same(['lang', ...args], { env });
      same(['lang', ...args], { env, setup: chosen });
    }
    assert.equal(JSON.parse(same(['lang', 'status', '--session', 'abc'], { env, setup: chosen }).stdout).chosen, true);
    const set = same(['lang', 'set', 'vi', '--session', 'abc'], { env });
    assert.deepEqual(Object.keys(set.files), ['.dfw/home/config.json', '.dfw/home/sessions/abc.json']);
    for (const args of [['set', ' EN-us '], ['set'], ['set', ''], ['set', 'fr', '--session', '../../x y/é😀'], ['set', 'de']]) {
      same(['lang', ...args], { env });
      same(['lang', ...args], { env, setup: chosen });
    }
    // The session comes from the environment when no flag names one.
    assert.equal(JSON.parse(same(['lang', 'set', 'ja'], { env: { ...env, DFW_SESSION_ID: 'from-env' } }).stdout).session, 'from-env');
    assert.equal(JSON.parse(same(['lang', 'status'], { env: { ...env, CLAUDE_SESSION_ID: 'abc' }, setup: chosen }).stdout).language, 'vi');
    assert.equal(same(['lang', 'nope'], { env }).stderr, 'Usage: dfw lang status|set <code> [--session <id>]\n');
    for (const args of [['status', '--session'], ['set', 'vi', '--session']]) assert.equal(sameButStderr(['lang', ...args], { env }).code, 1);
  });

  test('verify-snapshot: intact, modified, missing, added, no snapshot', () => {
    /** A project frozen as v1 by the binary (`__debug hot freeze`, the freeze of "dfw approve G2"). */
    const frozen = (after = () => {}) => (root) => {
      write(root, 'DESIGN.md', '# Design\n');
      write(root, '.dfw/state.json', st({ phase: 'concepts' }));
      helper(['hot', 'freeze', root, 'v1']);
      after(root);
    };
    const v1 = (root, rel) => path.join(root, 'design', 'approved', 'v1', rel);
    const intact = same(['verify-snapshot'], { setup: frozen() });
    assert.deepEqual([intact.code, intact.stdout], [0, 'Snapshot v1 intact.\n']);
    same(['verify-snapshot', 'v1'], { setup: frozen() });
    const tampered = same(['verify-snapshot'], {
      setup: frozen((root) => {
        fs.appendFileSync(v1(root, 'screens/items/a.html'), '<!-- edited -->');
        fs.rmSync(v1(root, 'tokens/tokens.json'));
        fs.rmSync(v1(root, 'docs/DESIGN.md'));
        fs.writeFileSync(v1(root, 'zz-extra.txt'), 'x');
        fs.mkdirSync(v1(root, 'new'), { recursive: true });
        fs.writeFileSync(v1(root, 'new/a.css'), 'x');
      }),
    });
    assert.equal(tampered.code, 2);
    assert.equal(tampered.stdout, 'Snapshot v1 was modified:\nmodified: screens/items/a.html\nmissing: tokens/tokens.json\nmissing: docs/DESIGN.md\nadded: new/a.css\nadded: zz-extra.txt\n');
    // After a reopen the last approved version is still the one checked.
    same(['verify-snapshot'], { setup: frozen((root) => write(root, '.dfw/state.json', { ...FROZEN, design: { ...APPROVED_DESIGN, version: 2, status: 'draft', locked: false } })) });
    for (const [args, opts] of [[['verify-snapshot'], {}], [['verify-snapshot', 'v9'], { setup: frozen() }], [['verify-snapshot'], { state: FROZEN }], [['verify-snapshot'], { state: null }]]) assert.equal(same(args, opts).code, 1);
    assert.equal(same(['verify-snapshot']).stderr, 'No approved snapshot.\n');
    assert.equal(sameButStderr(['verify-snapshot'], { setup: frozen((root) => fs.writeFileSync(v1(root, 'approval.lock.json'), '{ nope')) }).code, 1);
  });
});

describe('hot: comments', { skip: skipReason() }, () => {
  test('list and show', () => {
    assert.equal(same(['comments']).stdout, 'No comments.\n');
    assert.equal(same(['comments', 'list', '--json']).stdout, '[]\n');
    const opts = () => ({ setup: withSession(MIXED) });
    for (const args of [[], ['list'], ['list', '--json'], ['list', '--status', 'active'], ['list', '--status', 'fixed,resolved'], ['list', '--status', 'nope'], ['list', '--status'], ['list', '--status', 'active', '--json'], ['list', '--status=']]) same(['comments', ...args], opts());
    assert.equal(same(['comments', 'list', '--status', 'fixed'], opts()).stdout, 'fb_003 [fixed]  -> main > p\n    "Comment fb_003 "quoted" tiếng Việt"\n    last: claude: Wider now\n');
    assert.equal(JSON.parse(same(['comments', 'show', 'fb_005'], opts()).stdout).session, 'rv_1');
    for (const args of [['show', 'fb_999'], ['show']]) assert.equal(same(['comments', ...args], opts()).code, 1);
    assert.equal(same(['comments', 'show'], opts()).stderr, 'Comment undefined not found.\n');
    assert.equal(same(['comments', 'nope'], opts()).stderr, 'Unknown comments subcommand "nope".\n');
    // Comments of every session are listed, oldest session first.
    same(['comments', 'list'], { setup: all(withSession(MIXED.slice(0, 2), { closedAt: NOW }), (root) => write(root, '.dfw/reviews/rv_2.json', { schema: 1, id: 'rv_2', startedAt: NOW, designVersion: 'v2', comments: [comment('fb_007')] })) });
    for (const text of ['{ nope', '{}', '{"comments":5}']) assert.equal(sameButStderr(['comments', 'list'], { setup: (root) => write(root, '.dfw/reviews/rv_1.json', text) }).code, 1);
  });

  test('every status change from every status, with and without a message', () => {
    const ids = { open: 'fb_001', acknowledged: 'fb_002', fixed: 'fb_003', resolved: 'fb_004', reopened: 'fb_005', dismissed: 'fb_006' };
    const subs = [['ack'], ['fixed'], ['reply'], ['dismiss'], ['resolve'], ['resolve', '--by', 'user'], ['reopen']];
    let allowed = 0;
    for (const [from, id] of Object.entries(ids)) {
      for (const [sub, ...flags] of subs) {
        for (const message of [[], ['--message', `Note for ${sub}`]]) {
          const r = same(['comments', sub, id, ...flags, ...message], { setup: withSession(MIXED) });
          const label = `${from} ${sub} ${flags.join(' ')} ${message.length ? 'with a message' : ''}`;
          if (r.code === 0) {
            allowed++;
            assert.match(r.files['.dfw/reviews/rv_1.json'], /"updatedAt": "2026-01-02T03:04:05\.678Z"/, label);
            assert.equal(JSON.parse(r.stdout).id, id, label);
          } else {
            assert.equal(r.code, 1, label);
            assert.deepEqual(r.files, {}, label);
          }
          assert.ok(!('.dfw/reviews/.lock' in r.files), 'the lock is released');
        }
      }
    }
    // 0.5.2: setting the status a comment already has is refused (it was a no-op change before).
    assert.equal(allowed, 24);
    const at = (args) => same(['comments', ...args], { setup: withSession(MIXED) });
    assert.match(at(['resolve', 'fb_003']).stderr, /^Only the user resolves a comment/);
    assert.match(at(['fixed', 'fb_001']).stderr, /^Marking a comment fixed needs a short note/);
    assert.equal(at(['ack', 'fb_003']).stderr, 'Comment fb_003: cannot go from "fixed" to "acknowledged" (allowed: resolved, reopened).\n');
    assert.equal(at(['reply', 'fb_001']).stderr, '--message is required.\n');
    assert.match(at(['dismiss', 'fb_001']).stderr, /^Dismissing needs --message/);
    assert.equal(at(['ack', 'fb_999']).stderr, 'Comment fb_999 not found.\n');
    assert.equal(at(['ack']).stderr, 'Comment undefined not found.\n');
  });

  test('message, author and evidence flags', () => {
    const at = (args) => same(['comments', ...args], { setup: withSession(MIXED) });
    const fixed = JSON.parse(at(['fixed', 'fb_001', '--message', 'Bigger now', '--evidence', '.dfw/shots/a.png']).stdout);
    assert.deepEqual(fixed.resolution, { at: NOW, by: 'claude', note: 'Bigger now', evidence: '.dfw/shots/a.png' });
    for (const args of [
      ['fixed', 'fb_001', '--m', 'short flag'],
      ['fixed', 'fb_001', '--message', '', '--m', 'empty message falls back'],
      ['fixed', 'fb_001', '--message'],
      ['fixed', 'fb_001', '--message', 'x', '--evidence'],
      ['fixed', 'fb_001', '--message='],
      ['reply', 'fb_002', '--message', 'Which one do you mean?'],
      ['reply', 'fb_004', '--message'],
      ['dismiss', 'fb_001', '--message', 'Out of scope', '--by', 'user'],
      ['dismiss', 'fb_001', '--message', 'Out of scope', '--by'],
      ['resolve', 'fb_003', '--by', 'user', '--message', 'Looks right'],
      ['resolve', 'fb_003', '--by', 'someone'],
      ['reopen', 'fb_004', '--by', 'claude', '--message', 'Regressed'],
      ['reopen', 'fb_004', '--by='],
      ['ack', 'fb_002', '--message', 'same status adds a thread line'],
      ['ack', 'fb_002'],
    ]) at(args);
  });

  test('add: into the open session, a new session, source lines, bad input', () => {
    const full = { screen: 'items', variant: 'a', breakpoint: 'mobile', state: 'empty', theme: 'dark', file: 'screens/items/a.html', target: { dfwId: 'items.title', selector: 'h1', component: 'button' }, comment: '  Make the title bigger \n', screenshot: '.dfw/shots/a.png', provider: 'penpot', url: 'http://localhost:3000/a' };
    const added = same(['comments', 'add', '--json', JSON.stringify(full)], { setup: withSession(MIXED) });
    const c = JSON.parse(added.stdout);
    assert.deepEqual([c.id, c.comment, c.target.source, c.file], ['fb_007', 'Make the title bigger', 'design/screens/items/a.html:18', 'design/screens/items/a.html']);
    const inputs = [
      { comment: 'x' },
      { comment: 'x', file: 'screens/items/a.html', target: { dfwId: 'no.such.id' } },
      { comment: 'x', file: 'screens/none.html', target: { dfwId: 'items.title' } },
      { comment: 'x', file: 'screens/items/a.html', target: { selector: 'p', source: 'src/App.tsx:3:5' } },
      { comment: 'x', file: 'screens/items/a.html', target: { dfwId: 'items.add', source: '' } },
      { comment: 'x', target: 'text', screen: 0, variant: '', theme: false, provider: '' },
      { comment: 'x', url: 42, extra: 'ignored' },
      // 0.5.2: a file that is not a relative path inside design/ is stored as null.
      { comment: 'x', file: 5 },
    ];
    for (const input of inputs) {
      const json = ['comments', 'add', '--json', JSON.stringify(input)];
      same(json, { setup: withSession(MIXED) });
      // No session yet, and a closed last session: a new one named after the time is started.
      const fresh = same(json);
      assert.deepEqual(Object.keys(fresh.files), ['.dfw/reviews/rv_<now>.json']);
      assert.equal(JSON.parse(fresh.stdout).id, 'fb_001');
      const afterClosed = same(json, { setup: withSession(MIXED, { closedAt: NOW }) });
      assert.deepEqual(Object.keys(afterClosed.files), ['.dfw/reviews/rv_<now>.json']);
      assert.equal(JSON.parse(afterClosed.stdout).id, 'fb_007');
    }
    assert.equal(JSON.parse(same(['comments', 'add', '--json', '{"comment":"x"}'], { state: { ...FROZEN, design: { ...APPROVED_DESIGN, version: 3 } } }).files['.dfw/reviews/rv_<now>.json']).designVersion, 'v3');
    same(['comments', 'add', '--json', '{"comment":"x"}'], { state: null });
    // Ids continue after the highest one, whatever its form.
    assert.equal(JSON.parse(same(['comments', 'add', '--json', '{"comment":"x"}'], { setup: withSession([comment('fb_041'), comment('fb_7'), comment('note'), comment('fb_1e2')]) }).stdout).id, 'fb_101');
    for (const input of ['{}', '{"comment":"   "}', '{"comment":null}', 'null', '"text"', '[]']) {
      const r = same(['comments', 'add', '--json', input], { setup: withSession(MIXED) });
      assert.deepEqual([r.code, r.stderr, r.files], [1, 'A comment needs text.\n', {}], input);
    }
    assert.equal(same(['comments', 'add', '--json']).stderr, 'A comment needs text.\n');
    for (const args of [['--json', '{ nope'], [], ['--json', '{"comment":5}']]) assert.equal(sameButStderr(['comments', 'add', ...args], { setup: withSession(MIXED) }).code, 1);
  });

  test('new-round closes the open session and starts one', () => {
    const r = same(['comments', 'new-round'], { setup: withSession(MIXED) });
    assert.deepEqual(Object.keys(r.files), ['.dfw/reviews/rv_1.json', '.dfw/reviews/rv_<now>.json']);
    assert.equal(JSON.parse(r.files['.dfw/reviews/rv_1.json']).closedAt, NOW);
    assert.deepEqual(JSON.parse(r.stdout), { schema: 1, id: 'rv_<now>', startedAt: NOW, designVersion: 'v1', comments: [] });
    same(['comments', 'new-round']);
    same(['comments', 'new-round'], { state: null });
    same(['comments', 'new-round'], { state: { ...FROZEN, design: { ...APPROVED_DESIGN, version: 4 } }, setup: withSession(MIXED, { closedAt: '2026-01-01T00:00:00.000Z' }) });
  });

  test('inbox: delivers each comment state once; --peek leaves it', () => {
    assert.equal(same(['comments', 'inbox']).stdout, 'No new comments.\n');
    const first = same(['comments', 'inbox'], { setup: withSession(MIXED) });
    assert.match(first.stdout, /^2 new Studio comment\(s\) from the user:\n- fb_001 \[open\] items \/ a -> items\.title \(design\/screens\/items\/a\.html:18\)\n/);
    assert.match(first.stdout, /\n- fb_005 \[reopened\] .*\n {4}".*"\n {4}reopened by user: "Still too small"\n/);
    assert.deepEqual(JSON.parse(first.files['.dfw/reviews/.delivered.json']), ['fb_001:open:2026-01-01T00:00:00.000Z', 'fb_005:reopened:2026-01-01T02:00:00.000Z']);
    assert.deepEqual(same(['comments', 'inbox', '--peek'], { setup: withSession(MIXED) }).files, {});
    const delivered = (keys) => (root) => write(root, '.dfw/reviews/.delivered.json', keys);
    assert.equal(same(['comments', 'inbox'], { setup: all(withSession(MIXED), delivered(['fb_001:open:2026-01-01T00:00:00.000Z', 'fb_005:reopened:2026-01-01T02:00:00.000Z'])) }).stdout, 'No new comments.\n');
    same(['comments', 'inbox'], { setup: all(withSession(MIXED), delivered(['fb_001:open:2026-01-01T00:00:00.000Z', 'fb_005:fixed:x'])) });
    // A user's reply on an acknowledged comment, a live comment and one on the design system.
    const more = [
      comment('fb_001', 'acknowledged', { thread: [line('claude', 'On it', 'acknowledged'), line('user', 'Also the header')], updatedAt: '2026-01-01T03:00:00.000Z' }),
      comment('fb_002', 'open', { provider: 'dfw-live', screen: null, variant: null, file: null, target: { selector: 'main > button', source: 'src/App.tsx:12:7' }, url: 'http://localhost:3000/' }),
      comment('fb_003', 'open', { screen: null, variant: null, target: { component: 'button' } }),
      comment('fb_004', 'reopened'),
    ];
    const r = same(['comments', 'inbox'], { setup: withSession(more) });
    assert.match(r.stdout, /follow-up by user: "Also the header"/);
    assert.match(r.stdout, /\nComments marked "live" are on the running app/);
    assert.match(r.stdout, /- fb_003 \[open\] design system -> \? component button\n/);
    assert.equal(sameButStderr(['comments', 'inbox'], { setup: all(withSession(MIXED), (root) => write(root, '.dfw/reviews/.delivered.json', '{ nope')) }).code, 1);
  });

  test('wait: returns after the settle time when comments are there, or at the timeout', () => {
    const start = Date.now();
    const got = same(['comments', 'wait', '--timeout', '30'], { setup: withSession(MIXED) });
    assert.match(got.stdout, /^2 new Studio comment\(s\) from the user:\n[^]*\nAfter fixing, start "dfw comments wait" in the background again to keep listening\.\n$/);
    assert.ok('.dfw/reviews/.delivered.json' in got.files);
    assert.ok(Date.now() - start >= 1500, 'the binary waits the settle time');
    const none = same(['comments', 'wait', '--timeout', '1']);
    assert.equal(none.stdout, 'No new comments before the timeout. Re-run "dfw comments wait" in the background to keep listening.\n');
    // The flag alone is one second; a timeout in the past returns at once, comments or not.
    same(['comments', 'wait', '--timeout']);
    assert.deepEqual(same(['comments', 'wait', '--timeout', '-5'], { setup: withSession(MIXED) }).files, {});
    same(['comments', 'wait', '--timeout=0.2']);
  });
});

/**
 * Options for a hook run whose payload needs the project's path: `payload(root)` is evaluated
 * for each implementation's own copy (the harness reads `stdin` after it called `setup`).
 */
function hookOpts(payload, opts = {}) {
  const o = { ...opts };
  let root = null;
  o.setup = (r) => {
    root = r;
    opts.setup?.(r);
  };
  Object.defineProperty(o, 'stdin', { enumerable: true, get: () => (typeof payload === 'function' ? payload(root) : payload) });
  return o;
}
const json = (fn) => (root) => JSON.stringify(fn(root));
const HOSTS = [[], ['--host', 'copilot'], ['--host', 'claude']];

describe('hot: hooks', { skip: skipReason() }, () => {
  /** [label, payload(root)]: the same edit as each host sends it. */
  const edit = (rel, { tool = 'Edit', extra = {} } = {}) => [
    json((root) => ({ session_id: 's1', hook_event_name: 'PreToolUse', cwd: root, tool_name: tool, tool_input: { [tool === 'NotebookEdit' ? 'notebook_path' : 'file_path']: path.join(root, rel), ...extra } })),
    json((root) => ({ sessionId: 's1', hookEventName: 'preToolUse', cwd: root, toolName: tool === 'Write' ? 'create' : tool === 'Edit' ? 'str_replace_editor' : tool, toolArgs: JSON.stringify({ path: rel, ...extra }) })),
    json((root) => ({ cwd: root, toolName: tool === 'Write' ? 'write' : 'edit', toolArgs: { filePath: path.join(root, rel), ...extra } })),
  ];
  const patch = (body, cwd = (root) => root) => [
    json((root) => ({ cwd: cwd(root), toolName: 'apply_patch', toolArgs: body })),
    json((root) => ({ cwd: cwd(root), tool_name: 'ApplyPatch', tool_input: { patch: body } })),
    json((root) => ({ cwd: cwd(root), tool_name: 'apply_patch', tool_input: JSON.stringify({ input: body }) })),
    json((root) => ({ cwd: cwd(root), toolName: 'apply_patch', toolArgs: { content: body } })),
  ];
  const PATHS = ['design/screens/items/a.html', 'design/system/components/button.html', 'design/tokens/tokens.json', 'PRODUCT.md', 'DESIGN.md', '.dfw/notes.md', 'design/handoff/v1.md', 'design/approved/v1/screens/items/a.html', 'src/App.tsx', 'package.json', '..outside-looking-name.ts', 'design'];
  const STATES = {
    discovery: st(),
    frozen: FROZEN,
    implement: { ...FROZEN, phase: 'implement', gates: { ...FROZEN.gates, G3: GATE } },
    paused: st({ active: false }),
    reviewOnly: st({ mode: 'review-only', phase: 'audit' }),
  };

  /** Run one pre-tool-use payload for a host; returns the native result. */
  const pre = (host, payload, opts = {}) => same(['hook', 'pre-tool-use', ...host], hookOpts(payload, opts));

  test('pre-tool-use: allowed and denied paths, in every workflow state, for both hosts', () => {
    const seen = { allow: 0, deny: 0 };
    for (const [name, state] of Object.entries(STATES)) {
      for (const rel of PATHS) {
        for (const host of HOSTS) {
          for (const payload of edit(rel, { tool: 'Write' })) {
            const r = pre(host, payload, { state });
            assert.equal(r.code, 0);
            if (r.stdout) {
              seen.deny++;
              const out = JSON.parse(r.stdout);
              assert.equal((out.hookSpecificOutput || out).permissionDecision, 'deny', `${name} ${rel}`);
              assert.equal('hookSpecificOutput' in out, host[1] !== 'copilot');
              assert.deepEqual(r.files, {}, 'a refused edit leaves no presence note');
            } else seen.allow++;
          }
        }
      }
    }
    assert.ok(seen.allow > 100 && seen.deny > 100, JSON.stringify(seen));
    assert.equal(pre([], edit('src/App.tsx')[0], { state: STATES.implement }).stdout, '');
    assert.match(pre([], edit('src/App.tsx')[0]).stdout, /Missing: G2 \(approved, frozen design\), G3 \(approved technology stack\), phase \\"implement\\" \(current: \\"discovery\\"\)/);
    assert.match(pre([], edit('design/screens/items/a.html')[0], { state: FROZEN }).stdout, /design v1 is approved and locked/);
    assert.match(pre([], edit('design/approved/v1/a.html')[0]).stdout, /is immutable/);
    assert.match(pre([], edit('design/x.html')[0], { state: STATES.reviewOnly }).stdout, /review-only mode/);
  });

  test('pre-tool-use: every write tool; tools that do not write; a path outside the project', () => {
    for (const tool of ['Edit', 'Write', 'MultiEdit', 'NotebookEdit']) {
      for (const rel of ['src/a.ipynb', 'design/screens/items/a.html']) {
        for (const host of HOSTS) for (const payload of edit(rel, { tool })) pre(host, payload);
      }
    }
    for (const host of HOSTS) {
      for (const input of [
        (root) => ({ cwd: root, tool_name: 'Read', tool_input: { file_path: path.join(root, 'src/a.ts') } }),
        (root) => ({ cwd: root, tool_name: 'Bash', tool_input: { command: 'rm -rf src' } }),
        (root) => ({ cwd: root, toolName: 'view', toolArgs: '{"path":"src/a.ts"}' }),
        (root) => ({ cwd: root, tool_name: 'Write', tool_input: {} }),
        (root) => ({ cwd: root, tool_name: 'Write' }),
        (root) => ({ cwd: root }),
        () => ({}),
        (root) => ({ cwd: root, tool_name: 'Write', tool_input: { file_path: '/elsewhere/outside.ts' } }),
        (root) => ({ cwd: root, tool_name: 'Write', tool_input: { file_path: path.join(root, '..', 'sibling', 'a.ts') } }),
        (root) => ({ cwd: root, tool_name: 'Write', tool_input: { file_path: '../sibling/a.ts' } }),
        (root) => ({ cwd: path.join(root, 'design'), tool_name: 'Write', tool_input: { file_path: '../src/a.ts' } }),
      ]) {
        const r = pre(host, json(input));
        assert.deepEqual([r.code, r.stderr], [0, '']);
      }
    }
    assert.equal(pre([], json((root) => ({ cwd: root, tool_name: 'Write', tool_input: { file_path: '/elsewhere/outside.ts' } }))).stdout, '');
    assert.notEqual(pre([], json((root) => ({ cwd: path.join(root, 'design'), tool_name: 'Write', tool_input: { file_path: '../src/a.ts' } }))).stdout, '');
  });

  test('pre-tool-use: apply_patch bodies, paths relative to the session cwd', () => {
    const body = (...lines) => ['*** Begin Patch', ...lines, '*** End Patch'].join('\n');
    const bodies = [
      body('*** Add File: design/screens/new/a.html', '+<html></html>'),
      body('*** Update File: design/screens/items/a.html', '@@', '-old', '+new', '*** Add File: src/x.ts', '+x'),
      body('*** Update File: src/a.ts', '*** Move to: design/a.ts'),
      body('*** Delete File: design/approved/v1/a.html'),
      body('*** Update File:   .dfw/notes.md  '),
      body('*** Update File: design/a.html').replaceAll('\n', '\r\n'),
      body('*** Rename File: src/a.ts', 'no header: *** Add File: src/b.ts'),
      body('*** Add File: '),
      '',
    ];
    for (const b of bodies) {
      for (const host of HOSTS) for (const payload of patch(b)) pre(host, payload);
    }
    // Relative patch paths resolve against the session cwd (here design/), not the project root.
    const inDesign = patch(body('*** Add File: screens/a.html'), (root) => path.join(root, 'design'));
    const atRoot = patch(body('*** Add File: screens/a.html'));
    for (const host of HOSTS) {
      for (const payload of inDesign) assert.equal(pre(host, payload).stdout, '');
      for (const payload of atRoot) assert.notEqual(pre(host, payload).stdout, '');
    }
    assert.equal(JSON.parse(pre(['--host', 'copilot'], atRoot[0]).stdout).permissionDecision, 'deny');
  });

  test('pre-tool-use: the presence note names the element about to be edited', () => {
    const file = 'design/screens/items/a.html';
    const edits = [
      { old_string: '<h1 data-dfw-id="items.title">Inventory</h1>', new_string: '<h1 data-dfw-id="items.title">Stock</h1>' },
      { old_string: 'Counted on Monday.', new_string: 'Counted today.' },
      { old_string: 'Export', new_string: 'Download' },
      { old_string: '<dfw-use c="button" variant="ghost" id="items.export">', new_string: '' },
      { old_string: 'not in the file', new_string: 'x' },
      { old_string: '<!doctype html>', new_string: '' },
      { edits: [{ old_string: '12 items', new_string: '13 items' }, { old_string: 'Export', new_string: 'x' }] },
      { edits: [] },
      { content: '<html></html>' },
      { old_string: 5 },
    ];
    for (const extra of edits) {
      for (const tool of ['Edit', 'MultiEdit']) {
        for (const payload of edit(file, { tool, extra })) {
          const r = pre([], payload);
          assert.deepEqual(Object.keys(r.files), ['.dfw/presence.json']);
        }
      }
    }
    const note = (extra, rel = file, opts) => JSON.parse(pre([], edit(rel, { extra })[0], opts).files['.dfw/presence.json'] ?? 'null');
    assert.deepEqual(note(edits[0]), { file: 'screens/items/a.html', dfwId: 'items.title', at: NOW });
    assert.equal(note(edits[1]).dfwId, 'items.note', 'the nearest id before the text');
    assert.equal(note(edits[2]).dfwId, 'items.export', 'an instance id counts');
    assert.equal(note(edits[4]).dfwId, null);
    assert.equal(note(edits[6]).dfwId, 'items.summary', 'MultiEdit: the first edit');
    assert.deepEqual(note({ old_string: 'x' }, 'design/tokens/tokens.json'), { file: 'tokens/tokens.json', dfwId: null, at: NOW });
    assert.equal(note({}, 'design/screens/new/b.html').file, 'screens/new/b.html');
    // No note for files outside design/, snapshots, or a paused workflow.
    assert.equal(note({}, '.dfw/notes.md'), null);
    assert.equal(note({}, 'design/handoff/v1.md', { state: FROZEN }).file, 'handoff/v1.md');
    assert.equal(note(edits[0], file, { state: STATES.paused }), null);
    assert.equal(note({}, 'src/a.ts', { state: STATES.implement }), null);
  });

  test('pre-tool-use fails open identically: no state, broken state, odd payloads, unknown host', () => {
    const src = edit('src/App.tsx', { tool: 'Write' });
    for (const host of HOSTS) {
      for (const payload of src) {
        assert.equal(pre(host, payload, { state: null }).stdout, '', 'no workflow: nothing is gated');
        for (const text of ['{ nope', '', 'null', '[]', '{}', '"text"', '0', '{"design":{"status":"approved"}}', '{"active":false}']) {
          const r = sameButStderr(['hook', 'pre-tool-use', ...host], hookOpts(payload, { setup: (root) => write(root, '.dfw/state.json', text) }));
          assert.deepEqual([r.code, r.files], [0, {}], text);
        }
      }
    }
    // A state that cannot be read lets the edit through and says why on stderr.
    const broken = sameButStderr(['hook', 'pre-tool-use'], hookOpts(src[0], { setup: (root) => write(root, '.dfw/state.json', '{ nope') }));
    assert.deepEqual([broken.code, broken.stdout], [0, '']);
    assert.match(broken.stderr, /^dfw hook pre-tool-use error: Invalid JSON in .*state\.json/);
    for (const stdin of ['not json', '', '   ', 'null', '[]', '5', '"text"', '{"tool_name":"Write","tool_input":null}', '{"tool_name":"Write","tool_input":"null"}', '{"tool_name":"Write","tool_input":{"file_path":5}}', '{"tool_name":"Write","tool_input":{"file_path":"src/a.ts"},"cwd":5}', '{"tool_name":"Write","tool_input":[],"cwd":""}', '{"toolName":"apply_patch","toolArgs":5}']) {
      for (const host of HOSTS) {
        const r = sameButStderr(['hook', 'pre-tool-use', ...host], { stdin });
        assert.deepEqual([r.code, r.files], [0, {}], stdin);
      }
    }
    // Without a cwd in the payload the process's working directory (the project) is used.
    assert.notEqual(same(['hook', 'pre-tool-use'], { stdin: '{"tool_name":"Write","tool_input":{"file_path":"src/a.ts"}}' }).stdout, '');
    assert.notEqual(same(['hook', 'pre-tool-use'], { stdin: '{"tool_name":"Write","tool_input":{"file_path":"src/a.ts"}}', env: { DFW_PROJECT_DIR: '', CLAUDE_PROJECT_DIR: '' } }).stdout, '');
    for (const host of [['--host', 'nope'], ['--host'], ['--host', 'mcp'], ['--host', 'codex']]) {
      const r = sameButStderr(['hook', 'pre-tool-use', ...host], hookOpts(src[0]));
      assert.equal(r.code, 0);
    }
    assert.equal(sameButStderr(['hook', 'pre-tool-use', '--host', 'nope'], hookOpts(src[0])).stdout, '');
    // DFW_HOST names the host when no flag does.
    assert.equal(JSON.parse(same(['hook', 'pre-tool-use'], hookOpts(src[0], { env: { DFW_HOST: 'copilot' } })).stdout).permissionDecision, 'deny');
    // gate.allow of the project config opens a path.
    assert.equal(pre([], src[0], { setup: (root) => write(root, '.dfw/config.json', { gate: { allow: ['src/**'] } }) }).stdout, '');
    assert.equal(sameButStderr(['hook', 'pre-tool-use'], hookOpts(src[0], { setup: (root) => write(root, '.dfw/config.json', '{ nope') })).stdout, '');
  });

  test('hook: unknown and missing handler names fail the same way', () => {
    assert.deepEqual([same(['hook', 'nope']).code, same(['hook', 'nope']).stderr], [1, 'Unknown hook "nope"\n']);
    assert.equal(same(['hook']).stderr, 'Unknown hook "undefined"\n');
  });

  test('user-prompt-submit: undelivered comments ride along once (Claude); Copilot gets none here', () => {
    const payload = json((root) => ({ session_id: 's1', hook_event_name: 'UserPromptSubmit', cwd: root, prompt: 'hi' }));
    for (const host of HOSTS) {
      const args = ['hook', 'user-prompt-submit', ...host];
      assert.equal(same(args, hookOpts(payload)).stdout, '');
      const r = same(args, hookOpts(payload, { setup: withSession(MIXED) }));
      if (host[1] === 'copilot') assert.deepEqual([r.stdout, r.files], ['', {}]);
      else {
        assert.match(JSON.parse(r.stdout).hookSpecificOutput.additionalContext, /^2 new Studio comment\(s\) from the user:\n- fb_001 /);
        assert.equal(JSON.parse(r.stdout).hookSpecificOutput.hookEventName, 'UserPromptSubmit');
        assert.deepEqual(Object.keys(r.files), ['.dfw/reviews/.delivered.json']);
      }
      assert.equal(same(args, hookOpts(payload, { setup: all(withSession(MIXED), (root) => write(root, '.dfw/reviews/.delivered.json', ['fb_001:open:2026-01-01T00:00:00.000Z', 'fb_005:reopened:2026-01-01T02:00:00.000Z'])) })).stdout, '');
      for (const state of [null, STATES.paused]) assert.deepEqual(same(args, hookOpts(payload, { state, setup: withSession(MIXED) })).files, {});
      for (const [rel, text] of [['.dfw/state.json', '{ nope'], ['.dfw/reviews/rv_1.json', '{ nope'], ['.dfw/reviews/rv_1.json', '{}'], ['.dfw/reviews/.delivered.json', '{ nope']]) {
        const broken = sameButStderr(args, hookOpts(payload, { setup: all(withSession(MIXED), (root) => write(root, rel, text)) }));
        assert.deepEqual([broken.code, broken.stdout, broken.files], [0, '', {}], rel);
      }
      for (const stdin of ['', 'not json', 'null']) assert.equal(sameButStderr(args, { stdin, setup: withSession(MIXED) }).code, 0);
    }
  });

  test('session-start: status as context, the session id for later commands, no workflow', () => {
    // Never the env file of the session that runs these tests.
    const ss = (payload, opts = {}) => hookOpts(payload, { ...opts, env: { CLAUDE_ENV_FILE: '', ...opts.env } });
    const payload = (id = 's-1') => json((root) => ({ session_id: id, hook_event_name: 'SessionStart', cwd: root, source: 'startup' }));
    for (const host of HOSTS) {
      const args = ['hook', 'session-start', ...host];
      const r = same(args, ss(payload(), { setup: withSession(MIXED) }));
      const context = host[1] === 'copilot' ? JSON.parse(r.stdout).additionalContext : JSON.parse(r.stdout).hookSpecificOutput.additionalContext;
      assert.equal(context, 'dfw: product="Fixture" mode=new-product phase=discovery\ndesign: v1 draft\ngates: G1=open DS=open G2=open G3=open G4=open\nreview comments: 3 open, 1 awaiting user verification\nSession id for "dfw lang": s-1.\nThis project uses the dfw design-first workflow. Before working on its UI, invoke the dfw skill (it asks the communication language once per session and enforces approval gates).');
      for (const state of [FROZEN, STATES.reviewOnly, st({ product: '' })]) same(args, ss(payload(), { state }));
      for (const state of [null, STATES.paused]) assert.equal(same(args, ss(payload(), { state })).stdout, '');
      same(args, ss(json((root) => ({ sessionId: 'camel', cwd: root }))));
      assert.match(same(args, ss(json((root) => ({ cwd: root })))).stdout, /Session id for \\"dfw lang\\": unknown\./);
      same(args, ss(json((root) => ({ cwd: root, session_id: 42 }))));
      for (const stdin of ['', 'not json']) same(args, ss(stdin));
      // 0.5.2: a payload that is not an object is an empty payload (the project comes from the environment).
      same(args, ss('null'));
      for (const [rel, text] of [['.dfw/state.json', '{ nope'], ['.dfw/state.json', '{}'], ['.dfw/reviews/rv_1.json', '{ nope']]) {
        const broken = sameButStderr(args, ss(payload(), { setup: (root) => write(root, rel, text) }));
        assert.deepEqual([broken.code, broken.stdout], [0, ''], rel);
      }
    }
    // Claude Code only: the session id is appended to CLAUDE_ENV_FILE (here under .dfw/, where files are compared).
    const env = { CLAUDE_ENV_FILE: '.dfw/env.sh' };
    assert.equal(same(['hook', 'session-start'], ss(payload("it's-me"), { env })).files['.dfw/env.sh'], "export DFW_SESSION_ID='its-me'\n");
    assert.equal(same(['hook', 'session-start'], ss(payload(), { env, state: null, setup: (root) => write(root, '.dfw/env.sh', 'export A=1\n') })).files['.dfw/env.sh'], "export A=1\nexport DFW_SESSION_ID='s-1'\n");
    assert.deepEqual(same(['hook', 'session-start'], ss(json((root) => ({ cwd: root })), { env })).files, {});
    same(['hook', 'session-start'], ss(payload(), { env: { CLAUDE_ENV_FILE: 'no/such/folder/env.sh' } }));
    // Language sessions untouched for 30 days are pruned.
    const old = (root) => {
      write(root, '.dfw/home/sessions/old.json', '{}');
      write(root, '.dfw/home/sessions/new.json', '{}');
      const longAgo = new Date(Date.now() - 31 * 86400e3);
      fs.utimesSync(path.join(root, '.dfw/home/sessions/old.json'), longAgo, longAgo);
    };
    assert.deepEqual(same(['hook', 'session-start'], ss(payload(), { env: { DFW_HOME: '.dfw/home' }, setup: old })).files, { '.dfw/home/sessions/old.json': null });
  });

  test('post-tool-use: nothing to check (comments for Copilot); a UI edit runs the checks', () => {
    const post = (rel, tool = 'Edit') => json((root) => ({ session_id: 's1', hook_event_name: 'PostToolUse', cwd: root, tool_name: tool, tool_input: { file_path: path.join(root, rel) }, tool_response: { success: true } }));
    const quiet = [post('src/a.ts'), post('README.md'), post('design/approved/v1/screens/a.html'), post('src/a.html', 'Read'), post('.dfw/notes.md'), post('design/notes.txt'), json((root) => ({ cwd: root, tool_name: 'Write', tool_input: { file_path: '/elsewhere/a.html' } })), json((root) => ({ cwd: root }))];
    for (const host of HOSTS) {
      const args = ['hook', 'post-tool-use', ...host];
      for (const payload of quiet) {
        assert.equal(same(args, hookOpts(payload)).stdout, '');
        const r = same(args, hookOpts(payload, { setup: withSession(MIXED) }));
        if (host[1] === 'copilot') {
          assert.match(JSON.parse(r.stdout).additionalContext, /^2 new Studio comment\(s\) from the user:/);
          assert.deepEqual(Object.keys(r.files), ['.dfw/reviews/.delivered.json']);
        } else assert.deepEqual([r.stdout, r.files], ['', {}]);
      }
      // Checks switched off, a paused workflow and no workflow: a UI edit needs nothing either.
      const off = (root) => write(root, '.dfw/config.json', { checks: { onEdit: false } });
      same(args, hookOpts(post('design/screens/items/a.html'), { setup: all(off, withSession(MIXED)) }));
      for (const state of [null, STATES.paused]) assert.deepEqual(same(args, hookOpts(post('design/screens/items/a.html'), { state, setup: withSession(MIXED) })).files, {});
      // Lint runs on the edited screen, tokens are rebuilt after a token edit.
      for (const rel of ['design/screens/items/a.html', 'design/tokens/tokens.json', 'design/system/components/button.html', 'src/App.tsx', 'src/styles.css', 'design/design.json']) {
        same(args, hookOpts(post(rel, 'Write'), { setup: withSession(MIXED) }));
      }
      const bad = (root) => write(root, 'design/screens/items/a.html', '<html><body><img src="x.png"><p>Lorem ipsum</p></body></html>');
      const findings = same(args, hookOpts(post('design/screens/items/a.html'), { setup: bad }));
      assert.match(findings.stdout, /dfw checks on design\/screens\/items\/a\.html:/);
      const both = same(args, hookOpts(post('design/screens/items/a.html'), { setup: all(bad, withSession(MIXED)) }));
      assert.equal(/new Studio comment/.test(both.stdout), host[1] === 'copilot', 'the call with checks delivers the comments too');
      for (const [rel, text] of [['.dfw/state.json', '{ nope'], ['.dfw/config.json', '{ nope']]) {
        const broken = sameButStderr(args, hookOpts(post('design/screens/items/a.html'), { setup: (root) => write(root, rel, text) }));
        assert.deepEqual([broken.code, broken.stdout], [0, ''], rel);
      }
      for (const stdin of ['', 'not json', 'null', '{"tool_name":"Write","tool_input":{"file_path":5}}']) sameButStderr(args, { stdin, setup: withSession(MIXED) });
    }
  });

  test('stop: comments block once; never loops; the layout check runs when it applies', () => {
    const payload = (active = false) => json((root) => ({ session_id: 's1', hook_event_name: 'Stop', cwd: root, stop_hook_active: active }));
    const concepts = st({ phase: 'concepts' });
    for (const host of HOSTS) {
      const args = ['hook', 'stop', ...host];
      assert.equal(same(args, hookOpts(payload())).stdout, '');
      const r = same(args, hookOpts(payload(), { setup: withSession(MIXED) }));
      assert.deepEqual(Object.keys(JSON.parse(r.stdout)), ['decision', 'reason']);
      assert.equal(JSON.parse(r.stdout).decision, 'block');
      assert.match(JSON.parse(r.stdout).reason, /^2 new Studio comment\(s\) from the user:/);
      assert.deepEqual(Object.keys(r.files), ['.dfw/reviews/.delivered.json']);
      assert.deepEqual(same(args, hookOpts(payload(true), { setup: withSession(MIXED) })).files, {}, 'never loops');
      same(args, hookOpts(json((root) => ({ cwd: root, stopHookActive: true })), { setup: withSession(MIXED) }));
      for (const state of [null, STATES.paused, FROZEN, st({ phase: 'direction' })]) assert.equal(same(args, hookOpts(payload(), { state })).stdout, '');
      // In a design phase the layout check runs unless it is switched off...
      const off = (root) => write(root, '.dfw/config.json', { checks: { layoutOnStop: false } });
      assert.deepEqual(same(args, hookOpts(payload(), { state: concepts, setup: off })).files, {});
      // ...here every screen was checked already, so there is nothing to render.
      for (const state of [concepts, st({ phase: 'system' })]) assert.deepEqual(same(args, hookOpts(payload(), { state, setup: (root) => helper(['mark-checked', root]) })).files, {});
      for (const [rel, text] of [['.dfw/state.json', '{ nope'], ['.dfw/reviews/rv_1.json', '{ nope']]) {
        const broken = sameButStderr(args, hookOpts(payload(), { setup: (root) => write(root, rel, text) }));
        assert.deepEqual([broken.code, broken.stdout], [0, ''], rel);
      }
      for (const stdin of ['', 'not json', 'null']) sameButStderr(args, { stdin, state: concepts, setup: all(off, withSession(MIXED)) });
    }
  });
});

// The modules on their own, through the binary's hidden `__debug hot` command, each on its own
// copy of the fixture.
describe('hot: freeze and stack detection (the modules, called directly)', { skip: skipReason() }, () => {
  const tree = (dir) => {
    const out = {};
    const walk = (abs) => {
      for (const ent of fs.readdirSync(abs, { withFileTypes: true })) {
        const p = path.join(abs, ent.name);
        if (ent.isDirectory()) walk(p);
        else out[path.relative(dir, p).split(path.sep).join('/')] = fs.readFileSync(p, 'utf8');
      }
    };
    walk(dir);
    return out;
  };
  /** Run fn(root) on a fresh project folder prepared by setup; returns what fn returns plus the files. */
  function onCopy(setup, fn) {
    const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-hot-')));
    try {
      const root = path.join(tmp, 'project');
      fs.mkdirSync(root);
      setup(root);
      const r = fn(root);
      const mask = (text) => text.split(root).join('<root>');
      return { code: r.status, stdout: mask(r.stdout), stderr: mask(r.stderr), files: tree(root) };
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  }
  const env = { ...process.env, DFW_NOW: NOW, DFW_NATIVE: '', DFW_HOME: path.join(os.tmpdir(), 'dfw-golden-no-home') };
  const native = (what, args) => (root) => spawnSync(nativeBinary(), ['__debug', 'hot', what, root, ...args], { env, encoding: 'utf8' });

  test('freezeDesign: same snapshot, lock file, state and decision log', () => {
    const project = (state, extra = () => {}) => (root) => {
      fs.cpSync(path.join(REPO, 'tests', 'fixtures', 'penpot', 'project'), root, { recursive: true });
      write(root, '.dfw/state.json', state);
      write(root, 'PRODUCT.md', '# Product\n');
      write(root, 'DESIGN.md', '# Design tiếng Việt\n');
      write(root, 'design/handoff/v0.md', 'not part of a snapshot');
      write(root, 'design/approved/v0/old.html', 'an older snapshot');
      extra(root);
    };
    for (const [setup, args] of [
      [project(st({ phase: 'concepts' })), ['First version']],
      [project(st({ phase: 'concepts' })), []],
      [project(st({ phase: 'concepts', design: { version: 3, status: 'draft', locked: false, approved: [{ version: 'v2', hash: 'x', at: NOW }] } })), ['Third']],
      [project(FROZEN), ['again']],
      [project(st(), (root) => fs.rmSync(path.join(root, 'design', 'design.json'))), ['x']],
      [project(st(), (root) => write(root, 'design/approved/v1/left.html', 'x')), ['x']],
    ]) {
      const b = onCopy(setup, native('freeze', args));
      assert.deepEqual(b, expected(b));
    }
    const ok = onCopy(project(st({ phase: 'concepts' })), native('freeze', ['First version']));
    assert.equal(ok.code, 0);
    assert.ok('design/approved/v1/approval.lock.json' in ok.files && 'design/approved/v1/docs/PRODUCT.md' in ok.files);
    assert.ok(!('design/approved/v1/handoff/v0.md' in ok.files) && !('design/approved/v1/approved/v0/old.html' in ok.files));
  });

  // Studio, hooks and commands are separate processes that write one project at once. Four
  // binary writers (the reference also ran two of its own) add 30 comments each.
  test('the reviews lock holds between concurrent writers on one project', async () => {
    const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-lock-')));
    try {
      const root = path.join(tmp, 'project');
      write(root, '.dfw/state.json', st());
      const done = (child) => new Promise((resolve) => child.on('exit', resolve));
      const writers = ['a', 'b', 'c', 'd'].map((tag) => spawn(nativeBinary(), ['__debug', 'hot', 'add-comments', root, `writer-${tag}`, '30'], { env, stdio: 'ignore' }));
      assert.deepEqual(await Promise.all(writers.map(done)), [0, 0, 0, 0]);
      const sessions = fs.readdirSync(path.join(root, '.dfw', 'reviews')).filter((f) => f.startsWith('rv_'));
      const stored = sessions.flatMap((f) => JSON.parse(fs.readFileSync(path.join(root, '.dfw', 'reviews', f), 'utf8')).comments);
      assert.equal(stored.length, 120);
      assert.equal(new Set(stored.map((c) => c.id)).size, 120);
      assert.equal(new Set(stored.map((c) => c.comment)).size, 120);
      assert.ok(!fs.existsSync(path.join(root, '.dfw', 'reviews', '.lock')));
    } finally {
      fs.rmSync(tmp, { recursive: true, force: true });
    }
  });

  test('detectStack: same result for JavaScript, other and static projects', () => {
    const files = (map) => (root) => Object.entries(map).forEach(([rel, text]) => write(root, rel, text));
    const pkg = (p) => JSON.stringify(p);
    const projects = [
      {},
      { 'package.json': pkg({ dependencies: { next: '15.1.0', react: '^19.0.0', '@mui/material': '6.0.0', '@radix-ui/react-slot': '1' }, devDependencies: { tailwindcss: '4.0.0', sass: '1.80.0', typescript: '5.7.2' } }), 'components.json': '{}', 'pnpm-lock.yaml': '', 'yarn.lock': '' },
      { 'package.json': pkg({ dependencies: { vue: '3.5.0', vuetify: '3.7.0', react: '' }, devDependencies: { vue: '3.6.0-beta' } }), 'tsconfig.json': '{}', 'bun.lock': '', 'package-lock.json': '{}' },
      { 'package.json': pkg({ name: 'empty' }), 'Cargo.toml': '', 'index.html': '' },
      { 'package.json': pkg({ dependencies: ['odd'], devDependencies: 'text' }) },
      { 'composer.json': '{}', Gemfile: '', 'go.mod': '' },
      { 'manage.py': '', 'pyproject.toml': '' },
      { 'pubspec.yaml': '' },
      { 'index.html': '', 'about.html': '', 'readme.md': '' },
      { 'package.json': '{ nope' },
    ];
    for (const map of projects) {
      const clock = (r) => ({ ...r, stdout: r.stdout.replace(/"detectedAt":"[^"]+"/, '"detectedAt":"<now>"'), stderr: r.code ? '' : r.stderr });
      // Directory order is the file system's: the evidence of a static site is compared as a set.
      const sorted = (r) => (r.code ? r : { ...r, stdout: JSON.stringify({ ...JSON.parse(r.stdout), evidence: JSON.parse(r.stdout).evidence.sort() }) });
      const b = sorted(clock(onCopy(files(map), native('stack-detect', []))));
      assert.deepEqual(b, expected(b), JSON.stringify(Object.keys(map)));
    }
  });
});
