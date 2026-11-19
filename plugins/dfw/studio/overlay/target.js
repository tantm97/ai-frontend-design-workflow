// @ts-check
// What an element is, for the Studio: the element a click picks, its label, its layer status, and the
// comment target info of the feedback contract (selector, role and name, text, styles). Also shared
// with live.js in a running app (window.__dfwTargetInfo).

const LANDMARK_TAGS = new Set(['HEADER', 'NAV', 'MAIN', 'ASIDE', 'FOOTER', 'FORM', 'DIALOG']);
const LANDMARK_ROLES = new Set([
  'banner',
  'navigation',
  'main',
  'complementary',
  'contentinfo',
  'region',
  'search',
  'form',
  'dialog',
  'alertdialog',
  'toolbar',
  'tablist',
]);

/** Part of the overlay's own UI (never selected, never in layers). */
export const isUi = (/** @type {any} */ el) => el && el.closest && el.closest('[data-dfw-ui]');
/** An element shown in the Layers pane: ids, components, landmarks, labelled sections. */
export const isLayer = (/** @type {Element} */ el) =>
  el.hasAttribute('data-dfw-id') ||
  el.hasAttribute('data-dfw-component') ||
  LANDMARK_TAGS.has(el.tagName) ||
  LANDMARK_ROLES.has(el.getAttribute('role')) ||
  (el.tagName === 'SECTION' && (el.hasAttribute('aria-label') || el.hasAttribute('aria-labelledby')));

/** @returns {[number, number, number, number]} */
export const rectOf = (/** @type {Element} */ el) => {
  const r = el.getBoundingClientRect();
  return [r.left, r.top, r.width, r.height];
};

/**
 * The element a click or hover means: the nearest data-dfw-id or component, or the exact element.
 * @param {any} el
 * @param {boolean} exact
 * @returns {HTMLElement | null}
 */
export function pick(el, exact) {
  if (!el || el.nodeType !== 1 || el === document.documentElement || el === document.body || isUi(el)) return null;
  if (exact) return el;
  return el.closest('[data-dfw-id],[data-dfw-component]') || el;
}

/** Name shown on the Studio's selection box. */
export function labelOf(/** @type {HTMLElement} */ el) {
  if (el.dataset.dfwComponent) return `${el.dataset.dfwComponent}${el.dataset.dfwVariant ? ` · ${el.dataset.dfwVariant}` : ''}`;
  return el.dataset.dfwId || el.tagName.toLowerCase();
}

/** A selector for the element: its data-dfw-id, else a path from the nearest id. */
export function cssPath(/** @type {any} */ el) {
  if (el.dataset && el.dataset.dfwId) return `[data-dfw-id="${el.dataset.dfwId}"]`;
  const parts = [];
  let n = el;
  while (n && n.nodeType === 1 && n !== document.body) {
    if (n.id) {
      parts.unshift(`#${CSS.escape(n.id)}`);
      break;
    }
    const anc = n.parentElement && n.parentElement.closest('[data-dfw-id]');
    let sel = n.tagName.toLowerCase();
    const sibs = n.parentElement ? [...n.parentElement.children].filter((c) => c.tagName === n.tagName) : [];
    if (sibs.length > 1) sel += `:nth-of-type(${sibs.indexOf(n) + 1})`;
    parts.unshift(sel);
    if (anc && n.parentElement === anc) {
      parts.unshift(`[data-dfw-id="${anc.dataset.dfwId}"]`);
      break;
    }
    n = n.parentElement;
  }
  return parts.join(' > ');
}

const IMPLICIT_ROLE = {
  BUTTON: 'button',
  A: 'link',
  INPUT: 'textbox',
  SELECT: 'combobox',
  TEXTAREA: 'textbox',
  NAV: 'navigation',
  HEADER: 'banner',
  FOOTER: 'contentinfo',
  MAIN: 'main',
  FORM: 'form',
  TABLE: 'table',
  IMG: 'img',
  H1: 'heading',
  H2: 'heading',
  H3: 'heading',
  H4: 'heading',
  DIALOG: 'dialog',
  UL: 'list',
  OL: 'list',
  LI: 'listitem',
  SECTION: 'region',
  ASIDE: 'complementary',
};

/** Role and accessible name, roughly as a screen reader would get them. */
function a11y(/** @type {any} */ el) {
  let role = el.getAttribute('role') || IMPLICIT_ROLE[el.tagName] || null;
  if (el.tagName === 'INPUT') {
    const t = (el.getAttribute('type') || 'text').toLowerCase();
    role = { checkbox: 'checkbox', radio: 'radio', search: 'searchbox', button: 'button', submit: 'button', range: 'slider' }[t] || role;
  }
  let name = el.getAttribute('aria-label') || el.getAttribute('alt') || el.getAttribute('title') || '';
  if (!name && el.getAttribute('aria-labelledby'))
    name = el
      .getAttribute('aria-labelledby')
      .split(/\s+/)
      .map((/** @type {string} */ id) => document.getElementById(id)?.textContent || '')
      .join(' ');
  if (!name && el.id) name = document.querySelector(`label[for="${CSS.escape(el.id)}"]`)?.textContent || '';
  if (!name && el.getAttribute('placeholder')) name = el.getAttribute('placeholder');
  if (!name) name = el.textContent || '';
  return { role, name: name.replace(/\s+/g, ' ').trim().slice(0, 120) };
}

/** Comment target info (feedback contract). */
export function info(/** @type {any} */ el) {
  const r = el.getBoundingClientRect();
  const cs = getComputedStyle(el);
  return {
    dfwId: el.dataset?.dfwId || el.closest('[data-dfw-id]')?.dataset.dfwId || null,
    exact: !el.dataset?.dfwId,
    selector: cssPath(el),
    tag: el.tagName.toLowerCase(),
    component: el.dataset.dfwComponent || null,
    bbox: [Math.round(r.left + scrollX), Math.round(r.top + scrollY), Math.round(r.width), Math.round(r.height)],
    viewportBbox: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
    a11y: a11y(el),
    text: (el.innerText || el.value || '').replace(/\s+/g, ' ').trim().slice(0, 160),
    styles: {
      color: cs.color,
      background: cs.backgroundColor,
      font: `${cs.fontWeight} ${cs.fontSize}/${cs.lineHeight} ${cs.fontFamily.split(',')[0]}`,
      padding: cs.padding,
      radius: cs.borderRadius,
    },
    scroll: [Math.round(scrollX), Math.round(scrollY)],
  };
}
