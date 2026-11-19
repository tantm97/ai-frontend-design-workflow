// @ts-check
// The DOM diff of the in-place update (studio-ui.md U8): make the live document equal to a freshly
// parsed copy of its source, touching only what differs, so scroll, selection and focus stay. Nodes
// are matched by data-dfw-id (or id), else by position; identical nodes are left alone. Scripts are
// never added, removed or changed this way: morph throws and the caller reloads the frame.
// Works on any DOM (tests/unit/studio-overlay-morph.test.mjs runs it on linkedom).

/** Match key of a node: its data-dfw-id, else its id. */
export const keyOf = (/** @type {Node} */ n) =>
  n.nodeType === 1 ? /** @type {Element} */ (n).getAttribute('data-dfw-id') || /** @type {Element} */ (n).id || null : null;
const sameKind = (/** @type {Node} */ a, /** @type {Node} */ b) => a.nodeType === b.nodeType && a.nodeName === b.nodeName;
/** Path of a stylesheet URL (without the cache key a swap adds). */
export const sheetPath = (/** @type {string} */ href) => {
  try {
    return new URL(href, location.href).pathname;
  } catch {
    return href;
  }
};
const hasScript = (/** @type {Node} */ n) =>
  n.nodeType === 1 && (n.nodeName === 'SCRIPT' || /** @type {Element} */ (n).querySelector('script'));

/**
 * Copy the attributes of `to` onto `from` (keeping contenteditable of a text edit in progress).
 * @param {Element} from
 * @param {Element} to
 * @param {Set<Node>} changed
 */
function syncAttrs(from, to, changed) {
  // A stylesheet link is refreshed by swapSheets (its href only differs by the cache key).
  if (from.nodeName === 'LINK' && sheetPath(from.getAttribute('href') || '') === sheetPath(to.getAttribute('href') || '')) return;
  let diff = false;
  for (const a of [...from.attributes]) {
    if (a.name === 'contenteditable' || to.hasAttribute(a.name)) continue;
    from.removeAttribute(a.name);
    diff = true;
  }
  for (const a of [...to.attributes]) {
    if (from.getAttribute(a.name) === a.value) continue;
    from.setAttribute(a.name, a.value);
    diff = true;
  }
  if (diff) changed.add(from);
}

/**
 * Make `from` equal to `to`, touching only what differs. Changed elements are collected. Scripts never
 * change this way (throws). Elements with data-dfw-ui (the overlay's own) are left in place.
 * @param {Element} from the live element
 * @param {Element} to the same element in the new source
 * @param {Set<Node>} changed
 * @param {Element | null} [keep] an element the user is editing right now: left exactly as it is
 */
export function morph(from, to, changed, keep = null) {
  if (keep && from === keep) return; // never overwrite text the user is typing
  syncAttrs(from, to, changed);
  const olds = [...from.childNodes].filter((n) => !(n.nodeType === 1 && /** @type {Element} */ (n).hasAttribute('data-dfw-ui')));
  const keyed = new Map();
  for (const o of olds) {
    const k = keyOf(o);
    if (k && !keyed.has(k)) keyed.set(k, o);
  }
  const used = new Set();
  /** @type {[ChildNode, ChildNode | null][]} */
  const plan = [];
  let i = 0;
  for (const n of [...to.childNodes]) {
    const k = keyOf(n);
    let match = null;
    if (k) {
      const o = keyed.get(k);
      if (o && !used.has(o) && sameKind(o, n)) match = o;
    } else {
      while (i < olds.length && used.has(olds[i])) i++;
      // The same node a little further on (something was inserted or removed before it), else the next one of its kind.
      for (let j = i; j < Math.min(olds.length, i + 8) && !match; j++)
        if (!used.has(olds[j]) && !keyOf(olds[j]) && olds[j].isEqualNode(n)) match = olds[j];
      if (!match && olds[i] && sameKind(olds[i], n) && !keyOf(olds[i])) match = olds[i];
    }
    if (match) used.add(match);
    plan.push([n, match]);
  }
  for (const o of olds) {
    if (used.has(o)) continue;
    if (hasScript(o)) throw new Error('script removed');
    o.remove();
    if (o.nodeType === 1 || (o.nodeValue || '').trim()) changed.add(from);
  }
  /** @type {ChildNode | null} */
  let prev = null;
  for (const [n, match] of plan) {
    const want = prev ? prev.nextSibling : from.firstChild;
    let node = match;
    if (!node) {
      if (hasScript(n)) throw new Error('script added');
      node = /** @type {ChildNode} */ (from.ownerDocument.importNode(n, true));
      from.insertBefore(node, want);
      if (node.nodeType === 1) changed.add(node);
      else if ((node.nodeValue || '').trim()) changed.add(from);
    } else {
      if (node !== want) from.insertBefore(node, want);
      if (node.nodeType !== 1) {
        if (node.nodeValue !== n.nodeValue) {
          node.nodeValue = n.nodeValue;
          if (node.nodeType === 3) changed.add(from);
        }
      } else if (node.nodeName === 'SCRIPT') {
        if (/** @type {Element} */ (node).outerHTML !== /** @type {Element} */ (n).outerHTML) throw new Error('script changed');
      } else if (!node.isEqualNode(n)) morph(/** @type {Element} */ (node), /** @type {Element} */ (n), changed, keep);
    }
    prev = node;
  }
}
