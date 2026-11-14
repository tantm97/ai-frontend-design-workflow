// Rendered accessibility check: axe-core (Deque, MPL-2.0) injected over CDP into each composed screen
// (every state and theme) or into a live URL. Static lint cannot see computed contrast, ARIA validity,
// accessible names or landmark structure; axe can. axe.min.js is downloaded once, pinned and verified
// ("dfw setup --a11y"), and used unmodified (MPL-2.0 file-level terms), with its license file next to it.
// For live URLs it also clicks stateful controls (aria-pressed/expanded/selected/sort) and reports ones
// that change nothing ("dead-control").
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawnSync } from 'node:child_process';
import { pathToFileURL } from 'node:url';
import { globalDir, projectPaths, readJson, DfwError } from './util.mjs';
import { buildSystem, buildComposed } from './compose.mjs';
import { browserStatus, openRenderer, fontsReady } from './runtime.mjs';
import { loadConfig } from './config.mjs';

export const AXE = {
  version: '4.14.0',
  tarball: 'https://registry.npmjs.org/axe-core/-/axe-core-4.14.0.tgz',
  integrity: 'sha512-9WTZxEjsZ7b13TH8JPmbV2z8CHbl80/2hm3XPEG4JgNdQLK81IBRXmSxHfMAOkSqQeRxT/0dwNDz2GOm3zzpcQ==',
};
const axeDir = () => path.join(globalDir(), 'tools', 'axe-core', AXE.version);
const axeFile = () => path.join(axeDir(), 'axe.min.js');

export function axeStatus() {
  return fs.existsSync(axeFile()) ? { installed: true, path: axeFile(), version: AXE.version } : { installed: false, detail: 'axe-core not downloaded (dfw setup --a11y, ~550 KB, once)' };
}

/** Download the pinned axe-core tarball, check its npm integrity hash, keep axe.min.js + LICENSE. */
export async function installAxe({ log = console.error } = {}) {
  if (axeStatus().installed) return axeStatus();
  log(`Downloading axe-core ${AXE.version} from registry.npmjs.org ...`);
  const res = await fetch(AXE.tarball, { signal: AbortSignal.timeout(60000) });
  if (!res.ok) throw new DfwError(`axe-core download failed: HTTP ${res.status}`);
  const buf = Buffer.from(await res.arrayBuffer());
  const got = `sha512-${crypto.createHash('sha512').update(buf).digest('base64')}`;
  if (got !== AXE.integrity) throw new DfwError('axe-core download does not match the pinned integrity hash; refusing to use it.');
  const dir = axeDir();
  fs.mkdirSync(dir, { recursive: true });
  const tgz = path.join(dir, 'axe-core.tgz');
  fs.writeFileSync(tgz, buf);
  const r = spawnSync('tar', ['-xzf', tgz, '-C', dir, '--strip-components=1', 'package/axe.min.js', 'package/LICENSE'], { encoding: 'utf8' });
  fs.rmSync(tgz, { force: true });
  if (r.status !== 0 || !fs.existsSync(axeFile())) throw new DfwError(`could not extract axe-core (needs tar): ${(r.stderr || '').slice(0, 200)}`);
  return axeStatus();
}

const IMPACT = { critical: 'error', serious: 'error', moderate: 'warn', minor: 'info' };

