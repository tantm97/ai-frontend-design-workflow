// @ts-check
// Selection and the reports to the Studio: what is selected (with its inspector rows), where the
// selection and the comment pins are, and the document height (frames grow to their content).
import { post, st, ui } from './state.js';
import { info, labelOf, rectOf } from './target.js';
import { inspect } from './tokens.js';
import { MSG } from '#studio/shared/protocol.js';
import { MODE } from '#studio/shared/enums.js';

/** What the Studio needs to show a selected element. */
function selPayload(/** @type {HTMLElement} */ el, /** @type {boolean} */ withInspect) {
  const t = ui.editTarget(el);
  return {
    target: info(el),
    rect: rectOf(el),
    label: labelOf(el),
    layer: st.layerNode.get(el)?.node.i ?? null,
    own: {
      id: el.dataset.dfwId || null,
      comp: el.dataset.dfwComponent || null,
      variant: el.dataset.dfwVariant || null,
      size: el.dataset.dfwSize || null,
    },
    textEditable: !t.error,
    inspect: withInspect ? inspect(el) : null,
  };
}

/**
 * Select an element (null: nothing) and tell the Studio.
 * @param {HTMLElement | null} el
 * @param {{ inspectIt?: boolean, scroll?: boolean }} [opts]
 */
export function select(el, { inspectIt = true, scroll = false } = {}) {
  st.selected = el;
  ui.placeHandles();
  if (!el) return post(MSG.SELECT, { target: null });
  if (scroll) el.scrollIntoView({ block: 'nearest' });
  post(MSG.SELECT, { comment: st.mode === MODE.COMMENT, ...selPayload(el, inspectIt && st.mode !== MODE.COMMENT) });
}

export function reportSelection() {
  if (st.selected && st.selected.isConnected) post(MSG.SEL_RECT, { rect: rectOf(st.selected) });
  ui.placeHandles();
}

/** Where each comment pin's element is now. */
export function reportPins() {
  const out = [];
  for (const p of st.pins) {
    let el = null;
    try {
      el =
        (p.selector && document.querySelector(p.selector)) ||
        (p.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(p.dfwId)}"]`)) ||
        null;
    } catch {}
    if (el && el.getClientRects().length) out.push({ id: p.id, rect: rectOf(el) });
  }
  post(MSG.PIN_RECTS, { pins: out });
}

export function reportSize() {
  const d = document.documentElement;
  post(MSG.SIZE, { h: Math.max(d.scrollHeight, document.body ? document.body.scrollHeight : 0), title: document.title });
}
