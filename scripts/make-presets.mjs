#!/usr/bin/env node
// Generates the starter-kit token presets (W3C DTCG 2025.10) in core/kits/base/presets/.
// Every preset fills the same semantic contract the kit components use, so swapping a
// preset restyles every component. Contrast pairs are verified (WCAG 2.2 AA) before writing.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const OUT = path.join(ROOT, 'core', 'kits', 'base', 'presets');

const hexToComp = (h) => [1, 3, 5].map((i) => Math.round((parseInt(h.slice(i, i + 2), 16) / 255) * 10000) / 10000);
const col = (hex, d) => ({ $value: { colorSpace: 'srgb', components: hexToComp(hex), hex: hex.toLowerCase() }, ...(d ? { $description: d } : {}) });
const px = (v, d) => ({ $value: { value: v, unit: 'px' }, ...(d ? { $description: d } : {}) });
const shadow = (layers) => ({
  $value: layers.map(([x, y, b, s, a]) => ({ color: { colorSpace: 'srgb', components: [0.06, 0.07, 0.09], alpha: a }, offsetX: { value: x, unit: 'px' }, offsetY: { value: y, unit: 'px' }, blur: { value: b, unit: 'px' }, spread: { value: s, unit: 'px' } })),
});
const lum = (h) => {
  const c = hexToComp(h).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
};
const ratio = (a, b) => (Math.max(lum(a), lum(b)) + 0.05) / (Math.min(lum(a), lum(b)) + 0.05);

const STATUS = {
  ok: '#15803d', 'ok-tint': '#e8f5ec', 'ok-text': '#166534',
  info: '#1d4ed8', 'info-tint': '#e8effd', 'info-text': '#1e40af',
  warn: '#f0b429', 'warn-tint': '#fdf3d7', 'warn-text': '#7a4f00',
  err: '#c42b1c', 'err-tint': '#fdebe9', 'err-text': '#a32116',
};

