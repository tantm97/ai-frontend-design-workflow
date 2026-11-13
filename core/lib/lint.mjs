// dfw's own design-workspace lint: manifest integrity, stable element ids,
// token usage and a few accessibility basics. Fast and dependency-free so it
// can run inside the PostToolUse hook.
import fs from 'node:fs';
import path from 'node:path';
import { readJson, listFiles, projectPaths, toPosix } from './util.mjs';

const STRUCTURAL = ['button', 'a', 'input', 'select', 'textarea', 'nav', 'header', 'main', 'footer', 'section', 'form', 'table', 'dialog', 'aside', 'img', 'h1', 'h2', 'h3', 'ul', 'ol', 'label'];
const COLOR_RE = /#(?:[0-9a-fA-F]{3,4}){1,2}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color)\(/g;

function lineOf(text, index) {
  return text.slice(0, index).split('\n').length;
}

/** Lint one HTML screen/component file. */
export function lintHtml(rel, text) {
  const out = [];
  const add = (level, rule, index, message) => out.push({ level, rule, file: rel, line: index == null ? 1 : lineOf(text, index), message, source: 'dfw' });

  if (!/<html[^>]*\blang=/i.test(text) && /<html/i.test(text)) add('warn', 'html-lang', text.search(/<html/i), '<html> has no lang attribute.');
  if (!/tokens\.css/.test(text)) add('warn', 'tokens-link', null, 'Screen does not load design/tokens/tokens.css; values may not come from tokens.');

  // Stable ids.
  const ids = new Map();
  for (const m of text.matchAll(/data-dfw-id\s*=\s*"([^"]*)"/g)) {
    if (!m[1].trim()) add('error', 'dfw-id-empty', m.index, 'Empty data-dfw-id.');
    else if (!/^[a-z0-9][a-z0-9._-]*$/.test(m[1])) add('warn', 'dfw-id-format', m.index, `data-dfw-id "${m[1]}" should be lowercase dot.separated (e.g. "filter-bar.search").`);
    if (ids.has(m[1])) add('error', 'dfw-id-duplicate', m.index, `Duplicate data-dfw-id "${m[1]}" (first on line ${lineOf(text, ids.get(m[1]))}).`);
    else ids.set(m[1], m.index);
  }
  const body = text.search(/<body/i);
  let missing = 0;
  let firstMissing = null;
  const tagRe = new RegExp(`<(${STRUCTURAL.join('|')})\\b([^>]*)>`, 'gi');
  for (const m of text.matchAll(tagRe)) {
    if (m.index < body) continue;
    const attrs = m[2];
    if (!/data-dfw-id\s*=/.test(attrs)) {
      missing++;
      if (firstMissing == null) firstMissing = m.index;
    }
    if (m[1].toLowerCase() === 'img' && !/\balt\s*=/.test(attrs)) add('error', 'img-alt', m.index, '<img> without alt (WCAG 1.1.1). Use alt="" for decorative images.');
    if (m[1].toLowerCase() === 'button' && /\bstyle\s*=/.test(attrs)) add('warn', 'inline-style', m.index, 'Inline style on a button; use classes and tokens.');
  }
  if (missing) add('warn', 'dfw-id-missing', firstMissing, `${missing} structural/interactive element(s) have no data-dfw-id; click-to-comment cannot target them reliably.`);

  // Inline <style> colors outside tokens.
  for (const sm of text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)) {
    for (const cm of sm[1].matchAll(COLOR_RE)) {
      const before = sm[1].slice(Math.max(0, cm.index - 80), cm.index);
      if (/--[\w-]+\s*:\s*[^;]*$/.test(before)) continue; // custom property definition
      add('warn', 'hardcoded-color', sm.index + sm[0].indexOf(sm[1]) + cm.index, `Hard-coded color "${cm[0]}" in <style>; use a token (var(--...)).`);
    }
  }
  return out;
}

/** Lint a CSS file that is not the generated tokens file. */
export function lintCss(rel, text) {
  const out = [];
  if (/tokens\/tokens\.css$/.test(rel)) return out;
  const clean = text.replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/[^\n]/g, ' '));
  for (const m of clean.matchAll(COLOR_RE)) {
    const declStart = Math.max(clean.lastIndexOf('{', m.index), clean.lastIndexOf(';', m.index)) + 1;
    if (/^\s*--[\w-]+\s*:/.test(clean.slice(declStart, m.index))) continue; // defining a custom property is fine
    out.push({ level: 'warn', rule: 'hardcoded-color', file: rel, line: lineOf(clean, m.index), message: `Hard-coded color "${m[0]}"; use a token (var(--...)).`, source: 'dfw' });
  }
  if (/transition\s*:\s*all\b/.test(clean)) {
    out.push({ level: 'warn', rule: 'transition-all', file: rel, line: lineOf(clean, clean.search(/transition\s*:\s*all\b/)), message: '"transition: all" animates unintended properties; list the properties.', source: 'dfw' });
  }
  if (/outline\s*:\s*(none|0)\b/.test(clean) && !/:focus-visible/.test(clean)) {
    out.push({ level: 'error', rule: 'focus-removed', file: rel, line: lineOf(clean, clean.search(/outline\s*:\s*(none|0)\b/)), message: 'Focus outline removed without a :focus-visible replacement (WCAG 2.4.7).', source: 'dfw' });
  }
  return out;
}

