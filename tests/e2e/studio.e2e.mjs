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
<button class="cta" data-dfw-id="home.cta">New order</button></main></body></html>`,
);
const manifestFile = path.join(tmp, 'design/design.json');
const m = JSON.parse(fs.readFileSync(manifestFile, 'utf8'));
m.screens = [{ id: 'home', title: 'Home', variants: [{ id: 'a', label: 'A', file: 'screens/home/a.html' }, { id: 'b', label: 'B', file: 'screens/home/a.html' }] }];
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
  await page.waitForSelector('.frame-box iframe');
  const frames = await page.$$('.frame-box iframe');
  assert.equal(frames.length, 6, '2 variants x 3 breakpoints');
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

  // A pin is drawn in the matching frames.
  await page.keyboard.press('v');
  assert.equal(await page.getAttribute('body', 'data-mode'), 'view', 'V returns to view mode');
  const pinCount = await frame.locator('[data-dfw-ui] button').count();
  assert.equal(pinCount, 1, 'pin drawn on the commented element');

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

  // Live reload: editing the design file reloads frames.
  fs.writeFileSync(path.join(tmp, 'design/screens/home/a.html'), fs.readFileSync(path.join(tmp, 'design/screens/home/a.html'), 'utf8').replace('Welcome back', 'Good morning'));
  await page.waitForTimeout(1200);
  assert.equal(await frame.locator('[data-dfw-id="home.title"]').textContent(), 'Good morning', 'frame reloaded after edit');
  if (shotsDir) await page.screenshot({ path: path.join(shotsDir, 'studio-4-reloaded.png') });

  assert.deepEqual(errors, [], `no page errors: ${errors.join('; ')}`);
  console.log('PASS studio e2e');
} catch (e) {
  failed = true;
  console.error('FAIL studio e2e:', e.message);
} finally {
  await browser.close();
  dfw('studio', 'stop');
  if (!args.includes('--keep')) fs.rmSync(tmp, { recursive: true, force: true });
  else console.log(`kept ${tmp}`);
}
process.exit(failed ? 1 : 0);
