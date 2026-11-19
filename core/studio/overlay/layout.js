// @ts-check
// Layout edits: drag to reorder, resize with snapping (select tool, design files only). Constrained
// on purpose: no free positioning. Dragging reorders an element among the data-dfw-id children of a
// flex/grid container (or drops it into an empty one); resizing snaps to the grid (column span) or to
// tokens / fixed percentages. The Studio writes the change to the screen source.
import { post, st } from './state.js';
import { isUi } from './target.js';
import { reportSelection } from './select.js';

const FLOW = /^(inline-)?(flex|grid)$/;
const isFlow = (/** @type {any} */ el) => !!el && el.nodeType === 1 && FLOW.test(getComputedStyle(el).display);
const isGrid = (/** @type {Element} */ el) => /grid$/.test(getComputedStyle(el).display);
/** The data-dfw-id children of a container that are shown. @returns {HTMLElement[]} */
const idKids = (/** @type {Element} */ c) =>
  /** @type {HTMLElement[]} */ ([...c.children]).filter(
    (x) => x.dataset && x.dataset.dfwId && !x.hasAttribute('data-dfw-ui') && x.getClientRects().length,
  );
/** Can this element be moved or resized now? */
export const canLayout = (/** @type {HTMLElement | null} */ el) =>
  st.layoutCfg.enabled && st.mode === 'select' && !st.editing && el && el.isConnected && !!el.dataset.dfwId && el !== document.body;
export const canMove = (/** @type {HTMLElement | null} */ el) => canLayout(el) && isFlow(el.parentElement);
const PERCENTS = [25, 33.333, 50, 66.667, 75, 100];
const HANDLE = 'width:10px;height:10px;background:#fff;border:2px solid #0d99ff;border-radius:2px;pointer-events:auto;';

/** @type {{ ui: HTMLElement, hX: HTMLElement, hY: HTMLElement, line: HTMLElement, zone: HTMLElement, badge: HTMLElement } | null} */
let parts = null;
/** The overlay's own layer (handles, drop line, drop zone, badge), made on first use. */
function ui() {
  if (parts) return parts;
  const root = document.createElement('div');
  root.setAttribute('data-dfw-ui', '');
  root.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483646;';
  const mk = (/** @type {string} */ css) => {
    const d = document.createElement('div');
    d.setAttribute('data-dfw-ui', '');
    d.style.cssText = `position:fixed;display:none;box-sizing:border-box;${css}`;
    root.append(d);
    return d;
  };
  const hX = mk(`${HANDLE}cursor:ew-resize;`);
  const hY = mk(`${HANDLE}cursor:ns-resize;`);
  hX.dataset.dfwHandle = 'x';
  hY.dataset.dfwHandle = 'y';
  hX.title =
    'Drag to resize the width (snaps to grid columns, size tokens or percentages). Alt+Shift+Left/Right. Double-click: back to auto';
  hY.title = 'Drag to set a min-height (snaps to spacing/size tokens). Alt+Shift+Up/Down. Double-click: back to auto';
  const line = mk('background:#0d99ff;border-radius:2px;');
  const zone = mk('border:2px dashed #0d99ff;background:rgba(13,153,255,.12);border-radius:4px;'); // drop-into-container highlight
  const badge = mk(
    'padding:2px 6px;border-radius:4px;background:#0d99ff;color:#fff;font:600 11px/16px system-ui,sans-serif;white-space:nowrap;',
  );
  parts = { ui: root, hX, hY, line, zone, badge };
  return parts;
}
const mountUi = () => !ui().ui.isConnected && (document.body || document.documentElement).append(ui().ui);

/** Show the resize handles on the selected element when it can be resized. */
export function placeHandles() {
  const { hX, hY } = ui();
  const show = !st.drag?.active && !st.resize?.moved && st.selected && canLayout(st.selected) && st.selected.getClientRects().length;
  hX.style.display = hY.style.display = show ? 'block' : 'none';
  if (!show) return;
  mountUi();
  const r = st.selected.getBoundingClientRect();
  hX.style.left = `${r.right - 5}px`;
  hX.style.top = `${r.top + r.height / 2 - 5}px`;
  hY.style.left = `${r.left + r.width / 2 - 5}px`;
  hY.style.top = `${r.bottom - 5}px`;
}

