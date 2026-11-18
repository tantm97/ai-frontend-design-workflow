// Core modules of the native port: config (core/lib/config.mjs) and host (core/lib/host.mjs).
// Neither has a CLI command of its own yet, so the binary's hidden `__debug` command prints
// what the Rust module computes and the Node module is called directly.
// Run: DFW_BIN=native/target/debug/dfw node --test tests/conformance/
import { describe, test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { nativeBinary, skipReason, REPO } from './harness.mjs';
import { HOSTS, formatHookOutput, renderText } from '../../core/lib/host.mjs';

const CONFIG_URL = pathToFileURL(path.join(REPO, 'core', 'lib', 'config.mjs')).href;
// loadConfig reads DFW_HOME when it runs, so Node gets its own process with the same environment.
const NODE_LOAD = `import { loadConfig } from ${JSON.stringify(CONFIG_URL)}; console.log(JSON.stringify(loadConfig(process.argv[1] || null)));`;

function run(cmd, args, { env = {}, stdin = '' } = {}) {
  const clean = { ...process.env };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED', 'DFW_HOME', 'DFW_HOST', 'XDG_CONFIG_HOME']) delete clean[k];
  const r = spawnSync(cmd, args, { env: { ...clean, ...env }, input: stdin, encoding: 'utf8', timeout: 60_000 });
  if (r.error) throw r.error;
  return { code: r.status, stdout: r.stdout, stderr: r.stderr };
}

/** Merged config of a project from both implementations. files: { global, project, local } (JSON text or values). */
function loadBoth(files, { withRoot = true } = {}) {
  const tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-conformance-core-')));
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
    return {
      node: run(process.execPath, ['--input-type=module', '-e', NODE_LOAD, ...rootArg], { env }),
      native: run(nativeBinary(), ['__debug', 'config', ...rootArg], { env }),
    };
  } finally {
    fs.rmSync(tmp, { recursive: true, force: true });
  }
}

function assertSameConfig(files, opts) {
  const { node, native } = loadBoth(files, opts);
  assert.equal(node.code, 0, node.stderr);
  assert.equal(native.code, 0, native.stderr);
  assert.equal(native.stdout, node.stdout);
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
    // A layer that is not an object is replaced by the next one.
    assertSameConfig({ project: '[1,2]', local: '{"a":{"b":1}}' });
    assertSameConfig({ global: '"text"', project: '{}' });
  });

  test('broken JSON fails in both', () => {
    const { node, native } = loadBoth({ project: '{ nope' });
    assert.equal(node.code, 1);
    assert.equal(native.code, 1);
    assert.match(native.stderr, /^Error: Invalid JSON in .*config\.json: /);
    assert.match(node.stderr, /Error: Invalid JSON in .*config\.json: /);
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

  /** What Node's function does: the value, or the error message when it throws. */
  const attempt = (fn) => {
    try {
      return { value: fn() };
    } catch (err) {
      return { error: err.message };
    }
  };

  const compare = (label, expected, native, print) => {
    if ('error' in expected) {
      assert.equal(native.code, 1, `${label}: Node throws "${expected.error}"`);
      assert.equal(native.stderr, `Error: ${expected.error}\n`, label);
    } else {
      assert.equal(native.code, 0, `${label}: ${native.stderr}`);
      assert.equal(native.stdout, print(expected.value), label);
    }
  };

  test('hook output is identical for every host, event and result kind', () => {
    for (const host of [...Object.keys(HOSTS), 'no-such-host']) {
      for (const event of EVENTS) {
        for (const result of RESULTS) {
          const label = `${host} ${event} ${JSON.stringify(result)}`;
          const native = run(nativeBinary(), ['__debug', 'host-format', host, event], { stdin: JSON.stringify(result) });
          compare(label, attempt(() => formatHookOutput(host, event, result)), native, (v) => `${JSON.stringify(v)}\n`);
        }
      }
    }
  });

  test('skill text renders the same for every host', () => {
    const placeholders = Object.keys(HOSTS.claude.text).map((k) => `${k}: {{${k}}}`).join('\n');
    const blocks = '<!-- host:claude -->\nclaude only $1 $&\n<!-- /host -->\n<!--host:copilot,mcp-->\ncopilot and mcp\n<!--/host-->\n<!-- host:codex -->planned<!-- /host -->';
    const names = 'Use `impeccable:impeccable`, `dfw:screen-builder`, `other:skill` and `ux-research:user-research`. {{UNKNOWN}} {{lower}}';
    for (const host of [...Object.keys(HOSTS), 'no-such-host']) {
      for (const text of [`${placeholders}\n${blocks}\n${names}\n`, `${blocks}\n${names}`, '']) {
        const native = run(nativeBinary(), ['__debug', 'host-render', host], { stdin: text });
        compare(`${host} ${JSON.stringify(text.slice(0, 30))}`, attempt(() => renderText(text, host)), native, (v) => `${v}\n`);
      }
    }
  });
});
