// @ts-check
// Frames and pages: builds the canvas of the current page (sections, row gutters, frames), places
// them, draws the Flow arrows, loads frames lazily, the page list, page and frame selection, and
// frame reloads (in place when that is safe, studio-ui.md U8).
import { ACTIVE, esc, frameCount as countOf, notesHtml } from './model.js';
import { L, boundsOf, flowArrows, layoutSections } from './geometry.js';
import { $, $$, ALL, P, S, currentPage, pageModel, post, postAll, store, viewContext, viewport, world, ui } from './state.js';
import { updateHud } from './hud.js';
import { MSG } from '#studio/shared/protocol.js';
import { MODE, SYSTEM_SCREEN } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

/**
 * The canvas of each recently shown page, kept (hidden) so that going back to it shows its loaded
 * frames at once. Each has a decorations layer (sections, gutters, Flow arrows), rebuilt on every
 * show, and a frames layer whose iframes are never moved (a moved iframe reloads).
 * @type {Map<string, { el: HTMLElement, deco: HTMLElement, box: HTMLElement, frames: Map<string, any> }>}
 */
const canvases = new Map();
/** How many page canvases stay alive (each holds live documents). */
const KEEP_PAGES = 3;

/** The canvas of a page, created on first use and moved to the most recent end. */
function canvasOf(/** @type {string} */ id) {
  let c = canvases.get(id);
  if (c) canvases.delete(id);
  else {
    const el = document.createElement('div');
    el.className = 'page-canvas';
    el.dataset.page = id;
    const deco = document.createElement('div');
    deco.className = 'page-deco';
    const box = document.createElement('div');
    box.className = 'page-frames';
    el.append(deco, box);
    world.append(el);
    c = { el, deco, box, frames: new Map() };
  }
  canvases.set(id, c);
  // Forget the oldest pages beyond the limit (their frames go with them).
  for (const [k, old] of canvases) {
    if (canvases.size <= KEEP_PAGES) break;
    for (const f of old.frames.values()) dropFrame(f);
    old.el.remove();
    canvases.delete(k);
  }
  return c;
}

/** Remove a frame and forget its window. */
function dropFrame(/** @type {any} */ f) {
  clearTimeout(f.softTimer);
  if (f.iframe) S.byWin.delete(f.iframe.contentWindow);
  if (f.loading) loadDone(f);
  f.el.remove();
}

/** Every frame alive in a kept canvas (shown or hidden), e.g. to patch a changed file everywhere. */
export const liveFrames = () => [...canvases.values()].flatMap((c) => [...c.frames.values()]);

/** Is this frame on the page shown now? */
export const isShown = (/** @type {any} */ f) => S.frames.get(f?.key) === f;

/**
 * Rebuild the canvas of the current page (or of every page on "All pages"). Frames whose key and
 * source did not change keep their loaded iframe; only new frames load.
 */
