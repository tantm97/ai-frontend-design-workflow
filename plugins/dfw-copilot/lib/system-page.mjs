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

const ROLE_GROUPS = [
  ['Surfaces', ['canvas', 'surface', 'surface-sunken', 'surface-hover']],
  ['Text', ['ink', 'muted', 'subtle', 'accent-text']],
  ['Lines and focus', ['rule', 'border', 'border-strong', 'focus', 'focus-ring']],
  ['Action', ['accent', 'accent-hover', 'accent-soft', 'on-accent']],
  ['Brand and signature', ['brand', 'on-brand', 'signature']],
];
// What each role is checked against (and the WCAG target).
const ROLE_CHECK = {
  ink: ['surface', 7], muted: ['surface', 4.5], subtle: ['surface', 4.5], 'accent-text': ['accent-soft', 4.5], border: ['surface', 3], 'border-strong': ['surface', 3],
  focus: ['surface', 3], accent: ['surface', 4.5], 'on-accent': ['accent', 4.5], 'on-brand': ['brand', 4.5],
};
const TEXT_STYLES = [
  ['display-xl', 'Display XL', 'Hero numbers, one per screen'], ['display-lg', 'Display L', 'Page titles on marketing/overview'], ['display-md', 'Display M', 'Section titles, key figures'],
  ['display-sm', 'Display S', 'Screen titles, names to confirm'], ['title', 'Title', 'Card and dialog titles'], ['subtitle', 'Subtitle', 'Emphasised lines'],
  ['body', 'Body', 'Default reading text'], ['body-sm', 'Body S', 'Dense UI, tables'], ['caption', 'Caption', 'Labels, meta, timestamps'], ['eyebrow', 'Eyebrow', 'Section openers'], ['mono', 'Mono', 'IDs, codes, numbers to compare'],
];

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
  const vars = varMap(root);
  const byName = new Map(tokens.map((t) => [t.name, t]));
  HEX_LOOKUP = (name, depth = 0) => {
    const t = byName.get(name);
    if (!t || depth > 8) return null;
    if (typeof t.value === 'string' && /^\{[^}]+\}$/.test(t.value)) return HEX_LOOKUP(t.value.slice(1, -1), depth + 1);
    return t.value?.hex || (typeof t.value === 'string' && t.value.startsWith('#') ? t.value : null);
  };
  const role = (r) => HEX_LOOKUP(`color.${r}`);
  const aliasOf = (name) => {
    const t = byName.get(name);
    return t && typeof t.value === 'string' && t.value.startsWith('{') ? t.value.slice(1, -1).replace(/^color\.palette\./, '') : '';
  };
  const styleFile = path.join(projectPaths(root).design, 'system', 'style.json');
  const style = fs.existsSync(styleFile) ? readJson(styleFile, null) : null;
  const assetsDir = path.join(projectPaths(root).design, 'assets');
  const assets = fs.existsSync(assetsDir) ? fs.readdirSync(assetsDir).filter((f) => !f.startsWith('.')).sort() : [];

  // ---- palette: one compact strip per ramp ----
  const palette = tokens.filter((t) => t.type === 'color' && t.name.startsWith('color.palette.'));
  const ramps = new Map();
  for (const t of palette) {
    const key = t.name.replace('color.palette.', '');
    const m = /^(.*)-(\d+)$/.exec(key);
    const rampName = m ? m[1] : key;
    if (!ramps.has(rampName)) ramps.set(rampName, []);
    ramps.get(rampName).push({ step: m ? m[2] : '', key, token: t });
  }
  const usedSteps = new Map();
  for (const [, roles] of ROLE_GROUPS) for (const r of roles) { const a = aliasOf(`color.${r}`); if (a) (usedSteps.get(a) || usedSteps.set(a, []).get(a)).push(r); }
  const rampIntent = (list) => (list.find((x) => x.step === '500')?.token.description || '').replace(/\s*Used for:.*$/, '').replace(/\s*Reserve step.*$/, '');
  const paletteHtml = [...ramps].map(([name, list]) => `<div class="ds-ramp" data-dfw-id="system.palette.${esc(name)}">
  <div class="ds-ramp__head"><b>${esc(name)}</b><span>${esc(rampIntent(list))}</span></div>
  <div class="ds-ramp__chips">${list.map((x) => {
    const hex = hexOf(x.token.value) || '';
    const used = usedSteps.get(x.key);
    const dark = contrast(hex, '#ffffff') > contrast(hex, '#000000');
    return `<div class="ds-chip${used ? ' is-used' : ''}" style="background:var(${cssVarName(x.token.name)});color:${dark ? '#fff' : '#000'}" title="${esc(`${x.key} ${hex}${used ? ` · ${used.join(', ')}` : ''}`)}"><span>${esc(x.step)}</span>${used ? '<i></i>' : ''}</div>`;
  }).join('')}</div>
</div>`).join('');

  // ---- roles: grouped table with the palette step and the contrast check ----
  const roleRow = (r) => {
    const hex = role(r);
    if (!hex) return '';
    const t = byName.get(`color.${r}`);
    const chk = ROLE_CHECK[r];
    let check = '';
    if (chk && role(chk[0])) {
      const c = contrast(hex, role(chk[0]));
      check = `<span class="ds-pass ${c >= chk[1] ? 'ok' : 'bad'}">${c.toFixed(1)}:1 ${c >= chk[1] ? '✓' : '✕'} <small>on ${esc(chk[0])}</small></span>`;
    }
    return `<tr data-dfw-id="system.token.color.${esc(r)}"><td><i class="ds-dot" style="background:var(${cssVarName(`color.${r}`)})"></i></td><td><b>${esc(r)}</b></td><td><code>${esc(aliasOf(`color.${r}`) || hex)}</code></td><td>${esc((t?.description || '').replace(/\s*Used for:.*$/, ''))}</td><td>${check}</td></tr>`;
  };
  const rolesHtml = `<table class="ds-table">${ROLE_GROUPS.map(([g, rs]) => `<tbody><tr class="ds-table__group"><th colspan="5">${esc(g)}</th></tr>${rs.map(roleRow).join('')}</tbody>`).join('')}</table>`;

  // ---- status: four previews, not twelve swatches ----
  const statusNames = { ok: 'Success', info: 'Info', warn: 'Warning', err: 'Error' };
  const statusHtml = `<div class="ds-status">${Object.entries(statusNames).map(([k, label]) => byName.has(`color.status.${k}`) ? `<div class="ds-status__item" data-dfw-id="system.status.${k}">
  <div class="ds-status__fill" style="background:var(--color-status-${k});color:var(${k === 'warn' ? '--color-on-warn' : '--color-on-fill'})">${esc(label)} fill</div>
  <div class="ds-status__tint" style="background:var(--color-status-${k}-tint);color:var(--color-status-${k}-text)">${esc(label)} message text</div>
