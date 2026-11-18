// Regression tests for the defects fixed in the last Node release before the port (one per fix; each
// fails without its fix). The letters match the defect list of that release.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

import { newSession, addComment, updateComment, allComments, locateSource } from '../../core/lib/comments.mjs';
import { undelivered } from '../../core/lib/inbox.mjs';
import { newState, saveState, loadState, setPhase, setMode, approveGate } from '../../core/lib/state.mjs';
import { decideWrite } from '../../core/lib/gate.mjs';
import { loadConfig, getConfigValue, setConfigValue, DEFAULTS } from '../../core/lib/config.mjs';
import { verifySnapshot } from '../../core/lib/freeze.mjs';
import { getHost, normalizeHookInput, formatHookOutput, renderText } from '../../core/lib/host.mjs';
import { parseArgs, getPath } from '../../core/lib/util.mjs';
import { addIcons } from '../../core/lib/icons.mjs';

const DFW = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../core/bin/dfw.mjs');

function project() {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-stable-')));
  process.env.DFW_HOME = path.join(root, '.home');
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  saveState(root, newState({ mode: 'new-product' }));
  return root;
}
const env = (root) => {
  const e = { ...process.env, DFW_HOME: path.join(root, '.home'), CLAUDE_PROJECT_DIR: root };
  delete e.DFW_HOST;
  delete e.DFW_PROJECT_DIR;
  return e;
};
/** Run the CLI: { status, stdout, stderr }. */
const cli = (root, args, input) => spawnSync(process.execPath, [DFW, ...args], { cwd: root, env: env(root), input, encoding: 'utf8' });
const put = (root, rel, data) => {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), typeof data === 'string' ? data : JSON.stringify(data));
};
const readJsonFile = (root, rel) => JSON.parse(fs.readFileSync(path.join(root, rel), 'utf8'));
/** The error is the user's (DfwError): its message matches and no stack trace was printed. */
const assertUserError = (r, re) => {
  assert.equal(r.status, 1, r.stderr);
  assert.match(r.stderr, re);
  assert.doesNotMatch(r.stderr, /TypeError|\n\s+at /);
};

test('a: review rounds started in the same second get different session files', () => {
  const root = project();
  const first = newSession(root);
  addComment(root, { comment: 'kept' });
  const ids = [first, newSession(root), newSession(root)].map((s) => s.session.id);
  assert.equal(new Set(ids).size, 3, ids.join(' '));
  assert.ok(ids.every((id) => /^rv_\d{8}T\d{6}$/.test(id)), 'the id keeps its form');
  assert.deepEqual([...ids].sort(), ids, 'file names sort in creation order');
  assert.deepEqual(allComments(root).map((c) => c.comment), ['kept'], 'the first round and its comment survive');
  assert.equal(fs.readdirSync(path.join(root, '.dfw', 'reviews')).filter((f) => f.startsWith('rv_')).length, 3);
});

test('b: --level and --timeout without a value are refused', () => {
  const root = project();
  assertUserError(cli(root, ['decide', 'use', 'tabs', '--level']), /^--level needs a value\.$/m);
  assert.ok(!fs.existsSync(path.join(root, '.dfw', 'decisions.jsonl')), 'no level 1 decision was recorded');
  const started = Date.now();
  assertUserError(cli(root, ['comments', 'wait', '--timeout']), /^--timeout needs a value\.$/m);
  assert.ok(Date.now() - started < 900, 'it did not wait');
});

test('c: phase --force never writes a missing or unknown phase', () => {
  const root = project();
  assertUserError(cli(root, ['phase', '--force']), /Unknown phase "undefined"/);
  assertUserError(cli(root, ['phase', 'bogus', '--force']), /Unknown phase "bogus"/);
  assert.equal(loadState(root).phase, 'discovery');
  assert.throws(() => setPhase(root, loadState(root), 'bogus', { force: true }), /Unknown phase "bogus"/);
  assert.equal(setPhase(root, loadState(root), 'concepts', { force: true, reason: 'test' }).phase, 'concepts', '--force still overrides gate blockers');
});

test('d: the write gate applies to a file whose name starts with two dots', () => {
  const root = project();
  const decide = (f) => decideWrite({ root, state: loadState(root), config: loadConfig(root), absFile: f }).allow;
  assert.equal(decide('..x.ts'), false);
  assert.equal(decide(path.join(root, '..x', 'y.ts')), false);
  assert.equal(decide('...'), false);
  assert.equal(decide('../x.ts'), true, 'outside the project');
  assert.equal(decide('..'), true, 'outside the project');
});

