// @ts-check
// Layers: the tree of layer elements (ids, components, landmarks) the Studio's Layers pane shows, and
// arrow-key navigation through it.
import { st } from './state.js';
import { isLayer } from './target.js';
import { NAV_DIR } from '../shared/enums.js';

/**
 * @typedef {{ i: number, id: string | null, comp: string | null, variant: string | null, size: string | null, tag: string,
 *   name: string | null, hidden: boolean, children: LayerNode[] }} LayerNode
 */

/** Walk the body; at most about 3,000 layers. @returns {LayerNode[]} */
export function buildLayers() {
  st.layerEls = [];
  st.layerNode = new Map();
  const walk = (/** @type {Element} */ el, /** @type {LayerNode[]} */ out, /** @type {HTMLElement | null} */ parentNode) => {
    for (const c of /** @type {HTMLCollectionOf<HTMLElement>} */ (el.children)) {
      if (c.hasAttribute('data-dfw-ui') || /^(SCRIPT|STYLE|TEMPLATE|LINK|META|NOSCRIPT)$/.test(c.tagName)) continue;
      if (st.layerEls.length > 3000) return;
      if (isLayer(c)) {
        /** @type {LayerNode} */
        const node = {
          i: st.layerEls.length,
          id: c.dataset.dfwId || null,
          comp: c.dataset.dfwComponent || null,
          variant: c.dataset.dfwVariant || null,
          size: c.dataset.dfwSize || null,
          tag: c.tagName.toLowerCase(),
          name: c.getAttribute('aria-label') || null,
          hidden: c.getClientRects().length === 0,
          children: [],
        };
        st.layerEls.push(c);
        st.layerNode.set(c, { node, parent: parentNode });
        out.push(node);
        walk(c, node.children, c);
      } else walk(c, out, parentNode);
    }
  };
  /** @type {LayerNode[]} */
  const tree = [];
  if (document.body) walk(document.body, tree, null);
  return tree;
}

function layerAncestor(/** @type {Element} */ el) {
  for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) if (st.layerNode.has(n)) return n;
  return null;
}

/**
 * The layer an arrow key moves to from the selection: up = parent, down = first child, left/right =
 * previous/next sibling. Null when there is none.
 * @param {string} dir
 * @returns {HTMLElement | null}
 */
export function navLayer(dir) {
  const selected = st.selected;
  if (!st.layerEls.length || !st.layerNode.has(selected)) buildLayers();
  const cur = st.layerNode.has(selected) ? selected : layerAncestor(selected);
  if (!cur) return null;
  const meta = st.layerNode.get(cur);
  const sibs = meta.parent
    ? st.layerNode.get(meta.parent).node.children
    : [...st.layerNode.values()].filter((x) => !x.parent).map((x) => x.node);
  const idx = sibs.findIndex((/** @type {LayerNode} */ n) => st.layerEls[n.i] === cur);
  let next = null;
  if (dir === NAV_DIR.UP) next = cur !== selected ? cur : meta.parent;
  else if (dir === NAV_DIR.DOWN) next = meta.node.children[0] ? st.layerEls[meta.node.children[0].i] : null;
  else if (dir === NAV_DIR.LEFT) next = sibs[idx - 1] ? st.layerEls[sibs[idx - 1].i] : null;
  else if (dir === NAV_DIR.RIGHT) next = sibs[idx + 1] ? st.layerEls[sibs[idx + 1].i] : null;
  return next;
}
