// Golden tests of the Studio server and the review proxy of the binary (`dfw __studio-server`,
// `dfw __review-server`), started on a copy of the fixture project and probed over HTTP, SSE and a
// raw WebSocket upgrade. Every answer (status, content type, cache header and body, with ports,
// temp roots, session ids and CSP nonces masked), every event and every file written is compared
// with what the Node Studio of 0.5.2 answered (tests/golden/studio/). The review proxies sit in
// front of one tiny local app.
// Run: node --test tests/golden/
import { describe, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import net from 'node:net';
import zlib from 'node:zlib';
import { spawn, spawnSync } from 'node:child_process';
import { nativeBinary, skipReason, REPO, GOLDEN, NOW, MINIMAL_STATE, suite, expected } from './harness.mjs';

const test = suite('studio');
const FIX = path.join(REPO, 'tests', 'fixtures', 'penpot');
/** The tokens.css Node's buildTokens wrote for the fixture project (saved by the recording run). */
const TOKENS_CSS = path.join(GOLDEN, 'studio', 'fixtures', 'tokens.css');
const read = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));
const write = (root, rel, data) => {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), typeof data === 'string' ? data : JSON.stringify(data));
};
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function freePort() {
  return new Promise((resolve) => {
    const s = net.createServer().listen(0, '127.0.0.1', () => {
      const { port } = s.address();
      s.close(() => resolve(port));
    });
  });
}

/** Raw request (Host and Origin can be anything; fetch forbids setting them). */
function request(port, { method = 'GET', path: p = '/', headers = {}, body = null } = {}) {
  return new Promise((resolve, reject) => {
    const req = http.request({ host: '127.0.0.1', port, method, path: p, headers: { host: `127.0.0.1:${port}`, ...headers } }, (res) => {
      const chunks = [];
      res.on('data', (c) => chunks.push(c));
      res.on('end', () => resolve({ status: res.statusCode, headers: res.headers, raw: Buffer.concat(chunks) }));
    });
    req.on('error', reject);
    req.end(body);
  });
}

/** Wait until something answers on the port. */
async function ready(port, p = '/__dfw/api/manifest') {
  for (let i = 0; i < 100; i++) {
    try {
      return await request(port, { path: p });
    } catch {
      await sleep(100);
    }
  }
  throw new Error(`nothing answers on ${port}`);
}

/** The fixture export, built by the binary (`__debug penpot-build-export`, as Node's buildExport). */
function fixtureExport() {
  const r = spawnSync(nativeBinary(), ['__debug', 'penpot-build-export', path.join(FIX, 'renders.json'), path.join(FIX, 'project/design/tokens/tokens.json'), '2026-11-16T10:00:00.000Z'], { encoding: 'utf8' });
  assert.equal(r.status, 0, r.stderr);
  return JSON.parse(r.stdout);
}

/** Start the Studio of the binary on its own copy of the fixture project. */
async function startStudio(tmp) {
  const root = path.join(tmp, 'native', 'project');
  fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
  write(root, '.dfw/state.json', `${JSON.stringify(MINIMAL_STATE, null, 2)}\n`);
  // An open session, so no session named after the real second is started.
  write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [] });
  const doc = fixtureExport();
  write(root, `.dfw/penpot/exports/${doc.id}.json`, doc);
  write(root, '.dfw/penpot/export.json', doc);
  write(root, 'design/assets/logo.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>\n');
  write(root, 'design/screens/items/detail.html', '<!doctype html><html><head><title>D</title></head><body><a data-dfw-id="d.back" href="a.html">Back <b>to</b> list</a><dfw-use c="button" id="d.cta">Go</dfw-use></body></html>\n');
  // tokens.css as the recording run built it (tokens only: no components.css yet).
  fs.copyFileSync(TOKENS_CSS, path.join(root, 'design/tokens/tokens.css'));
  const port = await freePort();
  const env = { ...process.env, DFW_HOME: path.join(tmp, 'native', 'home'), DFW_NOW: NOW, DFW_PLUGIN_ROOT: path.join(REPO, 'core'), DFW_STUDIO_WATCH: 'poll' };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED']) delete env[k];
  const child = spawn(nativeBinary(), ['__studio-server', '--root', root, '--port', String(port)], { env, stdio: 'ignore' });
  await ready(port);
  return { root, port, child };
}

