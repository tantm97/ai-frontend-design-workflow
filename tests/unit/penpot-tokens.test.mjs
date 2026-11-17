import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import {
  dfwToPenpot, tokenToPenpot, tokenFromPenpot, penpotDimensionType, parseColor, parseDimension, flattenTokens,
  importPenpotTokens, exportPenpotTokens, parsePenpotJson, mapSets, readPenpotFile,
} from '../../core/lib/penpot-tokens.mjs';
import { tokensToDeclarations } from '../../core/lib/tokens.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIX = path.join(HERE, '..', 'fixtures', 'penpot');
const DFW = path.resolve(HERE, '../../core/bin/dfw.mjs');
const fixture = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));

function project({ tokens = true } = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-'));
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), '{}');
  fs.mkdirSync(path.join(root, 'design', 'tokens', 'themes'), { recursive: true });
  if (tokens) {
    fs.copyFileSync(path.join(FIX, 'dfw-tokens.json'), path.join(root, 'design/tokens/tokens.json'));
    fs.copyFileSync(path.join(FIX, 'dfw-theme-dark.json'), path.join(root, 'design/tokens/themes/dark.json'));
  }
  return root;
}
const cli = (root, ...args) => execFileSync(process.execPath, [DFW, ...args], { cwd: root, env: { ...process.env, DFW_HOME: path.join(root, '.home') }, encoding: 'utf8' });

/* ------------------------------------------------------------ per-type mapping */

