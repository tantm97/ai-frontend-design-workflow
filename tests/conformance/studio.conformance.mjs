// Cross-implementation tests of the Studio server and the review proxy (rust-core, studio group).
// Each case starts the Node server (`node core/bin/dfw.mjs __studio-server`) and the native one
// (`dfw __studio-server`) on two copies of the fixture project, sends the same requests to both and
// compares status, content type and body after masking what legitimately differs (ports, temp
// roots, session ids, CSP nonces). The review proxies sit in front of one tiny local app.
import { describe, test, before, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import net from 'node:net';
import zlib from 'node:zlib';
import { spawn } from 'node:child_process';
import { nativeBinary, skipReason, REPO, NOW, MINIMAL_STATE } from './harness.mjs';
import { buildExport } from '../../core/lib/penpot.mjs';
import { buildTokens } from '../../core/lib/tokens.mjs';

const FIX = path.join(REPO, 'tests', 'fixtures', 'penpot');
const NODE_CLI = path.join(REPO, 'core', 'bin', 'dfw.mjs');
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

const fixtureExport = () =>
  buildExport({ renders: read('renders.json'), tokens: { base: read('project/design/tokens/tokens.json'), themes: {} }, product: 'Fixture', generatedAt: '2026-11-16T10:00:00.000Z' });

/** Start one Studio on its own copy of the fixture project. */
async function startStudio(impl, tmp) {
  const root = path.join(tmp, impl, 'project');
  fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
  write(root, '.dfw/state.json', `${JSON.stringify(MINIMAL_STATE, null, 2)}\n`);
  // An open session, so no session named after the real second is started.
  write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [] });
  const doc = fixtureExport();
  write(root, `.dfw/penpot/exports/${doc.id}.json`, doc);
  write(root, '.dfw/penpot/export.json', doc);
  write(root, 'design/assets/logo.svg', '<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"/>\n');
  write(root, 'design/screens/items/detail.html', '<!doctype html><html><head><title>D</title></head><body><a data-dfw-id="d.back" href="a.html">Back <b>to</b> list</a><dfw-use c="button" id="d.cta">Go</dfw-use></body></html>\n');
  buildTokens(root);
  const port = await freePort();
  const env = { ...process.env, DFW_HOME: path.join(tmp, impl, 'home'), DFW_NOW: NOW, DFW_PLUGIN_ROOT: path.join(REPO, 'core'), DFW_STUDIO_WATCH: 'poll' };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED']) delete env[k];
  const [cmd, args] = impl === 'node' ? [process.execPath, [NODE_CLI]] : [nativeBinary(), []];
  const child = spawn(cmd, [...args, '__studio-server', '--root', root, '--port', String(port)], { env, stdio: 'ignore' });
  await ready(port);
  return { impl, root, port, child };
}

/** Mask what differs between the two runs. */
function mask(text, s) {
  return String(text)
    .split(s.root)
    .join('<root>')
    .split(JSON.stringify(s.root).slice(1, -1))
    .join('<root>')
    .replace(new RegExp(`\\b${s.port}\\b`, 'g'), '<port>')
    .replace(/rv_\d{8}T\d{6}/g, 'rv_<now>')
    .replace(/'nonce-[A-Za-z0-9+/=]+'/g, "'nonce-<n>'")
    .replace(/nonce="[A-Za-z0-9+/=]+"/g, 'nonce="<n>"');
}