test('e: a comment file or snapshot version from input never leaves design/', () => {
  const root = project();
  put(root, 'outside.html', '<p data-dfw-id="a">x</p>');
  put(root, 'design/screens/home/a.html', '<main>\n<p data-dfw-id="a">x</p>\n</main>');
  for (const bad of ['../outside.html', 'screens/../../outside.html', 'screens\\..\\..\\outside.html', path.join(root, 'outside.html'), 'C:\\outside.html', { a: 1 }]) {
    assert.equal(locateSource(root, bad, 'a'), null, JSON.stringify(bad));
    const c = addComment(root, { comment: 'x', file: bad, target: { dfwId: 'a' } });
    assert.equal(c.file, null, JSON.stringify(bad));
    assert.equal(c.target.source, null, JSON.stringify(bad));
  }
  const ok = addComment(root, { comment: 'x', file: 'screens/home/a.html', target: { dfwId: 'a' } });
  assert.equal(ok.file, 'design/screens/home/a.html');
  assert.equal(ok.target.source, 'design/screens/home/a.html:2');

  put(root, 'snap/approval.lock.json', { files: [] });
  assert.throws(() => verifySnapshot(root, '../../snap'), /Unknown snapshot "\.\.\/\.\.\/snap"/);
  assertUserError(cli(root, ['verify-snapshot', '../../snap']), /^Unknown snapshot "\.\.\/\.\.\/snap"\. A snapshot is named v<number>, for example v1\.$/m);
});

test('f: Copilot post-tool-use keeps comments undelivered when the checks throw', () => {
  const root = project();
  addComment(root, { comment: 'Make it bigger' });
  // A file path that is not text makes the edited-files step throw.
  const r = cli(root, ['hook', 'post-tool-use', '--host', 'copilot'], JSON.stringify({ cwd: root, tool_name: 'Write', tool_input: { file_path: 123 } }));
  assert.equal(r.status, 0, 'the hook fails open');
  assert.match(r.stderr, /dfw hook post-tool-use error/);
  assert.equal(r.stdout, '');
  assert.equal(undelivered(root).length, 1, 'the comment is still waiting for the next hook');
  const next = cli(root, ['hook', 'post-tool-use', '--host', 'copilot'], JSON.stringify({ cwd: root, tool_name: 'Read', tool_input: {} }));
  assert.match(JSON.parse(next.stdout).additionalContext, /Make it bigger/);
});

test('g: config set without a value is a usage error and writes nothing', () => {
  const root = project();
  for (const args of [['config', 'set', 'a.b'], ['config', 'set']]) assertUserError(cli(root, args), /^Usage: dfw config set <key> <value>/m);
  assert.ok(!fs.existsSync(path.join(root, '.dfw', 'config.json')));
});

test('h: approve G3 refuses a stack.json that is not an object', () => {
  const root = project();
  for (const content of ['[1,2]', '"next"', '5', 'null']) {
    put(root, '.dfw/stack.json', content);
    assertUserError(cli(root, ['approve', 'G3', '--note', 'ok']), /^G3 needs a stack proposal: \.dfw\/stack\.json must hold a JSON object/m);
    assert.equal(fs.readFileSync(path.join(root, '.dfw', 'stack.json'), 'utf8'), content, 'the file is left as it was');
    assert.equal(loadState(root).gates.G3, null);
  }
  put(root, '.dfw/stack.json', { framework: { name: 'x' } });
  assert.equal(cli(root, ['approve', 'G3', '--note', 'ok']).status, 0);
  assert.equal(readJsonFile(root, '.dfw/stack.json').status, 'approved');
});

test('i: setting the status a comment already has is refused', () => {
  const root = project();
  const c = addComment(root, { comment: 'x' });
  updateComment(root, c.id, { status: 'fixed', message: 'done' });
  updateComment(root, c.id, { status: 'resolved', by: 'user' });
  assert.throws(() => updateComment(root, c.id, { status: 'resolved', message: 'again', by: 'claude' }), /cannot go from "resolved" to "resolved" \(allowed: reopened\)/);
  assert.throws(() => updateComment(root, c.id, { status: 'resolved', by: 'user' }), /cannot go from "resolved" to "resolved"/);
  const after = allComments(root)[0];
  assert.equal(after.thread.length, 1, 'no message was added');
  updateComment(root, c.id, { message: 'a reply without a status still works' });
  assert.equal(allComments(root)[0].thread.length, 2);
});

