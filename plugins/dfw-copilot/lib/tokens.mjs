// Minimal DTCG (Design Tokens Format Module 2025.10) -> CSS custom properties.
// Covers the types a design studio needs; Style Dictionary takes over at
// implementation time for framework-specific outputs.
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, DfwError } from './util.mjs';

function collect(tree) {
  const tokens = new Map();
  const walk = (node, trail, type) => {
    if (typeof node !== 'object' || node === null) return;
    const t = node.$type || type;
    if ('$value' in node) {
      tokens.set(trail.join('.'), { value: node.$value, type: t });
      return;
    }
    for (const [k, v] of Object.entries(node)) if (!k.startsWith('$')) walk(v, [...trail, k], t);
  };
  walk(tree, [], undefined);
  return tokens;
}

export const cssVarName = (name) => `--${name.replace(/\./g, '-')}`;

function fmtNumber(n) {
  return Number.isInteger(n) ? String(n) : String(Math.round(n * 10000) / 10000);
}

function colorToCss(v) {
  if (typeof v === 'string') return v;
  if (v && typeof v === 'object') {
    const a = v.alpha ?? 1;
    const c = (v.components || []).map((x) => (x === 'none' ? 'none' : x));
    switch (v.colorSpace) {
      case 'srgb':
        if (v.hex && a === 1) return v.hex;
        return `rgb(${c.map((x) => (x === 'none' ? 'none' : fmtNumber(x * 255))).join(' ')}${a !== 1 ? ` / ${a}` : ''})`;
      case 'oklch':
        return `oklch(${fmtNumber(c[0])} ${fmtNumber(c[1])} ${c[2] === 'none' ? 'none' : fmtNumber(c[2])}${a !== 1 ? ` / ${a}` : ''})`;
      case 'oklab':
      case 'lab':
      case 'lch':
      case 'hsl':
      case 'hwb':
        return `${v.colorSpace}(${c.map(fmtNumber).join(' ')}${a !== 1 ? ` / ${a}` : ''})`;
      default:
        if (v.hex) return v.hex;
        return `color(${v.colorSpace} ${c.map(fmtNumber).join(' ')}${a !== 1 ? ` / ${a}` : ''})`;
    }
  }
  throw new Error(`Unsupported color value ${JSON.stringify(v)}`);
}

function valueToCss(value, type, resolve) {
  if (typeof value === 'string') {
    const alias = value.match(/^\{([^{}]+)\}$/);
    if (alias) return `var(${cssVarName(alias[1])})`;
    return value.replace(/\{([^{}]+)\}/g, (_, n) => `var(${cssVarName(n)})`);
  }
  switch (type) {
    case 'color':
      return colorToCss(value);
    case 'dimension':
    case 'duration':
      if (typeof value === 'object' && 'value' in value) return `${fmtNumber(value.value)}${value.unit}`;
      return String(value);
    case 'number':
    case 'fontWeight':
      return String(value);
    case 'fontFamily':
      return (Array.isArray(value) ? value : [value]).map((f) => (/[\s,]/.test(f) ? `"${f}"` : f)).join(', ');
    case 'cubicBezier':
      return `cubic-bezier(${value.map(fmtNumber).join(', ')})`;
    case 'shadow': {
      const one = (s) =>
        [s.inset ? 'inset' : '', valueToCss(s.offsetX, 'dimension'), valueToCss(s.offsetY, 'dimension'), valueToCss(s.blur, 'dimension'), valueToCss(s.spread, 'dimension'), valueToCss(s.color, 'color')]
          .filter(Boolean)
          .join(' ');
      return (Array.isArray(value) ? value : [value]).map(one).join(', ');
    }
    case 'border':
      return `${valueToCss(value.width, 'dimension')} ${value.style} ${valueToCss(value.color, 'color')}`;
    case 'transition':
      return `${valueToCss(value.duration, 'duration')} ${valueToCss(value.timingFunction, 'cubicBezier')} ${valueToCss(value.delay ?? { value: 0, unit: 'ms' }, 'duration')}`;
    case 'strokeStyle':
      return typeof value === 'string' ? value : 'dashed';
    case 'gradient':
      return value.map((s) => `${valueToCss(s.color, 'color')} ${fmtNumber(s.position * 100)}%`).join(', ');
    default:
      if (typeof value === 'number') return String(value);
      return JSON.stringify(value);
  }
}

/** Returns an array of [cssVar, cssValue] declarations (typography expands to sub-properties). */
export function tokensToDeclarations(tree) {
  const tokens = collect(tree);
  const decls = [];
  for (const [name, { value, type }] of tokens) {
    if (type === 'typography' && typeof value === 'object' && !Array.isArray(value)) {
      const map = { fontFamily: 'fontFamily', fontSize: 'dimension', fontWeight: 'fontWeight', letterSpacing: 'dimension', lineHeight: 'number' };
      for (const [k, t] of Object.entries(map)) {
        if (value[k] !== undefined) decls.push([`${cssVarName(name)}-${k.replace(/[A-Z]/g, (c) => `-${c.toLowerCase()}`)}`, valueToCss(value[k], t)]);
      }
      continue;
    }
    decls.push([cssVarName(name), valueToCss(value, type)]);
  }
  return decls;
}

/**
 * Build design/tokens/tokens.css from design/tokens/tokens.json and optional
 * design/tokens/themes/<name>.json overrides (emitted as [data-theme="<name>"]).
 */
export function buildTokens(root) {
  const dir = projectPaths(root).tokens;
  const base = path.join(dir, 'tokens.json');
  if (!fs.existsSync(base)) throw new DfwError('design/tokens/tokens.json not found.');
  const parse = (f) => {
    try {
      return JSON.parse(fs.readFileSync(f, 'utf8'));
    } catch (e) {
      throw new DfwError(`${path.relative(root, f)}: ${e.message}`);
    }
  };
  const block = (sel, decls) => `${sel} {\n${decls.map(([k, v]) => `  ${k}: ${v};`).join('\n')}\n}\n`;
  let css = `/* Generated by "dfw tokens build" from design/tokens/*.json (W3C DTCG). Do not edit by hand. */\n`;
  css += block(':root', tokensToDeclarations(parse(base)));
  const themesDir = path.join(dir, 'themes');
  const themes = [];
  if (fs.existsSync(themesDir)) {
    for (const f of fs.readdirSync(themesDir).filter((x) => x.endsWith('.json')).sort()) {
      const name = f.replace(/\.json$/, '');
      themes.push(name);
      css += '\n' + block(`[data-theme="${name}"]`, tokensToDeclarations(parse(path.join(themesDir, f))));
    }
  }
  const out = path.join(dir, 'tokens.css');
  fs.writeFileSync(out, css);
  return { out, themes };
}