/** What a client can observe of a response, masked. */
function seen(r, s, extra = []) {
  const out = { status: r.status, type: r.headers['content-type'] ?? null, cache: r.headers['cache-control'] ?? null, body: mask(r.raw.toString('utf8'), s) };
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
  let N;
  let R;
  before(async () => {
    tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-conformance-studio-')));
    [N, R] = await Promise.all([startStudio('node', tmp), startStudio('native', tmp)]);
  });
  after(() => {
    for (const s of [N, R]) s?.child.kill();
    fs.rmSync(tmp, { recursive: true, force: true });
  });

  /** The same request to both; their answers must match. */
  async function same(opts, extra = []) {
    const o = typeof opts === 'string' ? { path: opts } : opts;
    const hdrs = (s) => Object.fromEntries(Object.entries(o.headers || {}).map(([k, v]) => [k, String(v).replaceAll('<port>', s.port)]));
    const [a, b] = await Promise.all([N, R].map((s) => request(s.port, { ...o, headers: hdrs(s) })));
    const [x, y] = [seen(a, N, extra), seen(b, R, extra)];
    assert.deepEqual(y, x, `${o.method || 'GET'} ${o.path}`);
    return x;
  }

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
    for (const s of [N, R]) fs.renameSync(path.join(s.root, 'design/design.json'), path.join(s.root, 'design.json.off'));
    try {
      const m = JSON.parse((await same('/__dfw/api/manifest')).body);
      assert.equal(m.inferred, true);
    } finally {
      for (const s of [N, R]) fs.renameSync(path.join(s.root, 'design.json.off'), path.join(s.root, 'design/design.json'));
    }
  });

  test('Host and Origin: foreign host refused, cross-origin writes refused, reads allowed', async () => {
    await same({ path: '/__dfw/api/comments', headers: { host: 'evil.example' } });
    await same({ path: '/__dfw/api/manifest', headers: { host: 'localhost:<port>' } });
    await same({ path: '/__dfw/api/manifest', headers: { origin: 'https://evil.example' } });
    const body = JSON.stringify({ comment: 'from another site' });
    await same({ path: '/__dfw/api/comments', method: 'POST', headers: { origin: 'https://evil.example', 'content-type': 'text/plain' }, body });
    await same({ path: '/__dfw/api/edit', method: 'POST', headers: { origin: 'http://127.0.0.1:1' }, body: '{}' });
    assert.deepEqual(tree(R), tree(N));
  });

  test('comments lifecycle with events: add, list, patch, new session, state', async () => {
    const [en, er] = await Promise.all([events(N), events(R)]);
    try {
      const hello = await Promise.all([en.next('hello'), er.next('hello')]);
      assert.deepEqual(hello[1], hello[0]);
      const post = { path: '/__dfw/api/comments', method: 'POST', headers: { origin: 'http://127.0.0.1:<port>', 'content-type': 'application/json' } };
      assert.equal((await same({ ...post, body: JSON.stringify({ comment: 'Make it bold', screen: 'items', variant: 'a', breakpoint: 'desktop', file: 'screens/items/a.html', target: { dfwId: 'items.title', tag: 'h1', text: 'Inventory' } }) })).status, 201);
      const ev = await Promise.all([en.next('comments'), er.next('comments')]);
      assert.deepEqual(ev[1], ev[0]);
      await same({ ...post, body: '{"comment":"  "}' });
      await same({ ...post, body: '' });
      await same('/__dfw/api/comments');
      const patch = { path: '/__dfw/api/comments/fb_001', method: 'PATCH', headers: { 'content-type': 'application/json' } };
      await same({ ...patch, body: JSON.stringify({ message: 'Only the title, please' }) });
      await same({ ...patch, body: JSON.stringify({ status: 'dismissed', message: 'Not needed' }) });
      await same({ ...patch, body: JSON.stringify({ status: 'bogus' }) });
      await same({ path: '/__dfw/api/comments/fb_999', method: 'PATCH', body: '{}' });
      const from = [en.count(), er.count()];
      await same({ path: '/__dfw/api/sessions', method: 'POST' });
      const ev2 = await Promise.all([en.next('comments', from[0]), er.next('comments', from[1])]);
      assert.deepEqual(ev2[1], ev2[0]);
      await same('/__dfw/api/state');
      await same('/__dfw/api/comments');
      assert.deepEqual(tree(R), tree(N));
    } finally {
      en.close();
      er.close();
    }
  });

  test('edit and undo: same answers, same files written', async () => {
    const edit = (op) => same({ path: '/__dfw/api/edit', method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify(op) });
    assert.equal((await edit({ file: 'screens/items/a.html', op: 'text', id: 'items.title', value: 'Stock <now> & "then"' })).status, 200);
    await edit({ file: 'screens/items/a.html', op: 'text', id: 'items.summary', value: 'x' });
    await edit({ file: '../outside.html', op: 'text', id: 'x', value: 'y' });
    // The JSON parsers word their errors differently: only the status is compared.
    const bad = await Promise.all([N, R].map((s) => request(s.port, { path: '/__dfw/api/edit', method: 'POST', body: '{not json' })));
    assert.deepEqual(bad.map((r) => r.status), [500, 500]);
    const strip = (t) => Object.fromEntries(Object.entries(t).filter(([k]) => !k.startsWith('.dfw/edits/')));
    assert.deepEqual(strip(tree(R)), strip(tree(N)));
    const journal = (s) => Object.entries(tree(s)).filter(([k]) => k.startsWith('.dfw/edits/')).map(([, v]) => JSON.parse(v));
    assert.deepEqual(journal(R), journal(N));
    await same({ path: '/__dfw/api/undo', method: 'POST' });
    await same({ path: '/__dfw/api/undo', method: 'POST' });
    assert.deepEqual(tree(R), tree(N));
  });

  test('live reload: a design change and a review file change reach the pages; presence is relayed', async () => {
    const [en, er] = await Promise.all([events(N), events(R)]);
    try {
      await Promise.all([en.next('hello'), er.next('hello')]);
      for (const s of [N, R]) fs.appendFileSync(path.join(s.root, 'design/screens/items/a.html'), '\n<!-- touched -->\n');
      const d = await Promise.all([en.next('design'), er.next('design')]);
      assert.ok(d[0], 'node design event');
      assert.deepEqual(d[1], d[0]);
      const from = [en.count(), er.count()];
      for (const s of [N, R]) write(s.root, '.dfw/reviews/rv_test.json', { comments: [] });
      const c = await Promise.all([en.next('comments', from[0]), er.next('comments', from[1])]);
      assert.ok(c[0], 'node comments event');
      assert.deepEqual(c[1], c[0]);
      for (const s of [N, R]) write(s.root, '.dfw/presence.json', { file: 'screens/items/a.html', dfwId: 'items.title', at: new Date().toISOString() });
      const p = await Promise.all([en.next('presence'), er.next('presence')]);
      assert.deepEqual(p[0], { name: 'presence', data: '{"file":"screens/items/a.html","dfwId":"items.title"}' });
      assert.deepEqual(p[1], p[0]);
    } finally {
      en.close();
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
    assert.deepEqual(t(R), t(N));
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

async function startProxy(impl, tmp, target) {
  const root = path.join(tmp, impl, 'project');
  fs.mkdirSync(root, { recursive: true });
  write(root, '.dfw/state.json', `${JSON.stringify(MINIMAL_STATE, null, 2)}\n`);
  write(root, '.dfw/reviews/rv_1.json', { schema: 1, id: 'rv_1', startedAt: NOW, designVersion: 'v1', comments: [] });
  const port = await freePort();
  const env = { ...process.env, DFW_HOME: path.join(tmp, impl, 'home'), DFW_NOW: NOW, DFW_PLUGIN_ROOT: path.join(REPO, 'core') };
  for (const k of ['DFW_NATIVE', 'DFW_FORWARDED']) delete env[k];
  const [cmd, args] = impl === 'node' ? [process.execPath, [NODE_CLI]] : [nativeBinary(), []];
  const child = spawn(cmd, [...args, '__review-server', '--root', root, '--url', target, '--port', String(port), '--no-shots'], { env, stdio: 'ignore' });
  await ready(port, '/__dfw/api/live');
  return { impl, root, port, child };
}

describe('review proxy: Node and native answer the same', { skip: skipReason() }, () => {
  let tmp;
  let app;
  let N;
  let R;
  before(async () => {
    tmp = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-conformance-review-')));
    app = await fakeApp();
    const target = `http://localhost:${app.address().port}`;
    [N, R] = await Promise.all([startProxy('node', tmp, target), startProxy('native', tmp, target)]);
  });
  after(() => {
    for (const s of [N, R]) s?.child.kill();
    app?.close();
    app?.closeAllConnections();
    fs.rmSync(tmp, { recursive: true, force: true });
  });

  const HDRS = ['content-security-policy', 'content-security-policy-report-only', 'etag', 'location', 'set-cookie', 'content-encoding'];
  async function same(opts) {
    const o = typeof opts === 'string' ? { path: opts } : opts;
    const hdrs = (s) => Object.fromEntries(Object.entries({ host: 'localhost:<port>', ...o.headers }).map(([k, v]) => [k, String(v).replaceAll('<port>', s.port)]));
    const [a, b] = await Promise.all([N, R].map((s) => request(s.port, { ...o, headers: hdrs(s) })));
    const [x, y] = [seen(a, N, HDRS), seen(b, R, HDRS)];
    assert.deepEqual(y, x, `${o.method || 'GET'} ${o.path}`);
    return x;
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
    assert.deepEqual(tree(R), tree(N));
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
    const [a, b] = await Promise.all([ws(N), ws(R)]);
    assert.match(a, /^HTTP\/1\.1 101/);
    assert.equal(b, a);
  });

  test('app down: a readable 502', async () => {
    const port = await freePort();
    const [n, r] = await Promise.all([startProxy('node-down', tmp, `http://127.0.0.1:${port}`), startProxy('native-down', tmp, `http://127.0.0.1:${port}`)]);
    try {
      const [a, b] = await Promise.all([n, r].map((s) => request(s.port, { path: '/' })));
      assert.deepEqual([a.status, b.status], [502, 502]);
      for (const x of [a, b]) assert.match(x.raw.toString(), /dfw review: the app is not reachable/);
      assert.equal(b.headers['content-type'], a.headers['content-type']);
    } finally {
      n.child.kill();
      r.child.kill();
    }
  });
});
