// @ts-check
// Keyboard: the shortcuts of the canvas and the presenter (also forwarded by the frames' overlay),
// the tools (select, hand, comment, interact) and Space-to-pan. Shortcuts never fire in a field
// (studio-ui.md U7).
import { $, $$, P, S, post, postAll } from './state.js';
import { clearSelection } from './frames.js';
import { zoomCenter, zoomStep, zoomToFit, zoomToSelection } from './canvas.js';
import { updateHud } from './hud.js';
import { renderLayers } from './layers.js';
import { renderInspector } from './inspector.js';
import { undo } from './edits.js';
import { closeComposer, saveComment } from './comments.js';
import { closePop, cyclePins } from './pins.js';
import { closePresent, openPresent, presentStep, setPresentMode, toggleFullscreen, togglePresentPins } from './present.js';
import { MSG } from '#studio/shared/protocol.js';
import { MODE, PHASE, KEY } from '#studio/shared/enums.js';

/** @typedef {{ phase: 'down' | 'up', key: string, code: string, meta: boolean, ctrl: boolean, shift: boolean, alt: boolean }} KeyInput */

function setSpace(/** @type {boolean} */ on) {
  if (S.space === on) return;
  S.space = on;
  document.body.classList.toggle('space', on);
}

/** Tool of the canvas: 'select', 'hand', 'comment' or 'interact'. */
export function setMode(/** @type {string} */ mode) {
  S.mode = mode;
  document.body.dataset.mode = mode;
  for (const b of $$('.seg button')) b.setAttribute('aria-checked', String(b.dataset.mode === mode));
  if (mode !== MODE.COMMENT) closeComposer();
  S.hover = null;
  postAll({ type: MSG.MODE, mode });
  updateHud();
}

/** Keys of the presenter. */
function presentKey(/** @type {KeyInput} */ d, /** @type {boolean} */ mod) {
  const key = d.key;
  if (mod || d.alt) return;
  if (key === KEY.ESCAPE) {
    if (!$('#composer').hidden) return closeComposer();
    if (S.pop) return closePop();
    if (P.mode === MODE.COMMENT) return setPresentMode(MODE.PRESENT);
    return closePresent();
  }
  if (key === KEY.LEFT) return presentStep(-1);
  if (key === KEY.RIGHT) return presentStep(1);
  if (key === 'C' && d.shift) return togglePresentPins();
  if (key === 'c' || key === 'C') return setPresentMode(P.mode === MODE.COMMENT ? MODE.PRESENT : MODE.COMMENT);
  if (key === 'f' || key === 'F') return toggleFullscreen();
  if (key === 'p' || key === 'P') return closePresent();
  if (key === KEY.HELP) $('#shortcuts').hidden = false;
}

/** Escape: close the composer, the popover, the selection, then go back to the select tool. */
function escape() {
  if (!$('#composer').hidden) return closeComposer();
  if (S.pop) return closePop();
  if (S.sel) {
    postAll({ type: MSG.CLEAR_SELECTION });
    S.sel = null;
    renderLayers();
    renderInspector();
    return updateHud();
  }
  if (S.selKey) return clearSelection();
  if (S.mode !== MODE.SELECT) setMode(MODE.SELECT);
}

