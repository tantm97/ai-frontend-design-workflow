// Penpot screens bridge (optional). dfw's HTML stays the source of truth.
//
// Export: every screen x variant x breakpoint is rendered in headless Chrome (lib/cdp.mjs) and read
// back as a layer tree (boxes, text, fills, borders, radii, shadows, component instances). The
// result (.dfw/penpot/export.json) is what the dfw Penpot plugin (studio/penpot/) turns into
// Penpot boards: one board per screen x breakpoint, one Penpot page per dfw page, layer names =
// data-dfw-id, text as text layers, colours linked to tokens by name, component masters as
// Penpot components with instances.
//
// Import: the plugin sends back what the designer changed (texts, boxes, fills, added layers,
// Penpot comments, token values). dfw never rewrites its HTML from that: the change set becomes
// feedback comments (schemas/feedback.schema.json, provider "penpot") and a token diff, applied
// by the agent through the normal review loop and gates.
import fs from 'node:fs';
import path from 'node:path';
import crypto from 'node:crypto';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { projectPaths, readJson, writeJson, nowIso, toPosix, DfwError } from './util.mjs';
import { loadConfig } from './config.mjs';
import { buildComposed, buildSystem } from './compose.mjs';
import { studioPages } from '../studio/pages.mjs';
import { flattenTokens, tokenToPenpot, parseColor, readDfwTokens, dfwToPenpot, importPenpotTokens, shortValue } from './penpot-tokens.mjs';
import { addComment } from './comments.mjs';

export const EXPORT_SCHEMA = 'dfw-penpot-export/1';
export const CHANGES_SCHEMA = 'dfw-penpot-changes/1';
const BOARD_GAP_X = 160;
const BOARD_GAP_Y = 240;

export function penpotDir(root) {
  return path.join(projectPaths(root).dfw, 'penpot');
}

/* ------------------------------------------------------------------ in-page snapshot */