export function buildPage() {
  const canvas = canvasOf(S.page);
  for (const c of canvases.values()) c.el.hidden = c !== canvas;
  canvas.deco.innerHTML = '';
  const before = canvas.frames;
  canvas.frames = new Map();
  S.frames = canvas.frames;
  S.hover = null;
  S.layers = null;
  const pages = S.page === ALL ? S.pages.filter((p) => !p.flow) : [currentPage()].filter(Boolean);
  S.sections = [];
  for (const page of pages) {
    const model = pageModel(page);
    /** @type {Record<string, any>} */
    const sec = { id: page.id, model, rows: [] };
    sec.el = document.createElement('section');
    sec.el.className = 'psec';
    sec.el.dataset.page = page.id;
    sec.el.innerHTML = `<div class="psec-label">${esc(model.title)}</div>`;
    sec.head = document.createElement('header');
    sec.head.className = 'psec-head';
    sec.head.innerHTML = `<h1>${esc(model.title)}</h1>${model.description ? `<p>${esc(model.description)}</p>` : ''}`;
    canvas.deco.append(sec.el, sec.head);
    const hasNotes = model.rows.some((r) => r.notes?.length);
    sec.gutter = model.rows.some((r) => r.title || r.notes?.length) ? (hasNotes ? L.GUTTER_NOTES : L.GUTTER) : 0;
    for (const r of model.rows) {
      /** @type {Record<string, any>} */
      const row = { model: r, frames: [] };
      if (sec.gutter && (r.title || r.notes?.length)) {
        row.gut = document.createElement('div');
        row.gut.className = 'prow-gutter';
        row.gut.style.width = `${sec.gutter}px`;
        row.gut.innerHTML = r.notes?.length
          ? notesHtml(r.title, r.sub, r.notes)
          : `<h2>${esc(r.title)}</h2>${r.sub ? `<p>${esc(r.sub)}</p>` : ''}`;
        canvas.deco.append(row.gut);
      }
      for (const spec of r.frames) row.frames.push(addFrame(spec, page.id, canvas, before));
      sec.rows.push(row);
    }
    S.sections.push(sec);
  }
  // Frames no longer on the page go (the kept ones were taken out of `before`).
  for (const f of before.values()) dropFrame(f);
  // Measure text blocks once (offset sizes ignore the world transform).
  for (const sec of S.sections) {
    sec.headH = sec.head.offsetHeight;
    for (const r of sec.rows) r.gutH = r.gut ? r.gut.offsetHeight : 0;
  }
  positionFrames();
  for (const f of S.frames.values()) f.el.classList.remove('selected');
  scheduleLoads();
  $('#empty').hidden = S.frames.size > 0;
  renderPages();
  ui.renderLayers();
  ui.renderInspector();
  updateHud();
}

/**
 * The frame of one spec: the same frame as before when its key is still on the page (its iframe
 * kept, or pointed at the new source in place), else a new element with a placeholder until it
 * loads. A key used twice on "All pages" gets a ~2 suffix.
 * @param {import('./model.js').FrameSpec} spec
 * @param {string} pageId
 * @param {{ box: HTMLElement, frames: Map<string, any> }} canvas
 * @param {Map<string, any>} before the frames of the previous build
 */
function addFrame(spec, pageId, canvas, before) {
  let key = spec.key;
  for (let n = 2; canvas.frames.has(key); n++) key = `${spec.key}~${n}`;
  const old = before.get(key);
  if (old) {
    before.delete(key);
    // Keep what was measured (the grown height) unless the frame no longer grows to its content.
    const h = old.auto && spec.auto && old.loaded ? old.h : spec.h;
    Object.assign(old, spec, { key, page: pageId, h });
    if (!spec.auto) old.sizeStop = false;
    const title = $('.frame-title', old.el);
    title.firstChild.textContent = spec.title;
    $('a', title).href = spec.src;
    if (old.iframe && old.iframe.dataset.src !== spec.src) {
      // A new source (theme, state) navigates the same iframe: the old page stays until the new one paints.
      old.loaded = false;
      old.pinRects = [];
      old.iframe.dataset.src = spec.src;
      old.iframe.src = spec.src;
    }
    canvas.frames.set(key, old);
    return old;
  }
  /** @type {Record<string, any>} */
  const f = { ...spec, key, page: pageId, x: 0, y: 0, loaded: false, sizeCount: 0 };
  const el = document.createElement('div');
  el.className = 'frame';
  el.dataset.key = f.key;
  const open = `<a href="${esc(f.src)}" target="_blank" rel="noopener" title="${TEXT.openInNewTab}">↗</a>`;
  el.innerHTML = `<div class="frame-title" title="${esc(TEXT.frameTitleHint(f.title))}">${esc(f.title)}${open}</div><div class="placeholder">${esc(f.title)}</div>`;
  const t = $('.frame-title', el);
  t.addEventListener('mousedown', (/** @type {MouseEvent} */ e) => {
    if (/** @type {HTMLElement} */ (e.target).tagName === 'A' || e.button !== 0 || S.mode === MODE.HAND || S.space) return;
    e.stopPropagation();
    selectFrame(f.key);
  });
  t.addEventListener('dblclick', (/** @type {MouseEvent} */ e) => {
    if (/** @type {HTMLElement} */ (e.target).tagName === 'A') return;
    ui.openPresent(f);
  });
  f.el = el;
  canvas.frames.set(f.key, f);
  canvas.box.append(el);
  return f;
}

