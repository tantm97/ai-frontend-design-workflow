// "dfw tokens extract --url": a DTCG token seed from a reference site or the existing app, read from
// computed styles in headless Chrome. It is a starting point for the design system brief (ui-refinement,
// "make it feel like X"), never the design system itself: it writes its own file and changes nothing else.
import fs from 'node:fs';
import path from 'node:path';
import { openRenderer, fontsReady } from './runtime.mjs';
import { hexToRgb, hexToOklch } from './color.mjs';
import { nowIso } from './util.mjs';

// Runs in the page: count style values over visible elements (backgrounds weighted by area).
function collect() {
  const hex = (c) => {
    const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?\)/.exec(c);
    if (!m || (m[4] !== undefined && Number(m[4]) < 0.5)) return null;
    return '#' + [m[1], m[2], m[3]].map((v) => Math.round(Number(v)).toString(16).padStart(2, '0')).join('');
  };
  const out = { colors: {}, families: {}, sizes: {}, radii: {}, spaces: {} };
  const bump = (bag, k, n = 1, role) => {
    if (!k) return;
    bag[k] = bag[k] || { n: 0, roles: {} };
    bag[k].n += n;
    if (role) bag[k].roles[role] = 1;
  };
  for (const el of [...document.querySelectorAll('body, body *')].slice(0, 4000)) {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none') continue;
    const area = Math.min(50, (r.width * r.height) / 20000);
    bump(out.colors, hex(cs.backgroundColor), 1 + area, 'background');
    const text = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (text) {
      bump(out.colors, hex(cs.color), 1, 'text');
      bump(out.families, cs.fontFamily, 1);
      bump(out.sizes, cs.fontSize, 1);
    }
    if (parseFloat(cs.borderTopWidth) > 0) bump(out.colors, hex(cs.borderTopColor), 1, 'border');
    if (parseFloat(cs.borderTopLeftRadius) > 0) bump(out.radii, cs.borderTopLeftRadius, 1);
    for (const k of ['paddingTop', 'paddingLeft', 'rowGap', 'columnGap']) if (parseFloat(cs[k]) > 0) bump(out.spaces, cs[k], 1);
  }
  return out;
}

const px = (v) => Math.round(parseFloat(v) * 100) / 100;
const top = (bag, n) => Object.entries(bag).sort((a, b) => b[1].n - a[1].n).slice(0, n);

/** Merge colours that look the same (OKLCH distance), keeping the most used hex of each group. */
function clusterColors(bag, max = 10) {
  const kept = [];
  for (const [hex, v] of top(bag, 60)) {
    const c = hexToOklch(hex);
    const twin = kept.find((k) => Math.hypot(k.c.l - c.l, k.c.c - c.c, (Math.min(Math.abs(k.c.h - c.h), 360 - Math.abs(k.c.h - c.h)) / 360) * Math.min(k.c.c, c.c)) < 0.02);
    if (twin) {
      twin.uses += v.n;
      Object.assign(twin.roles, v.roles);
    } else kept.push({ hex, c, uses: v.n, roles: { ...v.roles } });
  }
  return kept.sort((a, b) => b.uses - a.uses).slice(0, max);
}

export async function extractTokens(url, { width = 1440, height = 900 } = {}) {
  const r = await openRenderer();
  let data;
  try {
    const page = await r.page({ width, height });
    await page.goto(url, { timeout: 45000 });
    await fontsReady(page);
    data = await page.evaluate(collect);
  } finally {
    await r.close();
  }
  const src = (uses, extra = {}) => ({ $extensions: { 'dfw.extracted': { uses: Math.round(uses), ...extra } } });
  const color = { $type: 'color' };
  clusterColors(data.colors).forEach((k, i) => {
    color[`c${String(i + 1).padStart(2, '0')}`] = { $value: { colorSpace: 'srgb', components: hexToRgb(k.hex).map((x) => Math.round(x * 10000) / 10000), hex: k.hex }, ...src(k.uses, { roles: Object.keys(k.roles) }) };
  });
  const dim = (bag, n, prefix) => {
    const g = { $type: 'dimension' };
    const uses = new Map();
    for (const [v, x] of top(bag, 40)) if (px(v) > 0) uses.set(px(v), (uses.get(px(v)) || 0) + x.n);
    const vals = [...uses.keys()].slice(0, n).sort((a, b) => a - b);
    vals.forEach((v, i) => (g[`${prefix}${i + 1}`] = { $value: { value: v, unit: 'px' }, ...src(uses.get(v)) }));
    return g;
  };
  const families = top(data.families, 3).map(([stack, v]) => ({ stack: stack.split(',').map((f) => f.trim().replace(/^["']|["']$/g, '')), uses: v.n }));
  const fontFamily = { $type: 'fontFamily' };
  families.forEach((f, i) => (fontFamily[['primary', 'secondary', 'tertiary'][i]] = { $value: f.stack, ...src(f.uses) }));
  return {
    $description: `Extracted from ${url} on ${nowIso().slice(0, 10)} by "dfw tokens extract". A seed for the design system brief, not the design system: name the roles, merge, check contrast (dfw system audit), then build tokens.json with dfw palette build / dfw type scale.`,
    color,
    font: { family: fontFamily, size: dim(data.sizes, 10, 's') },
    radius: dim(data.radii, 5, 'r'),
    space: dim(data.spaces, 10, 'sp'),
  };
}

export async function extractTokensToFile(root, url, out) {
  const file = path.resolve(root, out || 'design/system/extracted.tokens.json');
  const tokens = await extractTokens(url);
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, `${JSON.stringify(tokens, null, 2)}\n`);
  const n = (g) => Object.keys(g).filter((k) => !k.startsWith('$')).length;
  return `Wrote ${path.relative(root, file)}: ${n(tokens.color)} colours, ${n(tokens.font.family)} font families, ${n(tokens.font.size)} font sizes, ${n(tokens.radius)} radii, ${n(tokens.space)} spacing values. It is a seed: review it with the user before using any of it.`;
}
