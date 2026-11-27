// @ts-check
// Records the demo GIF of the dfw Design Studio (media/demo.gif): the canvas with several screens,
// a pan and a zoom, selecting an element (the inspector shows its component and tokens), a direct
// text edit, then click-to-comment and "Send to AI", and a short look at the Design System page.
//
// Usage: node scripts/demo-gif.mjs
// Needs: a built dfw binary ($DFW_BIN, else native/target/debug/dfw: cd native && cargo build),
// the dev-only "playwright" package (this repo's node_modules or $DFW_RUNTIME_DIR, as the Studio e2e
// test finds it) with its Chromium or a Chrome in $DFW_BROWSER / $CHROME_PATH, and ffmpeg ($FFMPEG,
// else ffmpeg on PATH). The project is a copy of tests/fixtures/penpot/project in a temp folder,
// with two more screens; it is removed at the end, and the Studio is stopped.
/* global document */
import fs from 'node:fs';
import net from 'node:net';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { setTimeout as delay } from 'node:timers/promises';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DFW = process.env.DFW_BIN
  ? path.resolve(REPO, process.env.DFW_BIN)
  : path.join(REPO, 'native', 'target', 'debug', process.platform === 'win32' ? 'dfw.exe' : 'dfw');
const FFMPEG = process.env.FFMPEG || 'ffmpeg';
const FIXTURE = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');
const OUT = path.join(REPO, 'media', 'demo.gif');
const PRODUCT = 'Fixture';

/** Recording: the browser viewport, then the GIF scaled to OUT_WIDTH at FPS. */
const VIEWPORT = { width: 1440, height: 900 };
const OUT_WIDTH = 960;
const FPS = 12;
const MAX_BYTES = 4 * 1024 * 1024;

/** Pacing (ms): a pause lets a viewer read a step; a move is how long the cursor travels. */
const PAUSE = { short: 600, step: 1000, read: 1600 };
const MOVE_MS = 450;
const MOVE_STEPS = 15;
const TYPE_DELAY_MS = 70;
const WHEEL = { steps: 8, pan: 30, zoom: 120, zoomKey: process.platform === 'darwin' ? 'Meta' : 'Control' };
const WAIT_MS = 15000;
/** Where to zoom in, as a fraction of the frame's height: a screen's centre, the colours of the Design System page. */
const ZOOM_AT = { centre: 0.5, system: 0.22 };

/** The screens added to the fixture page, the frame and elements the demo acts on, the texts typed. */
const PAGE_ID = 'stock';
const FRAME = 'items::a::desktop';
const TARGET = { select: 'items.add', edit: 'items.title', comment: 'items.summary' };
const EDIT_TEXT = 'Stock list';
const COMMENT_TEXT = 'Make the low-stock count stand out';
const SELECT_ALL = process.platform === 'darwin' ? 'Meta+a' : 'Control+a';
const SAVE_KEY = process.platform === 'darwin' ? 'Meta+Enter' : 'Control+Enter';

/** Studio selectors (core/studio/app.html and app/*.js). */
const SEL = {
  frame: '.page-canvas:not([hidden]) .frame iframe',
  frameByKey: (/** @type {string} */ k) => `.page-canvas:not([hidden]) iframe[name="${k}"]`,
  frameBox: (/** @type {string} */ k) => `.page-canvas:not([hidden]) .frame[data-key="${k}"]`,
  frameTitle: (/** @type {string} */ k) => `.page-canvas:not([hidden]) .frame[data-key="${k}"] .frame-title`,
  systemFrame: '.page-canvas:not([hidden]) .frame',
  byId: (/** @type {string} */ id) => `[data-dfw-id="${id}"]`,
  editing: '[contenteditable]',
  viewport: '#viewport',
  inspector: '#inspector .isec',
  composer: '#composer:not([hidden])',
  composerText: '#composer-text',
  send: '#send-drafts',
  toast: '#toast:not([hidden])',
  systemPage: '#page-list button[data-page="system"]',
  cursor: 'demo-cursor',
};
const KEY = { fit: 'Shift+1', toSelection: 'Shift+2', select: 'v', comment: 'c', commit: 'Enter' };
const CURSOR = { size: 18, color: '#e5484d', ring: 'rgba(229,72,77,.35)' };

