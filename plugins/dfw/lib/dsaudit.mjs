// Senior design-system review of the project's tokens and DESIGN.md: contrast, semantic colour
// collisions, neutral temperature, hue budget, type rhythm and hierarchy, family/radius sprawl,
// tinted elevation, and whether every decision has a written reason.
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, readJson } from './util.mjs';
import { auditColors, auditTypeScale, hexToOklch } from './color.mjs';

function collect(tree) {
  const out = new Map();
  const walk = (node, trail, type) => {
    if (typeof node !== 'object' || node === null) return;
    const t = node.$type || type;
    if ('$value' in node) return void out.set(trail.join('.'), { value: node.$value, type: t, description: node.$description || '' });
    for (const [k, v] of Object.entries(node)) if (!k.startsWith('$')) walk(v, [...trail, k], t);
  };
  walk(tree, [], undefined);
  return out;
}

function resolver(tokens) {
  const get = (name, depth = 0) => {
    const t = tokens.get(name);
    if (!t || depth > 10) return null;
    if (typeof t.value === 'string' && /^\{[^}]+\}$/.test(t.value)) return get(t.value.slice(1, -1), depth + 1);
    return t;
  };
  const hex = (name) => {
    const t = get(name);
    if (!t) return null;
    if (typeof t.value === 'string') return t.value.startsWith('#') ? t.value.slice(0, 7) : null;
    return t.value?.hex || null;
  };
  return { get, hex };
}

const ROLES = ['canvas', 'surface', 'surface-sunken', 'surface-hover', 'ink', 'muted', 'subtle', 'border', 'border-strong', 'rule', 'accent', 'accent-hover', 'accent-soft', 'accent-text', 'on-accent', 'focus', 'focus-ring', 'brand', 'on-brand', 'signature'];

export function auditDesignSystem(root) {
  const p = projectPaths(root);
  const f = path.join(p.tokens, 'tokens.json');
  const findings = [];
  const add = (level, rule, message) => findings.push({ level, rule, file: 'design/tokens/tokens.json', line: 1, message, source: 'dfw-ds' });
  if (!fs.existsSync(f)) return findings;
  const tree = readJson(f, {});
  const tokens = collect(tree);
  const r = resolver(tokens);
  const roles = {};
  for (const k of ROLES) {
    const h = r.hex(`color.${k}`);
    if (h) roles[k] = h;
  }
  const status = {};
  for (const s of ['ok', 'info', 'warn', 'err']) for (const suf of ['', '-tint', '-text']) {
    const h = r.hex(`color.status.${s}${suf}`);
    if (h) status[`${s}${suf}`] = h;
  }
  const exceptions = tree.$extensions?.dfw?.exceptions || [];
  const onFill = r.hex('color.on-fill') || '#ffffff';
  if (roles.accent && roles.surface && roles.ink) {
    for (const x of auditColors({ roles, status, onFill }, { exceptions })) add(x.level, x.rule, x.message);
  }

  // Usage rules: every palette entry and role should say what it is for.
  const colorTokens = [...tokens].filter(([n, t]) => t.type === 'color' && !n.startsWith('color.status.'));
  const undocumented = colorTokens.filter(([, t]) => !t.description).map(([n]) => n);
  if (undocumented.length > colorTokens.length / 3) add('warn', 'usage-rules', `${undocumented.length} of ${colorTokens.length} colour tokens have no $description (usage rule), e.g. ${undocumented.slice(0, 4).join(', ')}. A senior palette says what each colour is for and what it must never be used for.`);

  // Type.
  const size = (k) => {
    const t = r.get(`font.size.${k}`);
    return t?.value?.value ?? null;
  };
  const ui = ['2xs', 'xs', 'sm', 'md', 'lg', 'xl', '2xl'].map(size).filter(Boolean);
  const display = ['display-sm', 'display-md', 'display-lg', 'display-xl'].map(size).filter(Boolean);
  const manifest = readJson(p.manifest, {});
  for (const x of auditTypeScale({ ui, display }, { body: size('md') || 16, surface: manifest.surface || 'product' })) add(x.level, x.rule, x.message);
  if (!display.length) add('warn', 'type-display', 'No display sizes (font.size.display-*): titles and key numbers will sit at body scale, the most common "AI look".');
  const families = new Set([...tokens].filter(([, t]) => t.type === 'fontFamily').map(([, t]) => (Array.isArray(t.value) ? t.value[0] : t.value)));
  if (families.size > 3) add('warn', 'type-families', `${families.size} font families (${[...families].join(', ')}); 2 (+ mono) is the ceiling.`);

  // Shape and elevation.
  const radii = new Set([...tokens].filter(([n]) => n.startsWith('radius.') && !n.endsWith('pill')).map(([, t]) => t.value?.value));
  if (radii.size > 5) add('warn', 'radius-sprawl', `${radii.size} radius values; 3-4 steps keep shapes consistent.`);
  const shadows = [...tokens].filter(([, t]) => t.type === 'shadow');
  const canvasC = roles.canvas ? hexToOklch(roles.canvas).c : 0;
  for (const [n, t] of shadows) {
    const layers = Array.isArray(t.value) ? t.value : [t.value];
    const grey = layers.every((l) => {
      const c = l.color?.components;
      return c && Math.max(...c) - Math.min(...c) < 0.02 && Math.max(...c) < 0.05;
    });
    if (grey && canvasC > 0.008) {
      add('warn', 'shadow-tint', `${n} uses a neutral black shadow on a tinted canvas; tint shadows with the darkest neutral so elevation belongs to the palette.`);
      break;
    }
  }
  // Spacing grid.
  const off = [...tokens].filter(([n, t]) => n.startsWith('space.') && t.value?.unit === 'px' && t.value.value > 2 && t.value.value % 4 !== 0 && t.value.value !== 6);
  if (off.length) add('warn', 'space-grid', `Spacing off the 4px grid: ${off.map(([n, t]) => `${n}=${t.value.value}`).join(', ')}.`);

  // Rationale in DESIGN.md.
  const design = path.join(root, 'DESIGN.md');
  const text = fs.existsSync(design) ? fs.readFileSync(design, 'utf8') : '';
  let body = '';
  for (const part of text.split(/^(?=## )/m)) if (/^## (?:Rationale|Decisions and rationale)\s*$/m.test(part.split('\n')[0])) body += part.split('\n').slice(1).join('\n');
  const bullets = (body.match(/^\s*[-*] .{20,}/gm) || []).length;
  if (bullets < 5) findings.push({ level: 'warn', rule: 'rationale', file: 'DESIGN.md', line: 1, message: `DESIGN.md "## Rationale" has ${bullets} decision(s); write at least 5 "decision - because <context fact or principle>" lines (accent choice, neutrals, type scale, density, motif).`, source: 'dfw-ds' });
  return findings;
}