/** Place sections, gutters and frames from their current sizes (a frame grows to its content). */
export function positionFrames() {
  S.secBounds = new Map();
  const sizes = S.sections.map((/** @type {any} */ sec) => ({
    headH: sec.headH,
    headW: sec.head.offsetWidth,
    gutter: sec.gutter,
    flow: !!sec.model.flow,
    rows: sec.rows.map((/** @type {any} */ r) => ({ gutH: r.gutH, frames: r.frames })),
  }));
  layoutSections(sizes).forEach((place, i) => {
    const sec = S.sections[i];
    sec.head.style.cssText = `left:${place.head.x}px;top:${place.head.y}px`;
    sec.rows.forEach((/** @type {any} */ r, j) => {
      const at = place.rows[j];
      if (r.gut) r.gut.style.cssText = `left:${at.gut.x}px;top:${at.gut.y}px;width:${at.gut.w}px`;
      r.frames.forEach((/** @type {any} */ f, k) => {
        f.x = at.frames[k].x;
        f.y = at.frames[k].y;
        f.el.style.cssText = `left:${f.x}px;top:${f.y}px;width:${f.w}px;height:${f.h}px`;
      });
    });
    const [x, y, w, h] = place.bounds;
    sec.el.style.cssText = `left:${x}px;top:${y}px;width:${w}px;height:${h}px`;
    S.secBounds.set(sec.id, place.bounds);
  });
  drawFlow();
}

/** Arrows between the frames of the Flow page (world coordinates, so they pan and zoom with the canvas). */
export function drawFlow() {
  const deco = canvases.get(S.page)?.deco;
  deco?.querySelector('svg.flow')?.remove();
  if (!S.sections.some((/** @type {any} */ s) => s.model.flow)) return;
  const byScreen = new Map();
  for (const f of S.frames.values()) if (f.screen && !byScreen.has(f.screen.id)) byScreen.set(f.screen.id, f);
  const b = pageBounds();
  if (!b) return;
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('class', 'flow');
  svg.setAttribute('width', String(b[0] + b[2]));
  svg.setAttribute('height', String(b[1] + b[3]));
  svg.innerHTML =
    '<defs><marker id="flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z"/></marker></defs>';
  for (const a of flowArrows(S.manifest.flows || [], byScreen)) {
    const path = document.createElementNS(ns, 'path');
    path.setAttribute('d', a.d);
    path.setAttribute('marker-end', 'url(#flow-arrow)');
    svg.append(path);
    if (a.label) {
      const t = document.createElementNS(ns, 'text');
      t.setAttribute('x', String(a.lx));
      t.setAttribute('y', String(a.ly));
      t.textContent = a.label;
      svg.append(t);
    }
  }
  deco?.prepend(svg);
}

/** Frames loading now (same-origin frames run on the Studio's thread: a few at a time). */
const MAX_LOADING = 2;
/** The camera must be still this long before frames load (a fast wheel loads nothing on the way). */
const SETTLE_MS = 180;
let loadingCount = 0;
let lastMove = 0;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let pumpT;

/** The camera moved: loading waits until it settles. */
export function noteCameraMove() {
  lastMove = performance.now();
  scheduleLoads();
}

/** Load the frames near the viewport once the camera is still. */
export function scheduleLoads() {
  clearTimeout(pumpT);
  const wait = SETTLE_MS - (performance.now() - lastMove);
  pumpT = setTimeout(pumpLoads, Math.max(0, wait));
}