const PRESETS = {
  crisp: {
    description: 'Crisp: neutral cool greys, indigo accent, compact controls, small radii, hairline dividers. Productive SaaS / admin tools.',
    fonts: { body: ['Inter', 'Segoe UI', 'system-ui', 'sans-serif'], display: ['Inter', 'Segoe UI', 'system-ui', 'sans-serif'], mono: ['JetBrains Mono', 'Consolas', 'monospace'] },
    head: '<link rel="preconnect" href="https://fonts.googleapis.com">\n<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>\n<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Inter:opsz,wght@14..32,400..700&family=JetBrains+Mono:wght@400;500&display=swap">',
    color: { canvas: '#f7f7f8', surface: '#ffffff', 'surface-sunken': '#f4f4f5', 'surface-hover': '#f1f1f3', ink: '#18181b', muted: '#52525b', subtle: '#71717a', border: '#8b8b94', 'border-strong': '#52525b', rule: '#e4e4e7', accent: '#4f46e5', 'accent-hover': '#4338ca', 'accent-soft': '#eef0ff', 'accent-text': '#4338ca', 'on-accent': '#ffffff', focus: '#4f46e5', 'focus-ring': '#c7c9fb' },
    radius: { sm: 4, md: 6, lg: 10, xl: 14 },
    size: { control: 36, 'control-sm': 30, 'control-lg': 44 },
    tracking: { tight: -0.02, normal: -0.006, wide: 0.02 },
    shadow: { sm: [[0, 1, 2, 0, 0.06]], md: [[0, 1, 2, 0, 0.05], [0, 4, 12, -2, 0.08]], lg: [[0, 2, 4, 0, 0.06], [0, 16, 40, -8, 0.18]] },
  },
  soft: {
    description: 'Soft: warm neutrals, deep teal accent, roomy controls, generous radii, soft elevation. Friendly consumer / service products.',
    fonts: { body: ['Plus Jakarta Sans', 'Segoe UI', 'system-ui', 'sans-serif'], display: ['Plus Jakarta Sans', 'Segoe UI', 'system-ui', 'sans-serif'], mono: ['JetBrains Mono', 'Consolas', 'monospace'] },
    head: '<link rel="preconnect" href="https://fonts.googleapis.com">\n<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>\n<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Plus+Jakarta+Sans:wght@400..700&family=JetBrains+Mono:wght@400;500&display=swap">',
    color: { canvas: '#f6f4f0', surface: '#ffffff', 'surface-sunken': '#f3f0ea', 'surface-hover': '#f0ece5', ink: '#1f1c18', muted: '#5c564d', subtle: '#7a7368', border: '#8f877b', 'border-strong': '#5c564d', rule: '#e8e3da', accent: '#0f6e66', 'accent-hover': '#0b5b54', 'accent-soft': '#e3f2ef', 'accent-text': '#0b5b54', 'on-accent': '#ffffff', focus: '#0f6e66', 'focus-ring': '#b7e0d9' },
    radius: { sm: 6, md: 10, lg: 16, xl: 20 },
    size: { control: 40, 'control-sm': 34, 'control-lg': 48 },
    tracking: { tight: -0.015, normal: 0, wide: 0.02 },
    shadow: { sm: [[0, 1, 2, 0, 0.05]], md: [[0, 2, 6, -1, 0.06], [0, 10, 24, -6, 0.1]], lg: [[0, 4, 10, -2, 0.08], [0, 24, 56, -12, 0.2]] },
  },
  industrial: {
    description: 'Industrial: high-contrast slate, signal blue accent, large targets, square-ish radii, flat surfaces. Operations, counters, factory floors.',
    fonts: { body: ['IBM Plex Sans', 'Segoe UI', 'system-ui', 'sans-serif'], display: ['IBM Plex Sans', 'Segoe UI', 'system-ui', 'sans-serif'], mono: ['IBM Plex Mono', 'Consolas', 'monospace'] },
    head: '<link rel="preconnect" href="https://fonts.googleapis.com">\n<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>\n<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=IBM+Plex+Sans:wght@400;500;600;700&family=IBM+Plex+Mono:wght@400;500&display=swap">',
    color: { canvas: '#eceff1', surface: '#ffffff', 'surface-sunken': '#f2f4f5', 'surface-hover': '#eef1f3', ink: '#111518', muted: '#4a5359', subtle: '#69737a', border: '#7d878e', 'border-strong': '#3a4247', rule: '#d9dee1', accent: '#0b5cad', 'accent-hover': '#094b8e', 'accent-soft': '#e3eefa', 'accent-text': '#094b8e', 'on-accent': '#ffffff', focus: '#0b5cad', 'focus-ring': '#b5d0ef' },
    radius: { sm: 2, md: 4, lg: 6, xl: 8 },
    size: { control: 44, 'control-sm': 36, 'control-lg': 56 },
    tracking: { tight: -0.01, normal: 0, wide: 0.03 },
    shadow: { sm: [[0, 1, 1, 0, 0.08]], md: [[0, 1, 2, 0, 0.08], [0, 6, 16, -4, 0.12]], lg: [[0, 2, 6, 0, 0.1], [0, 20, 48, -10, 0.24]] },
  },
};

const CHECKS = [
  ['ink', 'surface', 4.5], ['ink', 'canvas', 4.5], ['muted', 'surface', 4.5], ['muted', 'canvas', 4.5], ['muted', 'surface-sunken', 4.5],
  ['subtle', 'surface', 4.5], ['border', 'surface', 3], ['accent', 'surface', 4.5], ['on-accent', 'accent', 4.5], ['on-accent', 'accent-hover', 4.5],
  ['accent-text', 'accent-soft', 4.5], ['focus', 'surface', 3],
];
const STATUS_CHECKS = [['ok-text', 'ok-tint'], ['info-text', 'info-tint'], ['warn-text', 'warn-tint'], ['err-text', 'err-tint']];

