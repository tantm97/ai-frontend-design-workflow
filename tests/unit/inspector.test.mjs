// Live-app source mapping: data-insp-path parsing, "dfw inspector setup" (G3 gating, config
// patching on fixture projects) and "dfw review" (proxy + overlay injection + comments).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import http from 'node:http';
import net from 'node:net';
import zlib from 'node:zlib';
import { fileURLToPath } from 'node:url';

import { parseInspPath, detectBundler, patchConfig, inspectorPlan, inspectorSetup, stackApproves, INSPECTOR } from '../../core/lib/inspector.mjs';
import { createLiveServer, startLiveServer, injectLive, assertLocalTarget, LIVE_TAGS, liveTags, normalizeShortcut, patchCsp, patchMetaCsp, fixSetCookie } from '../../core/lib/live.mjs';
import { newState, saveState, loadState, approveGate } from '../../core/lib/state.mjs';
import { allComments } from '../../core/lib/comments.mjs';
import { undelivered, formatForClaude } from '../../core/lib/inbox.mjs';
import { writeJson } from '../../core/lib/util.mjs';

const FIX = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '../fixtures/inspector');

function copyFixture(name) {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), `dfw-insp-${name}-`));
  fs.cpSync(path.join(FIX, name), dir, { recursive: true });
  process.env.DFW_HOME = path.join(dir, '.home');
  return dir;
}

test('parseInspPath: file:line:col:tag, Windows paths, absolute paths, missing attribute', () => {
  assert.deepEqual(parseInspPath('src/components/Foo.tsx:42:7:div'), { file: 'src/components/Foo.tsx', line: 42, column: 7, tag: 'div', source: 'src/components/Foo.tsx:42:7' });
  assert.equal(parseInspPath('src/App.vue:3:5:el-button').source, 'src/App.vue:3:5');
  assert.equal(parseInspPath('src/App.tsx:3:5').source, 'src/App.tsx:3:5', 'no tag');
  assert.equal(parseInspPath('src/App.tsx:3:Header').source, 'src/App.tsx:3', 'no column');
  assert.equal(parseInspPath('./src/a.jsx:1:1:p').file, 'src/a.jsx');
  const win = parseInspPath('C:\\work\\shop\\src\\components\\Foo.tsx:42:7:div');
  assert.equal(win.file, 'C:/work/shop/src/components/Foo.tsx');
  assert.equal(win.line, 42);
  assert.equal(parseInspPath('C:\\work\\shop\\src\\components\\Foo.tsx:42:7:div', 'C:\\work\\shop').source, 'src/components/Foo.tsx:42:7', 'Windows absolute -> project-relative');
  assert.equal(parseInspPath('C:/work/shop/src/Foo.tsx:9:1:span', 'C:\\work\\shop').source, 'src/Foo.tsx:9:1');
  assert.equal(parseInspPath('/home/me/shop/src/Foo.tsx:42:7:div', '/home/me/shop').source, 'src/Foo.tsx:42:7', 'POSIX absolute -> relative');
  assert.equal(parseInspPath('/elsewhere/lib/x.tsx:1:2:a', '/home/me/shop').source, '/elsewhere/lib/x.tsx:1:2', 'outside the project stays absolute');
  for (const bad of [null, undefined, '', '   ', 'src/Foo.tsx', 'div', 42, ':1:2']) assert.equal(parseInspPath(bad), null, String(bad));
});

test('detectBundler: vite, webpack, next (webpack vs turbopack), nuxt', () => {
  assert.deepEqual([detectBundler(path.join(FIX, 'vite')).bundler, detectBundler(path.join(FIX, 'vite')).configFile], ['vite', 'vite.config.ts']);
  assert.equal(detectBundler(path.join(FIX, 'webpack')).bundler, 'webpack');
  const n14 = detectBundler(path.join(FIX, 'next-webpack'));
  assert.deepEqual([n14.bundler, n14.nextMode, n14.configFile], ['next', 'webpack', 'next.config.js']);
  assert.equal(detectBundler(path.join(FIX, 'next16')).nextMode, 'turbopack', 'Next 16 dev uses Turbopack');
  assert.equal(detectBundler(path.join(FIX, 'nuxt')).bundler, 'nuxt');
});

