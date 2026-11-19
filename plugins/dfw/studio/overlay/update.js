// @ts-check
// Soft update: apply a changed source in place (no reload, no flash, scroll kept), then tell the
// Studio what changed so it can point at it. Anything unusual reloads the frame.
import { post, st } from './state.js';
import { isUi, labelOf, pick, rectOf } from './target.js';
import { reportPins, reportSelection, reportSize } from './select.js';
import { morph, sheetPath } from './morph.js';

/** Tell the Studio the overlay is ready (soft: after an in-place update). */
export const ready = (/** @type {unknown} */ soft) => {
  reportSize();
  post('dfw:ready', { href: location.pathname + location.search, title: document.title, soft: soft === true });
};

/** Reload the stylesheets whose file changed: the new one loads next to the old one, which goes when it is ready. */
function swapSheets(/** @type {string[]} */ files) {
  for (const link of /** @type {HTMLLinkElement[]} */ ([...document.querySelectorAll('link[rel~="stylesheet"][href]')])) {
    const p = sheetPath(link.getAttribute('href'));
    if (!files.some((f) => p.endsWith(`/${f}`))) continue;
    const next = /** @type {HTMLLinkElement} */ (link.cloneNode());
    next.setAttribute('href', `${p}?v=${Date.now()}`);
    const done = () => {
      link.remove();
      reportSize();
      reportSelection();
      if (st.pins.length) reportPins();
    };
    next.addEventListener('load', done);
    next.addEventListener('error', done);
    link.after(next);
  }
}

/** dfw:soft-reload: fetch this page's source again and morph the document into it. */
export async function softReload(/** @type {{ files?: string[], agent?: boolean }} */ d) {
  try {
    const res = await fetch(location.href, { cache: 'no-store' });
    if (!res.ok) throw new Error(String(res.status));
    const doc = new DOMParser().parseFromString(await res.text(), 'text/html');
    // A page script may have built DOM the source does not have: patching would remove it and not run the script again.
    if ([...doc.scripts].some((s) => !s.hasAttribute('data-dfw-overlay'))) throw new Error('page script');
    /** @type {Set<Node>} */
    const changed = new Set();
    const keep = st.editing ? st.editing.el : null;
    morph(document.head, doc.head, changed, keep);
    morph(document.body, doc.body, changed, keep);
    swapSheets(d.files || []);
    st.hoverEl = null;
    st.layerEls = [];
    st.layerNode = new Map();
    st.tokenCache = null;
    if (st.selected && !st.selected.isConnected) st.selected = null;
    ready(true);
    // What changed, for the Studio to point at: the outermost changed elements that are on screen.
    const els = /** @type {HTMLElement[]} */ ([...changed]).filter(
      (el) =>
        el.nodeType === 1 &&
        el.isConnected &&
        el !== document.body &&
        !document.head.contains(el) &&
        !isUi(el) &&
        el.getClientRects().length,
    );
    const tops = els.filter((el) => !els.some((o) => o !== el && o.contains(el)));
    post('dfw:changed', {
      agent: !!d.agent,
      count: tops.length,
      items: tops.slice(0, 4).map((el) => ({ rect: rectOf(el), label: labelOf(pick(el, false) || el) })),
    });
  } catch {
    location.reload(); // anything unusual (scripts, parse problems): the plain reload is always right
  }
}
