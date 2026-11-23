// @ts-check
// Messages from the Studio (the parent window): tool, tokens, pins, layers, selection, layout keys,
// soft updates and the agent's presence.
import { post, st } from './state.js';
import { labelOf, rectOf } from './target.js';
import { buildLayers, navLayer } from './layers.js';
import { endEdit, startEdit } from './edit.js';
import { reportPins, select } from './select.js';
import { layoutKey, placeHandles } from './layout.js';
import { verifyLayout } from './verify.js';
import { softReload } from './update.js';
import { MSG } from '../shared/protocol.js';
import { MODE } from '../shared/enums.js';
import { TEXT } from '../shared/text.js';

const interactive = () => st.mode === MODE.SELECT || st.mode === MODE.COMMENT;

/** The element with this data-dfw-id (or matching the selector), or null; a bad selector is no match. */
function find(/** @type {{ dfwId?: string, selector?: string, component?: string }} */ d) {
  try {
    return (
      (d.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(d.dfwId)}"]`)) ||
      (d.selector && document.querySelector(d.selector)) ||
      (d.component && document.querySelector(`[data-dfw-component="${CSS.escape(d.component)}"]`)) ||
      null
    );
  } catch {
    return null;
  }
}

/**
 * What each Studio message does, by type.
 * @type {Record<string, (d: any) => void>}
 */
const ON = {
  [MSG.MODE]: (d) => {
    st.mode = d.mode;
    if (!interactive()) {
      st.hoverEl = null;
      post(MSG.HOVER, { rect: null });
    }
    document.documentElement.style.cursor = st.mode === MODE.COMMENT ? 'crosshair' : '';
    placeHandles();
  },
  [MSG.LAYOUT]: (d) => {
    st.layoutCfg = { enabled: !!d.enabled, scope: d.scope || '' };
    placeHandles();
  },
  [MSG.LAYOUT_KEY]: (d) => layoutKey(d.key, !!d.shift),
  [MSG.LAYOUT_VERIFY]: verifyLayout,
  [MSG.TOKENS]: (d) => {
    st.tokenVars = d.vars || [];
    st.tokenCache = null;
  },
  [MSG.PINS]: (d) => {
    st.pins = d.pins || [];
    reportPins();
  },
  [MSG.LAYERS]: () =>
    post(MSG.LAYERS, { tree: buildLayers(), selected: st.selected ? (st.layerNode.get(st.selected)?.node.i ?? null) : null }),
  [MSG.LAYER_HOVER]: (d) => {
    const el = st.layerEls[d.i];
    post(MSG.HOVER, el ? { rect: rectOf(el), label: labelOf(el), comp: !!el.dataset.dfwComponent } : { rect: null });
  },
  [MSG.LAYER_SELECT]: (d) => {
    if (st.layerEls[d.i]) select(st.layerEls[d.i], { scroll: true });
  },
  [MSG.SELECT_ID]: (d) => {
    if (!st.layerEls.length) buildLayers();
    // A component name is not looked up here: only the id, else the selector.
    const el = /** @type {HTMLElement | null} */ (find({ dfwId: d.dfwId, selector: d.selector }));
    if (el) select(el, { scroll: !!d.scroll });
    else if (d.required) post(MSG.TOAST, { message: TEXT.elementNotFound(d.dfwId || d.selector) });
  },
  [MSG.CLEAR_SELECTION]: () => {
    st.selected = null;
    if (st.editing) endEdit(false);
    placeHandles();
  },
  [MSG.SOFT_RELOAD]: softReload,
  [MSG.PRESENCE]: (d) => {
    // The id, else the component (no selector).
    const el = /** @type {HTMLElement | null} */ (find({ dfwId: d.dfwId, component: d.component }));
    post(
      MSG.PRESENCE,
      el && el.getClientRects().length ? { rect: rectOf(el), label: labelOf(el) } : { rect: null, fallback: !!d.fallback },
    );
  },
  [MSG.EDIT]: () => {
    if (st.selected) startEdit(st.selected);
  },
  [MSG.NAV]: (d) => {
    if (!st.selected) return;
    const next = navLayer(d.dir);
    if (next) select(next, { scroll: true });
  },
};

/** Listen to the Studio: only the parent window, from our own origin (javascript.md J6). */
export function listenStudio() {
  addEventListener('message', (e) => {
    if (e.source !== parent || e.origin !== location.origin) return;
    const d = e.data || {};
    if (Object.hasOwn(ON, d.type)) ON[d.type](d);
  });
}