/** Mask what differs between the two runs. */
function mask(text, s) {
  const t = String(text)
    .split(s.root)
    .join('<root>')
    .split(JSON.stringify(s.root).slice(1, -1))
    .join('<root>')
    .replace(new RegExp(`\\b${s.port}\\b`, 'g'), '<port>')
    .replace(/rv_\d{8}T\d{6}/g, 'rv_<now>')
    .replace(/'nonce-[A-Za-z0-9+/=]+'/g, "'nonce-<n>'")
    .replace(/nonce="[A-Za-z0-9+/=]+"/g, 'nonce="<n>"');
  // The port of the app behind a review proxy differs per run too.
  return s.app ? t.replace(new RegExp(`\\b${s.app}\\b`, 'g'), '<app>') : t;
}

// Browser files the servers send as they are: the recorded body names the file, so a change to
// the browser code does not need a new recording (the bytes are the file's, checked here).
const BROWSER_FILES = ['app.html', 'app.js', 'app.css', 'overlay.js', 'live.js', 'penpot/plugin.js', 'penpot/ui.html', 'penpot/icon.svg'];
const browserFile = (raw) => BROWSER_FILES.find((rel) => raw.equals(fs.readFileSync(path.join(REPO, 'core', 'studio', rel))));

/** What a client can observe of a response, masked. */
function seen(r, s, extra = []) {
  const file = browserFile(r.raw);
  const body = file ? `<core/studio/${file}>` : mask(r.raw.toString('utf8'), s);
  const out = { status: r.status, type: r.headers['content-type'] ?? null, cache: r.headers['cache-control'] ?? null, body };
  for (const h of extra) out[h] = r.headers[h] === undefined ? null : mask([].concat(r.headers[h]).join('\n'), s);
  return out;
}

/** Files under .dfw/ and design/ of a project, masked. */
function tree(s) {
  const out = {};
  const walk = (dir) => {
    for (const e of fs.readdirSync(dir, { withFileTypes: true })) {
      const abs = path.join(dir, e.name);
      if (e.isDirectory()) walk(abs);
      else if (!/\.log$/.test(e.name)) out[mask(path.relative(s.root, abs), s)] = mask(fs.readFileSync(abs, 'utf8'), s);
    }
  };
  for (const top of ['.dfw', 'design']) if (fs.existsSync(path.join(s.root, top))) walk(path.join(s.root, top));
  return out;
}

/** An event stream reader: `next(name)` waits for the next event of that name. */
async function events(s) {
  const got = [];
  const res = await new Promise((resolve, reject) => {
    const req = http.get({ host: '127.0.0.1', port: s.port, path: '/__dfw/events', headers: { host: `127.0.0.1:${s.port}` } }, resolve);
    req.on('error', reject);
  });
  let buf = '';
  res.on('data', (c) => {
    buf += c;
    let i;
    while ((i = buf.indexOf('\n\n')) >= 0) {
      const block = buf.slice(0, i);
      buf = buf.slice(i + 2);
      const name = /^event: (.*)$/m.exec(block)?.[1];
      const data = /^data: (.*)$/m.exec(block)?.[1];
      got.push({ name, data: mask(data, s) });
    }
  });
  return {
    async next(name, from = 0, ms = 6000) {
      const end = Date.now() + ms;
      while (Date.now() < end) {
        const e = got.slice(from).find((x) => x.name === name);
        if (e) return e;
        await sleep(25);
      }
      return null;
    },
    count: () => got.length,
    first: () => got[0],
    close: () => res.destroy(),
  };
}