function showBadge(/** @type {string} */ text, /** @type {number} */ x, /** @type {number} */ y) {
  const { badge } = ui();
  mountUi();
  badge.textContent = text;
  badge.style.display = 'block';
  badge.style.left = `${Math.min(x + 12, innerWidth - badge.offsetWidth - 4)}px`;
  badge.style.top = `${Math.max(4, y - 28)}px`;
}
const hideFeedback = () => {
  const { line, zone, badge } = ui();
  line.style.display = 'none';
  zone.style.display = 'none';
  badge.style.display = 'none';
};
const scoped = (/** @type {string} */ t) => (st.layoutCfg.scope ? `${t} · ${st.layoutCfg.scope}` : t);

/** px value of every space.* / size.* dimension token (resolved in this document). */
function tokenLengths() {
  const key = document.documentElement.dataset.theme || '';
  if (st.lengthCache && st.lengthCache.key === key && st.lengthCache.n === st.tokenVars.length) return st.lengthCache.list;
  const box = document.createElement('div');
  box.setAttribute('data-dfw-ui', '');
  box.style.cssText = 'position:absolute;left:-99999px;top:0;visibility:hidden;';
  const cand = st.tokenVars.filter((t) => /^(space|size)\./.test(t.name) && t.type !== 'typography');
  const probes = cand.map((t) => {
    const d = document.createElement('div');
    d.style.cssText = `width:var(${t.var});height:0;`;
    box.append(d);
    return d;
  });
  (document.body || document.documentElement).append(box);
  const list = probes
    .map((d, i) => ({ name: cand[i].name, var: cand[i].var, px: d.getBoundingClientRect().width }))
    .filter((t) => t.px > 0);
  box.remove();
  list.sort((a, b) => a.px - b.px || Number(/\.\d/.test(a.name)) - Number(/\.\d/.test(b.name)));
  st.lengthCache = { key, n: st.tokenVars.length, list };
  return list;
}

/**
 * @typedef {{ value: string, css: string, label: string, px: number }} Snap value (sent to the server), css (preview)
 */

/**
 * Snap options for a resize, sorted by px.
 * @param {HTMLElement} el
 * @param {string} axis 'x' or 'y'
 * @returns {{ prop: string, list: Snap[] }}
 */
export function snapOptions(el, axis) {
  const p = el.parentElement;
  if (axis === 'x' && isGrid(p)) {
    const cs = getComputedStyle(p);
    const tracks = cs.gridTemplateColumns
      .split(/\s+/)
      .filter((t) => /px$/.test(t))
      .map(parseFloat);
    const gap = parseFloat(cs.columnGap) || 0;
    const n = Math.max(1, tracks.length);
    const track = tracks.length ? tracks.reduce((a, b) => a + b, 0) / tracks.length : p.clientWidth;
    return {
      prop: 'grid-column',
      list: Array.from({ length: n }, (_, i) => ({
        value: `span ${i + 1}`,
        css: `span ${i + 1}`,
        label: `span ${i + 1} / ${n}`,
        px: (i + 1) * track + i * gap,
      })),
    };
  }
  const toks = tokenLengths();
  if (axis === 'x') {
    const pcs = getComputedStyle(p);
    const inner = p.clientWidth - parseFloat(pcs.paddingLeft) - parseFloat(pcs.paddingRight);
    const pct = PERCENTS.map((v) => ({ value: `${v}%`, css: `${v}%`, label: `${Math.round(v)}%`, px: (inner * v) / 100 }));
    const sizes = toks
      .filter((t) => t.name.startsWith('size.') && t.px <= inner)
      .map((t) => ({ value: t.name, css: `var(${t.var})`, label: `${t.name} · ${Math.round(t.px)}px`, px: t.px }));
    return { prop: 'width', list: [...pct, ...sizes].sort((a, b) => a.px - b.px) };
  }
  return {
    prop: 'min-height',
    list: toks.map((t) => ({ value: t.name, css: `var(${t.var})`, label: `${t.name} · ${Math.round(t.px)}px`, px: t.px })),
  };
}