/** Validate a DTCG token tree: every leaf has $value; aliases resolve. */
export function lintTokens(rel, json) {
  const out = [];
  const leaves = new Map();
  const walk = (node, trail, inheritedType) => {
    if (typeof node !== 'object' || node === null) return;
    const type = node.$type || inheritedType;
    if ('$value' in node) {
      leaves.set(trail.join('.'), { node, type });
      if (!type) out.push({ level: 'warn', rule: 'token-type', file: rel, line: 1, message: `Token "${trail.join('.')}" has no $type (own or inherited).`, source: 'dfw' });
      return;
    }
    for (const [k, v] of Object.entries(node)) {
      if (k.startsWith('$')) continue;
      if (/[{}.]/.test(k)) out.push({ level: 'error', rule: 'token-name', file: rel, line: 1, message: `Token/group name "${k}" must not contain "{", "}" or ".".`, source: 'dfw' });
      walk(v, [...trail, k], type);
    }
  };
  walk(json, [], undefined);
  const strings = (v) => (typeof v === 'string' ? [v] : Array.isArray(v) ? v.flatMap(strings) : v && typeof v === 'object' ? Object.values(v).flatMap(strings) : []);
  for (const [name, { node }] of leaves) {
    for (const m of strings(node.$value).flatMap((s) => [...s.matchAll(/\{([^{}]+)\}/g)])) {
      if (!leaves.has(m[1])) out.push({ level: 'error', rule: 'token-alias', file: rel, line: 1, message: `Token "${name}" references unknown token "{${m[1]}}".`, source: 'dfw' });
    }
  }
  if (!leaves.size) out.push({ level: 'error', rule: 'token-empty', file: rel, line: 1, message: 'Token file has no tokens.', source: 'dfw' });
  return out;
}

export function lintManifest(root) {
  const p = projectPaths(root);
  const out = [];
  const rel = 'design/design.json';
  let m;
  try {
    m = readJson(p.manifest, null);
  } catch (e) {
    return [{ level: 'error', rule: 'manifest-json', file: rel, line: 1, message: e.message, source: 'dfw' }];
  }
  if (!m) return [{ level: 'error', rule: 'manifest-missing', file: rel, line: 1, message: 'design/design.json is missing.', source: 'dfw' }];
  if (!Array.isArray(m.screens) || !m.screens.length) out.push({ level: 'error', rule: 'manifest-screens', file: rel, line: 1, message: 'Manifest has no screens.', source: 'dfw' });
  const seen = new Set();
  for (const s of m.screens || []) {
    if (!s.id) out.push({ level: 'error', rule: 'manifest-screen-id', file: rel, line: 1, message: 'A screen has no id.', source: 'dfw' });
    if (seen.has(s.id)) out.push({ level: 'error', rule: 'manifest-screen-dup', file: rel, line: 1, message: `Duplicate screen id "${s.id}".`, source: 'dfw' });
    seen.add(s.id);
    if (!Array.isArray(s.variants) || !s.variants.length) {
      out.push({ level: 'error', rule: 'manifest-variants', file: rel, line: 1, message: `Screen "${s.id}" has no variants.`, source: 'dfw' });
      continue;
    }
    for (const v of s.variants) {
      if (!v.file) out.push({ level: 'error', rule: 'manifest-variant-file', file: rel, line: 1, message: `Variant "${s.id}/${v.id}" has no file.`, source: 'dfw' });
      else if (!fs.existsSync(path.join(p.design, v.file))) out.push({ level: 'error', rule: 'manifest-file-missing', file: rel, line: 1, message: `Variant "${s.id}/${v.id}" points to missing file design/${v.file}.`, source: 'dfw' });
    }
  }
  return out;
}

/** Lint the whole design workspace, or only the given files (absolute or project-relative). */
export function lintDesign(root, files = null) {
  const p = projectPaths(root);
  const out = [];
  const targets = files
    ? files.map((f) => toPosix(path.relative(root, path.resolve(root, f))))
    : listFiles(p.design, { exclude: ['approved'] }).map((r) => `design/${r}`);
  if (!files || targets.includes('design/design.json')) out.push(...lintManifest(root));
  for (const rel of targets) {
    if (rel.startsWith('design/approved/')) continue;
    const abs = path.join(root, rel);
    if (!fs.existsSync(abs)) continue;
    const text = fs.readFileSync(abs, 'utf8');
    if (rel.endsWith('.html')) out.push(...lintHtml(rel, text));
    else if (rel.endsWith('.css')) out.push(...lintCss(rel, text));
    else if (/tokens\/.*\.json$/.test(rel) || rel.endsWith('.tokens.json')) {
      try {
        out.push(...lintTokens(rel, JSON.parse(text)));
      } catch (e) {
        out.push({ level: 'error', rule: 'token-json', file: rel, line: 1, message: e.message, source: 'dfw' });
      }
    }
  }
  return out;
}
