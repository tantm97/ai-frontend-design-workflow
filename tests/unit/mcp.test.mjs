import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn, execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { mcpConfigText } from '../../core/lib/mcp.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const DFW = path.join(REPO, 'plugins', 'dfw', 'bin', 'dfw.mjs');

/** Talk JSON-RPC to "dfw mcp" over stdio. */
async function session(env, fn) {
  const child = spawn(process.execPath, [DFW, 'mcp'], { env, stdio: ['pipe', 'pipe', 'inherit'] });
  const waiting = new Map();
  let buf = '';
  child.stdout.on('data', (d) => {
    buf += d;
    let i;
    while ((i = buf.indexOf('\n')) >= 0) {
      const m = JSON.parse(buf.slice(0, i));
      buf = buf.slice(i + 1);
      waiting.get(m.id)?.(m);
    }
  });
  let id = 0;
  const call = (method, params) =>
    new Promise((resolve) => {
      const n = ++id;
      waiting.set(n, resolve);
      child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', id: n, method, params })}\n`);
    });
  try {
    await fn(call, (method) => child.stdin.write(`${JSON.stringify({ jsonrpc: '2.0', method })}\n`));
  } finally {
    child.stdin.end();
    await new Promise((r) => child.on('close', r));
  }
}

test('mcp server: initialize, guide, CLI tool, write gate', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-mcp-'));
  const env = { ...process.env, DFW_HOME: path.join(root, '.home'), CLAUDE_PROJECT_DIR: '' };
  execFileSync(process.execPath, [DFW, 'init', '--root', root], { cwd: root, env });
  await session(env, async (call, notify) => {
    const init = await call('initialize', { protocolVersion: '2025-06-18', capabilities: {}, clientInfo: { name: 't', version: '1' } });
    assert.equal(init.result.serverInfo.name, 'dfw');
    assert.match(init.result.instructions, /dfw_can_write/);
    notify('notifications/initialized'); // no reply expected
    const tools = (await call('tools/list')).result.tools.map((t) => t.name);
    assert.deepEqual(tools, ['dfw_guide', 'dfw', 'dfw_can_write']);

    const g = (await call('tools/call', { name: 'dfw_guide', arguments: {} })).result;
    assert.equal(g.isError, false);
    assert.match(g.content[0].text, /dfw_can_write/, 'guide rendered for MCP');
    assert.doesNotMatch(g.content[0].text, /__DFW_SKILL_DIR__|CLAUDE_SKILL_DIR|AskUserQuestion|\{\{[A-Z_]+\}\}/);
    const ref = (await call('tools/call', { name: 'dfw_guide', arguments: { topic: 'references/phase-concepts.md' } })).result;
    assert.match(ref.content[0].text, /Phase: concepts/);
    const bad = (await call('tools/call', { name: 'dfw_guide', arguments: { topic: '../../package.json' } })).result;
    assert.equal(bad.isError, true);

    const status = (await call('tools/call', { name: 'dfw', arguments: { args: ['status'], cwd: root } })).result;
    assert.equal(status.isError, false);
    assert.match(status.content[0].text, /discovery|phase/i);
    const blocked = (await call('tools/call', { name: 'dfw', arguments: { args: ['install'] } })).result;
    assert.equal(blocked.isError, true);

    const gate = (await call('tools/call', { name: 'dfw_can_write', arguments: { files: ['src/App.tsx', 'design/screens/a.html'], cwd: root } })).result;
    assert.equal(gate.isError, true);
    assert.match(gate.content[0].text, /DENIED {2}src\/App\.tsx/);
    assert.match(gate.content[0].text, /ALLOWED design\/screens\/a\.html/);

    assert.equal((await call('nope/method')).error.code, -32601);
  });
});

test('mcp install: config text for OpenCode, Codex, Cursor (add, keep other servers, remove)', () => {
  const command = { cmd: '/x/dfw', args: [] };
  const oc = JSON.parse(mcpConfigText('opencode', '{"mcp":{"other":{"type":"local","command":["o"]}}}', { command }));
  assert.deepEqual(oc.mcp.dfw, { type: 'local', command: ['/x/dfw', 'mcp'], enabled: true });
  assert.ok(oc.mcp.other, 'other servers kept');
  const cu = JSON.parse(mcpConfigText('mcpServers', '', { command }));
  assert.deepEqual(cu.mcpServers.dfw, { command: '/x/dfw', args: ['mcp'] });
  assert.equal(JSON.parse(mcpConfigText('mcpServers', JSON.stringify(cu), { command, remove: true })).mcpServers.dfw, undefined);
  assert.throws(() => mcpConfigText('mcpServers', '{broken', { command }), /cannot parse/);

  const toml = 'model = "o3"\n\n[mcp_servers.other]\ncommand = "o"\n';
  const added = mcpConfigText('toml', toml, { command });
  assert.match(added, /\[mcp_servers\.dfw\]\ncommand = "\/x\/dfw"\nargs = \["mcp"\]/);
  assert.match(added, /\[mcp_servers\.other\]/);
  assert.equal(mcpConfigText('toml', added, { command }), added, 'idempotent');
  assert.equal(mcpConfigText('toml', added, { command, remove: true }).trim(), toml.trim());
});

test('mcp install CLI writes each tool config and backs up an existing one', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-mcphome-'));
  const env = { ...process.env, HOME: home, XDG_CONFIG_HOME: path.join(home, '.config'), CODEX_HOME: path.join(home, '.codex') };
  fs.mkdirSync(path.join(home, '.cursor'));
  fs.writeFileSync(path.join(home, '.cursor', 'mcp.json'), '{"mcpServers":{}}');
  const run = (...a) => execFileSync(process.execPath, [DFW, 'mcp', ...a], { env, encoding: 'utf8' });
  assert.match(run('install', 'all', '--print'), /\[mcp_servers\.dfw\]/);
  assert.equal(fs.existsSync(path.join(home, '.codex', 'config.toml')), false, '--print writes nothing');
  run('install', 'all');
  assert.ok(JSON.parse(fs.readFileSync(path.join(home, '.config', 'opencode', 'opencode.json'), 'utf8')).mcp.dfw);
  assert.match(fs.readFileSync(path.join(home, '.codex', 'config.toml'), 'utf8'), /\[mcp_servers\.dfw\]/);
  assert.ok(JSON.parse(fs.readFileSync(path.join(home, '.cursor', 'mcp.json'), 'utf8')).mcpServers.dfw);
  assert.ok(fs.existsSync(path.join(home, '.cursor', 'mcp.json.dfw-backup')));
  run('uninstall', 'cursor');
  assert.equal(JSON.parse(fs.readFileSync(path.join(home, '.cursor', 'mcp.json'), 'utf8')).mcpServers.dfw, undefined);
});
