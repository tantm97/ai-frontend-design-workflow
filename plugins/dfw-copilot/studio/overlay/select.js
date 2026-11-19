// @ts-check
// Selection and the reports to the Studio: what is selected (with its inspector rows), where the
// selection and the comment pins are, and the document height (frames grow to their content).
import { post, st } from './state.js';
import { info, labelOf, rectOf } from './target.js';
import { inspect } from './tokens.js';
import { editTarget } from './edit.js';
import { placeHandles } from './layout.js';

/** What the Studio needs to show a selected element. */
function selPayload(/** @type {HTMLElement} */ el, /** @type {boolean} */ withInspect) {
  const t = editTarget(el);
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
  placeHandles();
  if (!el) return post('dfw:select', { target: null });
  if (scroll) el.scrollIntoView({ block: 'nearest' });
  post('dfw:select', { comment: st.mode === 'comment', ...selPayload(el, inspectIt && st.mode !== 'comment') });
}

export function reportSelection() {
  if (st.selected && st.selected.isConnected) post('dfw:sel-rect', { rect: rectOf(st.selected) });
  placeHandles();
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
  post('dfw:pin-rects', { pins: out });
}

export function reportSize() {
  const d = document.documentElement;
  post('dfw:size', { h: Math.max(d.scrollHeight, document.body ? document.body.scrollHeight : 0), title: document.title });
}
