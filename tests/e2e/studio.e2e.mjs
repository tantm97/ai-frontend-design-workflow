// End-to-end test of the dfw Studio click-to-comment flow (real browser via Playwright).
// Usage: node tests/e2e/studio.e2e.mjs [--keep] [--shots <dir>]
// Dev-only: needs the "playwright" package (its click/keyboard API drives the Studio). It is
// resolved from this repo's node_modules or from $DFW_RUNTIME_DIR (a folder with playwright installed).
// The browser is Playwright's own Chromium, or $DFW_BROWSER / $CHROME_PATH (e.g. the CI runner's
// Google Chrome, so no browser download is needed). With $CI set, a missing playwright fails.
// dfw is the binary in $DFW_BIN, else native/target/debug/dfw (cd native && cargo build); without
// either the test fails. dfw itself does not use Playwright (it drives the browser over CDP).
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import assert from 'node:assert/strict';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createRequire } from 'node:module';
import { setTimeout as delay } from 'node:timers/promises';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..');
const DFW = process.env.DFW_BIN
  ? path.resolve(REPO, process.env.DFW_BIN)
  : path.join(REPO, 'native', 'target', 'debug', process.platform === 'win32' ? 'dfw.exe' : 'dfw');
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
if (!fs.existsSync(DFW)) {
  console.error(`FAIL studio e2e: no dfw binary at ${DFW}. Build it (cd native && cargo build) or set DFW_BIN.`);
  process.exit(1);
}
if (shotsDir) fs.mkdirSync(shotsDir, { recursive: true });

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-e2e-'));
const env = { ...process.env, DFW_HOME: path.join(tmp, '.dfwhome') };
const dfw = (...a) => execFileSync(DFW, a, { cwd: tmp, env, encoding: 'utf8' });
const studio = (...a) => dfw('studio', ...a);
/** The background waits started by the test, stopped at the end whatever happened. */
const waits = [];
/** A background `dfw comments wait`, as the agent runs it; `exited` gives its stdout. */
function startWait() {
  const child = spawn(DFW, ['comments', 'wait', '--timeout', '60'], { cwd: tmp, env, stdio: ['ignore', 'pipe', 'inherit'] });
  waits.push(child);
  let out = '';
  child.stdout.on('data', (d) => (out += d));
  const exited = new Promise((resolve) => child.on('exit', () => resolve(out)));
  return { child, exited };
}

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
<dfw-use c="button" variant="primary" id="home.buy" hint="Ships &quot;today&quot; &amp; free">Buy now</dfw-use>
<a data-dfw-id="home.link" href="../detail/a.html">Order details</a>
<p class="when-empty" data-dfw-id="home.empty">No orders yet.</p><p class="when-loading" data-dfw-id="home.loading">Loading orders</p><p class="when-error" data-dfw-id="home.error">Orders could not load.</p></main></body></html>`,
);
fs.mkdirSync(path.join(tmp, 'design/screens/detail'), { recursive: true });
fs.writeFileSync(
  path.join(tmp, 'design/screens/detail/a.html'),
  `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Detail</title><link rel="stylesheet" href="../../tokens/tokens.css"></head>