test('j: names that are prototype keys are not ids', () => {
  const root = project();
  for (const name of ['constructor', 'toString', '__proto__', 'hasOwnProperty']) {
    assert.throws(() => setMode(root, loadState(root), name), new RegExp(`Unknown mode "${name}"\\.`), name);
    assert.throws(() => newState({ mode: name }), new RegExp(`Unknown mode "${name}"\\.`), name);
    assert.throws(() => getHost(name), new RegExp(`Unknown host "${name}"\\.`), name);
    assert.throws(() => approveGate(root, loadState(root), name, { note: 'x' }), new RegExp(`Unknown gate "${name}"\\.`), name);
    assert.equal(normalizeHookInput({ tool_name: name }).tool_name, name, 'a tool keeps its own name');
    assert.equal(getPath(DEFAULTS, name), undefined);
    assert.equal(getPath(DEFAULTS, `studio.${name}`), undefined);
  }
  assertUserError(cli(root, ['mode', 'constructor']), /^Unknown mode "constructor"\. Modes: /m);
  for (const key of ['__proto__.x', 'a.constructor', 'a.prototype.b']) assertUserError(cli(root, ['config', 'set', key, '1']), /^Invalid key ".*": "(__proto__|constructor|prototype)" is a reserved name\.$/m);
  assert.ok(!fs.existsSync(path.join(root, '.dfw', 'config.json')));
  assert.match(cli(root, ['hook', 'constructor'], '{"block":"x"}').stderr, /Unknown hook "constructor"/);
  assert.match(cli(root, ['mcp', 'install', 'constructor', '--print']).stderr, /Unknown tool "constructor"\. MCP install targets: /);
  assert.deepEqual(addIcons(root, ['constructor', 'toString']).added, [], 'not icons');
  assert.ok(!fs.existsSync(path.join(root, 'design', 'system', 'icons', 'constructor.svg')));

  // A "__proto__" key in a config file is dropped, in the last layer too.
  put(root, '.dfw/config.local.json', '{"__proto__":{"polluted":"yes"},"constructor":{"x":1},"studio":{"__proto__":{"port":1}}}');
  const config = loadConfig(root, { warn: () => {} });
  assert.equal(config.polluted, undefined);
  assert.equal(Object.getPrototypeOf(config), Object.prototype);
  assert.equal(Object.hasOwn(config, 'constructor'), false);
  assert.equal(config.studio.port, 4600);
  assert.equal(getConfigValue(root, 'polluted'), undefined);
  assert.equal(getConfigValue(root, 'toString'), undefined);
  assert.equal(getConfigValue(root, 'studio.breakpoints.0.id'), 'desktop', 'a list is read by index');
  assert.equal(getConfigValue(root, 'studio.breakpoints.length'), undefined);

  // Flags named "_" or a reserved key are dropped with their value.
  assert.deepEqual(parseArgs(['a', '--_=x', 'b']), { _: ['a', 'b'] });
  assert.deepEqual(parseArgs(['a', '--_', 'x', 'b']), { _: ['a', 'b'] });
  const args = parseArgs(['--__proto__', 'x', '--constructor', '--prototype=1', 'y', '--json']);
  assert.deepEqual(args, { _: ['y'], json: true });
  assert.deepEqual(Object.keys(args), ['_', 'json']);
});

test('k: a config file that is not a JSON object is a clear error', () => {
  const root = project();
  for (const content of ['[1,2]', '"text"', '5', 'null', 'true']) {
    put(root, '.dfw/config.json', content);
    assert.throws(() => loadConfig(root), (e) => e.exitCode === 1 && /config\.json must hold a JSON object\. Fix it or delete it\.$/.test(e.message), content);
    assertUserError(cli(root, ['config', 'get', 'studio.port']), /^Config file .*config\.json must hold a JSON object\. Fix it or delete it\.$/m);
    assertUserError(cli(root, ['config', 'set', 'x', '1']), /must hold a JSON object/);
    assert.equal(fs.readFileSync(path.join(root, '.dfw', 'config.json'), 'utf8'), content, 'the file is left as it was');
  }
  put(root, '.dfw/config.json', '{}');
  put(root, '.home/config.json', '[]');
  assert.throws(() => loadConfig(root), /\.home.config\.json must hold a JSON object/);
});

