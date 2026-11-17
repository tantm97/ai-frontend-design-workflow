import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { buildTokens } from '../../core/lib/tokens.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DFW = path.resolve(HERE, '../../core/bin/dfw.mjs');
const FIX = path.resolve(HERE, '../fixtures/penpot/project');

/** Collect Studio server-sent events until `want` names have all arrived (or the timeout). */
async function events(base, want, act, timeoutMs = 6000) {
  const ctl = new AbortController();
  const got = new Set();
  const res = await fetch(`${base}/__dfw/events`, { signal: ctl.signal });
  const reader = res.body.getReader();
  const dec = new TextDecoder();
  const done = (async () => {
    let buf = '';
    for (;;) {
      const { value, done: end } = await reader.read();
      if (end) return;
      buf += dec.decode(value, { stream: true });
      for (const m of buf.matchAll(/^event: (\w+)$/gm)) got.add(m[1]);
      if (want.every((w) => got.has(w))) return;
    }
  })();
  await new Promise((r) => setTimeout(r, 200));
  act();
  await Promise.race([done, new Promise((r) => setTimeout(r, timeoutMs))]);
  ctl.abort();
  return got;
}

// fs.watch is unreliable on some filesystems (it missed review updates on the GitHub runner), so the
// Studio also polls mtimes. With DFW_STUDIO_WATCH=poll, fs.watch is off and polling alone must work.
test('studio live reload works by polling alone (DFW_STUDIO_WATCH=poll)', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-watch-'));
  fs.cpSync(FIX, root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw', 'reviews'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ design: { version: 1, status: 'draft', locked: false } }));
  buildTokens(root);
  const port = 48000 + Math.floor(Math.random() * 1000);
  const child = spawn(process.execPath, [DFW, '__studio-server', '--root', root, '--port', String(port)], {
    stdio: 'ignore',
    env: { ...process.env, DFW_HOME: path.join(root, '.home'), DFW_STUDIO_WATCH: 'poll' },
  });
  const base = `http://127.0.0.1:${port}`;
  try {
    for (let i = 0; i < 50; i++) {
      try {
        await fetch(`${base}/__dfw/api/manifest`);
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    const review = path.join(root, '.dfw', 'reviews', 'rv_test.json');
    const got = await events(base, ['comments'], () => fs.writeFileSync(review, JSON.stringify({ comments: [] })));
    assert.ok(got.has('comments'), `expected a comments event, got ${[...got].join(', ')}`);
    const screen = fs.readdirSync(path.join(root, 'design', 'screens'))[0];
    const dir = path.join(root, 'design', 'screens', screen);
    const file = path.join(dir, fs.readdirSync(dir).find((f) => f.endsWith('.html')));
    const got2 = await events(base, ['design'], () => fs.appendFileSync(file, '\n<!-- touched -->\n'));
    assert.ok(got2.has('design'), `expected a design event, got ${[...got2].join(', ')}`);
  } finally {
    child.kill();
    fs.rmSync(root, { recursive: true, force: true });
  }
});
