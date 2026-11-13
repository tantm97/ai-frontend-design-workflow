// End-to-end test of the dfw Studio click-to-comment flow (real browser via Playwright).
// Usage: node tests/e2e/studio.e2e.mjs [--keep] [--shots <dir>]
// Needs Playwright installed for dfw (dfw setup --playwright, or DFW_RUNTIME_DIR).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { loadPlaywright, playwrightStatus } from '../../core/lib/runtime.mjs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const DFW = path.join(REPO, 'core', 'bin', 'dfw');
const args = process.argv.slice(2);
const shotsDir = args.includes('--shots') ? path.resolve(args[args.indexOf('--shots') + 1]) : null;

if (!playwrightStatus().installed) {
  console.log('SKIP: Playwright not installed for dfw (dfw setup --playwright).');
  process.exit(0);
}

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
<body><main data-dfw-id="detail.main"><h1 data-dfw-id="detail.title">Order 1042</h1><button type="button" data-dfw-goto="home/b">Back home</button></main></body></html>`,
);
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
fs.writeFileSync(manifestFile, JSON.stringify(m, null, 2));

const port = 4700 + Math.floor(Math.random() * 200);
const started = dfw('studio', 'start', '--port', String(port));
const url = started.match(/http:\/\/\S+/)[0];
const { chromium } = await loadPlaywright();
const browser = await chromium.launch();
let failed = false;
try {
  const page = await browser.newPage({ viewport: { width: 1600, height: 1000 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  await page.goto(url);
  await page.waitForSelector('.frame iframe');
  const frames = await page.$$('.frame iframe');
  // Auto sections: one row per breakpoint (primary variants) + a Variants row.
  assert.equal(frames.length, 5, '2 screens x 2 breakpoints + 1 variant');
  assert.deepEqual(await page.$$eval('.prow-gutter h2', (h) => h.map((x) => x.textContent)), ['Desktop 1440', 'Tablet 834', 'Variants']);
  assert.equal(await page.textContent('.frame[data-key="home::a::desktop"] .frame-title'), 'D · Home · A↗');
  const pages = await page.$$eval('#page-list button', (bs) => bs.map((b) => b.textContent));
  assert.match(pages[0], /All pages/, 'All pages entry first');
  assert.match(pages[1], /Design System/, 'Design System is the first page');
  assert.match(pages[2], /Screens\s*5/, 'Screens page with frame count');
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

  // Live reload: editing the design file reloads frames.
  fs.writeFileSync(path.join(tmp, 'design/screens/home/a.html'), fs.readFileSync(path.join(tmp, 'design/screens/home/a.html'), 'utf8').replace('Welcome back', 'Good morning'));
  await page.waitForTimeout(1200);
  assert.equal(await frame.locator('[data-dfw-id="home.title"]').textContent(), 'Good morning', 'frame reloaded after edit');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-4-reloaded.png') });

  assert.deepEqual(errors, [], `no page errors: ${errors.join('; ')}`);
  console.log('PASS studio e2e');
} catch (e) {
  failed = true;
  console.error('FAIL studio e2e:', e.message.split('\n')[0], (e.stack.match(/studio\.e2e\.mjs:\d+/) || [''])[0]);
} finally {
  await browser.close();
  dfw('studio', 'stop');
  if (!args.includes('--keep')) fs.rmSync(tmp, { recursive: true, force: true });
  else console.log(`kept ${tmp}`);
}
process.exit(failed ? 1 : 0);