test('patchConfig: vite adds the import and the plugin to the top-level plugins list only', () => {
  const src = fs.readFileSync(path.join(FIX, 'vite/vite.config.ts'), 'utf8');
  const r = patchConfig(src, { bundler: 'vite', file: 'vite.config.ts' });
  assert.equal(r.changed, true);
  assert.match(r.text, /import react from '@vitejs\/plugin-react';\nimport \{ codeInspectorPlugin \} from 'code-inspector-plugin';/);
  assert.match(r.text, /^ {2}plugins: \[codeInspectorPlugin\(\{ bundler: 'vite', pathType: 'relative', hotKeys: false, showSwitch: false, hideConsole: true, server: 'close' \}\), react\(\)\],$/m);
  assert.match(r.text, /rollupOptions: \{ plugins: \[\] \}/, 'nested plugin lists untouched');
  assert.deepEqual(patchConfig(r.text, { bundler: 'vite', file: 'vite.config.ts' }), { text: r.text, changed: false }, 'idempotent');
  const fn = patchConfig(fs.readFileSync(path.join(FIX, 'vite-noplugins/vite.config.js'), 'utf8'), { bundler: 'vite', file: 'vite.config.js' });
  assert.match(fn.text, /defineConfig\(\(\{ mode \}\) => \(\{\n {2}plugins: \[codeInspectorPlugin\(\{ bundler: 'vite'/, 'function config without plugins gets a plugins list');
});

test('patchConfig: webpack (CommonJS) and Next.js (webpack dev-only hook, Turbopack rules)', () => {
  const wp = patchConfig(fs.readFileSync(path.join(FIX, 'webpack/webpack.config.js'), 'utf8'), { bundler: 'webpack', file: 'webpack.config.js' });
  assert.match(wp.text, /^'use strict';\nconst \{ codeInspectorPlugin \} = require\('code-inspector-plugin'\);/);
  assert.match(wp.text, /plugins: \[codeInspectorPlugin\(\{ bundler: 'webpack'.*\}\), new HtmlWebpackPlugin\(\)\]/);

  const n14 = patchConfig(fs.readFileSync(path.join(FIX, 'next-webpack/next.config.js'), 'utf8'), { bundler: 'next', file: 'next.config.js', nextMode: 'webpack' });
  assert.match(n14.text, /require\('code-inspector-plugin'\)/);
  assert.match(n14.text, /webpack: \(config, \{ dev \}\) => \{\n {4}if \(dev\) config\.plugins\.push\(codeInspectorPlugin\(\{ bundler: 'webpack'/, 'only in dev');
  assert.match(n14.text, /reactStrictMode: true/);

  const n16 = patchConfig(fs.readFileSync(path.join(FIX, 'next16/next.config.ts'), 'utf8'), { bundler: 'next', file: 'next.config.ts', nextMode: 'turbopack' });
  assert.match(n16.text, /import type \{ NextConfig \} from 'next';\nimport \{ codeInspectorPlugin \} from 'code-inspector-plugin';/);
  assert.match(n16.text, /const nextConfig: NextConfig = \{\n {2}turbopack: \{ rules: codeInspectorPlugin\(\{ bundler: 'turbopack'/);

  assert.ok(patchConfig('module.exports = { webpack: (c) => c };', { bundler: 'next', nextMode: 'webpack' }).manual, 'existing webpack hook: manual');
  assert.ok(patchConfig('export default defineNuxtConfig({})', { bundler: 'nuxt' }).manual);
  assert.ok(patchConfig('const x = 1;', { bundler: 'vite', file: 'vite.config.js' }).manual, 'unknown shape: manual, no guess');
});

test('inspectorPlan: pinned dev dependency; unsupported bundlers get manual instructions', () => {
  const p = inspectorPlan(path.join(FIX, 'vite'));
  assert.equal(p.supported, true);
  assert.deepEqual(p.dependency.name, 'code-inspector-plugin');
  assert.match(INSPECTOR.version, /^\d+\.\d+\.\d+$/, 'exact version');
  assert.equal(p.dependency.dev, true);
  const nuxt = inspectorPlan(path.join(FIX, 'nuxt'));
  assert.equal(nuxt.supported, false);
  assert.match(nuxt.manual, /vite: \{ plugins: \[codeInspectorPlugin/);
  assert.equal(inspectorPlan(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-insp-empty-'))).supported, false);
});

test('inspector setup --write: refused without G3 or without the dependency in the stack, applied after approval', () => {
  const root = copyFixture('vite');
  const before = fs.readFileSync(path.join(root, 'vite.config.ts'), 'utf8');
  assert.throws(() => inspectorSetup(root, { write: true }), /refused: G3 .*not approved/, 'no workflow state');
  const s = newState({ mode: 'existing-project' });
  saveState(root, s);
  writeJson(path.join(root, '.dfw/stack.json'), { kind: 'javascript', framework: { name: 'React' }, dependencies: [] });
  approveGate(root, loadState(root), 'G3', { note: 'React + Vite' });
  assert.throws(() => inspectorSetup(root, { write: true }), /does not list "code-inspector-plugin"/, 'G3 approved but the dependency was not');
  assert.equal(fs.readFileSync(path.join(root, 'vite.config.ts'), 'utf8'), before, 'nothing written when refused');
  assert.equal(inspectorSetup(root, { write: false }).written.length, 0, 'dry run never writes');

  writeJson(path.join(root, '.dfw/stack.json'), { kind: 'javascript', dependencies: [{ name: 'code-inspector-plugin', version: INSPECTOR.version, dev: true }] });
  const r = inspectorSetup(root, { write: true });
  assert.deepEqual(r.written.sort(), ['package.json', 'vite.config.ts']);
  assert.match(fs.readFileSync(path.join(root, 'vite.config.ts'), 'utf8'), /codeInspectorPlugin\(\{ bundler: 'vite'/);
  const pkg = JSON.parse(fs.readFileSync(path.join(root, 'package.json'), 'utf8'));
  assert.equal(pkg.devDependencies['code-inspector-plugin'], INSPECTOR.version, 'pinned, devDependencies');
  assert.equal(pkg.dependencies['code-inspector-plugin'], undefined, 'never a runtime dependency');
  assert.deepEqual(inspectorSetup(root, { write: true }).written, [], 'second run: nothing to do');

  assert.ok(stackApproves({ devDependencies: { 'code-inspector-plugin': '2.0.9' } }));
  assert.ok(stackApproves({ dependencies: ['code-inspector-plugin@2.0.9'] }));
  assert.ok(!stackApproves({ dependencies: [{ name: 'react' }] }));
});

test('injectLive / assertLocalTarget', () => {
  assert.equal(injectLive('<html><head><title>x</title></head><body></body></html>'), `<html><head><title>x</title>${LIVE_TAGS}</head><body></body></html>`);
  assert.equal(injectLive('<body class="a"><p>x</p></body>'), `<body class="a">${LIVE_TAGS}<p>x</p></body>`);
  assert.equal(injectLive(injectLive('<head></head>')), injectLive('<head></head>'), 'once');
  assert.equal(assertLocalTarget('http://localhost:5173/cart').origin, 'http://localhost:5173');
  assert.ok(assertLocalTarget('http://127.0.0.1:3000'));
  assert.throws(() => assertLocalTarget('https://example.com'), /only proxies a local dev server/);
  assert.throws(() => assertLocalTarget('file:///etc/passwd'), /http\(s\)/);
});

const APP_HTML = `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Shop</title></head>
<body><main data-insp-path="src/pages/Cart.tsx:10:5:main"><h1 data-insp-path="src/pages/Cart.tsx:11:7:h1">Cart</h1>
<section data-insp-path="ABS/src/components/Summary.tsx:42:7:section"><p id="total"><span class="amount">$12.00</span></p></section>
<button id="plain">No source</button></main></body></html>`;

/** A tiny "dev server": HTML with data-insp-path, a gzip route, a redirect, and a WebSocket-style upgrade echo. */
async function fakeApp(root) {
  const html = APP_HTML.replace('ABS', root.split(path.sep).join('/'));
  const app = http.createServer((req, res) => {
    if (req.url === '/gz') {
      res.writeHead(200, { 'content-type': 'text/html', 'content-encoding': 'gzip' });
      return res.end(zlib.gzipSync('<html><head></head><body>gz</body></html>'));
    }
    if (req.url === '/old') {
      res.writeHead(302, { location: `http://127.0.0.1:${app.address().port}/cart` });
      return res.end();
    }
    if (req.url === '/app.js') {
      res.writeHead(200, { 'content-type': 'text/javascript' });
      return res.end('console.log(1)');
    }
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', etag: '"x"' });
    res.end(html);
  });
  app.on('upgrade', (req, socket) => {
    socket.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nX-Seen-Host: ${req.headers.host}\r\n\r\n`);
    socket.on('data', (d) => socket.write(`echo:${d}`));
  });
  await new Promise((r) => app.listen(0, '127.0.0.1', r));
  return app;
}

async function liveSetup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-live-'));
  process.env.DFW_HOME = path.join(root, '.home');
  saveState(root, newState({ mode: 'existing-project' }));
  const app = await fakeApp(root);
  const target = `http://127.0.0.1:${app.address().port}`;
  const proxy = createLiveServer({ root, target, shots: false });
  await new Promise((r) => proxy.listen(0, '127.0.0.1', r));
  const base = `http://127.0.0.1:${proxy.address().port}`;
  const sockets = new Set();
  for (const srv of [app, proxy]) srv.on('connection', (s) => sockets.add(s));
  const close = () => {
    proxy.close();
    app.close();
    for (const s of sockets) s.destroy();
  };
  return { root, app, proxy, base, target, close };
}

test('dfw review proxy: injects the overlay into HTML, passes assets, redirects and upgrades through, stores comments with source', async () => {
  const L = await liveSetup();
  try {
    const page = await fetch(`${L.base}/cart`);
    const html = await page.text();
    assert.match(html, /<script defer src="\/__dfw\/overlay\.js" data-dfw-live nonce="[A-Za-z0-9+/=]+"><\/script><script defer src="\/__dfw\/live\.js" data-shortcut="alt\+c" nonce="[A-Za-z0-9+/=]+"><\/script><\/head>/, 'overlay injected');
    assert.equal(page.headers.get('etag'), null, 'injected HTML is not cached under the upstream etag');
    assert.ok((await (await fetch(`${L.base}/gz`)).text()).includes('data-dfw-live'), 'gzip HTML decoded and injected');
    assert.equal(await (await fetch(`${L.base}/app.js`)).text(), 'console.log(1)', 'assets untouched');
    const redirect = await fetch(`${L.base}/old`, { redirect: 'manual' });
    assert.equal(redirect.headers.get('location'), `${L.base}/cart`, 'redirects stay on the proxy');
    for (const f of ['overlay.js', 'live.js']) assert.equal((await fetch(`${L.base}/__dfw/${f}`)).status, 200);

    // WebSocket upgrade (HMR) passthrough.
    const reply = await new Promise((resolve, reject) => {
      const s = net.connect(L.proxy.address().port, '127.0.0.1', () => s.write(`GET /hmr HTTP/1.1\r\nHost: 127.0.0.1:${L.proxy.address().port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n`));
      let buf = '';
      s.on('data', (d) => {
        buf += d;
        if (buf.includes('\r\n\r\n') && !buf.includes('echo:')) s.write('ping');
        if (buf.includes('echo:ping')) {
          s.destroy();
          resolve(buf);
        }
      });
      s.on('error', reject);
    });
    assert.match(reply, /^HTTP\/1\.1 101/);
    assert.match(reply, new RegExp(`X-Seen-Host: ${L.target.replace('http://', '')}`), 'Host rewritten to the app');

    // A comment as live.js posts it: absolute data-insp-path -> project-relative source.
    const abs = `${L.root.split(path.sep).join('/')}/src/components/Summary.tsx:42:7:section`;
    const res = await fetch(`${L.base}/__dfw/api/comments`, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ comment: 'Total should be bold', screen: '/cart', url: '/cart?x=1', viewport: [390, 844], target: { selector: '#total > span', tag: 'span', text: '$12.00', insp: abs, source: 'evil/should-not-pass.ts:1' } }),
    });
    assert.equal(res.status, 201);
    const c = await res.json();
    assert.equal(c.target.source, 'src/components/Summary.tsx:42:7');
    assert.equal(c.target.inspPath, abs);
    assert.deepEqual([c.screen, c.variant, c.url, c.breakpoint, c.provider, c.file], ['/cart', 'live', '/cart?x=1', 'mobile', 'dfw-live', null]);
    // Without the inspector attribute: selector + text only.
    const c2 = await (await fetch(`${L.base}/__dfw/api/comments`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ comment: 'Make it primary', url: '/cart', target: { selector: '#plain', text: 'No source' } }) })).json();
    assert.equal(c2.target.source, null);
    assert.equal(c2.target.selector, '#plain');
    assert.equal((await fetch(`${L.base}/__dfw/api/comments`, { method: 'POST', headers: { 'content-type': 'application/json' }, body: '{"comment":"  "}' })).status, 400);

    // Same inbox as Studio comments.
    const inbox = undelivered(L.root);
    assert.deepEqual(inbox.map((x) => x.id), [c.id, c2.id]);
    const msg = formatForClaude(inbox);
    assert.match(msg, /\/cart \/ live \/ mobile -> #total > span \(src\/components\/Summary\.tsx:42:7\)/);
    assert.match(msg, /open target\.source/);
    assert.equal(allComments(L.root).length, 2);
  } finally {
    L.close();
  }
});

test('dfw review proxy: app down -> readable 502', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-live-down-'));
  const free = await new Promise((r) => {
    const s = net.createServer().listen(0, '127.0.0.1', () => {
      const p = s.address().port;
      s.close(() => r(p));
    });
  });
  const proxy = createLiveServer({ root, target: `http://127.0.0.1:${free}`, shots: false });
  await new Promise((r) => proxy.listen(0, '127.0.0.1', r));
  try {
    const res = await fetch(`http://127.0.0.1:${proxy.address().port}/`);
    assert.equal(res.status, 502);
    assert.match(await res.text(), /not reachable/);
  } finally {
    proxy.close();
  }
});

test('dfw review in a browser: click-to-comment carries the nearest data-insp-path (needs Chrome)', { skip: !(await import('../../core/lib/runtime.mjs')).browserStatus().installed }, async () => {
  const { openRenderer } = await import('../../core/lib/runtime.mjs');
  const L = await liveSetup();
  const r = await openRenderer();
  try {
    const page = await r.page({ width: 1440, height: 900 });
    await page.goto(`${L.base}/cart`);
    const ok = await page.evaluate(async () => {
      for (let i = 0; i < 50 && !window.__dfwLive; i++) await new Promise((res) => setTimeout(res, 50));
      const ui = document.querySelector('dfw-live').shadowRoot;
      // Option+C on macOS types "ç": matched by event.code.
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ç', code: 'KeyC', altKey: true, bubbles: true }));
      if (ui.querySelector('.toggle').getAttribute('aria-pressed') !== 'true') return 'comment mode not on';
      const span = document.querySelector('#total .amount');
      span.dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      if (ui.querySelector('.pop').hidden) return 'composer not open';
      const src = ui.querySelector('.src').textContent;
      ui.querySelector('textarea').value = 'Total should be bold';
      ui.querySelector('.pop').requestSubmit();
      for (let i = 0; i < 50 && !/Sent to Claude/.test(ui.querySelector('.toast').textContent); i++) await new Promise((res) => setTimeout(res, 50));
      return `${src}|${ui.querySelector('.toast').textContent}`;
    });
    assert.match(ok, /Summary\.tsx:42:7\|Sent to Claude · comment #001/);
    const [c] = allComments(L.root);
    assert.equal(c.target.source, 'src/components/Summary.tsx:42:7', 'nearest ancestor with the attribute');
    assert.equal(c.target.tag, 'span');
    assert.equal(c.target.text, '$12.00');
    assert.match(c.target.selector, /#total > span/);
    assert.equal(c.screen, '/cart');
    assert.equal(c.breakpoint, 'desktop');
  } finally {
    await r.close();
    L.close();
  }
});

test('review shortcut: normalised combos, Alt+C default, invalid combos refused', () => {
  assert.equal(normalizeShortcut('Alt+C'), 'alt+c');
  assert.equal(normalizeShortcut('option+c'), 'alt+c');
  assert.equal(normalizeShortcut('shift+ctrl+k'), 'ctrl+shift+k');
  assert.equal(normalizeShortcut('cmd+shift+1'), 'shift+meta+1');
  assert.equal(normalizeShortcut('c'), 'c', 'a bare key is allowed when the user asks for it');
  for (const off of ['off', 'none', '', false]) assert.equal(normalizeShortcut(off), 'off');
  for (const bad of ['alt', 'alt+cc', 'alt+c+d', 'hyper+c', 'ctrl+enter']) assert.throws(() => normalizeShortcut(bad), /Invalid review shortcut/, bad);
  assert.match(LIVE_TAGS, /data-shortcut="alt\+c"/, 'Alt/Option+C by default, not a bare C');
  assert.match(liveTags({ shortcut: 'off', nonce: 'a"b' }), /data-shortcut="off" nonce="a&quot;b"/);
});

test('patchCsp: adds the proxy origin + nonce for the injected scripts and the comment POST, nothing else', () => {
  const o = { origin: 'http://localhost:4700', nonce: 'N0nce' };
  // strict-dynamic: host sources are ignored, so the nonce is what lets the scripts run.
  assert.equal(
    patchCsp("default-src 'none'; script-src 'strict-dynamic' 'nonce-app' https:; style-src 'self'; img-src data:; frame-ancestors 'none'; report-uri /csp", o),
    "default-src 'none'; script-src 'strict-dynamic' 'nonce-app' https: http://localhost:4700 'nonce-N0nce'; style-src 'self'; img-src data:; frame-ancestors 'none'; report-uri /csp; connect-src http://localhost:4700",
  );
  // default-src only: script-src/connect-src are created from it, default-src itself untouched.
  assert.equal(patchCsp("default-src 'self' https://cdn.example", o), "default-src 'self' https://cdn.example; script-src 'self' https://cdn.example 'nonce-N0nce'; connect-src 'self' https://cdn.example");
  // 'unsafe-inline' without nonces: adding a nonce would disable the app's inline scripts, so origin only.
  assert.equal(patchCsp("script-src https://cdn.example 'unsafe-inline'", o), "script-src https://cdn.example 'unsafe-inline' http://localhost:4700");
  // script-src-elem and script-src both patched; several policies patched one by one.
  assert.equal(patchCsp("script-src-elem 'self'; script-src 'none', connect-src 'self'", o), "script-src-elem 'self' 'nonce-N0nce'; script-src http://localhost:4700 'nonce-N0nce', connect-src 'self'");
  assert.equal(patchCsp("img-src 'self'", o), "img-src 'self'", 'scripts and fetch not restricted: unchanged');
  assert.equal(patchCsp('', o), '');
  const meta = patchMetaCsp(`<head><meta http-equiv="Content-Security-Policy" content="script-src &#39;nonce-app&#39;"><META HTTP-EQUIV='content-security-policy' CONTENT="default-src 'self'"></head>`, o);
  assert.equal(meta, `<head><meta http-equiv="Content-Security-Policy" content="script-src 'nonce-app' http://localhost:4700 'nonce-N0nce'"><META HTTP-EQUIV='content-security-policy' content="default-src 'self'; script-src 'self' 'nonce-N0nce'; connect-src 'self'"></head>`);
});

test('fixSetCookie: only a Domain naming the other loopback host is dropped', () => {
  assert.deepEqual(fixSetCookie(['a=1; Domain=127.0.0.1; Path=/', 'b=2; domain=localhost; HttpOnly', 'c=3; Path=/', 'd=4; Domain=example.test'], 'localhost'), ['a=1; Path=/', 'b=2; domain=localhost; HttpOnly', 'c=3; Path=/', 'd=4; Domain=example.test']);
  assert.deepEqual(fixSetCookie('a=1; Domain=.localhost', '127.0.0.1'), ['a=1']);
});

const STRICT = "default-src 'none'; script-src 'nonce-app' 'strict-dynamic'; style-src 'self'; img-src 'self'; frame-ancestors 'none'";

/** An app with a strict CSP, a login cookie and its own single-key "c" shortcut. */
async function strictApp() {
  const seen = [];
  const app = http.createServer((req, res) => {
    seen.push({ url: req.url, cookie: req.headers.cookie || '' });
    if (req.url === '/login') {
      res.writeHead(200, { 'content-type': 'text/plain', 'set-cookie': ['sid=s3cret; Path=/; HttpOnly; SameSite=Lax', 'pref=1; Domain=127.0.0.1; Path=/'] });
      return res.end('ok');
    }
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'content-security-policy': STRICT, 'content-security-policy-report-only': "script-src 'nonce-app'" });
    res.end(`<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="script-src 'nonce-app' 'strict-dynamic'"><title>Strict</title>
<script nonce="app">window.appKeys = []; addEventListener('keydown', (e) => appKeys.push(e.key));</script></head>
<body><main><h1 id="t">Strict app</h1><input id="q"><p id="who">${String(req.headers.cookie || 'anon').replace(/[<&]/g, '')}</p></main></body></html>`);
  });
  await new Promise((r) => app.listen(0, '127.0.0.1', r));
  return { app, seen };
}

test('dfw review proxy: strict CSP is extended for the injected scripts only; cookies pass both ways', async () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-live-csp-'));
  process.env.DFW_HOME = path.join(root, '.home');
  const { app, seen } = await strictApp();
  const { server, url } = await startLiveServer({ root, target: `http://localhost:${app.address().port}`, port: 0, shots: false, shortcut: 'Option+C' });
  try {
    const port = server.address().port;
    assert.equal(url, `http://localhost:${port}/`, 'localhost app -> localhost proxy URL (same cookie jar)');
    assert.equal(server.address().address, '127.0.0.1', 'loopback only, never 0.0.0.0');
    const res = await fetch(url, { headers: { cookie: 'sid=s3cret' } });
    const html = await res.text();
    const nonce = html.match(/data-dfw-live nonce="([^"]+)"/)[1];
    assert.match(html, /data-shortcut="alt\+c"/, '--shortcut normalised into the page');
    const csp = res.headers.get('content-security-policy');
    assert.equal(csp, `default-src 'none'; script-src 'nonce-app' 'strict-dynamic' http://localhost:${port} 'nonce-${nonce}'; style-src 'self'; img-src 'self'; frame-ancestors 'none'; connect-src http://localhost:${port}`);
    assert.equal(res.headers.get('content-security-policy-report-only'), `script-src 'nonce-app' http://localhost:${port} 'nonce-${nonce}'`);
    assert.ok(html.includes(`content="script-src 'nonce-app' 'strict-dynamic' http://localhost:${port} 'nonce-${nonce}'"`), 'meta CSP patched too');
    assert.ok(html.includes(`<script nonce="app">`), "the app's own nonce untouched");
    assert.equal(seen.at(-1).cookie, 'sid=s3cret', 'Cookie header forwarded to the app');
    const second = await fetch(url);
    assert.notEqual((await second.text()).match(/data-dfw-live nonce="([^"]+)"/)[1], nonce, 'a fresh nonce per response');
    const login = await fetch(`${url}login`);
    assert.deepEqual(login.headers.getSetCookie(), ['sid=s3cret; Path=/; HttpOnly; SameSite=Lax', 'pref=1; Path=/'], 'Domain=127.0.0.1 dropped for a localhost page, the rest untouched');
    assert.equal((await (await fetch(`http://127.0.0.1:${port}/__dfw/api/live`)).json()).url, url);
  } finally {
    server.close();
    app.close();
  }
});

test('dfw review in a browser: strict CSP app, localhost cookies, Alt/Option+C not C (needs Chrome)', { skip: !(await import('../../core/lib/runtime.mjs')).browserStatus().installed }, async () => {
  const { openRenderer } = await import('../../core/lib/runtime.mjs');
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-live-csp-b-'));
  process.env.DFW_HOME = path.join(root, '.home');
  saveState(root, newState({ mode: 'existing-project' }));
  const { app, seen } = await strictApp();
  const appUrl = `http://localhost:${app.address().port}`;
  const { server, url } = await startLiveServer({ root, target: appUrl, port: 0, shots: false });
  const r = await openRenderer();
  try {
    const page = await r.page({ width: 1280, height: 800 });
    await page.goto(`${appUrl}/login`); // the user signs in on the app itself
    await page.goto(url);
    assert.equal(seen.at(-1).cookie, 'sid=s3cret', "the app's localhost cookie reaches it through the proxy");
    const out = await page.evaluate(async () => {
      for (let i = 0; i < 50 && !window.__dfwLive; i++) await new Promise((res) => setTimeout(res, 50));
      if (!window.__dfwLive) return 'live.js blocked by CSP';
      if (typeof window.__dfwTargetInfo !== 'function') return 'overlay.js blocked by CSP';
      if (!Array.isArray(window.appKeys)) return "the app's own nonce script did not run";
      const ui = document.querySelector('dfw-live').shadowRoot;
      const pressed = () => ui.querySelector('.toggle').getAttribute('aria-pressed');
      const key = (init, el = document.body) => el.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, composed: true, ...init }));
      if (getComputedStyle(ui.querySelector('.bar')).position !== 'fixed') return 'review UI styles blocked by style-src';
      key({ key: 'c', code: 'KeyC' });
      if (pressed() !== 'false') return 'plain C was stolen from the app';
      if (!window.appKeys.includes('c')) return 'the app did not get C';
      const q = document.querySelector('#q');
      q.focus();
      key({ key: 'ç', code: 'KeyC', altKey: true }, q);
      if (pressed() !== 'false') return 'shortcut fired while typing in an input';
      q.blur();
      key({ key: 'ç', code: 'KeyC', altKey: true });
      if (pressed() !== 'true') return 'Option+C did not toggle comment mode';
      document.querySelector('#t').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      ui.querySelector('textarea').value = 'Bigger title';
      ui.querySelector('.pop').requestSubmit();
      for (let i = 0; i < 50 && !/Sent to Claude|Not sent/.test(ui.querySelector('.toast').textContent); i++) await new Promise((res) => setTimeout(res, 50));
      return ui.querySelector('.toast').textContent;
    });
    assert.match(out, /Sent to Claude · comment #001/, 'comment POST allowed by connect-src');
    assert.equal(allComments(root)[0].comment, 'Bigger title');
  } finally {
    await r.close();
    server.close();
    app.close();
  }
});
