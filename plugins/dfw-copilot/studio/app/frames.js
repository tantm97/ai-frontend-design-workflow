// @ts-check
// Frames and pages: builds the canvas of the current page (sections, row gutters, frames), places
// them, draws the Flow arrows, loads frames lazily, the page list, page and frame selection, and
// frame reloads (in place when that is safe, studio-ui.md U8).
import { ACTIVE, esc, frameCount as countOf, notesHtml } from './model.js';
import { L, boundsOf, flowArrows, layoutSections } from './geometry.js';
import { $, $$, ALL, P, S, currentPage, pageModel, post, postAll, store, viewContext, viewport, world } from './state.js';
import { applyCam, zoomToFit, zoomToSection } from './canvas.js';
import { updateHud } from './hud.js';
import { renderLayers } from './layers.js';
import { renderInspector } from './inspector.js';
import { loadPresent, openPresent } from './present.js';

/** @type {IntersectionObserver | null} */
let io = null;

/** Rebuild the canvas of the current page (or of every page on "All pages"). */
export function buildPage() {
  world.innerHTML = '';
  S.frames.clear();
  for (const [w, f] of S.byWin) if (!f.present) S.byWin.delete(w);
  S.hover = null;
  S.layers = null;
  io?.disconnect();
  io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) if (e.isIntersecting) loadFrame(S.frames.get(/** @type {HTMLElement} */ (e.target).dataset.key));
    },
    { root: viewport, rootMargin: '400px' },
  );
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
    world.append(sec.el, sec.head);
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
        world.append(row.gut);
      }
      for (const spec of r.frames) row.frames.push(addFrame(spec, page.id));
      sec.rows.push(row);
    }
    S.sections.push(sec);
  }
  // Measure text blocks once (offset sizes ignore the world transform).
  for (const sec of S.sections) {
    sec.headH = sec.head.offsetHeight;
    for (const r of sec.rows) r.gutH = r.gut ? r.gut.offsetHeight : 0;
  }
  positionFrames();
  for (const f of S.frames.values()) io.observe(f.el);
  $('#empty').hidden = S.frames.size > 0;
  renderPages();
  renderLayers();
  renderInspector();
  updateHud();
}

/**
 * The element of one frame (title bar + placeholder until it scrolls into view); a key used twice
 * on "All pages" gets a ~2 suffix.
 * @param {import('./model.js').FrameSpec} spec
 * @param {string} pageId
 */
function addFrame(spec, pageId) {
  let key = spec.key;
  for (let n = 2; S.frames.has(key); n++) key = `${spec.key}~${n}`;
  /** @type {Record<string, any>} */
  const f = { ...spec, key, page: pageId, x: 0, y: 0, loaded: false, sizeCount: 0 };
  const el = document.createElement('div');
  el.className = 'frame';
  el.dataset.key = f.key;
  el.innerHTML = `<div class="frame-title" title="${esc(f.title)} (double-click to present)">${esc(f.title)}<a href="${esc(f.src)}" target="_blank" rel="noopener" title="Open in a new tab">↗</a></div><div class="placeholder">${esc(f.title)}</div>`;
  const t = $('.frame-title', el);
  t.addEventListener('mousedown', (/** @type {MouseEvent} */ e) => {
    if (/** @type {HTMLElement} */ (e.target).tagName === 'A' || e.button !== 0 || S.mode === 'hand' || S.space) return;
    e.stopPropagation();
    selectFrame(f.key);
  });
  t.addEventListener('dblclick', (/** @type {MouseEvent} */ e) => {
    if (/** @type {HTMLElement} */ (e.target).tagName === 'A') return;
    openPresent(f);
  });
  f.el = el;
  S.frames.set(f.key, f);
  world.append(el);
  return f;
}

/** Place sections, gutters and frames from their current sizes (a frame grows to its content). */
export function positionFrames() {
  S.secBounds = {};
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
    S.secBounds[sec.id] = place.bounds;
  });
  drawFlow();
}

/** Arrows between the frames of the Flow page (world coordinates, so they pan and zoom with the canvas). */
export function drawFlow() {
  world.querySelector('svg.flow')?.remove();
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
  world.prepend(svg);
}