/**
 * The option nearest to px (the first of equals).
 * @template {{ px: number }} T
 * @param {T[]} list
 * @param {number} px
 * @returns {T}
 */
export const nearest = (list, px) => list.reduce((best, o) => (Math.abs(o.px - px) < Math.abs(best.px - px) ? o : best), list[0]);
/** CSS property -> style property. */
export const styleProp = { 'grid-column': 'gridColumn', width: 'width', 'min-height': 'minHeight' };

/** Mousedown on a handle: start a resize along its axis. */
export function startResize(/** @type {string} */ axis, /** @type {MouseEvent} */ e) {
  const el = st.selected;
  const opts = snapOptions(el, axis);
  if (!opts.list.length) return post('dfw:toast', { message: 'No size tokens to snap to.' });
  st.resize = {
    el,
    axis,
    rect: el.getBoundingClientRect(),
    x: e.clientX,
    y: e.clientY,
    moved: false,
    ...opts,
    pick: null,
    prev: el.style[styleProp[opts.prop]],
  };
  placeHandles();
}

export function moveResize(/** @type {MouseEvent} */ e) {
  const r = st.resize;
  // A click (or the double-click that resets) on a handle is not a resize.
  if (!r.moved && Math.hypot(e.clientX - r.x, e.clientY - r.y) < 3) return;
  if (!r.moved) {
    r.moved = true;
    placeHandles();
  }
  const px = r.axis === 'x' ? e.clientX - r.rect.left : e.clientY - r.rect.top;
  /** @type {Snap} */
  const o = nearest(r.list, px);
  if (o !== r.pick) {
    r.pick = o;
    r.el.style[styleProp[r.prop]] = o.css;
    reportSelection();
  }
  showBadge(scoped(r.prop === 'grid-column' ? o.label : `${r.prop} ${o.label}`), e.clientX, e.clientY);
}

export function endResize(/** @type {boolean} */ commit) {
  const r = st.resize;
  st.resize = null;
  hideFeedback();
  if (commit && r.moved && r.pick)
    post('dfw:layout-op', { op: 'layout', id: r.el.dataset.dfwId, prop: r.prop, value: r.pick.value, label: r.pick.label });
  else r.el.style[styleProp[r.prop]] = r.prev;
  placeHandles();
  reportSelection();
}

/** An empty flex/grid container with a data-dfw-id: nothing in it but whitespace (and dfw UI). */
const isEmptyBox = (/** @type {any} */ n) =>
  n.dataset && n.dataset.dfwId && isFlow(n) && ![...n.children].some((k) => !k.hasAttribute('data-dfw-ui')) && !n.textContent.trim();

/**
 * Where a dragged element would land: { ref, position, vertical } next to a data-dfw-id child of a flex/grid
 * container, or { ref: container, position: 'inside' } over an empty flex/grid container that has a data-dfw-id.
 * @param {number} x
 * @param {number} y
 * @returns {{ ref: HTMLElement, position: string, vertical?: boolean, r: DOMRect } | null}
 */