const SCREENS = [
  {
    id: 'detail',
    title: 'Item detail',
    body: `<main data-dfw-id="detail">
    <div class="bar"><h1 data-dfw-id="detail.title">Steel bolts M8</h1><dfw-use c="button" variant="primary" id="detail.order">Reorder</dfw-use></div>
    <section class="panel" data-dfw-id="detail.panel">
      <p class="rule" data-dfw-id="detail.stock">In stock: <strong>40</strong> boxes</p>
      <p data-dfw-id="detail.where">Shelf B2, aisle 4</p>
      <p data-dfw-id="detail.supplier">Supplier: North Hardware</p>
    </section>
  </main>`,
  },
  {
    id: 'suppliers',
    title: 'Suppliers',
    body: `<main data-dfw-id="suppliers">
    <div class="bar"><h1 data-dfw-id="suppliers.title">Suppliers</h1><dfw-use c="button" variant="ghost" id="suppliers.add">Add supplier</dfw-use></div>
    <section class="panel" data-dfw-id="suppliers.list">
      <p class="rule">North Hardware · 14 items</p>
      <p class="rule">Harbour Tools · 9 items</p>
      <p>Ridge Fasteners · 5 items</p>
    </section>
  </main>`,
  },
];

function loadPlaywright() {
  for (const base of [REPO, process.env.DFW_RUNTIME_DIR].filter(Boolean)) {
    try {
      return createRequire(path.join(base, 'package.json'))('playwright');
    } catch {}
  }
  return null;
}

/** A port nothing listens on now. */
function freePort() {
  return new Promise((resolve, reject) => {
    const srv = net.createServer();
    srv.on('error', reject);
    srv.listen(0, '127.0.0.1', () => {
      const addr = srv.address();
      srv.close(() => resolve(typeof addr === 'object' && addr ? addr.port : 0));
    });
  });
}

/** The fixture copy, with SCREENS written next to its own screen (same styles) and listed on its page. */
function makeProject(dir, dfw) {
  fs.cpSync(FIXTURE, dir, { recursive: true });
  dfw('init', '--product', PRODUCT);
  const manifestFile = path.join(dir, 'design', 'design.json');
  const manifest = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
  const base = fs.readFileSync(path.join(dir, 'design', 'screens', 'items', 'a.html'), 'utf8');
  for (const s of SCREENS) {
    const file = `screens/${s.id}/a.html`;
    const html = base.replace(/<title>[^<]*<\/title>/, `<title>${s.title} - ${PRODUCT}</title>`).replace(/<main[\s\S]*<\/main>/, s.body);
    fs.mkdirSync(path.join(dir, 'design', 'screens', s.id), { recursive: true });
    fs.writeFileSync(path.join(dir, 'design', file), html);
    manifest.screens.push({ id: s.id, title: s.title, variants: [{ id: 'a', file }] });
    manifest.pages.find((p) => p.id === PAGE_ID).screens.push(s.id);
  }
  fs.writeFileSync(manifestFile, JSON.stringify(manifest, null, 2));
  dfw('build');
}

/** A dot in the Studio page that shows where the (invisible) Playwright mouse is. */
async function addCursor(page) {
  await page.evaluate(
    ([id, c, ms]) => {
      const dot = document.createElement('div');
      dot.id = id;
      Object.assign(dot.style, {
        position: 'fixed',
        left: '0',
        top: '0',
        width: `${c.size}px`,
        height: `${c.size}px`,
        margin: `-${c.size / 2}px 0 0 -${c.size / 2}px`,
        borderRadius: '50%',
        background: c.color,
        boxShadow: `0 0 0 6px ${c.ring}`,
        border: '2px solid #fff',
        pointerEvents: 'none',
        zIndex: '2147483647',
        transition: `translate ${ms}ms ease, scale 150ms ease`,
        translate: '50vw 50vh',
      });
      document.body.append(dot);
    },
    [SEL.cursor, CURSOR, MOVE_MS],
  );
}

/** Move the mouse and the dot together to (x, y). */
async function moveTo(page, x, y) {
  await page.evaluate(([id, x, y]) => (document.getElementById(id).style.translate = `${x}px ${y}px`), [SEL.cursor, x, y]);
  await page.mouse.move(x, y, { steps: MOVE_STEPS });
  await delay(MOVE_MS);
}