// Runs in the page after axe is injected.
async function runAxe() {
  const r = await window.axe.run(document, { runOnly: { type: 'tag', values: ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa', 'best-practice'] }, resultTypes: ['violations'] });
  return r.violations.map((v) => ({ id: v.id, impact: v.impact, help: v.help, nodes: v.nodes.length, target: String(v.nodes[0]?.target?.[0] || '').slice(0, 80), summary: String(v.nodes[0]?.failureSummary || '').split('\n').slice(1, 2).join(' ').trim().slice(0, 160) }));
}

// Runs in a live page: click stateful controls and report the ones that change nothing.
async function deadControls() {
  const sel = 'button[aria-pressed], button[aria-expanded], [role="tab"][aria-selected], [role="switch"], th[aria-sort] button, button[aria-sort]';
  const out = [];
  for (const el of [...document.querySelectorAll(sel)].slice(0, 12)) {
    if (el.closest('form') || el.disabled || !el.getClientRects().length) continue;
    let changed = 0;
    const mo = new MutationObserver((m) => (changed += m.length));
    mo.observe(document.body, { subtree: true, childList: true, attributes: true, characterData: true });
    el.click();
    await new Promise((r) => setTimeout(r, 150));
    mo.disconnect();
    if (!changed) out.push((el.getAttribute('aria-label') || el.textContent || el.tagName).trim().slice(0, 60));
  }
  return out;
}

/**
 * opts: { url?, screens?: [ids], states?: bool, themes?: bool }. Returns { tool, status, findings, detail? }.
 * status "unverifiable" (no browser / axe not downloaded) is reported, never treated as a pass.
 */
export async function a11yCheck(root, opts = {}) {
  const bs = browserStatus();
  if (!bs.installed) return { tool: 'axe', status: 'unverifiable', detail: bs.detail, findings: [] };
  const ax = axeStatus();
  if (!ax.installed) return { tool: 'axe', status: 'unverifiable', detail: ax.detail, findings: [] };
  const axeSrc = fs.readFileSync(ax.path, 'utf8');
  const jobs = [];
  if (opts.url) jobs.push({ url: opts.url, label: opts.url, rel: opts.url, width: 1280, height: 800, live: true });
  else {
    const p = projectPaths(root);
    const m = readJson(p.manifest, {});
    const bps = m.breakpoints || loadConfig(root).studio.breakpoints;
    const bp = bps.find((b) => b.width >= 1024) || bps[0];
    buildSystem(root);
    const rels = [];
    for (const s of m.screens || []) {
      if (opts.screens && !opts.screens.includes(s.id)) continue;
      for (const v of s.variants || []) {
        const rel = `design/${v.file}`;
        rels.push(rel);
        for (const st of opts.states ? [null, ...(s.states || m.states || [])] : [null]) {
          for (const th of opts.themes ? [null, ...(m.themes || [])] : [null]) {
            jobs.push({ rel, label: `${s.id}/${v.id}${st ? ` [${st}]` : ''}${th ? ` {${th}}` : ''}`, state: st, theme: th, width: bp.width, height: bp.height });
          }
        }
      }
    }
    const files = new Map(buildComposed(root, [...new Set(rels)]).map((r) => [r.rel, r.out]));
    for (const j of jobs) j.url = pathToFileURL(files.get(j.rel)).href;
  }
  if (!jobs.length) return { tool: 'axe', status: 'skipped', findings: [] };
  const renderer = await openRenderer();
  const findings = [];
  try {
    let next = 0;
    const worker = async () => {
      while (next < jobs.length) {
        const j = jobs[next++];
        let page = null;
        try {
          page = await renderer.page({ width: j.width, height: j.height });
          await page.goto(j.url, { timeout: 30000 });
          await page.evaluate((a) => {
            if (a.state) document.documentElement.setAttribute('data-state', a.state);
            if (a.theme) document.documentElement.dataset.theme = a.theme;
          }, { state: j.state, theme: j.theme });
          await fontsReady(page);
          await page.send('Runtime.evaluate', { expression: axeSrc });
          for (const v of await page.evaluate(runAxe)) {
            findings.push({ level: IMPACT[v.impact] || 'warn', rule: `axe-${v.id}`, file: j.rel, line: 1, source: 'axe', message: `${j.label}: ${v.help} (${v.nodes} element(s), e.g. ${v.target})${v.summary ? `: ${v.summary}` : ''}` });
          }
          if (j.live) for (const name of await page.evaluate(deadControls)) findings.push({ level: 'warn', rule: 'dead-control', file: j.rel, line: 1, source: 'axe', message: `${j.label}: control "${name}" announces a state (aria-pressed/expanded/selected/sort) but clicking it changes nothing.` });
        } catch (e) {
          findings.push({ level: 'warn', rule: 'axe-render', file: j.rel, line: 1, source: 'axe', message: `${j.label}: could not check (${e.message.slice(0, 120)})` });
        } finally {
          await page?.close();
        }
      }
    };
    await Promise.all(Array.from({ length: Math.min(6, jobs.length) }, worker));
  } finally {
    await renderer.close();
  }
  return { tool: 'axe', status: 'ran', findings };
}
