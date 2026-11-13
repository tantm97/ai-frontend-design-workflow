// Generates the "Design System" page: foundations (from DTCG tokens) and every
// component master with its variants and examples. The page still contains
// <dfw-use>/<dfw-icon>; callers compose it like any other design file.
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, readJson } from './util.mjs';
import { cssVarName, tokensToDeclarations } from './tokens.mjs';
import { loadComponents, listIcons, escapeHtml as esc } from './compose.mjs';
import { auditDesignSystem } from './dsaudit.mjs';

function collect(tree) {
  const out = [];
  const walk = (node, trail, type) => {
    if (typeof node !== 'object' || node === null) return;
    const t = node.$type || type;
    if ('$value' in node) return void out.push({ name: trail.join('.'), type: t, value: node.$value, description: node.$description || '' });
    for (const [k, v] of Object.entries(node)) if (!k.startsWith('$')) walk(v, [...trail, k], t);
  };
  walk(tree, [], undefined);
  return out;
}

function tokenTree(root) {
  const f = path.join(projectPaths(root).design, 'tokens', 'tokens.json');
  return fs.existsSync(f) ? readJson(f, {}) : {};
}

/** cssVar -> resolved CSS value (aliases followed). */
function varMap(root) {
  const m = new Map(tokensToDeclarations(tokenTree(root)));
  const resolve = (v, depth = 0) => (depth > 8 ? v : String(v).replace(/var\((--[\w-]+)\)/g, (all, n) => (m.has(n) ? resolve(m.get(n), depth + 1) : all)));
  return { get: (n) => (m.has(n) ? resolve(m.get(n)) : null), resolve };
}

/** Spec line for a component: key declarations of its root class with token names and resolved values. */
function specLine(def, vars) {
  const cls = (/class="([\w-]+)/.exec(def.template) || [])[1];
  if (!cls) return '';
  const rule = new RegExp(`(^|[}\\s,])\\.${cls}\\s*\\{([^}]*)\\}`).exec(def.styles);
  if (!rule) return '';
  const want = ['min-height', 'height', 'width', 'padding', 'gap', 'border-radius', 'font', 'font-size', 'border'];
  const parts = [];
  for (const d of rule[2].split(';')) {
    const i = d.indexOf(':');
    if (i < 0) continue;
    const prop = d.slice(0, i).trim();
    const val = d.slice(i + 1).trim();
    if (!want.includes(prop)) continue;
    const resolved = vars.resolve(val).replace(/"[^"]*",.*$/, '').replace(/\s+/g, ' ').trim();
    const names = [...val.matchAll(/var\(--([\w-]+)\)/g)].map((x) => x[1]).join(', ');
    parts.push(`<span><b>${esc(prop)}</b> ${esc(resolved.slice(0, 48))}${names ? ` <i>${esc(names)}</i>` : ''}</span>`);
  }
  return parts.length ? `<div class="ds-spec">${parts.join('')}</div>` : '';
}

function loadTokens(root) {
  const dir = path.join(projectPaths(root).design, 'tokens');
  if (!fs.existsSync(dir)) return [];
  return fs
    .readdirSync(dir)
    .filter((f) => f.endsWith('.json'))
    .flatMap((f) => collect(readJson(path.join(dir, f), {})));
}

let HEX_LOOKUP = () => null;
const hexOf = (v) => (typeof v === 'string' ? (v.startsWith('#') ? v : /^\{[^}]+\}$/.test(v) ? HEX_LOOKUP(v.slice(1, -1)) : null) : v?.hex || null);
function lum(hex) {
  const c = [1, 3, 5].map((i) => parseInt(hex.slice(i, i + 2), 16) / 255).map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2];
}
export function contrast(a, b) {
  if (!a || !b || a.length < 7 || b.length < 7) return null;
  const [x, y] = [lum(a), lum(b)];
  return (Math.max(x, y) + 0.05) / (Math.min(x, y) + 0.05);
}
const dim = (v) => (typeof v === 'object' && v && 'value' in v ? `${v.value}${v.unit}` : String(v));