<body><main data-dfw-id="detail.main"><h1 data-dfw-id="detail.title">Order 1042</h1><button type="button" data-dfw-goto="home/b">Back home</button><ul id="made"></ul><img id="logo" alt="" src="../../assets/logo.svg" width="8" height="8"></main>
<p class="when-empty">No lines.</p><p class="when-loading">Loading lines</p><p class="when-error">Lines could not load.</p>
<script>for (const n of ['Bolts', 'Nuts']) document.getElementById('made').append(Object.assign(document.createElement('li'), { textContent: n }));</script></body></html>`,
);
const logoFile = path.join(tmp, 'design/assets/logo.svg');
const logo = (fill) => `<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8"><rect width="8" height="8" fill="${fill}"/></svg>\n`;
fs.mkdirSync(path.dirname(logoFile), { recursive: true });
fs.writeFileSync(logoFile, logo('#ff0000'));
fs.mkdirSync(path.join(tmp, 'design/system/components'), { recursive: true });
fs.writeFileSync(
  path.join(tmp, 'design/system/components/button.html'),
  `<dfw-component name="button" variants="primary secondary" sizes="md lg" props="hint, tone=calm" description="Action button">
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
m.screens = [
  {
    id: 'home',
    title: 'Home',
    variants: [
      { id: 'a', label: 'A', file: 'screens/home/a.html' },
      { id: 'b', label: 'B', file: 'screens/home/a.html' },
    ],
  },
  { id: 'detail', title: 'Detail', variants: [{ id: 'a', label: 'A', file: 'screens/detail/a.html' }] },
];
m.states = ['empty', 'loading', 'error'];
fs.writeFileSync(manifestFile, JSON.stringify(m, null, 2));

/**
 * Text set in a monospace font with a weight above 400, in `target`'s own document (not inside its
 * frames). Mono fonts often ship one weight, so bold renders as faux bold or not at all
 * (studio-ui.md U2, README §0 row 3). Read from computed styles: the `font` shorthand and
 * inheritance set weights a CSS scan cannot see. `monoVar` is the custom property of the
 * document's own mono font.
 */
const boldMono = (target, monoVar) =>
  target.evaluate((monoVar) => {
    const first = (f) =>
      f
        .split(',')[0]
        .trim()
        .replace(/^["']|["']$/g, '')
        .toLowerCase();
    const monos = new Set(['ui-monospace', 'sfmono-regular', 'menlo', 'consolas', 'monospace']);
    const own = getComputedStyle(document.documentElement).getPropertyValue(monoVar).trim();
    if (own) monos.add(first(own));
    const found = [];
    for (const el of document.querySelectorAll('body *')) {
      if (![...el.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent.trim())) continue;
      const cs = getComputedStyle(el);
      if (monos.has(first(cs.fontFamily)) && Number(cs.fontWeight) > 400) {
        found.push(
          `${el.tagName.toLowerCase()}.${el.className} "${el.textContent.trim().slice(0, 24)}": ${cs.fontFamily} ${cs.fontWeight}`,
        );
      }
    }
    return found;
  }, monoVar);

const port = 4700 + Math.floor(Math.random() * 200);
const started = studio('start', '--port', String(port));
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
  assert.deepEqual(await boldMono(page, '--mono'), [], "no bold text in the Studio's mono font");
  // Steps wait on conditions, not sleeps (testing.md T12). A changed design file reaches the frames
  // through the server's 'design' event (after the watcher's debounce); a frame that took the change
  // (patched in place or reloaded) posts dfw:ready again. Both are counted here.
  await page.evaluate(() => {
    window.__e2e = { design: 0, ready: {} };
    new EventSource('/__dfw/events').addEventListener('design', () => window.__e2e.design++);
    addEventListener('message', (e) => {
      const name = e.source && e.source.name;
      if (e.data && e.data.type === 'dfw:ready' && name) window.__e2e.ready[name] = (window.__e2e.ready[name] || 0) + 1;
    });
  });
  /** The counters now, to wait for what comes after. */
  const mark = () => page.evaluate(() => JSON.parse(JSON.stringify(window.__e2e)));
  /** A design event came after `since` and the frame `key` posted dfw:ready after it. */
  const settled = (key, since) =>
    page.waitForFunction(([k, s]) => window.__e2e.design > s.design && (window.__e2e.ready[k] || 0) > (s.ready[k] || 0), [key, since], {
      timeout: 10000,
    });
  /** The camera is applied on the next animation frame. */
  const painted = () => page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
  /** The Studio has the frame's overlay ready (tool, tokens and layout settings sent). */
  const frameReady = (key) => page.waitForFunction((k) => window.__dfwStudio.frames.get(k)?.loaded === true, key);
  /** The send bar shows the listener state `state` (the Studio re-checks every 3 s). */
  const listenerIs = (state) => page.waitForFunction((s) => document.querySelector('#listen').dataset.on === s, state, { timeout: 10000 });
  /**
   * Wait until a file on disk passes `ok`. A file has no page event to wait on, so it is read again
   * every 50 ms until the deadline (testing.md T12: a condition with a deadline, not a fixed sleep).
   */
  const fileUntil = async (file, ok, timeout = 5000) => {
    for (const end = Date.now() + timeout; !ok(fs.readFileSync(file, 'utf8')) && Date.now() < end;) await delay(50);
  };
  /** Wait until an element of a frame has a computed grid-column-start (the frame may reload meanwhile). */
  const waitSpan = async (name, id, want, msg) => {
    const fr = page.frame({ name });
    const get = ([i]) => {
      const el = document.querySelector(`[data-dfw-id="${i}"]`);
      return el ? getComputedStyle(el).gridColumnStart : null;
    };
    await fr
      .waitForFunction(
        ([i, w]) => {
          const el = document.querySelector(`[data-dfw-id="${i}"]`);
          return !!el && getComputedStyle(el).gridColumnStart === w;
        },
        [id, want],
        { timeout: 6000 },
      )
      .catch(() => {});
    assert.equal(await fr.evaluate(get, [id]), want, msg);
  };
  const count = () => page.$$eval('.page-canvas:not([hidden]) .frame[data-key]', (f) => f.length); // frames of the page shown (kept pages are hidden), not iframes (off-screen iframes load lazily)
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
  await page.waitForSelector('.page-canvas:not([hidden]) svg.flow path[marker-end]', { state: 'attached' });
  assert.equal(await page.$$eval('.page-canvas:not([hidden]) svg.flow path[marker-end]', (p) => p.length), 2);
  assert.deepEqual((await page.$$eval('.page-canvas:not([hidden]) svg.flow text', (t) => t.map((x) => x.textContent))).sort(), [
    'Back home',
    'Order details',
  ]);
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
  assert.deepEqual(await page.$$eval('.page-canvas:not([hidden]) .prow-gutter h2', (h) => h.map((x) => x.textContent)), [
    'Desktop 1440',
    'Tablet 834',
    'Variants',
  ]);
  assert.equal(await page.textContent('.page-canvas:not([hidden]) .frame[data-key="home::a::desktop"] .frame-title'), 'D · Home · A↗');
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
  assert.deepEqual(shotFiles, [
    'design-system.png',
    'detail__a__desktop.png',
    'detail__a__tablet.png',
    'home__a__desktop.png',
    'home__a__tablet.png',
    'home__b__desktop.png',
  ]);
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
  const bad = await page.evaluate(() =>
    fetch('/__dfw/api/shoot', { method: 'POST', body: JSON.stringify({ frames: [{ src: 'file:///etc/passwd' }] }) }).then((r) => r.status),
  );
  assert.equal(bad, 400, 'only Studio design URLs can be screenshotted');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-1-canvas.png') });

  // Enter comment mode and click the CTA inside the first (desktop, variant A) frame.
  await page.keyboard.press('c');
  const frame = page.frameLocator('.page-canvas:not([hidden]) iframe[name="home::a::desktop"]');
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

  // A Studio comment is a draft: the agent gets nothing until the user presses "Send to AI".
  assert.match(await page.textContent('.comment[data-id="fb_001"] .status'), /Draft/);
  assert.match(dfw('comments', 'inbox', '--peek'), /No new comments/, 'a draft is not delivered');
  assert.match(await page.textContent('#listen'), /not listening/, 'no dfw comments wait runs');
  assert.doesNotMatch(await page.textContent('#listen'), /terminal/, 'the Studio never sends the user to the terminal');
  assert.equal(await page.isVisible('#listen-copy'), true, 'not listening: a request to copy for the agent');
  if (shotsDir) await page.locator('#tab-comments').screenshot({ path: path.join(shotsDir, 'studio-2a-not-listening.png') });
  assert.equal(await page.textContent('#send-drafts'), 'Send to AI (1)');
  const sendBg = () => page.$eval('#send-drafts', (b) => getComputedStyle(b).backgroundColor);
  const enabledBg = await sendBg();
  await page.click('#send-drafts');
  await page.waitForFunction(() => /^Saved and queued\./.test(document.querySelector('#toast').textContent));
  await page.waitForSelector('.comment[data-id="fb_001"] .status[data-s="open"]');
  assert.equal(await page.textContent('.comment[data-id="fb_001"] .status'), 'Sent · waiting for the AI');
  assert.match(dfw('comments', 'inbox', '--peek'), /fb_001/, 'sent: the inbox delivers it');
  assert.equal(await page.isDisabled('#send-drafts'), true, 'nothing left to send');
  assert.notEqual(await sendBg(), enabledBg, 'a disabled Send looks disabled');
  // The agent starts listening: the queued comment wakes the wait at once, then the agent works on it.
  const first = startWait();
  const woke = await first.exited;
  assert.match(woke, /fb_001/, 'the wait prints the queued comment');
  assert.match(woke, /start "dfw comments wait --timeout 3600" again in the background/, 'and says to re-arm');
  await page.waitForFunction(() => document.querySelector('.comment[data-id="fb_001"] .status')?.textContent === 'Received by the AI');
  await listenerIs('working');
  assert.match(await page.textContent('#listen'), /The AI is working on your comments/);
  if (shotsDir) await page.locator('#tab-comments').screenshot({ path: path.join(shotsDir, 'studio-2b-working.png') });
  // A wait that is killed sends no event: the Studio's re-check shows it within seconds.
  const second = startWait();
  await listenerIs('listening');
  assert.equal(await page.isVisible('#listen-copy'), false, 'listening: nothing to copy');
  if (shotsDir) await page.locator('#tab-comments').screenshot({ path: path.join(shotsDir, 'studio-2c-listening.png') });
  second.child.kill('SIGKILL');
  await second.exited;
  await page.waitForFunction(() => document.querySelector('#listen').dataset.on !== 'listening', null, { timeout: 10000 });
  // The user answers the agent in the Studio; the reply reaches the inbox as a follow-up.
  await page.click('.comment[data-id="fb_001"] .comment-actions button:has-text("Reply")');
  await page.fill('.comment[data-id="fb_001"] .comment-actions textarea', 'Use the darker blue.');
  await page.click('.comment[data-id="fb_001"] .comment-actions button.ok');
  await page.waitForFunction(() =>
    /Use the darker blue\./.test(document.querySelector('.comment[data-id="fb_001"] .thread')?.textContent || ''),
  );
  assert.match(dfw('comments', 'inbox'), /follow-up by user: "Use the darker blue\."/, 'the reply reaches the agent');

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
  await page.waitForFunction((b) => {
    const r = document.querySelector('#pins button[data-id="fb_001"]').getBoundingClientRect();
    return r.x !== b.x || r.y !== b.y;
  }, pinBefore);
  const pinAfter = await page.locator('#pins button[data-id="fb_001"]').first().boundingBox();
  assert.notDeepEqual(pinBefore, pinAfter, 'pin moved with zoom');
  await page.keyboard.press('-');
  await page.waitForFunction(() => parseInt(document.querySelector('#zoom-label').textContent) < 100);
  assert.ok(parseInt(await page.textContent('#zoom-label')) < 100, 'minus zooms out');
  await page.keyboard.press('Shift+1');
  await painted();

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
  await page.click('.page-canvas:not([hidden]) .frame[data-key="home::b::desktop"] .frame-title');
  assert.equal(await page.getAttribute('.page-canvas:not([hidden]) .frame[data-key="home::b::desktop"]', 'class'), 'frame selected');
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
  assert.deepEqual(await boldMono(page, '--mono'), [], 'no bold text in the mono font of the inspector');
  await frame.locator('[data-dfw-id="home.raw"]').click();
  // Wait for the selection itself: the inspector of home.cta already holds "raw value (no
  // token)", so waiting for that text went on while home.cta was still selected, and the
  // keyboard steps below then started from the wrong row (an intermittent timeout).
  await page.waitForFunction(() => window.__dfwStudio.sel?.own?.id === 'home.raw');
  await page.waitForFunction(() => /raw value \(no token\)/.test(document.querySelector('#inspector').textContent));
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-5-inspect.png') });
  // Layers from the keyboard (studio-ui.md U7): one row is in the tab order; arrows move, Enter selects.
  assert.equal(await page.locator('#layers .row[tabindex="0"]').count(), 1, 'one roving tab stop');
  assert.equal(await page.getAttribute('#layers .row[tabindex="0"]', 'aria-selected'), 'true', 'the tab stop is the selected layer');
  await page.focus('#layers .row[tabindex="0"]');
  await page.keyboard.press('ArrowUp');
  await page.waitForFunction(() => document.activeElement?.matches('#layers .row[tabindex="0"]:not([aria-selected="true"])'));
  await page.keyboard.press('Enter');
  await page.waitForFunction(() => window.__dfwStudio.sel?.own?.id === 'home.cta');
  await page.waitForFunction(() => document.activeElement?.matches('#layers .row[aria-selected="true"][tabindex="0"]'));
  assert.equal(await page.evaluate(() => document.body.classList.contains('space')), false);
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-5a-layers-keyboard.png') });
  const vp = await page.locator('#viewport').boundingBox();
  // The canvas viewport never scrolls: the camera moves the canvas. A frame's scrollIntoView (what
  // overlay/select.js does for a keyboard or layer selection) reaches every ancestor that can scroll,
  // the Studio page included; with the element below the view, a scrollable #viewport moved the
  // whole canvas under a HUD placed from the camera, and every selection box was off by that much.
  await page.mouse.move(vp.x + 8, vp.y + 8);
  for (let i = 0; i < 3; i++) await page.mouse.wheel(0, -2000);
  await painted();
  await frame.locator('[data-dfw-id="home.cta"]').evaluate((el) => el.scrollIntoView({ block: 'nearest' }));
  await painted();
  assert.deepEqual(await page.$eval('#viewport', (v) => [v.scrollLeft, v.scrollTop]), [0, 0], 'the canvas viewport does not scroll');
  // Picking a layer brings its element into view (same zoom) when the camera is elsewhere.
  await page.mouse.move(vp.x + 8, vp.y + vp.height - 8);
  for (let i = 0; i < 6; i++) await page.mouse.wheel(0, 2000);
  await page.waitForTimeout(250);
  const zBefore = await page.evaluate(() => window.__dfwStudio.cam.z);
  const elInView = () =>
    page.evaluate(() => {
      const S = window.__dfwStudio;
      const f = S.frames.get(S.sel.key);
      const [x, y, w, h] = S.sel.rect;
      const r = document.querySelector('#viewport').getBoundingClientRect();
      const sx = (f.x + x) * S.cam.z + S.cam.x;
      const sy = (f.y + y) * S.cam.z + S.cam.y;
      return sx + w * S.cam.z > 0 && sx < r.width && sy + h * S.cam.z > 0 && sy < r.height;
    });
  assert.equal(await elInView(), false, 'the selection is out of view after panning away');
  await page.click('#layers .row[aria-selected="true"] .nm');
  await page.waitForFunction(
    () => {
      const S = window.__dfwStudio;
      const f = S.frames.get(S.sel?.key);
      if (!f || !S.sel.rect) return false;
      const sy = (f.y + S.sel.rect[1]) * S.cam.z + S.cam.y;
      return sy > 0 && sy < document.querySelector('#viewport').clientHeight;
    },
    null,
    { timeout: 5000 },
  );
  assert.equal(await elInView(), true, 'the layer pick revealed the element');
  assert.equal(await page.evaluate(() => window.__dfwStudio.cam.z), zBefore, 'the zoom did not change');
  // The selection box sits on the element. The frame's scrollIntoView reaches the Studio page too;
  // a scrolled #viewport moved the whole canvas under a box placed from the camera alone.
  await painted();
  assert.deepEqual(
    await page.$eval('#viewport', (v) => [v.scrollLeft, v.scrollTop]),
    [0, 0],
    'the canvas viewport never scrolls (the camera moves it)',
  );
  const elBox = await frame.locator('[data-dfw-id="home.cta"]').boundingBox();
  const selBox = await page.locator('#sel-box').boundingBox();
  for (const [k, a, b] of [
    ['x', selBox.x + 1, elBox.x],
    ['y', selBox.y + 1, elBox.y],
    ['width', selBox.width - 2, elBox.width],
    ['height', selBox.height - 2, elBox.height],
  ])
    assert.ok(Math.abs(a - b) < 1.5, `selection box ${k} ${a} matches the element's ${b}`);

  // Direct text edit writes back to the source; Cmd/Ctrl+Z undoes it.
  const src = path.join(tmp, 'design/screens/home/a.html');
  const waitFile = async (re, msg) => {
    await fileUntil(src, (t) => re.test(t));
    assert.match(fs.readFileSync(src, 'utf8'), re, msg);
  };
  await frame.locator('[data-dfw-id="home.title"]').dblclick();
  await frame.locator('[data-dfw-id="home.title"][contenteditable]').waitFor();
  await page.keyboard.type('Hello there');
  const beforeEdit = await mark();
  await page.keyboard.press('Enter');
  await waitFile(/<h1 data-dfw-id="home.title">Hello there<\/h1>/, 'text written to source');
  assert.equal(fs.readdirSync(path.join(tmp, '.dfw/edits')).length, 1, 'edit journaled');
  await settled('home::a::desktop', beforeEdit);
  const beforeUndo = await mark();
  await page.keyboard.press(process.platform === 'darwin' ? 'Meta+z' : 'Control+z');
  await waitFile(/<h1 data-dfw-id="home.title">Welcome back<\/h1>/, 'undo restored the source');

  // Component instance: the inspector's variant select rewrites <dfw-use>; Go to master opens the system page.
  await settled('home::a::desktop', beforeUndo);
  await frame.locator('[data-dfw-id="home.buy"]').click();
  await page.waitForSelector('#inspector select[data-edit="variant"]');
  assert.equal(await frame.locator('[data-dfw-id="home.buy"]').getAttribute('data-dfw-component'), 'button');
  // Each prop input shows the value written on the <dfw-use> (not the master's default).
  assert.equal(await page.inputValue('#inspector input[data-prop="hint"]'), 'Ships "today" & free', 'prop value shown');
  assert.equal(await page.inputValue('#inspector input[data-prop="tone"]'), '', 'unset prop left empty');
  await page.selectOption('#inspector select[data-edit="variant"]', 'secondary');
  await waitFile(
    /<dfw-use c="button" variant="secondary" id="home.buy" hint="Ships &quot;today&quot; &amp; free">Buy now<\/dfw-use>/,
    'variant written to source',
  );
  await page.click('#inspector [data-act="master"]');
  await page.waitForSelector('.page-canvas:not([hidden]) iframe[name="system"]');
  assert.match(await page.textContent('#page-list [aria-current="page"]'), /Design System/);
  await page.waitForFunction(() => window.__dfwStudio.sel?.target?.dfwId === 'system.component.button', null, { timeout: 8000 });
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-6-master.png') });

  // Page switch back.
  await page.click('#page-list button[data-page="screens"]');
  await page.waitForSelector('.page-canvas:not([hidden]) iframe[name="home::a::desktop"]');
  const beforeUndoAll = await mark();
  undoAll();
  await settled('home::a::desktop', beforeUndoAll);

  // All pages: every page section stacked on one canvas, Design System first.
  await page.click('#page-list button[data-page="__all"]');
  await page.waitForFunction(() => document.querySelectorAll('.page-canvas:not([hidden]) .psec').length === 2);
  assert.deepEqual(await page.$$eval('.page-canvas:not([hidden]) .psec', (s) => s.map((x) => x.dataset.page)), ['system', 'screens']);
  await page.click('#page-list button[data-page="screens"]');
  assert.equal(await page.getAttribute('#page-list button[data-page="__all"]', 'aria-current'), 'page', 'single click stays in All pages');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-7-all-pages.png') });
  await page.dblclick('#page-list button[data-page="screens"]');
  await page.waitForFunction(() => document.querySelectorAll('.page-canvas:not([hidden]) .psec').length === 1);
  await page.waitForSelector('.page-canvas:not([hidden]) iframe[name="home::a::desktop"]');
  await page.keyboard.press('Shift+1');
  await frameReady('home::a::desktop');

  // Interact tool: links in a frame act as prototype links on the canvas (jump to the target frame).
  await page.keyboard.press('i');
  assert.equal(await page.getAttribute('body', 'data-mode'), 'interact');
  await frame.locator('[data-dfw-id="home.link"]').click();
  await page.waitForFunction(() => window.__dfwStudio.selKey === 'detail::a::desktop');
  assert.equal(await frame.locator('[data-dfw-id="home.title"]').count(), 1, 'the source frame did not navigate away');
  await page.keyboard.press('v');
  await page.keyboard.press('Shift+1');
  await painted();

  // Present: full-window, interactive prototype with link + data-dfw-goto navigation and comments.
  await page.click('.page-canvas:not([hidden]) .frame[data-key="home::a::desktop"] .frame-title');
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
  await page.waitForFunction(() => /Draft #002 saved/.test(document.querySelector('#toast').textContent));
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
  // Sent while the agent listens: the wait wakes with the comment at once.
  const third = startWait();
  await listenerIs('listening');
  await page.click('#send-drafts');
  await page.waitForFunction(() => /^Sent 1 comment to the AI\./.test(document.querySelector('#toast').textContent));
  assert.match(await third.exited, /fb_002/, 'Send to AI wakes the listening wait');
  await page.waitForSelector('.comment[data-id="fb_002"] .status[data-s="open"]');
  dfw('comments', 'ack', 'fb_002', '--message', 'On it.');
  await page.waitForSelector('.comment[data-id="fb_002"] .status[data-s="acknowledged"]');
  assert.match(await page.textContent('.comment[data-id="fb_002"] .status'), /AI is working/);
  dfw('comments', 'fixed', 'fb_002', '--message', 'Lede shortened.');
  await page.click('.tabs button[data-filter="fixed"]');
  await page.waitForSelector('.comment[data-id="fb_002"] .status[data-s="fixed"]');
  assert.match(await page.textContent('.comment[data-id="fb_002"] .status'), /please verify/);
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-9-live-status.png') });
  // A comment opens from the keyboard: Shift+Tab from its first action reaches its "#n" button; Space presses it, not the hand tool.
  await page.focus('.comment[data-id="fb_002"] .comment-actions button');
  await page.keyboard.press('Shift+Tab');
  await page.waitForFunction(() => document.activeElement?.matches('.comment[data-id="fb_002"] button.open'));
  await page.keyboard.press('Space');
  await page.waitForSelector('.comment.selected[data-id="fb_002"]');
  await page.waitForFunction(() => window.__dfwStudio.selKey?.startsWith('home::a::'));
  assert.equal(await page.evaluate(() => document.body.classList.contains('space')), false, 'Space did not start panning');
  // The selection shows the Inspect panel: the focus moves to its tab instead of falling to the page.
  await page.waitForFunction(() => document.activeElement?.matches('.ptabs button[data-tab="inspect"]'));
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-9a-comment-keyboard.png') });
  // Opening the comment went to the first page with that screen (Flow): back to Screens.
  await page.click('#page-list button[data-page="screens"]');
  await page.waitForSelector('.page-canvas:not([hidden]) iframe[name="home::a::desktop"]');
  // Going to another page and back keeps the loaded frames (no blank canvas, no reload).
  await frameReady('home::a::desktop');
  await page.evaluate(() => (document.querySelector('.page-canvas:not([hidden]) iframe[name="home::a::desktop"]').dataset.kept = 'yes'));
  await page.click('#page-list button[data-page="flow"]');
  await page.click('#page-list button[data-page="screens"]');
  assert.equal(
    await page.evaluate(() => document.querySelector('.page-canvas:not([hidden]) iframe[name="home::a::desktop"]')?.dataset.kept),
    'yes',
    'the frame kept its document across the page switch',
  );
  assert.equal(await page.evaluate(() => window.__dfwStudio.frames.get('home::a::desktop').loaded), true);
  // The live budget: with more frames than the budget (lowered to 3 here), at most that many
  // documents stay alive over every kept page, the hidden pages' first; the others show their
  // placeholder and load again when they are needed.
  const liveDocs = () => page.evaluate(() => document.querySelectorAll('.world iframe').length);
  const budget = await page.evaluate(() => window.__dfwStudio.liveBudget);
  assert.ok((await liveDocs()) > 3, 'the small fixture keeps every frame under the default budget');
  await page.evaluate(() => (window.__dfwStudio.liveBudget = 3));
  await page.click('#page-list button[data-page="__all"]');
  await page.keyboard.press('Shift+1');
  await page.waitForFunction(() => document.querySelectorAll('.world iframe').length <= 3, null, { timeout: 8000 });
  await page.waitForFunction(() => [...window.__dfwStudio.frames.values()].filter((f) => f.loaded).length === 3, null, { timeout: 8000 });
  assert.ok((await page.$$('.page-canvas:not([hidden]) .frame .placeholder')).length > 0, 'frames over the budget show their placeholder');
  assert.ok((await liveDocs()) <= 3, 'live documents stay within the budget');
  await page.evaluate((n) => (window.__dfwStudio.liveBudget = n), budget);
  await page.dblclick('#page-list button[data-page="screens"]');
  await page.keyboard.press('Shift+1');
  await frameReady('home::a::desktop');

  // Live update: the agent's edit shows up in the frame without a reload, with a cursor on what it touches.
  const homeFile = path.join(tmp, 'design/screens/home/a.html');
  await frame.locator('html').evaluate(() => (window.__dfwKept = true));
  // 1. The pre-tool-use hook (what Claude Code runs before an Edit) announces the element.
  execFileSync(DFW, ['hook', 'pre-tool-use'], {
    cwd: tmp,
    env,
    encoding: 'utf8',
    input: JSON.stringify({
      cwd: tmp,
      tool_name: 'Edit',
      tool_input: { file_path: homeFile, old_string: 'Welcome back', new_string: 'Good morning' },
    }),
  });
  await page.waitForFunction(
    () => /AI is editing home\.title/.test(document.querySelector('#agent .box.agent:not([hidden]) .tag')?.textContent || ''),
    null,
    { timeout: 5000 },
  );
  assert.equal(await page.locator('#agent-cursor').isHidden(), false, 'the agent cursor is shown');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-4a-agent-editing.png') });
  // 2. The edit lands: the frame is patched in place and the changed element is marked.
  fs.writeFileSync(homeFile, fs.readFileSync(homeFile, 'utf8').replace('Welcome back', 'Good morning'));
  await page.waitForFunction(
    () => /AI changed/.test(document.querySelector('#agent .box.agent.changed:not([hidden]) .tag')?.textContent || ''),
    null,
    { timeout: 5000 },
  );
  assert.equal(await frame.locator('[data-dfw-id="home.title"]').textContent(), 'Good morning', 'frame updated after edit');
  assert.equal(await frame.locator('html').evaluate(() => window.__dfwKept === true), true, 'the frame was patched, not reloaded');
  await page.waitForFunction(() => !document.querySelector('#agent .box.agent:not([hidden])'), null, { timeout: 6000 }); // the mark goes away by itself
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-4-reloaded.png') });

  // What cannot be patched in place reloads the frame: a screen whose own script builds DOM, and a changed image.
  const detail = page.frameLocator('.page-canvas:not([hidden]) iframe[name="detail::a::desktop"]');
  const detailFile = path.join(tmp, 'design/screens/detail/a.html');
  const kept = () => detail.locator('html').evaluate(() => window.__dfwKept === true);
  /** The frame took the change (it posts dfw:ready after the design event) and lost the marker: it reloaded. */
  const reloaded = async (what, since) => {
    await settled('detail::a::desktop', since);
    assert.equal(await kept(), false, `${what}: the frame was reloaded`);
  };
  assert.equal(await detail.locator('#made li').count(), 2);
  await detail.locator('html').evaluate(() => (window.__dfwKept = true));
  let since = await mark();
  fs.writeFileSync(detailFile, fs.readFileSync(detailFile, 'utf8').replace('Order 1042', 'Order 1043'));
  await reloaded('a screen with a script', since);
  assert.equal(await detail.locator('[data-dfw-id="detail.title"]').textContent(), 'Order 1043');
  assert.equal(await detail.locator('#made li').count(), 2, 'the list its script builds is still there');
  await detail.locator('html').evaluate(() => (window.__dfwKept = true));
  since = await mark();
  fs.writeFileSync(logoFile, logo('#0000ff'));
  await reloaded('a changed image', since);
  fs.writeFileSync(detailFile, fs.readFileSync(detailFile, 'utf8').replace('Order 1043', 'Order 1042'));
  await detail.locator('[data-dfw-id="detail.title"]', { hasText: 'Order 1042' }).waitFor();

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
  await page.waitForSelector('.page-canvas:not([hidden]) .frame[data-key="board::a::desktop"] iframe');
  await page.click('.page-canvas:not([hidden]) .frame[data-key="board::a::desktop"] .frame-title');
  await page.keyboard.press('Shift+2');
  await frameReady('board::a::desktop');
  const board = page.frameLocator('.page-canvas:not([hidden]) iframe[name="board::a::desktop"]');
  const waitBoard = async (re, msg) => {
    await fileUntil(boardFile, (t) => re.test(t));
    assert.match(fs.readFileSync(boardFile, 'utf8'), re, msg);
  };
  const center = (b) => [b.x + b.width / 2, b.y + b.height / 2];
  await board.locator('[data-dfw-id="board.three"]').click();
  await board.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  const b3 = await board.locator('[data-dfw-id="board.three"]').boundingBox();
  const b1 = await board.locator('[data-dfw-id="board.one"]').boundingBox();
  since = await mark();
  await page.mouse.move(...center(b3));
  await page.mouse.down();
  await page.mouse.move(b1.x + b1.width * 0.2, b1.y + b1.height / 2, { steps: 10 });
  await page.mouse.up();
  await waitBoard(
    /<section class="cards" data-dfw-id="board.cards">\n {6}<div data-dfw-id="board.three">Three<\/div>\n {6}<div data-dfw-id="board.one">One<\/div>\n {6}<div data-dfw-id="board.two">Two<\/div>\n {4}<\/section>/,
    'drag reordered the source (indentation kept)',
  );
  await settled('board::a::desktop', since); // the live update after the drag
  await board.locator('[data-dfw-id="board.two"]').click();
  await board.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  const b2 = await board.locator('[data-dfw-id="board.two"]').boundingBox();
  const hx = await board.locator('[data-dfw-handle="x"]').boundingBox();
  since = await mark();
  await page.mouse.move(...center(hx));
  await page.mouse.down();
  await page.mouse.move(b2.x + b2.width * 2.05, hx.y + hx.height / 2, { steps: 10 });
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-10-resize.png') });
  await page.mouse.up();
  await waitBoard(
    /<style data-dfw-layout>[\s\S]*\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 2; \}[\s\S]*<\/style>\n<\/head>/,
    'resize wrote a grid-column span rule',
  );
  // The rule wins over the more specific class rule "main .cards > div { grid-column: span 1 }".
  await waitSpan('board::a::desktop', 'board.two', 'span 2', 'layout rule beats the more specific screen rule');
  // Keyboard alternative: Alt+Shift+Right steps the span.
  await settled('board::a::desktop', since);
  await board.locator('[data-dfw-id="board.two"]').click();
  await page.keyboard.press('Alt+Shift+ArrowRight');
  await waitBoard(/\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 3; \}/, 'Alt+Shift+Right widened the span');
  await waitSpan('board::a::desktop', 'board.two', 'span 3', 'computed span follows the keyboard step');

  // Narrower breakpoint: a resize in the Mobile frame lands in @media (max-width: 390px) and only affects that width.
  await page.click('#bp-menu summary');
  await page.check('#breakpoints label:has-text("Mobile") input');
  await page.click('#bp-menu summary');
  await page.waitForSelector('.page-canvas:not([hidden]) .frame[data-key="board::a::mobile"]', { state: 'attached' });
  // Select a frame by its title even when it is off-screen (titles select on mousedown).
  const pickFrame = (key) =>
    page.evaluate(
      (k) =>
        document
          .querySelector(`.page-canvas:not([hidden]) .frame[data-key="${k}"] .frame-title`)
          .dispatchEvent(new MouseEvent('mousedown', { bubbles: true, button: 0 })),
      key,
    );
  await pickFrame('board::a::mobile');
  await page.keyboard.press('Shift+2');
  await page.waitForSelector('.page-canvas:not([hidden]) .frame[data-key="board::a::mobile"] iframe', { state: 'attached' });
  await frameReady('board::a::mobile');
  const boardM = page.frameLocator('.page-canvas:not([hidden]) iframe[name="board::a::mobile"]');
  await boardM.locator('[data-dfw-id="board.two"]').click();
  await boardM.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  const m2b = await boardM.locator('[data-dfw-id="board.two"]').boundingBox();
  const mhx = await boardM.locator('[data-dfw-handle="x"]').boundingBox();
  since = await mark();
  await page.mouse.move(...center(mhx));
  await page.mouse.down();
  await page.mouse.move(m2b.x + m2b.width / 3, mhx.y + mhx.height / 2, { steps: 10 }); // ~1 of the 3 spanned columns
  await page.mouse.up();
  await waitBoard(
    /\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 3; \}\n {4}@media \(max-width: 390px\) \{\n {6}\[data-dfw-id='board.two'\]:not\(#dfw\) \{ grid-column: span 1; \}\n {4}\}/,
    'mobile resize went into the 390px media block; base rule kept',
  );
  await waitSpan('board::a::mobile', 'board.two', 'span 1', 'mobile frame (390px) uses the media rule');
  await waitSpan('board::a::desktop', 'board.two', 'span 3', 'desktop frame (1440px) is unaffected');

  // Reset to auto: Alt+Shift+0 in the Mobile frame drops only the 390px rule (and the empty @media block).
  await settled('board::a::mobile', since);
  await boardM.locator('[data-dfw-id="board.two"]').click();
  since = await mark();
  await page.keyboard.press('Alt+Shift+Digit0');
  await fileUntil(boardFile, (t) => !/@media/.test(t));
  assert.doesNotMatch(fs.readFileSync(boardFile, 'utf8'), /@media/, 'Alt+Shift+0 removed the media rule and its empty block');
  await waitSpan('board::a::mobile', 'board.two', 'span 3', 'mobile falls back to the base rule');
  // Double-click the width handle in the Desktop frame: the base rule goes, and with it the whole block.
  await pickFrame('board::a::desktop');
  await page.keyboard.press('Shift+2');
  await settled('board::a::desktop', since); // the desktop frame shows the same file
  await board.locator('[data-dfw-id="board.two"]').click();
  await board.locator('[data-dfw-handle="x"]').waitFor({ state: 'visible' });
  since = await mark();
  await board.locator('[data-dfw-handle="x"]').dblclick();
  await fileUntil(boardFile, (t) => !/data-dfw-layout/.test(t));
  assert.doesNotMatch(
    fs.readFileSync(boardFile, 'utf8'),
    /data-dfw-layout/,
    'double-click on the handle reset the span; empty block removed',
  );
  await waitSpan('board::a::desktop', 'board.two', 'span 1', 'back to the screen rule');

  // Drop into an empty grid container with a data-dfw-id (dashed highlight while dragging).
  await settled('board::a::desktop', since);
  await board.locator('[data-dfw-id="board.one"]').click();
  const bOne = await board.locator('[data-dfw-id="board.one"]').boundingBox();
  const bEmpty = await board.locator('[data-dfw-id="board.empty"]').boundingBox();
  since = await mark();
  await page.mouse.move(...center(bOne));
  await page.mouse.down();
  await page.mouse.move(...center(bEmpty), { steps: 10 });
  assert.equal(
    await board
      .locator('[data-dfw-ui]')
      .evaluateAll((els) => els.some((e) => e.style.borderStyle === 'dashed' && e.style.display === 'block')),
    true,
    'empty container highlighted',
  );
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-11-drop-inside.png') });
  await page.mouse.up();
  await waitBoard(
    /<section class="cards" data-dfw-id="board.empty">\n {6}<div data-dfw-id="board.one">One<\/div>\n {4}<\/section>/,
    'dropped into the empty container (source moved and indented)',
  );

  // A rule that still wins (!important) is named instead of the resize silently doing nothing; the inspector resets it.
  await settled('board::a::desktop', since);
  await board.locator('[data-dfw-id="board.pinned"]').click();
  since = await mark();
  await page.keyboard.press('Alt+Shift+ArrowRight');
  await waitBoard(/\[data-dfw-id='board.pinned'\]:not\(#dfw\) \{ grid-column: span 2; \}/, 'pinned resize written');
  await page.waitForFunction(() => /did not change/.test(document.querySelector('#toast').textContent), null, { timeout: 8000 });
  assert.match(await page.textContent('#toast'), /"\.cards > \.pinned" still sets grid-column/, 'toast names the conflicting selector');
  assert.equal(await page.evaluate(() => window.__dfwStudio.layoutConflict?.selector), '.cards > .pinned');
  await settled('board::a::desktop', since);
  await board.locator('[data-dfw-id="board.pinned"]').click();
  await page.click('#inspector [data-act="layout-reset"]');
  await fileUntil(boardFile, (t) => !/data-dfw-layout/.test(t));
  assert.doesNotMatch(fs.readFileSync(boardFile, 'utf8'), /data-dfw-layout/, 'inspector "Reset size to auto" removed the rule');
  const lint = JSON.parse(spawnSync(DFW, ['lint', 'design/screens/board/a.html', '--json'], { cwd: tmp, env, encoding: 'utf8' }).stdout);
  assert.equal(lint.tally.error, 0, `lint clean after layout edits: ${JSON.stringify(lint.results).slice(0, 400)}`);

  // The link the agent gives (`dfw studio start` prints it) opens the Design System page, not the
  // page the Studio would open by itself; an unknown page id is ignored.
  const link = studio('status').match(/^design system: (\S+)$/m)?.[1];
  assert.equal(link, `${url}?page=system`);
  const linked = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  try {
    await linked.goto(link);
    await linked.waitForFunction(() => window.__dfwStudio?.page === 'system');
    await linked.goto(`${url}?page=nope`);
    await linked.waitForFunction(() => !!window.__dfwStudio?.page);
    assert.notEqual(await linked.evaluate(() => window.__dfwStudio.page), 'nope');
  } finally {
    await linked.close();
  }

  // The Design System page of a project with a style guide (palette ramps, used-token chips):
  // no bold text in its mono font either.
  // Its own folder: inside the first project, dfw would find that project instead.
  const dsProject = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-e2e-ds-'));
  const inDs = (...a) => execFileSync(DFW, a, { cwd: dsProject, env, encoding: 'utf8' });
  inDs('init', '--product', 'E2E Palette');
  inDs('system', 'init', '--style', 'heritage-signboard');
  const dsPort = port + 200;
  const dsUrl = inDs('studio', 'start', '--port', String(dsPort)).match(/http:\/\/\S+/)[0];
  try {
    const ds = await browser.newPage({ viewport: { width: 1440, height: 900 } });
    await ds.goto(new URL('/__dfw/system.html', dsUrl).href);
    await ds.waitForSelector('.ds-chip.is-used');
    assert.deepEqual(await boldMono(ds, '--font-family-mono'), [], "no bold text in the Design System page's mono font");
    if (shotsDir) await ds.screenshot({ path: path.join(shotsDir, 'studio-12-ds-palette.png'), fullPage: true });
  } finally {
    inDs('studio', 'stop');
    fs.rmSync(dsProject, { recursive: true, force: true });
  }

  assert.deepEqual(errors, [], `no page errors: ${errors.join('; ')}`);
  console.log('PASS studio e2e');
} catch (e) {
  failed = true;
  console.error('FAIL studio e2e:', e.message, (e.stack.match(/studio\.e2e\.mjs:\d+/) || [''])[0]);
  if (shotsDir && page) await page.screenshot({ path: path.join(shotsDir, 'studio-failure.png'), fullPage: true }).catch(() => {});
} finally {
  for (const w of waits) w.kill('SIGKILL');
  await browser.close();
  studio('stop');
  if (!args.includes('--keep')) fs.rmSync(tmp, { recursive: true, force: true });
  else console.log(`kept ${tmp}`);
}
process.exit(failed ? 1 : 0);
