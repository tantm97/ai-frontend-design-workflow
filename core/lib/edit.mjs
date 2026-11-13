// Direct edits from the Studio, written back to the design SOURCE files.
// Small, surgical changes only: element text, component instance attributes
// (variant / size / declared props) and layout spacing via space tokens.
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

/** Resolve a design file (relative to design/ or to the project root) and enforce the edit boundary. */
export function resolveDesignFile(root, file) {
  if (!file || typeof file !== 'string') throw new DfwError('No source file for this frame.');
  const P = projectPaths(root);
  const rel = file.replace(/^\/+/, '').replace(/^design\//, '');
  const abs = path.resolve(P.design, rel);
  if (!abs.startsWith(P.design + path.sep)) throw new DfwError('Edits are limited to files under design/.');
  if (abs === P.approved || abs.startsWith(P.approved + path.sep)) throw new DfwError('design/approved/ is frozen; edits there are refused.');
  if (!abs.endsWith('.html')) throw new DfwError('Only HTML design files can be edited from the Studio.');
  if (!fs.existsSync(abs)) throw new DfwError(`design/${toPosix(rel)} does not exist.`);
  return { abs, rel: `design/${toPosix(path.relative(P.design, abs))}` };
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
    if (i > 0) out.push({ prop: cur.slice(0, i).trim().toLowerCase(), value: cur.slice(i + 1).trim() });
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

function componentDef(root, attrs) {
  const name = attrs.c || attrs.component;
  const def = loadComponents(root).get(name);
  if (!def) throw new DfwError(`Unknown component "${name}".`);
  return def;
}

/** Pure transform: returns the new source text for an edit op (throws DfwError on refusal). */
export function transform(root, text, op) {
  const el = locate(text, op.id);
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
      let decl = null;
      if (op.value != null && op.value !== '') {
        const tokens = loadTokenNames(root);
        const name = String(op.value).startsWith('--') ? [...tokens].find(([, t]) => t.var === op.value)?.[0] : op.value;
        if (!name || !name.startsWith('space.') || !tokens.has(name)) throw new DfwError(`"${op.value}" is not a space.* token.`);
        decl = `var(${tokens.get(name).var})`;
      }
      const style = setStyleProp(el.attrMap.style || '', op.prop, decl);
      return replaceOpen(setAttr(openTag, 'style', style || null));
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
let seq = 0;

/** Apply one edit op {file, op, id, value, prop}; journals it for undo. */
export function applyEdit(root, op) {
  const { abs, rel } = resolveDesignFile(root, op.file);
  const before = fs.readFileSync(abs, 'utf8');
  const after = transform(root, before, op);
  if (after === before) return { file: rel, changed: false, line: lineAt(before, locate(before, op.id).start) };
  writeAtomic(abs, after);
  const stamp = `${String(Date.now()).padStart(14, '0')}-${String(seq++ % 10000).padStart(4, '0')}`;
  const record = { at: nowIso(), file: rel, op: { op: op.op, id: op.id, prop: op.prop, value: op.value }, before, after };
  writeAtomic(path.join(editsDir(root), `${stamp}.json`), JSON.stringify(record));
  return { file: rel, changed: true, line: lineAt(after, locate(after, op.id).start) };
}

/** Undo the most recent edit (refuses if the file changed since). */
export function undoEdit(root) {
  const dir = editsDir(root);
  const files = fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => f.endsWith('.json')).sort() : [];
  if (!files.length) throw new DfwError('Nothing to undo.');
  const recFile = path.join(dir, files.at(-1));
  const rec = JSON.parse(fs.readFileSync(recFile, 'utf8'));
  const { abs } = resolveDesignFile(root, rec.file);
  if (fs.readFileSync(abs, 'utf8') !== rec.after) {
    throw new DfwError(`${rec.file} changed after that edit (by Claude or an editor); undo refused to avoid losing work.`);
  }
  writeAtomic(abs, rec.before);
  fs.unlinkSync(recFile);
  return { file: rec.file, op: rec.op, remaining: files.length - 1 };
}
