// Rendered layout check: what a reviewer sees, not what the HTML says.
// Renders each screen (composed, CSS inlined) at each breakpoint/state in Playwright and measures:
//   overlap        two content elements painting over each other (MUST)
//   h-overflow     page wider than the viewport (MUST)
//   anchor         an element marked data-dfw-anchor (feedback, primary action) not fully on screen,
//                  at rest or when lists grow (stress mode clones [data-dfw-grow] children) (MUST)
//   clipped        text cut off by its box (SHOULD)
//   void           large empty band between content blocks, or a mostly empty first viewport (SHOULD)
//   consistency    the same component + variant rendering differently across screens, or screens
//                  of the same page layout with different content edges (SHOULD)
// Also runs on the generated Design System page.
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, readJson, writeJson, toPosix } from './util.mjs';
import { buildComposed, buildSystem, composeHtml, loadComponents } from './compose.mjs';
import { renderSystemPage } from './system-page.mjs';
import { playwrightStatus, loadPlaywright } from './runtime.mjs';
import { loadConfig } from './config.mjs';

// Runs inside the page. Returns raw measurements; thresholds are applied in Node.
function measure(opts) {
  const vw = innerWidth;
  const vh = innerHeight;
  const vis = (el) => {
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 1 && r.height > 1;
  };
  // Stress: grow lists so overflow behaviour is tested, not assumed.
  if (opts.stress) {
    for (const list of document.querySelectorAll('[data-dfw-grow]')) {
      const kids = [...list.children];
      if (!kids.length) continue;
      const target = Number(list.getAttribute('data-dfw-grow')) || 12;
      let i = 0;
      while (list.children.length < target) list.appendChild(kids[i++ % kids.length].cloneNode(true));
    }
  }
  const out = { vw, vh, overlaps: [], clipped: [], anchors: [], hOverflow: 0, blocks: [], components: [], content: null };
  out.hOverflow = Math.max(0, document.documentElement.scrollWidth - vw);
  const label = (el) => el.getAttribute('data-dfw-id') || (el.getAttribute('data-dfw-component') ? `${el.getAttribute('data-dfw-component')}${el.getAttribute('data-dfw-variant') ? `/${el.getAttribute('data-dfw-variant')}` : ''}` : el.tagName.toLowerCase() + (el.className && typeof el.className === 'string' ? '.' + el.className.split(' ')[0] : ''));

  // Content blocks: elements that paint something meaningful (text, media, controls, filled/bordered boxes),
  // excluding page-sized containers.
  // Visible rectangle: the element box clipped by every scrolling/clipping ancestor.
  const visibleRect = (el) => {
    const b = el.getBoundingClientRect();
    let r = { left: b.left, top: b.top, right: b.right, bottom: b.bottom };
    for (let a = el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.overflowX === 'visible' && cs.overflowY === 'visible') continue;
      const ab = a.getBoundingClientRect();
      r = { left: Math.max(r.left, ab.left), top: Math.max(r.top, ab.top), right: Math.min(r.right, ab.right), bottom: Math.min(r.bottom, ab.bottom) };
      if (r.right <= r.left || r.bottom <= r.top) return null;
    }
    return { ...r, width: r.right - r.left, height: r.bottom - r.top };
  };
  const painted = [];
  for (const el of document.body.querySelectorAll('*')) {
    if (!vis(el)) continue;
    const r = visibleRect(el);
    if (!r || r.width < 1 || r.height < 1) continue;
    if (r.width * r.height > vw * vh * 0.6) continue;
    const cs = getComputedStyle(el);
    const hasText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    const replaced = /^(IMG|SVG|INPUT|SELECT|TEXTAREA|BUTTON|CANVAS|VIDEO|svg)$/.test(el.tagName);
    const boxed = (cs.backgroundColor !== 'rgba(0, 0, 0, 0)' && cs.backgroundColor !== 'transparent') || parseFloat(cs.borderTopWidth) > 0 || cs.boxShadow !== 'none';
    if (hasText || replaced || boxed) painted.push({ el, r, hasText, replaced, boxed, cs });
  }
  out.blocks = painted.map((p) => ({ x: p.r.left, y: p.r.top + scrollY, w: p.r.width, h: p.r.height }));

  // Overlap between leaf content (text/media/controls) that are not nested.
  const leaves = painted.filter((p) => (p.hasText || p.replaced) && p.cs.position !== 'fixed');
  for (let i = 0; i < leaves.length; i++) {
    for (let j = i + 1; j < leaves.length; j++) {
      const a = leaves[i];
      const b = leaves[j];
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      // Overlap designed inside one component master (e.g. an icon over a select) is intentional.
      const ra = a.el.closest('[data-dfw-component]');
      if (ra && ra === b.el.closest('[data-dfw-component]')) continue;
      const x = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const y = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (x > 2 && y > 2 && x * y > 0.15 * Math.min(a.r.width * a.r.height, b.r.width * b.r.height)) {
        if (out.overlaps.length < 20) out.overlaps.push({ a: label(a.el.closest('[data-dfw-id],[data-dfw-component]') || a.el), b: label(b.el.closest('[data-dfw-id],[data-dfw-component]') || b.el), area: Math.round(x * y) });
      }
    }
  }
  // Boxes: two component instances with a visible box (card, stat, banner, table...) that are not nested
  // must not intrude into each other, even if their texts do not touch yet.
  const boxes = painted.filter((p) => p.boxed && p.el.hasAttribute('data-dfw-component') && !/absolute|fixed/.test(p.cs.position));
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i];
      const b = boxes[j];
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      const x = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const y = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (x > 2 && y > 2 && out.overlaps.length < 20) out.overlaps.push({ a: label(a.el), b: label(b.el), area: Math.round(x * y), boxes: true });
    }
  }
  // Clipped text.
  for (const p of painted) {
    if (!p.hasText) continue;
    const cs = p.cs;
    const clipsX = /hidden|clip/.test(cs.overflowX) && p.el.scrollWidth > p.el.clientWidth + 1 && cs.textOverflow !== 'ellipsis';
    const clipsY = /hidden|clip/.test(cs.overflowY) && p.el.scrollHeight > p.el.clientHeight + 1 && cs.webkitLineClamp === 'none';
    if ((clipsX || clipsY) && out.clipped.length < 20) out.clipped.push({ el: label(p.el), text: p.el.textContent.trim().slice(0, 40) });
  }
  // Anchors must be fully visible in the first viewport.
  for (const el of document.querySelectorAll('[data-dfw-anchor]')) {
    if (!vis(el)) continue;
    const r = el.getBoundingClientRect();
    out.anchors.push({ name: el.getAttribute('data-dfw-anchor') || label(el), top: Math.round(r.top), bottom: Math.round(r.bottom), left: Math.round(r.left), right: Math.round(r.right), inView: r.top >= -1 && r.bottom <= vh + 1 && r.left >= -1 && r.right <= vw + 1 });
  }
  // Main content region (excluding navigation) for whitespace analysis.
  const main = document.querySelector('main, [role="main"]') || document.body;
  const mr = main.getBoundingClientRect();
  out.content = { x: Math.max(0, mr.left), w: Math.min(vw, mr.right) - Math.max(0, mr.left), top: Math.max(0, mr.top + scrollY), docH: document.documentElement.scrollHeight };
  // Component signatures for cross-screen consistency.
  for (const el of document.querySelectorAll('[data-dfw-component]')) {
    if (!vis(el)) continue;
    const cs = getComputedStyle(el);
    out.components.push({ c: el.getAttribute('data-dfw-component'), v: el.getAttribute('data-dfw-variant') || '', s: el.getAttribute('data-dfw-size') || '', sig: [cs.fontSize, cs.fontWeight, cs.letterSpacing, cs.paddingTop, cs.paddingLeft, cs.borderTopLeftRadius].join(' ') });
  }
  const firstBlock = main.querySelector('[data-dfw-id],[data-dfw-component]');
  out.contentLeft = firstBlock ? Math.round(firstBlock.getBoundingClientRect().left - mr.left) : null;
  return out;
}

