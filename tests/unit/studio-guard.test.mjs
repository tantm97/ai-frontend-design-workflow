import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { addComment, allComments } from '../../core/lib/comments.mjs';
import { createLiveServer } from '../../core/lib/live.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const DFW = path.resolve(HERE, '../../core/bin/dfw.mjs');
const FIX = path.resolve(HERE, '../fixtures/penpot/project');

function project() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-guard-'));
  fs.cpSync(FIX, root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw', 'reviews'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ design: { version: 1, status: 'draft', locked: false } }));
  return root;
}

/** Raw request, so Host and Origin can be anything (fetch forbids setting them). */
function request(port, { method = 'GET', path: p = '/', headers = {}, body = null }) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port, method, path: p, headers }, (res) => {
      res.resume();
      res.on('end', () => resolve(res.statusCode));
    });
    req.on('error', reject);
    req.end(body);
  });
}

const COMMENT = JSON.stringify({ comment: 'from another site', screen: 'home' });

// A page on another site can send a "simple" POST to 127.0.0.1 without a preflight; the agent would
// read the comment as the user's words. A DNS-rebinding page reaches the server with its own Host.
test('the Studio refuses writes from another origin and requests for another host', async () => {
  const root = project();
  const port = 47000 + Math.floor(Math.random() * 900);
  const child = spawn(process.execPath, [DFW, '__studio-server', '--root', root, '--port', String(port)], { stdio: 'ignore', env: { ...process.env, DFW_HOME: path.join(root, '.home'), DFW_STUDIO_WATCH: 'poll' } });
  try {
    for (let i = 0; i < 50; i++) {
      try {
        await request(port, { path: '/__dfw/api/manifest' });
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    const post = (headers) => request(port, { method: 'POST', path: '/__dfw/api/comments', headers: { 'content-type': 'text/plain', ...headers }, body: COMMENT });
    assert.equal(await post({ origin: 'https://evil.example' }), 403);
    assert.equal(await request(port, { path: '/__dfw/api/comments', headers: { host: 'evil.example' } }), 403);
    assert.equal(allComments(root).length, 0);
    assert.equal(await post({ origin: `http://127.0.0.1:${port}` }), 201);
    assert.equal(await post({ origin: `http://localhost:${port}`, host: `localhost:${port}` }), 201);
    assert.equal(await post({}), 201); // curl and the CLI send no Origin
    assert.equal(allComments(root).length, 3);
  } finally {
    child.kill();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('dfw review refuses comments from another origin and requests for another host', async () => {
  const root = project();
  const server = createLiveServer({ root, target: 'http://127.0.0.1:9', shots: false });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  const port = server.address().port;
  try {
    const post = (headers) => request(port, { method: 'POST', path: '/__dfw/api/comments', headers: { 'content-type': 'text/plain', ...headers }, body: COMMENT });
    assert.equal(await post({ origin: 'https://evil.example' }), 403);
    assert.equal(await request(port, { path: '/', headers: { host: 'evil.example' } }), 403);
    assert.equal(allComments(root).length, 0);
    assert.equal(await post({ origin: `http://127.0.0.1:${port}` }), 201);
  } finally {
    server.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
});

// The Studio server and the CLI write the same session file from different processes.
test('comments from concurrent processes are all stored with unique ids', () => {
  const root = project();
  try {
    addComment(root, { comment: 'first' });
    const script = `import { addComment } from ${JSON.stringify(path.resolve(HERE, '../../core/lib/comments.mjs'))}; for (let i = 0; i < 40; i++) addComment(process.argv[1], { comment: process.argv[2] + i });`;
    const run = (tag) => new Promise((resolve) => spawn(process.execPath, ['--input-type=module', '-e', script, root, tag], { stdio: 'ignore' }).on('exit', resolve));
    return Promise.all([run('a'), run('b'), run('c')]).then((codes) => {
      assert.deepEqual(codes, [0, 0, 0]);
      const all = allComments(root);
      assert.equal(all.length, 121);
      assert.equal(new Set(all.map((c) => c.id)).size, 121);
      assert.ok(!fs.existsSync(path.join(root, '.dfw', 'reviews', '.lock')));
      fs.rmSync(root, { recursive: true, force: true });
    });
  } catch (e) {
    fs.rmSync(root, { recursive: true, force: true });
    throw e;
  }
});
