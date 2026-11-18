// End-to-end test of the dfw Studio click-to-comment flow (real browser via Playwright).
// Usage: node tests/e2e/studio.e2e.mjs [--keep] [--shots <dir>]
// Dev-only: needs the "playwright" package (its click/keyboard API drives the Studio). It is
// resolved from this repo's node_modules or from $DFW_RUNTIME_DIR (a folder with playwright installed).
// The browser is Playwright's own Chromium, or $DFW_BROWSER / $CHROME_PATH (e.g. the CI runner's
// Google Chrome, so no browser download is needed). With $CI set, a missing playwright fails.
// dfw itself does not use Playwright (it renders through lib/cdp.mjs).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const DFW = path.join(REPO, 'core', 'bin', 'dfw.mjs');
const args = process.argv.slice(2);
const shotsDir = args.includes('--shots') ? path.resolve(args[args.indexOf('--shots') + 1]) : null;

function loadPlaywright() {
  for (const base of [REPO, process.env.DFW_RUNTIME_DIR].filter(Boolean)) {
    try {
      return createRequire(path.join(base, 'package.json'))('playwright');
    } catch {}
  }
  return null;
}
const pw = loadPlaywright();
if (!pw) {
  console.log('SKIP: playwright not found (npm i -D playwright, or set DFW_RUNTIME_DIR).');
  process.exit(process.env.CI ? 1 : 0);
}
if (shotsDir) fs.mkdirSync(shotsDir, { recursive: true });

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-e2e-'));
const env = { ...process.env, DFW_HOME: path.join(tmp, '.dfwhome') };
const dfw = (...a) => execFileSync(process.execPath, [DFW, ...a], { cwd: tmp, env, encoding: 'utf8' });

dfw('init', '--product', 'E2E Shop');
fs.mkdirSync(path.join(tmp, 'design/screens/home'), { recursive: true });
fs.writeFileSync(
  path.join(tmp, 'design/screens/home/a.html'),
  `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Home</title>
<link rel="stylesheet" href="../../tokens/tokens.css"><link rel="stylesheet" href="../../styles/base.css">
<style>main{padding:var(--space-8)} .cta{background:var(--color-accent);color:var(--color-surface);border:0;padding:var(--space-3) var(--space-6);border-radius:var(--radius-md)}</style>
</head><body><main data-dfw-id="home.main"><h1 data-dfw-id="home.title">Welcome back</h1>
<p data-dfw-id="home.lede">Your orders and stock at a glance.</p>
<button class="cta" data-dfw-id="home.cta">New order</button>
<p data-dfw-id="home.raw" style="color:#ff00aa">Hard-coded colour</p>
<dfw-use c="button" variant="primary" id="home.buy">Buy now</dfw-use>
<a data-dfw-id="home.link" href="../detail/a.html">Order details</a></main></body></html>`,
);
fs.mkdirSync(path.join(tmp, 'design/screens/detail'), { recursive: true });
fs.writeFileSync(
  path.join(tmp, 'design/screens/detail/a.html'),
  `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Detail</title><link rel="stylesheet" href="../../tokens/tokens.css"></head>
<body><main data-dfw-id="detail.main"><h1 data-dfw-id="detail.title">Order 1042</h1><button type="button" data-dfw-goto="home/b">Back home</button><ul id="made"></ul><img id="logo" alt="" src="../../assets/logo.svg" width="8" height="8"></main>
<script>for (const n of ['Bolts', 'Nuts']) document.getElementById('made').append(Object.assign(document.createElement('li'), { textContent: n }));</script></body></html>`,
);
const logoFile = path.join(tmp, 'design/assets/logo.svg');
const logo = (fill) => `<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="${fill}"/></svg>\n`;
fs.mkdirSync(path.dirname(logoFile), { recursive: true });
fs.writeFileSync(logoFile, logo('#ff0000'));
fs.mkdirSync(path.join(tmp, 'design/system/components'), { recursive: true });
fs.writeFileSync(
  path.join(tmp, 'design/system/components/button.html'),
  `<dfw-component name="button" variants="primary secondary" sizes="md lg" description="Action button">
<style>.btn{border:0;border-radius:var(--radius-md);padding:var(--space-2) var(--space-4)} .btn--primary{background:var(--color-accent);color:var(--color-surface)} .btn--secondary{background:var(--color-surface);color:var(--color-ink)}</style>
<template><button class="btn btn--{{variant}} btn--{{size}}" type="button"><dfw-slot></dfw-slot></button></template>
<dfw-examples><dfw-use c="button">Primary</dfw-use> <dfw-use c="button" variant="secondary">Secondary</dfw-use></dfw-examples>
</dfw-component>`,
);

