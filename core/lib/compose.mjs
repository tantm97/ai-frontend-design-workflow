// Design-system composition: screens reuse components with <dfw-use> and
// icons with <dfw-icon>; this module expands them into plain HTML. Studio,
// checks, screenshots and handoff all see the composed output, so a change to
// a component master updates every screen (Figma-style master/instance).
//
// Component file: design/system/components/<name>.html
//   <dfw-component name="button" variants="primary secondary" sizes="md lg"
//                  props="kbd" replaces="button" description="...">
//     <style> .btn { ... } </style>
//     <template>
//       <button class="btn btn--{{variant}} btn--{{size}}" type="button"><dfw-slot></dfw-slot><kbd data-dfw-if="kbd">{{kbd}}</kbd></button>
//     </template>
//     <dfw-examples> <dfw-use c="button" variant="secondary">Cancel</dfw-use> ... </dfw-examples>
//   </dfw-component>
//
// Instance in a screen:
//   <dfw-use c="button" variant="primary" size="lg" kbd="Enter" id="slip.confirm">Confirm</dfw-use>
//   - id -> data-dfw-id on the root element; class is appended; any other
//     attribute that is not a declared prop is copied onto the root element.
//   - inner HTML fills <dfw-slot>; <dfw-fill slot="x">...</dfw-fill> fills <dfw-slot name="x">.
//   - props="a b=default" declares props (optional default); {{prop}} placeholders are HTML-escaped; data-dfw-if="prop" drops the element when prop is empty,
//     data-dfw-if="!prop" drops it when prop is set.
// Icon: <dfw-icon name="scan-line" size="20"></dfw-icon> -> inline SVG from design/system/icons/<name>.svg.
// Head: <dfw-head></dfw-head> inside <head> -> design/system/head.html (font links etc., {{base}} = path to design/)
//       + links to tokens/tokens.css, styles/base.css (if present) and system/components.css.
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, toPosix } from './util.mjs';

