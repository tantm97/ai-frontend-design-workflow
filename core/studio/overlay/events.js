// @ts-check
// Input in the frame: hover, click and double-click select (or comment), drags and handles edit the
// layout, wheel and keys go to the Studio so the canvas pans and zooms over frames, and in the
// interact and present tools links navigate the prototype.
import { post, st } from './state.js';
import { isTypingTarget } from './fields.js';
import { isUi, labelOf, pick, rectOf } from './target.js';
import { buildLayers } from './layers.js';
import { endEdit, norm, startEdit } from './edit.js';
import { reportPins, reportSelection, select } from './select.js';
import { armDrag, canLayout, canMove, endDrag, endResize, moveDrag, moveResize, resetLayout, snapOptions, startResize } from './layout.js';
import { MSG } from '../shared/protocol.js';
import { MODE, PHASE, KEY } from '../shared/enums.js';

const capture = { capture: true };
const interactive = () => st.mode === MODE.SELECT || st.mode === MODE.COMMENT;
const inEdit = (/** @type {EventTarget} */ t) => st.editing && st.editing.el.contains(/** @type {Node} */ (t));
const exact = (/** @type {MouseEvent} */ e) => e.metaKey || e.ctrlKey || e.altKey;
const hoverPayload = (/** @type {HTMLElement} */ t) => ({ rect: rectOf(t), label: labelOf(t), comp: !!t.dataset.dfwComponent });

let raf = 0;
function onMouseMove(/** @type {MouseEvent} */ e) {
  if (st.midPan) return post(MSG.PAN, { phase: PHASE.MOVE, sx: e.screenX, sy: e.screenY });
  if (st.resize || st.drag) {
    if (!(e.buttons & 1)) return st.resize ? endResize(false) : endDrag(false); // released outside the frame
    return st.resize ? moveResize(e) : moveDrag(e);
  }
  if (!interactive() || st.editing) return;
  const t = pick(e.target, exact(e));
  if (t === st.hoverEl) return;
  st.hoverEl = t;
  cancelAnimationFrame(raf);
  raf = requestAnimationFrame(() => post(MSG.HOVER, t ? hoverPayload(t) : { rect: null }));
}

function onMouseDown(/** @type {MouseEvent} */ e) {
  const t = /** @type {HTMLElement} */ (e.target);
  if (e.button === 1 && st.mode !== MODE.PRESENT) {
    e.preventDefault();
    st.midPan = true;
    return post(MSG.PAN, { phase: PHASE.START, sx: e.screenX, sy: e.screenY });
  }
  st.suppressClick = false;
  if (inEdit(t)) return;
  if (st.editing) endEdit(true);
  if (interactive() && !isUi(t)) {
    e.preventDefault();
    e.stopPropagation();
  }
  post(MSG.FOCUS);
  if (e.button !== 0 || st.mode !== MODE.SELECT) return;
  if (t.dataset && t.dataset.dfwHandle && canLayout(st.selected)) {
    e.preventDefault();
    e.stopPropagation();
    return startResize(t.dataset.dfwHandle, e);
  }
  if (st.selected && st.selected.contains(t) && canMove(st.selected) && !exact(e)) armDrag(e);
}

function onMouseUp(/** @type {MouseEvent} */ e) {
  if (st.midPan) {
    st.midPan = false;
    e.preventDefault();
    post(MSG.PAN, { phase: PHASE.END });
  }
  if (st.resize) endResize(true);
  else if (st.drag) endDrag(true);
}

function onClick(/** @type {MouseEvent} */ e) {
  if (st.suppressClick) {
    st.suppressClick = false;
    e.preventDefault();
    e.stopPropagation();
    return;
  }
  if (inEdit(e.target) || isUi(e.target)) return;
  if (!interactive()) return prototypeClick(e);
  e.preventDefault();
  e.stopPropagation();
  const t = pick(e.target, exact(e));
  if (!st.layerEls.length) buildLayers();
  select(t);
}

function onDblClick(/** @type {MouseEvent} */ e) {
  if (st.mode !== MODE.SELECT || inEdit(e.target)) return;
  e.preventDefault();
  e.stopPropagation();
  const t = /** @type {HTMLElement} */ (e.target);
  if (t.dataset && t.dataset.dfwHandle && canLayout(st.selected))
    return resetLayout(st.selected, snapOptions(st.selected, t.dataset.dfwHandle).prop);
  if (t.nodeType === 1 && !t.children.length && norm(t.textContent)) {
    select(t, { inspectIt: true });
    startEdit(t);
  } else if (t.nodeType === 1) select(t);
}