function autoExamples(def) {
  const vs = def.variants.length ? def.variants : [''];
  const ss = def.sizes.length ? def.sizes : [''];
  const label = def.name.replace(/[-_]/g, ' ');
  return vs
    .flatMap((v) => ss.map((s) => `<dfw-use c="${def.name}"${v ? ` variant="${v}"` : ''}${s ? ` size="${s}"` : ''}>${esc(label)}</dfw-use>`))
    .join('\n');
}

/**
 * @param {string} root project root
 * @param {{designBase?: string}} opts URL prefix under which design/ is served (default: relative from design/system/).
 */
export function renderSystemPage(root, { designBase = '../' } = {}) {
  const manifest = readJson(path.join(projectPaths(root).design, 'design.json'), {});
  const tokens = loadTokens(root);
  const lib = loadComponents(root);
  const icons = listIcons(root);
  const headFile = path.join(projectPaths(root).design, 'system', 'head.html');
  const head = fs.existsSync(headFile) ? fs.readFileSync(headFile, 'utf8').replaceAll('{{base}}', designBase) : '';
  const byType = (t) => tokens.filter((x) => x.type === t);
  const colors = byType('color');
  const vars = varMap(root);
  const byName = new Map(tokens.map((t) => [t.name, t]));
  HEX_LOOKUP = (name, depth = 0) => {
    const t = byName.get(name);
    if (!t || depth > 8) return null;
    if (typeof t.value === 'string' && /^\{[^}]+\}$/.test(t.value)) return HEX_LOOKUP(t.value.slice(1, -1), depth + 1);
    return t.value?.hex || (typeof t.value === 'string' && t.value.startsWith('#') ? t.value : null);
  };
  const styleFile = path.join(projectPaths(root).design, 'system', 'style.json');
  const style = fs.existsSync(styleFile) ? readJson(styleFile, null) : null;
  const assetsDir = path.join(projectPaths(root).design, 'assets');
  const assets = fs.existsSync(assetsDir) ? fs.readdirSync(assetsDir).filter((f) => !f.startsWith('.')).sort() : [];
  const surface = hexOf(colors.find((c) => /(^|\.)surface$/.test(c.name))?.value) || '#ffffff';
  const ink = hexOf(colors.find((c) => /(^|\.)ink$/.test(c.name))?.value) || '#000000';

  const colorGroups = new Map();
  for (const c of colors) {
    const g = c.name.split('.').slice(0, -1).join('.') || 'color';
    if (!colorGroups.has(g)) colorGroups.set(g, []);
    colorGroups.get(g).push(c);
  }
  const swatch = (c) => {
    const hex = hexOf(c.value);
    const cs = contrast(hex, surface);
    const ci = contrast(hex, ink);
    const alias = typeof c.value === 'string' && c.value.startsWith('{') ? c.value.slice(1, -1).replace(/^color\./, '') : '';
    return `<figure class="ds-swatch" data-dfw-id="system.token.${esc(c.name)}">
  <i style="background:var(${cssVarName(c.name)})"></i>
  <figcaption><b>${esc(c.name.replace(/^color\./, '').replace(/^palette\./, ''))}</b><code>${esc(hex || '')}${alias ? ` = ${esc(alias)}` : ''}</code>${c.description ? `<span class="ds-usage">${esc(c.description)}</span>` : ''}${cs ? `<small>${cs.toFixed(1)}:1 on surface · ${ci.toFixed(1)}:1 on ink</small>` : ''}</figcaption>
</figure>`;
  };

  const families = byType('fontFamily');
  const sizes = tokens.filter((t) => t.type === 'dimension' && /^font\.size\./.test(t.name));
  const spaces = tokens.filter((t) => t.type === 'dimension' && /^space\./.test(t.name));
  const radii = tokens.filter((t) => t.type === 'dimension' && /^radius\./.test(t.name));
  const shadows = byType('shadow');
  const sample = manifest.sampleText || 'Quick brown fox 0123456789';

  const groups = new Map();
  for (const d of lib.values()) {
    if (!groups.has(d.group)) groups.set(d.group, []);
    groups.get(d.group).push(d);
  }
  const compSection = (d) => `<section class="ds-comp" data-dfw-id="system.component.${esc(d.name)}" id="c-${esc(d.name)}">
  <header><h3>${esc(d.name)}</h3><p>${esc(d.description)}</p>
  <dl>${d.variants.length ? `<dt>Variants</dt><dd>${d.variants.map(esc).join(' · ')}</dd>` : ''}${d.sizes.length ? `<dt>Sizes</dt><dd>${d.sizes.map(esc).join(' · ')}</dd>` : ''}${d.props.length ? `<dt>Props</dt><dd>${d.props.map(esc).join(' · ')}</dd>` : ''}<dt>Master</dt><dd><code>${esc(d.file)}</code></dd></dl></header>
  <div class="ds-right">${specLine(d, vars)}<div class="ds-examples">${d.examples || autoExamples(d)}</div></div>
</section>`;

  return `<!doctype html>
<html lang="${esc(manifest.lang || 'en')}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Design System - ${esc(manifest.product || 'Product')}</title>
<meta name="description" content="Foundations and components of the ${esc(manifest.product || '')} design system.">
${head}
<link rel="stylesheet" href="${designBase}tokens/tokens.css">
<link rel="stylesheet" href="${designBase}system/components.css">
<style>
  /* Page chrome for the generated system page (tooling, not product UI). */
  .ds { font: 14px/1.5 system-ui, sans-serif; color: #1d1d1f; background: #fff; margin: 0; padding: 48px 56px 96px; display: grid; gap: 56px; }
  .ds > header h1 { font-size: 32px; margin: 0; letter-spacing: -0.02em; }
  .ds > section > h2 { font-size: 20px; margin: 0 0 16px; padding-bottom: 8px; border-bottom: 1px solid #e5e5e5; }
  .ds-comp > header h3 { font-size: 16px; margin: 0; }
  .ds code { font: 12px ui-monospace, monospace; color: #6e6e73; overflow-wrap: anywhere; }
  .ds-grid { display: grid; grid-template-columns: repeat(auto-fill, minmax(180px, 1fr)); gap: 12px; }
  .ds-group { margin: 0 0 20px; } .ds-group > h4 { margin: 0 0 8px; font-size: 12px; text-transform: uppercase; letter-spacing: .06em; color: #6e6e73; }
  .ds-swatch { margin: 0; border: 1px solid #e5e5e5; border-radius: 8px; overflow: hidden; }
  .ds-swatch i { display: block; height: 56px; border-bottom: 1px solid #e5e5e5; }
  .ds-swatch figcaption { padding: 8px 10px; display: grid; gap: 2px; font-size: 12px; } .ds-swatch small { color: #6e6e73; }
  .ds-type { display: grid; gap: 12px; } .ds-type div { display: grid; grid-template-columns: 160px 1fr; align-items: baseline; gap: 16px; }
  .ds-scale { display: grid; gap: 8px; } .ds-scale div { display: grid; grid-template-columns: 120px 1fr; align-items: center; gap: 16px; }
  .ds-bar { height: 12px; background: #c7d2fe; border-radius: 2px; }
  .ds-radius { display: flex; flex-wrap: wrap; gap: 16px; } .ds-radius div { width: 96px; display: grid; gap: 6px; font-size: 12px; } .ds-radius i { display: block; height: 64px; border: 2px solid #1d1d1f; }
  .ds-icons { display: grid; grid-template-columns: repeat(auto-fill, minmax(96px, 1fr)); gap: 8px; } .ds-icons div { display: grid; justify-items: center; gap: 6px; padding: 12px 4px; border: 1px solid #eee; border-radius: 8px; font-size: 11px; color: #6e6e73; }
  .ds-comp { display: grid; grid-template-columns: 280px 1fr; gap: 32px; padding: 24px 0; border-top: 1px solid #eee; }
  .ds-comp > header p { color: #6e6e73; margin: 4px 0 12px; } .ds-comp > header dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 0; font-size: 12px; } .ds-comp > header dt { color: #6e6e73; } .ds-comp > header dd { margin: 0; }
  .ds-examples { display: flex; flex-wrap: wrap; gap: 16px; align-items: flex-start; align-content: flex-start; padding: 24px; border-radius: 8px; background: var(--color-canvas, #f5f5f7); }
  .ds-audit { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; } .ds-audit li { padding: 8px 12px; border-radius: 6px; background: #f5f5f7; } .ds-audit li b { font-size: 11px; margin-right: 6px; } .ds-audit .lv-error { background: #fdebe9; } .ds-audit .lv-warn { background: #fdf3d7; }
  .ds-usage { color: #3a3a3c; line-height: 1.35; }
  .ds-right { display: grid; gap: 12px; align-content: start; min-width: 0; } .ds-spec { display: flex; flex-wrap: wrap; gap: 4px 16px; font-size: 11px; color: #6e6e73; }
  .ds-spec b { color: #1d1d1f; font-weight: 600; } .ds-spec i { font-style: normal; color: #8e8e93; }
  .ds-style { display: grid; grid-template-columns: 1.2fr 1fr; gap: 32px; }
  .ds-style h3 { font-size: 13px; text-transform: uppercase; letter-spacing: .06em; color: #6e6e73; margin: 0 0 6px; }
  .ds-style p, .ds-style li { margin: 0; line-height: 1.5; } .ds-style ul { margin: 0; padding-left: 18px; display: grid; gap: 4px; }
  .ds-style .concept { font-size: 20px; line-height: 1.4; color: #1d1d1f; }
  .ds-cols { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 24px; }
  .ds-assets { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 12px; } .ds-assets figure { margin: 0; border: 1px solid #eee; border-radius: 8px; overflow: hidden; font-size: 11px; } .ds-assets img { width: 100%; height: 120px; object-fit: contain; background: #f5f5f7; display: block; } .ds-assets figcaption { padding: 6px 8px; color: #6e6e73; overflow-wrap: anywhere; }
  .ds-empty { padding: 24px; border: 1px dashed #c7c7cc; border-radius: 8px; color: #6e6e73; }
</style>
</head>
<body class="ds" data-dfw-id="system">
  <header data-dfw-id="system.header"><h1>Design System</h1><p>${esc(manifest.product || '')} · tokens: design/tokens/*.json · components: design/system/components/*.html</p></header>

  ${style ? `<section data-dfw-id="system.style"><h2>Style · ${esc(style.name)}</h2>
  <div class="ds-style"><div><p class="concept">${esc(style.concept)}</p>
  <div class="ds-cols" style="margin-top:20px"><div><h3>Do</h3><ul>${(style.do || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div><div><h3>Don't</h3><ul>${(style.dont || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div></div></div>
  <div style="display:grid;gap:16px"><div><h3>Layout and grid</h3><p>${esc(style.layout || '')}</p></div>
  <div><h3>Signature motifs</h3><ul>${(style.motifs || []).map((m) => `<li><b>${esc(m.name)}</b>: ${esc(m.use)}</li>`).join('')}</ul></div>
  <div><h3>Component rules</h3><ul>${(style.components || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div>
  ${style.rationale?.length ? `<div><h3>Why (rationale)</h3><ul>${style.rationale.map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div>` : ''}
  ${style.axes ? `<div><h3>Personality</h3><p>${Object.entries(style.axes).map(([k, v]) => `${esc(k)} ${'●'.repeat(v)}${'○'.repeat(5 - v)}`).join(' · ')}</p></div>` : ''}
  <div><h3>Fonts</h3><p>${esc(style.fonts.display)} (display) · ${esc(style.fonts.body)} (body) · ${esc(style.fonts.mono)} (mono)${style.fontsVariant === 'latin' ? ' · English-only set' : ''}</p></div></div></div>
  </section>` : ''}

  ${(() => {
    const f = auditDesignSystem(root);
    const label = { error: 'MUST', warn: 'SHOULD', info: 'NOTE' };
    return `<section data-dfw-id="system.audit"><h2>Senior review (${f.length ? `${f.filter((x) => x.level === 'error').length} must · ${f.filter((x) => x.level === 'warn').length} should` : 'clean'})</h2>${f.length ? `<ul class="ds-audit">${f.map((x) => `<li class="lv-${x.level}"><b>${label[x.level]}</b> ${esc(x.message)}</li>`).join('')}</ul>` : '<p>No findings: contrast, semantic colours, neutrals, type rhythm and rationale all pass.</p>'}</section>`;
  })()}

  <section data-dfw-id="system.colors"><h2>Color</h2>
  ${[...colorGroups].map(([g, list]) => `<div class="ds-group"><h4>${esc(g)}</h4><div class="ds-grid">${list.map(swatch).join('')}</div></div>`).join('') || '<p class="ds-empty">No color tokens yet.</p>'}
  </section>

  <section data-dfw-id="system.typography"><h2>Typography</h2>
  <div class="ds-type">${families.map((f) => `<div data-dfw-id="system.token.${esc(f.name)}"><code>${esc(f.name)}</code><span style="font-family:var(${cssVarName(f.name)});font-size:24px">${esc(sample)}</span></div>`).join('')}
  ${sizes.map((s) => `<div data-dfw-id="system.token.${esc(s.name)}"><code>${esc(s.name.replace(/^font\.size\./, ''))} · ${esc(dim(s.value))}</code><span style="font-family:var(--font-family-body, inherit);font-size:var(${cssVarName(s.name)})">${esc(sample)}</span></div>`).join('')}</div>
  </section>

  <section data-dfw-id="system.spacing"><h2>Spacing</h2>
  <div class="ds-scale">${spaces.map((s) => `<div data-dfw-id="system.token.${esc(s.name)}"><code>${esc(s.name)} · ${esc(dim(s.value))}</code><span class="ds-bar" style="width:var(${cssVarName(s.name)})"></span></div>`).join('') || '<p class="ds-empty">No spacing tokens yet.</p>'}</div>
  </section>

  <section data-dfw-id="system.shape"><h2>Radius and elevation</h2>
  <div class="ds-radius">${radii.map((r) => `<div><i style="border-radius:var(${cssVarName(r.name)})"></i><code>${esc(r.name)} · ${esc(dim(r.value))}</code></div>`).join('')}${shadows.map((s) => `<div><i style="border:0;background:#fff;box-shadow:var(${cssVarName(s.name)})"></i><code>${esc(s.name)}</code></div>`).join('')}</div>
  </section>

  <section data-dfw-id="system.icons"><h2>Icons (${icons.length})</h2>
  ${icons.length ? `<div class="ds-icons">${icons.map((n) => `<div><dfw-icon name="${esc(n)}"></dfw-icon>${esc(n)}</div>`).join('')}</div>` : '<p class="ds-empty">No icons yet. Add them with: dfw icons add &lt;name&gt; …</p>'}
  </section>

  <section data-dfw-id="system.assets"><h2>Assets (${assets.length})</h2>
  ${assets.length ? `<div class="ds-assets">${assets.map((f) => `<figure>${/\.(png|jpe?g|svg|webp|gif)$/i.test(f) ? `<img src="${designBase}assets/${encodeURI(f)}" alt="">` : ''}<figcaption>${esc(f)}</figcaption></figure>`).join('')}</div><p style="margin-top:8px;color:#6e6e73">Brand assets are used as supplied: never redrawn, recoloured or cropped into new marks.</p>` : '<p class="ds-empty">No assets in design/assets/ yet (logos, ornaments, photography). Placeholders must be labelled as placeholders.</p>'}
  </section>

  <section data-dfw-id="system.components"><h2>Components (${lib.size})</h2>
  ${[...groups].map(([g, list]) => `<div class="ds-group"><h4>${esc(g)}</h4>${list.map(compSection).join('')}</div>`).join('') || '<p class="ds-empty">No components yet. Create design/system/components/&lt;name&gt;.html (see the design-format reference).</p>'}
  </section>
</body>
</html>
`;
}