function dropAt(x, y) {
  const d = st.drag.el;
  let n = /** @type {any} */ (document.elementFromPoint(x, y));
  let c = null;
  const home = d.parentElement;
  for (let e = n; e && e !== home && e !== document.documentElement; e = e.parentElement) {
    if (isUi(e) || d.contains(e)) break;
    if (isEmptyBox(e)) return { ref: e, position: 'inside', r: e.getBoundingClientRect() };
    if (isFlow(e) && idKids(e).length) break;
  }
  const hr = home.getBoundingClientRect();
  if (x >= hr.left && x <= hr.right && y >= hr.top && y <= hr.bottom) c = home;
  for (; !c && n && n !== document.documentElement; n = n.parentElement) {
    if (isUi(n) || d.contains(n)) return null;
    if (isFlow(n) && idKids(n).some((k) => k !== d)) c = n;
  }
  if (!c) return null;
  const kids = idKids(c).filter((k) => k !== d);
  if (!kids.length) return null;
  const dist = (/** @type {DOMRect} */ r) => Math.hypot(Math.max(r.left - x, 0, x - r.right), Math.max(r.top - y, 0, y - r.bottom));
  let best = null;
  for (const k of kids) {
    const r = k.getBoundingClientRect();
    const dd = dist(r);
    if (!best || dd < best.dd) best = { k, r, dd };
  }
  const { k, r } = best;
  let horizontal; // true: decide by x (items flow left-right)
  if (isGrid(c)) horizontal = y >= r.top && y <= r.bottom;
  else horizontal = getComputedStyle(c).flexDirection.startsWith('row');
  const before = horizontal ? x < r.left + r.width / 2 : y < r.top + r.height / 2;
  return { ref: k, position: before ? 'before' : 'after', vertical: horizontal, r };
}

/** Mousedown on the selected element: a drag starts once the pointer moved 5px. */
export function armDrag(/** @type {MouseEvent} */ e) {
  st.drag = { el: st.selected, x: e.clientX, y: e.clientY, active: false };
}

export function moveDrag(/** @type {MouseEvent} */ e) {
  const drag = st.drag;
  const { line, zone } = ui();
  if (!drag.active) {
    if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
    drag.active = true;
    drag.prevOpacity = drag.el.style.opacity;
    drag.prevPointer = drag.el.style.pointerEvents;
    drag.el.style.opacity = '0.4';
    drag.el.style.pointerEvents = 'none';
    placeHandles();
    post('dfw:hover', { rect: null });
  }
  const t = (drag.drop = dropAt(e.clientX, e.clientY));
  mountUi();
  if (!t) {
    line.style.display = zone.style.display = 'none';
    return showBadge('Drop next to a sibling in a flex or grid container, or into an empty one', e.clientX, e.clientY);
  }
  const { r } = t;
  if (t.position === 'inside') {
    line.style.display = 'none';
    Object.assign(zone.style, {
      left: `${r.left}px`,
      top: `${r.top}px`,
      width: `${r.width}px`,
      height: `${Math.max(r.height, 8)}px`,
      display: 'block',
    });
    return showBadge(`Into ${t.ref.dataset.dfwId}`, e.clientX, e.clientY);
  }
  zone.style.display = 'none';
  const edge = t.position === 'before' ? (t.vertical ? r.left : r.top) : t.vertical ? r.right : r.bottom;
  Object.assign(
    line.style,
    t.vertical
      ? { left: `${edge - 2}px`, top: `${r.top}px`, width: '3px', height: `${r.height}px` }
      : { left: `${r.left}px`, top: `${edge - 2}px`, width: `${r.width}px`, height: '3px' },
  );
  line.style.display = 'block';
  showBadge(`${t.position === 'before' ? 'Before' : 'After'} ${t.ref.dataset.dfwId}`, e.clientX, e.clientY);
}

