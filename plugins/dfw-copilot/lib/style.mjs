// Curated style-guide library (kits/styles/*.json): search by tags, show, and apply a guide
// as DTCG tokens on top of the kit contract (named palette with usage rules + semantic roles
// as aliases), fonts, display scale, shape, tinted elevation, density and signature motifs.
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, readJson, writeJson, DfwError } from './util.mjs';

export const STYLES_DIR = path.join(PLUGIN_ROOT, 'kits', 'styles');
const PRESETS_DIR = path.join(PLUGIN_ROOT, 'kits', 'base', 'presets');

export function listStyles() {
  if (!fs.existsSync(STYLES_DIR)) return [];
  return fs
    .readdirSync(STYLES_DIR)
    .filter((f) => f.endsWith('.json'))
    .map((f) => readJson(path.join(STYLES_DIR, f)));
}

export function getStyle(id) {
  if (/\.json$/.test(id)) {
    // A project-specific guide (same format as kits/styles/*.json), e.g. design/system/style-source.json.
    const g = readJson(path.resolve(id), null);
    if (!g) throw new DfwError(`Style guide file not found: ${id}`);
    return { status: getStyle('swiss-grid').status, onFill: '#ffffff', motifs: [], contrasts: [], fits: [], tags: [], do: [], dont: [], components: [], ...g };
  }
  const f = path.join(STYLES_DIR, `${id}.json`);
  if (!fs.existsSync(f)) throw new DfwError(`Unknown style guide "${id}". List them: dfw style list`);
  return readJson(f);
}

export const isStyle = (id) => /\.json$/.test(id) || fs.existsSync(path.join(STYLES_DIR, `${id}.json`));

/** Rank guides by tag/concept match. */
export function searchStyles(query) {
  const words = String(query).toLowerCase().split(/[\s,]+/).filter(Boolean);
  return listStyles()
    .map((g) => {
      const hay = `${g.name} ${g.concept} ${g.fits.join(' ')}`.toLowerCase();
      let score = 0;
      for (const w of words) {
        if (g.tags.includes(w)) score += 5;
        else if (g.tags.some((t) => t.includes(w))) score += 2;
        if (hay.includes(w)) score += 1;
      }
      return { id: g.id, name: g.name, score, tags: g.tags, concept: g.concept };
    })
    .filter((x) => x.score > 0 || !words.length)
    .sort((a, b) => b.score - a.score || a.id.localeCompare(b.id));
}

const hexComp = (h) => [1, 3, 5].map((i) => Math.round((parseInt(h.slice(i, i + 2), 16) / 255) * 10000) / 10000);
const col = (hex, d) => ({ $value: { colorSpace: 'srgb', components: hexComp(hex), hex: hex.toLowerCase() }, ...(d ? { $description: d } : {}) });
const px = (v) => ({ $value: { value: v, unit: 'px' } });

const ROLE_DESC = {
  canvas: 'App/page background.', surface: 'Cards, tables, inputs.', 'surface-sunken': 'Table headers, wells, quiet panels.', 'surface-hover': 'Hover background.',
  ink: 'Primary text.', muted: 'Secondary text.', subtle: 'Placeholders, disabled text.', border: 'Control boundaries (3:1).', 'border-strong': 'Hovered/emphasised boundaries.',
  rule: 'Dividers only, never the sole boundary of a control.', accent: 'Primary action, current item, links.', 'accent-hover': 'Hover/pressed accent.', 'accent-soft': 'Selected rows, active nav background.',
  'accent-text': 'Accent-coloured text on light backgrounds.', 'on-accent': 'Text on accent fills.', focus: 'Focus ring.', 'focus-ring': 'Soft focus halo.',
  brand: 'Large brand surfaces (headers, hero bands, sidebars).', 'on-brand': 'Text on brand surfaces.', signature: 'Signature motif colour (ornament, tape, rules) — not for body text.',
};
const DENSITY = {
  compact: { control: 32, 'control-sm': 28, 'control-lg': 40 },
  regular: { control: 40, 'control-sm': 34, 'control-lg': 48 },
  large: { control: 44, 'control-sm': 36, 'control-lg': 56 },
};