/** Whitespace analysis on measured blocks (pure; unit-tested). */
export function findVoids(m, { minBand = 0.22 } = {}) {
  const res = [];
  if (!m.content || !m.blocks.length) return res;
  const { x, w, top } = m.content;
  const inMain = m.blocks.filter((b) => b.x + b.w > x + 4 && b.x < x + w - 4 && b.y + b.h > top);
  if (!inMain.length) return res;
  const bottom = Math.max(...inMain.map((b) => b.y + b.h));
  // Vertical coverage in 8px rows across the main region.
  const step = 8;
  const rows = Math.ceil((bottom - top) / step);
  const covered = new Array(rows).fill(false);
  for (const b of inMain) for (let r = Math.max(0, Math.floor((b.y - top) / step)); r < Math.min(rows, Math.ceil((b.y + b.h - top) / step)); r++) covered[r] = true;
  let run = 0;
  for (let r = 0; r <= rows; r++) {
    if (r < rows && !covered[r]) run++;
    else {
      if (run * step > m.vh * minBand) res.push({ kind: 'gap', from: Math.round(top + (r - run) * step), height: run * step });
      run = 0;
    }
  }
  // First-viewport fill: area of the main region covered by blocks.
  const cell = 24;
  const cols = Math.ceil(w / cell);
  const vrows = Math.ceil((m.vh - top) / cell);
  if (cols > 0 && vrows > 0) {
    const grid = new Uint8Array(cols * vrows);
    for (const b of inMain) {
      for (let r = Math.max(0, Math.floor((b.y - top) / cell)); r < Math.min(vrows, Math.ceil((b.y + b.h - top) / cell)); r++)
        for (let c = Math.max(0, Math.floor((b.x - x) / cell)); c < Math.min(cols, Math.ceil((b.x + b.w - x) / cell)); c++) grid[r * cols + c] = 1;
    }
    const fill = grid.reduce((a, v) => a + v, 0) / grid.length;
    // Empty band at the bottom of the first viewport while content ends early.
    const tail = m.vh - Math.min(m.vh, bottom);
    if (bottom < m.vh && tail > m.vh * 0.3) res.push({ kind: 'tail', height: Math.round(tail), fill: Math.round(fill * 100) });
    else if (fill < 0.35) res.push({ kind: 'sparse', fill: Math.round(fill * 100) });
  }
  return res;
}

