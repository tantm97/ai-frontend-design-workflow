// Optional Lighthouse run for the verify phase (G4): performance, best practices and SEO of the running
// app. Opt-in per run ("dfw check --url <u> --lighthouse"): it runs a pinned Lighthouse through npx (a
// one-time ~20 MB download into the npm cache) against the Chrome dfw already uses. Accessibility is left
// to axe (dfw check). Error reporting stays off.
import path from 'node:path';
import fs from 'node:fs';
import { spawnSync } from 'node:child_process';
import { nodeExecutable } from './util.mjs';
import { findBrowser } from './cdp.mjs';

const LIGHTHOUSE_VERSION = '13.6.0';

export function runLighthouse(url, { timeoutMs = 240000 } = {}) {
  const node = nodeExecutable();
  const browser = findBrowser();
  if (!node || !browser) return { tool: 'lighthouse', status: 'unverifiable', detail: !node ? 'needs Node.js (npx)' : 'no Chrome/Edge/Chromium', findings: [] };
  const npx = path.join(path.dirname(node), process.platform === 'win32' ? 'npx.cmd' : 'npx');
  if (!fs.existsSync(npx)) return { tool: 'lighthouse', status: 'unverifiable', detail: `npx not found next to ${node}`, findings: [] };
  const r = spawnSync(npx, ['-y', `lighthouse@${LIGHTHOUSE_VERSION}`, url, '--output=json', '--output-path=stdout', '--quiet', '--no-enable-error-reporting', '--only-categories=performance,best-practices,seo', '--chrome-flags=--headless=new --no-first-run --no-default-browser-check --disable-features=HttpsUpgrades,HttpsFirstBalancedModeAutoEnable,HttpsFirstModeV2ForTypicallySecureUsers'], {
    encoding: 'utf8',
    timeout: timeoutMs,
    maxBuffer: 64 * 1024 * 1024,
    env: { ...process.env, CHROME_PATH: browser.path, NO_UPDATE_NOTIFIER: '1' },
    shell: process.platform === 'win32',
  });
  return lighthouseReport(r.stdout, url, r.stderr || r.error?.message);
}

/** Lighthouse's JSON output -> a dfw check result (status "error" when there is no usable report). */
export function lighthouseReport(stdout, url, errorDetail = '') {
  let lhr = null;
  try {
    lhr = JSON.parse(stdout);
  } catch {}
  // JSON.parse(null) is null (spawn failed: no stdout at all), and "null" is valid JSON too.
  if (!lhr || typeof lhr !== 'object') return { tool: 'lighthouse', status: 'error', detail: String(errorDetail || 'no output').slice(0, 300), findings: [] };
  const findings = [];
  // A category Lighthouse could not score has score null: that is not a 0.
  const scores = Object.values(lhr.categories || {}).map((c) => `${c.title} ${typeof c.score === 'number' ? Math.round(c.score * 100) : 'n/a'}`);
  for (const cat of Object.values(lhr.categories || {})) {
    for (const ref of cat.auditRefs || []) {
      const a = lhr.audits?.[ref.id];
      if (!a || a.score == null || a.score >= 0.9 || !['binary', 'numeric', 'metricSavings'].includes(a.scoreDisplayMode) || !ref.weight) continue;
      findings.push({ level: a.score < 0.5 ? 'warn' : 'info', rule: `lighthouse-${a.id}`, file: url, line: 1, source: 'lighthouse', message: `${cat.title}: ${a.title}${a.displayValue ? ` (${a.displayValue})` : ''}` });
    }
  }
  return { tool: 'lighthouse', status: 'ran', detail: `scores: ${scores.join(', ')}`, findings };
}
