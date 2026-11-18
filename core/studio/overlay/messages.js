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

const interactive = () => st.mode === 'select' || st.mode === 'comment';

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
  'dfw:env': (d) => {
    if (d.theme) document.documentElement.dataset.theme = d.theme;
    else delete document.documentElement.dataset.theme;
    if (d.state) document.documentElement.dataset.state = d.state;
    else delete document.documentElement.dataset.state;
  },
  'dfw:mode': (d) => {
    st.mode = d.mode;
    if (!interactive()) {
      st.hoverEl = null;
      post('dfw:hover', { rect: null });
    }
    document.documentElement.style.cursor = st.mode === 'comment' ? 'crosshair' : '';
    placeHandles();
  },
  'dfw:layout': (d) => {
    st.layoutCfg = { enabled: !!d.enabled, scope: d.scope || '' };
    placeHandles();
  },
  'dfw:layout-key': (d) => layoutKey(d.key, !!d.shift),
  'dfw:layout-verify': verifyLayout,
  'dfw:tokens': (d) => {
    st.tokenVars = d.vars || [];
    st.tokenCache = null;
  },
  'dfw:pins': (d) => {
    st.pins = d.pins || [];
    reportPins();
  },
  'dfw:layers': () =>
    post('dfw:layers', { tree: buildLayers(), selected: st.selected ? (st.layerNode.get(st.selected)?.node.i ?? null) : null }),
  'dfw:layer-hover': (d) => {
    const el = st.layerEls[d.i];
    post('dfw:hover', el ? { rect: rectOf(el), label: labelOf(el), comp: !!el.dataset.dfwComponent } : { rect: null });
  },
  'dfw:layer-select': (d) => {
    if (st.layerEls[d.i]) select(st.layerEls[d.i], { scroll: true });
  },
  'dfw:select-id': (d) => {
    if (!st.layerEls.length) buildLayers();
    // A component name is not looked up here: only the id, else the selector.
    const el = /** @type {HTMLElement | null} */ (find({ dfwId: d.dfwId, selector: d.selector }));
    if (el) select(el, { scroll: !!d.scroll });
    else if (d.required) post('dfw:toast', { message: `Element ${d.dfwId || d.selector} not found in this frame.` });
  },
  'dfw:clear-selection': () => {
    st.selected = null;
    if (st.editing) endEdit(false);
    placeHandles();
  },
  'dfw:soft-reload': softReload,
  'dfw:presence': (d) => {
    // The id, else the component (no selector).
    const el = /** @type {HTMLElement | null} */ (find({ dfwId: d.dfwId, component: d.component }));
    post(
      'dfw:presence',
      el && el.getClientRects().length ? { rect: rectOf(el), label: labelOf(el) } : { rect: null, fallback: !!d.fallback },
    );
  },
  'dfw:reinspect': () => {
    if (st.selected && st.selected.isConnected) select(st.selected);
  },
  'dfw:edit': () => {
    if (st.selected) startEdit(st.selected);
  },
  'dfw:nav': (d) => {
    if (!st.selected) return;
    const next = navLayer(d.dir);
    if (next) select(next, { scroll: true });
  },
};

/** Listen to the Studio (the parent window only). */
export function listenStudio() {
  addEventListener('message', (e) => {
    if (e.source !== parent) return;
    const d = e.data || {};
    if (Object.hasOwn(ON, d.type)) ON[d.type](d);
  });
}
