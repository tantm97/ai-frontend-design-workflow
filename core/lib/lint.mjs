// dfw's own design-workspace lint: manifest integrity, stable element ids,
// token usage and a few accessibility basics. Fast and dependency-free so it
// can run inside the PostToolUse hook.
import fs from 'node:fs';
import path from 'node:path';
import { readJson, listFiles, projectPaths, toPosix } from './util.mjs';
import { composeHtml, loadComponents, designBaseFor, findElement } from './compose.mjs';
import { hexToOklch } from './color.mjs';

const STRUCTURAL = ['button', 'a', 'input', 'select', 'textarea', 'nav', 'header', 'main', 'footer', 'section', 'form', 'table', 'dialog', 'aside', 'img', 'h1', 'h2', 'h3', 'ul', 'ol', 'label'];
const COLOR_RE = /#(?:[0-9a-fA-F]{3,4}){1,2}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color)\(/g;

function lineOf(text, index) {
  return text.slice(0, index).split('\n').length;
}

/* ---------------------------------------------------------------- generic-AI tells ("slop") */

// Colours AI-generated UIs default to. Exact values only: a brand that really uses one keeps it with a recorded decision.
const AI_DEFAULT_COLORS = {
  '#6366f1': 'Tailwind indigo', '#4f46e5': 'Tailwind indigo', '#4338ca': 'Tailwind indigo', '#3730a3': 'Tailwind indigo',
  '#8b5cf6': 'Tailwind violet', '#7c3aed': 'Tailwind violet', '#a855f7': 'Tailwind purple',
  '#d97757': 'the terracotta accent of the "warm cream + serif" default', '#f4f1ea': 'the warm cream ground of the "cream + serif + terracotta" default',
};
const FILLER_RE = /lorem ipsum|\bfeature (?:one|two|three)\b|placeholder text|sample content|your (?:headline|tagline) here/gi;
const METRIC_RE = /\b\d+(?:\.\d+)?\s*[x×]\s+(?:faster|more|better|cheaper|productive)\b|\b99\.9+\s*%\s*uptime\b/gi;
const PLACEHOLDER_IMG_RE = /\b(?:images\.unsplash\.com|source\.unsplash\.com|placehold\.co|placekitten\.com|picsum\.photos|via\.placeholder\.com|placeholder\.com)\b/gi;
const EMOJI_RE = /\p{Extended_Pictographic}/u;
// Marketing filler that reads as machine-written in product UI (dfw's own list).
const COPY_TELL_RE = /\b(?:seamless(?:ly)?|effortless(?:ly)?|supercharge|unlock (?:the )?(?:power|potential)|elevate your|game[- ]changer|cutting[- ]edge|next[- ]level|revolutioni[sz]e|empower(?:s|ing)? (?:you|teams)|in today's fast[- ]paced|harness the power|at your fingertips|look no further|delve into)\b/gi;
const VAGUE_ERROR_RE = /\b(?:oops!?|something went wrong|an error (?:has )?occurred)\b/gi;
const CLICK_HERE_RE = /<a\b[^>]*>\s*(?:click here|here|read more|learn more)\s*<\/a>/gi;

/** Interface-guideline rules checkable in CSS: zoom-triggering input text, raw z-index. */
function guidelineCss(css, at, add) {
  for (const b of css.matchAll(/([^{}]*)\{([^{}]*)\}/g)) {
    const sel = b[1];
    const fs = /font-size\s*:\s*(\d+(?:\.\d+)?)px/.exec(b[2]);
    if (fs && Number(fs[1]) < 16 && /(^|[\s,>+~(])(input|select|textarea)\b/.test(sel)) {
      add('warn', 'input-font-size', at(b.index), `Form field text is ${fs[1]}px: iOS zooms the page on focus below 16px. Use at least 16px on touch breakpoints.`);
    }
    const z = /z-index\s*:\s*(-?\d+)/.exec(b[2]);
    if (z && Math.abs(Number(z[1])) > 1) add('warn', 'z-index-raw', at(b.index), `Raw z-index ${z[1]}: stacking order drifts when every component picks its own number. Use a z-index token (e.g. var(--z-overlay)).`);
  }
}

/** Generic-AI tells in CSS: two-stop purple/blue "trust" gradients and the rounded card with a coloured left border. */
function slopCss(css, at, add) {
  guidelineCss(css, at, add);
  for (const g of css.matchAll(/linear-gradient\(([^;{}]*)\)/gi)) {
    const hues = [...g[1].matchAll(/#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b/g)].map((m) => hexToOklch(m[0])).filter((c) => c.c > 0.1).map((c) => c.h);
    if (hues.length >= 2 && hues.every((h) => h >= 190 && h < 360) && Math.max(...hues) - Math.min(...hues) > 20) {
      add('warn', 'ai-gradient', at(g.index), 'Purple/blue/cyan two-stop gradient: the textbook AI "trust" gradient. Use a flat surface and intentional type, or a gradient built from the palette with a reason.');
    }
  }
  for (const b of css.matchAll(/\{([^{}]*)\}/g)) {
    if (/border-left(?:-width)?\s*:\s*[^;]*\b(?:[2-9]|\d{2,})px/.test(b[1]) && /border-radius\s*:/.test(b[1])) {
      add('warn', 'left-border-card', at(b.index), 'Rounded box with a thick coloured left border: the stock "AI dashboard tile". Drop the radius or the left border.');
    }
  }
}

/** Generic-AI tells in an HTML file: default colours, emoji icons, filler copy, invented metrics, placeholder image hosts, CSS tells. */
function slopHtml(text, add) {
  for (const m of text.matchAll(/#[0-9a-fA-F]{6}\b/g)) {
    const name = AI_DEFAULT_COLORS[m[0].toLowerCase()];
    if (name) add('warn', 'ai-default-color', m.index, `${m[0]} is ${name}, a colour AI-generated UIs default to. Use the palette (dfw palette build); keep it only if the brand requires it (dfw decide).`);
  }
  for (const sm of text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)) {
    const off = sm.index + sm[0].indexOf(sm[1]);
    slopCss(sm[1], (i) => off + i, add);
  }
  const body = Math.max(0, text.search(/<body|<template/i));
  const markup = text.slice(body).replace(/<style[\s\S]*?<\/style>|<script[\s\S]*?<\/script>/gi, (s) => ' '.repeat(s.length));
  for (const m of markup.matchAll(/<(h[1-6]|button|li|label)\b[^>]*>([\s\S]*?)<\/\1>/gi)) {
    if (EMOJI_RE.test(m[2].replace(/<[^>]*>/g, ''))) add('error', 'emoji-icon', body + m.index, `Emoji in <${m[1]}> used as an icon. Use a monoline SVG icon (dfw icons add <name>) with currentColor.`);
  }
  for (const m of markup.matchAll(/class\s*=\s*"[^"]*\bicon\b[^"]*"[^>]*>\s*(\S{1,4})/g)) {
    if (EMOJI_RE.test(m[1])) add('error', 'emoji-icon', body + m.index, 'Emoji used as an icon. Use a monoline SVG icon (dfw icons add <name>) with currentColor.');
  }
  for (const m of markup.matchAll(FILLER_RE)) add('error', 'filler-copy', body + m.index, `Filler copy "${m[0]}". Write real, domain-specific content (PRODUCT.md data shapes), or solve the empty space with composition.`);
  const textOnly = markup.replace(/<[^>]*>/g, (t) => ' '.repeat(t.length));
  for (const m of textOnly.matchAll(COPY_TELL_RE)) add('warn', 'copy-tell', body + m.index, `"${m[0]}" is marketing filler that reads as machine-written. Say what the product does, in the user's words.`);
  for (const m of textOnly.matchAll(VAGUE_ERROR_RE)) add('warn', 'vague-error', body + m.index, `"${m[0]}" says nothing. An error message states what happened, why if known, and what the user can do; keep their input.`);
  for (const m of textOnly.matchAll(/(?<![.])\.\.\.(?![.])/g)) add('info', 'ellipsis-char', body + m.index, 'Use the ellipsis character "…" instead of three dots.');
  for (const m of markup.matchAll(CLICK_HERE_RE)) add('warn', 'link-text', body + m.index, 'Link text must say where it goes ("View invoice 1042"), not "click here" / "learn more" (WCAG 2.4.4).');
  for (const m of markup.matchAll(METRIC_RE)) add('warn', 'invented-metric', body + m.index, `"${m[0]}" looks like an invented metric. Use a real figure from the brief, or a clearly labelled placeholder.`);
  for (const m of text.matchAll(PLACEHOLDER_IMG_RE)) add('warn', 'placeholder-image-host', m.index, `External placeholder image host "${m[0]}": fragile and obviously generic. Use a local asset in design/assets/ or a designed placeholder.`);
}

/** Lint one HTML screen/component file. */
export function lintHtml(rel, text) {
  const out = [];
  const add = (level, rule, index, message) => out.push({ level, rule, file: rel, line: index == null ? 1 : lineOf(text, index), message, source: 'dfw' });

  if (!/<html[^>]*\blang=/i.test(text) && /<html/i.test(text)) add('warn', 'html-lang', text.search(/<html/i), '<html> has no lang attribute.');
  if (!/tokens\.css|<dfw-head\b/.test(text)) add('warn', 'tokens-link', null, 'Screen does not load design/tokens/tokens.css; values may not come from tokens.');

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
  slopHtml(text, add);
  return out;
}

// Visual properties belong to the design system (tokens + component masters);
// screens may only arrange components (layout).
const VISUAL_PROPS = /^(color|background(-color|-image)?|border(-(top|right|bottom|left))?(-color)?|outline-color|font(-family|-size|-weight|-style)?|line-height|letter-spacing|text-transform|box-shadow|border-radius|text-decoration(-color)?|fill|stroke)$/;

/** Screen-only rules: compose issues, visual styling outside the design system, raw elements that have a component. */
export function lintScreen(root, rel, text, lib = loadComponents(root)) {
  const out = [];
  const add = (level, rule, line, message) => out.push({ level, rule, file: rel, line: line || 1, message, source: 'dfw' });
  const r = composeHtml(root, text, { lib, designBase: designBaseFor(rel) });
  for (const i of r.issues) add(i.level, i.rule, i.line, i.message);
  if (lib.size) {
    for (const sm of text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)) {
      const css = sm[1].replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, ' '));
      const offset = sm.index + sm[0].indexOf(sm[1]);
      for (const dm of css.matchAll(/(^|[;{\s])([a-z-]+)\s*:\s*([^;{}]+)/g)) {
        const prop = dm[2];
        if (prop.startsWith('--') || !VISUAL_PROPS.test(prop)) continue;
        add('error', 'screen-visual-style', lineOf(text, offset + dm.index), `"${prop}" set in a screen. Visual styling lives in the design system: use or extend a component variant (design/system/components/) or a token; screens only handle layout.`);
      }
    }
    for (const m of text.matchAll(/\sstyle\s*=\s*"([^"]*)"/g)) {
      for (const d of m[1].split(';')) {
        const prop = d.split(':')[0].trim().toLowerCase();
        if (prop && !prop.startsWith('--') && VISUAL_PROPS.test(prop)) add('error', 'screen-visual-style', lineOf(text, m.index), `Inline "${prop}" in a screen; visual styling belongs to the design system.`);
      }
    }
    const replaced = new Map();
    for (const d of lib.values()) for (const t of d.replaces) replaced.set(t, d.name);
    const body = Math.max(0, text.search(/<body/i));
    // Raw elements inside instances whose master accepts them (segmented buttons, tabs) are fine.
    const allowed = [];
    for (const u of text.matchAll(/<dfw-use\b[^>]*\bc="([\w-]+)"/g)) {
      const d = lib.get(u[1]);
      if (!d?.accepts?.length) continue;
      const el = findElement(text, u.index);
      if (el) allowed.push({ from: el.innerStart, to: el.innerEnd, tags: d.accepts });
    }
    const counts = new Map();
    for (const [tag, comp] of replaced) {
      const re = new RegExp(`<${tag}\\b`, 'gi');
      for (const m of text.matchAll(re)) {
        if (m.index < body) continue;
        if (allowed.some((r) => m.index > r.from && m.index < r.to && r.tags.includes(tag))) continue;
        if (!counts.has(tag)) counts.set(tag, { n: 0, line: lineOf(text, m.index), comp });
        counts.get(tag).n++;
      }
    }
    for (const [tag, c] of counts) add('warn', 'use-component', c.line, `${c.n} raw <${tag}> element(s); the design system has component "${c.comp}" for this. Use <dfw-use c="${c.comp}">.`);
    const primaries = (text.match(/variant="primary"/g) || []).length;
    if (primaries > 2) add('warn', 'primary-overuse', 1, `${primaries} primary-variant instances on one screen: the accent loses its meaning. Keep one primary action per area (two at most per screen).`);
    if (!r.instances.length && /<body/i.test(text)) add('warn', 'no-components', body ? lineOf(text, body) : 1, 'Screen uses no design-system components; build screens from design/system/components.');
    if (lib.has('page') && r.instances.length && !r.instances.some((x) => x.component === 'page') && !rel.includes('/_')) add('warn', 'page-layout', 1, 'Screen does not use the page layout master (<dfw-use c="page" variant="task|flow|split">): paddings, widths and overflow behaviour will drift from other screens.');
    if (lib.has('app-shell') && r.instances.length && !r.instances.some((x) => x.component === 'app-shell') && !rel.includes('/_')) add('warn', 'app-shell', 1, 'Screen does not use the shared app-shell: every screen should be framed by the same navigation and layout (<dfw-use c="app-shell" current="...">).');
  }
  out.push(...stateCoverage(root, rel, text));
  return out;
}

// Data surfaces and forms must design their non-happy states (the most common silent gap in AI-built UI).
const DATA_SURFACE_RE = /<table\b|<dfw-use\b[^>]*\bc="[\w-]*(?:table|list|grid|feed)[\w-]*"/i;
const FORM_RE = /<form\b|<dfw-use\b[^>]*\bc="[\w-]*form[\w-]*"/i;

/** Warn when a screen with a table/list or a form declares no loading/empty/error states (design.json states). */
export function stateCoverage(root, rel, text) {
  const m = readJson(projectPaths(root).manifest, null);
  const file = rel.replace(/^design\//, '');
  const sc = (m?.screens || []).find((s) => (s.variants || []).some((v) => v.file === file));
  if (!sc || String(sc.id).startsWith('_')) return [];
  const states = new Set((sc.states?.length ? sc.states : m.states || []).map((x) => String(x).toLowerCase()));
  const need = [];
  if (DATA_SURFACE_RE.test(text)) need.push('loading', 'empty', 'error');
  if (FORM_RE.test(text)) need.push('error');
  const missing = [...new Set(need)].filter((s) => !states.has(s));
  if (!missing.length) return [];
  return [{ level: 'warn', rule: 'state-coverage', file: rel, line: 1, source: 'dfw', message: `Screen "${sc.id}" has ${DATA_SURFACE_RE.test(text) ? 'a data surface' : 'a form'} but no ${missing.join('/')} state. Add them to its "states" in design.json and design each (.when-<state> blocks): empty needs a headline, explanation and primary action; error says what happened and how to recover, keeping user input. Also check edge content (very long text, missing optional fields, large counts).` }];
}

/** Lint a component master file. */
export function lintComponentFile(rel, text) {
  const out = [];
  const add = (level, rule, index, message) => out.push({ level, rule, file: rel, line: index == null ? 1 : lineOf(text, index), message, source: 'dfw' });
  if (!/<dfw-component\b/.test(text)) add('error', 'component-missing', null, 'Component file has no <dfw-component name="..."> element.');
  for (const m of text.matchAll(/<dfw-component\b([^>]*)>/g)) {
    if (!/\bname\s*=\s*"[a-z][a-z0-9-]*"/.test(m[1])) add('error', 'component-name', m.index, 'Component needs name="kebab-case".');
  }
  if (!/<template\b/.test(text)) add('error', 'component-template', null, 'Component has no <template>.');
  for (const sm of text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)) {
    for (const cm of sm[1].matchAll(COLOR_RE)) {
      const before = sm[1].slice(Math.max(0, cm.index - 80), cm.index);
      if (/--[\w-]+\s*:\s*[^;]*$/.test(before)) continue;
      add('error', 'hardcoded-color', sm.index + sm[0].indexOf(sm[1]) + cm.index, `Hard-coded color "${cm[0]}" in a component; use a color token.`);
    }
    for (const pm of sm[1].matchAll(/(?:padding|margin|gap|font-size|border-radius)[\w-]*\s*:\s*([^;}]*)/g)) {
      const raw = pm[1].replace(/var\([^)]*\)/g, '').match(/\b(\d+(?:\.\d+)?)px\b/g) || [];
      if (!raw.some((v) => parseFloat(v) > 2)) continue;
      add('warn', 'raw-dimension', sm.index + sm[0].indexOf(sm[1]) + pm.index, `Raw length "${pm[0].trim()}" in a component; use space/font/radius tokens.`);
    }
  }
  slopHtml(text, add);
  // Interactive masters design their state matrix: hover, keyboard focus, disabled.
  const tpl = (/<template\b[^>]*>([\s\S]*?)<\/template>/.exec(text) || [])[1] || '';
  // Only the root element counts (a card containing a link is not a control); native controls keep their own states unless restyled.
  const root = (/^\s*<([a-z][\w-]*)\b([^>]*)>/i.exec(tpl) || [])[1]?.toLowerCase();
  const css = [...text.matchAll(/<style[^>]*>([\s\S]*?)<\/style>/gi)].map((x) => x[1]).join('\n');
  const need = [];
  if (root === 'button' || root === 'a' || /^\s*<[^>]*role="(?:button|tab|switch|menuitem)"/.test(tpl)) need.push([':hover', /:hover/]);
  if (root === 'button' || (/^(input|select|textarea)$/.test(root || '') && /appearance\s*:\s*none/.test(css))) need.push(['disabled', /:disabled|\[disabled\]|\[aria-disabled/]);
  if (/outline\s*:\s*(none|0)\b/.test(css)) need.push([':focus-visible', /:focus-visible|:focus-within/]);
  const missing = need.filter(([, re]) => !re.test(css)).map(([n]) => n);
  if (missing.length) add('warn', 'component-states', null, `Interactive component has no ${missing.join(' / ')} style. Design its state matrix (default, hover, focus-visible, active, disabled, plus loading/selected where they apply).`);
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
  const add = (level, rule, index, message) => out.push({ level, rule, file: rel, line: lineOf(clean, index), message, source: 'dfw' });
  slopCss(clean, (i) => i, add);
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
  // Near-duplicate colours: two tokens that look the same are one decision made twice.
  const colors = [];
  for (const [name, { node, type }] of leaves) {
    if (type !== 'color') continue;
    const hex = typeof node.$value === 'string' ? node.$value : node.$value?.hex;
    if (typeof hex === 'string' && /^#[0-9a-f]{6}$/i.test(hex)) colors.push({ name, hex: hex.toLowerCase(), c: hexToOklch(hex) });
  }
  const reported = new Set();
  for (let i = 0; i < colors.length; i++) {
    for (let j = i + 1; j < colors.length; j++) {
      const a = colors[i];
      const b = colors[j];
      if (a.hex === b.hex || reported.has(b.name)) continue; // identical values are usually deliberate aliases
      const dh = Math.min(Math.abs(a.c.h - b.c.h), 360 - Math.abs(a.c.h - b.c.h)) * Math.min(a.c.c, b.c.c) * 0.0175;
      const d = Math.hypot(a.c.l - b.c.l, a.c.c - b.c.c, dh);
      if (d < 0.005) {
        reported.add(b.name);
        out.push({ level: 'warn', rule: 'near-duplicate-color', file: rel, line: 1, message: `Tokens "${a.name}" (${a.hex}) and "${b.name}" (${b.hex}) are visually the same colour. Merge them or alias one to the other.`, source: 'dfw' });
      }
    }
  }
  for (const [name, { node }] of leaves) {
    for (const s of strings(node.$value)) {
      const what = AI_DEFAULT_COLORS[String(s).trim().toLowerCase()];
      if (what) out.push({ level: 'warn', rule: 'ai-default-color', file: rel, line: 1, message: `Token "${name}" is ${s} (${what}), a colour AI-generated UIs default to. Derive the palette from the brand (dfw palette build); keep it only if the brand requires it (dfw decide).`, source: 'dfw' });
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
  if (!Array.isArray(m.screens) || !m.screens.length) out.push({ level: 'warn', rule: 'manifest-screens', file: rel, line: 1, message: 'Manifest has no screens yet (fine during the system phase).', source: 'dfw' });
  const screenIds = new Set((m.screens || []).map((x) => x.id));
  for (const sc of m.screens || []) {
    if (String(sc.id).startsWith('_')) continue;
    const bp = sc.blueprint;
    if (!bp) out.push({ level: 'warn', rule: 'blueprint', file: rel, line: 1, message: `Screen "${sc.id}" has no blueprint (goal, layout, aboveFold, data, feedback). Agree it with the user before building: it is cheaper to fix a blueprint than a screen.`, source: 'dfw' });
    else for (const k of ['goal', 'layout', 'aboveFold', 'data', 'feedback']) if (!bp[k] || (Array.isArray(bp[k]) && !bp[k].length)) out.push({ level: 'warn', rule: 'blueprint', file: rel, line: 1, message: `Screen "${sc.id}" blueprint is missing "${k}".`, source: 'dfw' });
  }
  for (const pg of m.pages || []) {
    for (const sid of pg.screens || []) if (!screenIds.has(sid)) out.push({ level: 'error', rule: 'manifest-page-screen', file: rel, line: 1, message: `Page "${pg.id}" lists unknown screen "${sid}".`, source: 'dfw' });
  }
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
    ? files.flatMap((f) => {
        const abs = path.resolve(root, f);
        // A directory target lints every file under it.
        if (fs.existsSync(abs) && fs.statSync(abs).isDirectory()) return listFiles(abs).map((r) => toPosix(path.relative(root, path.join(abs, r))));
        return [toPosix(path.relative(root, abs))];
      })
    : listFiles(p.design, { exclude: ['approved'] }).map((r) => `design/${r}`);
  if (!files || targets.includes('design/design.json')) out.push(...lintManifest(root));
  const lib = loadComponents(root);
  for (const rel of targets) {
    if (rel.startsWith('design/approved/')) continue;
    const abs = path.join(root, rel);
    if (!fs.existsSync(abs)) continue;
    const text = fs.readFileSync(abs, 'utf8');
    if (rel === 'design/system/head.html') continue; // font links snippet, not a page
    if (rel.startsWith('design/system/components/') && rel.endsWith('.html')) out.push(...lintComponentFile(rel, text));
    else if (rel.endsWith('.html')) {
      out.push(...lintHtml(rel, text));
      if (rel.startsWith('design/screens/')) out.push(...lintScreen(root, rel, text, lib));
    } else if (rel === 'design/system/components.css') continue;
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
