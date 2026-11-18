// Direct edits from the Studio, written back to the design SOURCE files.
// Small, surgical changes only: element text, component instance attributes
// (variant / size / declared props), layout spacing via space tokens, and
// constrained layout manipulation: reordering an element next to a sibling or
// into an empty container (moves its source) and resizing that snaps to the
// system (grid-column span, width/min-height tokens) written to a dfw-owned
// <style data-dfw-layout> block (or reset to auto by removing the rule).
// Every edit is journaled in .dfw/edits/<stamp>.json so it can be undone.
//
// Elements are located by data-dfw-id (or the id of a <dfw-use>/<dfw-icon>
// instance) with a tag scanner that skips comments, <script> and <style>, and
// the element extent comes from compose.findElement. Nothing is regex-replaced
// across the whole file.
import fs from 'node:fs';
import path from 'node:path';
import { findElement, parseAttrs, loadComponents } from './compose.mjs';
import { projectPaths, readJson, DfwError, toPosix, nowIso } from './util.mjs';
import { cssVarName } from './tokens.mjs';

const TAG_SCAN = /<!--[\s\S]*?-->|<(script|style)\b(?:[^>"']|"[^"]*"|'[^']*')*>[\s\S]*?<\/\1\s*>|<([a-zA-Z][\w-]*)\b((?:[^>"']|"[^"]*"|'[^']*')*)>/gi;

export const SPACING_PROPS = new Set([
  'gap', 'row-gap', 'column-gap',
  'padding', 'padding-top', 'padding-right', 'padding-bottom', 'padding-left', 'padding-block', 'padding-inline',
  'margin', 'margin-top', 'margin-right', 'margin-bottom', 'margin-left', 'margin-block', 'margin-inline',
]);

const escText = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
const escAttr = (s) => String(s).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');
const lineAt = (text, i) => text.slice(0, i).split('\n').length;

/** Canonical path (symlinks and letter case as the file system has them); also works for a path that does not exist yet. */
function realPath(p) {
  const rest = [];
  for (let cur = p; ; ) {
    try {
      return path.join(fs.realpathSync.native(cur), ...rest);
    } catch {
      const up = path.dirname(cur);
      if (up === cur) return p;
      rest.unshift(path.basename(cur));
      cur = up;
    }
  }
}