describe('studio server: Node and native answer the same', { skip: skipReason() }, () => {
  let tmp;
  let R;
  before(async () => {
    tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-studio-')));
    R = await startStudio(tmp);
  });
  after(() => {
    R?.child.kill();
    fs.rmSync(tmp, { recursive: true, force: true });
  });

  /** A request to the Studio; its answer must be the recorded one. */
  async function same(opts, extra = []) {
    const o = typeof opts === 'string' ? { path: opts } : opts;
    const headers = Object.fromEntries(Object.entries(o.headers || {}).map(([k, v]) => [k, String(v).replaceAll('<port>', R.port)]));
    const y = seen(await request(R.port, { ...o, headers }), R, extra);
    assert.deepEqual(y, expected(y), `${o.method || 'GET'} ${o.path}`);
    return y;
  }
  /** The files of the project (or a part of them) must be the recorded ones. */
  const sameFiles = (t) => assert.deepEqual(t, expected(t));

  test('static files, design files (composed, overlay injected), the system page, 404s', async () => {
    for (const p of ['/', '/index.html', '/__dfw/static/app.js', '/__dfw/static/app.css', '/__dfw/static/penpot/ui.html', '/__dfw/static/nope.js', '/__dfw/static/../../package.json', '/__dfw/overlay.js', '/favicon.ico']) await same(p);
    const page = await same('/design/screens/items/a.html');
    assert.match(page.body, /<script src="\/__dfw\/overlay\.js" data-dfw-overlay><\/script><\/body>/);
    const detail = await same('/design/screens/items/detail.html');
    assert.match(detail.body, /system\/components\.css/, 'component CSS linked for an instance');
    for (const p of ['/design/tokens/tokens.json', '/design/tokens/tokens.css', '/design/system/components.css', '/design/assets/logo.svg', '/design/design.json', '/design/screens', '/design/missing.html', '/design/../../../etc/passwd', '/design/%E0%A4%A', '/nope', '/__dfw/shots/x.png', '/__dfw/api/edit', '/__dfw/api/comments/fb_1']) await same(p);
    await same({ path: '/', method: 'HEAD' });
    const sys = await same('/__dfw/system.html');
    assert.match(sys.body, /data-dfw-overlay/);
  });

  test('API reads: manifest (with and without design.json), tokens, components, state', async () => {
    for (const p of ['/__dfw/api/manifest', '/__dfw/api/tokens', '/__dfw/api/components', '/__dfw/api/state', '/__dfw/api/comments']) await same(p);
    fs.renameSync(path.join(R.root, 'design/design.json'), path.join(R.root, 'design.json.off'));
    try {
      const m = JSON.parse((await same('/__dfw/api/manifest')).body);
      assert.equal(m.inferred, true);
    } finally {
      fs.renameSync(path.join(R.root, 'design.json.off'), path.join(R.root, 'design/design.json'));
    }
  });

  test('Host and Origin: foreign host refused, cross-origin writes refused, reads allowed', async () => {
    await same({ path: '/__dfw/api/comments', headers: { host: 'evil.example' } });
    await same({ path: '/__dfw/api/manifest', headers: { host: 'localhost:<port>' } });
    await same({ path: '/__dfw/api/manifest', headers: { origin: 'https://evil.example' } });
    const body = JSON.stringify({ comment: 'from another site' });
    await same({ path: '/__dfw/api/comments', method: 'POST', headers: { origin: 'https://evil.example', 'content-type': 'text/plain' }, body });
    await same({ path: '/__dfw/api/edit', method: 'POST', headers: { origin: 'http://127.0.0.1:1' }, body: '{}' });
    sameFiles(tree(R));
  });

  test('comments lifecycle with events: add, list, patch, new session, state', async () => {
    const er = await events(R);
    try {
      const hello = await er.next('hello');
      assert.deepEqual(hello, expected(hello));
      const post = { path: '/__dfw/api/comments', method: 'POST', headers: { origin: 'http://127.0.0.1:<port>', 'content-type': 'application/json' } };
      assert.equal((await same({ ...post, body: JSON.stringify({ comment: 'Make it bold', screen: 'items', variant: 'a', breakpoint: 'desktop', file: 'screens/items/a.html', target: { dfwId: 'items.title', tag: 'h1', text: 'Inventory' } }) })).status, 201);
      const ev = await er.next('comments');
      assert.deepEqual(ev, expected(ev));
      await same({ ...post, body: '{"comment":"  "}' });
      await same({ ...post, body: '' });
      await same('/__dfw/api/comments');
      const patch = { path: '/__dfw/api/comments/fb_001', method: 'PATCH', headers: { 'content-type': 'application/json' } };
      await same({ ...patch, body: JSON.stringify({ message: 'Only the title, please' }) });
      await same({ ...patch, body: JSON.stringify({ status: 'dismissed', message: 'Not needed' }) });
      await same({ ...patch, body: JSON.stringify({ status: 'bogus' }) });
      await same({ path: '/__dfw/api/comments/fb_999', method: 'PATCH', body: '{}' });
      const from = er.count();
      await same({ path: '/__dfw/api/sessions', method: 'POST' });
      const ev2 = await er.next('comments', from);
      assert.deepEqual(ev2, expected(ev2));
      await same('/__dfw/api/state');
      await same('/__dfw/api/comments');
      sameFiles(tree(R));
    } finally {
      er.close();
    }
  });

  test('edit and undo: same answers, same files written', async () => {
    const edit = (op) => same({ path: '/__dfw/api/edit', method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(op) });
    assert.equal((await edit({ file: 'screens/items/a.html', op: 'text', id: 'items.title', value: 'Stock <now> & "then"' })).status, 200);
    await edit({ file: 'screens/items/a.html', op: 'text', id: 'items.summary', value: 'x' });
    await edit({ file: '../outside.html', op: 'text', id: 'x', value: 'y' });
    // The JSON parsers word their errors differently: only the status is compared.
    const bad = await request(R.port, { path: '/__dfw/api/edit', method: 'POST', body: '{not json' });
    assert.equal(bad.status, 500);
    assert.equal(bad.status, expected(bad.status));
    const strip = (t) => Object.fromEntries(Object.entries(t).filter(([k]) => !k.startsWith('.dfw/edits/')));
    sameFiles(strip(tree(R)));
    const journal = (s) => Object.entries(tree(s)).filter(([k]) => k.startsWith('.dfw/edits/')).map(([, v]) => JSON.parse(v));
    sameFiles(journal(R));
    await same({ path: '/__dfw/api/undo', method: 'POST' });
    await same({ path: '/__dfw/api/undo', method: 'POST' });
    sameFiles(tree(R));
  });

  test('live reload: a design change and a review file change reach the pages; presence is relayed', async () => {
    const er = await events(R);
    try {
      await er.next('hello');
      fs.appendFileSync(path.join(R.root, 'design/screens/items/a.html'), '\n<!-- touched -->\n');
      const d = await er.next('design');
      assert.ok(d, 'design event');
      assert.deepEqual(d, expected(d));
      const from = er.count();
      write(R.root, '.dfw/reviews/rv_test.json', { comments: [] });
      const c = await er.next('comments', from);
      assert.ok(c, 'comments event');
      assert.deepEqual(c, expected(c));
      write(R.root, '.dfw/presence.json', { file: 'screens/items/a.html', dfwId: 'items.title', at: new Date().toISOString() });
      const p = await er.next('presence');
      assert.deepEqual(p, { name: 'presence', data: '{"file":"screens/items/a.html","dfwId":"items.title"}' });
      assert.deepEqual(p, expected(p));
    } finally {
      er.close();
    }
  });

  test('penpot routes: plugin files over CORS, export.json, changes and sync', async () => {
    const cors = ['access-control-allow-origin', 'access-control-allow-private-network'];
    for (const p of ['/__dfw/penpot/manifest.json', '/__dfw/penpot/plugin.js', '/__dfw/penpot/icon.svg', '/__dfw/penpot/ui.html', '/__dfw/penpot/export.json', '/__dfw/penpot/nope']) await same(p, cors);
    await same({ path: '/__dfw/penpot/plugin.js', method: 'OPTIONS', headers: { origin: 'https://design.penpot.app' } }, cors);
    const doc = fixtureExport();
    const changes = JSON.stringify({ ...read('changes.json'), exportId: doc.id });
    await same({ path: '/__dfw/penpot/changes', method: 'POST', headers: { origin: 'https://design.penpot.app', 'content-type': 'application/json' }, body: changes });
    assert.equal((await same({ path: '/__dfw/penpot/changes', method: 'POST', headers: { origin: 'http://127.0.0.1:<port>', 'content-type': 'application/json' }, body: changes })).status, 200);
    const thread = (seq, content) => ({ seq, resolved: false, page: 'Stock', boardKey: 'items|a|desktop', x: 120, y: 175, comments: [{ content, user: 'Designer' }] });
    const sync = await same({ path: '/__dfw/penpot/sync', method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ fileId: 'file-1', exportId: doc.id, threads: [thread(1, 'Make low stock more visible')] }) });
    assert.equal(sync.status, 200, sync.body);
    await same({ path: '/__dfw/penpot/export.json', method: 'POST' });
    const t = (s) => Object.fromEntries(Object.entries(tree(s)).filter(([k]) => !k.startsWith('.dfw/edits/')).map(([k, v]) => [k, v.replace(/"(at|createdAt|updatedAt|importedAt)":"[^"]*"/g, '"$1":"<t>"')]));
    sameFiles(t(R));
  });
});

/* ------------------------------------------------------------------ review proxy */

const STRICT = "default-src 'none'; script-src 'nonce-app' 'strict-dynamic'; style-src 'self'; img-src 'self'; frame-ancestors 'none'";
const APP_HTML = `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="script-src 'nonce-app' 'strict-dynamic'"><title>App</title></head><body><main data-insp-path="src/App.tsx:3:5:main"><h1>App</h1></main></body></html>`;

/** A tiny dev server: strict CSP, gzip and br pages, a cookie for the other loopback name, a redirect, a WebSocket echo. */
async function fakeApp() {
  const app = http.createServer((req, res) => {
    const html = { 'content-type': 'text/html; charset=utf-8', 'content-security-policy': STRICT, etag: '"v1"' };
    if (req.url === '/gz') {
      res.writeHead(200, { ...html, 'content-encoding': 'gzip' });
      return res.end(zlib.gzipSync(APP_HTML));
    }
    if (req.url === '/br') {
      res.writeHead(200, { ...html, 'content-encoding': 'br' });
      return res.end(zlib.brotliCompressSync(APP_HTML));
    }
    if (req.url === '/deflate') {
      res.writeHead(200, { ...html, 'content-encoding': 'deflate' });
      return res.end(zlib.deflateSync(APP_HTML));
    }
    if (req.url === '/login') {
      res.writeHead(200, { 'content-type': 'text/plain', 'set-cookie': ['sid=s3cret; Path=/; HttpOnly', 'pref=1; Domain=127.0.0.1; Path=/', 'x=2; Domain=example.test'] });
      return res.end('ok');
    }
    if (req.url === '/old') {
      res.writeHead(302, { location: `http://localhost:${app.address().port}/new?x=1` });
      return res.end();
    }
    if (req.url === '/app.js') {
      res.writeHead(200, { 'content-type': 'text/javascript' });
      return res.end('console.log(1)');
    }
    if (req.url === '/echo') {
      const chunks = [];
      req.on('data', (c) => chunks.push(c));
      return req.on('end', () => {
        res.writeHead(200, { 'content-type': 'text/plain' });
        res.end(`${req.method} ${req.headers.host} ${req.headers.origin || '-'} ${Buffer.concat(chunks)}`);
      });
    }
    res.writeHead(200, { ...html, 'content-security-policy-report-only': "script-src 'nonce-app'" });
    res.end(APP_HTML);
  });
  app.on('upgrade', (req, socket) => {
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nX-Seen-Host: ${req.headers.host}\r\n\r\n`);
    socket.on('data', (d) => socket.write(`echo:${d}`));
  });
  await new Promise((r) => app.listen(0, '127.0.0.1', r));
  return app;
}

async function startProxy(tmp, name, target) {
  const root = path.join(tmp, name, 'project');
  fs.mkdirSync(root, { recursive: true });
  write(root, '.dfw/state.json', `${JSON.stringify(MINIMAL_STATE, null, 2)}\n`);
  write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [] });
  const port = await freePort();
  const env = { ...process.env, DFW_HOME: path.join(tmp, name, 'home'), DFW_NOW: NOW, DFW_PLUGIN_ROOT: path.join(REPO, 'core') };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED']) delete env[k];
  const child = spawn(nativeBinary(), ['__review-server', '--root', root, '--url', target, '--port', String(port), '--no-shots'], { env, stdio: 'ignore' });
  await ready(port, '/__dfw/api/live');
  return { root, port, app: new URL(target).port, child };
}

describe('review proxy: Node and native answer the same', { skip: skipReason() }, () => {
  let tmp;
  let app;
  let R;
  before(async () => {
    tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-review-')));
    app = await fakeApp();
    R = await startProxy(tmp, 'native', `http://localhost:${app.address().port}`);
  });
  after(() => {
    R?.child.kill();
    app?.close();
    app?.closeAllConnections();
    fs.rmSync(tmp, { recursive: true, force: true });
  });

  const HDRS = ['content-security-policy', 'content-security-policy-report-only', 'etag', 'location', 'set-cookie', 'content-encoding'];
  async function same(opts) {
    const o = typeof opts === 'string' ? { path: opts } : opts;
    const headers = Object.fromEntries(Object.entries({ host: 'localhost:<port>', ...o.headers }).map(([k, v]) => [k, String(v).replaceAll('<port>', R.port)]));
    const y = seen(await request(R.port, { ...o, headers }), R, HDRS);
    assert.deepEqual(y, expected(y), `${o.method || 'GET'} ${o.path}`);
    return y;
  }

  test('HTML: strict CSP header and meta patched with a nonce, scripts injected; gzip, br and deflate decoded', async () => {
    const page = await same('/');
    assert.match(page.body, /data-dfw-live nonce="<n>"/);
    assert.match(page['content-security-policy'], /'nonce-<n>'/);
    for (const p of ['/gz', '/br', '/deflate']) assert.match((await same(p)).body, /data-dfw-live/);
    await same({ path: '/', headers: { host: '127.0.0.1:<port>' } });
  });

  test('pass-through: assets, redirects, cookies, request bodies and Origin rewritten to the app', async () => {
    await same('/app.js');
    const r = await same('/old');
    assert.equal(r.location, 'http://localhost:<port>/new?x=1');
    const c = await same('/login');
    assert.equal(c['set-cookie'], 'sid=s3cret; Path=/; HttpOnly\npref=1; Path=/\nx=2; Domain=example.test');
    await same({ path: '/echo', method: 'POST', headers: { origin: 'http://localhost:<port>', 'content-type': 'text/plain' }, body: 'hello' });
    await same({ path: '/', method: 'HEAD' });
  });

  test('own routes, comments and refusals', async () => {
    for (const p of ['/__dfw/api/live', '/__dfw/overlay.js', '/__dfw/live.js']) await same(p);
    await same({ path: '/', headers: { host: 'evil.example' } });
    const post = { path: '/__dfw/api/comments', method: 'POST', headers: { 'content-type': 'application/json' } };
    await same({ ...post, headers: { ...post.headers, origin: 'https://evil.example' }, body: '{"comment":"x"}' });
    const c = await same({ ...post, body: JSON.stringify({ comment: 'Bold title', url: '/cart?x=1', viewport: [390, 844], target: { selector: 'h1', insp: 'src/App.tsx:3:5:main', source: 'evil.ts:1' } }) });
    assert.equal(c.status, 201);
    assert.match(c.body, /"source":"src\/App\.tsx:3:5"/);
    await same({ ...post, body: JSON.stringify({ comment: 'No source', target: { selector: '#x' } }) });
    await same({ ...post, body: '{"comment":"  "}' });
    await same({ ...post, body: JSON.stringify({ comment: 'x'.repeat(1_000_001) }) });
    const files = tree(R);
    assert.deepEqual(files, expected(files));
  });

  test('WebSocket upgrade is passed through with the Host rewritten', async () => {
    const ws = (s) =>
      new Promise((resolve, reject) => {
        const sock = net.connect(s.port, '127.0.0.1', () => sock.write(`GET /hmr HTTP/1.1\r\nHost: localhost:${s.port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n`));
        let buf = '';
        sock.on('data', (d) => {
          buf += d;
          if (buf.includes('\r\n\r\n') && !buf.includes('echo:')) sock.write('ping');
          if (buf.includes('echo:ping')) {
            sock.destroy();
            resolve(buf);
          }
        });
        sock.on('error', reject);
      });
    const b = mask(await ws(R), R);
    assert.match(b, /^HTTP\/1\.1 101/);
    assert.equal(b, expected(b));
  });

  test('app down: a readable 502', async () => {
    const port = await freePort();
    const r = await startProxy(tmp, 'native-down', `http://127.0.0.1:${port}`);
    try {
      const b = await request(r.port, { path: '/' });
      assert.equal(b.status, 502);
      assert.match(b.raw.toString(), /dfw review: the app is not reachable/);
      const got = { status: b.status, type: b.headers['content-type'] ?? null };
      assert.deepEqual(got, expected(got));
    } finally {
      r.child.kill();
    }
  });
});