function shadows(tint, hard) {
  const c = hexComp(tint);
  const layer = (x, y, b, s, a) => ({ color: { colorSpace: 'srgb', components: c, alpha: a }, offsetX: { value: x, unit: 'px' }, offsetY: { value: y, unit: 'px' }, blur: { value: b, unit: 'px' }, spread: { value: s, unit: 'px' } });
  const out = {
    $type: 'shadow',
    sm: { $value: [layer(0, 1, 2, 0, 0.08)], $description: `Tinted with ${tint}, not black.` },
    md: { $value: [layer(0, 2, 6, -1, 0.08), layer(0, 12, 28, -8, 0.16)] },
    lg: { $value: [layer(0, 4, 10, -2, 0.1), layer(0, 24, 56, -12, 0.24)] },
  };
  if (hard) out.hard = { $value: [{ ...layer(4, 4, 0, 0, 1) }], $description: 'Hard offset shadow (signature of this style).' };
  return out;
}

function fontsFor(g, which) {
  const f = which === 'latin' && g.fontsLatin ? g.fontsLatin : g.fonts;
  return f;
}

export function headFor(g, which = 'vi') {
  const f = fontsFor(g, which);
  const fams = new Map();
  const add = (name, w) => fams.set(name, [...new Set([...(fams.get(name) || []), ...w.split(';')])]);
  add(f.body, '400;500;600;700');
  add(f.display, f.displayWeights || '400;600');
  add(f.mono, '400;500');
  const q = [...fams].map(([n, w]) => `family=${n.replace(/ /g, '+')}:wght@${w.map(Number).sort((a, b) => a - b).join(';')}`).join('&');
  return `<!-- Fonts for style "${g.id}" (${which === 'latin' ? 'English-only' : 'Vietnamese-capable'}; SIL OFL). Self-host for production. -->\n<link rel="preconnect" href="https://fonts.googleapis.com">\n<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>\n<link rel="stylesheet" href="https://fonts.googleapis.com/css2?${q}&display=swap">\n`;
}

/** DTCG token tree for a guide (pure). */
export function styleTokens(g, { fonts = 'vi' } = {}) {
  const base = readJson(path.join(PRESETS_DIR, `${g.base}.tokens.json`));
  const t = JSON.parse(JSON.stringify(base));
  const palette = Object.fromEntries(Object.entries(g.palette).map(([k, v]) => [k, col(v.hex, v.usage)]));
  const roleVal = (v) => (v.startsWith('#') ? col(v).$value : `{color.palette.${v}}`);
  const status = Object.fromEntries(Object.entries(g.status).map(([k, v]) => [k, col(v)]));
  t.color = {
    $type: 'color',
    palette,
    ...Object.fromEntries(Object.entries(g.roles).map(([k, v]) => [k, { $value: roleVal(v), $description: ROLE_DESC[k] || '' }])),
    'on-fill': col(g.onFill || '#ffffff', 'Text on ok/err/info fills.'),
    'on-warn': col('#111518', 'Text on the warn fill.'),
    status,
  };
  const f = fontsFor(g, fonts);
  const stack = (name, fallback) => [name, ...fallback];
  t.font.family = {
    $type: 'fontFamily',
    body: { $value: stack(f.body, ['Segoe UI', 'system-ui', 'sans-serif']) },
    display: { $value: stack(f.display, /Serif|Garamond|Playfair|Bodoni|Fraunces|Newsreader|Lora|Merriweather/.test(f.display) ? ['Georgia', 'serif'] : ['Segoe UI', 'system-ui', 'sans-serif']), $description: g.fonts.note || '' },
    mono: { $value: stack(f.mono, ['Consolas', 'monospace']) },
  };
  const [xl, lg, md, sm] = g.type.display;
  if (g.type.ui) for (const [k, v] of Object.entries(g.type.ui)) t.font.size[k] = { ...px(v), ...(k === 'md' ? { $description: `Body. Scale: base ${g.type.base}, UI ratio ${g.type.ratio}, display ratio ${g.type.displayRatio}.` } : {}) };
  Object.assign(t.font.size, { 'display-xl': px(xl), 'display-lg': px(lg), 'display-md': px(md), 'display-sm': px(sm) });
  const serifDisplay = /Serif|Garamond|Playfair|Bodoni|Fraunces|Newsreader/.test(f.display);
  t.font.weight.display = { $value: serifDisplay ? 400 : 600 };
  t.font.leading.display = { $value: serifDisplay ? 1.05 : 1.02 };
  t.font.tracking.display = { $value: { value: serifDisplay ? -0.01 : -0.025, unit: 'em' } };
  t.font.eyebrow = {
    size: { $type: 'dimension', $value: { value: g.type.eyebrow.size, unit: 'px' } },
    weight: { $type: 'fontWeight', $value: g.type.eyebrow.weight },
    tracking: { $type: 'dimension', $value: { value: parseFloat(g.type.eyebrow.tracking), unit: 'em' } },
    case: { $type: 'string', $value: g.type.eyebrow.upper ? 'uppercase' : 'none', $description: 'text-transform for eyebrows (dfw string token)' },
  };
  t.radius = { $type: 'dimension', ...Object.fromEntries(Object.entries(g.radius).map(([k, v]) => [k, px(v)])), pill: px(999) };
  Object.assign(t.size, Object.fromEntries(Object.entries(DENSITY[g.density] || DENSITY.regular).map(([k, v]) => [k, px(v)])));
  t.shadow = shadows(g.shadowTint, g.hardShadow);
  t.$description = `Style guide "${g.name}" (${g.id}): ${g.concept}`;
  t.$extensions = { dfw: { style: g.id, fonts } };
  return t;
}

