// Core modules: config (core::config) and host (core::host). Neither has a CLI command of its
// own, so the binary's hidden `__debug` command prints what the module computes; the recorded
// results are what the Node modules (core/lib/config.mjs, host.mjs of 0.5.2) computed.
// Run: node --test tests/golden/
import { describe } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { nativeBinary, skipReason, suite, expected, REPO } from './harness.mjs';

const test = suite('core');

/** Hosts of `core::host` (the HOSTS table of host.mjs) and the text placeholders they render. */
const HOSTS = ['claude', 'copilot', 'mcp', 'opencode', 'codex', 'cursor'];
const PLACEHOLDERS = ['HOST_NAME', 'SKILL_DIR', 'ASK_TOOL', 'BACKGROUND', 'HOST_SETTINGS', 'DFW_CLI', 'INVOKE_SKILL'];

function run(cmd, args, { env = {}, stdin = '' } = {}) {
  const clean = { ...process.env };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED', 'DFW_HOME', 'DFW_HOST', 'XDG_CONFIG_HOME']) delete clean[k];
  const r = spawnSync(cmd, args, { env: { ...clean, ...env }, input: stdin, encoding: 'utf8', timeout: 60_000 });
  if (r.error) throw r.error;
  return { code: r.status, stdout: r.stdout, stderr: r.stderr };
}