function screenJobs(root, m, { screens, bps, states, stress }) {
  const jobs = [];
  for (const s of m.screens || []) {
    if (screens && !screens.includes(s.id)) continue;
    for (const v of s.variants || []) {
      const rel = `design/${v.file}`;
      for (const bp of bps) {
        for (const st of states(s)) {
          jobs.push({ screen: s.id, variant: v.id, rel, bp, state: st, stress: false });
          if (stress && st == null) jobs.push({ screen: s.id, variant: v.id, rel, bp, state: null, stress: true });
        }
      }
    }
  }
  return jobs;
}

/** Screens whose files (or the masters they use) changed since the last layout check. */
export function changedScreens(root) {
  const p = projectPaths(root);
  const cacheFile = path.join(p.dfw, 'layout-cache.json');
  const cache = readJson(cacheFile, {});
  const m = readJson(p.manifest, {});
  const mtime = (f) => (fs.existsSync(f) ? fs.statSync(f).mtimeMs : 0);
  const compDir = path.join(p.design, 'system', 'components');
  const changedComps = new Set();
  if (fs.existsSync(compDir)) for (const f of fs.readdirSync(compDir)) if (mtime(path.join(compDir, f)) > (cache[`c:${f}`] || 0)) changedComps.add(f.replace('.html', ''));
  const tokensChanged = mtime(path.join(p.tokens, 'tokens.json')) > (cache['t:tokens'] || 0);
  const out = [];
  for (const s of m.screens || []) {
    for (const v of s.variants || []) {
      const abs = path.join(p.design, v.file);
      if (!fs.existsSync(abs)) continue;
      const text = fs.readFileSync(abs, 'utf8');
      const uses = [...text.matchAll(/<dfw-use\b[^>]*\bc="([\w-]+)"/g)].map((x) => x[1]);
      if (tokensChanged || mtime(abs) > (cache[`s:${v.file}`] || 0) || uses.some((u) => changedComps.has(u))) out.push(s.id);
    }
  }
  return { screens: [...new Set(out)], changedComps: [...changedComps], tokensChanged };
}