/** Resolve a design file (relative to design/ or to the project root) and enforce the edit boundary. */
export function resolveDesignFile(root, file) {
  if (!file || typeof file !== 'string') throw new DfwError('No source file for this frame.');
  const P = projectPaths(root);
  const rel = file.replace(/^\/+/, '').replace(/^design\//, '');
  // Canonical paths: on a case-insensitive file system "Approved/x.html" IS the frozen file, and a symlink may leave design/.
  const design = realPath(P.design);
  const approved = realPath(P.approved);
  const abs = realPath(path.resolve(P.design, rel));
  if (!abs.startsWith(design + path.sep)) throw new DfwError('Edits are limited to files under design/.');
  if (abs === approved || abs.startsWith(approved + path.sep)) throw new DfwError('design/approved/ is frozen; edits there are refused.');
  if (!abs.endsWith('.html')) throw new DfwError('Only HTML design files can be edited from the Studio.');
  if (!fs.existsSync(abs)) throw new DfwError(`design/${toPosix(rel)} does not exist.`);
  return { abs, rel: `design/${toPosix(path.relative(design, abs))}` };
}

/** Locate the unique element with data-dfw-id="id" (or <dfw-use|dfw-icon id="id">) in source text. */
export function locate(text, id) {
  if (!id) throw new DfwError('This element has no data-dfw-id, so it cannot be located in the source. Comment instead.');
  const hits = [];
  TAG_SCAN.lastIndex = 0;
  let m;
  while ((m = TAG_SCAN.exec(text))) {
    if (!m[2]) continue;
    const tag = m[2].toLowerCase();
    const a = parseAttrs(m[3]);
    if (a['data-dfw-id'] === id || ((tag === 'dfw-use' || tag === 'dfw-icon') && a.id === id)) hits.push({ index: m.index, tag, attrs: a });
  }
  if (!hits.length) throw new DfwError(`"${id}" was not found in the source file (it may come from a component master or shared include).`);
  if (hits.length > 1) throw new DfwError(`"${id}" appears ${hits.length} times in the source file; ids must be unique per file.`);
  const h = hits[0];
  const el = findElement(text, h.index);
  if (!el || el.unclosed) throw new DfwError(`Could not find the end of <${h.tag}> for "${id}".`);
  return { ...el, start: h.index, tag: h.tag, attrMap: h.attrs, isInstance: h.tag === 'dfw-use' };
}

/** Set (value != null) or remove (value == null) one attribute inside an open tag string. */
export function setAttr(openTag, name, value) {
  const head = /^<[a-zA-Z][\w-]*/.exec(openTag)[0];
  const re = /\s+([^\s"'<>/=]+)(?:\s*=\s*(?:"[^"]*"|'[^']*'|[^\s"'=<>`]+))?/y;
  re.lastIndex = head.length;
  let m;
  while ((m = re.exec(openTag))) {
    if (m[1].toLowerCase() === name.toLowerCase()) {
      const repl = value == null ? '' : ` ${name}="${escAttr(value)}"`;
      return openTag.slice(0, m.index) + repl + openTag.slice(m.index + m[0].length);
    }
  }
  if (value == null) return openTag;
  const tail = /\s*\/?>$/.exec(openTag);
  return openTag.slice(0, tail.index) + ` ${name}="${escAttr(value)}"` + openTag.slice(tail.index);
}

/** Split an inline style into [{prop, value}] respecting parentheses and quotes. */
export function parseStyle(style) {
  const out = [];
  let depth = 0;
  let q = null;
  let cur = '';
  const flush = () => {
    const i = cur.indexOf(':');
    const prop = cur.slice(0, i).trim();
    // Custom properties are case-sensitive (--colW is not --colw); other names are normalised.
    if (i > 0) out.push({ prop: prop.startsWith('--') ? prop : prop.toLowerCase(), value: cur.slice(i + 1).trim() });
    cur = '';
  };
  for (const ch of style || '') {
    if (q) {
      if (ch === q) q = null;
    } else if (ch === '"' || ch === "'") q = ch;
    else if (ch === '(') depth++;
    else if (ch === ')') depth = Math.max(0, depth - 1);
    else if (ch === ';' && depth === 0) {
      flush();
      continue;
    }
    cur += ch;
  }
  flush();
  return out;
}

export function setStyleProp(style, prop, value) {
  const family = prop === 'gap' ? /^(row-gap|column-gap)$/ : prop === 'padding' || prop === 'margin' ? new RegExp(`^${prop}-`) : null;
  const decls = parseStyle(style).filter((d) => d.prop !== prop && !(family && family.test(d.prop)));
  if (value != null) decls.push({ prop, value });
  return decls.map((d) => `${d.prop}: ${d.value}`).join('; ');
}

/** Token names -> css var names from design/tokens/tokens.json (name: "space.4", var: "--space-4"). */
export function loadTokenNames(root) {
  const tree = readJson(path.join(projectPaths(root).tokens, 'tokens.json'), null);
  const names = new Map();
  const walk = (node, trail, type) => {
    if (typeof node !== 'object' || node === null) return;
    const t = node.$type || type;
    if ('$value' in node) {
      names.set(trail.join('.'), { var: cssVarName(trail.join('.')), type: t, value: node.$value });
      return;
    }
    for (const [k, v] of Object.entries(node)) if (!k.startsWith('$')) walk(v, [...trail, k], t);
  };
  if (tree) walk(tree, [], undefined);
  return names;
}

/** Resolve a token name ("space.4") or css var ("--space-4") with one of the prefixes to "var(--space-4)". */
function tokenVar(root, value, prefixes) {
  const tokens = loadTokenNames(root);
  const v = String(value);
  const name = v.startsWith('--') ? [...tokens].find(([, t]) => t.var === v)?.[0] : v;
  if (!name || !prefixes.some((p) => name.startsWith(p)) || !tokens.has(name)) throw new DfwError(`"${value}" is not a ${prefixes.map((p) => `${p}*`).join(' or ')} token.`);
  return `var(${tokens.get(name).var})`;
}

/* ---------------------------------------------------------------- layout: reorder + resize */

export const LAYOUT_PROPS = ['grid-column', 'width', 'min-height'];
export const WIDTH_PERCENTS = ['25%', '33.333%', '50%', '66.667%', '75%', '100%'];
const MAX_SPAN = 24;

/** Validate a resize value and return the CSS to write (null = remove the declaration). */
export function layoutValue(root, prop, value) {
  const v = String(value ?? '').trim();
  if (!v || v === 'auto') return null; // reset to the screen's own layout
  if (prop === 'grid-column') {
    const m = /^(?:span\s+)?(\d+)$/.exec(v);
    if (!m || +m[1] < 1 || +m[1] > MAX_SPAN) throw new DfwError(`grid-column must be "span N" with N from 1 to ${MAX_SPAN}.`);
    return `span ${+m[1]}`;
  }
  if (prop === 'width' && v.endsWith('%')) {
    if (!WIDTH_PERCENTS.includes(v)) throw new DfwError(`Width percentages snap to ${WIDTH_PERCENTS.join(', ')}.`);
    return v;
  }
  return tokenVar(root, v, ['size.', 'space.']);
}

/** Line facts for a source range: its indentation and whether it sits alone on its line(s). */
function lineSpan(text, start, end) {
  const ls = text.lastIndexOf('\n', start - 1) + 1;
  const nl = text.indexOf('\n', end);
  const le = nl === -1 ? text.length : nl;
  const indent = text.slice(ls, start);
  return { ls, le, indent, own: /^[ \t]*$/.test(indent) && /^[ \t]*$/.test(text.slice(end, le)) };
}

/** Move the source of element `el` before/after element `ref` (spans from locate), keeping indentation. */
export function moveSource(text, el, ref, position) {
  if (ref.start >= el.start && ref.end <= el.end) throw new DfwError('An element cannot be moved into itself.');
  if (el.start >= ref.start && el.end <= ref.end) throw new DfwError('Drop next to a sibling, not next to a parent of the element.');
  const a = lineSpan(text, el.start, el.end);
  const b = lineSpan(text, ref.start, ref.end);
  const block = text.slice(el.start, el.end);
  let cut;
  let at;
  let ins;
  if (a.own && b.own) {
    // Whole lines: cut the element's lines and insert them at the ref's line with the ref's indentation.
    cut = [a.ls, Math.min(text.length, a.le + 1)];
    const lines = block.split('\n');
    const body = [lines[0], ...lines.slice(1).map((l) => (l.startsWith(a.indent) ? b.indent + l.slice(a.indent.length) : l))].join('\n');
    if (position === 'before') [at, ins] = [b.ls, `${b.indent}${body}\n`];
    else if (b.le < text.length) [at, ins] = [b.le + 1, `${b.indent}${body}\n`];
    else [at, ins] = [b.le, `\n${b.indent}${body}`];
  } else {
    cut = [el.start, el.end];
    at = position === 'before' ? ref.start : ref.end;
    ins = block;
  }
  if (at > cut[0] && at < cut[1]) throw new DfwError('Cannot move the element there.');
  return at >= cut[1]
    ? text.slice(0, cut[0]) + text.slice(cut[1], at) + ins + text.slice(at)
    : text.slice(0, at) + ins + text.slice(at, cut[0]) + text.slice(cut[1]);
}

/** Move the source of element `el` to the end of container `box` (drop into an empty flex/grid container). */
export function moveInside(text, el, box) {
  if (box.start >= el.start && box.end <= el.end) throw new DfwError('An element cannot be moved into itself.');
  if (box.end === box.openEnd || box.innerStart == null) throw new DfwError(`<${box.tag}> cannot contain other elements.`);
  if (box.isInstance) throw new DfwError('Dropping into a component instance is not supported; its content is defined by the master and its slots.');
  const a = lineSpan(text, el.start, el.end);
  const block = text.slice(el.start, el.end);
  const inner = text.slice(box.innerStart, box.innerEnd);
  const boxLine = lineSpan(text, box.start, box.start);
  const closeLine = lineSpan(text, box.innerEnd, box.innerEnd);
  const reindent = (to) => {
    const lines = block.split('\n');
    return [lines[0], ...lines.slice(1).map((l) => (l.startsWith(a.indent) ? to + l.slice(a.indent.length) : l))].join('\n');
  };
  let cut = [el.start, el.end];
  let at = box.innerEnd;
  let ins = block;
  if (a.own) {
    cut = [a.ls, Math.min(text.length, a.le + 1)];
    if (/^[ \t]*$/.test(closeLine.indent) && closeLine.ls > box.innerStart) {
      // The closing tag sits on its own line: insert a new line above it, one level deeper.
      const ind = `${closeLine.indent}  `;
      [at, ins] = [closeLine.ls, `${ind}${reindent(ind)}\n`];
    } else if (!inner.trim() && /^[ \t]*$/.test(boxLine.indent)) {
      // An empty one-line container: open it up.
      const ind = `${boxLine.indent}  `;
      [at, ins] = [box.innerStart, `\n${ind}${reindent(ind)}\n${boxLine.indent}`];
    }
  }
  if (at > cut[0] && at < cut[1]) throw new DfwError('Cannot move the element there.');
  // Whitespace-only content of a one-line container is replaced; otherwise nothing is removed.
  const drop = at === box.innerStart && !inner.trim() ? box.innerEnd - box.innerStart : 0;
  return at >= cut[1]
    ? text.slice(0, cut[0]) + text.slice(cut[1], at) + ins + text.slice(at + drop)
    : text.slice(0, at) + ins + text.slice(at + drop, cut[0]) + text.slice(cut[1]);
}

const LAYOUT_BLOCK = /<style\b[^>]*\bdata-dfw-layout\b[^>]*>([\s\S]*?)<\/style\s*>/gi;
const HAND_EDITED =
  "The <style data-dfw-layout> block was edited by hand in a form the Studio cannot rewrite safely; fix it in code (one [data-dfw-id='...'] { ... } rule per element, optionally inside @media (max-width: Npx)).";

/** Parse the dfw layout block: Map(maxWidth (0 = base) -> Map(id -> Map(prop -> value))). */
export function parseLayoutCss(css) {
  const model = new Map();
  // Single quotes on purpose: tools that look for data-dfw-id="<id>" (lint duplicates, comment source lines) must find the element, not this rule.
  // The :not(#dfw) suffix (see LAYOUT_SPECIFICITY) is optional so blocks written before it still parse.
  const RULE = /\[data-dfw-id=(['"])([^'"\\]+)\1\](?::not\(#dfw\))?\s*\{([^{}]*)\}/g;
  const take = (chunk, key) =>
    chunk.replace(RULE, (_, _q, id, decls) => {
      if (!model.has(key)) model.set(key, new Map());
      const rules = model.get(key);
      const r = rules.get(id) || new Map();
      for (const d of parseStyle(decls)) r.set(d.prop, d.value);
      rules.set(id, r);
      return '';
    });
  let rest = css.replace(/\/\*[\s\S]*?\*\//g, '');
  rest = rest.replace(/@media\s*\(\s*max-width\s*:\s*(\d+)px\s*\)\s*\{((?:[^{}]|\{[^{}]*\})*)\}/g, (_, w, inner) => {
    if (take(inner, Number(w)).trim()) throw new DfwError(HAND_EDITED);
    return '';
  });
  if (take(rest, 0).trim()) throw new DfwError(HAND_EDITED);
  return model;
}

/**
 * Every layout rule is `[data-dfw-id='x']:not(#dfw)`: the :not(#id) adds one id of specificity (1,1,0) without
 * changing what matches, so a resize beats class/attribute/type screen rules (`.dash .card { grid-column: span 3 }`)
 * wherever they sit, without !important. Rules that still win (id selectors, inline style, !important) are reported
 * by the Studio after the write, naming the conflicting selector.
 */
export const LAYOUT_SPECIFICITY = ':not(#dfw)';

function renderLayout(model, ind) {
  const pad = `${ind}  `;
  const rule = (p, id, r) => `${p}[data-dfw-id='${id}']${LAYOUT_SPECIFICITY} { ${[...r].map(([k, v]) => `${k}: ${v};`).join(' ')} }`;
  const out = [`${ind}<style data-dfw-layout>`, `${pad}/* dfw Studio layout edits (drag/resize): layout properties only, one rule per element; :not(#dfw) lifts specificity over screen rules. */`];
  for (const [id, r] of model.get(0) || []) out.push(rule(pad, id, r));
  for (const w of [...model.keys()].filter(Boolean).sort((x, y) => y - x)) {
    out.push(`${pad}@media (max-width: ${w}px) {`);
    for (const [id, r] of model.get(w)) out.push(rule(`${pad}  `, id, r));
    out.push(`${pad}}`);
  }
  out.push(`${ind}</style>`);
  return out.join('\n');
}

/**
 * Set (cssValue != null) or remove one layout declaration (prop == null: the element's whole rule) for data-dfw-id `id` in the screen's
 * <style data-dfw-layout> block (created before </head>, removed when empty). maxWidth > 0 scopes
 * it to @media (max-width: <maxWidth>px); media blocks are kept widest first so narrower ones win.
 */
export function setLayoutRule(text, id, prop, cssValue, maxWidth = 0) {
  const blocks = [...text.matchAll(LAYOUT_BLOCK)];
  if (blocks.length > 1) throw new DfwError('The screen has more than one <style data-dfw-layout> block; merge them in code.');
  const hit = blocks[0];
  const model = hit ? parseLayoutCss(hit[1]) : new Map();
  const rules = model.get(maxWidth) || new Map();
  const r = rules.get(id) || new Map();
  if (prop == null) r.clear();
  else if (cssValue == null) r.delete(prop);
  else r.set(prop, cssValue);
  if (r.size) rules.set(id, r);
  else rules.delete(id);
  if (rules.size) model.set(maxWidth, rules);
  else model.delete(maxWidth);
  const empty = !model.size;
  if (hit) {
    const end = hit.index + hit[0].length;
    const s = lineSpan(text, hit.index, end);
    if (empty) return s.own ? text.slice(0, s.ls) + text.slice(Math.min(text.length, s.le + 1)) : text.slice(0, hit.index) + text.slice(end);
    const ind = s.own ? s.indent : '';
    return text.slice(0, hit.index) + renderLayout(model, ind).slice(ind.length) + text.slice(end);
  }
  if (empty) return text;
  const head = /<\/head\s*>/i.exec(text);
  if (!head) throw new DfwError('The screen has no </head>; layout rules go into a <style data-dfw-layout> block in the head.');
  const s = lineSpan(text, head.index, head.index);
  if (/^[ \t]*$/.test(s.indent)) return text.slice(0, s.ls) + renderLayout(model, `${s.indent}  `) + '\n' + text.slice(s.ls);
  return text.slice(0, head.index) + '\n' + renderLayout(model, '') + '\n' + text.slice(head.index);
}

/** Studio edits follow the write gate: refused once the design is approved and locked (G2), or in review-only mode. */
export function assertEditable(root) {
  const state = readJson(projectPaths(root).state, null);
  if (!state || state.active === false) return;
  if (state.design?.locked) throw new DfwError(`Design v${state.design.version} is approved and locked (G2); Studio edits are refused. Changing it needs your approval and "dfw reopen --reason <why>".`);
  if (state.mode === 'review-only') throw new DfwError('This project is in review-only mode; Studio edits are refused. Leave a comment instead.');
}

function componentDef(root, attrs) {
  const name = attrs.c || attrs.component;
  const def = loadComponents(root).get(name);
  if (!def) throw new DfwError(`Unknown component "${name}".`);
  return def;
}

/** Component master whose template declares data-dfw-id="id" (an element the screen source does not own). */
export function masterOf(root, id) {
  for (const def of loadComponents(root).values()) {
    const scan = new RegExp(TAG_SCAN.source, 'gi'); // own lastIndex: locate() may run in between
    let m;
    while ((m = scan.exec(def.template))) if (m[2] && parseAttrs(m[3])['data-dfw-id'] === id) return def;
  }
  return null;
}

/** locate() that explains ids built by a component template (app-shell slots, layout masters). */
function locateOwn(root, text, id) {
  try {
    return locate(text, id);
  } catch (e) {
    const def = id && /was not found/.test(e.message) ? masterOf(root, id) : null;
    if (!def) throw e;
    throw new DfwError(`"${id}" is part of the component master "${def.name}" (${def.file}), not of this screen, so the Studio cannot move or resize it here. Edit the master to change it, or leave a comment.`);
  }
}

/** Pure transform: returns the new source text for an edit op (throws DfwError on refusal). */
export function transform(root, text, op) {
  const el = locateOwn(root, text, op.id);
  const openTag = text.slice(el.start, el.openEnd);
  const replaceOpen = (tag) => text.slice(0, el.start) + tag + text.slice(el.openEnd);
  switch (op.op) {
    case 'text': {
      if (el.end === el.openEnd) throw new DfwError(`<${el.tag}> has no text content to edit.`);
      const inner = text.slice(el.innerStart, el.innerEnd);
      if (/<[a-zA-Z!]/.test(inner)) throw new DfwError(`"${op.id}" contains child elements in the source; edit it in code or leave a comment.`);
      const value = String(op.value ?? '').replace(/\s*\n\s*/g, ' ').trim();
      if (!value) throw new DfwError('Text cannot be empty.');
      const [, lead, , trail] = /^(\s*)([\s\S]*?)(\s*)$/.exec(inner);
      return text.slice(0, el.innerStart) + lead + escText(value) + trail + text.slice(el.innerEnd);
    }
    case 'variant':
    case 'size': {
      if (!el.isInstance) throw new DfwError(`"${op.id}" is not a component instance (<dfw-use>).`);
      const def = componentDef(root, el.attrMap);
      const allowed = op.op === 'variant' ? def.variants : def.sizes;
      if (!allowed.length) throw new DfwError(`Component "${def.name}" has no ${op.op}s.`);
      if (!allowed.includes(op.value)) throw new DfwError(`Component "${def.name}" has no ${op.op} "${op.value}" (${allowed.join(', ')}).`);
      return replaceOpen(setAttr(openTag, op.op, op.value));
    }
    case 'prop': {
      if (!el.isInstance) throw new DfwError(`"${op.id}" is not a component instance (<dfw-use>).`);
      const def = componentDef(root, el.attrMap);
      if (!def.props.includes(op.prop)) throw new DfwError(`Component "${def.name}" has no prop "${op.prop}" (${def.props.join(', ') || 'none'}).`);
      const v = op.value == null || op.value === '' ? null : String(op.value);
      return replaceOpen(setAttr(openTag, op.prop, v));
    }
    case 'spacing': {
      if (el.isInstance) throw new DfwError('Spacing of a component instance is defined by its master; change the component or leave a comment.');
      if (!SPACING_PROPS.has(op.prop)) throw new DfwError(`"${op.prop}" is not editable here; only gap, padding and margin.`);
      const decl = op.value != null && op.value !== '' ? tokenVar(root, op.value, ['space.']) : null;
      const style = setStyleProp(el.attrMap.style || '', op.prop, decl);
      return replaceOpen(setAttr(openTag, 'style', style || null));
    }
    case 'move': {
      if (!['before', 'after', 'inside'].includes(op.position)) throw new DfwError('Move position must be "before", "after" or "inside".');
      if (!op.ref || op.ref === op.id) throw new DfwError('Move needs another element to drop next to (or a container to drop into).');
      const ref = locateOwn(root, text, op.ref);
      return op.position === 'inside' ? moveInside(text, el, ref) : moveSource(text, el, ref, op.position);
    }
    case 'layout': {
      // No prop: reset the element's whole rule at this breakpoint scope (back to the screen's own layout).
      if (op.prop != null && !LAYOUT_PROPS.includes(op.prop)) throw new DfwError(`"${op.prop}" is not resizable here; only ${LAYOUT_PROPS.join(', ')}.`);
      if (/["'\\]/.test(op.id)) throw new DfwError(`"${op.id}" cannot be used in a layout rule.`);
      const mw = op.maxWidth == null || op.maxWidth === '' ? 0 : Number(op.maxWidth);
      if (!Number.isInteger(mw) || mw < 0 || mw > 10000) throw new DfwError(`Invalid breakpoint width "${op.maxWidth}".`);
      return setLayoutRule(text, op.id, op.prop ?? null, op.prop == null ? null : layoutValue(root, op.prop, op.value), mw);
    }
    default:
      throw new DfwError(`Unknown edit op "${op.op}".`);
  }
}

function writeAtomic(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.${Date.now()}.tmp`;
  fs.writeFileSync(tmp, data);
  fs.renameSync(tmp, file);
}

const editsDir = (root) => path.join(projectPaths(root).dfw, 'edits');
export const MAX_JOURNAL = 200; // undo depth; each record holds two full copies of the file
let seq = 0;

/** Apply one edit op {file, op, id, value, prop}; journals it for undo. */
export function applyEdit(root, op) {
  assertEditable(root);
  const { abs, rel } = resolveDesignFile(root, op.file);
  const before = fs.readFileSync(abs, 'utf8');
  const after = transform(root, before, op);
  if (after === before) return { file: rel, changed: false, line: lineAt(before, locate(before, op.id).start) };
  writeAtomic(abs, after);
  const stamp = `${String(Date.now()).padStart(14, '0')}-${String(seq++ % 10000).padStart(4, '0')}`;
  const record = { at: nowIso(), file: rel, op: { op: op.op, id: op.id, prop: op.prop, value: op.value, ref: op.ref, position: op.position, maxWidth: op.maxWidth }, before, after };
  writeAtomic(path.join(editsDir(root), `${stamp}.json`), JSON.stringify(record));
  const journal = fs.readdirSync(editsDir(root)).filter((f) => f.endsWith('.json')).sort();
  for (const f of journal.slice(0, Math.max(0, journal.length - MAX_JOURNAL))) fs.rmSync(path.join(editsDir(root), f), { force: true });
  return { file: rel, changed: true, line: lineAt(after, locate(after, op.id).start) };
}

/** Undo the most recent edit (refuses if the file changed since). */
export function undoEdit(root) {
  assertEditable(root);
  const dir = editsDir(root);
  const files = fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => f.endsWith('.json')).sort() : [];
  let recFile;
  let rec;
  // An edit to a file that was deleted since cannot be undone and must not block the older edits behind it.
  for (; files.length; files.pop()) {
    recFile = path.join(dir, files.at(-1));
    rec = JSON.parse(fs.readFileSync(recFile, 'utf8'));
    if (fs.existsSync(path.join(root, rec.file))) break;
    fs.unlinkSync(recFile);
  }
  if (!files.length) throw new DfwError('Nothing to undo.');
  const { abs } = resolveDesignFile(root, rec.file);
  if (fs.readFileSync(abs, 'utf8') !== rec.after) {
    throw new DfwError(`${rec.file} changed after that edit (by Claude or an editor); undo refused to avoid losing work.`);
  }
  writeAtomic(abs, rec.before);
  fs.unlinkSync(recFile);
  return { file: rec.file, op: rec.op, remaining: files.length - 1 };
}