/** A key from the Studio page or forwarded by a frame. */
export function handleKey(/** @type {KeyInput} */ d) {
  const key = d.key;
  if (d.phase === PHASE.UP) {
    if (key === KEY.SPACE) setSpace(false);
    return;
  }
  const mod = d.meta || d.ctrl;
  if (!$('#shortcuts').hidden) {
    if (key === KEY.ESCAPE || key === KEY.HELP) $('#shortcuts').hidden = true;
    return;
  }
  if (P.open) return presentKey(d, mod);
  if (key === KEY.SPACE) return setSpace(true);
  if (mod && (key === 'z' || key === 'Z') && !d.shift) return undo();
  if (mod && (key === '=' || key === '+')) return zoomStep(1);
  if (mod && key === '-') return zoomStep(-1);
  if (mod && key === '0') return zoomCenter(1);
  if (d.alt && !mod && (key.startsWith('Arrow') || (d.shift && d.code === 'Digit0')) && S.sel && S.mode === MODE.SELECT) {
    const f = S.frames.get(S.sel.key);
    if (f && !f.system && f.file) post(f, { type: MSG.LAYOUT_KEY, key: key.startsWith('Arrow') ? key : 'reset', shift: d.shift });
    return;
  }
  if (mod || d.alt) return;
  if (d.shift && (d.code === 'Digit1' || key === '!')) return zoomToFit();
  if (d.shift && (d.code === 'Digit2' || key === '@')) return zoomToSelection();
  switch (key) {
    case KEY.ESCAPE:
      return escape();
    case 'v':
    case 'V':
      return setMode(MODE.SELECT);
    case 'h':
    case 'H':
      return setMode(MODE.HAND);
    case 'i':
    case 'I':
      return setMode(MODE.INTERACT);
    case 'p':
    case 'P':
      return openPresent();
    case 'f':
    case 'F':
      return toggleFullscreen();
    case 'C':
      if (d.shift) return cyclePins();
    // falls through
    case 'c':
      return setMode(S.mode === MODE.COMMENT ? MODE.SELECT : MODE.COMMENT);
    case '+':
    case '=':
      return zoomStep(1);
    case '-':
    case '_':
      return zoomStep(-1);
    case '0':
      return zoomCenter(1);
    case KEY.HELP:
      $('#shortcuts').hidden = false;
      return;
    case KEY.UP:
    case KEY.DOWN:
    case KEY.LEFT:
    case KEY.RIGHT:
      if (S.sel) post(S.frames.get(S.sel.key), { type: MSG.NAV, dir: key.slice(5).toLowerCase() });
      return;
    case KEY.ENTER:
      if (S.sel?.textEditable) post(S.frames.get(S.sel.key), { type: MSG.EDIT });
  }
}

/**
 * Space and Enter belong to a control the keyboard focused (Tab, a roving row): they press it.
 * After a mouse click the control is focused but not :focus-visible, and Space still pans.
 */
const pressesControl = (/** @type {KeyboardEvent} */ e) =>
  (e.key === KEY.SPACE || e.key === KEY.ENTER) &&
  !!(/** @type {HTMLElement} */ (e.target).closest?.('button, a[href], summary, [role="treeitem"]')?.matches(':focus-visible'));

/** Keys on the Studio page itself (fields keep theirs). */
export function initKeys() {
  document.addEventListener('keydown', (e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    if (t.closest('textarea, input, select')) {
      if (t.id === 'composer-text' && e.key === KEY.ENTER && (e.metaKey || e.ctrlKey)) saveComment();
      if (e.key === KEY.ESCAPE) {
        closeComposer();
        t.blur();
      }
      return;
    }
    if (pressesControl(e)) return;
    /** @type {KeyInput} */
    const d = { phase: PHASE.DOWN, key: e.key, code: e.code, meta: e.metaKey, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey };
    if (e.key === KEY.SPACE || e.key.startsWith('Arrow') || ((e.metaKey || e.ctrlKey) && ['z', '=', '-', '0', '+'].includes(e.key)))
      e.preventDefault();
    handleKey(d);
  });
  document.addEventListener('keyup', (e) => {
    if (e.key !== KEY.SPACE) return;
    // Space is the hand tool: it does not press a button the mouse left focused (one the keyboard focused, it does).
    if (!(/** @type {HTMLElement} */ (e.target).closest('textarea, input, select')) && !pressesControl(e)) e.preventDefault();
    setSpace(false);
  });
  // Toolbar buttons should not keep keyboard focus after a click (Space/Enter belong to the canvas).
  document
    .querySelector('.topbar')
    .addEventListener('mousedown', (e) => /** @type {HTMLElement} */ (e.target).closest('button') && e.preventDefault());
  window.addEventListener('blur', () => setSpace(false));
}