export function endDrag(/** @type {boolean} */ commit) {
  const g = st.drag;
  st.drag = null;
  hideFeedback();
  if (!g?.active) return placeHandles();
  g.el.style.opacity = g.prevOpacity;
  g.el.style.pointerEvents = g.prevPointer;
  st.suppressClick = true; // the click that ends the drag must not re-select (reset on the next mousedown)
  const t = g.drop;
  if (commit && t && t.position === 'inside') {
    t.ref.append(g.el); // optimistic; the frame reloads from the source
    post('dfw:layout-op', {
      op: 'move',
      id: g.el.dataset.dfwId,
      ref: t.ref.dataset.dfwId,
      position: 'inside',
      label: `Moved into ${t.ref.dataset.dfwId}`,
    });
  } else if (commit && t) {
    const next = t.position === 'before' ? t.ref : t.ref.nextSibling;
    if (
      next !== g.el &&
      !(t.position === 'before' && g.el.nextElementSibling === t.ref) &&
      !(t.position === 'after' && t.ref.nextElementSibling === g.el)
    ) {
      t.ref.parentElement.insertBefore(g.el, next); // optimistic; the frame reloads from the source
      post('dfw:layout-op', {
        op: 'move',
        id: g.el.dataset.dfwId,
        ref: t.ref.dataset.dfwId,
        position: t.position,
        label: `Moved ${t.position} ${t.ref.dataset.dfwId}`,
      });
    }
  }
  placeHandles();
  reportSelection();
}

/**
 * Reset to auto: drop the element's layout rule (prop null: the whole rule) at this frame's breakpoint scope.
 * @param {HTMLElement} el
 * @param {string | null} [prop]
 */
export function resetLayout(el, prop = null) {
  for (const p of prop ? [prop] : Object.keys(styleProp)) el.style[styleProp[p]] = '';
  reportSelection();
  placeHandles();
  post('dfw:layout-op', {
    op: 'layout',
    id: el.dataset.dfwId,
    prop,
    value: null,
    label: prop ? `${prop} reset to auto` : 'Size reset to auto',
  });
}

/**
 * Alt+Arrow: move among siblings; Alt+Shift+Arrow: step the span / width (Left/Right) or min-height (Up/Down); Alt+Shift+0: reset.
 * @param {string} key an Arrow key name, or 'reset'
 * @param {boolean} shift
 */
export function layoutKey(key, shift) {
  const el = st.selected;
  if (!canLayout(el)) return post('dfw:toast', { message: 'Select an element with a data-dfw-id to move or resize it.' });
  if (key === 'reset') return resetLayout(el);
  const back = key === 'ArrowLeft' || key === 'ArrowUp';
  if (!shift) {
    if (!canMove(el)) return post('dfw:toast', { message: 'Only children of a flex or grid container can be reordered.' });
    const sibs = idKids(el.parentElement);
    const i = sibs.indexOf(el);
    const ref = sibs[back ? i - 1 : i + 1];
    if (!ref) return post('dfw:toast', { message: back ? 'Already the first item.' : 'Already the last item.' });
    const position = back ? 'before' : 'after';
    ref.parentElement.insertBefore(el, back ? ref : ref.nextSibling);
    reportSelection();
    return post('dfw:layout-op', {
      op: 'move',
      id: el.dataset.dfwId,
      ref: ref.dataset.dfwId,
      position,
      label: `Moved ${position} ${ref.dataset.dfwId}`,
    });
  }
  const axis = key === 'ArrowLeft' || key === 'ArrowRight' ? 'x' : 'y';
  const { prop, list } = snapOptions(el, axis);
  if (!list.length) return post('dfw:toast', { message: 'No size tokens to snap to.' });
  let cur;
  if (prop === 'grid-column') {
    const m = /span\s+(\d+)/.exec(getComputedStyle(el).gridColumnEnd) || /span\s+(\d+)/.exec(getComputedStyle(el).gridColumnStart);
    cur = list.findIndex((o) => o.value === `span ${m ? m[1] : 1}`);
  } else {
    const now = axis === 'x' ? el.getBoundingClientRect().width : parseFloat(getComputedStyle(el).minHeight) || 0;
    cur = list.indexOf(nearest(list, now));
    if (axis === 'y' && !now) cur = -1;
  }
  const o = list[Math.max(0, Math.min(list.length - 1, cur + (back ? -1 : 1)))];
  if (cur >= 0 && o === list[cur]) return post('dfw:toast', { message: `Already ${o.label}.` });
  el.style[styleProp[prop]] = o.css;
  reportSelection();
  post('dfw:layout-op', { op: 'layout', id: el.dataset.dfwId, prop, value: o.value, label: o.label });
}