function pumpLoads() {
  if (performance.now() - lastMove < SETTLE_MS) return scheduleLoads();
  if (loadingCount >= MAX_LOADING) return;
  const r = viewport.getBoundingClientRect();
  const cx = r.width / 2;
  const cy = r.height / 2;
  const { x, y, z } = S.cam;
  const dist = (/** @type {any} */ f) => Math.hypot((f.x + f.w / 2) * z + x - cx, (f.y + Math.min(f.h, 900) / 2) * z + y - cy);
  // Near = within 400 screen px of the viewport, from the camera and the frame's world box (no
  // IntersectionObserver: a rebuilt observer that never reported left frames blank for good).
  const m = 400;
  const near = (/** @type {any} */ f) => {
    const l = f.x * z + x;
    const t = f.y * z + y;
    return l < r.width + m && l + f.w * z > -m && t < r.height + m && t + f.h * z > -m;
  };
  const todo = [...S.frames.values()].filter((f) => !f.iframe && near(f)).sort((a, b) => dist(a) - dist(b));
  for (const f of todo.slice(0, MAX_LOADING - loadingCount)) loadFrame(f);
}

// Safety net: a tab that comes back to the front, a resized window, and a slow check every second
// load whatever is near the viewport and not loaded yet.
document.addEventListener('visibilitychange', () => document.visibilityState === 'visible' && scheduleLoads());
addEventListener('resize', () => scheduleLoads());
setInterval(() => document.visibilityState === 'visible' && scheduleLoads(), 1000);

/** A frame finished loading (or gave up): let the next one start. */
function loadDone(/** @type {any} */ f) {
  if (!f.loading) return;
  f.loading = false;
  clearTimeout(f.loadTimer);
  loadingCount = Math.max(0, loadingCount - 1);
  scheduleLoads();
}

/** Put the iframe in a frame (once it is near the viewport and the camera is still). */
export function loadFrame(/** @type {any} */ f) {
  if (!f || f.iframe) return;
  const iframe = document.createElement('iframe');
  iframe.name = f.key;
  iframe.title = f.title;
  iframe.dataset.src = f.src;
  iframe.src = f.src;
  f.iframe = iframe;
  f.loading = true;
  loadingCount++;
  // The load event also comes for a page that failed (404); a page that never ends frees its slot.
  iframe.addEventListener('load', () => loadDone(f));
  f.loadTimer = setTimeout(() => loadDone(f), 6000);
  $('.placeholder', f.el)?.remove();
  f.el.append(iframe);
  S.byWin.set(iframe.contentWindow, f);
}

/** World rectangle around every section of the canvas. */
export const pageBounds = () => boundsOf([...S.secBounds.values()]);

/** The page list in the left pane, with frame and open-comment counts. */
export function renderPages() {
  const list = $('#page-list');
  const ctx = viewContext();
  list.innerHTML = '';
  const all = document.createElement('li');
  const total = S.pages.reduce((n, p) => n + (p.system ? 0 : countOf(ctx, p)), 0);
  all.innerHTML = `<button type="button" data-page="${ALL}" class="all" ${S.page === ALL ? 'aria-current="page"' : ''}><span>${TEXT.label.allPages}</span><span class="n">${total}</span></button>`;
  $('button', all).onclick = () => setPage(ALL, { fit: S.page === ALL });
  list.append(all);
  for (const p of S.pages) {
    const n = p.system ? '' : countOf(ctx, p);
    const open = S.comments.filter(
      (c) => ACTIVE.includes(c.status) && (p.system ? c.screen === SYSTEM_SCREEN : p.screens.includes(c.screen)),
    ).length;
    const li = document.createElement('li');
    const inAll = S.page === ALL;
    const count = open ? `<span class="c" title="${TEXT.openComments}">${open}</span>` : '';
    li.innerHTML =
      `<button type="button" data-page="${esc(p.id)}" ${p.id === S.page ? 'aria-current="page"' : ''} title="${inAll ? TEXT.pageInAllHint : ''}">` +
      `<span>${esc(p.title)}</span>${count}<span class="n">${n}</span></button>`;
    const b = $('button', li);
    b.onclick = () => (S.page === ALL ? ui.zoomToSection(p.id) : setPage(p.id));
    b.ondblclick = () => setPage(p.id);
    list.append(li);
  }
}

