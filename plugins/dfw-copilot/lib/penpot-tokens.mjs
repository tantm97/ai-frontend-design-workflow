// Penpot tokens bridge: dfw DTCG 2025.10 tokens <-> the JSON Penpot's Tokens panel imports and exports.
// Penpot is an optional bridge; design/tokens/*.json stays the source of truth.
//
// Penpot's token file (single-file form, as written by "Export" in the Tokens panel) is a
// Tokens Studio-style multi-set document:
//   { "<set name>": { <nested token groups with $value/$type/$description> },
//     "$themes":   [{ "name", "group", "description", "isSource", "selectedTokenSets": { "<set>": "enabled" } }],
//     "$metadata": { "tokenSetOrder": [...], "activeThemes": ["<group>/<name>"], "activeSets": [...] } }
// Values are strings ("16px", "#0066ff", "400"); type names are Penpot's (fontFamilies, fontSizes,
// fontWeights, borderRadius, borderWidth, spacing, sizing, ...). See references/penpot.md for sources.
//
// dfw mapping: design/tokens/tokens.json -> set "base"; design/tokens/themes/<name>.json -> set
// "themes/<name>"; one Penpot theme per dfw theme in group "dfw" ("default" = base only).
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, readJson, writeJson, DfwError } from './util.mjs';
import { oklchToHex } from './color.mjs';

const BASE_SET = 'base';
const THEME_PREFIX = 'themes/';
const THEME_GROUP = 'dfw';

// Token and set names come from a Penpot file (untrusted): a path segment like "__proto__" would
// write into Object.prototype instead of the tree.
const RESERVED = new Set(['__proto__', 'constructor', 'prototype']);
/** "color.brand.500" -> ['color', 'brand', '500'], or null when a segment is reserved. */
function tokenPath(name) {
  const parts = String(name).split('.');
  return parts.some((p) => RESERVED.has(p)) ? null : parts;
}

const isAlias = (v) => typeof v === 'string' && /\{[^{}]+\}/.test(v);
const round = (n, d = 4) => Math.round(n * 10 ** d) / 10 ** d;
const fmt = (n) => String(round(n, 4));
const clone = (x) => (x === undefined ? x : JSON.parse(JSON.stringify(x)));
const canon = (x) => (Array.isArray(x) ? x.map(canon) : x && typeof x === 'object' ? Object.fromEntries(Object.keys(x).sort().map((k) => [k, canon(x[k])])) : x);
const same = (a, b) => JSON.stringify(canon(a)) === JSON.stringify(canon(b));

/** Flatten a DTCG tree: Map(name -> { type, value, description }) in document order. */
export function flattenTokens(tree) {
  const out = new Map();
  const walk = (node, trail, type) => {
    if (typeof node !== 'object' || node === null || Array.isArray(node)) return;
    const t = node.$type || type;
    if ('$value' in node) {
      out.set(trail.join('.'), { type: t, value: node.$value, description: node.$description });
      return;
    }
    for (const [k, v] of Object.entries(node)) if (!k.startsWith('$')) walk(v, [...trail, k], t);
  };
  walk(tree, [], undefined);
  return out;
}

/* ------------------------------------------------------------------ dfw value -> Penpot string */

function colorOut(v, note) {
  if (typeof v === 'string') return v;
  if (!v || typeof v !== 'object') throw new Error(`not a colour: ${JSON.stringify(v)}`);
  const a = v.alpha ?? 1;
  let rgb;
  if (v.colorSpace === 'srgb' && Array.isArray(v.components) && v.components.every((x) => typeof x === 'number')) {
    if (v.hex && a === 1) return v.hex.toLowerCase();
    rgb = v.components.map((x) => Math.round(Math.min(1, Math.max(0, x)) * 255));
  } else if (v.colorSpace === 'oklch' && v.components?.every((x) => typeof x === 'number')) {
    const hex = oklchToHex({ l: v.components[0], c: v.components[1], h: v.components[2] });
    note(`oklch converted to sRGB ${hex}`);
    if (a === 1) return hex;
    rgb = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16));
  } else if (v.hex) {
    note(`${v.colorSpace} colour exported through its hex fallback`);
    if (a === 1) return v.hex.toLowerCase();
    rgb = [1, 3, 5].map((i) => parseInt(v.hex.slice(i, i + 2), 16));
  } else throw new Error(`colour space "${v.colorSpace}" has no sRGB form (add a "hex" fallback)`);
  if (a === 1) return '#' + rgb.map((x) => x.toString(16).padStart(2, '0')).join('');
  return `rgba(${rgb.join(', ')}, ${fmt(a)})`;
}

