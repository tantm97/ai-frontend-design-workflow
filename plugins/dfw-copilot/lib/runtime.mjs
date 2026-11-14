// Headless rendering for screenshots and the layout check. It drives the Chrome / Edge /
// Chromium already on the machine over the DevTools protocol (lib/cdp.mjs): nothing to
// npm-install. "dfw setup --browser" downloads a headless Chrome only when none is found.
import { launch, fontsReady, browserStatus, installBrowser } from './cdp.mjs';

export { browserStatus, installBrowser, fontsReady };

/** Start a browser plus one shared context per device scale (shared memory cache, cached fonts). */
export async function openRenderer() {
  const browser = await launch();
  const ctxs = new Map();
  return {
    browser,
    async page({ width, height, scale = 1 }) {
      if (!ctxs.has(scale)) ctxs.set(scale, browser.newContext({ deviceScaleFactor: scale }));
      const page = await (await ctxs.get(scale)).newPage();
      await page.setViewportSize({ width, height });
      return page;
    },
    close: () => browser.close(),
  };
}

/**
 * Screenshot a list of jobs: [{url, width, height, out, fullPage, selector, scale}].
 * Returns [{out, ok, error}].
 */
export async function screenshot(jobs, { concurrency = 6 } = {}) {
  const r = await openRenderer();
  const results = new Array(jobs.length);
  let next = 0;
  const worker = async () => {
    while (next < jobs.length) {
      const i = next++;
      const j = jobs[i];
      let page = null;
      try {
        page = await r.page(j);
        // "load" + fonts, not "network idle": the studio keeps a live-reload connection open.
        await page.goto(j.url, { timeout: 30000 });
        await fontsReady(page);
        await page.waitForTimeout(60);
        await page.screenshot({ path: j.out, fullPage: j.fullPage !== false, selector: j.selector });
        results[i] = { out: j.out, ok: true };
      } catch (e) {
        results[i] = { out: j.out, ok: false, error: e.message };
      } finally {
        await page?.close();
      }
    }
  };
  try {
    await Promise.all(Array.from({ length: Math.min(concurrency, jobs.length) }, worker));
  } finally {
    await r.close();
  }
  return results;
}