test('penpot tokens: colour mapping both ways', () => {
  const t = (value) => tokenToPenpot('color.x', { type: 'color', value });
  assert.deepEqual(t({ colorSpace: 'srgb', components: [0, 0.4, 1], hex: '#0066FF' }), { type: 'color', value: '#0066ff' });
  assert.equal(t({ colorSpace: 'srgb', components: [0, 0, 0], alpha: 0.5 }).value, 'rgba(0, 0, 0, 0.5)');
  assert.equal(t('{color.blue}').value, '{color.blue}');
  const notes = [];
  assert.match(tokenToPenpot('c', { type: 'color', value: { colorSpace: 'oklch', components: [0.6, 0.1, 250] } }, (n) => notes.push(n)).value, /^#[0-9a-f]{6}$/);
  assert.match(notes[0], /oklch/);
  assert.ok(t({ colorSpace: 'lab', components: [50, 0, 0] }).unsupported);

  assert.deepEqual(parseColor('#0066ff'), { colorSpace: 'srgb', components: [0, 0.4, 1], hex: '#0066ff' });
  assert.deepEqual(parseColor('#fff'), { colorSpace: 'srgb', components: [1, 1, 1], hex: '#ffffff' });
  assert.equal(parseColor('#00000080').alpha, 0.502);
  assert.deepEqual(parseColor('rgba(255, 102, 0, 0.5)'), { colorSpace: 'srgb', components: [1, 0.4, 0], alpha: 0.5, hex: '#ff6600' });
  assert.equal(parseColor('hsl(210, 50%, 40%)').hex, '#336699');
  assert.equal(parseColor('darken({a}, 10)'), null);
});

test('penpot tokens: dimension types by name (radius, spacing, sizing, border width, font size)', () => {
  const px = { value: 4, unit: 'px' };
  assert.equal(penpotDimensionType('radius.md', px), 'borderRadius');
  assert.equal(penpotDimensionType('space.1-5', px), 'spacing');
  assert.equal(penpotDimensionType('size.control', px), 'sizing');
  assert.equal(penpotDimensionType('border.width', px), 'borderWidth');
  assert.equal(penpotDimensionType('focus.width', px), 'borderWidth');
  assert.equal(penpotDimensionType('font.size.md', px), 'fontSizes');
  assert.equal(penpotDimensionType('font.size.md', { value: 1, unit: 'rem' }), 'dimension');
  assert.equal(penpotDimensionType('font.tracking.tight', { value: 0.5, unit: 'px' }), 'letterSpacing');
  assert.equal(penpotDimensionType('elevation.offset', px), 'dimension');
  assert.deepEqual(tokenToPenpot('space.4', { type: 'dimension', value: { value: 16, unit: 'px' } }), { type: 'spacing', value: '16px' });
  for (const pt of ['dimension', 'spacing', 'sizing', 'borderRadius', 'borderWidth', 'fontSizes', 'letterSpacing']) {
    assert.deepEqual(tokenFromPenpot('x', { type: pt, value: '12' }).value, { value: 12, unit: 'px' }, pt);
  }
  assert.deepEqual(parseDimension('1.5rem'), { value: 1.5, unit: 'rem' });
  const w = [];
  const expr = tokenFromPenpot('space.8', { type: 'spacing', value: '{space.4} * 2' }, 'dimension', (x) => w.push(x));
  assert.equal(expr.value, 'calc({space.4} * 2)');
  assert.match(w[0], /math expression/);
  assert.equal(Object.fromEntries(tokensToDeclarations({ space: { 8: { $type: 'dimension', $value: expr.value } } }))['--space-8'], 'calc(var(--space-4) * 2)');
  assert.equal(tokenToPenpot('space.8', { type: 'dimension', value: expr.value }).value, '{space.4} * 2');
});

test('penpot tokens: font family, weight, number, duration, string, unsupported', () => {
  assert.deepEqual(tokenToPenpot('font.family.body', { type: 'fontFamily', value: ['Inter', 'Segoe UI', 'sans-serif'] }), { type: 'fontFamilies', value: 'Inter, Segoe UI, sans-serif' });
  assert.deepEqual(tokenFromPenpot('f', { type: 'fontFamilies', value: '"Source Sans 3", sans-serif' }).value, ['Source Sans 3', 'sans-serif']);
  assert.deepEqual(tokenToPenpot('w', { type: 'fontWeight', value: 600 }), { type: 'fontWeights', value: '600' });
  assert.equal(tokenFromPenpot('w', { type: 'fontWeights', value: 'Semi Bold' }).value, 600);
  assert.equal(tokenFromPenpot('w', { type: 'fontWeights', value: '300' }).value, 300);
  assert.deepEqual(tokenToPenpot('n', { type: 'number', value: 1.5 }), { type: 'number', value: '1.5' });
  assert.equal(tokenFromPenpot('o', { type: 'opacity', value: '45%' }).value, 0.45);
  const notes = [];
  assert.deepEqual(tokenToPenpot('duration.slow', { type: 'duration', value: { value: 0.3, unit: 's' } }, (n) => notes.push(n)), { type: 'number', value: '300' });
  assert.match(notes[0], /no duration type/);
  assert.deepEqual(tokenFromPenpot('duration.slow', { type: 'number', value: '300' }), { type: 'duration', value: { value: 300, unit: 'ms' } });
  assert.deepEqual(tokenFromPenpot('motion.x', { type: 'number', value: '300' }, 'duration'), { type: 'duration', value: { value: 300, unit: 'ms' } });
  assert.deepEqual(tokenToPenpot('font.eyebrow.case', { type: 'string', value: 'uppercase' }), { type: 'textCase', value: 'uppercase' });
  assert.deepEqual(tokenFromPenpot('c', { type: 'textCase', value: 'uppercase' }), { type: 'string', value: 'uppercase' });
  assert.ok(tokenToPenpot('s', { type: 'string', value: 'hello' }).unsupported);
  assert.ok(tokenToPenpot('e', { type: 'cubicBezier', value: [0.2, 0, 0, 1] }).unsupported);
  assert.ok(tokenToPenpot('b', { type: 'border', value: {} }).unsupported);
  assert.ok(tokenFromPenpot('b', { type: 'boolean', value: 'true' }).unsupported);
  assert.equal(tokenToPenpot('d', { type: 'color', value: { colorSpace: 'srgb', components: [0, 0, 0], hex: '#000000' }, description: 'Ink' }).description, 'Ink');
});

test('penpot tokens: shadow and typography composites', () => {
  const shadow = [{ color: { colorSpace: 'srgb', components: [0, 0, 0], hex: '#000000', alpha: 0.5 }, offsetX: { value: 0, unit: 'px' }, offsetY: { value: 2, unit: 'px' }, blur: { value: 4, unit: 'px' }, spread: { value: 0, unit: 'px' } }];
  const ps = tokenToPenpot('shadow.sm', { type: 'shadow', value: shadow });
  assert.deepEqual(ps, { type: 'shadow', value: [{ color: 'rgba(0, 0, 0, 0.5)', offsetX: '0', offsetY: '2', blur: '4', spread: '0', inset: false }] });
  const back = tokenFromPenpot('shadow.sm', ps);
  assert.deepEqual(back.value[0].offsetY, { value: 2, unit: 'px' });
  assert.equal(back.value[0].color.alpha, 0.5);
  assert.equal(tokenFromPenpot('s', { type: 'shadow', value: { color: '{color.ink}', x: '0', y: '1', blur: '0', spread: '0', type: 'innerShadow' } }).value.inset, true);

  const typo = { fontFamily: '{font.family.body}', fontSize: { value: 16, unit: 'px' }, fontWeight: 600, letterSpacing: { value: 0, unit: 'px' }, lineHeight: 1.4 };
  const pt = tokenToPenpot('type.body', { type: 'typography', value: typo });
  assert.deepEqual(pt.value, { fontFamilies: '{font.family.body}', fontSizes: '16px', fontWeights: '600', letterSpacing: '0px', lineHeights: '1.4' });
  assert.deepEqual(tokenFromPenpot('type.body', pt).value, typo);
  assert.deepEqual(tokenFromPenpot('t', { type: 'typography', value: { fontFamily: 'Inter', fontSize: '14', lineHeight: '150%' } }).value, { fontFamily: ['Inter'], fontSize: { value: 14, unit: 'px' }, lineHeight: 1.5 });
});

/* ------------------------------------------------------------ file structure */

test('penpot tokens: export structure (sets, themes, metadata, aliases, unsupported reported)', () => {
  const { json, report } = dfwToPenpot({ base: fixture('dfw-tokens.json'), themes: { dark: fixture('dfw-theme-dark.json') } });
  assert.deepEqual(Object.keys(json), ['base', 'themes/dark', '$themes', '$metadata']);
  assert.deepEqual(json.$metadata, { tokenSetOrder: ['base', 'themes/dark'], activeThemes: ['dfw/default'], activeSets: ['base'] });
  assert.deepEqual(json.$themes.map((t) => [t.group, t.name, t.selectedTokenSets]), [['dfw', 'default', { base: 'enabled' }], ['dfw', 'dark', { base: 'enabled', 'themes/dark': 'enabled' }]]);
  assert.deepEqual(json.base.color.accent, { $value: '{color.blue.600}', $type: 'color' });
  assert.equal(json.base.color.ink.$description, 'Body text');
  assert.equal(json['themes/dark'].color.ink.$value, '#f0f0f2');
  assert.deepEqual(report.unsupported.map((u) => u.token).sort(), ['easing.out', 'font.eyebrow.note']);
  assert.ok(report.lossy.some((l) => l.token === 'duration.fast'));
  // Every exported token has a Penpot type name.
  const types = new Set();
  const walk = (n) => (n && typeof n === 'object' ? ('$value' in n ? types.add(n.$type) : Object.values(n).forEach(walk)) : null);
  walk(json.base);
  assert.deepEqual([...types].sort(), ['borderRadius', 'borderWidth', 'color', 'dimension', 'fontFamilies', 'fontSizes', 'fontWeights', 'number', 'shadow', 'sizing', 'spacing', 'textCase', 'typography']);
});

test('penpot tokens: single-set files and set mapping', () => {
  const single = parsePenpotJson({ color: { a: { $value: '#000000', $type: 'color' } } }, 'core');
  assert.equal(mapSets(single).baseName, 'core');
  const multi = parsePenpotJson({ brand: {}, 'themes/dark': {}, 'Light/Colors': {}, $themes: [], $metadata: { tokenSetOrder: ['brand', 'themes/dark', 'Light/Colors'] } });
  const m = mapSets(multi);
  assert.equal(m.baseName, 'brand');
  assert.deepEqual(m.map.get('themes/dark'), { kind: 'theme', name: 'dark' });
  assert.deepEqual(m.skipped, ['Light/Colors']);
  // Multi-file export (folder of set files + $themes.json/$metadata.json).
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-multi-'));
  fs.mkdirSync(path.join(dir, 'themes'));
  fs.writeFileSync(path.join(dir, 'base.json'), JSON.stringify({ color: { a: { $value: '#000000', $type: 'color' } } }));
  fs.writeFileSync(path.join(dir, 'themes', 'dark.json'), JSON.stringify({ color: { a: { $value: '#ffffff', $type: 'color' } } }));
  fs.writeFileSync(path.join(dir, '$metadata.json'), JSON.stringify({ tokenSetOrder: ['base', 'themes/dark'] }));
  fs.writeFileSync(path.join(dir, '$themes.json'), '[]');
  const r = readPenpotFile(dir);
  assert.deepEqual([...r.sets.keys()].sort(), ['base', 'themes/dark']);
});

/* ------------------------------------------------------------ round trips */

test('penpot tokens: dfw -> penpot -> dfw is exact in the exporting project', () => {
  const root = project();
  const before = fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8');
  const beforeDark = fs.readFileSync(path.join(root, 'design/tokens/themes/dark.json'), 'utf8');
  const { file } = exportPenpotTokens(root, {});
  assert.ok(file.endsWith(path.join('.dfw', 'penpot', 'tokens.penpot.json')));
  const r = importPenpotTokens(root, file, {});
  for (const d of Object.values(r.diff)) assert.deepEqual([d.added, d.changed, d.removed], [[], [], []]);
  assert.deepEqual(r.written, []);
  assert.equal(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'), before);
  assert.equal(fs.readFileSync(path.join(root, 'design/tokens/themes/dark.json'), 'utf8'), beforeDark);
});

test('penpot tokens: dfw -> penpot -> fresh project keeps every supported token (8-bit sRGB colours)', () => {
  const src = project();
  const { file } = exportPenpotTokens(src, {});
  const dst = project({ tokens: false });
  importPenpotTokens(dst, file, {});
  const a = flattenTokens(fixture('dfw-tokens.json'));
  const b = flattenTokens(JSON.parse(fs.readFileSync(path.join(dst, 'design/tokens/tokens.json'), 'utf8')));
  const unsupported = ['easing.out', 'font.eyebrow.note'];
  assert.deepEqual([...b.keys()].sort(), [...a.keys()].filter((k) => !unsupported.includes(k)).sort());
  for (const [name, t] of b) {
    const o = a.get(name);
    assert.equal(t.type, o.type, name);
    if (o.type === 'color' && typeof o.value === 'object') {
      if (o.value.hex) assert.equal(t.value.hex, o.value.hex, name);
      continue;
    }
    if (o.type === 'shadow') continue; // colour inside quantised; geometry checked below
    assert.deepEqual(t.value, o.value, name);
  }
  assert.deepEqual(b.get('shadow.sm').value[0].offsetY, { value: 1, unit: 'px' });
  assert.ok(fs.existsSync(path.join(dst, 'design/tokens/themes/dark.json')));
});

test('penpot tokens: import of a Penpot-edited file -> diff, notes, skipped sets', () => {
  const root = project();
  const r = importPenpotTokens(root, path.join(FIX, 'penpot-edited.tokens.json'), {});
  const d = r.diff.base;
  assert.deepEqual(d.changed.map((c) => c.token).sort(), ['color.blue.600', 'duration.fast', 'font.family.body', 'font.size.md', 'radius.md', 'space.4']);
  assert.deepEqual(d.added.map((c) => c.token).sort(), ['color.brand', 'color.mix', 'opacity.muted']);
  assert.ok(d.kept.some((k) => k.token === 'color.veil'));
  assert.deepEqual(r.diff['themes/dark'].changed.map((c) => c.token), ['color.ink']);
  assert.deepEqual(r.skippedSets, ['Brands/A']);
  assert.deepEqual(r.unsupported.map((u) => u.token), ['flag']);
  const t = JSON.parse(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'));
  assert.deepEqual(t.duration.fast, { $value: { value: 150, unit: 'ms' } }); // group $type kept, value in place
  assert.deepEqual(t.color.blue['600'].$value, { colorSpace: 'srgb', components: [0, 0.3333, 0.8667], hex: '#0055dd' });
  assert.deepEqual(t.easing.out.$value, [0.2, 0, 0, 1]); // unsupported tokens stay
  assert.equal(t.font.weight.bold.$value, 700); // "Bold" == 700: unchanged
  // --prune removes tokens deleted in Penpot.
  const p = importPenpotTokens(project(), path.join(FIX, 'penpot-edited.tokens.json'), { prune: true });
  assert.ok(p.diff.base.removed.some((x) => x.token === 'color.veil'));
});

test('penpot tokens: CLI export/import, refused while frozen, dry run allowed', () => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-cli-'));
  cli(root, 'init', '--product', 'T');
  fs.copyFileSync(path.join(FIX, 'dfw-tokens.json'), path.join(root, 'design/tokens/tokens.json'));
  const out = cli(root, 'tokens', 'export', '--format', 'penpot', '--out', 'pp.json');
  assert.match(out, /Wrote pp\.json/);
  assert.match(out, /Not exported: base easing\.out \(cubicBezier\)/);
  assert.match(cli(root, 'tokens', 'import', path.join(FIX, 'penpot-edited.tokens.json'), '--dry-run'), /color\.blue\.600: #0066ff -> #0055dd[\s\S]*Dry run/);
  const state = JSON.parse(fs.readFileSync(path.join(root, '.dfw/state.json'), 'utf8'));
  state.design.locked = true;
  state.design.status = 'approved';
  fs.writeFileSync(path.join(root, '.dfw/state.json'), JSON.stringify(state));
  assert.throws(() => cli(root, 'tokens', 'import', path.join(FIX, 'penpot-edited.tokens.json')), /frozen \(G2\)[\s\S]*dfw reopen/);
  assert.match(cli(root, 'tokens', 'import', path.join(FIX, 'penpot-edited.tokens.json'), '--dry-run'), /Dry run/);
  state.design.locked = false;
  fs.writeFileSync(path.join(root, '.dfw/state.json'), JSON.stringify(state));
  assert.match(cli(root, 'tokens', 'import', path.join(FIX, 'penpot-edited.tokens.json')), /tokens\.css rebuilt/);
  assert.match(fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8'), /--color-blue-600: #0055dd;/);
});

test('penpot tokens: exact round trip for every starter preset', () => {
  const presets = path.resolve(HERE, '../../core/kits/base/presets');
  for (const f of fs.readdirSync(presets).filter((x) => x.endsWith('.tokens.json'))) {
    const root = project({ tokens: false });
    fs.copyFileSync(path.join(presets, f), path.join(root, 'design/tokens/tokens.json'));
    const before = fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8');
    const { file, report } = exportPenpotTokens(root, {});
    assert.deepEqual(report.unsupported.map((u) => u.type), ['cubicBezier'], f);
    const r = importPenpotTokens(root, file, {});
    assert.deepEqual(r.written, [], f);
    assert.equal(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'), before, f);
  }
});