export function markChecked(root) {
  const p = projectPaths(root);
  const cache = {};
  const compDir = path.join(p.design, 'system', 'components');
  const now = Date.now();
  if (fs.existsSync(compDir)) for (const f of fs.readdirSync(compDir)) cache[`c:${f}`] = now;
  cache['t:tokens'] = now;
  for (const s of readJson(p.manifest, {}).screens || []) for (const v of s.variants || []) cache[`s:${v.file}`] = now;
  writeJson(path.join(p.dfw, 'layout-cache.json'), cache);
}

/**
 * Run the rendered layout check. Returns findings in the dfw check format.
 * opts: { screens?: [ids], bp?: [ids], states?: true, stress?: true, system?: true }
 */
export async function layoutCheck(root, opts = {}) {
  if (!playwrightStatus().installed) return { status: 'unavailable', detail: 'Playwright not installed (dfw setup --playwright)', findings: [] };
  const p = projectPaths(root);
  const m = readJson(p.manifest, {});
  const config = loadConfig(root);
  const allBps = m.breakpoints || config.studio.breakpoints;
  const bps = opts.bp ? allBps.filter((b) => opts.bp.includes(b.id)) : allBps;
  buildSystem(root);
  const jobs = screenJobs(root, m, { screens: opts.screens, bps, states: (s) => (opts.states ? [null, ...(s.states || m.states || [])] : [null]), stress: opts.stress !== false });
  const files = new Map(buildComposed(root, [...new Set(jobs.map((j) => j.rel))]).map((r) => [r.rel, r.out]));
  if (opts.system !== false && loadComponents(root).size) {
    const sysOut = path.join(p.dfw, 'build', 'design', 'system', 'index.html');
    fs.mkdirSync(path.dirname(sysOut), { recursive: true });
    const html = composeHtml(root, renderSystemPage(root, { designBase: '../' }), { designBase: '../' }).html;
    // Inline local CSS like buildComposed does.
    const srcDir = path.join(p.design, 'system');
    fs.writeFileSync(sysOut, html.replace(/<link\b[^>]*rel=["']stylesheet["'][^>]*>/gi, (tag) => {
      const href = (/href=["']([^"']+)["']/.exec(tag) || [])[1];
      const f = href && !/^(https?:)?\/\//.test(href) ? path.resolve(srcDir, href) : null;
      return f && fs.existsSync(f) ? `<style>${fs.readFileSync(f, 'utf8')}</style>` : tag;
    }).replace(/<head([^>]*)>/i, `<head$1><base href="${toPosix(path.relative(path.dirname(sysOut), srcDir))}/">`));
    files.set('design/system/index.html', sysOut);
    const bp = bps.find((b) => b.width >= 1280) || bps[0];
    jobs.push({ screen: '_system', variant: 'page', rel: 'design/system/index.html', bp, state: null, stress: false, system: true });
  }
  const { chromium } = await loadPlaywright();
  const browser = await chromium.launch();
  const results = [];
  try {
    let next = 0;
    const worker = async () => {
      while (next < jobs.length) {
        const j = jobs[next++];
        const page = await browser.newPage({ viewport: { width: j.bp.width, height: j.bp.height } });
        try {
          await page.goto('file://' + files.get(j.rel), { waitUntil: 'load', timeout: 30000 });
          if (j.state) await page.evaluate((st) => document.documentElement.setAttribute('data-state', st), j.state);
          await page.evaluate(() => (document.fonts ? document.fonts.ready.then(() => true) : true));
          await page.waitForTimeout(60);
          results.push({ ...j, m: await page.evaluate(measure, { stress: j.stress }) });
        } catch (e) {
          results.push({ ...j, error: e.message });
        } finally {
          await page.close();
        }
      }
    };
    await Promise.all(Array.from({ length: Math.min(4, jobs.length) }, worker));
  } finally {
    await browser.close();
  }

  const findings = [];
  const where = (r) => `${r.screen === '_system' ? 'Design System page' : `${r.screen}/${r.variant}`} @ ${r.bp.id}${r.state ? ` [${r.state}]` : ''}${r.stress ? ' [stress: long lists]' : ''}`;
  const add = (r, level, rule, message) => findings.push({ level, rule, file: r.rel, line: 1, message: `${where(r)}: ${message}`, source: 'layout' });
  for (const r of results) {
    if (r.error) {
      add(r, 'warn', 'render', `could not render (${r.error.slice(0, 120)})`);
      continue;
    }
    const mm = r.m;
    if (mm.hOverflow > 1) add(r, 'error', 'h-overflow', `page is ${mm.hOverflow}px wider than the viewport.`);
    for (const o of mm.overlaps.slice(0, 5)) add(r, 'error', 'overlap', `"${o.a}" overlaps "${o.b}" (${o.area}px²).`);
    for (const a of mm.anchors) if (!a.inView && r.screen !== '_system') add(r, 'error', 'anchor', `anchored "${a.name}" is not fully visible (top ${a.top}, bottom ${a.bottom}, viewport ${mm.vh}). Anchored feedback and primary actions must stay on screen${r.stress ? ' even when lists grow: scroll the list inside its container' : ''}.`);
    for (const c of mm.clipped.slice(0, 3)) add(r, 'warn', 'clipped', `text clipped in "${c.el}" ("${c.text}…").`);
    if (!r.stress) {
      for (const v of findVoids(mm)) {
        if (v.kind === 'gap') add(r, 'warn', 'void', `empty band of ${v.height}px between content blocks (from y=${v.from}). Close it, or let the block above fill the space.`);
        else if (v.kind === 'tail' && r.screen !== '_system') add(r, 'warn', 'void', `content ends early: ${v.height}px empty at the bottom of the first screen (${v.fill}% of the view filled). Fill the height (task layouts) or tighten the layout.`);
        else if (v.kind === 'sparse') add(r, 'warn', 'void', `only ${v.fill}% of the first screen holds content.`);
      }
    }
  }
  // Cross-screen consistency.
  const sigs = new Map();
  for (const r of results.filter((x) => x.m && !x.stress && x.screen !== '_system')) {
    for (const c of r.m.components) {
      const key = `${c.c}/${c.v}/${c.s}@${r.bp.id}`;
      if (!sigs.has(key)) sigs.set(key, new Map());
      const bySig = sigs.get(key);
      if (!bySig.has(c.sig)) bySig.set(c.sig, new Set());
      bySig.get(c.sig).add(r.screen);
    }
  }
  for (const [key, bySig] of sigs) {
    if (bySig.size > 1) {
      const variants = [...bySig].map(([sig, scr]) => `${[...scr].join(', ')}: ${sig}`).join(' | ');
      findings.push({ level: 'warn', rule: 'consistency', file: 'design/design.json', line: 1, message: `Component ${key.split('@')[0]} renders differently across screens (${variants}). A screen is overriding the master; fix the master or the screen.`, source: 'layout' });
    }
  }
  return { status: 'ran', findings, renders: results.length };
}
