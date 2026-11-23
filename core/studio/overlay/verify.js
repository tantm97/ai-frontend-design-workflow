// @ts-check
// After a resize was written and the frame reloaded: check the element's computed value really
// changed. If a screen rule still wins (an id selector, inline style or !important), name it instead
// of failing silently.
import { post } from './state.js';
import { styleProp } from './layout.js';
import { MSG } from '#studio/shared/protocol.js';
import { LAYOUT_PROP } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

/**
 * Style rules of a sheet (recursing into @media that matches now, @layer, @supports) for which pick(rule) is truthy.
 * @param {CSSRuleList | any[]} rules
 * @param {(r: any) => any} pick
 * @param {any[]} [out]
 * @returns {any[]}
 */
function rulesOf(rules, pick, out = []) {
  for (const r of /** @type {any[]} */ ([...rules])) {
    if (r.cssRules && !r.selectorText) {
      if (!r.media || matchMedia(r.media.mediaText).matches) rulesOf(r.cssRules, pick, out);
    } else if (r.selectorText && pick(r)) out.push(r);
  }
  return out;
}

/** Check a resize the Studio saved (dfw:layout-verify) and report a conflicting rule. */
export function verifyLayout(/** @type {{ id: string, prop: string }} */ d) {
  const el = /** @type {HTMLElement | null} */ (document.querySelector(`[data-dfw-id="${CSS.escape(d.id)}"]`));
  const sheet = /** @type {HTMLStyleElement | null} */ (document.querySelector('style[data-dfw-layout]'))?.sheet;
  if (!el || !sheet || !styleProp[d.prop]) return;
  const own = rulesOf(sheet.cssRules, (r) => r.style.getPropertyValue(d.prop) && el.matches(r.selectorText));
  if (!own.length) return; // not at this width (or already reset)
  const read = () =>
    (d.prop === LAYOUT_PROP.GRID_COLUMN ? ['gridColumnStart', 'gridColumnEnd'] : [styleProp[d.prop]])
      .map((k) => getComputedStyle(el)[k])
      .join(' / ');
  const prev = el.style.getPropertyValue(d.prop);
  const prevPrio = el.style.getPropertyPriority(d.prop);
  const now = read();
  el.style.setProperty(d.prop, own.at(-1).style.getPropertyValue(d.prop), 'important');
  const ideal = read();
  el.style.setProperty(d.prop, prev, prevPrio);
  if (now === ideal) return;
  const by = conflictFor(el, d.prop, sheet);
  post(MSG.LAYOUT_CONFLICT, { id: d.id, prop: d.prop, selector: by });
  post(MSG.TOAST, {
    message: TEXT.layoutConflict(d.id, d.prop, by),
  });
}

const LONGHANDS = {
  [LAYOUT_PROP.GRID_COLUMN]: ['grid-column', 'grid-column-start', 'grid-column-end', 'grid-area'],
  [LAYOUT_PROP.WIDTH]: ['width'],
  [LAYOUT_PROP.MIN_HEIGHT]: ['min-height'],
};

/**
 * Rough selector specificity [ids, classes/attributes/pseudo-classes, types], only to rank conflicting rules.
 * @param {string} sel
 * @returns {[number, number, number]}
 */
export function specificity(sel) {
  const s = sel
    .replace(/:where\([^)]*\)/g, '')
    .replace(/:(?:not|is|has)\(/g, '(')
    .replace(/"[^"]*"|'[^']*'/g, '');
  return [
    (s.match(/#[\w-]/g) || []).length,
    (s.match(/\.[\w-]|\[|:(?!:)[\w-]/g) || []).length,
    (s.replace(/[#.:][\w-]+|\[[^\]]*\]/g, ' ').match(/(^|[\s>+~(,])[a-z][\w-]*/gi) || []).length,
  ];
}

/**
 * Selector of the rule that beats the dfw layout rule for prop on el (inline style, then !important, then specificity).
 * @param {HTMLElement} el
 * @param {string} prop
 * @param {CSSStyleSheet} dfwSheet
 */
function conflictFor(el, prop, dfwSheet) {
  const names = LONGHANDS[prop];
  if (names.some((n) => el.style.getPropertyValue(n))) return 'style attribute';
  const hits = [];
  const matches = (/** @type {string} */ x) => {
    try {
      return el.matches(x);
    } catch {
      return false;
    }
  };
  for (const sh of document.styleSheets) {
    if (sh === dfwSheet) continue;
    let rules = [];
    try {
      rules = rulesOf(sh.cssRules, (r) => names.some((n) => r.style.getPropertyValue(n)));
    } catch {} // cross-origin sheet
    for (const r of rules) {
      const sel = r.selectorText
        .split(',')
        .map((/** @type {string} */ x) => x.trim())
        .find(matches);
      const n = names.find((x) => r.style.getPropertyValue(x));
      if (sel) hits.push({ sel, imp: r.style.getPropertyPriority(n) === 'important', sp: specificity(sel) });
    }
  }
  hits.sort((a, b) => Number(b.imp) - Number(a.imp) || b.sp[0] - a.sp[0] || b.sp[1] - a.sp[1] || b.sp[2] - a.sp[2]);
  return hits[0]?.sel || null;
}
