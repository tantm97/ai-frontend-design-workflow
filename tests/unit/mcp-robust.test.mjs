// "dfw mcp": input that is valid JSON but not a request, and installs over an existing config.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { mcpConfigText, mcpInstall, mcpInstalled } from '../../core/lib/mcp.mjs';

const DFW = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../../core/bin/dfw.mjs');
const command = { cmd: '/opt/dfw/bin/dfw', args: [] };

test('mcp server: null, a number or a batch array get an error reply; the next request is still served', async () => {
  const child = spawn(process.execPath, [DFW, 'mcp'], { stdio: ['pipe', 'pipe', 'pipe'] });
  let out = '';
  let err = '';
  child.stdout.on('data', (d) => (out += d));
  child.stderr.on('data', (d) => (err += d));
  child.stdin.end('null\n42\n[{"jsonrpc":"2.0","id":1,"method":"ping"}]\n{"jsonrpc":"2.0","id":2,"method":"ping"}\n');
  const code = await new Promise((r) => child.on('close', r));
  assert.equal(code, 0, err);
  const replies = out.trim().split('\n').map((l) => JSON.parse(l));
  assert.deepEqual(replies.map((m) => [m.id, m.error?.code ?? 'ok']), [[null, -32600], [null, -32600], [null, -32600], [2, 'ok']]);
});

test('mcp install: a CRLF config.toml that already has dfw is replaced, not duplicated', () => {
  const crlf = '[model]\r\nname = "x"\r\n\r\n[mcp_servers.dfw]\r\ncommand = "/old/dfw"\r\nargs = ["mcp"]\r\n\r\n[mcp_servers.other]\r\ncommand = "o"\r\n';
  const next = mcpConfigText('toml', crlf, { command });
  assert.equal(next.match(/\[mcp_servers\.dfw\]/g).length, 1);
  assert.ok(!next.includes('/old/dfw'));
  assert.match(next, /\[mcp_servers\.other\]/);
  assert.ok(!mcpConfigText('toml', crlf, { remove: true }).includes('mcp_servers.dfw'));
});

test('mcp install: the first backup of the user config survives later installs', () => {
  const home = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-mcp-backup-'));
  const before = process.env.CODEX_HOME;
  process.env.CODEX_HOME = home;
  try {
    const file = path.join(home, 'config.toml');
    const original = '[model]\nname = "x"\n';
    fs.writeFileSync(file, original);
    mcpInstall('codex');
    assert.equal(mcpInstalled('codex'), true);
    assert.equal(fs.readFileSync(`${file}.dfw-backup`, 'utf8'), original);
    mcpInstall('codex', { remove: true });
    assert.equal(fs.readFileSync(`${file}.dfw-backup`, 'utf8'), original, 'uninstall does not replace it with a config that has dfw');
    mcpInstall('codex');
    assert.equal(fs.readFileSync(`${file}.dfw-backup`, 'utf8'), original, 'still the config from before dfw');
  } finally {
    if (before === undefined) delete process.env.CODEX_HOME;
    else process.env.CODEX_HOME = before;
  }
});