// dfw wraps Penpot math in calc() so the CSS stays valid; Penpot gets the bare expression back.
const uncalc = (s) => String(s).replace(/^calc\((.*)\)$/s, '$1');
const isExpression = (s) => typeof s === 'string' && /[-+*/%^]|\b(round|ceil|floor|min|max|abs|sqrt|pow|log|exp)\(/.test(s.replace(/\{[^{}]+\}/g, 'x').replace(/^-/, ''));

function dimOut(v, note, { bare = false } = {}) {
  if (typeof v === 'string' || typeof v === 'number') return uncalc(v);
  if (!v || typeof v !== 'object' || typeof v.value !== 'number') throw new Error(`not a dimension: ${JSON.stringify(v)}`);
  if (v.unit === 'px') return bare ? fmt(v.value) : `${fmt(v.value)}px`;
  note(`unit "${v.unit}": Penpot works in px; check the value after import`);
  return `${fmt(v.value)}${v.unit}`;
}

function shadowOut(v, note) {
  if (typeof v === 'string') return v;
  const one = (s) => {
    if (typeof s === 'string') return s;
    return { color: colorOut(s.color, note), offsetX: dimOut(s.offsetX, note, { bare: true }), offsetY: dimOut(s.offsetY, note, { bare: true }), blur: dimOut(s.blur, note, { bare: true }), spread: dimOut(s.spread ?? { value: 0, unit: 'px' }, note, { bare: true }), inset: Boolean(s.inset) };
  };
  return Array.isArray(v) ? v.map(one) : one(v);
}

const TYPO_OUT = { fontFamily: 'fontFamilies', fontSize: 'fontSizes', fontWeight: 'fontWeights', letterSpacing: 'letterSpacing', lineHeight: 'lineHeights', textCase: 'textCase', textDecoration: 'textDecoration' };
const TEXT_CASES = ['none', 'uppercase', 'lowercase', 'capitalize'];
const TEXT_DECORATIONS = ['underline', 'strike-through', 'line-through'];

/** Penpot type for a dfw dimension token, from its name (dfw has one "dimension" type, Penpot several). */
export function penpotDimensionType(name, value) {
  const n = name.toLowerCase();
  const px = !value || typeof value !== 'object' || value.unit === 'px';
  if (/(^|\.)font\.size(\.|$)/.test(n) || /(^|\.)font-?size(\.|$)/.test(n)) return px ? 'fontSizes' : 'dimension';
  if (/(^|\.)(tracking|letter-?spacing)(\.|$)/.test(n)) return px ? 'letterSpacing' : 'dimension';
  if (/(^|\.)(radius|radii|rounded|corner)(\.|$)/.test(n)) return 'borderRadius';
  if (/(^|\.)(border|stroke|outline|focus)\.(width|width-[\w-]+)$/.test(n) || /(^|\.)(border-?width|stroke-?width)(\.|$)/.test(n)) return 'borderWidth';
  if (/(^|\.)(space|spacing|gap|gutter|inset|padding|margin)(\.|$)/.test(n)) return 'spacing';
  if (/(^|\.)(size|sizing|width|height|min-?width|max-?width)(\.|$)/.test(n)) return 'sizing';
  return 'dimension';
}

/**
 * Convert one dfw token to its Penpot form. Returns { type, value } or { unsupported: reason }.
 * `note(msg)` collects lossy conversions.
 */
export function tokenToPenpot(name, { type, value, description }, note = () => {}) {
  const res = (t, v) => ({ type: t, value: v, ...(description ? { description } : {}) });
  try {
    switch (type) {
      case 'color':
        return res('color', isAlias(value) ? value : colorOut(value, note));
      case 'dimension':
        return res(penpotDimensionType(name, value), typeof value === 'string' ? uncalc(value) : dimOut(value, note));
      case 'fontFamily':
        return res('fontFamilies', Array.isArray(value) ? value.join(', ') : String(value));
      case 'fontWeight':
        return res('fontWeights', String(value));
      case 'number':
        return res('number', uncalc(value));
      case 'duration': {
        note('duration exported as a Penpot "number" token in ms (Penpot has no duration type)');
        if (isAlias(value) || typeof value !== 'object') return res('number', uncalc(value));
        return res('number', fmt(value.unit === 's' ? value.value * 1000 : value.value));
      }
      case 'string':
        if (TEXT_CASES.includes(value)) return res('textCase', value);
        if (TEXT_DECORATIONS.includes(value)) return res('textDecoration', value === 'line-through' ? 'strike-through' : value);
        return { unsupported: 'Penpot has no free-text token type (only textCase/textDecoration values map)' };
      case 'shadow':
        return res('shadow', shadowOut(value, note));
      case 'typography': {
        if (isAlias(value)) return res('typography', value);
        const v = {};
        for (const [k, pk] of Object.entries(TYPO_OUT)) {
          if (value[k] === undefined) continue;
          const sub = value[k];
          if (isAlias(sub)) v[pk] = sub;
          else if (k === 'fontFamily') v[pk] = Array.isArray(sub) ? sub.join(', ') : String(sub);
          else if (k === 'fontSize' || k === 'letterSpacing') v[pk] = dimOut(sub, note);
          else v[pk] = typeof sub === 'object' ? dimOut(sub, note) : String(sub);
        }
        return res('typography', v);
      }
      default:
        return { unsupported: `Penpot has no "${type}" token type` };
    }
  } catch (e) {
    return { unsupported: e.message };
  }
}

/** Nested Penpot set from flat entries [name, {type, value, description}]; names with a reserved segment are left out. */
export function nest(entries) {
  const root = {};
  for (const [name, t] of entries) {
    const parts = tokenPath(name);
    if (!parts) continue;
    let node = root;
    for (const p of parts.slice(0, -1)) node = node[p] ??= {};
    node[parts.at(-1)] = { $value: t.value, $type: t.type, ...(t.description ? { $description: t.description } : {}) };
  }
  return root;
}

/**
 * dfw tokens -> Penpot token file.
 * input: { base: <tokens.json tree>, themes: { <name>: <tree> } }
 * Returns { json, report: { exported, unsupported: [{set, token, type, reason}], lossy: [{set, token, note}] } }
 */
export function dfwToPenpot({ base, themes = {} }) {
  const report = { exported: 0, sets: [], unsupported: [], lossy: [], skippedThemes: [] };
  const json = {};
  const sets = [[BASE_SET, base], ...Object.entries(themes).map(([n, t]) => [THEME_PREFIX + n, t])];
  for (const [set, tree] of sets) {
    const entries = [];
    for (const [name, tok] of flattenTokens(tree || {})) {
      const r = tokenToPenpot(name, tok, (note) => report.lossy.push({ set, token: name, note }));
      if (r.unsupported) report.unsupported.push({ set, token: name, type: tok.type, reason: r.unsupported });
      else {
        entries.push([name, r]);
        report.exported++;
      }
    }
    json[set] = nest(entries);
  }
  report.sets = sets.map(([s]) => s);
  json.$themes = [
    { name: 'default', group: THEME_GROUP, description: 'dfw base tokens (design/tokens/tokens.json)', isSource: false, selectedTokenSets: { [BASE_SET]: 'enabled' } },
    ...Object.keys(themes).map((n) => ({ name: n, group: THEME_GROUP, description: `dfw theme design/tokens/themes/${n}.json over the base`, isSource: false, selectedTokenSets: { [BASE_SET]: 'enabled', [THEME_PREFIX + n]: 'enabled' } })),
  ];
  json.$metadata = { tokenSetOrder: sets.map(([s]) => s), activeThemes: [`${THEME_GROUP}/default`], activeSets: [BASE_SET] };
  return { json, report };
}

/* ------------------------------------------------------------------ Penpot string -> dfw value */

const NAMED_WEIGHTS = { thin: 100, hairline: 100, extralight: 200, ultralight: 200, light: 300, regular: 400, normal: 400, book: 400, medium: 500, semibold: 600, demibold: 600, bold: 700, extrabold: 800, ultrabold: 800, black: 900, heavy: 900, extrablack: 950 };

function srgb(r, g, b, a = 1) {
  const comps = [r, g, b].map((x) => round(x / 255));
  const hex = '#' + [r, g, b].map((x) => Math.round(x).toString(16).padStart(2, '0')).join('');
  return a === 1 ? { colorSpace: 'srgb', components: comps, hex } : { colorSpace: 'srgb', components: comps, alpha: round(a), hex };
}

function hslToRgb(h, s, l) {
  s /= 100;
  l /= 100;
  const k = (n) => (n + h / 30) % 12;
  const f = (n) => l - s * Math.min(l, 1 - l) * Math.max(-1, Math.min(k(n) - 3, Math.min(9 - k(n), 1)));
  return [f(0), f(8), f(4)].map((x) => x * 255);
}

/** Parse a Penpot colour string. Returns a DTCG colour object, or null when it is not a plain colour. */
export function parseColor(s) {
  const t = String(s).trim();
  let m = /^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$/i.exec(t);
  if (m) {
    let h = m[1];
    if (h.length <= 4) h = [...h].map((c) => c + c).join('');
    const n = [0, 2, 4, 6].map((i) => (h.length > i ? parseInt(h.slice(i, i + 2), 16) : null));
    return srgb(n[0], n[1], n[2], n[3] === null ? 1 : n[3] / 255);
  }
  m = /^rgba?\(\s*([\d.]+)[\s,]+([\d.]+)[\s,]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/i.exec(t);
  if (m) {
    const a = m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : parseFloat(m[4]);
    return srgb(+m[1], +m[2], +m[3], a);
  }
  m = /^hsla?\(\s*([\d.]+)(?:deg)?[\s,]+([\d.]+)%[\s,]+([\d.]+)%(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/i.exec(t);
  if (m) {
    const a = m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : parseFloat(m[4]);
    const [r, g, b] = hslToRgb(+m[1], +m[2], +m[3]);
    return srgb(Math.round(r), Math.round(g), Math.round(b), a);
  }
  return null;
}

/** "16", "16px", "-2.5", "1.5rem" -> { value, unit } (unitless = px); null when it is an expression. */
export function parseDimension(s, defUnit = 'px') {
  if (typeof s === 'number') return { value: s, unit: defUnit };
  const m = /^\s*(-?\d*\.?\d+)\s*(px|rem|em)?\s*$/i.exec(String(s));
  return m ? { value: Number(m[1]), unit: (m[2] || defUnit).toLowerCase() } : null;
}

function parseNumber(s) {
  if (typeof s === 'number') return s;
  const m = /^\s*(-?\d*\.?\d+)\s*(%)?\s*$/.exec(String(s));
  if (!m) return null;
  return m[2] ? round(Number(m[1]) / 100) : Number(m[1]);
}

function parseWeight(s, warn) {
  if (typeof s === 'number') return s;
  const t = String(s).trim();
  if (/^\d+$/.test(t)) return Number(t);
  const key = t.toLowerCase().replace(/[\s_-]+/g, '');
  const italic = /italic$/.test(key);
  const w = NAMED_WEIGHTS[key.replace(/italic$/, '')] ?? (italic && key === 'italic' ? 400 : undefined);
  if (w === undefined) return null;
  if (italic) warn('italic is not part of a dfw fontWeight token: set font-style in the master');
  return w;
}

const splitFamilies = (s) => (Array.isArray(s) ? s : String(s).split(',')).map((x) => String(x).trim().replace(/^["']|["']$/g, '')).filter(Boolean);

const PENPOT_DIMENSION_TYPES = new Set(['dimension', 'spacing', 'sizing', 'borderRadius', 'borderWidth', 'strokeWidth', 'fontSizes', 'fontSize', 'letterSpacing']);

/**
 * Convert one Penpot token to dfw. `prefer` is the current dfw type for this name (keeps duration etc.).
 * Returns { type, value, description } or { unsupported: reason }; `warn(msg)` collects notes.
 */
export function tokenFromPenpot(name, { type, value, description }, prefer, warn = () => {}) {
  const res = (t, v) => ({ type: t, value: v, ...(description ? { description } : {}) });
  const raw = (t, why) => {
    if ((t === 'dimension' || t === 'number' || t === 'duration') && isExpression(value)) {
      warn(`math expression kept as calc(${value}) (check that the CSS is what you expect)`);
      return res(t === 'duration' ? 'duration' : t, `calc(${value})`);
    }
    warn(`${why}; kept as the raw string "${typeof value === 'string' ? value : JSON.stringify(value)}"`);
    return res(t, value);
  };
  // A number token named like a duration is a dfw duration (Penpot has no duration type).
  if (!prefer && type === 'number' && /(^|\.)(duration|delay)(\.|$)/.test(name)) prefer = 'duration';
  if (isAlias(value) && typeof value === 'string' && /^\{[^{}]+\}$/.test(value.trim())) {
    // A plain alias: keep it; the dfw type follows the current token or the Penpot type.
    const t = prefer || { color: 'color', fontFamilies: 'fontFamily', fontFamily: 'fontFamily', fontWeights: 'fontWeight', fontWeight: 'fontWeight', number: 'number', opacity: 'number', rotation: 'number', shadow: 'shadow', boxShadow: 'shadow', typography: 'typography', textCase: 'string', textDecoration: 'string' }[type] || (PENPOT_DIMENSION_TYPES.has(type) ? 'dimension' : null);
    if (!t) return { unsupported: `Penpot type "${type}" has no dfw equivalent` };
    return res(t, value.trim());
  }
  switch (type) {
    case 'color': {
      const c = parseColor(value);
      return c ? res('color', c) : raw('color', 'not a plain hex/rgb/hsl colour (expression?)');
    }
    case 'fontFamilies':
    case 'fontFamily':
      return res('fontFamily', splitFamilies(value));
    case 'fontWeights':
    case 'fontWeight': {
      const w = parseWeight(value, warn);
      return w === null ? raw('fontWeight', 'not a numeric or named weight') : res('fontWeight', w);
    }
    case 'number':
    case 'opacity':
    case 'rotation': {
      const n = parseNumber(value);
      if (n === null) return raw(prefer === 'duration' ? 'duration' : 'number', 'not a plain number');
      if (prefer === 'duration') return res('duration', { value: n, unit: 'ms' });
      if (type !== 'number') warn(`Penpot ${type} token stored as a dfw number`);
      return res('number', n);
    }
    case 'textCase':
    case 'textDecoration':
      return res('string', String(value));
    case 'shadow':
    case 'boxShadow': {
      if (typeof value === 'string') return raw('shadow', 'shadow given as a string');
      const one = (s) => {
        if (typeof s === 'string') return s;
        const d = (x) => (isAlias(x) ? x : parseDimension(x ?? 0) ?? x);
        const c = isAlias(s.color) ? s.color : parseColor(s.color) ?? s.color;
        const out = { color: c, offsetX: d(s.offsetX ?? s.x), offsetY: d(s.offsetY ?? s.y), blur: d(s.blur), spread: d(s.spread) };
        if (s.inset || s.type === 'innerShadow') out.inset = true;
        return out;
      };
      return res('shadow', Array.isArray(value) ? value.map(one) : one(value));
    }
    case 'typography': {
      if (typeof value !== 'object' || !value) return raw('typography', 'typography given as a string');
      const v = {};
      const pick = (...keys) => keys.map((k) => value[k]).find((x) => x !== undefined);
      const fam = pick('fontFamilies', 'fontFamily', 'font-family');
      if (fam !== undefined) v.fontFamily = isAlias(fam) ? fam : splitFamilies(fam);
      const size = pick('fontSizes', 'fontSize', 'font-size');
      if (size !== undefined) v.fontSize = isAlias(size) ? size : parseDimension(size) ?? size;
      const w = pick('fontWeights', 'fontWeight', 'font-weight');
      if (w !== undefined) v.fontWeight = isAlias(w) ? w : parseWeight(w, warn) ?? w;
      const ls = pick('letterSpacing', 'letter-spacing');
      if (ls !== undefined) v.letterSpacing = isAlias(ls) ? ls : parseDimension(ls) ?? ls;
      const lh = pick('lineHeights', 'lineHeight', 'line-height');
      if (lh !== undefined) v.lineHeight = isAlias(lh) ? lh : parseNumber(lh) ?? lh;
      const tc = pick('textCase', 'text-case');
      if (tc !== undefined) v.textCase = tc;
      const td = pick('textDecoration', 'text-decoration');
      if (td !== undefined) v.textDecoration = td;
      return res('typography', v);
    }
    default:
      if (PENPOT_DIMENSION_TYPES.has(type)) {
        const d = parseDimension(value);
        return d ? res('dimension', d) : raw('dimension', 'not a plain number with px/rem/em');
      }
      return { unsupported: `Penpot type "${type}" has no dfw equivalent` };
  }
}

/* ------------------------------------------------------------------ reading a Penpot file */

function walkSet(tree, inheritedType, reserved = []) {
  const out = new Map();
  const walk = (node, trail, type) => {
    if (!tokenPath(trail.join('.'))) {
      reserved.push(trail.join('.'));
      return;
    }
    if (typeof node !== 'object' || node === null || Array.isArray(node)) return;
    const t = node.$type || node.type || type;
    if ('$value' in node || ('value' in node && 'type' in node && trail.length)) {
      out.set(trail.join('.'), { type: t, value: '$value' in node ? node.$value : node.value, description: node.$description ?? node.description });
      return;
    }
    for (const [k, v] of Object.entries(node)) if (!k.startsWith('$')) walk(v, [...trail, ...k.split('.')], t);
  };
  walk(tree, [], inheritedType);
  return out;
}

/**
 * Read a Penpot token export: a single JSON file (multi-set or single set) or a folder of
 * set files with $themes.json / $metadata.json. Returns { sets: Map(setName -> tree), themes, metadata }.
 */
export function readPenpotFile(file) {
  const parse = (f) => {
    try {
      return JSON.parse(fs.readFileSync(f, 'utf8'));
    } catch (e) {
      throw new DfwError(`${f}: ${e.message}`);
    }
  };
  if (!fs.existsSync(file)) throw new DfwError(`${file} not found.`);
  if (fs.statSync(file).isDirectory()) {
    const sets = new Map();
    const walk = (dir, rel) => {
      for (const e of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
        if (e.isDirectory()) walk(path.join(dir, e.name), [...rel, e.name]);
        else if (e.name.endsWith('.json') && !e.name.startsWith('$')) sets.set([...rel, e.name.replace(/\.json$/, '')].join('/'), parse(path.join(dir, e.name)));
      }
    };
    walk(file, []);
    const themes = fs.existsSync(path.join(file, '$themes.json')) ? parse(path.join(file, '$themes.json')) : [];
    const metadata = fs.existsSync(path.join(file, '$metadata.json')) ? parse(path.join(file, '$metadata.json')) : {};
    return { sets, themes, metadata };
  }
  return parsePenpotJson(parse(file), path.basename(file, '.json'));
}

/** Like Penpot: a document without $themes/$metadata is one set; otherwise top-level keys are sets. */
export function parsePenpotJson(doc, fallbackName = BASE_SET) {
  if (!doc || typeof doc !== 'object') throw new DfwError('Not a Penpot token file (expected a JSON object).');
  if (!('$themes' in doc) && !('$metadata' in doc)) return { sets: new Map([[fallbackName, doc]]), themes: [], metadata: {}, single: true };
  const sets = new Map(Object.entries(doc).filter(([k]) => !k.startsWith('$')));
  return { sets, themes: doc.$themes || [], metadata: doc.$metadata || {} };
}

/** Which dfw file each Penpot set feeds: "base" -> tokens.json, "themes/<n>" (or "theme/<n>") -> themes/<n>.json. */
export function mapSets(parsed) {
  const order = [...new Set([...(parsed.metadata.tokenSetOrder || []), ...parsed.sets.keys()])].filter((s) => parsed.sets.has(s));
  const map = new Map();
  const skipped = [];
  // A lone "themes/<n>" set in a multi-set file is still a theme, never the base (it would overwrite tokens.json).
  const baseName = parsed.single ? order[0] : parsed.sets.has(BASE_SET) ? BASE_SET : order.find((s) => !/^themes?\//.test(s));
  for (const s of order) {
    const m = /^themes?\/(.+)$/.exec(s);
    if (s === baseName) map.set(s, { kind: 'base' });
    else if (m && /^[\w.-]+$/.test(m[1])) map.set(s, { kind: 'theme', name: m[1] });
    else skipped.push(s);
  }
  return { map, skipped, baseName };
}

/* ------------------------------------------------------------------ merging into dfw trees */

function inheritedTypeAt(tree, parts) {
  let t;
  let node = tree;
  for (const p of parts) {
    if (!node || typeof node !== 'object') return t;
    if (node.$type) t = node.$type;
    node = node[p];
  }
  return t;
}

function setToken(tree, name, tok) {
  const parts = tokenPath(name);
  if (!parts) return;
  let node = tree;
  for (const p of parts.slice(0, -1)) {
    if (typeof node[p] !== 'object' || node[p] === null || '$value' in node[p]) node[p] = {};
    node = node[p];
  }
  const leaf = parts.at(-1);
  const inherited = inheritedTypeAt(tree, parts);
  const prev = node[leaf] && typeof node[leaf] === 'object' && '$value' in node[leaf] ? node[leaf] : null;
  const next = prev ? { ...prev } : {};
  if (tok.type !== inherited) next.$type = tok.type;
  else delete next.$type;
  next.$value = tok.value;
  if (tok.description) next.$description = tok.description;
  else if (prev && tok.description === undefined) delete next.$description;
  // Key order: $type, $value, $description, then anything else ($extensions...).
  const ordered = {};
  for (const k of ['$type', '$value', '$description']) if (k in next) ordered[k] = next[k];
  for (const [k, v] of Object.entries(next)) if (!(k in ordered)) ordered[k] = v;
  node[leaf] = ordered;
}

function deleteToken(tree, name) {
  const parts = name.split('.');
  const stack = [tree];
  for (const p of parts.slice(0, -1)) {
    const n = stack.at(-1)[p];
    if (!n || typeof n !== 'object') return;
    stack.push(n);
  }
  delete stack.at(-1)[parts.at(-1)];
  for (let i = stack.length - 1; i > 0; i--) {
    if (Object.keys(stack[i]).some((k) => !k.startsWith('$'))) break;
    delete stack[i - 1][parts[i - 1]];
  }
}

/**
 * Merge one Penpot set into a dfw tree. Tokens whose Penpot value equals what dfw would export
 * keep dfw's exact value (lossless round trip). Returns { tree, diff, unsupported, warnings }.
 */
export function mergeSet(current, penpotSet, { set, prune = false } = {}) {
  const tree = clone(current) || {};
  const cur = flattenTokens(current || {});
  const reserved = [];
  const incoming = walkSet(penpotSet, undefined, reserved);
  const diff = { added: [], changed: [], removed: [], kept: [] };
  const unsupported = reserved.map((token) => ({ set, token, type: undefined, reason: 'reserved name (__proto__, constructor, prototype)' }));
  const warnings = [];
  for (const [name, pt] of incoming) {
    const c = cur.get(name);
    if (c) {
      const exported = tokenToPenpot(name, c);
      if (!exported.unsupported && same(exported.value, pt.value)) {
        if ((exported.description || undefined) === (pt.description || undefined)) continue; // unchanged
        // Only the description was edited in Penpot: keep dfw's exact value (oklch, seconds, ...).
        setToken(tree, name, { type: c.type, value: c.value, description: pt.description || undefined });
        diff.changed.push({ token: name, type: c.type, from: c.value, to: c.value, descriptionOnly: true });
        continue;
      }
    }
    const r = tokenFromPenpot(name, pt, c?.type, (w) => warnings.push({ set, token: name, note: w }));
    if (r.unsupported) {
      unsupported.push({ set, token: name, type: pt.type, reason: r.unsupported });
      continue;
    }
    if (c && same(c.value, r.value) && c.type === r.type && (c.description || undefined) === (r.description || undefined)) continue;
    setToken(tree, name, r);
    if (c) diff.changed.push({ token: name, type: r.type, from: c.value, to: r.value });
    else diff.added.push({ token: name, type: r.type, value: r.value });
  }
  for (const [name, c] of cur) {
    if (incoming.has(name)) continue;
    const ex = tokenToPenpot(name, c);
    if (ex.unsupported) continue; // never exported (reported at export): keep silently-but-known
    if (prune) {
      deleteToken(tree, name);
      diff.removed.push({ token: name, type: c.type, value: c.value });
    } else diff.kept.push({ token: name, type: c.type });
  }
  return { tree, diff, unsupported, warnings };
}

/* ------------------------------------------------------------------ project IO */

/** Read design/tokens/tokens.json + themes. opt-* themes (temporary preset comparisons) are left out unless asked. */
export function readDfwTokens(root, { optThemes = false } = {}) {
  const dir = projectPaths(root).tokens;
  const baseFile = path.join(dir, 'tokens.json');
  if (!fs.existsSync(baseFile)) throw new DfwError('design/tokens/tokens.json not found.');
  const base = readJson(baseFile);
  const themes = {};
  const skipped = [];
  const td = path.join(dir, 'themes');
  if (fs.existsSync(td)) {
    for (const f of fs.readdirSync(td).filter((x) => x.endsWith('.json')).sort()) {
      const n = f.replace(/\.json$/, '');
      if (n.startsWith('opt-') && !optThemes) skipped.push(n);
      else themes[n] = readJson(path.join(td, f));
    }
  }
  return { base, themes, skipped };
}

export function exportPenpotTokens(root, { out, optThemes = false } = {}) {
  const t = readDfwTokens(root, { optThemes });
  const { json, report } = dfwToPenpot(t);
  report.skippedThemes = t.skipped;
  const file = path.resolve(root, out || path.join('.dfw', 'penpot', 'tokens.penpot.json'));
  writeJson(file, json);
  return { file, json, report };
}

/**
 * Import a Penpot token file into design/tokens. Refuses while the design is frozen (G2) unless dryRun.
 * Returns { written: [files], diff: { [set]: diff }, unsupported, warnings, skippedSets }.
 */
export function importPenpotTokens(root, file, { state = null, dryRun = false, prune = false } = {}) {
  if (!dryRun && state?.design?.locked) {
    throw new DfwError(`Design v${state.design.version} is approved and frozen (G2): token changes from Penpot are not applied. Show them with --dry-run, and if the user wants them, run "dfw reopen --reason <why>" first.`);
  }
  if (!dryRun && state?.mode === 'review-only') throw new DfwError('This project is in review-only mode: use --dry-run to report token differences.');
  const parsed = readPenpotFile(path.resolve(file));
  const { map, skipped } = mapSets(parsed);
  if (![...map.values()].some((m) => m.kind === 'base')) throw new DfwError('No base token set found in the Penpot file (expected a set named "base").');
  const dir = projectPaths(root).tokens;
  const cur = fs.existsSync(path.join(dir, 'tokens.json')) ? readDfwTokens(root, { optThemes: true }) : { base: {}, themes: {} };
  const result = { written: [], diff: {}, unsupported: [], warnings: [], skippedSets: skipped };
  const writes = [];
  for (const [set, m] of map) {
    const current = m.kind === 'base' ? cur.base : cur.themes[m.name] || {};
    const r = mergeSet(current, parsed.sets.get(set), { set, prune });
    result.diff[set] = r.diff;
    result.unsupported.push(...r.unsupported);
    result.warnings.push(...r.warnings);
    const target = m.kind === 'base' ? path.join(dir, 'tokens.json') : path.join(dir, 'themes', `${m.name}.json`);
    if (r.diff.added.length || r.diff.changed.length || r.diff.removed.length || !fs.existsSync(target)) writes.push([target, r.tree]);
  }
  if (!dryRun) {
    for (const [f, tree] of writes) {
      writeJson(f, tree);
      result.written.push(path.relative(root, f));
    }
  }
  return result;
}

export const shortValue = (v) => {
  const s = typeof v === 'string' ? v : v && typeof v === 'object' && v.hex && Object.keys(v).length <= 4 ? v.hex + (v.alpha !== undefined ? `/${v.alpha}` : '') : v && typeof v === 'object' && 'unit' in v ? `${v.value}${v.unit}` : JSON.stringify(v);
  return s.length > 60 ? s.slice(0, 57) + '...' : s;
};

/** Human-readable summary for the CLI. */
export function formatImport(r, { dryRun = false } = {}) {
  const lines = [];
  for (const [set, d] of Object.entries(r.diff)) {
    const n = d.added.length + d.changed.length + d.removed.length;
    lines.push(`${set}: ${n ? `${d.changed.length} changed, ${d.added.length} added, ${d.removed.length} removed` : 'no changes'}${d.kept.length ? ` (${d.kept.length} missing in Penpot, kept; --prune removes them)` : ''}`);
    for (const c of d.changed) lines.push(c.descriptionOnly ? `  ~ ${c.token}: description changed (value kept)` : `  ~ ${c.token}: ${shortValue(c.from)} -> ${shortValue(c.to)}`);
    for (const c of d.added) lines.push(`  + ${c.token} (${c.type}): ${shortValue(c.value)}`);
    for (const c of d.removed) lines.push(`  - ${c.token}`);
  }
  if (r.skippedSets.length) lines.push(`Skipped Penpot sets (name them "base" or "themes/<name>" to import): ${r.skippedSets.join(', ')}`);
  for (const u of r.unsupported) lines.push(`Not imported: ${u.set} ${u.token} (${u.type}): ${u.reason}`);
  for (const w of r.warnings) lines.push(`Note: ${w.set} ${w.token}: ${w.note}`);
  lines.push(dryRun ? 'Dry run: nothing written.' : r.written.length ? `Wrote ${r.written.join(', ')}; tokens.css rebuilt.` : 'Nothing to write.');
  return lines.join('\n');
}

export function formatExport({ file, report }, root) {
  const lines = [`Wrote ${path.relative(root, file) || file}: ${report.exported} token(s) in Penpot format (sets: ${report.sets.join(', ')}).`];
  if (report.skippedThemes?.length) lines.push(`Left out option themes: ${report.skippedThemes.join(', ')} (temporary preset comparisons; --opt-themes includes them).`);
  for (const u of report.unsupported) lines.push(`Not exported: ${u.set} ${u.token} (${u.type}): ${u.reason}`);
  const lossy = new Map();
  for (const l of report.lossy) lossy.set(l.note, [...(lossy.get(l.note) || []), l.token]);
  for (const [note, toks] of lossy) lines.push(`Lossy: ${note}: ${toks.slice(0, 6).join(', ')}${toks.length > 6 ? `, +${toks.length - 6}` : ''}`);
  lines.push('Import it in Penpot: Tokens panel > Tools > Import (replaces the file\'s token sets). Re-import changes with "dfw tokens import <file>".');
  return lines.join('\n');
}