</div>` : '').join('')}</div>`;

  // ---- proportion 60/30/10 ----
  const proportion = role('canvas') && role('accent') ? `<div class="ds-prop" aria-label="Colour proportion"><span style="flex:6;background:var(--color-canvas)">60 · canvas + surfaces</span><span style="flex:3;background:var(--color-ink);color:var(--color-canvas)">30 · ink + text</span><span style="flex:1;background:var(--color-accent);color:var(--color-on-accent)">10 · accent</span>${role('signature') ? '<span style="flex:.4;background:var(--color-signature)" title="signature"></span>' : ''}</div>` : '';

  // ---- typography: families + named text styles in their real font ----
  const families = byType('fontFamily');
  const fam = (n) => byName.get(`font.family.${n}`);
  const famName = (n) => (Array.isArray(fam(n)?.value) ? fam(n).value[0] : fam(n)?.value || '');
  const familyHtml = families.length ? `<div class="ds-fams">${['display', 'body', 'mono'].filter((n) => fam(n)).map((n) => `<div class="ds-fam"><span class="ds-fam__aa" style="font-family:var(--font-family-${n})">Aa</span><b>${esc(famName(n))}</b><small>${esc({ display: 'Display: titles and key numbers', body: 'UI and reading text', mono: 'Identifiers and codes' }[n])}</small></div>`).join('')}</div>` : '';
  const sizeOf = (k) => byName.get(`font.size.${k}`)?.value?.value;
  const scaleNote = byName.get('font.size.md')?.description || '';
  const hasText = lib.has('text');
  const sample = manifest.sampleText || 'The quick brown fox · 0123456789';
  const textHtml = hasText ? `<table class="ds-table ds-types">${TEXT_STYLES.map(([v, label, use]) => {
    const size = v.startsWith('display') ? sizeOf(v) : { title: sizeOf('xl'), subtitle: sizeOf('lg'), body: sizeOf('md'), 'body-sm': sizeOf('sm'), caption: sizeOf('xs'), eyebrow: byName.get('font.eyebrow.size')?.value?.value, mono: sizeOf('sm') }[v];
    return `<tr><td class="ds-types__meta"><b>${esc(label)}</b><small>${size ? `${size}px · ` : ''}${esc(use)}</small></td><td><dfw-use c="text" variant="${v}" as="div">${esc(v.startsWith('display') ? sample.split('·')[0].trim() : v === 'mono' ? 'TP-TOOL-00421 · 20417' : sample)}</dfw-use></td></tr>`;
  }).join('')}</table>` : '';

  const spaces = tokens.filter((t) => t.type === 'dimension' && /^space\./.test(t.name)).sort((a, b) => (a.value?.value || 0) - (b.value?.value || 0));
  const radii = tokens.filter((t) => t.type === 'dimension' && /^radius\./.test(t.name));
  const shadows = byType('shadow');

  const groups = new Map();
  for (const d of lib.values()) {
    if (!groups.has(d.group)) groups.set(d.group, []);
    groups.get(d.group).push(d);
  }
  const compSection = (d) => `<section class="ds-comp" data-dfw-id="system.component.${esc(d.name)}" id="c-${esc(d.name)}">
  <header><h3>${esc(d.name)}</h3><p>${esc(d.description)}</p>
  <dl>${d.variants.length ? `<dt>Variants</dt><dd>${d.variants.map(esc).join(' · ')}</dd>` : ''}${d.sizes.length ? `<dt>Sizes</dt><dd>${d.sizes.map(esc).join(' · ')}</dd>` : ''}${d.props.length ? `<dt>Props</dt><dd>${d.props.map(esc).join(' · ')}</dd>` : ''}<dt>Master</dt><dd><code>${esc(d.file.replace('design/system/components/', ''))}</code></dd></dl></header>
  <div class="ds-right">${specLine(d, vars)}${d.preview === 'frame' ? `<div class="ds-frames">${(d.examples || autoExamples(d)).split(/\n(?=<)/).filter((x) => x.trim()).map((ex) => `<div class="ds-frame"><div class="ds-frame__inner">${ex}</div></div>`).join('')}</div>` : `<div class="ds-examples">${d.examples || autoExamples(d)}</div>`}</div>
</section>`;

  // The project's own reasoning (DESIGN.md) wins over the generic style-guide text.
  const designMd = path.join(root, 'DESIGN.md');
  const md = fs.existsSync(designMd) ? fs.readFileSync(designMd, 'utf8') : '';
  const section = (h) => {
    const part = md.split(/^(?=## )/m).find((p) => new RegExp(`^## ${h}\\s*$`, 'm').test(p.split('\n')[0]));
    return part ? part.split('\n').slice(1).join('\n').replace(/<!--[\s\S]*?-->/g, '').trim() : '';
  };
  const mdInline = (t) => esc(t).replace(/\*\*([^*]+)\*\*/g, '<b>$1</b>').replace(/`([^`]+)`/g, '<code>$1</code>');
  const direction = section('Direction').split(/\n\s*\n/)[0] || '';
  const projectWhy = (section('Rationale').match(/^\s*[-*] .+$/gm) || []).map((l) => l.replace(/^\s*[-*] /, ''));
  const audit = auditDesignSystem(root);
  const label = { error: 'MUST', warn: 'SHOULD', info: 'NOTE' };

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
  /* The documentation page uses the system's own fonts and colours, so nothing on it is foreign to the system. */
  .ds { margin: 0; padding: 56px 64px 120px; display: grid; gap: 72px; background: var(--color-surface, #fff); color: var(--color-ink, #111); font: var(--font-weight-regular, 400) var(--font-size-sm, 14px) / 1.5 var(--font-family-body, system-ui, sans-serif); }
  .ds code, .ds .ds-mono { font-family: var(--font-family-mono, ui-monospace, monospace); font-size: var(--font-size-xs, 12px); color: var(--color-muted, #555); overflow-wrap: anywhere; }
  .ds > header { display: grid; gap: 8px; }
  .ds > header h1 { margin: 0; font: var(--font-weight-display, 600) var(--font-size-display-md, 40px) / 1.05 var(--font-family-display, inherit); letter-spacing: var(--font-tracking-display, -0.02em); }
  .ds > header p { margin: 0; color: var(--color-muted); }
  .ds > section > h2 { margin: 0 0 24px; padding-bottom: 12px; border-bottom: 1px solid var(--color-rule); font: var(--font-weight-semibold, 600) var(--font-size-xl, 20px) / 1.2 var(--font-family-body); }
  .ds h3 { margin: 0; font: var(--font-weight-semibold, 600) var(--font-size-md, 16px) / 1.3 var(--font-family-body); }
  .ds h4 { margin: 32px 0 12px; font: var(--font-weight-semibold, 600) var(--font-size-xs, 12px) / 1.3 var(--font-family-body); text-transform: uppercase; letter-spacing: .08em; color: var(--color-muted); }
  .ds small { color: var(--color-muted); }
  .ds-cols { display: grid; grid-template-columns: repeat(2, minmax(0, 1fr)); gap: 24px 48px; }
  .ds-style { display: grid; grid-template-columns: 1.2fr 1fr; gap: 48px; }
  .ds-style .concept { margin: 0; font: var(--font-weight-regular) var(--font-size-xl, 20px) / 1.45 var(--font-family-body); }
  .ds-style ul { margin: 0; padding-left: 18px; display: grid; gap: 6px; }
  .ds-style h4 { margin-top: 0; }
  .ds-style > div { display: grid; gap: 24px; align-content: start; }
  /* palette */
  .ds-ramp { display: grid; grid-template-columns: 240px minmax(0, 1fr); gap: 24px; align-items: center; padding: 10px 0; }
  .ds-ramp__head { display: grid; gap: 2px; } .ds-ramp__head span { color: var(--color-muted); font-size: var(--font-size-xs, 12px); line-height: 1.4; }
  .ds-ramp__chips { display: grid; grid-template-columns: repeat(11, minmax(0, 1fr)); border-radius: 6px; overflow: hidden; }
  .ds-chip { position: relative; height: 48px; display: flex; align-items: flex-end; padding: 6px; font: 500 10px/1 var(--font-family-mono, monospace); }
  .ds-chip span { opacity: .75; } .ds-chip.is-used span { opacity: 1; font-weight: 700; }
  .ds-chip i { position: absolute; top: 6px; right: 6px; width: 6px; height: 6px; border-radius: 50%; background: currentColor; }
  .ds-legend { margin-top: 8px; color: var(--color-muted); font-size: var(--font-size-xs, 12px); }
  .ds-prop { display: flex; height: 40px; border-radius: 6px; overflow: hidden; border: 1px solid var(--color-rule); margin-bottom: 8px; }
  .ds-prop span { display: flex; align-items: center; padding: 0 12px; font-size: var(--font-size-xs, 12px); white-space: nowrap; overflow: hidden; }
  /* tables */
  .ds-table { width: 100%; border-collapse: collapse; }
  .ds-table td, .ds-table th { padding: 10px 12px 10px 0; border-bottom: 1px solid var(--color-rule); text-align: left; vertical-align: middle; }
  .ds-table__group th { padding-top: 24px; font-size: var(--font-size-xs, 12px); font-weight: 600; text-transform: uppercase; letter-spacing: .08em; color: var(--color-muted); }
  .ds-table td:nth-child(4) { color: var(--color-muted); }
  .ds-dot { display: block; width: 28px; height: 28px; border-radius: 6px; box-shadow: inset 0 0 0 1px rgb(0 0 0 / .08); }
  .ds-pass { white-space: nowrap; font-size: var(--font-size-xs, 12px); font-weight: 600; } .ds-pass.ok { color: var(--color-status-ok-text, #166534); } .ds-pass.bad { color: var(--color-status-err-text, #a32116); }
  .ds-status { display: grid; grid-template-columns: repeat(4, minmax(0, 1fr)); gap: 16px; }
  .ds-status__item { display: grid; border-radius: 6px; overflow: hidden; border: 1px solid var(--color-rule); }
  .ds-status__fill, .ds-status__tint { padding: 12px 14px; font-weight: 600; font-size: var(--font-size-sm, 14px); }
  /* type */
  .ds-fams { display: grid; grid-template-columns: repeat(3, minmax(0, 1fr)); gap: 16px; margin-bottom: 24px; }
  .ds-fam { display: grid; gap: 4px; padding: 20px; border: 1px solid var(--color-rule); border-radius: 8px; }
  .ds-fam__aa { font-size: 56px; line-height: 1; margin-bottom: 8px; }
  .ds-types td { padding: 16px 16px 16px 0; }
  .ds-types__meta { width: 220px; } .ds-types__meta b, .ds-types__meta small { display: block; }
  /* space / shape */
  .ds-space { display: grid; gap: 6px; }
  .ds-space div { display: grid; grid-template-columns: 140px 1fr; align-items: center; gap: 16px; }
  .ds-space i { display: block; height: 10px; background: var(--color-accent-soft); box-shadow: inset 0 0 0 1px var(--color-accent); border-radius: 2px; }
  .ds-shapes { display: flex; flex-wrap: wrap; gap: 24px; padding: 32px; background: var(--color-canvas); border-radius: 8px; }
  .ds-shapes div { display: grid; gap: 8px; justify-items: start; }
  .ds-shapes i { display: block; width: 104px; height: 72px; background: var(--color-surface); border: 1px solid var(--color-border); }
  .ds-shapes i.sh { border: 0; }
  /* icons, assets, components */
  .ds-icons { display: grid; grid-template-columns: repeat(auto-fill, minmax(96px, 1fr)); gap: 4px; }
  .ds-icons div { display: grid; justify-items: center; gap: 8px; padding: 14px 4px; font-size: 11px; color: var(--color-muted); }
  .ds-assets { display: grid; grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); gap: 12px; } .ds-assets figure { margin: 0; border: 1px solid var(--color-rule); border-radius: 8px; overflow: hidden; font-size: 11px; } .ds-assets img { width: 100%; height: 120px; object-fit: contain; background: var(--color-canvas); display: block; } .ds-assets figcaption { padding: 6px 8px; color: var(--color-muted); }
  .ds-comp { display: grid; grid-template-columns: 280px minmax(0, 1fr); gap: 32px; padding: 32px 0; border-top: 1px solid var(--color-rule); }
  .ds-comp > header p { color: var(--color-muted); margin: 4px 0 12px; }
  .ds-comp > header dl { display: grid; grid-template-columns: auto 1fr; gap: 4px 12px; margin: 0; font-size: var(--font-size-xs, 12px); } .ds-comp > header dt { color: var(--color-muted); } .ds-comp > header dd { margin: 0; }
  .ds-right { display: grid; gap: 12px; align-content: start; min-width: 0; }
  .ds-spec { display: flex; flex-wrap: wrap; gap: 4px 16px; font-size: 11px; color: var(--color-muted); } .ds-spec b { color: var(--color-ink); font-weight: 600; } .ds-spec i { font-style: normal; opacity: .75; }
  .ds-examples { display: flex; flex-wrap: wrap; gap: 16px; align-items: flex-start; align-content: flex-start; padding: 24px; border-radius: 8px; background: var(--color-canvas); }
  /* Layout components (app shells) preview as scaled 1366×768 screens. */
  .ds-frames { display: flex; flex-wrap: wrap; gap: 24px; }
  .ds-frame { width: 615px; height: 346px; overflow: hidden; border: 1px solid var(--color-rule); border-radius: 8px; box-shadow: var(--shadow-sm); }
  .ds-frame__inner { width: 1366px; height: 768px; transform: scale(0.45); transform-origin: 0 0; }
  .ds-frame__inner .shell { min-height: 768px; height: 768px; }
  .ds-audit { list-style: none; margin: 0; padding: 0; display: grid; gap: 6px; } .ds-audit li { padding: 10px 14px; border-radius: 6px; background: var(--color-surface-sunken); } .ds-audit li b { font-size: 11px; margin-right: 8px; letter-spacing: .04em; }
  .ds-audit .lv-error { background: var(--color-status-err-tint); color: var(--color-status-err-text); } .ds-audit .lv-warn { background: var(--color-status-warn-tint); color: var(--color-status-warn-text); }
  .ds-empty { padding: 24px; border: 1px dashed var(--color-border); border-radius: 8px; color: var(--color-muted); }
  .ds-group { margin: 0 0 8px; }
</style>
</head>
<body class="ds" data-dfw-id="system">
  <header data-dfw-id="system.header"><h1>Design System</h1><p>${esc(manifest.product || '')}${style ? ` · ${esc(style.name)}` : ''}</p></header>

  ${style ? `<section data-dfw-id="system.style"><h2>Concept</h2>
  <div class="ds-style"><div><p class="concept">${direction && !/^TODO/.test(direction) ? mdInline(direction) : esc(style.concept)}</p>
  ${(projectWhy.length ? projectWhy : style.rationale || []).length ? `<div><h4>Why${projectWhy.length ? ' (DESIGN.md)' : ' (style guide)'}</h4><ul>${(projectWhy.length ? projectWhy : style.rationale).map((x) => `<li>${mdInline(x)}</li>`).join('')}</ul></div>` : ''}
  <div class="ds-cols"><div><h4>Do</h4><ul>${(style.do || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div><div><h4>Don't</h4><ul>${(style.dont || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div></div></div>
  <div><div><h4>Layout and grid</h4><p>${esc(style.layout || '')}</p></div>
  <div><h4>Signature motifs</h4><ul>${(style.motifs || []).map((m) => `<li><b>${esc(m.name)}</b> · ${esc(m.use)}</li>`).join('')}</ul></div>
  <div><h4>Component rules</h4><ul>${(style.components || []).map((x) => `<li>${esc(x)}</li>`).join('')}</ul></div></div></div>
  </section>` : ''}

  <section data-dfw-id="system.audit"><h2>Senior review · ${audit.length ? `${audit.filter((x) => x.level === 'error').length} must, ${audit.filter((x) => x.level === 'warn').length} should` : 'clean'}</h2>
  ${audit.length ? `<ul class="ds-audit">${audit.map((x) => `<li class="lv-${x.level}"><b>${label[x.level]}</b>${esc(x.message)}</li>`).join('')}</ul>` : '<p>Contrast, semantic colours, neutral temperature, type rhythm and rationale all pass.</p>'}
  </section>

  <section data-dfw-id="system.colors"><h2>Colour</h2>
  ${proportion}
  ${rolesHtml}
  <h4>Status</h4>
  ${statusHtml}
  ${ramps.size ? `<h4>Palette</h4>${paletteHtml}<p class="ds-legend">Each ramp is an OKLCH scale (50 lightest → 950 darkest). A dot marks a step used by a role; other steps are reserved for states and charts.</p>` : ''}
  </section>

  <section data-dfw-id="system.typography"><h2>Typography</h2>
  ${familyHtml}
  ${scaleNote ? `<p class="ds-legend" style="margin:0 0 8px">${esc(scaleNote)}</p>` : ''}
  ${textHtml || '<p class="ds-empty">Add the text component to see the type styles.</p>'}
  </section>

  <section data-dfw-id="system.spacing"><h2>Spacing</h2>
  <div class="ds-space">${spaces.map((s) => `<div data-dfw-id="system.token.${esc(s.name)}"><code>${esc(s.name.replace('space.', 'space-'))} · ${esc(dim(s.value))}</code><i style="width:var(${cssVarName(s.name)})"></i></div>`).join('') || '<p class="ds-empty">No spacing tokens yet.</p>'}</div>
  </section>

  <section data-dfw-id="system.shape"><h2>Shape and elevation</h2>
  <div class="ds-shapes">${radii.filter((r) => !/pill/.test(r.name)).map((r) => `<div><i style="border-radius:var(${cssVarName(r.name)})"></i><code>${esc(r.name.replace('radius.', 'radius-'))} · ${esc(dim(r.value))}</code></div>`).join('')}${shadows.map((s) => `<div><i class="sh" style="border-radius:var(--radius-md, 4px);box-shadow:var(${cssVarName(s.name)})"></i><code>${esc(s.name.replace('shadow.', 'shadow-'))}</code></div>`).join('')}</div>
  </section>

  <section data-dfw-id="system.icons"><h2>Icons · ${icons.length} (Lucide)</h2>
  ${icons.length ? `<div class="ds-icons">${icons.map((n) => `<div><dfw-icon name="${esc(n)}"></dfw-icon>${esc(n)}</div>`).join('')}</div>` : '<p class="ds-empty">No icons yet. Add them with: dfw icons add &lt;name&gt; …</p>'}
  </section>

  <section data-dfw-id="system.assets"><h2>Assets · ${assets.length}</h2>
  ${assets.length ? `<div class="ds-assets">${assets.map((f) => `<figure>${/\.(png|jpe?g|svg|webp|gif)$/i.test(f) ? `<img src="${designBase}assets/${encodeURI(f)}" alt="">` : ''}<figcaption>${esc(f)}</figcaption></figure>`).join('')}</div><p class="ds-legend">Brand assets are used as supplied: never redrawn, recoloured or cropped into new marks.</p>` : '<p class="ds-empty">No assets in design/assets/ yet (logos, ornaments, photography). Placeholders must be labelled as placeholders.</p>'}
  </section>

  <section data-dfw-id="system.components"><h2>Components · ${lib.size}</h2>
  ${[...groups].map(([g, list]) => `<div class="ds-group"><h4>${esc(g)}</h4>${list.map(compSection).join('')}</div>`).join('') || '<p class="ds-empty">No components yet. Create design/system/components/&lt;name&gt;.html (see the design-format reference).</p>'}
  </section>
</body>
</html>
`;
}
