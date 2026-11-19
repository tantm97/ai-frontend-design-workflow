// @ts-check
// Design tokens: reverse-map computed values to token names, for the Studio's inspector. Each token's
// CSS variable is resolved once per theme by probe elements; a style value then maps to the tokens
// that produce it, filtered by family (spacing tokens for padding, radius tokens for radius...).
import { st } from './state.js';

const BASE = { color: 'rgb(1, 2, 3)', paddingLeft: '0px', fontWeight: '100', boxShadow: 'none', fontSize: '3.3px' };

function buildTokenCache() {
  const box = document.createElement('div');
  box.setAttribute('data-dfw-ui', '');
  box.style.cssText = `position:absolute;left:-99999px;top:0;visibility:hidden;pointer-events:none;color:${BASE.color};font-size:${BASE.fontSize};font-weight:${BASE.fontWeight};`;
  const probes = st.tokenVars.map((t) => {
    const d = document.createElement('div');
    const v = `var(${t.var})`;
    d.style.cssText = `color:${v};padding-left:${v};font-weight:${v};box-shadow:${v};font-size:${v};`;
    box.append(d);
    return d;
  });
  (document.body || document.documentElement).append(box);
  const maps = { color: new Map(), length: new Map(), fontWeight: new Map(), shadow: new Map(), fontSize: new Map() };
  const add = (/** @type {Map<string, string[]>} */ m, /** @type {string} */ k, /** @type {string} */ name) =>
    (m.get(k) || m.set(k, []).get(k)).push(name);
  probes.forEach((d, i) => {
    const cs = getComputedStyle(d);
    const name = st.tokenVars[i].name;
    if (cs.color !== BASE.color) add(maps.color, cs.color, name);
    if (cs.paddingLeft !== BASE.paddingLeft) add(maps.length, cs.paddingLeft, name);
    if (cs.fontWeight !== BASE.fontWeight) add(maps.fontWeight, cs.fontWeight, name);
    if (cs.boxShadow !== BASE.boxShadow) add(maps.shadow, cs.boxShadow, name);
    if (cs.fontSize !== BASE.fontSize) add(maps.fontSize, cs.fontSize, name);
  });
  box.remove();
  return { key: `${document.documentElement.dataset.theme || ''}`, maps };
}

/** Line-height tokens resolve against a font size, so they are probed per font size. */
function lineHeightMap(/** @type {string} */ fontSize) {
  const m = new Map();
  const box = document.createElement('div');
  box.setAttribute('data-dfw-ui', '');
  box.style.cssText = `position:absolute;left:-99999px;top:0;visibility:hidden;font-size:${fontSize};line-height:1.111;`;
  const base = document.createElement('div');
  box.append(base);
  const cand = st.tokenVars.filter((t) => /lead|line/i.test(t.name));
  const probes = cand.map((t) => {
    const d = document.createElement('div');
    d.style.lineHeight = `var(${t.var})`;
    box.append(d);
    return d;
  });
  (document.body || document.documentElement).append(box);
  const b = getComputedStyle(base).lineHeight;
  probes.forEach((d, i) => {
    const v = getComputedStyle(d).lineHeight;
    if (v !== b) (m.get(v) || m.set(v, []).get(v)).push(cand[i].name);
  });
  box.remove();
  return m;
}

/** Semantic names (no ".<digit>" step) first, then shorter names. */
export const semanticFirst = (/** @type {Iterable<string>} */ names) =>
  [...names].sort((a, b) => Number(/\.\d/.test(a)) - Number(/\.\d/.test(b)) || a.length - b.length);
/** Token name families per style group. */
const FAMILY = {
  padding: /^space\b|spacing/i,
  gap: /^space\b|spacing/i,
  margin: /^space\b|spacing/i,
  'border-radius': /radius|rounded/i,
  'font-size': /font|text|size/i,
  'font-weight': /weight|font/i,
  'line-height': /lead|line/i,
  'box-shadow': /shadow|elevation/i,
};

/**
 * Tokens whose value is `value`, of the group's family, semantic names first.
 * @param {string} group
 * @param {string} value
 * @param {Map<string, string[]>} map value -> token names
 */
export function match(group, value, map) {
  const names = map.get(value) || [];
  const fam = FAMILY[group];
  const ok = fam ? names.filter((n) => fam.test(n)) : names;
  return semanticFirst(ok);
}

/**
 * The styles of an element the inspector shows, each with the tokens it maps to.
 * @param {Element} el
 * @returns {{ group: string, prop: string, value: string, tokens: string[] }[]}
 */
export function inspect(el) {
  if (!st.tokenCache || st.tokenCache.key !== `${document.documentElement.dataset.theme || ''}`) st.tokenCache = buildTokenCache();
  const M = st.tokenCache.maps;
  const cs = getComputedStyle(el);
  /** @type {{ group: string, prop: string, value: string, tokens: string[] }[]} */
  const rows = [];
  const row = (
    /** @type {string} */ group,
    /** @type {string} */ prop,
    /** @type {string} */ value,
    /** @type {Map<string, string[]>} */ map,
  ) => rows.push({ group, prop, value, tokens: match(group, value, map) });
  const transparent = (/** @type {string} */ c) => c === 'rgba(0, 0, 0, 0)' || c === 'transparent';
  row('color', 'color', cs.color, M.color);
  if (!transparent(cs.backgroundColor)) row('background-color', 'background-color', cs.backgroundColor, M.color);
  if (parseFloat(cs.borderTopWidth) > 0 && cs.borderTopStyle !== 'none' && !transparent(cs.borderTopColor))
    row('border-color', 'border-color', cs.borderTopColor, M.color);
  row('font-size', 'font-size', cs.fontSize, M.fontSize);
  row('font-weight', 'font-weight', cs.fontWeight, M.fontWeight);
  if (cs.lineHeight !== 'normal') row('line-height', 'line-height', cs.lineHeight, lineHeightMap(cs.fontSize));
  const sides = (/** @type {string} */ group, /** @type {string[]} */ names) => {
    const v = names.map((n) => cs.getPropertyValue(n));
    if (v.every((x) => x === '0px' || x === 'auto')) return;
    if (v.every((x) => x === v[0])) return row(group, group, v[0], M.length);
    if (v[0] === v[2] && v[1] === v[3] && group !== 'border-radius') {
      if (v[0] !== '0px') row(group, `${group}-block`, v[0], M.length);
      if (v[1] !== '0px') row(group, `${group}-inline`, v[1], M.length);
      return;
    }
    names.forEach((n, i) => v[i] !== '0px' && v[i] !== 'auto' && row(group, n, v[i], M.length));
  };
  sides('padding', ['padding-top', 'padding-right', 'padding-bottom', 'padding-left']);
  if (/flex|grid/.test(cs.display)) {
    const rg = cs.rowGap;
    const cg = cs.columnGap;
    if (rg === cg) {
      if (rg !== 'normal' && rg !== '0px') row('gap', 'gap', rg, M.length);
    } else {
      if (rg !== 'normal' && rg !== '0px') row('gap', 'row-gap', rg, M.length);
      if (cg !== 'normal' && cg !== '0px') row('gap', 'column-gap', cg, M.length);
    }
  }
  sides('margin', ['margin-top', 'margin-right', 'margin-bottom', 'margin-left']);
  sides('border-radius', ['border-top-left-radius', 'border-top-right-radius', 'border-bottom-right-radius', 'border-bottom-left-radius']);
  if (cs.boxShadow !== 'none') row('box-shadow', 'box-shadow', cs.boxShadow, M.shadow);
  return rows;
}