const ATTR_RE = /([^\s"'<>/=]+)(?:\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s"'=<>`]+)))?/g;
const VOID = new Set(['area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input', 'link', 'meta', 'source', 'track', 'wbr']);
const RESERVED = new Set(['c', 'component', 'variant', 'size', 'id', 'class']);

export const escapeHtml = (s) => String(s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;');

const decode = (s) => s.replace(/&(quot|#39|apos|lt|gt|amp);/g, (_, e) => ({ quot: '"', '#39': "'", apos: "'", lt: '<', gt: '>', amp: '&' })[e]);

export function parseAttrs(src) {
  const attrs = {};
  for (const m of src.matchAll(ATTR_RE)) attrs[m[1]] = decode(m[2] ?? m[3] ?? m[4] ?? '');
  return attrs;
}

export function attrsToString(attrs) {
  return Object.entries(attrs)
    .map(([k, v]) => (v === '' && !/^(data-|aria-|id$|class$|alt$|value$)/.test(k) ? ` ${k}` : ` ${k}="${escapeHtml(v)}"`))
    .join('');
}

/** Find the end of the element whose start tag begins at `start`. Returns { openEnd, innerStart, innerEnd, end }. */
export function findElement(html, start) {
  const open = /^<([a-zA-Z][\w-]*)\b((?:[^>"']|"[^"]*"|'[^']*')*)>/.exec(html.slice(start));
  if (!open) return null;
  const tag = open[1].toLowerCase();
  const openEnd = start + open[0].length;
  if (VOID.has(tag) || /\/\s*$/.test(open[2])) return { tag, attrs: open[2].replace(/\/\s*$/, ''), openEnd, innerStart: openEnd, innerEnd: openEnd, end: openEnd };
  const re = new RegExp(`<(/?)${tag.replace(/[-]/g, '\\-')}\\b((?:[^>"']|"[^"]*"|'[^']*')*)>`, 'gi');
  re.lastIndex = openEnd;
  let depth = 1;
  let m;
  while ((m = re.exec(html))) {
    if (m[1]) depth--;
    else if (!/\/\s*$/.test(m[2])) depth++;
    if (depth === 0) return { tag, attrs: open[2], openEnd, innerStart: openEnd, innerEnd: m.index, end: m.index + m[0].length };
  }
  return { tag, attrs: open[2], openEnd, innerStart: openEnd, innerEnd: html.length, end: html.length, unclosed: true };
}

function lineOf(text, index) {
  return text.slice(0, index).split('\n').length;
}

// ---------------------------------------------------------------- library

function systemDir(root) {
  return path.join(projectPaths(root).design, 'system');
}

/** Load all component masters. Cached by mtime. */
const cache = new Map();
export function loadComponents(root) {
  const dir = path.join(systemDir(root), 'components');
  const lib = new Map();
  if (!fs.existsSync(dir)) return lib;
  for (const f of fs.readdirSync(dir).filter((x) => x.endsWith('.html')).sort()) {
    const abs = path.join(dir, f);
    const st = fs.statSync(abs);
    const key = `${abs}:${st.mtimeMs}`;
    let defs = cache.get(key);
    if (!defs) {
      defs = parseComponentFile(fs.readFileSync(abs, 'utf8'), `design/system/components/${f}`);
      cache.set(key, defs);
    }
    for (const d of defs) lib.set(d.name, d);
  }
  return lib;
}

export function parseComponentFile(text, rel) {
  const defs = [];
  let i = 0;
  while ((i = text.indexOf('<dfw-component', i)) !== -1) {
    const el = findElement(text, i);
    if (!el) break;
    const a = parseAttrs(el.attrs);
    const inner = text.slice(el.innerStart, el.innerEnd);
    const styles = [...inner.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)].map((m) => m[1].trim()).join('\n');
    const tpl = /<template\b[^>]*>([\s\S]*?)<\/template>/i.exec(inner);
    const ex = /<dfw-examples\b[^>]*>([\s\S]*?)<\/dfw-examples>/i.exec(inner);
    const list = (s) => (s || '').split(/[\s,]+/).filter(Boolean);
    const variants = list(a.variants);
    const sizes = list(a.sizes);
    defs.push({
      name: a.name,
      file: rel,
      line: lineOf(text, i),
      description: a.description || '',
      group: a.group || 'Components',
      variants,
      sizes,
      defaultVariant: a['default-variant'] || variants[0] || '',
      defaultSize: a['default-size'] || sizes[0] || '',
      props: list(a.props).map((x) => x.split('=')[0]),
      propDefaults: Object.fromEntries(list(a.props).filter((x) => x.includes('=')).map((x) => [x.split('=')[0], x.slice(x.indexOf('=') + 1)])),
      replaces: list(a.replaces),
      styles,
      template: tpl ? tpl[1].trim() : '',
      examples: ex ? ex[1].trim() : '',
    });
    i = el.end;
  }
  return defs;
}

export function loadIcon(root, name) {
  const abs = path.join(systemDir(root), 'icons', `${name}.svg`);
  return fs.existsSync(abs) ? fs.readFileSync(abs, 'utf8') : null;
}

export function listIcons(root) {
  const dir = path.join(systemDir(root), 'icons');
  return fs.existsSync(dir) ? fs.readdirSync(dir).filter((f) => f.endsWith('.svg')).map((f) => f.slice(0, -4)).sort() : [];
}

/** Concatenated component CSS (design/system/components.css is generated from it). */
export function componentsCss(root, lib = loadComponents(root)) {
  const parts = ['/* Generated by dfw from design/system/components/*.html. Do not edit by hand. */'];
  for (const d of lib.values()) if (d.styles) parts.push(`/* ${d.name} (${d.file}) */\n${d.styles}`);
  return parts.join('\n\n') + '\n';
}

// ---------------------------------------------------------------- expansion

function fillTemplate(def, props, slots) {
  let out = def.template;
  // Conditionals first (on raw template, before substitution).
  let guard = 0;
  for (let k = out.search(/data-dfw-if\s*=/); k !== -1 && guard++ < 200; k = out.search(/data-dfw-if\s*=/)) {
    const start = out.lastIndexOf('<', k);
    const el = findElement(out, start);
    if (!el) break;
    const a = parseAttrs(el.attrs);
    const cond = a['data-dfw-if'];
    const neg = cond.startsWith('!');
    const val = props[neg ? cond.slice(1) : cond];
    const truthy = val != null && val !== '' && val !== 'false';
    const keep = neg ? !truthy : truthy;
    if (!keep) out = out.slice(0, start) + out.slice(el.end);
    else out = out.slice(0, start) + out.slice(start, el.openEnd).replace(/\s*data-dfw-if\s*=\s*"[^"]*"/, '') + out.slice(el.openEnd);
  }
  out = out.replace(/\{\{\s*([\w-]+)\s*\}\}/g, (_, n) => escapeHtml(props[n] ?? ''));
  out = out.replace(/<dfw-slot(?:\s+name="([\w-]+)")?\s*(?:\/>|>([\s\S]*?)<\/dfw-slot>)/g, (_, n, fallback) => {
    const v = slots[n || 'default'];
    return v != null && v.trim() !== '' ? v : fallback || '';
  });
  return out;
}

function annotateRoot(html, extra, cls) {
  const m = /^(\s*)<([a-zA-Z][\w-]*)\b((?:[^>"']|"[^"]*"|'[^']*')*?)(\/?)>/.exec(html);
  if (!m) return html;
  const attrs = parseAttrs(m[3]);
  if (cls) attrs.class = attrs.class ? `${attrs.class} ${cls}` : cls;
  for (const [k, v] of Object.entries(extra)) if (v != null) attrs[k] = v;
  return `${m[1]}<${m[2]}${attrsToString(attrs)}${m[4]}>` + html.slice(m[0].length);
}

function expandIcon(root, attrs, issues, at) {
  const svg = loadIcon(root, attrs.name);
  if (!svg) {
    issues.push({ level: 'error', rule: 'icon-unknown', index: at, message: `Unknown icon "${attrs.name}" (add it: dfw icons add ${attrs.name}).` });
    return `<span data-dfw-missing-icon="${escapeHtml(attrs.name || '')}"></span>`;
  }
  const size = attrs.size || '24';
  const extra = { width: size, height: size, 'aria-hidden': attrs.label ? undefined : 'true', role: attrs.label ? 'img' : undefined, 'aria-label': attrs.label, 'data-dfw-icon': attrs.name };
  if (attrs['stroke-width']) extra['stroke-width'] = attrs['stroke-width'];
  if (attrs.id) extra['data-dfw-id'] = attrs.id;
  return annotateRoot(svg.replace(/<\?xml[^>]*>\s*/, '').replace(/<!--[\s\S]*?-->\s*/g, '').trim(), extra, attrs.class ? `icon ${attrs.class}` : 'icon');
}

/**
 * Expand <dfw-use> and <dfw-icon> in an HTML string.
 * Returns { html, instances: [{component, variant, id, line}], issues: [{level, rule, line, message}] }.
 */
export function expandHead(root, html, designBase = '../../') {
  if (!/<dfw-head\b/.test(html)) return html;
  const d = projectPaths(root).design;
  const headFile = path.join(d, 'system', 'head.html');
  const parts = [];
  if (fs.existsSync(headFile)) parts.push(fs.readFileSync(headFile, 'utf8').replace(/<!--[\s\S]*?-->\s*/g, '').replaceAll('{{base}}', designBase).trim());
  parts.push(`<link rel="stylesheet" href="${designBase}tokens/tokens.css">`);
  if (fs.existsSync(path.join(d, 'styles', 'base.css'))) parts.push(`<link rel="stylesheet" href="${designBase}styles/base.css">`);
  parts.push(`<link rel="stylesheet" href="${designBase}system/components.css">`);
  return html.replace(/<dfw-head\b[^>]*>(?:\s*<\/dfw-head>)?/, parts.join('\n'));
}

/** Relative path from a design file (relative to the project root) to design/, with trailing slash. */
export function designBaseFor(rel) {
  const depth = toPosix(rel).replace(/^design\//, '').split('/').length - 1;
  return depth ? '../'.repeat(depth) : './';
}

export function composeHtml(root, html, { lib = loadComponents(root), depth = 0, sourceLine = 0, srcText = html, designBase } = {}) {
  if (depth === 0 && designBase) {
    const before = html.split('\n').length;
    html = expandHead(root, html, designBase);
    srcText = html;
    sourceLine -= html.split('\n').length - before; // report lines of the source file, not of the expanded head
  }
  const instances = [];
  const issues = [];
  if (depth > 12) {
    issues.push({ level: 'error', rule: 'compose-depth', index: 0, message: 'Component nesting deeper than 12 levels (recursive component?).' });
    return { html, instances, issues };
  }
  let out = '';
  let i = 0;
  const re = /<dfw-(use|icon)\b/g;
  let m;
  while ((m = re.exec(html))) {
    const start = m.index;
    const el = findElement(html, start);
    if (!el) break;
    out += html.slice(i, start);
    const attrs = parseAttrs(el.attrs);
    if (m[1] === 'icon') {
      out += expandIcon(root, attrs, issues, start);
    } else {
      const name = attrs.c || attrs.component;
      const def = lib.get(name);
      if (!def) {
        issues.push({ level: 'error', rule: 'component-unknown', index: start, message: `Unknown component "${name}". Define it in design/system/components/ first (design system before screens).` });
        out += `<div data-dfw-missing-component="${escapeHtml(name || '')}">${html.slice(el.innerStart, el.innerEnd)}</div>`;
      } else {
        const variant = attrs.variant || def.defaultVariant;
        const size = attrs.size || def.defaultSize;
        if (def.variants.length && !def.variants.includes(variant)) issues.push({ level: 'error', rule: 'component-variant', index: start, message: `Component "${name}" has no variant "${variant}" (variants: ${def.variants.join(', ')}).` });
        if (def.sizes.length && !def.sizes.includes(size)) issues.push({ level: 'error', rule: 'component-size', index: start, message: `Component "${name}" has no size "${size}" (sizes: ${def.sizes.join(', ')}).` });
        const props = { ...def.propDefaults, variant, size };
        const passthrough = {};
        for (const [k, v] of Object.entries(attrs)) {
          if (RESERVED.has(k)) continue;
          if (def.props.includes(k)) props[k] = v;
          else passthrough[k] = v;
        }
        // Slots
        let inner = html.slice(el.innerStart, el.innerEnd);
        const slots = {};
        let fillIdx;
        while ((fillIdx = inner.search(/<dfw-fill\b/)) !== -1) {
          const f = findElement(inner, fillIdx);
          if (!f) break;
          slots[parseAttrs(f.attrs).slot || 'default'] = inner.slice(f.innerStart, f.innerEnd);
          inner = inner.slice(0, fillIdx) + inner.slice(f.end);
        }
        if (slots.default == null) slots.default = inner;
        let body = fillTemplate(def, props, slots);
        body = annotateRoot(body, { ...passthrough, 'data-dfw-id': attrs.id, 'data-dfw-component': name, 'data-dfw-variant': def.variants.length ? variant : undefined, 'data-dfw-size': def.sizes.length ? size : undefined }, attrs.class);
        instances.push({ component: name, variant, size, id: attrs.id || null, index: start });
        const nested = composeHtml(root, body, { lib, depth: depth + 1 });
        out += nested.html;
        instances.push(...nested.instances.map((x) => ({ ...x, index: start })));
        issues.push(...nested.issues.map((x) => ({ ...x, index: start })));
      }
    }
    i = el.end;
    re.lastIndex = el.end;
  }
  out += html.slice(i);
  const withLines = (arr) => arr.map(({ index, ...rest }) => ({ ...rest, line: depth === 0 ? lineOf(srcText, index) + sourceLine : undefined }));
  return { html: out, instances: depth === 0 ? withLines(instances) : instances, issues: depth === 0 ? withLines(issues) : issues };
}

/** Compose a design HTML file (relative to the project root). */
export function composeFile(root, rel) {
  const text = fs.readFileSync(path.join(root, rel), 'utf8');
  const r = composeHtml(root, text, { designBase: designBaseFor(rel) });
  return { ...r, needsComponentsCss: !/system\/components\.css/.test(text) && r.instances.length > 0 };
}

/** Write design/system/components.css and the composed copies under .dfw/build/ (for linters, screenshots, handoff). */
export function buildSystem(root) {
  const p = projectPaths(root);
  const lib = loadComponents(root);
  const sys = systemDir(root);
  const written = [];
  if (lib.size) {
    fs.mkdirSync(sys, { recursive: true });
    const f = path.join(sys, 'components.css');
    const css = componentsCss(root, lib);
    if (!fs.existsSync(f) || fs.readFileSync(f, 'utf8') !== css) {
      fs.writeFileSync(f, css);
      written.push(toPosix(path.relative(root, f)));
    }
  }
  return { components: lib.size, written };
}

/** Compose every design HTML into .dfw/build/<same path>; returns [{rel, out}] for tools that need plain HTML. */
export function buildComposed(root, rels) {
  const p = projectPaths(root);
  const outDir = path.join(p.dfw, 'build');
  const lib = loadComponents(root);
  const res = [];
  for (const rel of rels) {
    const abs = path.join(root, rel);
    if (!fs.existsSync(abs)) continue;
    const text = fs.readFileSync(abs, 'utf8');
    if (!/<dfw-(use|icon|head)\b/.test(text)) {
      res.push({ rel, out: abs, composed: false });
      continue;
    }
    const r = composeHtml(root, text, { lib, designBase: designBaseFor(rel) });
    const out = path.join(outDir, rel);
    fs.mkdirSync(path.dirname(out), { recursive: true });
    // Keep relative asset links working from the build copy.
    const base = toPosix(path.relative(path.dirname(out), path.dirname(abs))) || '.';
    const html = r.html.replace(/<head([^>]*)>/i, `<head$1>\n<base href="${base}/">`);
    fs.writeFileSync(out, html);
    res.push({ rel, out, composed: true });
  }
  return res;
}