let failed = 0;
fs.mkdirSync(OUT, { recursive: true });
for (const [name, p] of Object.entries(PRESETS)) {
  const c = p.color;
  for (const [a, b, min] of CHECKS) {
    const r = ratio(c[a], c[b]);
    if (r < min) { console.error(`${name}: ${a} on ${b} = ${r.toFixed(2)} < ${min}`); failed++; }
  }
  for (const [a, b] of STATUS_CHECKS) if (ratio(STATUS[a], STATUS[b]) < 4.5) { console.error(`${name}: status ${a}/${b} ${ratio(STATUS[a], STATUS[b]).toFixed(2)}`); failed++; }
  for (const f of ['ok', 'err']) if (ratio('#ffffff', STATUS[f]) < 4.5) { console.error(`status ${f} fill`); failed++; }
  if (ratio('#111518', STATUS.warn) < 4.5) failed++;

  const tokens = {
    $description: `dfw starter preset "${name}". ${p.description} Generated by scripts/make-presets.mjs; copy into design/tokens/tokens.json and adapt.`,
    color: {
      $type: 'color',
      ...Object.fromEntries(Object.entries(c).map(([k, v]) => [k, col(v)])),
      'on-fill': col('#ffffff', 'Text on ok/err/info fills.'),
      'on-warn': col('#111518', 'Text on the warn fill.'),
      status: { ...Object.fromEntries(Object.entries(STATUS).map(([k, v]) => [k, col(v)])) },
    },
    font: {
      family: { $type: 'fontFamily', body: { $value: p.fonts.body }, display: { $value: p.fonts.display }, mono: { $value: p.fonts.mono } },
      size: { $type: 'dimension', '2xs': px(11), xs: px(12), sm: px(14), md: px(16), lg: px(18), xl: px(20), '2xl': px(24), '3xl': px(30), '4xl': px(36), 'display-sm': px(28), 'display-md': px(36), 'display-lg': px(48), 'display-xl': px(64) },
      weight: { $type: 'fontWeight', regular: { $value: 400 }, medium: { $value: 500 }, semibold: { $value: 600 }, bold: { $value: 700 }, display: { $value: 600 } },
      leading: { $type: 'number', tight: { $value: 1.15 }, snug: { $value: 1.35 }, body: { $value: 1.5 }, display: { $value: 1.08 } },
      tracking: { $type: 'dimension', ...Object.fromEntries(Object.entries(p.tracking).map(([k, v]) => [k, { $value: { value: v, unit: 'em' } }])), display: { $value: { value: p.tracking.tight - 0.005, unit: 'em' } } },
      eyebrow: { size: { $type: 'dimension', $value: { value: 11, unit: 'px' } }, weight: { $type: 'fontWeight', $value: 600 }, tracking: { $type: 'dimension', $value: { value: 0.08, unit: 'em' } }, case: { $type: 'string', $value: 'uppercase', $description: 'text-transform for eyebrows (dfw string token)' } },
    },
    space: { $type: 'dimension', half: px(2), '1': px(4), '1-5': px(6), '2': px(8), '3': px(12), '4': px(16), '5': px(20), '6': px(24), '8': px(32), '10': px(40), '12': px(48), '16': px(64) },
    radius: { $type: 'dimension', ...Object.fromEntries(Object.entries(p.radius).map(([k, v]) => [k, px(v)])), pill: px(999) },
    border: { width: { $type: 'dimension', $value: { value: 1, unit: 'px' } }, 'width-strong': { $type: 'dimension', $value: { value: 2, unit: 'px' } } },
    focus: { width: { $type: 'dimension', $value: { value: 2, unit: 'px' } }, offset: { $type: 'dimension', $value: { value: 2, unit: 'px' } } },
    size: {
      $type: 'dimension',
      control: px(p.size.control), 'control-sm': px(p.size['control-sm']), 'control-lg': px(p.size['control-lg']),
      'target-min': px(24, 'WCAG 2.2 AA minimum target.'),
      'icon-xs': px(14), 'icon-sm': px(16), icon: px(20), 'icon-lg': px(32), 'icon-xl': px(40),
      'avatar-xl': px(88), sidebar: px(240), rail: px(84), 'rail-item': px(60),
      dialog: px(480), 'dialog-sm': px(400), 'dialog-lg': px(640), band: px(88),
    },
    shadow: { $type: 'shadow', ...Object.fromEntries(Object.entries(p.shadow).map(([k, v]) => [k, shadow(v)])) },
    duration: { $type: 'duration', fast: { $value: { value: 120, unit: 'ms' } }, base: { $value: { value: 200, unit: 'ms' } } },
    easing: { $type: 'cubicBezier', out: { $value: [0.2, 0, 0, 1] } },
  };
  fs.writeFileSync(path.join(OUT, `${name}.tokens.json`), JSON.stringify(tokens, null, 2) + '\n');
  fs.writeFileSync(path.join(OUT, `${name}.head.html`), `<!-- Fonts for preset "${name}" (SIL OFL). Use {{base}} for links to files under design/. Self-host for production. -->\n${p.head}\n`);
}
if (failed) {
  console.error(`${failed} contrast check(s) failed.`);
  process.exit(1);
}
console.log(`Wrote ${Object.keys(PRESETS).length} presets to ${path.relative(ROOT, OUT)} (all contrast checks pass).`);