/** Merged config of a project, with the recorded result. files: { global, project, local } (JSON text or values). */
function loadBoth(files, { withRoot = true } = {}) {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-core-')));
  try {
    const root = path.join(tmp, 'project');
    const home = path.join(tmp, 'home');
    const write = (file, content) => {
      if (content === undefined) return;
      fs.mkdirSync(path.dirname(file), { recursive: true });
      fs.writeFileSync(file, typeof content === 'string' ? content : JSON.stringify(content, null, 2));
    };
    fs.mkdirSync(root, { recursive: true });
    write(path.join(home, 'config.json'), files.global);
    write(path.join(root, '.dfw', 'config.json'), files.project);
    write(path.join(root, '.dfw', 'config.local.json'), files.local);
    const env = { DFW_HOME: home };
    const rootArg = withRoot ? [root] : [];
    const mask = (r) => ({ code: r.code, stdout: r.stdout.split(tmp).join('<tmp>'), stderr: r.stderr.split(tmp).join('<tmp>').split(REPO).join('<repo>') });
    const native = mask(run(nativeBinary(), ['__debug', 'config', ...rootArg], { env }));
    return { want: expected(native), native };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function assertSameConfig(files, opts) {
  const { want, native } = loadBoth(files, opts);
  assert.equal(want.code, 0, want.stderr);
  assert.equal(native.code, 0, native.stderr);
  assert.equal(native.stdout, want.stdout);
  return JSON.parse(native.stdout);
}

describe('core: config', { skip: skipReason() }, () => {
  test('no config files: the defaults, with and without a project root', () => {
    const c = assertSameConfig({});
    assert.deepEqual(c.gate.allow, ['.dfw/**', 'design/handoff/**']);
    assertSameConfig({}, { withRoot: false });
  });

  test('project config only', () => {
    const c = assertSameConfig({ project: { schema: 1, checks: { onEdit: false, timeoutMs: 2500.5 }, studio: { port: 4700 }, custom: { z: 1, a: [1, 2, { b: null }] } } });
    assert.equal(c.checks.onEdit, false);
    assert.equal(c.studio.breakpoints.length, 3);
  });

  test('project and project-local config: local wins, new keys keep their order', () => {
    const c = assertSameConfig({
      project: { studio: { port: 4700, breakpoints: [{ id: 'wide', label: 'Wide', width: 1920, height: 1080 }] }, review: { shortcut: 'ctrl+shift+k' }, env: { MY_TOOL: 'x', DO_NOT_TRACK: '0' } },
      local: { studio: { port: 4800 }, language: { default: 'vi' }, extra: { nested: { deep: true } }, env: { LOCAL_ONLY: 'tiếng Việt "quoted"  ' } },
    });
    assert.equal(c.studio.port, 4800);
    assert.equal(c.studio.breakpoints[0].id, 'wide');
  });

  test('gate.allow, gate.immutable and skills.disabled are unions across layers', () => {
    const c = assertSameConfig({
      global: { gate: { allow: ['global/**'] }, skills: { disabled: ['hallmark'] } },
      project: { gate: { allow: ['docs/**', '.dfw/**'], immutable: ['vendor/**'], designPaths: ['ui/**'] }, skills: { disabled: ['taste-reference', 'hallmark'] } },
      local: { gate: { allow: ['notes/**', 'docs/**', 'global/**'], immutable: ['design/approved/**', 'dist/**'] } },
    });
    assert.deepEqual(c.gate.allow, ['.dfw/**', 'design/handoff/**', 'global/**', 'docs/**', 'notes/**']);
    assert.deepEqual(c.gate.immutable, ['design/approved/**', 'vendor/**', 'dist/**']);
    assert.deepEqual(c.gate.designPaths, ['ui/**']);
    assert.deepEqual(c.skills.disabled, ['hallmark', 'taste-reference']);
    assertSameConfig({ global: { gate: { allow: ['global/**'] }, studio: { port: 4650 } } }, { withRoot: false });
  });

  test('odd values merge by the JavaScript rules', () => {
    assertSameConfig({
      project: '{"gate":{"allow":"docs/**","immutable":[1,{"a":1},null,"x","x"]},"skills":"ab","__proto__":{"x":1},"checks":{"__proto__":{"y":1},"maxFindings":1e21}}',
      local: '{"gate":{"allow":0,"immutable":[1.0,{"a":1},null,false]},"studio":null,"skills":{"disabled":"one"},"env":[1,2]}',
    });
    assertSameConfig({ project: '{"gate":null,"skills":[["x"],"y"]}' });
    assertSameConfig({ project: '{"gate":"text","skills":7}', local: '{"skills":{"disabled":[["a"],["a"]]}}' });
    // 0.5.2: a layer that is not an object is an error to fix, no longer replaced by the next one.
    for (const layers of [{ project: '[1,2]', local: '{"a":{"b":1}}' }, { global: '"text"', project: '{}' }]) {
      const { want, native } = loadBoth(layers);
      assert.deepEqual([want.code, native.code], [1, 1]);
      assert.match(native.stderr, /^Config file .*config\.json must hold a JSON object\. Fix it or delete it\.\n$/);
      assert.match(want.stderr, /Config file .*config\.json must hold a JSON object\. Fix it or delete it\./);
    }
  });

  test('broken JSON fails in both', () => {
    const { want, native } = loadBoth({ project: '{ nope' });
    assert.equal(want.code, 1);
    assert.equal(native.code, 1);
    assert.match(native.stderr, /^Error: Invalid JSON in .*config\.json: /);
    assert.match(want.stderr, /Error: Invalid JSON in .*config\.json: /);
  });
});

describe('core: host', { skip: skipReason() }, () => {
  const EVENTS = ['SessionStart', 'UserPromptSubmit', 'PreToolUse', 'PostToolUse', 'Stop'];
  const RESULTS = [
    null,
    {},
    { context: 'Some context\nwith "quotes" and tiếng Việt' },
    { deny: 'Not yet: approve the design first.' },
    { block: 'Open comments remain.' },
    { context: 'c', deny: 'd' },
    { context: 'c', deny: 'd', block: 'b' },
    { context: '', deny: '', block: '' },
  ];

  /** The recorded result: `{ code: 1, stderr }` where the function threw, else `{ code: 0, stdout }`. */
  const compare = (label, native) => {
    const seen = native.code === 0 ? { code: 0, stdout: native.stdout } : { code: native.code, stderr: native.stderr };
    assert.deepEqual(seen, expected(seen), label);
  };

  test('hook output is identical for every host, event and result kind', () => {
    for (const host of [...HOSTS, 'no-such-host']) {
      for (const event of EVENTS) {
        for (const result of RESULTS) {
          const label = `${host} ${event} ${JSON.stringify(result)}`;
          const native = run(nativeBinary(), ['__debug', 'host-format', host, event], { stdin: JSON.stringify(result) });
          compare(label, native);
        }
      }
    }
  });

  test('skill text renders the same for every host', () => {
    const placeholders = PLACEHOLDERS.map((k) => `${k}: {{${k}}}`).join('\n');
    const blocks = '<!-- host:claude -->\nclaude only $1 $&\n<!-- /host -->\n<!--host:copilot,mcp-->\ncopilot and mcp\n<!--/host-->\n<!-- host:codex -->planned<!-- /host -->';
    const names = 'Use `impeccable:impeccable`, `dfw:screen-builder`, `other:skill` and `ux-research:user-research`. {{UNKNOWN}} {{lower}}';
    for (const host of [...HOSTS, 'no-such-host']) {
      for (const text of [`${placeholders}\n${blocks}\n${names}\n`, `${blocks}\n${names}`, '']) {
        const native = run(nativeBinary(), ['__debug', 'host-render', host], { stdin: text });
        compare(`${host} ${JSON.stringify(text.slice(0, 30))}`, native);
      }
    }
  });
});