/** The centre of a locator in page coordinates. */
async function centre(locator) {
  const b = await locator.boundingBox();
  if (!b) throw new Error('demo: an element to point at is not on screen');
  return [b.x + b.width / 2, b.y + b.height / 2];
}

/** Zoom by `dy` wheel units (negative zooms in) at a point of a locator, `at` of its height down. */
async function zoomAt(page, locator, dy, at = ZOOM_AT.centre) {
  const b = await locator.boundingBox();
  if (!b) throw new Error('demo: an element to zoom at is not on screen');
  await moveTo(page, b.x + b.width / 2, b.y + b.height * at);
  await page.keyboard.down(WHEEL.zoomKey);
  for (let i = 0; i < WHEEL.steps; i++) {
    await page.mouse.wheel(0, dy / WHEEL.steps);
    await delay(MOVE_MS / WHEEL.steps);
  }
  await page.keyboard.up(WHEEL.zoomKey);
}

/** Point at a locator, then click (or double-click) it with a short pulse of the dot. */
async function clickOn(page, locator, clickCount = 1) {
  const [x, y] = await centre(locator);
  await moveTo(page, x, y);
  await page.evaluate((id) => (document.getElementById(id).style.scale = '0.7'), SEL.cursor);
  await page.mouse.click(x, y, { clickCount });
  await delay(PAUSE.short / 4);
  await page.evaluate((id) => (document.getElementById(id).style.scale = '1'), SEL.cursor);
}

/** The demo itself, step by step. */
async function demo(page) {
  const frame = page.frameLocator(SEL.frameByKey(FRAME));
  const vp = await page.locator(SEL.viewport).boundingBox();
  const mid = [vp.x + vp.width / 2, vp.y + vp.height / 2];

  // 1. The canvas with every screen of the page, fitted.
  await page.keyboard.press(KEY.fit);
  await moveTo(page, mid[0], mid[1]);
  await delay(PAUSE.read);

  // 2. Pan (scroll), zoom in at the pointer (Cmd/Ctrl + scroll), then fit again.
  for (let i = 0; i < WHEEL.steps; i++) await page.mouse.wheel(WHEEL.pan, WHEEL.pan / 2);
  await delay(PAUSE.short);
  for (let i = 0; i < WHEEL.steps; i++) await page.mouse.wheel(-WHEEL.pan, -WHEEL.pan / 2);
  await delay(PAUSE.short);
  await zoomAt(page, page.locator(SEL.frameBox(FRAME)), -WHEEL.zoom);
  await delay(PAUSE.step);
  await page.keyboard.press(KEY.fit);
  await delay(PAUSE.step);

  // Zoom to the first desktop frame so its text is readable.
  await clickOn(page, page.locator(SEL.frameTitle(FRAME)));
  await page.keyboard.press(KEY.toSelection);
  await delay(PAUSE.step);

  // 3. Select a component instance: the inspector shows the component, its variant and tokens.
  await page.keyboard.press(KEY.select);
  await clickOn(page, frame.locator(SEL.byId(TARGET.select)));
  await page.waitForSelector(SEL.inspector);
  await delay(PAUSE.read);

  // 4. Direct edit: double-click the heading, type a new text, Enter writes it to the source.
  await clickOn(page, frame.locator(SEL.byId(TARGET.edit)), 2);
  await frame.locator(`${SEL.byId(TARGET.edit)}${SEL.editing}`).waitFor({ timeout: WAIT_MS });
  await page.keyboard.press(SELECT_ALL);
  await page.keyboard.type(EDIT_TEXT, { delay: TYPE_DELAY_MS });
  await delay(PAUSE.short);
  await page.keyboard.press(KEY.commit);
  await frame.locator(SEL.byId(TARGET.edit), { hasText: EDIT_TEXT }).waitFor({ timeout: WAIT_MS });
  await delay(PAUSE.step);

  // 5. Click-to-comment: C, click an element, type, save the draft, then Send to AI.
  await page.keyboard.press(KEY.comment);
  await delay(PAUSE.short);
  await clickOn(page, frame.locator(SEL.byId(TARGET.comment)));
  await page.waitForSelector(SEL.composer);
  await page.locator(SEL.composerText).focus();
  await page.keyboard.type(COMMENT_TEXT, { delay: TYPE_DELAY_MS });
  await delay(PAUSE.short);
  await page.keyboard.press(SAVE_KEY);
  await delay(PAUSE.step);
  await clickOn(page, page.locator(SEL.send));
  await page.waitForSelector(SEL.toast, { timeout: WAIT_MS });
  await delay(PAUSE.read);

  // 6. The Design System page, zoomed in on its colours.
  await page.keyboard.press(KEY.select);
  await clickOn(page, page.locator(SEL.systemPage));
  await page.waitForSelector(SEL.frame);
  await page.keyboard.press(KEY.fit);
  await delay(PAUSE.step);
  await zoomAt(page, page.locator(SEL.systemFrame).first(), -WHEEL.zoom, ZOOM_AT.system);
  await delay(PAUSE.read);
}

