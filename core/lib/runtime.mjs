// Optional local runtime for screenshots: Playwright (Apache-2.0) installed into
// ~/.config/dfw/runtime on explicit request ("dfw setup --playwright").
import fs from 'node:fs';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { globalDir, DfwError } from './util.mjs';

export const PLAYWRIGHT_VERSION = '1.64.1'; // pinned; released 2026-10-14

export function runtimeDir() {
  return process.env.DFW_RUNTIME_DIR || path.join(globalDir(), 'runtime');
}

export function playwrightStatus() {
  const dir = runtimeDir();
  const pkg = path.join(dir, 'node_modules', 'playwright', 'package.json');
  if (!fs.existsSync(pkg)) return { installed: false, dir };
  const version = JSON.parse(fs.readFileSync(pkg, 'utf8')).version;
  return { installed: true, version, dir };
}

export function installPlaywright({ log = console.error } = {}) {
  const dir = runtimeDir();
  fs.mkdirSync(dir, { recursive: true });
  const pkgFile = path.join(dir, 'package.json');
  if (!fs.existsSync(pkgFile)) fs.writeFileSync(pkgFile, JSON.stringify({ name: 'dfw-runtime', private: true }, null, 2));
  log(`Installing playwright@${PLAYWRIGHT_VERSION} into ${dir} ...`);
  let r = spawnSync('npm', ['install', '--no-fund', '--no-audit', `playwright@${PLAYWRIGHT_VERSION}`], { cwd: dir, stdio: 'inherit' });
  if (r.status !== 0) throw new DfwError('npm install playwright failed.');
  log('Installing Chromium for Playwright ...');
  r = spawnSync(path.join(dir, 'node_modules', '.bin', 'playwright'), ['install', 'chromium'], { cwd: dir, stdio: 'inherit' });
  if (r.status !== 0) throw new DfwError('playwright install chromium failed.');
  return playwrightStatus();
}

export async function loadPlaywright() {
  const st = playwrightStatus();
  if (!st.installed) throw new DfwError('Playwright is not installed for dfw. Ask the user, then run: dfw setup --playwright');
  const require = createRequire(path.join(st.dir, 'package.json'));
  return require('playwright');
}

/**
 * Screenshot a list of jobs: [{url, width, height, out, fullPage, selector}].
 * Returns [{out, ok, error}].
 */
export async function screenshot(jobs) {
  const { chromium } = await loadPlaywright();
  const browser = await chromium.launch();
  const results = [];
  try {
    for (const j of jobs) {
      const page = await browser.newPage({ viewport: { width: j.width, height: j.height }, deviceScaleFactor: j.scale || 1 });
      try {
        await page.goto(j.url, { waitUntil: 'networkidle', timeout: 30000 });
        await page.evaluate(() => document.fonts && document.fonts.ready);
        fs.mkdirSync(path.dirname(j.out), { recursive: true });
        if (j.selector) {
          const el = await page.$(j.selector);
          if (!el) throw new Error(`selector not found: ${j.selector}`);
          await el.screenshot({ path: j.out });
        } else {
          await page.screenshot({ path: j.out, fullPage: j.fullPage !== false });
        }
        results.push({ out: j.out, ok: true });
      } catch (e) {
        results.push({ out: j.out, ok: false, error: e.message });
      } finally {
        await page.close();
      }
    }
  } finally {
    await browser.close();
  }
  return results;
}
