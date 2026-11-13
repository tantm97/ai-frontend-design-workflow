import { test } from 'node:test';
import assert from 'node:assert/strict';
import { hexToOklch, oklchToHex, contrast, buildPalette, auditColors, typeScale, auditTypeScale, hueDistance } from '../../core/lib/color.mjs';
import { matchStyles, listStyles } from '../../core/lib/style.mjs';

const resolve = (pal) => Object.fromEntries(Object.entries(pal.roles).map(([k, v]) => [k, v.startsWith('#') ? v : pal.ramps[v.slice(0, v.lastIndexOf('-'))][v.slice(v.lastIndexOf('-') + 1)]]));

test('oklch round trip and contrast', () => {
  for (const hex of ['#0b5cad', '#f5c400', '#9a2f22', '#fbf7ef']) {
    const back = oklchToHex(hexToOklch(hex));
    assert.ok(contrast(back, hex) < 1.02, `${hex} -> ${back}`);
  }
  assert.equal(Math.round(contrast('#000000', '#ffffff')), 21);
});

test('palette: roles picked by contrast, info merges into a blue accent, audit clean', () => {
  const pal = buildPalette({ brand: { hue: 250, chroma: 0.12 }, neutral: { hue: 240, tint: 0.008 }, signature: { hue: 95, chroma: 0.17 } }, { brand: 'xanh', neutral: 'thep', signature: 'vang' });
  const roles = resolve(pal);
  assert.ok(contrast(roles.ink, roles.surface) >= 13);
  assert.ok(contrast(roles.border, roles.surface) >= 3);
  assert.ok(contrast(roles['on-accent'], roles.accent) >= 4.5);
  assert.equal(pal.status.info, pal.ramps.xanh[600] === pal.status.info ? pal.status.info : pal.status.info);
  assert.ok(pal.notes.some((n) => /info uses the accent/.test(n)));
  assert.deepEqual(auditColors({ roles, status: pal.status }).filter((f) => f.level === 'error'), []);
});

test('audit: a red accent next to error red is a MUST; ink accent fixes it', () => {
  const red = buildPalette({ brand: { hue: 28, chroma: 0.16 }, neutral: { hue: 80, tint: 0.012 } });
  const f = auditColors({ roles: resolve(red), status: red.status });
  assert.ok(f.some((x) => x.level === 'error' && x.rule === 'accent-vs-status'));
  const ink = buildPalette({ brand: { hue: 28, chroma: 0.16 }, neutral: { hue: 80, tint: 0.012 }, accentFrom: 'neutral' });
  const g = auditColors({ roles: resolve(ink), status: ink.status });
  assert.ok(!g.some((x) => x.rule === 'accent-vs-status'));
  // status red stays red (not bent to orange to dodge the accent)
  assert.ok(hueDistance(hexToOklch(ink.status.err).h, 27) <= 8);
});

test('audit: mixed neutral temperatures and hand-picked type sizes are flagged', () => {
  const roles = { canvas: '#f3f5f8', surface: '#ffffff', 'surface-sunken': '#eef1f5', rule: '#dde2e8', border: '#7c8590', muted: '#5a5048', ink: '#2a2118', accent: '#1d4ed8', 'on-accent': '#ffffff', 'accent-soft': '#e8effd', 'accent-text': '#1e40af', focus: '#1d4ed8' };
  assert.ok(auditColors({ roles, status: {} }).some((f) => f.rule === 'neutral-temperature'));
  const t = auditTypeScale({ ui: [12, 13, 14, 15, 16], display: [22, 26, 34, 40] }, { body: 16 });
  assert.ok(t.some((f) => f.rule === 'type-ratio'));
  assert.ok(t.some((f) => f.rule === 'type-steps'));
  const ts = typeScale({ base: 16, ratio: 1.2, displayRatio: 1.414, displayFrom: 30 });
  assert.deepEqual(Object.values(ts.display), [30, 42, 60, 85]);
  assert.deepEqual(auditTypeScale({ ui: ['2xs', 'xs', 'sm', 'md', 'lg', 'xl', '2xl'].map((k) => ts.ui[k]), display: Object.values(ts.display) }, { body: 16 }), []);
});

test('style library: every guide has rationale, axes, a generated palette and no MUST audit', () => {
  for (const g of listStyles()) {
    assert.ok(g.rationale.length >= 3, g.id);
    assert.equal(Object.keys(g.axes).length, 7, g.id);
    assert.ok(Object.keys(g.palette).length >= 22, g.id);
    assert.ok(!g.audit.some((a) => a.level === 'error'), g.id);
    const acc = g.roles.accent.startsWith('#') ? g.roles.accent : g.palette[g.roles.accent].hex;
    const oc = hexToOklch(acc);
    if (oc.c > 0.05) for (const s of ['err', 'ok']) assert.ok(hueDistance(oc.h, hexToOklch(g.status[s]).h) >= 25, `${g.id} accent vs ${s}`);
  }
});

test('style match ranks by personality with explanations', () => {
  const r = matchStyles({ personality: { formality: 1, energy: 5, era: 5, warmth: 4, density: 2, richness: 4, seriousness: 1 } });
  assert.equal(r[0].id, 'playful-bold');
  const ops = matchStyles({ personality: { formality: 3, energy: 2, era: 4, warmth: 2, density: 5, richness: 1, seriousness: 4 }, constraints: { dark: false } });
  assert.ok(['ops-dense', 'swiss-grid', 'industrial-utility'].includes(ops[0].id));
  assert.ok(ops.at(-1).mismatches.length > 0);
});