function undoAll() {
  // Restore the source from the edit journal so the live-reload step starts clean.
  const dir = path.join(tmp, '.dfw/edits');
  for (const f of fs.existsSync(dir) ? fs.readdirSync(dir).sort().reverse() : []) {
    const rec = JSON.parse(fs.readFileSync(path.join(dir, f), 'utf8'));
    fs.writeFileSync(path.join(tmp, rec.file), rec.before);
    fs.unlinkSync(path.join(dir, f));
  }
}
const manifestFile = path.join(tmp, 'design/design.json');
const m = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
m.screens = [{ id: 'home', title: 'Home', variants: [{ id: 'a', label: 'A', file: 'screens/home/a.html' }, { id: 'b', label: 'B', file: 'screens/home/a.html' }] },
  { id: 'detail', title: 'Detail', variants: [{ id: 'a', label: 'A', file: 'screens/detail/a.html' }] },
];
m.states = ['empty', 'loading', 'error'];
fs.writeFileSync(manifestFile, JSON.stringify(m, null, 2));

const port = 4700 + Math.floor(Math.random() * 200);
const started = dfw('studio', 'start', '--port', String(port));
const url = started.match(/http:\/\/\S+/)[0];
const { chromium } = pw;
const executablePath = process.env.DFW_BROWSER || process.env.CHROME_PATH || undefined;
const browser = await chromium.launch(executablePath ? { executablePath } : {});
let failed = false;
let page = null;
try {
  page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(url);
  await page.waitForSelector('.frame iframe');
  const count = () => page.$$eval('.frame[data-key]', (f) => f.length); // frames, not iframes (off-screen iframes load lazily)
  const summary = (id) => page.textContent(`#${id} summary`);
  // Default: every breakpoint and every state. 2 screens x 3 breakpoints + 1 variant + 2 screens x 3 states.
  assert.equal(await count(), 13);
  assert.equal(await summary('bp-menu'), 'All breakpoints');
  assert.equal(await summary('state-menu'), 'States: all');
  const pages = await page.$$eval('#page-list button', (bs) => bs.map((b) => b.textContent));
  assert.match(pages[0], /All pages/, 'All pages entry first');
  assert.match(pages[1], /Design System/, 'Design System is the first page');
  assert.match(pages[2], /Flow\s*2/, 'Flow page (home <-> detail links) after the Design System');
  assert.match(pages[3], /Screens\s*13/, 'Screens page with frame count');
  // Flow page: one frame per linked screen, one labelled arrow per link.
  await page.click('#page-list button[data-page="flow"]');
  await page.waitForSelector('svg.flow path[marker-end]', { state: 'attached' });
  assert.equal(await page.$$eval('svg.flow path[marker-end]', (p) => p.length), 2);
  assert.deepEqual((await page.$$eval('svg.flow text', (t) => t.map((x) => x.textContent))).sort(), ['Back home', 'Order details']);
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-0-flow.png') });
  await page.click('#page-list button[data-page="screens"]');
  // State menu: "All" off -> no state frames; pick some -> only those; "All" back on.
  await page.click('#state-menu summary');
  await page.uncheck('#states .ms-all input');
  assert.equal(await count(), 7);
  assert.equal(await summary('state-menu'), 'States: off');
  await page.check('#states label:has-text("empty") input');
  await page.check('#states label:has-text("error") input');
  assert.equal(await count(), 11);
  assert.equal(await summary('state-menu'), 'States: empty, error');
  assert.equal(await page.$eval('#states .ms-all input', (i) => i.indeterminate), true);
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-0-menus.png') });
  await page.check('#states .ms-all input');
  assert.equal(await count(), 13);
  await page.uncheck('#states .ms-all input');
  // Breakpoint menu (opening it closes the state menu): "All" off keeps the first one.
  await page.click('#bp-menu summary');
  assert.equal(await page.$eval('#state-menu', (d) => d.open), false);
  await page.uncheck('#breakpoints .ms-all input');
  assert.equal(await count(), 3, 'desktop only: 2 screens + 1 variant');
  assert.equal(await summary('bp-menu'), 'Desktop 1440');
  await page.click('#breakpoints label:has-text("Desktop") input');
  assert.equal(await page.isChecked('#breakpoints label:has-text("Desktop") input'), true, 'last breakpoint stays on');
  await page.check('#breakpoints label:has-text("Tablet") input');
  await page.click('#bp-menu summary');
  // Auto sections: one row per breakpoint (primary variants) + a Variants row.
  assert.equal(await count(), 5, '2 screens x 2 breakpoints + 1 variant');
  assert.deepEqual(await page.$$eval('.prow-gutter h2', (h) => h.map((x) => x.textContent)), ['Desktop 1440', 'Tablet 834', 'Variants']);
  assert.equal(await page.textContent('.frame[data-key="home::a::desktop"] .frame-title'), 'D · Home · A↗');
  // Screenshot all: every screen of every page (5 frames + Design System) -> PNG files under .dfw/shots/studio/<time>/.
  // The camera opens a picker (all screens checked by default); "Screenshot N" shoots the checked ones.
  await page.click('#shoot-btn');
  await page.waitForFunction(() => /\d/.test(document.querySelector('#shoot-go').textContent)); // the menu renders on its (async) toggle event
  assert.equal(await page.textContent('#shoot-go'), 'Screenshot 6');
  await page.click('#shoot-go');
  await page.waitForFunction(() => /PNG saved/.test(document.querySelector('#toast').textContent), null, { timeout: 60000 });
  const shootMsg = await page.textContent('#toast');
  assert.match(shootMsg, /^6\/6 PNG saved in \.dfw\/shots\/studio\//);
  const shotRoot = path.join(tmp, '.dfw/shots/studio');
  const shotFiles = fs.readdirSync(path.join(shotRoot, fs.readdirSync(shotRoot)[0])).sort();
  assert.deepEqual(shotFiles, ['design-system.png', 'detail__a__desktop.png', 'detail__a__tablet.png', 'home__a__desktop.png', 'home__a__tablet.png', 'home__b__desktop.png']);
  // Pick only some screens: untick All, tick Detail -> only its frames are shot; the choice is remembered.
  await page.click('#shoot-btn');
  await page.uncheck('#shoot-items .ms-all input');
  assert.equal(await page.isDisabled('#shoot-go'), true, 'nothing picked');
  await page.check('#shoot-items label:has-text("Detail") input');
  assert.equal(await page.textContent('#shoot-go'), 'Screenshot 2');
  await page.click('#shoot-go');
  await page.waitForFunction(() => /^2\/2 PNG saved/.test(document.querySelector('#toast').textContent), null, { timeout: 60000 });
  assert.equal(await page.evaluate(() => document.querySelector('#shoot-menu').open), false, 'menu closes after shooting');
  await page.click('#shoot-btn');
  await page.waitForFunction(() => /\d/.test(document.querySelector('#shoot-go').textContent));
  assert.equal(await page.textContent('#shoot-go'), 'Screenshot 2', 'picked screens are remembered');
  await page.check('#shoot-items .ms-all input');
  await page.click('#shoot-btn');
  const bad = await page.evaluate(() => fetch('/__dfw/api/shoot', { method: 'POST', body: JSON.stringify({ frames: [{ src: 'file:///etc/passwd' }] }) }).then((r) => r.status));
  assert.equal(bad, 400, 'only Studio design URLs can be screenshotted');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-1-canvas.png') });

  // Enter comment mode and click the CTA inside the first (desktop, variant A) frame.
  await page.keyboard.press('c');
  const frame = page.frameLocator('iframe[name="home::a::desktop"]');
  await frame.locator('[data-dfw-id="home.cta"]').hover();
  await frame.locator('[data-dfw-id="home.cta"]').click();
  await page.waitForSelector('#composer:not([hidden])');
  const target = await page.textContent('#composer-target');
  assert.match(target, /home\.cta/);
  await page.fill('#composer-text', 'Make the primary action more prominent and move it next to the title.');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-2-composer.png') });
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');
  await page.waitForSelector('.comment[data-id="fb_001"]');
  assert.equal(await page.isVisible('#composer'), false, 'composer closes after saving');

  // The comment is stored in the feedback contract with source mapping.
  const stored = JSON.parse(dfw('comments', 'list', '--json'));
  assert.equal(stored.length, 1);
  assert.equal(stored[0].target.dfwId, 'home.cta');
  assert.equal(stored[0].screen, 'home');
  assert.equal(stored[0].breakpoint, 'desktop');
  assert.match(stored[0].target.source, /design\/screens\/home\/a\.html:\d+/);
  assert.ok(stored[0].target.a11y.role === 'button', 'a11y role captured');

  // A pin is drawn (in screen space, over every frame of that screen/variant).
  await page.keyboard.press('v');
  assert.equal(await page.getAttribute('body', 'data-mode'), 'select', 'V returns to the select tool');
  await page.waitForSelector('#pins button[data-id="fb_001"]');
  const pinCount = await page.locator('#pins button[data-id="fb_001"]').count();
  assert.equal(pinCount, 2, 'pin drawn in both breakpoint frames of variant A');

  // Pins follow the canvas: zooming moves them.
  const pinBefore = await page.locator('#pins button[data-id="fb_001"]').first().boundingBox();
  await page.keyboard.press('0');
  await page.waitForFunction(() => document.querySelector('#zoom-label').textContent === '100%');
  await page.waitForTimeout(50);
  const pinAfter = await page.locator('#pins button[data-id="fb_001"]').first().boundingBox();
  assert.notDeepEqual(pinBefore, pinAfter, 'pin moved with zoom');
  await page.keyboard.press('-');
  await page.waitForTimeout(50);
  assert.ok(parseInt(await page.textContent('#zoom-label')) < 100, 'minus zooms out');
  await page.keyboard.press('Shift+1');
  await page.waitForTimeout(100);

  // Claude marks it fixed (CLI); the studio shows it under "To verify"; the user resolves it.
  dfw('comments', 'fixed', 'fb_001', '--message', 'CTA moved next to the title and given the accent fill.');
  await page.click('.tabs button[data-filter="fixed"]');
  await page.waitForSelector('.comment[data-id="fb_001"] .status[data-s="fixed"]');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-3-to-verify.png') });
  await page.click('.comment[data-id="fb_001"] button.ok');
  await page.waitForFunction(() => !document.querySelector('.comment[data-id="fb_001"]'));
  const after = JSON.parse(dfw('comments', 'show', 'fb_001'));
  assert.equal(after.status, 'resolved');
  assert.equal(after.thread.at(-1).by, 'user');

  // Frame select by title.
  await page.click('.frame[data-key="home::b::desktop"] .frame-title');
  assert.equal(await page.getAttribute('.frame[data-key="home::b::desktop"]', 'class'), 'frame selected');
  await page.waitForSelector('#layers .row');

  // Element select -> inspector shows the design tokens in use.
  await frame.locator('[data-dfw-id="home.cta"]').click();
  await page.waitForFunction(() => /color\.accent/.test(document.querySelector('#inspector').textContent));
  const insp = await page.textContent('#inspector');
  assert.match(insp, /home\.cta/);
  assert.match(insp, /background-color\s*color\.accent/);
  assert.match(insp, /padding-block/);
  assert.equal(await page.$eval('#inspector select[data-space]', (s) => s.value), 'space.3', 'padding-block maps to space.3');
  assert.ok(await page.isVisible('#layers .row[aria-selected="true"]'), 'layer highlighted');
  await frame.locator('[data-dfw-id="home.raw"]').click();
  await page.waitForFunction(() => /raw value \(no token\)/.test(document.querySelector('#inspector').textContent));
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-5-inspect.png') });

  // Direct text edit writes back to the source; Cmd/Ctrl+Z undoes it.
  const src = path.join(tmp, 'design/screens/home/a.html');
  const waitFile = async (re, msg) => {
    for (let i = 0; i < 50 && !re.test(fs.readFileSync(src, 'utf8')); i++) await page.waitForTimeout(100);
    assert.match(fs.readFileSync(src, 'utf8'), re, msg);
  };
  await frame.locator('[data-dfw-id="home.title"]').dblclick();
  await frame.locator('[data-dfw-id="home.title"][contenteditable]').waitFor();
  await page.keyboard.type('Hello there');
  await page.keyboard.press('Enter');
  await waitFile(/<h1 data-dfw-id="home.title">Hello there<\/h1>/, 'text written to source');
  assert.equal(fs.readdirSync(path.join(tmp, '.dfw/edits')).length, 1, 'edit journaled');
  await page.waitForTimeout(500);
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+z' : 'Control+z');
  await waitFile(/<h1 data-dfw-id="home.title">Welcome back<\/h1>/, 'undo restored the source');

  // Component instance: the inspector's variant select rewrites <dfw-use>; Go to master opens the system page.
  await page.waitForTimeout(600);
  await frame.locator('[data-dfw-id="home.buy"]').click();
  await page.waitForSelector('#inspector select[data-edit="variant"]');
  assert.equal(await frame.locator('[data-dfw-id="home.buy"]').getAttribute('data-dfw-component'), 'button');
  await page.selectOption('#inspector select[data-edit="variant"]', 'secondary');
  await waitFile(/<dfw-use c="button" variant="secondary" id="home.buy">Buy now<\/dfw-use>/, 'variant written to source');
  await page.click('#inspector [data-act="master"]');
  await page.waitForSelector('iframe[name="system"]');
  assert.match(await page.textContent('#page-list [aria-current="page"]'), /Design System/);
  await page.waitForFunction(() => window.__dfwStudio.sel?.target?.dfwId === 'system.component.button', null, { timeout: 8000 });
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-6-master.png') });

  // Page switch back.
  await page.click('#page-list button[data-page="screens"]');
  await page.waitForSelector('iframe[name="home::a::desktop"]');
  undoAll();
  await page.waitForTimeout(800);

  // All pages: every page section stacked on one canvas, Design System first.
  await page.click('#page-list button[data-page="__all"]');
  await page.waitForFunction(() => document.querySelectorAll('.psec').length === 2);
  assert.deepEqual(await page.$$eval('.psec', (s) => s.map((x) => x.dataset.page)), ['system', 'screens']);
  await page.click('#page-list button[data-page="screens"]');
  assert.equal(await page.getAttribute('#page-list button[data-page="__all"]', 'aria-current'), 'page', 'single click stays in All pages');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-7-all-pages.png') });
  await page.dblclick('#page-list button[data-page="screens"]');
  await page.waitForFunction(() => document.querySelectorAll('.psec').length === 1);
  await page.waitForSelector('iframe[name="home::a::desktop"]');
  await page.keyboard.press('Shift+1');
  await page.waitForTimeout(800);

  // Interact tool: links in a frame act as prototype links on the canvas (jump to the target frame).
  await page.keyboard.press('i');
  assert.equal(await page.getAttribute('body', 'data-mode'), 'interact');
  await frame.locator('[data-dfw-id="home.link"]').click();
  await page.waitForFunction(() => window.__dfwStudio.selKey === 'detail::a::desktop');
  assert.equal(await frame.locator('[data-dfw-id="home.title"]').count(), 1, 'the source frame did not navigate away');
  await page.keyboard.press('v');
  await page.keyboard.press('Shift+1');
  await page.waitForTimeout(300);

  // Present: full-window, interactive prototype with link + data-dfw-goto navigation and comments.
  await page.click('.frame[data-key="home::a::desktop"] .frame-title');
  await page.keyboard.press('p');
  await page.waitForSelector('#present:not([hidden])');
  const pf = page.frameLocator('#p-frame');
  await pf.locator('[data-dfw-id="home.link"]').waitFor();
  assert.match(await page.textContent('#p-title'), /Home · A/);
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-8-present.png') });
  // Present screenshot: the screen being shown, saved and downloaded.
  const [download] = await Promise.all([page.waitForEvent('download', { timeout: 60000 }), page.click('#p-shoot')]);
  assert.equal(download.suggestedFilename(), 'home__a__desktop.png');
  await page.waitForFunction(() => /^1\/1 PNG saved/.test(document.querySelector('#toast').textContent));
  await pf.locator('[data-dfw-id="home.link"]').click();
  await page.waitForFunction(() => /Detail/.test(document.querySelector('#p-title').textContent));
  await pf.locator('[data-dfw-goto="home/b"]').click();
  await page.waitForFunction(() => /Home · B/.test(document.querySelector('#p-title').textContent));
  assert.equal(await page.textContent('#p-count'), '5 / 5', 'Home B is the last frame (Variants row)');
  await page.keyboard.press('ArrowRight'); // wraps to the first frame
  await page.waitForFunction(() => document.querySelector('#p-count').textContent === '1 / 5');
  await page.keyboard.press('ArrowRight');
  await page.waitForFunction(() => /Detail/.test(document.querySelector('#p-title').textContent));
  await page.keyboard.press('ArrowLeft');
  await page.waitForFunction(() => /Home · A/.test(document.querySelector('#p-title').textContent));
  await pf.locator('[data-dfw-id="home.lede"]').waitFor();
  await page.keyboard.press('c');
  await page.waitForFunction(() => document.querySelector('#p-comment').getAttribute('aria-pressed') === 'true');
  await pf.locator('[data-dfw-id="home.lede"]').click();
  await page.waitForSelector('#composer:not([hidden])');
  await page.fill('#composer-text', 'Shorter lede, please.');
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter');
  await page.waitForFunction(() => /Sent to Claude/.test(document.querySelector('#toast').textContent));
  const c2 = JSON.parse(dfw('comments', 'show', 'fb_002'));
  assert.equal(c2.target.dfwId, 'home.lede');
  assert.equal(c2.screen, 'home');
  await page.waitForSelector('#pins button[data-id="fb_002"]:not([hidden])');
  await page.keyboard.press('Escape');
  await page.keyboard.press('Escape');
  await page.waitForSelector('#present', { state: 'hidden' });

  // Live comment status: Claude acknowledges via the CLI -> chip updates over SSE.
  await page.click('.ptabs button[data-tab="comments"]');
  await page.click('.tabs button[data-filter="active"]');
  await page.waitForSelector('.comment[data-id="fb_002"] .status[data-s="open"]');
  dfw('comments', 'ack', 'fb_002', '--message', 'On it.');
  await page.waitForSelector('.comment[data-id="fb_002"] .status[data-s="acknowledged"]');
  assert.match(await page.textContent('.comment[data-id="fb_002"] .status'), /Claude is working/);
  dfw('comments', 'fixed', 'fb_002', '--message', 'Lede shortened.');
  await page.click('.tabs button[data-filter="fixed"]');
  await page.waitForSelector('.comment[data-id="fb_002"] .status[data-s="fixed"]');
  assert.match(await page.textContent('.comment[data-id="fb_002"] .status'), /please verify/);
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-9-live-status.png') });

  // Live update: the agent's edit shows up in the frame without a reload, with a cursor on what it touches.
  const homeFile = path.join(tmp, 'design/screens/home/a.html');
  await frame.locator('html').evaluate(() => (window.__dfwKept = true));
  // 1. The pre-tool-use hook (what Claude Code runs before an Edit) announces the element.
  execFileSync(process.execPath, [DFW, 'hook', 'pre-tool-use'], { cwd: tmp, env, encoding: 'utf8', input: JSON.stringify({ cwd: tmp, tool_name: 'Edit', tool_input: { file_path: homeFile, old_string: 'Welcome back', new_string: 'Good morning' } }) });
  await page.waitForFunction(() => /Claude is editing home\.title/.test(document.querySelector('#agent .box.agent:not([hidden]) .tag')?.textContent || ''), null, { timeout: 5000 });
  assert.equal(await page.locator('#agent-cursor').isHidden(), false, 'the agent cursor is shown');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-4a-agent-editing.png') });
  // 2. The edit lands: the frame is patched in place and the changed element is marked.
  fs.writeFileSync(homeFile, fs.readFileSync(homeFile, 'utf8').replace('Welcome back', 'Good morning'));
  await page.waitForFunction(() => /Claude changed/.test(document.querySelector('#agent .box.agent.changed:not([hidden]) .tag')?.textContent || ''), null, { timeout: 5000 });
  assert.equal(await frame.locator('[data-dfw-id="home.title"]').textContent(), 'Good morning', 'frame updated after edit');
  assert.equal(await frame.locator('html').evaluate(() => window.__dfwKept === true), true, 'the frame was patched, not reloaded');
  await page.waitForFunction(() => !document.querySelector('#agent .box.agent:not([hidden])'), null, { timeout: 6000 }); // the mark goes away by itself
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-4-reloaded.png') });

  // What cannot be patched in place reloads the frame: a screen whose own script builds DOM, and a changed image.
  const detail = page.frameLocator('iframe[name="detail::a::desktop"]');
  const detailFile = path.join(tmp, 'design/screens/detail/a.html');
  const kept = () => detail.locator('html').evaluate(() => window.__dfwKept === true);
  const reloaded = async (what) => {
    for (let i = 0; i < 50 && (await kept().catch(() => true)); i++) await page.waitForTimeout(100);
    assert.equal(await kept(), false, `${what}: the frame was reloaded`);
  };
  assert.equal(await detail.locator('#made li').count(), 2);
  await detail.locator('html').evaluate(() => (window.__dfwKept = true));
  fs.writeFileSync(detailFile, fs.readFileSync(detailFile, 'utf8').replace('Order 1042', 'Order 1043'));
  await reloaded('a screen with a script');
  assert.equal(await detail.locator('[data-dfw-id="detail.title"]').textContent(), 'Order 1043');
  assert.equal(await detail.locator('#made li').count(), 2, 'the list its script builds is still there');
  await detail.locator('html').evaluate(() => (window.__dfwKept = true));
  fs.writeFileSync(logoFile, logo('#0000ff'));
  await reloaded('a changed image');
  fs.writeFileSync(detailFile, fs.readFileSync(detailFile, 'utf8').replace('Order 1043', 'Order 1042'));
  for (let i = 0; i < 50 && (await detail.locator('[data-dfw-id="detail.title"]').textContent().catch(() => '')) !== 'Order 1042'; i++) await page.waitForTimeout(100);

  // Constrained layout edits: drag to reorder grid items, drag the handle to change the column span.
  const boardFile = path.join(tmp, 'design/screens/board/a.html');
  fs.mkdirSync(path.dirname(boardFile), { recursive: true });
  fs.writeFileSync(
    boardFile,
    `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8"><title>Board</title>
  <link rel="stylesheet" href="../../tokens/tokens.css">
  <style>.cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-4); padding: var(--space-8) } .cards > * { min-height: var(--space-16) }
    main .cards > div { grid-column: span 1 } .cards > .pinned { grid-column: span 1 !important }</style>
</head>
<body>
  <main data-dfw-id="board.main">
    <section class="cards" data-dfw-id="board.cards">
      <div data-dfw-id="board.one">One</div>
      <div data-dfw-id="board.two">Two</div>
      <div data-dfw-id="board.three">Three</div>
    </section>
    <section class="cards" data-dfw-id="board.more">
      <div class="pinned" data-dfw-id="board.pinned">Pinned</div>
    </section>
    <section class="cards" data-dfw-id="board.empty"></section>
  </main>
</body>
</html>`,
  );
  const m2 = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
  m2.screens.push({ id: 'board', title: 'Board', variants: [{ id: 'a', label: 'A', file: 'screens/board/a.html' }] });
  fs.writeFileSync(manifestFile, JSON.stringify(m2, null, 2));
  await page.waitForSelector('.frame[data-key="board::a::desktop"] iframe');
  await page.click('.frame[data-key="board::a::desktop"] .frame-title');
  await page.keyboard.press('Shift+2');
  await page.waitForTimeout(300);
  const board = page.frameLocator('iframe[name="board::a::desktop"]');
  const waitBoard = async (re, msg) => {
    for (let i = 0; i < 50 && !re.test(fs.readFileSync(boardFile, 'utf8')); i++) await page.waitForTimeout(100);
    assert.match(fs.readFileSync(boardFile, 'utf8'), re, msg);
  };
  const center = (b) => [b.x + b.width / 2, b.y + b.height / 2];
  await board.locator('[data-dfw-id="board.three"]').click();
  await board.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  const b3 = await board.locator('[data-dfw-id="board.three"]').boundingBox();
  const b1 = await board.locator('[data-dfw-id="board.one"]').boundingBox();
  await page.mouse.move(...center(b3));
  await page.mouse.down();
  await page.mouse.move(b1.x + b1.width * 0.2, b1.y + b1.height / 2, { steps: 10 });
  await page.mouse.up();
  await waitBoard(/<section class="cards" data-dfw-id="board.cards">\n      <div data-dfw-id="board.three">Three<\/div>\n      <div data-dfw-id="board.one">One<\/div>\n      <div data-dfw-id="board.two">Two<\/div>\n    <\/section>/, 'drag reordered the source (indentation kept)');
  await page.waitForTimeout(800); // live reload
  await board.locator('[data-dfw-id="board.two"]').click();
  await board.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  const b2 = await board.locator('[data-dfw-id="board.two"]').boundingBox();
  const hx = await board.locator('[data-dfw-handle="x"]').boundingBox();
  await page.mouse.move(...center(hx));
  await page.mouse.down();
  await page.mouse.move(b2.x + b2.width * 2.05, hx.y + hx.height / 2, { steps: 10 });
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-10-resize.png') });
  await page.mouse.up();
  await waitBoard(/<style data-dfw-layout>[\s\S]*\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 2; \}[\s\S]*<\/style>\n<\/head>/, 'resize wrote a grid-column span rule');
  // The rule wins over the more specific class rule "main .cards > div { grid-column: span 1 }".
  const span = (loc) => loc.evaluate((el) => getComputedStyle(el).gridColumnStart);
  const waitSpan = async (loc, want, msg) => {
    let got;
    for (let i = 0; i < 60; i++) {
      got = await span(loc).catch(() => null);
      if (got === want) break;
      await page.waitForTimeout(100);
    }
    assert.equal(got, want, msg);
  };
  await waitSpan(board.locator('[data-dfw-id="board.two"]'), 'span 2', 'layout rule beats the more specific screen rule');
  // Keyboard alternative: Alt+Shift+Right steps the span.
  await page.waitForTimeout(800);
  await board.locator('[data-dfw-id="board.two"]').click();
  await page.keyboard.press('Alt+Shift+ArrowRight');
  await waitBoard(/\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 3; \}/, 'Alt+Shift+Right widened the span');
  await waitSpan(board.locator('[data-dfw-id="board.two"]'), 'span 3', 'computed span follows the keyboard step');

  // Narrower breakpoint: a resize in the Mobile frame lands in @media (max-width: 390px) and only affects that width.
  await page.click('#bp-menu summary');
  await page.check('#breakpoints label:has-text("Mobile") input');
  await page.click('#bp-menu summary');
  await page.waitForSelector('.frame[data-key="board::a::mobile"]', { state: 'attached' });
  // Select a frame by its title even when it is off-screen (titles select on mousedown).
  const pickFrame = (key) => page.evaluate((k) => document.querySelector(`.frame[data-key="${k}"] .frame-title`).dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 })), key);
  await pickFrame('board::a::mobile');
  await page.keyboard.press('Shift+2');
  await page.waitForSelector('.frame[data-key="board::a::mobile"] iframe', { state: 'attached' });
  await page.waitForTimeout(800);
  const boardM = page.frameLocator('iframe[name="board::a::mobile"]');
  await boardM.locator('[data-dfw-id="board.two"]').click();
  await boardM.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  const m2b = await boardM.locator('[data-dfw-id="board.two"]').boundingBox();
  const mhx = await boardM.locator('[data-dfw-handle="x"]').boundingBox();
  await page.mouse.move(...center(mhx));
  await page.mouse.down();
  await page.mouse.move(m2b.x + m2b.width / 3, mhx.y + mhx.height / 2, { steps: 10 }); // ~1 of the 3 spanned columns
  await page.mouse.up();
  await waitBoard(/\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 3; \}\n {4}@media \(max-width: 390px\) \{\n {6}\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 1; \}\n {4}\}/, 'mobile resize went into the 390px media block; base rule kept');
  await waitSpan(boardM.locator('[data-dfw-id="board.two"]'), 'span 1', 'mobile frame (390px) uses the media rule');
  await waitSpan(board.locator('[data-dfw-id="board.two"]'), 'span 3', 'desktop frame (1440px) is unaffected');

  // Reset to auto: Alt+Shift+0 in the Mobile frame drops only the 390px rule (and the empty @media block).
  await page.waitForTimeout(800);
  await boardM.locator('[data-dfw-id="board.two"]').click();
  await page.keyboard.press('Alt+Shift+Digit0');
  for (let i = 0; i < 50 && /@media/.test(fs.readFileSync(boardFile, 'utf8')); i++) await page.waitForTimeout(100);
  assert.doesNotMatch(fs.readFileSync(boardFile, 'utf8'), /@media/, 'Alt+Shift+0 removed the media rule and its empty block');
  await waitSpan(boardM.locator('[data-dfw-id="board.two"]'), 'span 3', 'mobile falls back to the base rule');
  // Double-click the width handle in the Desktop frame: the base rule goes, and with it the whole block.
  await pickFrame('board::a::desktop');
  await page.keyboard.press('Shift+2');
  await page.waitForTimeout(600);
  await board.locator('[data-dfw-id="board.two"]').click();
  await board.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  await board.locator('[data-dfw-handle="x"]').dblclick();
  for (let i = 0; i < 50 && /data-dfw-layout/.test(fs.readFileSync(boardFile, 'utf8')); i++) await page.waitForTimeout(100);
  assert.doesNotMatch(fs.readFileSync(boardFile, 'utf8'), /data-dfw-layout/, 'double-click on the handle reset the span; empty block removed');
  await waitSpan(board.locator('[data-dfw-id="board.two"]'), 'span 1', 'back to the screen rule');

  // Drop into an empty grid container with a data-dfw-id (dashed highlight while dragging).
  await page.waitForTimeout(800);
  await board.locator('[data-dfw-id="board.one"]').click();
  const bOne = await board.locator('[data-dfw-id="board.one"]').boundingBox();
  const bEmpty = await board.locator('[data-dfw-id="board.empty"]').boundingBox();
  await page.mouse.move(...center(bOne));
  await page.mouse.down();
  await page.mouse.move(...center(bEmpty), { steps: 10 });
  assert.equal(await board.locator('[data-dfw-ui]').evaluateAll((els) => els.some((e) => e.style.borderStyle === 'dashed' && e.style.display === 'block')), true, 'empty container highlighted');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-11-drop-inside.png') });
  await page.mouse.up();
  await waitBoard(/<section class="cards" data-dfw-id="board.empty">\n      <div data-dfw-id="board.one">One<\/div>\n    <\/section>/, 'dropped into the empty container (source moved and indented)');

  // A rule that still wins (!important) is named instead of the resize silently doing nothing; the inspector resets it.
  await page.waitForTimeout(800);
  await board.locator('[data-dfw-id="board.pinned"]').click();
  await page.keyboard.press('Alt+Shift+ArrowRight');
  await waitBoard(/\[data-dfw-id='board.pinned'\]:not\(#dfw\) \{ grid-column: span 2; \}/, 'pinned resize written');
  await page.waitForFunction(() => /did not change/.test(document.querySelector('#toast').textContent), null, { timeout: 8000 });
  assert.match(await page.textContent('#toast'), /"\.cards > \.pinned" still sets grid-column/, 'toast names the conflicting selector');
  assert.equal(await page.evaluate(() => window.__dfwStudio.layoutConflict?.selector), '.cards > .pinned');
  await page.waitForTimeout(300);
  await board.locator('[data-dfw-id="board.pinned"]').click();
  await page.click('#inspector [data-act="layout-reset"]');
  for (let i = 0; i < 50 && /data-dfw-layout/.test(fs.readFileSync(boardFile, 'utf8')); i++) await page.waitForTimeout(100);
  assert.doesNotMatch(fs.readFileSync(boardFile, 'utf8'), /data-dfw-layout/, 'inspector "Reset size to auto" removed the rule');
  const lint = JSON.parse(spawnSync(process.execPath, [DFW, 'lint', 'design/screens/board/a.html', '--json'], { cwd: tmp, env, encoding: 'utf8' }).stdout);
  assert.equal(lint.tally.error, 0, `lint clean after layout edits: ${JSON.stringify(lint.results).slice(0, 400)}`);

  assert.deepEqual(errors, [], `no page errors: ${errors.join('; ')}`);
  console.log('PASS studio e2e');
} catch (e) {
  failed = true;
  console.error('FAIL studio e2e:', e.message, (e.stack.match(/studio\.e2e\.mjs:\d+/) || [''])[0]);
  if (shotsDir && page) await page.screenshot({ path: path.join(shotsDir, 'studio-failure.png'), fullPage: true }).catch(() => {});
} finally {
  await browser.close();
  dfw('studio', 'stop');
  if (!args.includes('--keep')) fs.rmSync(tmp, { recursive: true, force: true });
  else console.log(`kept ${tmp}`);
}
process.exit(failed ? 1 : 0);