test('l: host helpers answer with their own messages, not JavaScript errors', () => {
  const root = project();
  assert.deepEqual(formatHookOutput('mcp', 'SessionStart', { context: 'x' }), { additionalContext: 'x' });
  assert.deepEqual(formatHookOutput('opencode', 'PostToolUse', { context: 'x' }), { additionalContext: 'x' });
  assert.equal(formatHookOutput('copilot', 'UserPromptSubmit', { context: 'x' }), null, 'a context table still decides');
  assert.throws(() => renderText('a {{ASK_TOOL}}', 'opencode'), /^Error: Host "opencode" has no text placeholders\. Hosts with text: claude, copilot, mcp$/);
  for (const raw of [null, [], 'text', 5]) {
    const n = normalizeHookInput(raw);
    assert.equal(n.tool_name, undefined);
    assert.deepEqual(n.tool_input, {});
    assert.equal(n.stop_hook_active, false);
  }
  const hook = cli(root, ['hook', 'session-start'], 'null');
  assert.equal(hook.status, 0);
  assert.equal(hook.stderr, '');
  assert.match(JSON.parse(hook.stdout).hookSpecificOutput.additionalContext, /phase=discovery/);
  const bare = cli(root, ['hook', 'session-start', '--host'], '{}');
  assert.equal(bare.status, 0, 'a hook fails open');
  assert.equal(bare.stderr, 'dfw hook session-start error: --host needs a value.\n');
  assertUserError(cli(root, ['install', '--host']), /^--host needs a value\.$/m);
});

test('m: a flag that takes a value is refused without one', () => {
  const root = project();
  for (const [flag, ...args] of [['root', 'status'], ['port', 'studio', 'status'], ['note', 'approve', 'G1'], ['reason', 'reopen'], ['message', 'comments', 'reply', 'fb_001'], ['session', 'lang', 'set', 'vi'], ['status', 'comments', 'list'], ['url', 'review', 'start']]) {
    assertUserError(cli(root, [...args, `--${flag}`]), new RegExp(`^--${flag} needs a value\\.$`, 'm'));
  }
  assert.equal(loadState(root).gates.G1, null, 'no approval with the note "true"');
  assert.equal(cli(root, ['status', '--json', '--root', root]).status, 0, 'a value, and flags without one, still work');
  assert.equal(cli(root, ['comments', 'list', '--status=open']).status, 0);
});

test('n: config set refuses a key that goes through a list', () => {
  const root = project();
  assert.equal(cli(root, ['config', 'set', 'gate.allow', '["src/**"]']).status, 0);
  assertUserError(cli(root, ['config', 'set', 'gate.allow.0.b', '5']), /^Cannot set "gate\.allow\.0\.b": "gate\.allow" is a list; set the whole list instead\.$/m);
  assertUserError(cli(root, ['config', 'set', 'gate.allow.extra', '5']), /"gate\.allow" is a list/);
  assert.deepEqual(readJsonFile(root, '.dfw/config.json'), { gate: { allow: ['src/**'] } });
});

test('o: a tool name that is not a string names no tool', () => {
  for (const tool of [['Write'], { name: 'Write' }, 5, true]) {
    assert.equal(normalizeHookInput({ tool_name: tool }).tool_name, undefined, JSON.stringify(tool));
    assert.equal(normalizeHookInput({ toolName: tool }).tool_name, undefined, JSON.stringify(tool));
  }
  assert.equal(normalizeHookInput({ tool_name: 'create' }).tool_name, 'Write');
  assert.equal(normalizeHookInput({ toolName: 'Bash' }).tool_name, 'Bash');
});

test('p: a Studio pid that is not a positive integer is never signalled', () => {
  const root = project();
  // -1 would address every process of the user: "running" without the fix (the test only reads status).
  for (const pid of [-1, 0, -5, 1.5, '12', true, null]) {
    put(root, '.dfw/studio.json', { pid, url: 'http://127.0.0.1:4600/' });
    assert.equal(JSON.parse(cli(root, ['status', '--json']).stdout).studio, null, String(pid));
    assert.equal(cli(root, ['studio', 'status']).stdout, 'studio not running\n', String(pid));
  }
  put(root, '.dfw/studio.json', { pid: process.pid, url: 'http://127.0.0.1:4600/' });
  assert.equal(JSON.parse(cli(root, ['status', '--json']).stdout).studio.pid, process.pid);
});

// ---- findings of the code review of this release ----

test('review: "dfw check --setup" exits 1 when the setup was refused', () => {
  const root = project();
  put(root, 'fake-engine', 'not an engine');
  // IMPECCABLE_BIN names an engine dfw cannot verify: setup stops before anything is run or downloaded.
  const r = spawnSync(process.execPath, [DFW, 'check', '--setup'], { cwd: root, encoding: 'utf8', env: { ...env(root), IMPECCABLE_HOME: path.join(root, 'no-engine'), IMPECCABLE_BIN: path.join(root, 'fake-engine') } });
  assertUserError(r, /^Impeccable setup stopped: /m);
  assert.equal(r.stdout, '');
});

test('review: .dfw/presence.json is ignored by git in a new project', () => {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-stable-')));
  assert.equal(cli(root, ['init', '--root', root]).status, 0);
  assert.ok(fs.readFileSync(path.join(root, '.dfw', '.gitignore'), 'utf8').split('\n').includes('presence.json'));
});