/** Motif masters as one component with a variant per motif. */
export function motifComponent(g) {
  if (!g.motifs?.length) return null;
  const desc = g.motifs.map((m) => `${m.name}: ${m.use}`).join(' | ').replace(/"/g, "'");
  return `<dfw-component name="motif" group="Signature" variants="${g.motifs.map((m) => m.name).join(' ')}" props="as=div"
  description="Signature motifs of the ${g.name} style. ${desc}">
<style>
/* Motifs belong to the "${g.id}" style. While comparing other option themes, decorative motifs
   are hidden and motifs with content fall back to plain text (content never disappears). */
[data-theme^="opt-"]:not([data-theme="opt-${g.id}"]) .motif:empty { display: none; }
[data-theme^="opt-"]:not([data-theme="opt-${g.id}"]) .motif:not(:empty) { all: unset; font-family: var(--font-family-mono); font-size: var(--font-size-sm); color: var(--color-muted); }
${g.motifs.map((m) => m.css).join('\n')}
</style>
<template>
<{{as}} class="motif motif--{{variant}}"><dfw-slot></dfw-slot></{{as}}>
</template>
<dfw-examples>
${g.motifs.map((m) => `<div style="min-width:240px"><dfw-use c="motif" variant="${m.name}">${m.name}</dfw-use></div>`).join('\n')}
</dfw-examples>
</dfw-component>
`;
}

/** Apply a guide to the project: tokens.json, head.html, motif master, design/system/style.json. */
export function applyStyle(root, id, { fonts = 'vi' } = {}) {
  const g = getStyle(id);
  id = g.id;
  const p = projectPaths(root);
  fs.mkdirSync(p.tokens, { recursive: true });
  writeJson(path.join(p.tokens, 'tokens.json'), styleTokens(g, { fonts }));
  const sys = path.join(p.design, 'system');
  fs.mkdirSync(path.join(sys, 'components'), { recursive: true });
  fs.writeFileSync(path.join(sys, 'head.html'), headFor(g, fonts));
  const motif = motifComponent(g);
  if (motif) fs.writeFileSync(path.join(sys, 'components', 'motif.html'), motif);
  const { palette, roles, concept, fits, avoid, layout, components, do: dos, dont, motifs, name, tags, contrasts, rationale = [], seeds, type, axes, audit = [], paletteNotes = [] } = g;
  writeJson(path.join(sys, 'style.json'), { id, name, tags, axes, concept, rationale, seeds, type: { base: type.base, ratio: type.ratio, displayRatio: type.displayRatio }, fits, avoid, layout, components, do: dos, dont, motifs: motifs.map(({ name: n, use }) => ({ name: n, use })), roles, contrasts, audit, paletteNotes, fonts: fontsFor(g, fonts), fontsVariant: fonts });
  // Seed DESIGN.md with the guide's reasoning so the DS gate starts from reasons, not from a blank page.
  const designMd = path.join(root, 'DESIGN.md');
  if (fs.existsSync(designMd) && g.rationale?.length) {
    const text = fs.readFileSync(designMd, 'utf8');
    const block = `## Rationale\n<!-- Seeded from style guide "${id}". Rewrite each line for THIS product: decision - because <context fact or principle>. -->\n${g.rationale.map((r) => `- ${r}`).join('\n')}\n`;
    const seeded = /^## Rationale\s*\n<!-- Seeded from style guide "[^"]*"[\s\S]*?-->\n(?:- .*\n?)*/m;
    if (seeded.test(text)) fs.writeFileSync(designMd, text.replace(seeded, block)); // still the untouched seed of another guide: replace it
    else if (/^## Rationale\s*$/m.test(text)) {
      if (!/^## Rationale\s*$\n(?:<!--[\s\S]*?-->\n)?- /m.test(text)) fs.writeFileSync(designMd, text.replace(/^## Rationale\s*$\n(?:<!--[\s\S]*?-->\n)?/m, block));
    } else fs.writeFileSync(designMd, `${text.trimEnd()}\n\n${block}`);
  }
  return { id, fonts: fontsFor(g, fonts) };
}

export const AXES = {
  formality: ['casual', 'formal'], energy: ['calm', 'energetic'], era: ['traditional', 'modern'], warmth: ['cool', 'warm'],
  density: ['airy', 'dense'], richness: ['minimal', 'ornamented'], seriousness: ['playful', 'serious'],
};

/**
 * Rank guides against a brief's personality (1-5 per axis) and hard constraints. Explains the fit
 * so the choice is reasoned, not keyword-matched.
 * brief: { personality: {axis: 1-5}, weights?: {axis: n}, constraints?: { dark?: bool, surface?: 'product'|'marketing' }, keywords?: [] }
 */
export function matchStyles(brief) {
  const want = brief.personality || {};
  const w = brief.weights || {};
  return listStyles()
    .map((g) => {
      let dist = 0;
      let total = 0;
      const diffs = [];
      for (const axis of Object.keys(AXES)) {
        if (want[axis] == null || g.axes?.[axis] == null) continue;
        const weight = w[axis] ?? 1;
        const d = Math.abs(want[axis] - g.axes[axis]);
        dist += d * d * weight;
        total += 16 * weight;
        if (d >= 2) diffs.push(`${axis} ${g.axes[axis]} vs wanted ${want[axis]} (more ${AXES[axis][g.axes[axis] > want[axis] ? 1 : 0]})`);
      }
      let fit = total ? Math.round(100 * (1 - Math.sqrt(dist / total))) : 0;
      const notes = [];
      if (brief.constraints?.dark != null && Boolean(g.dark) !== Boolean(brief.constraints.dark)) {
        fit -= 25;
        notes.push(g.dark ? 'dark-first style' : 'light-first style');
      }
      const kw = (brief.keywords || []).filter((k) => g.tags.includes(String(k).toLowerCase()));
      fit = Math.min(100, fit + kw.length * 3);
      return { id: g.id, name: g.name, fit, matches: kw, mismatches: diffs, notes, concept: g.concept };
    })
    .sort((a, b) => b.fit - a.fit);
}

export function formatStyle(g) {
  const lines = [
    `${g.name} (${g.id})`,
    `Concept: ${g.concept}`,
    `Fits: ${g.fits.join('; ')}${g.avoid?.length ? ` · Avoid: ${g.avoid.join('; ')}` : ''}`,
    `Tags: ${g.tags.join(', ')}`,
    `Fonts (VI): display ${g.fonts.display}, body ${g.fonts.body}, mono ${g.fonts.mono}${g.fontsLatin ? ` · English-only alt: ${g.fontsLatin.display} / ${g.fontsLatin.body}` : ''}`,
    `Type: base ${g.type.base}, UI ratio ${g.type.ratio}, display ratio ${g.type.displayRatio} -> display ${g.type.display.join(' / ')} px · radius ${Object.values(g.radius).join('/')} · density ${g.density}${g.dark ? ' · dark' : ''}`,
    `Personality: ${Object.entries(g.axes || {}).map(([k, v]) => `${k} ${v}/5`).join(', ')}`,
    'Why (senior rationale):',
    ...(g.rationale || []).map((r) => `  - ${r}`),
    'Palette (OKLCH ramps; role steps):',
    ...Object.entries(g.roles).map(([k, v]) => `  ${k.padEnd(14)} ${v.startsWith('#') ? v : `${v} ${g.palette[v]?.hex || ''}`}`),
    `Roles: accent=${g.roles.accent}, ink=${g.roles.ink}, canvas=${g.roles.canvas}, brand=${g.roles.brand}, signature=${g.roles.signature}`,
    'Motifs:',
    ...g.motifs.map((m) => `  ${m.name}: ${m.use}`),
    `Layout: ${g.layout}`,
    'Components:',
    ...g.components.map((c) => `  - ${c}`),
    `Do: ${g.do.join(' · ')}`,
    `Don't: ${g.dont.join(' · ')}`,
  ];
  return lines.join('\n');
}