// Runs inside the rendered page (self-contained: no closures). Returns a raw layer tree with
// page coordinates, colours normalised to { hex, opacity } and box-shadows parsed.
export function snapshotPage() {
  const round = (n) => Math.round(n * 100) / 100;
  const px = (v) => parseFloat(v) || 0;
  const cv = document.createElement('canvas');
  cv.width = cv.height = 1;
  const ctx = cv.getContext('2d', { willReadFrequently: true });
  const cache = new Map();
  const color = (c) => {
    if (!c || c === 'transparent') return null;
    if (cache.has(c)) return cache.get(c);
    let out = null;
    const m = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/.exec(c);
    let rgba;
    if (m) rgba = [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : +m[4]];
    else {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = '#000';
      ctx.fillStyle = c;
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      rgba = [d[0], d[1], d[2], d[3] / 255];
    }
    if (rgba[3] > 0) out = { hex: '#' + rgba.slice(0, 3).map((x) => Math.round(x).toString(16).padStart(2, '0')).join(''), opacity: round(rgba[3]) };
    cache.set(c, out);
    return out;
  };
  const splitTop = (s) => {
    const parts = [];
    let depth = 0;
    let cur = '';
    for (const ch of s) {
      if (ch === '(') depth++;
      if (ch === ')') depth--;
      if (ch === ',' && depth === 0) {
        parts.push(cur.trim());
        cur = '';
      } else cur += ch;
    }
    if (cur.trim()) parts.push(cur.trim());
    return parts;
  };
  const shadows = (s) => {
    if (!s || s === 'none') return null;
    return splitTop(s).map((part) => {
      const inset = /\binset\b/.test(part);
      const p = part.replace(/\binset\b/, '').trim();
      const cm = /([a-z-]+\([^()]*(?:\([^()]*\)[^()]*)*\)|#[0-9a-f]{3,8}\b)/i.exec(p);
      const nums = (cm ? p.replace(cm[0], '') : p).trim().split(/\s+/).filter(Boolean).map(px);
      return { inset, x: nums[0] || 0, y: nums[1] || 0, blur: nums[2] || 0, spread: nums[3] || 0, color: color(cm ? cm[0] : 'rgba(0,0,0,0.25)') };
    });
  };
  const visible = (cs) => cs.display !== 'none' && cs.visibility !== 'hidden' && Number(cs.opacity) > 0;
  const rect = (r) => ({ x: round(r.left + scrollX), y: round(r.top + scrollY), w: round(r.width), h: round(r.height) });
  const textStyle = (cs) => ({
    fontFamily: cs.fontFamily.split(',')[0].trim().replace(/^["']|["']$/g, ''),
    fontSize: px(cs.fontSize),
    fontWeight: String(cs.fontWeight),
    fontStyle: cs.fontStyle === 'italic' ? 'italic' : 'normal',
    lineHeight: cs.lineHeight === 'normal' ? 1.2 : round(px(cs.lineHeight) / px(cs.fontSize)),
    letterSpacing: cs.letterSpacing === 'normal' ? 0 : round(px(cs.letterSpacing)),
    color: color(cs.color),
    align: ['center', 'right', 'justify'].includes(cs.textAlign) ? cs.textAlign : cs.textAlign === 'end' ? 'right' : 'left',
    transform: ['uppercase', 'lowercase', 'capitalize'].includes(cs.textTransform) ? cs.textTransform : null,
    decoration: /underline/.test(cs.textDecorationLine) ? 'underline' : /line-through/.test(cs.textDecorationLine) ? 'line-through' : null,
  });
  const visual = (cs) => {
    const v = {};
    const bg = color(cs.backgroundColor);
    if (bg) v.fill = bg;
    if (cs.backgroundImage && cs.backgroundImage !== 'none') v.backgroundImage = cs.backgroundImage.slice(0, 160);
    const sides = ['Top', 'Right', 'Bottom', 'Left'].map((s) => {
      const w = px(cs[`border${s}Width`]);
      const c = color(cs[`border${s}Color`]);
      return w > 0 && c && !/none|hidden/.test(cs[`border${s}Style`]) ? { w, color: c, style: cs[`border${s}Style`] } : null;
    });
    if (sides.some(Boolean)) v.borders = sides;
    const radii = ['TopLeft', 'TopRight', 'BottomRight', 'BottomLeft'].map((c) => px(cs[`border${c}Radius`]));
    if (radii.some(Boolean)) v.radius = radii;
    const sh = shadows(cs.boxShadow);
    if (sh) v.shadows = sh;
    if (Number(cs.opacity) < 1) v.opacity = Number(cs.opacity);
    if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') v.clip = true;
    return v;
  };
  const hasVisual = (v) => Boolean(v.fill || v.borders || v.shadows || v.backgroundImage);
  const BLOCKERS = /^(svg|img|input|textarea|select|button|video|canvas|picture|iframe)$/i;
  const isTextLeaf = (el) => {
    if (!el.textContent.trim()) return false;
    for (const d of el.querySelectorAll('*')) {
      const c = getComputedStyle(d);
      if (c.display === 'none') continue;
      if (!/^(inline|contents)$/.test(c.display) || BLOCKERS.test(d.tagName) || d.hasAttribute('data-dfw-id') || d.hasAttribute('data-dfw-component')) return false;
      if (hasVisual(visual(c))) return false;
    }
    return true;
  };
  const contentRect = (el) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    return rect(r.getBoundingClientRect());
  };
  const label = (el) => {
    const comp = el.getAttribute('data-dfw-component');
    if (comp) return [comp, el.getAttribute('data-dfw-variant'), el.getAttribute('data-dfw-size')].filter(Boolean).join('/');
    const cls = typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/)[0] : '';
    return el.tagName.toLowerCase() + cls;
  };
  const textNode = (characters, r, cs, anchor, name) => ({ kind: 'text', name: name || 'text', dfwId: null, anchor, ...r, characters, text: textStyle(cs) });

  const walk = (el, anchor) => {
    const cs = getComputedStyle(el);
    if (!visible(cs) || el.hasAttribute('data-dfw-overlay')) return null;
    const tag = el.tagName.toLowerCase();
    if (/^(script|style|template|noscript|link|meta|head)$/.test(tag)) return null;
    const id = el.getAttribute('data-dfw-id');
    const a = id || anchor || null;
    const comp = el.getAttribute('data-dfw-component');
    const node = { kind: 'frame', name: id || label(el), tag, dfwId: id || null, anchor: a, ...rect(el.getBoundingClientRect()), style: visual(cs), children: [] };
    if (comp) node.component = { name: comp, variant: el.getAttribute('data-dfw-variant') || null, size: el.getAttribute('data-dfw-size') || null };
    if (tag === 'svg') {
      const clone = el.cloneNode(true);
      clone.setAttribute('width', String(node.w));
      clone.setAttribute('height', String(node.h));
      if (!clone.getAttribute('xmlns')) clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
      const c = color(cs.color);
      return { ...node, kind: 'svg', svg: clone.outerHTML.replace(/currentColor/g, c ? c.hex : '#000000'), icon: el.getAttribute('data-dfw-icon') || null, style: {}, children: undefined };
    }
    if (tag === 'img') return { ...node, kind: 'image', src: el.currentSrc || el.src || null, alt: el.alt || '', children: undefined };
    if (/^(input|textarea|select)$/.test(tag)) {
      const value = tag === 'select' ? el.options[el.selectedIndex]?.text || '' : el.type === 'checkbox' || el.type === 'radio' ? '' : el.value || el.placeholder || '';
      if (value.trim()) {
        const pad = { l: px(cs.paddingLeft) + px(cs.borderLeftWidth), t: px(cs.borderTopWidth) };
        const t = textNode(value.trim(), { x: node.x + pad.l, y: node.y + pad.t, w: Math.max(1, node.w - pad.l - px(cs.paddingRight)), h: Math.max(1, node.h - 2 * pad.t) }, cs, a, 'value');
        t.placeholder = !el.value;
        node.children.push(t);
      }
      return node;
    }
    if (isTextLeaf(el)) {
      const t = textNode(el.innerText.trim(), contentRect(el), cs, a, null);
      if (hasVisual(node.style) || comp || node.style.clip || node.style.opacity !== undefined) {
        node.children.push(t);
        return node;
      }
      // A plain text element: the text layer itself carries the element's id and name.
      return { ...t, name: id || 'text', dfwId: id || null, tag };
    }
    for (const child of el.childNodes) {
      if (child.nodeType === 3 && child.textContent.trim()) {
        const r = document.createRange();
        r.selectNodeContents(child);
        node.children.push(textNode(child.textContent.trim().replace(/\s+/g, ' '), rect(r.getBoundingClientRect()), cs, a, null));
      } else if (child.nodeType === 1) {
        const c = walk(child, a);
        if (c) node.children.push(c);
      }
    }
    return node;
  };
  const html = getComputedStyle(document.documentElement);
  const body = getComputedStyle(document.body);
  const bg = color(body.backgroundColor) || color(html.backgroundColor) || { hex: '#ffffff', opacity: 1 };
  const root = walk(document.body, null) || { children: [] };
  return {
    width: Math.max(innerWidth, document.documentElement.scrollWidth),
    height: Math.max(innerHeight, document.documentElement.scrollHeight),
    background: bg,
    children: root.children,
    title: document.title,
  };
}

/* ------------------------------------------------------------------ normalising snapshots */

const isWrapper = (n) => n.kind === 'frame' && !n.dfwId && !n.component && !n.style?.fill && !n.style?.borders && !n.style?.shadows && !n.style?.clip && n.style?.opacity === undefined && !n.style?.backgroundImage;

/** Hoist plain layout wrappers (no id, no component, nothing painted) and drop empty frames. */
export function simplify(nodes) {
  const out = [];
  for (const n of nodes || []) {
    if (n.children) n.children = simplify(n.children);
    if (isWrapper(n)) out.push(...(n.children || []));
    else if (n.kind === 'frame' && !n.children?.length && !n.dfwId && !n.component && !n.style?.fill && !n.style?.borders && !n.style?.shadows) continue;
    else out.push(n);
  }
  return out;
}

/** Colour tokens resolved to { hex, opacity } (aliases followed), most semantic name first. */
export function colorTokenIndex(tree) {
  const flat = flattenTokens(tree || {});
  const resolve = (name, depth = 0) => {
    const t = flat.get(name);
    if (!t || depth > 10) return null;
    const m = typeof t.value === 'string' && /^\{([^{}]+)\}$/.exec(t.value.trim());
    return m ? resolve(m[1], depth + 1) : t;
  };
  const out = [];
  for (const [name, t] of flat) {
    if (t.type !== 'color') continue;
    const r = resolve(name);
    if (!r) continue;
    const p = tokenToPenpot(name, r);
    const c = p.unsupported ? null : parseColor(p.value);
    if (!c) continue;
    const alias = typeof t.value === 'string';
    // Prefer semantic names (aliases, no numeric scale step) over palette steps.
    const score = (alias ? 0 : 2) + (/\.\d+$/.test(name) ? 4 : 0) + name.split('.').length * 0.1;
    out.push({ name, hex: c.hex, opacity: c.alpha ?? 1, score });
  }
  return out.sort((a, b) => a.score - b.score);
}

export function radiusTokenIndex(tree) {
  const out = [];
  for (const [name, t] of flattenTokens(tree || {})) {
    if (t.type === 'dimension' && /(^|\.)radius(\.|$)/.test(name) && t.value?.unit === 'px') out.push({ name, px: t.value.value });
  }
  return out;
}

const TEXTISH = /(^|[.-])(ink|text|on|fg|foreground|muted|subtle)([.-]|$)/;
/** Token with exactly this colour; text colours prefer ink/text/on-* names, fills the others. */
export function findColorToken(idx, c, role = 'fill') {
  if (!c) return null;
  const hits = idx.filter((t) => t.hex === c.hex && Math.abs(t.opacity - c.opacity) < 0.01);
  const pref = hits.find((t) => (role === 'text') === TEXTISH.test(t.name));
  return (pref || hits[0])?.name || null;
}

/** Turn raw snapshot nodes into export layers: board-relative boxes, fills/strokes with token links, paths. */
export function toLayers(nodes, { colors = [], radii = [], projectRoot = null } = {}) {
  const layer = (n, p) => {
    const st = n.style || {};
    const l = { kind: n.kind, name: n.name, path: p, dfwId: n.dfwId || null, anchor: n.anchor || null, x: n.x, y: n.y, w: n.w, h: n.h };
    if (n.tag) l.tag = n.tag;
    if (st.fill) l.fills = [{ color: st.fill.hex, opacity: st.fill.opacity, token: findColorToken(colors, st.fill) }];
    if (st.borders) {
      const [t, r, b, lft] = st.borders;
      const uniform = st.borders.every((x) => x && x.w === t.w && x.color.hex === t.color.hex);
      if (uniform) l.strokes = [{ color: t.color.hex, opacity: t.color.opacity, width: t.w, style: t.style === 'dashed' || t.style === 'dotted' ? t.style : 'solid', token: findColorToken(colors, t.color, 'stroke') }];
      else l.sideBorders = { top: t, right: r, bottom: b, left: lft }; // Penpot strokes are uniform: drawn as thin rectangles
    }
    if (st.radius) {
      l.radius = st.radius;
      const tok = st.radius.every((x) => x === st.radius[0]) ? radii.find((x) => x.px === st.radius[0])?.name : null;
      if (tok) l.radiusToken = tok;
    }
    if (st.shadows) l.shadows = st.shadows.map((s) => ({ ...s, color: s.color || { hex: '#000000', opacity: 0.25 } }));
    if (st.opacity !== undefined) l.opacity = st.opacity;
    if (st.clip) l.clip = true;
    if (st.backgroundImage) l.note = `background-image not exported (${st.backgroundImage.slice(0, 60)})`;
    if (n.kind === 'text') {
      l.characters = n.characters;
      l.text = { ...n.text, color: n.text.color, colorToken: findColorToken(colors, n.text.color, 'text') };
      if (n.placeholder) l.placeholder = true;
    }
    if (n.kind === 'svg') {
      l.svg = n.svg;
      if (n.icon) l.icon = n.icon;
    }
    if (n.kind === 'image') {
      l.alt = n.alt;
      l.src = null;
      if (n.src && projectRoot && n.src.startsWith('file:')) {
        const rel = toPosix(path.relative(projectPaths(projectRoot).design, fileURLToPath(n.src)));
        if (!rel.startsWith('..')) l.src = `/design/${rel}`;
      } else if (n.src && /^https?:/.test(n.src)) l.src = n.src;
    }
    if (n.component) l.component = { ...n.component, key: componentKey(n.component) };
    if (n.children?.length) l.children = n.children.map((c, i) => layer(c, p === '' ? String(i) : `${p}.${i}`));
    if (l.children || l.kind === 'frame') l.sig = signature(l);
    return l;
  };
  return nodes.map((n, i) => layer(n, String(i)));
}

export const componentKey = (c) => [c.name, c.variant || '', c.size || ''].join('|');

/** Structural signature (kinds, names, child structure; not text, positions or colours). */
export function signature(l) {
  return `${l.kind}:${l.component ? l.component.key : l.name}[${(l.children || []).map((c) => (c.sig !== undefined ? c.sig : `${c.kind}:${c.name}`)).join(',')}]`;
}

/** Rebase a subtree to its own origin (component main). */
function rebase(l, ox, oy) {
  const c = { ...l, x: Math.round((l.x - ox) * 100) / 100, y: Math.round((l.y - oy) * 100) / 100 };
  if (l.children) c.children = l.children.map((ch) => rebase(ch, ox, oy));
  return c;
}

/**
 * Build the export document from rendered snapshots.
 * renders: [{ page: {id,title}, screen, variant, breakpoint: {id,width,height}, file, snapshot }]
 */
export function buildExport({ renders, tokens, product = '', designVersion = 'v1', projectRoot = null, generatedAt = nowIso() }) {
  const colors = colorTokenIndex(tokens?.base);
  const radii = radiusTokenIndex(tokens?.base);
  const components = new Map();
  const pages = new Map();
  for (const r of renders) {
    if (!pages.has(r.page.id)) pages.set(r.page.id, { id: r.page.id, title: r.page.title || r.page.id, boards: [] });
    const layers = toLayers(simplify(r.snapshot.children), { colors, radii, projectRoot });
    const board = {
      key: `${r.screen}|${r.variant}|${r.breakpoint.id}`,
      name: `${r.screen} / ${r.variant} @ ${r.breakpoint.id}`,
      screen: r.screen,
      variant: r.variant,
      breakpoint: r.breakpoint.id,
      file: r.file,
      width: r.breakpoint.width,
      height: Math.max(r.breakpoint.height, Math.ceil(r.snapshot.height)),
      viewportHeight: r.breakpoint.height,
      background: { color: r.snapshot.background.hex, opacity: r.snapshot.background.opacity, token: findColorToken(colors, r.snapshot.background) },
      layers,
    };
    const visit = (l) => {
      if (l.component && !components.has(l.component.key)) {
        components.set(l.component.key, { key: l.component.key, name: [l.component.name, l.component.variant, l.component.size].filter(Boolean).join(' / '), component: l.component.name, variant: l.component.variant, size: l.component.size, sig: l.sig, from: `${board.key}#${l.path}`, root: rebase(l, l.x, l.y) });
      }
      for (const c of l.children || []) visit(c);
    };
    layers.forEach(visit);
    pages.get(r.page.id).boards.push(board);
  }
  // Canvas layout per page: one row per breakpoint, screens/variants left to right.
  for (const pg of pages.values()) {
    const rows = [...new Set(pg.boards.map((b) => b.breakpoint))];
    let y = 0;
    for (const bp of rows) {
      let x = 0;
      let rowH = 0;
      for (const b of pg.boards.filter((bb) => bb.breakpoint === bp)) {
        b.x = x;
        b.y = y;
        x += b.width + BOARD_GAP_X;
        rowH = Math.max(rowH, b.height);
      }
      y += rowH + BOARD_GAP_Y;
    }
  }
  const tokenFile = tokens ? dfwToPenpot(tokens) : null;
  const id = `px_${generatedAt.replace(/[-:.TZ]/g, '').slice(0, 14)}_${crypto.createHash('sha256').update(JSON.stringify([...pages.values()])).digest('hex').slice(0, 8)}`;
  return {
    schema: EXPORT_SCHEMA,
    id,
    generatedAt,
    product,
    designVersion,
    tokens: tokenFile?.json || null,
    tokenReport: tokenFile ? { unsupported: tokenFile.report.unsupported, lossy: tokenFile.report.lossy } : null,
    colors: colors.map(({ name, hex, opacity }) => ({ name, hex, opacity })),
    components: [...components.values()],
    pages: [...pages.values()],
  };
}

/* ------------------------------------------------------------------ export (render + write) */

export function selectRenders(root, { page = null, bp = null } = {}) {
  const p = projectPaths(root);
  const m = readJson(p.manifest, null);
  if (!m) throw new DfwError('design/design.json not found.');
  const allBps = m.breakpoints || loadConfig(root).studio.breakpoints;
  const bps = bp ? allBps.filter((b) => String(bp).split(',').includes(b.id)) : allBps;
  if (!bps.length) throw new DfwError(`No breakpoint matches --bp ${bp} (have: ${allBps.map((b) => b.id).join(', ')}).`);
  const byId = new Map((m.screens || []).map((s) => [s.id, s]));
  const pages = studioPages(m).filter((pg) => !pg.system && !pg.flow && (!page || pg.id === page));
  if (page && !pages.length) throw new DfwError(`No page "${page}" in design/design.json.`);
  const jobs = [];
  for (const pg of pages) {
    for (const sid of pg.screens) {
      const s = byId.get(sid);
      for (const v of s?.variants || []) {
        for (const b of bps) jobs.push({ page: { id: pg.id, title: pg.title }, screen: s.id, variant: v.id, breakpoint: b, file: `design/${v.file}` });
      }
    }
  }
  return { manifest: m, jobs };
}

/** Render and write .dfw/penpot/export.json (+ exports/<id>.json, tokens.penpot.json). */
export async function exportPenpot(root, { page = null, bp = null, out = null } = {}) {
  const { openRenderer, fontsReady, browserStatus } = await import('./runtime.mjs');
  if (!browserStatus().installed) throw new DfwError('dfw penpot export renders in Chrome/Edge/Chromium: none found (dfw setup --browser).');
  const { manifest, jobs } = selectRenders(root, { page, bp });
  if (!jobs.length) throw new DfwError('No screens to export (check --page/--bp and design/design.json).');
  buildSystem(root);
  const files = new Map(buildComposed(root, [...new Set(jobs.map((j) => j.file))]).map((r) => [r.rel, r.out]));
  const renderer = await openRenderer();
  const renders = [];
  const errors = [];
  try {
    let next = 0;
    const worker = async () => {
      while (next < jobs.length) {
        const j = jobs[next++];
        let pg = null;
        try {
          pg = await renderer.page({ width: j.breakpoint.width, height: j.breakpoint.height });
          await pg.goto(pathToFileURL(files.get(j.file)).href, { timeout: 30000 });
          await fontsReady(pg);
          await pg.waitForTimeout(40);
          renders.push({ ...j, snapshot: await pg.evaluate(snapshotPage) });
        } catch (e) {
          errors.push(`${j.screen}/${j.variant}@${j.breakpoint.id}: ${e.message}`);
        } finally {
          await pg?.close();
        }
      }
    };
    await Promise.all(Array.from({ length: Math.min(4, jobs.length) }, worker));
  } finally {
    await renderer.close();
  }
  const order = (r) => jobs.findIndex((j) => j.screen === r.screen && j.variant === r.variant && j.breakpoint.id === r.breakpoint.id);
  renders.sort((a, b) => order(a) - order(b));
  let tokens = null;
  try {
    tokens = readDfwTokens(root);
  } catch {}
  const state = readJson(projectPaths(root).state, {});
  const doc = buildExport({ renders, tokens, product: manifest.product || '', designVersion: `v${state.design?.version ?? 1}`, projectRoot: root });
  doc.errors = errors;
  const dir = penpotDir(root);
  writeJson(path.join(dir, 'exports', `${doc.id}.json`), doc);
  const file = path.resolve(root, out || path.join('.dfw', 'penpot', 'export.json'));
  writeJson(file, doc);
  if (doc.tokens) writeJson(path.join(dir, 'tokens.penpot.json'), doc.tokens);
  return { file, doc };
}

/** Geometry-free outline of an export (stable across platforms and fonts): used by tests and --outline. */
export function exportStructure(doc) {
  const lines = [];
  const row = (l, ind) => {
    const bits = [l.kind, l.name, `path=${l.path}`];
    if (l.anchor && l.anchor !== l.name) bits.push(`anchor=${l.anchor}`);
    if (l.component) bits.push(`component=${l.component.key}`);
    if (l.characters) bits.push(JSON.stringify(l.characters));
    if (l.fills?.[0]) bits.push(`fill=${l.fills[0].token || l.fills[0].color}`);
    if (l.strokes?.[0]) bits.push(`stroke=${l.strokes[0].token || l.strokes[0].color}`);
    if (l.sideBorders) bits.push(`borders=${Object.entries(l.sideBorders).filter(([, v]) => v).map(([k]) => k).join('+')}`);
    if (l.radiusToken) bits.push(`radius=${l.radiusToken}`);
    if (l.text) bits.push(`color=${l.text.colorToken || l.text.color?.hex}`);
    lines.push(ind + bits.join(' '));
    for (const c of l.children || []) row(c, ind + '  ');
  };
  for (const p of doc.pages) {
    lines.push(`page ${p.id} "${p.title}"`);
    for (const b of p.boards) {
      lines.push(`  board "${b.name}" key=${b.key} ${b.width}w background=${b.background.token || b.background.color}`);
      for (const l of b.layers) row(l, '    ');
    }
  }
  for (const c of doc.components) lines.push(`component "${c.name}" key=${c.key} sig=${c.sig}`);
  return lines;
}

export function exportSummary(doc, file, root) {
  const boards = doc.pages.reduce((n, p) => n + p.boards.length, 0);
  let layers = 0;
  const count = (l) => {
    layers++;
    (l.children || []).forEach(count);
  };
  doc.pages.forEach((p) => p.boards.forEach((b) => b.layers.forEach(count)));
  const lines = [
    `Wrote ${path.relative(root, file)} (${doc.id}): ${doc.pages.length} page(s), ${boards} board(s), ${layers} layer(s), ${doc.components.length} component(s).`,
    doc.tokens ? 'Tokens for Penpot: .dfw/penpot/tokens.penpot.json (Tokens panel > Tools > Import, before building the boards, so colours link to tokens).' : 'No design/tokens/tokens.json: colours are not linked to tokens.',
  ];
  for (const e of doc.errors || []) lines.push(`Render failed: ${e}`);
  lines.push('In Penpot: start the Studio (dfw studio start), open Plugins (Ctrl+Alt+P), install the manifest from "dfw penpot plugin", run "dfw bridge" > Build boards.');
  return lines.join('\n');
}

/* ------------------------------------------------------------------ import (change set) */

const norm = (s) => String(s ?? '').replace(/\s+/g, ' ').trim();

export function flattenLayers(layers, out = new Map()) {
  for (const l of layers || []) {
    out.set(l.path, l);
    flattenLayers(l.children, out);
  }
  return out;
}

function deepestAt(flat, x, y) {
  let best = null;
  for (const l of flat.values()) {
    if (!(l.anchor || l.dfwId)) continue;
    if (x >= l.x && x <= l.x + l.w && y >= l.y && y <= l.y + l.h && (!best || l.w * l.h <= best.w * best.h)) best = l;
  }
  return best;
}

const TOL = 1.5;

/**
 * Compare what the plugin sent back with the export it was built from.
 * Returns { changes: [...], warnings: [...] }. Pure (no IO).
 */
export function diffChanges(baseline, changes) {
  if (changes?.schema !== CHANGES_SCHEMA) throw new DfwError(`Not a dfw Penpot change file (schema "${changes?.schema}", expected ${CHANGES_SCHEMA}).`);
  const boards = new Map(baseline.pages.flatMap((p) => p.boards).map((b) => [b.key, b]));
  const out = [];
  const warnings = [];
  const seenBoards = new Set();
  const where = (b) => ({ board: b.key, screen: b.screen, variant: b.variant, breakpoint: b.breakpoint, file: b.file });
  for (const cb of changes.boards || []) {
    const b = boards.get(cb.key);
    if (!b) {
      warnings.push(`Board "${cb.name || cb.key}" is not in export ${baseline.id}: skipped.`);
      continue;
    }
    seenBoards.add(b.key);
    const flat = flattenLayers(b.layers);
    const present = new Set();
    const deltas = new Map();
    for (const n of cb.layers || []) {
      const l = flat.get(n.path);
      if (l) deltas.set(n.path, { x: n.x - l.x, y: n.y - l.y });
    }
    // A layer that moved exactly like its parent was not moved itself.
    const ownMove = (p, d) => {
      const parent = p.includes('.') ? deltas.get(p.slice(0, p.lastIndexOf('.'))) : null;
      return !(parent && Math.abs(parent.x - d.x) <= TOL && Math.abs(parent.y - d.y) <= TOL);
    };
    for (const n of cb.layers || []) {
      const l = flat.get(n.path);
      if (!l) {
        warnings.push(`${b.name}: layer path ${n.path} unknown in the export; treated as added.`);
        (cb.added ||= []).push(n);
        continue;
      }
      present.add(n.path);
      const target = { anchor: l.anchor, dfwId: l.dfwId, path: l.path, name: l.name, bbox: [l.x, l.y, l.w, l.h], kind: l.kind };
      if (l.kind === 'text' && n.characters !== undefined && norm(n.characters) !== norm(l.characters)) {
        out.push({ type: 'text', ...where(b), ...target, from: l.characters, to: n.characters, placeholder: Boolean(l.placeholder) });
      }
      const d = { x: n.x - l.x, y: n.y - l.y, w: n.w - l.w, h: n.h - l.h };
      const moved = (Math.abs(d.x) > TOL || Math.abs(d.y) > TOL) && ownMove(n.path, d);
      // Text boxes follow Penpot's font metrics (auto width/height): their size is not a design change.
      const resized = l.kind !== 'text' && (Math.abs(d.w) > TOL || Math.abs(d.h) > TOL);
      if (moved || resized) out.push({ type: resized ? 'resize' : 'move', ...where(b), ...target, from: [l.x, l.y, l.w, l.h], to: [n.x, n.y, n.w, n.h], delta: d });
      const before = l.kind === 'text' ? l.text?.color : l.fills?.[0];
      const after = n.fills?.[0];
      if (after && before && (String(after.color).toLowerCase() !== String(before.hex || before.color).toLowerCase() || Math.abs((after.opacity ?? 1) - (before.opacity ?? 1)) > 0.01)) {
        const tokenNow = baseline.colors.find((c) => c.hex === String(after.color).toLowerCase() && Math.abs(c.opacity - (after.opacity ?? 1)) < 0.01)?.name || null;
        out.push({ type: 'fill', ...where(b), ...target, from: { color: before.hex || before.color, opacity: before.opacity ?? 1, token: l.kind === 'text' ? l.text?.colorToken : l.fills?.[0]?.token }, to: { color: String(after.color).toLowerCase(), opacity: after.opacity ?? 1, token: tokenNow } });
      } else if (after && !before && l.kind !== 'text') {
        out.push({ type: 'fill', ...where(b), ...target, from: null, to: { color: String(after.color).toLowerCase(), opacity: after.opacity ?? 1 } });
      }
      if (n.name && n.name !== l.name && l.dfwId) out.push({ type: 'rename', ...where(b), ...target, from: l.name, to: n.name });
    }
    // Removed: topmost layers of the export missing from the board.
    for (const l of flat.values()) {
      if (present.has(l.path)) continue;
      const parent = l.path.includes('.') ? l.path.slice(0, l.path.lastIndexOf('.')) : null;
      if (parent && !present.has(parent) && flat.has(parent)) continue;
      out.push({ type: 'removed', ...where(b), anchor: l.anchor, dfwId: l.dfwId, path: l.path, name: l.name, bbox: [l.x, l.y, l.w, l.h], kind: l.kind, text: l.characters });
    }
    for (const a of cb.added || []) {
      const host = deepestAt(flat, a.x + (a.w || 0) / 2, a.y + (a.h || 0) / 2);
      out.push({ type: 'added', ...where(b), anchor: host?.anchor || host?.dfwId || null, name: a.name, kind: a.kind || a.type, bbox: [a.x, a.y, a.w, a.h], characters: a.characters });
    }
  }
  for (const key of changes.missingBoards || []) if (boards.has(key)) out.push({ type: 'board-removed', ...where(boards.get(key)), anchor: null });
  // Comments sit on the board as the designer sees it: resolve them against the pushed (current) boxes.
  const current = new Map();
  for (const cb of changes.boards || []) {
    const b = boards.get(cb.key);
    if (!b) continue;
    const flat = flattenLayers(b.layers);
    const m = new Map();
    for (const n of cb.layers || []) if (flat.has(n.path)) m.set(n.path, { ...flat.get(n.path), x: n.x, y: n.y, w: n.w, h: n.h });
    current.set(cb.key, m);
  }
  for (const t of changes.comments || []) {
    const b = t.boardKey ? boards.get(t.boardKey) : null;
    const host = b ? deepestAt(current.get(b.key) || flattenLayers(b.layers), t.x, t.y) : null;
    out.push({
      type: 'comment',
      ...(b ? where(b) : { board: null, screen: null, variant: null, breakpoint: null, file: null }),
      anchor: host?.anchor || host?.dfwId || null,
      bbox: host ? [host.x, host.y, host.w, host.h] : null,
      thread: t.seq ?? null,
      resolved: Boolean(t.resolved),
      messages: (t.comments || []).map((c) => ({ author: c.user || 'designer', content: c.content, at: c.date || null })),
    });
  }
  return { changes: out, warnings };
}

const fmtColor = (c) => (c ? `${c.color}${c.opacity !== undefined && c.opacity < 1 ? ` @${Math.round(c.opacity * 100)}%` : ''}${c.token ? ` (${c.token})` : ''}` : 'none');

export function describeChange(c) {
  switch (c.type) {
    case 'text':
      return `${c.placeholder ? 'Placeholder' : 'Text'} changed: "${norm(c.from)}" -> "${norm(c.to)}"`;
    case 'move':
      return `Moved by ${Math.round(c.delta.x)}, ${Math.round(c.delta.y)} px (now x ${Math.round(c.to[0])}, y ${Math.round(c.to[1])})`;
    case 'resize':
      return `Resized ${Math.round(c.from[2])}x${Math.round(c.from[3])} -> ${Math.round(c.to[2])}x${Math.round(c.to[3])} px${Math.abs(c.delta.x) > TOL || Math.abs(c.delta.y) > TOL ? `, moved by ${Math.round(c.delta.x)}, ${Math.round(c.delta.y)}` : ''}`;
    case 'fill':
      return `Colour changed: ${fmtColor(c.from)} -> ${fmtColor(c.to)}${c.to && !c.to.token ? ' (no token has this value: use a token or propose one)' : ''}`;
    case 'rename':
      return `Layer renamed "${c.from}" -> "${c.to}" (data-dfw-id stays; rename only if intended)`;
    case 'removed':
      return `Removed in Penpot: ${c.kind} "${c.name}"${c.text ? ` ("${norm(c.text).slice(0, 60)}")` : ''}`;
    case 'added':
      return `Added in Penpot: ${c.kind} "${c.name}" at ${c.bbox.map((v) => Math.round(v || 0)).join(', ')}${c.characters ? ` with text "${norm(c.characters).slice(0, 80)}"` : ''}`;
    case 'board-removed':
      return 'Board deleted in Penpot';
    case 'comment':
      return c.messages.map((m) => `${m.author}: ${m.content}`).join('\n');
    default:
      return c.type;
  }
}

/** design/<file>:<line> of the element (data-dfw-id) or instance (<dfw-use id>) with this id. */
export function sourceLine(root, file, id) {
  if (!file) return null;
  const abs = path.join(root, file);
  if (!id || !fs.existsSync(abs)) return file;
  const text = fs.readFileSync(abs, 'utf8');
  const esc = id.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const m = new RegExp(`(?:data-dfw-id|<dfw-use\\b[^>]*\\bid)="${esc}"`).exec(text);
  return m ? `${file}:${text.slice(0, m.index).split('\n').length}` : file;
}

const fingerprint = (x) => crypto.createHash('sha256').update(JSON.stringify(x)).digest('hex').slice(0, 16);

/** Penpot token sets sent by the plugin -> a Penpot token file (same shape as the Tokens panel export). */
export function tokensFromPlugin(t) {
  if (!t?.sets?.length) return null;
  const doc = {};
  for (const s of t.sets) {
    const set = (doc[s.name] = {});
    for (const tok of s.tokens || []) {
      const parts = String(tok.name).split('.');
      let node = set;
      for (const p of parts.slice(0, -1)) node = node[p] ??= {};
      node[parts.at(-1)] = { $value: tok.value, $type: tok.type, ...(tok.description ? { $description: tok.description } : {}) };
    }
  }
  doc.$themes = (t.themes || []).map((th) => ({ name: th.name, group: th.group || '', description: th.description || '', isSource: false, selectedTokenSets: Object.fromEntries((th.sets || []).map((s) => [s, 'enabled'])) }));
  doc.$metadata = { tokenSetOrder: t.sets.map((s) => s.name), activeSets: t.sets.filter((s) => s.active).map((s) => s.name) };
  return doc;
}

/**
 * Import a change file: write feedback comments (provider "penpot") and the token diff.
 * Never edits design/. Returns { baseline, changes, comments, tokenDiff, tokenFile, warnings, skipped }.
 */
export function importChanges(root, changes, { dryRun = false, state = null } = {}) {
  const dir = penpotDir(root);
  const exportId = changes?.exportId;
  const baseline = (exportId && readJson(path.join(dir, 'exports', `${exportId}.json`), null)) || readJson(path.join(dir, 'export.json'), null);
  if (!baseline) throw new DfwError('No Penpot export found (.dfw/penpot/export.json). Run "dfw penpot export" first; the change file must come from boards built from it.');
  const { changes: list, warnings } = diffChanges(baseline, changes);
  if (exportId && baseline.id !== exportId) warnings.push(`Change file refers to export ${exportId}, which is not on disk; compared with the latest export ${baseline.id}.`);
  const frozen = Boolean(state?.design?.locked);
  const result = { baseline: baseline.id, changes: list, comments: [], skipped: 0, warnings, tokenDiff: null, tokenFile: null, frozen };

  // Token diff (dry run against design/tokens; never applied here).
  const tdoc = tokensFromPlugin(changes.tokens) || (changes.tokenFile && typeof changes.tokenFile === 'object' ? changes.tokenFile : null);
  if (tdoc) {
    const f = path.join(dir, 'tokens.from-penpot.json');
    if (!dryRun) writeJson(f, tdoc);
    try {
      const tmp = dryRun ? path.join(dir, `.tokens-${process.pid}.json`) : f;
      if (dryRun) writeJson(tmp, tdoc);
      const r = importPenpotTokens(root, tmp, { dryRun: true });
      if (dryRun) fs.rmSync(tmp, { force: true });
      result.tokenDiff = r;
      result.tokenFile = path.relative(root, f);
    } catch (e) {
      warnings.push(`Token diff skipped: ${e.message}`);
    }
  }

  // Group design changes per element (board + anchor) into one comment each; Penpot comments stay separate.
  const groups = new Map();
  for (const c of list) {
    const key = c.type === 'comment' ? `comment|${c.board}|${c.thread}|${c.messages.length}` : `${c.board}|${c.anchor || c.path || ''}`;
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(c);
  }
  const ledgerFile = path.join(dir, 'imported.json');
  const ledger = new Set(readJson(ledgerFile, []));
  const gateNote = frozen ? `\n(Design v${state.design.version} is approved and frozen: discuss with the user; applying this needs "dfw reopen".)` : '';
  const pending = [];
  for (const [key, cs] of groups) {
    const c0 = cs[0];
    const fp = fingerprint(cs.map(({ type, board, path: p, anchor, from, to, messages, characters, bbox }) => ({ type, board, p, anchor, from, to, messages, characters, bbox })));
    if (ledger.has(fp)) {
      result.skipped++;
      continue;
    }
    if (c0.type === 'comment' && c0.resolved) {
      result.skipped++;
      continue;
    }
    const header = c0.type === 'comment' ? '[Penpot comment]' : `[Penpot] ${c0.anchor ? `"${c0.anchor}"` : c0.name ? `layer "${c0.name}"` : 'board'}:`;
    const body = c0.type === 'comment' ? describeChange(c0) : cs.map((c) => `- ${describeChange(c)}`).join('\n');
    const input = {
      comment: `${header}\n${body}${gateNote}`,
      screen: c0.screen,
      variant: c0.variant,
      breakpoint: c0.breakpoint,
      file: c0.file ? c0.file.replace(/^design\//, '') : null,
      provider: 'penpot',
      target: {
        dfwId: c0.anchor || null,
        source: sourceLine(root, c0.file, c0.anchor),
        exact: Boolean(c0.anchor && c0.dfwId && c0.anchor === c0.dfwId),
        ...(c0.anchor ? { selector: `[data-dfw-id="${c0.anchor}"]` } : {}),
        ...(c0.bbox && c0.bbox.every((v) => typeof v === 'number') ? { bbox: c0.bbox.map((v) => Math.round(v)) } : {}),
        ...(c0.type === 'text' ? { text: norm(c0.from) } : {}),
      },
    };
    pending.push([fp, input, key]);
  }
  if (result.tokenDiff) {
    const lines = [];
    for (const [set, d] of Object.entries(result.tokenDiff.diff)) {
      for (const c of d.changed) lines.push(`- ${set} ${c.token}: ${shortValue(c.from)} -> ${shortValue(c.to)}`);
      for (const c of d.added) lines.push(`- ${set} ${c.token} added (${c.type}): ${shortValue(c.value)}`);
    }
    if (lines.length) {
      const fp = fingerprint(lines);
      if (!ledger.has(fp)) {
        pending.push([fp, { comment: `[Penpot] Token values changed in Penpot:\n${lines.slice(0, 40).join('\n')}${lines.length > 40 ? `\n- ... ${lines.length - 40} more` : ''}\nReview with "dfw tokens import ${result.tokenFile} --dry-run"; apply only with the user's agreement (the import is refused while the design is frozen).${gateNote}`, provider: 'penpot', target: {} }, 'tokens']);
      } else result.skipped++;
    }
  }
  if (!dryRun) {
    for (const [fp, input] of pending) {
      result.comments.push(addComment(root, input));
      ledger.add(fp);
    }
    writeJson(ledgerFile, [...ledger]);
    writeJson(path.join(dir, 'changes', `${(changes.pushedAt || nowIso()).replace(/[-:.]/g, '')}.json`), changes);
  } else result.comments = pending.map(([, input]) => ({ id: '(dry run)', ...input }));
  return result;
}

export function formatImportChanges(r, root, { dryRun = false } = {}) {
  const by = {};
  for (const c of r.changes) by[c.type] = (by[c.type] || 0) + 1;
  const lines = [`Compared with export ${r.baseline}: ${r.changes.length ? Object.entries(by).map(([k, v]) => `${v} ${k}`).join(', ') : 'no changes'}.`];
  for (const c of r.comments) lines.push(`${c.id} ${[c.screen, c.variant, c.breakpoint].filter(Boolean).join('/') || '(design)'} -> ${c.target?.dfwId || '?'}\n    ${c.comment.split('\n').join('\n    ')}`);
  if (r.skipped) lines.push(`${r.skipped} skipped (already imported, or a resolved Penpot thread).`);
  if (r.tokenDiff) {
    const n = Object.values(r.tokenDiff.diff).reduce((s, d) => s + d.changed.length + d.added.length, 0);
    lines.push(`Token sets from Penpot: ${n} token change(s)${r.tokenFile ? `; file ${r.tokenFile} (dfw tokens import ${r.tokenFile} --dry-run)` : ''}.`);
  }
  for (const w of r.warnings) lines.push(`Note: ${w}`);
  if (r.frozen) lines.push('The design is frozen (G2): the changes are recorded as comments only.');
  lines.push(dryRun ? 'Dry run: no comments written.' : r.comments.length ? 'Comments are in the review loop (dfw comments list); nothing in design/ was changed.' : 'No new comments.');
  return lines.join('\n');
}