const fwd = (/** @type {KeyboardEvent} */ e, /** @type {string} */ type) =>
  post(MSG.KEY, { phase: type, key: e.key, code: e.code, meta: e.metaKey, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey });

function onKeyDown(/** @type {KeyboardEvent} */ e) {
  if ((st.resize || st.drag) && e.key === KEY.ESCAPE) {
    e.preventDefault();
    return st.resize ? endResize(false) : endDrag(false);
  }
  if (st.editing) {
    if (e.key === KEY.ENTER && !e.shiftKey) {
      e.preventDefault();
      endEdit(true);
    } else if (e.key === KEY.ESCAPE) {
      e.preventDefault();
      endEdit(false);
    }
    return;
  }
  const target = /** @type {any} */ (e.target);
  const typing = isTypingTarget(target);
  if (st.mode === MODE.PRESENT || st.mode === MODE.INTERACT) {
    // The page is live: keep its keyboard behaviour; only forward what the Studio needs.
    if (typing) return e.key === KEY.ESCAPE && fwd(e, PHASE.DOWN);
    if (st.mode === MODE.PRESENT && (e.key === KEY.LEFT || e.key === KEY.RIGHT) && !e.metaKey && !e.ctrlKey) e.preventDefault();
    if (st.mode === MODE.INTERACT && e.key === KEY.SPACE) e.preventDefault();
    return fwd(e, PHASE.DOWN);
  }
  if (typing && !interactive()) return;
  if (/** @type {string[]} */ ([KEY.SPACE, KEY.UP, KEY.DOWN, KEY.LEFT, KEY.RIGHT, KEY.TAB]).includes(e.key) || e.metaKey || e.ctrlKey) {
    if (!(e.metaKey && ['c', 'v', 'x', 'a', 'r', 'q', 'w', 't'].includes(e.key.toLowerCase()))) e.preventDefault();
  }
  fwd(e, PHASE.DOWN);
}

/** Interact and present tools: data-dfw-goto and links to other design files navigate the prototype. */
function prototypeClick(/** @type {MouseEvent} */ e) {
  const target = /** @type {any} */ (e.target);
  const goto = target.closest && target.closest('[data-dfw-goto]');
  if (goto) {
    e.preventDefault();
    e.stopPropagation();
    return post(MSG.GOTO, { goto: goto.getAttribute('data-dfw-goto') });
  }
  const a = target.closest && target.closest('a[href]');
  if (!a || e.defaultPrevented) return;
  const href = a.getAttribute('href');
  if (!href || href.startsWith('#') || /^(javascript|mailto|tel):/i.test(href)) return;
  const url = new URL(href, location.href);
  e.preventDefault();
  if (url.origin !== location.origin) return void window.open(url.href, '_blank', 'noopener');
  post(MSG.GOTO, { href: url.pathname + url.search });
}

/** Listen to pointer, wheel, keys and scroll in the frame (capture: the page's own handlers come after). */
export function listenInput() {
  document.addEventListener('mousemove', onMouseMove, capture);
  document.addEventListener('mouseleave', () => {
    st.hoverEl = null;
    post(MSG.HOVER, { rect: null });
  });
  document.addEventListener('mousedown', onMouseDown, capture);
  document.addEventListener('mouseup', onMouseUp, capture);
  document.addEventListener('auxclick', (e) => e.button === 1 && e.preventDefault(), capture);
  document.addEventListener('click', onClick, capture);
  document.addEventListener('dblclick', onDblClick, capture);
  // Prototype forms have no backend: never submit (and never navigate the frame away).
  document.addEventListener('submit', (e) => e.cancelable && e.preventDefault(), capture);
  addEventListener(
    'wheel',
    (e) => {
      if (st.mode === MODE.PRESENT) return; // the presented page scrolls normally
      e.preventDefault();
      post(MSG.WHEEL, {
        dx: e.deltaX,
        dy: e.deltaY,
        dm: e.deltaMode,
        ctrl: e.ctrlKey || e.metaKey,
        shift: e.shiftKey,
        x: e.clientX,
        y: e.clientY,
      });
    },
    { passive: false, capture: true },
  );
  document.addEventListener('keydown', onKeyDown, capture);
  document.addEventListener('keyup', (e) => !st.editing && st.mode !== MODE.PRESENT && e.key === KEY.SPACE && fwd(e, PHASE.UP), capture);
  addEventListener(
    'scroll',
    () => {
      reportSelection();
      if (st.pins.length) reportPins();
      if (st.hoverEl) post(MSG.HOVER, hoverPayload(st.hoverEl));
    },
    true,
  );
}