/** Put the iframe in a frame (once it is near the viewport). */
export function loadFrame(/** @type {any} */ f) {
  if (!f || f.iframe) return;
  const iframe = document.createElement('iframe');
  iframe.name = f.key;
  iframe.title = f.title;
  iframe.src = f.src;
  f.iframe = iframe;
  $('.placeholder', f.el)?.remove();
  f.el.append(iframe);
  S.byWin.set(iframe.contentWindow, f);
}

/** World rectangle around every section of the canvas. */
export const pageBounds = () => boundsOf(Object.values(S.secBounds || {}));

/** The page list in the left pane, with frame and open-comment counts. */
export function renderPages() {
  const list = $('#page-list');
  const ctx = viewContext();
  list.innerHTML = '';
  const all = document.createElement('li');
  const total = S.pages.reduce((n, p) => n + (p.system ? 0 : countOf(ctx, p)), 0);
  all.innerHTML = `<button type="button" data-page="${ALL}" class="all" ${S.page === ALL ? 'aria-current="page"' : ''}><span>All pages</span><span class="n">${total}</span></button>`;
  $('button', all).onclick = () => setPage(ALL, { fit: S.page === ALL });
  list.append(all);
  for (const p of S.pages) {
    const n = p.system ? '' : countOf(ctx, p);
    const open = S.comments.filter(
      (c) => ACTIVE.includes(c.status) && (p.system ? c.screen === 'system' : p.screens.includes(c.screen)),
    ).length;
    const li = document.createElement('li');
    const inAll = S.page === ALL;
    li.innerHTML = `<button type="button" data-page="${esc(p.id)}" ${p.id === S.page ? 'aria-current="page"' : ''} title="${inAll ? 'Click: go to this section · Double-click: open the page alone' : ''}"><span>${esc(p.title)}</span>${open ? `<span class="c" title="Open comments">${open}</span>` : ''}<span class="n">${n}</span></button>`;
    const b = $('button', li);
    b.onclick = () => (S.page === ALL ? zoomToSection(p.id) : setPage(p.id));
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
  if (id !== ALL && !S.pages.some((p) => p.id === id)) id = S.pages.find((p) => !p.system && !p.flow && p.screens.length)?.id || 'system';
  S.cams[S.page] = { ...S.cam };
  S.page = id;
  store.set('page', id);
  S.sel = null;
  S.selKey = null;
  buildPage();
  if (S.cams[id] && !fit) {
    S.cam = { ...S.cams[id] };
    applyCam();
  } else zoomToFit({ initial: true });
}

/**
 * @param {string} key
 * @param {{ keepSel?: boolean }} [opts]
 */
export function selectFrame(key, { keepSel = false } = {}) {
  if (S.selKey !== key) S.layers = null;
  S.selKey = key;
  if (!keepSel && S.sel) {
    postAll({ type: 'dfw:clear-selection' });
    S.sel = null;
  }
  for (const f of S.frames.values()) f.el.classList.toggle('selected', f.key === key);
  const f = S.frames.get(key);
  if (f?.loaded && (!S.layers || S.layers.key !== key)) post(f, { type: 'dfw:layers' });
  renderLayers();
  renderInspector();
  updateHud();
}

export function clearSelection() {
  postAll({ type: 'dfw:clear-selection' });
  S.sel = null;
  S.selKey = null;
  for (const f of S.frames.values()) f.el.classList.remove('selected');
  renderLayers();
  renderInspector();
  updateHud();
}

/** Rebuild the canvas after a toolbar change, keeping the camera and the selected frame. */
export function rebuildKeepCam() {
  const selKey = S.selKey;
  buildPage();
  applyCam();
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
  if (f?.present) return P.open && loadPresent();
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
  post(f, { type: 'dfw:soft-reload', files, agent: Date.now() > S.selfEditUntil });
}

/** Highlight the page buttons of a section on "All pages". */
export function markSection(/** @type {string} */ id) {
  for (const x of $$('#page-list button')) x.classList.toggle('focus', x.dataset.page === id);
}