const pw = loadPlaywright();
if (!pw) {
  console.error('demo-gif: playwright not found (npm i -D playwright, or set DFW_RUNTIME_DIR).');
  process.exit(1);
}
if (!fs.existsSync(DFW)) {
  console.error(`demo-gif: no dfw binary at ${DFW}. Build it (cd native && cargo build) or set DFW_BIN.`);
  process.exit(1);
}

const tmp = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-demo-'));
const project = path.join(tmp, 'project');
const videoDir = path.join(tmp, 'video');
const env = { ...process.env, DFW_HOME: path.join(tmp, '.dfwhome') };
const dfw = (...a) => execFileSync(DFW, a, { cwd: project, env, encoding: 'utf8' });
fs.mkdirSync(project);
makeProject(project, dfw);

const port = await freePort();
const url = dfw('studio', 'start', '--port', String(port)).match(/http:\/\/\S+/)[0];
const executablePath = process.env.DFW_BROWSER || process.env.CHROME_PATH || undefined;
const browser = await pw.chromium.launch(executablePath ? { executablePath } : {});
let failed = false;
try {
  const context = await browser.newContext({ viewport: VIEWPORT, recordVideo: { dir: videoDir, size: VIEWPORT } });
  const page = await context.newPage();
  const opened = Date.now();
  await page.goto(url);
  await page.waitForSelector(SEL.frame);
  await page.locator(SEL.frameBox(FRAME)).waitFor();
  await page.frameLocator(SEL.frameByKey(FRAME)).locator(SEL.byId(TARGET.edit)).waitFor({ timeout: WAIT_MS });
  await addCursor(page);
  await delay(PAUSE.short);
  const start = (Date.now() - opened) / 1000;
  await demo(page);
  const length = (Date.now() - opened) / 1000 - start;
  const video = page.video();
  await context.close();
  const webm = await video.path();

  // Two-pass GIF: one palette for the whole clip, then dithered frames that change only where the UI did.
  const palette = path.join(tmp, 'palette.png');
  const filters = `fps=${FPS},scale=${OUT_WIDTH}:-1:flags=lanczos`;
  const trim = ['-ss', start.toFixed(2), '-t', length.toFixed(2), '-i', webm];
  execFileSync(FFMPEG, ['-v', 'error', '-y', ...trim, '-vf', `${filters},palettegen=stats_mode=diff`, palette]);
  fs.mkdirSync(path.dirname(OUT), { recursive: true });
  execFileSync(FFMPEG, [
    '-v',
    'error',
    '-y',
    ...trim,
    '-i',
    palette,
    '-lavfi',
    `${filters}[x];[x][1:v]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle`,
    OUT,
  ]);
  const bytes = fs.statSync(OUT).size;
  console.log(`demo-gif: wrote ${path.relative(REPO, OUT)} (${bytes} bytes, ${length.toFixed(1)} s, ${FPS} fps, ${OUT_WIDTH} px wide)`);
  if (bytes > MAX_BYTES) {
    console.error(`demo-gif: the GIF is over ${MAX_BYTES} bytes; lower FPS or OUT_WIDTH.`);
    failed = true;
  }
} catch (e) {
  failed = true;
  console.error('demo-gif:', e.message);
} finally {
  await browser.close();
  dfw('studio', 'stop');
  fs.rmSync(tmp, { recursive: true, force: true });
}
process.exit(failed ? 1 : 0);
