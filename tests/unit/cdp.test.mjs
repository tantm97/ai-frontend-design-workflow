// lib/cdp.mjs against a fake browser (a small Node script that speaks just enough DevTools protocol):
// no Chrome needed. It never answers Page.navigate or Runtime.evaluate, like a dev server that
// stalls or a page stuck in a loop.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { launch, checkedDownloadUrl } from '../../core/lib/cdp.mjs';

const FAKE = `#!/usr/bin/env node
import http from 'node:http';
import crypto from 'node:crypto';
import fs from 'node:fs';
const mode = process.env.DFW_FAKE_BROWSER_MODE || 'ok';
if (process.env.DFW_FAKE_BROWSER_PID) fs.writeFileSync(process.env.DFW_FAKE_BROWSER_PID, String(process.pid));
if (mode === 'exit') process.exit(3);
const srv = http.createServer();
srv.on('upgrade', (req, sock) => {
  const acc = crypto.createHash('sha1').update(req.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  sock.write('HTTP/1.1 101 Switching Protocols\\r\\nUpgrade: websocket\\r\\nConnection: Upgrade\\r\\nSec-WebSocket-Accept: ' + acc + '\\r\\n\\r\\n');
  let buf = Buffer.alloc(0);
  const send = (o) => {
    const p = Buffer.from(JSON.stringify(o));
    sock.write(Buffer.concat([p.length < 126 ? Buffer.from([0x81, p.length]) : Buffer.from([0x81, 126, p.length >> 8, p.length & 255]), p]));
  };
  sock.on('error', () => {});
  sock.on('data', (d) => {
    buf = Buffer.concat([buf, d]);
    for (;;) {
      if (buf.length < 2) return;
      let len = buf[1] & 127;
      let off = 2;
      if (len === 126) {
        if (buf.length < 4) return;
        len = buf.readUInt16BE(2);
        off = 4;
      }
      if (buf.length < off + 4 + len) return;
      const mask = buf.subarray(off, off + 4);
      const pl = Buffer.from(buf.subarray(off + 4, off + 4 + len));
      for (let i = 0; i < pl.length; i++) pl[i] ^= mask[i & 3];
      const op = buf[0] & 15;
      buf = buf.subarray(off + 4 + len);
      if (op !== 1) continue;
      const m = JSON.parse(pl.toString());
      if (m.method === 'Page.navigate' || m.method === 'Runtime.evaluate') continue;
      if (m.method === 'Browser.close') process.exit(0);
      send({ id: m.id, result: { 'Target.createBrowserContext': { browserContextId: 'c1' }, 'Target.createTarget': { targetId: 't1' }, 'Target.attachToTarget': { sessionId: 's1' } }[m.method] || {} });
    }
  });
});
srv.listen(0, '127.0.0.1', () => process.stderr.write('DevTools listening on ws://127.0.0.1:' + (mode === 'badws' ? 1 : srv.address().port) + '/devtools/browser/x\\n'));
`;

const posix = process.platform !== 'win32';
const ENV = ['DFW_BROWSER', 'DFW_FAKE_BROWSER_MODE', 'DFW_FAKE_BROWSER_PID', 'TMPDIR'];

/** Run fn with the fake browser as DFW_BROWSER and an empty temp dir (so leftovers are visible). */
async function withFake(mode, fn) {
  const dir = fs.mkdtempSync(path.join(fs.realpathSync(os.tmpdir()), 'dfw-cdp-test-'));
  const exe = path.join(dir, 'fake-browser.mjs');
  fs.writeFileSync(exe, FAKE, { mode: 0o755 });
  const tmp = path.join(dir, 'tmp');
  fs.mkdirSync(tmp);
  const saved = Object.fromEntries(ENV.map((k) => [k, process.env[k]]));
  Object.assign(process.env, { DFW_BROWSER: exe, DFW_FAKE_BROWSER_MODE: mode, DFW_FAKE_BROWSER_PID: path.join(dir, 'pid'), TMPDIR: tmp });
  try {
    await fn({ tmp, pid: () => Number(fs.readFileSync(path.join(dir, 'pid'), 'utf8')) });
  } finally {
    for (const k of ENV) {
      if (saved[k] === undefined) delete process.env[k];
      else process.env[k] = saved[k];
    }
  }
}