/**
 * Show a page (its camera is kept per page).
 * @param {string} id
 * @param {{ fit?: boolean }} [opts]
 */
export function setPage(id, { fit = false } = {}) {
  if (id !== ALL && !S.pages.some((p) => p.id === id))
    id = S.pages.find((p) => !p.system && !p.flow && p.screens.length)?.id || SYSTEM_SCREEN;
  S.cams.set(S.page, { ...S.cam });
  S.page = id;
  store.set('page', id);
  S.sel = null;
  S.selKey = null;
  buildPage();
  const cam = S.cams.get(id);
  if (cam && !fit) {
    S.cam = { ...cam };
    ui.applyCam();
  } else ui.zoomToFit({ initial: true });
}

/**
 * @param {string} key
 * @param {{ keepSel?: boolean }} [opts]
 */
export function selectFrame(key, { keepSel = false } = {}) {
  if (S.selKey !== key) S.layers = null;
  S.selKey = key;
  if (!keepSel && S.sel) {
    postAll({ type: MSG.CLEAR_SELECTION });
    S.sel = null;
  }
  for (const f of S.frames.values()) f.el.classList.toggle('selected', f.key === key);
  const f = S.frames.get(key);
  if (f?.loaded && (!S.layers || S.layers.key !== key)) post(f, { type: MSG.LAYERS });
  ui.renderLayers();
  ui.renderInspector();
  updateHud();
}

export function clearSelection() {
  postAll({ type: MSG.CLEAR_SELECTION });
  S.sel = null;
  S.selKey = null;
  for (const f of S.frames.values()) f.el.classList.remove('selected');
  ui.renderLayers();
  ui.renderInspector();
  updateHud();
}

/** Rebuild the canvas after a toolbar change, keeping the camera and the selected frame. */
export function rebuildKeepCam() {
  const selKey = S.selKey;
  buildPage();
  ui.applyCam();
  if (selKey && S.frames.has(selKey)) selectFrame(selKey);
}

/** Every frame of every page with the current breakpoint/state/theme choices (a screen shown on two pages once). */
export function allFrameSpecs() {
  const seen = new Map();
  for (const p of S.pages) for (const r of pageModel(p).rows) for (const f of r.frames) seen.set(f.key, f);
  return [...seen.values()];
}

/** Reload a frame (or the presenter) from the server. */
export function reloadFrame(/** @type {any} */ f) {
  if (f?.present) return P.open && ui.loadPresent();
  if (!f?.iframe) return;
  clearTimeout(f.softTimer);
  f.loaded = false;
  f.pinRects = [];
  try {
    f.iframe.contentWindow.location.reload();
  } catch {
    f.iframe.src = f.src;
  }
}

/**
 * Apply a source change inside the frame (no reload, no flash). The frame reloads itself, or is
 * reloaded here, when that is not possible.
 * @param {any} f
 * @param {string[]} [files]
 */
export function softReload(f, files = []) {
  if (!f || f.present || !f.iframe || !f.loaded) return reloadFrame(f);
  // Only markup and stylesheets are patched in place: a changed image, font or script needs the reload.
  // (A name without an extension is a folder the watcher reported.)
  if (files.some((x) => /\.(?!(?:html|css|json)$)[\w-]+$/.test(x))) return reloadFrame(f);
  clearTimeout(f.softTimer);
  f.softTimer = setTimeout(() => reloadFrame(f), 4000);
  post(f, { type: MSG.SOFT_RELOAD, files, agent: Date.now() > S.selfEditUntil });
}

/** Highlight the page buttons of a section on "All pages". */
export function markSection(/** @type {string} */ id) {
  for (const x of $$('#page-list button')) x.classList.toggle('focus', x.dataset.page === id);
}
