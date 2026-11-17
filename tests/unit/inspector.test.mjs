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
import { createLiveServer, injectLive, assertLocalTarget, LIVE_TAGS } from '../../core/lib/live.mjs';
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
    assert.ok(html.includes(LIVE_TAGS), 'overlay injected');
    assert.equal(page.headers.get('etag'), null, 'injected HTML is not cached under the upstream etag');
    assert.ok((await (await fetch(`${L.base}/gz`)).text()).includes(LIVE_TAGS), 'gzip HTML decoded and injected');
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
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'c', bubbles: true }));
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