const alive = (pid) => {
  try {
    process.kill(pid, 0);
    return true;
  } catch {
    return false;
  }
};
const until = async (fn, ms = 3000) => {
  for (const end = Date.now() + ms; Date.now() < end; ) {
    if (fn()) return true;
    await new Promise((r) => setTimeout(r, 25));
  }
  return fn();
};

async function newPage(browser) {
  const ctx = await browser.newContext({ cacheFonts: false });
  return ctx.newPage();
}

test('cdp: a navigation that never answers is a caught timeout, not an unhandled rejection', { skip: !posix }, async () => {
  await withFake('ok', async ({ tmp, pid }) => {
    const unhandled = [];
    const onUnhandled = (e) => unhandled.push(e);
    process.on('unhandledRejection', onUnhandled);
    const listeners = process.listenerCount('exit');
    const browser = await launch();
    try {
      assert.equal(process.listenerCount('exit'), listeners + 1, 'the browser is killed if this process ends first');
      const page = await newPage(browser);
      const t0 = Date.now();
      await assert.rejects(page.goto('http://127.0.0.1:9/slow', { timeout: 200 }), /timeout 200ms waiting for Page\.loadEventFired/);
      assert.ok(Date.now() - t0 < 2000, 'goto returns at its timeout, not when Page.navigate answers');
      await new Promise((r) => setTimeout(r, 50));
      assert.deepEqual(unhandled, []);
    } finally {
      process.off('unhandledRejection', onUnhandled);
      await browser.close();
    }
    assert.equal(process.listenerCount('exit'), listeners, 'guard removed on close');
    assert.ok(await until(() => !alive(pid())), 'browser process gone');
    assert.deepEqual(fs.readdirSync(tmp), [], 'temporary profile removed');
  });
});

test('cdp: a command the browser never answers fails after the command timeout', { skip: !posix }, async () => {
  await withFake('ok', async () => {
    const browser = await launch({ commandTimeoutMs: 200 });
    try {
      const page = await newPage(browser);
      await assert.rejects(page.evaluate(() => 1), /Runtime\.evaluate: no reply from the browser within 200 ms/);
      assert.deepEqual(await page.send('Page.enable'), {}, 'the connection still works');
    } finally {
      await browser.close();
    }
  });
});

test('cdp: a browser that fails to start leaves no process and no temporary profile', { skip: !posix }, async () => {
  await withFake('exit', async ({ tmp }) => {
    await assert.rejects(launch(), /browser exited \(3\) before it was ready/);
    assert.deepEqual(fs.readdirSync(tmp), []);
  });
  await withFake('badws', async ({ tmp, pid }) => {
    await assert.rejects(launch({ timeoutMs: 5000 }), /cannot connect to the browser/);
    assert.deepEqual(fs.readdirSync(tmp), []);
    assert.ok(await until(() => !alive(pid())), 'browser process killed');
  });
});

test('cdp: the headless Chrome download is https on storage.googleapis.com only', () => {
  const ok = 'https://storage.googleapis.com/chrome-for-testing-public/131.0.6778.85/mac-arm64/chrome-headless-shell-mac-arm64.zip';
  assert.equal(checkedDownloadUrl(ok), ok);
  for (const bad of [
    'http://storage.googleapis.com/chrome-for-testing-public/x.zip',
    'https://storage.googleapis.com.evil.example/x.zip',
    'https://evil.example/storage.googleapis.com/x.zip',
    'https://user:pw@storage.googleapis.com/x.zip',
    'https://storage.googleapis.com@evil.example/x.zip',
    'file:///tmp/x.zip',
    'not a url',
    undefined,
  ]) {
    assert.throws(() => checkedDownloadUrl(bad), /refusing to download a browser/, String(bad));
  }
});
