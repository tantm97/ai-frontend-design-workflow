import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { composeHtml, composeFile, buildSystem, loadComponents, designBaseFor } from '../../core/lib/compose.mjs';
import { lintDesign, lintScreen } from '../../core/lib/lint.mjs';
import { systemInit, systemOptions, systemChoose, listPresets } from '../../core/lib/system.mjs';
import { searchIcons, addIcons } from '../../core/lib/icons.mjs';
import { renderSystemPage } from '../../core/lib/system-page.mjs';
import { KIT_COMPONENTS, inventoryFindings, kitRefs, kitRef } from '../../core/lib/inventory.mjs';

/** Test fixture only: put starter-kit masters into a project (the CLI never copies them). */
function useKit(root, names = null) {
  const dir = path.join(root, 'design/system/components');
  fs.mkdirSync(dir, { recursive: true });
  for (const f of fs.readdirSync(KIT_COMPONENTS).filter((x) => x.endsWith('.html'))) if (!names || names.includes(f.replace('.html', ''))) fs.copyFileSync(path.join(KIT_COMPONENTS, f), path.join(dir, f));
  addIcons(root, ['chevron-down', 'triangle-alert', 'search', 'x', 'check', 'package', 'house', 'file-clock', 'settings', 'layout-dashboard', 'circle-check', 'circle-x', 'circle-alert', 'octagon-alert', 'package-open', 'ellipsis', 'wrench', 'log-in', 'log-out', 'plus']);
  buildSystem(root);
}

function project() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-sys-'));
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), '{}');
  fs.mkdirSync(path.join(root, 'design', 'tokens'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design', 'design.json'), JSON.stringify({ schema: 1, product: 'T', screens: [] }));
  return root;
}

test('compose: instances, slots, props, passthrough, nested, icons, head', () => {
  const root = project();
  systemInit(root, { preset: 'crisp' });
  useKit(root, ['button', 'card', 'badge']);
  const html = `<html><head><dfw-head></dfw-head></head><body>
<dfw-use c="card" title="A &quot;x&quot;" id="c1"><p>Body</p><dfw-fill slot="actions"><dfw-use c="button" variant="ghost" size="sm" id="c1.more" aria-label="More">More</dfw-use></dfw-fill></dfw-use>
<dfw-use c="badge" variant="err" icon="triangle-alert">Overdue</dfw-use>
<dfw-use c="button" variant="nope">X</dfw-use>
</body></html>`;
  const r = composeHtml(root, html, { designBase: '../../' });
  assert.match(r.html, /<link rel="stylesheet" href="\.\.\/\.\.\/tokens\/tokens\.css">/);
  assert.match(r.html, /system\/components\.css/);
  assert.match(r.html, /fonts\.googleapis\.com/);
  assert.match(r.html, /<section class="card card--default"[^>]*data-dfw-id="c1"[^>]*data-dfw-component="card"/);
  assert.match(r.html, /<h2 class="card__title">A &quot;x&quot;<\/h2>/);
  assert.match(r.html, /class="btn btn--ghost btn--sm"[^>]*aria-label="More"[^>]*data-dfw-id="c1.more"/);
  assert.match(r.html, /<p>Body<\/p>/);
  assert.match(r.html, /data-dfw-icon="triangle-alert"/);
  assert.doesNotMatch(r.html, /dfw-use|dfw-slot|dfw-fill|data-dfw-if/);
  assert.deepEqual(r.issues.map((i) => i.rule), ['component-variant']);
  assert.equal(r.issues[0].line, 4);
  assert.equal(r.instances.filter((i) => i.component === 'button').length, 2);
  assert.equal(designBaseFor('design/screens/a/b.html'), '../../');
});

test('compose: unknown component and icon are reported, recursion is bounded', () => {
  const root = project();
  const r = composeHtml(root, '<dfw-use c="ghost">x</dfw-use><dfw-icon name="nope"></dfw-icon>');
  assert.deepEqual(r.issues.map((i) => i.rule).sort(), ['component-unknown', 'icon-unknown']);
  fs.mkdirSync(path.join(root, 'design/system/components'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/system/components/loop.html'), '<dfw-component name="loop"><template><div><dfw-use c="loop"></dfw-use></div></template></dfw-component>');
  const r2 = composeHtml(root, '<dfw-use c="loop"></dfw-use>');
  assert.ok(r2.issues.some((i) => i.rule === 'compose-depth'));
});

test('system: init writes tokens + an empty inventory and copies no components; options and choose', () => {
  const root = project();
  fs.writeFileSync(path.join(root, 'design/tokens/tokens.json'), '{"$description":"Placeholder tokens created by dfw init."}');
  const r = systemInit(root, { preset: 'soft' });
  assert.ok(r.tokensWritten);
  assert.ok(r.inventoryCreated);
  assert.equal(r.copied, undefined);
  const css = fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8');
  assert.match(css, /--color-accent: #0f6e66/);
  assert.match(css, /--size-control: 40px/);
  assert.equal(loadComponents(root).size, 0, 'no kit masters are copied');
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, 'design/system/inventory.json'), 'utf8')).components, []);
  assert.equal(systemInit(root, { preset: 'soft' }).inventoryCreated, false, 'an existing inventory is kept');
  assert.deepEqual(listPresets().map((p) => p.name).sort(), ['crisp', 'industrial', 'soft']);

  const o = systemOptions(root, ['crisp', 'industrial']);
  assert.deepEqual(o.themes, ['opt-crisp', 'opt-industrial']);
  assert.match(fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8'), /\[data-theme="opt-industrial"\]/);
  assert.match(fs.readFileSync(path.join(root, 'design/system/head.html'), 'utf8'), /IBM\+Plex\+Sans/);
  assert.deepEqual(JSON.parse(fs.readFileSync(path.join(root, 'design/design.json'), 'utf8')).themes, ['opt-crisp', 'opt-industrial']);
  systemChoose(root, 'industrial');
  assert.match(fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8'), /--color-accent: #0b5cad/);
  assert.doesNotMatch(fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8'), /opt-/);
});

test('kit: every reference master composes its examples without issues and lints clean', () => {
  const root = project();
  systemInit(root, { preset: 'crisp' });
  useKit(root);
  for (const d of loadComponents(root).values()) {
    const r = composeHtml(root, d.examples || `<dfw-use c="${d.name}"></dfw-use>`);
    assert.deepEqual(r.issues, [], `${d.name}: ${JSON.stringify(r.issues)}`);
  }
  const findings = lintDesign(root).filter((f) => f.file.startsWith('design/system/') && f.level === 'error');
  assert.deepEqual(findings, []);
  const page = renderSystemPage(root, { designBase: '../' });
  const composed = composeHtml(root, page);
  assert.deepEqual(composed.issues, []);
  assert.match(composed.html, /data-dfw-id="system.component.button"/);
});

test('lint: screens must compose the design system and keep visual styling out', () => {
  const root = project();
  systemInit(root, { preset: 'crisp' });
  useKit(root, ['button']);
  const bad = `<!doctype html><html lang="en"><head><dfw-head></dfw-head><style>.x { display: grid; color: red; gap: 8px } .y{font-size:14px}</style></head>
<body><main data-dfw-id="m"><button data-dfw-id="b">Raw</button><span style="background: #fff">x</span></main></body></html>`;
  const f = lintScreen(root, 'design/screens/s/a.html', bad);
  const rules = f.map((x) => x.rule);
  assert.equal(rules.filter((r) => r === 'screen-visual-style').length, 3);
  assert.ok(rules.includes('use-component'));
  assert.ok(rules.includes('no-components'));
  const good = `<!doctype html><html lang="en"><head><dfw-head></dfw-head><style>.x { display: grid; gap: var(--space-2) }</style></head><body><main data-dfw-id="m"><dfw-use c="button" id="m.ok">OK</dfw-use></main></body></html>`;
  assert.deepEqual(lintScreen(root, 'design/screens/s/b.html', good), []);
});

test('icons: search ranks exact names, add copies svg and suggests on miss', () => {
  const root = project();
  assert.equal(searchIcons('scan barcode', 1)[0].name, 'scan-barcode');
  const r = addIcons(root, ['search', 'serch']);
  assert.deepEqual(r.added, ['search']);
  assert.ok(r.suggestions.serch.length >= 0);
  assert.match(fs.readFileSync(path.join(root, 'design/system/icons/search.svg'), 'utf8'), /^<svg[^>]*viewBox="0 0 24 24"/);
});

test('composeFile + buildSystem for a screen file', () => {
  const root = project();
  systemInit(root, { preset: 'crisp' });
  useKit(root, ['button']);
  fs.mkdirSync(path.join(root, 'design/screens/s'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/screens/s/a.html'), '<html><head><dfw-head></dfw-head></head><body><dfw-use c="button" id="go">Go</dfw-use></body></html>');
  const r = composeFile(root, 'design/screens/s/a.html');
  assert.match(r.html, /href="\.\.\/\.\.\/system\/components\.css"/);
  assert.equal(buildSystem(root).components, 1);
});

test('style guides: library, search, apply as aliased tokens with usage rules, latin fonts, motifs, options', async () => {
  const { listStyles, searchStyles, applyStyle, styleTokens, getStyle } = await import('../../core/lib/style.mjs');
  const { buildTokens } = await import('../../core/lib/tokens.mjs');
  assert.equal(listStyles().length, 12);
  assert.equal(searchStyles('factory counter internal')[0].id, 'industrial-utility');
  const root = project();
  systemInit(root, { style: 'heritage-signboard' });
  assert.deepEqual([...loadComponents(root).keys()], ['motif'], 'a style brings only its motif master');
  useKit(root, ['button', 'text']);
  const css = fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8');
  assert.match(css, /--color-accent: var\(--color-palette-giay-900\)/, 'heritage uses an ink accent: lacquer red is the error hue');
  assert.match(css, /--font-family-display: "Noto Serif Display"/);
  assert.match(css, /--font-size-display-xl: 85px/);
  const tok = JSON.parse(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'));
  assert.match(tok.color.palette['kim-500'].$description, /ornament/);
  assert.match(tok.color.palette['kim-500'].$description, /Used for: signature motif/);
  assert.ok(fs.existsSync(path.join(root, 'design/system/components/motif.html')));
  assert.ok(loadComponents(root).get('motif').variants.includes('signboard-frame'));
  assert.match(fs.readFileSync(path.join(root, 'design/system/head.html'), 'utf8'), /Noto\+Serif\+Display/);
  const page = composeHtml(root, renderSystemPage(root, { designBase: '../' }));
  assert.deepEqual(page.issues, []);
  assert.match(page.html, /<h2>Concept<\/h2>/);
  assert.match(page.html, /class="ds-table"/);
  assert.match(page.html, /class="ds-ramp"/);
  assert.match(page.html, /class="ds-spec"/);
  // English-only font set
  const latin = styleTokens(getStyle('editorial-luxe'), { fonts: 'latin' });
  assert.equal(latin.font.family.display.$value[0], 'Bodoni Moda');
  // options with styles
  const o = systemOptions(root, ['industrial-utility', 'swiss-grid']);
  assert.deepEqual(o.themes, ['opt-industrial-utility', 'opt-swiss-grid']);
  systemChoose(root, 'swiss-grid');
  assert.match(fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8'), /--color-accent: var\(--color-palette-coban-600\)/);
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'design/system/style.json'), 'utf8')).id, 'swiss-grid');
  // text component with prop defaults
  const t = composeHtml(root, '<dfw-use c="text" variant="display-lg" as="h1">Hi</dfw-use><dfw-use c="text">p</dfw-use>');
  assert.match(t.html, /^<h1 class="t t--display-lg t--tone-default"/);
  assert.match(t.html, /<p class="t t--body t--tone-default"[^>]*>p<\/p>/);
  buildTokens(root);
});

test('inventory: kit copies, orphans, unbuilt entries and unused masters are findings', () => {
  const root = project();
  systemInit(root, { preset: 'crisp' });
  const rules = () => inventoryFindings(root).map((f) => `${f.rule}:${f.level}`).sort();
  assert.deepEqual(rules(), [], 'empty system: nothing to report');
  useKit(root, ['button']);
  assert.deepEqual(rules(), ['inventory-missing:warn', 'kit-copy:error']);
  const dir = path.join(root, 'design/system/components');
  fs.writeFileSync(path.join(dir, 'button.html'), '<dfw-component name="button" variants="primary"><style>.b { color: var(--color-ink); }</style><template><button class="b" type="button"><dfw-slot>Go</dfw-slot></button></template><dfw-examples><dfw-use c="button">Thêm vào giỏ</dfw-use></dfw-examples></dfw-component>');
  const kitExamples = fs.readFileSync(path.join(KIT_COMPONENTS, 'badge.html'), 'utf8').match(/<dfw-examples>[\s\S]*?<\/dfw-examples>/)[0];
  fs.writeFileSync(path.join(dir, 'tag.html'), `<dfw-component name="tag"><style>.t { color: var(--color-muted); }</style><template><span class="t"><dfw-slot></dfw-slot></span></template>${kitExamples}</dfw-component>`);
  fs.writeFileSync(path.join(root, 'design/system/inventory.json'), JSON.stringify({ components: [{ name: 'button', purpose: 'Add to bag', usedBy: ['product'] }, { name: 'price', purpose: 'Price with old price', usedBy: ['product'] }] }));
  assert.deepEqual(rules(), ['inventory-orphan:warn', 'inventory-unbuilt:warn', 'kit-copy:error']);
  fs.rmSync(path.join(dir, 'tag.html'));
  fs.writeFileSync(path.join(dir, 'price.html'), '<dfw-component name="price"><template><span><dfw-slot></dfw-slot></span></template></dfw-component>');
  assert.deepEqual(rules(), []);
  fs.writeFileSync(path.join(root, 'design/design.json'), JSON.stringify({ schema: 1, product: 'T', screens: [{ id: 'product', variants: [{ id: 'a', file: 'screens/product/a.html' }] }] }));
  fs.mkdirSync(path.join(root, 'design/screens/product'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/screens/product/a.html'), '<dfw-use c="button">Thêm</dfw-use>');
  assert.deepEqual(rules(), ['unused-master:warn'], 'price is never used once screens exist');
  assert.ok(kitRefs().some((r) => r.name === 'button' && r.states.includes('focus-visible')));
  assert.match(kitRef('button'), /<dfw-component name="button"/);
});

test('compose: a nested instance keeps its own slot fills', () => {
  const root = project();
  fs.mkdirSync(path.join(root, 'design/system/components'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/system/components/box.html'), '<dfw-component name="box"><template><section><dfw-slot></dfw-slot><footer><dfw-slot name="legend"></dfw-slot></footer></section></template></dfw-component>');
  fs.writeFileSync(path.join(root, 'design/system/components/bar.html'), '<dfw-component name="bar"><template><div class="bar"><dfw-slot></dfw-slot><ul><dfw-slot name="legend"></dfw-slot></ul></div></template></dfw-component>');
  const r = composeHtml(root, '<dfw-use c="box"><dfw-use c="bar"><i>seg</i><dfw-fill slot="legend"><li>inner</li></dfw-fill></dfw-use><dfw-fill slot="legend">outer</dfw-fill></dfw-use>');
  assert.match(r.html, /<ul><li>inner<\/li><\/ul>/);
  assert.match(r.html, /<footer>outer<\/footer>/);
});

test('compose: {{prop==value?text}} marks the current item of a shared shell', () => {
  const root = project();
  fs.mkdirSync(path.join(root, 'design/system/components'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/system/components/shell.html'), '<dfw-component name="shell" props="current"><template><nav><a aria-current="{{current==home?page}}">H</a><a aria-current="{{current==list?page}}">L</a></nav><main><dfw-slot></dfw-slot></main></template></dfw-component>');
  const r = composeHtml(root, '<dfw-use c="shell" current="list">x</dfw-use>');
  assert.match(r.html, /<a aria-current="">H<\/a><a aria-current="page">L<\/a>/);
});

test('layout: void detection on measured blocks', async () => {
  const { findVoids } = await import('../../core/lib/layout.mjs');
  const base = { vw: 1366, vh: 768, content: { x: 0, w: 1366, top: 56, docH: 768 } };
  // Content ends at y=300 on a 768px screen: a tail void.
  const tail = findVoids({ ...base, blocks: [{ x: 100, y: 80, w: 900, h: 220 }] });
  assert.equal(tail[0].kind, 'tail');
  // Two blocks with a 400px empty band between them.
  const gap = findVoids({ ...base, content: { ...base.content, docH: 1400 }, blocks: [{ x: 100, y: 80, w: 900, h: 200 }, { x: 100, y: 700, w: 900, h: 500 }] });
  assert.ok(gap.some((v) => v.kind === 'gap' && v.height >= 400));
  // A filled screen: no voids.
  assert.deepEqual(findVoids({ ...base, blocks: [{ x: 0, y: 56, w: 1366, h: 712 }] }), []);
});

test('compose: prop lists with spaces, commas and spaced defaults', async () => {
  const { parseComponentFile } = await import('../../core/lib/compose.mjs');
  const [a] = parseComponentFile('<dfw-component name="x" props="asset name location site=Asset Tracking, printed"><template><i></i></template></dfw-component>', 'x.html');
  assert.deepEqual(a.props, ['asset', 'name', 'location', 'site', 'printed']);
  assert.equal(a.propDefaults.site, 'Asset Tracking');
  const [b] = parseComponentFile('<dfw-component name="y" props="as=p tone=default"><template><i></i></template></dfw-component>', 'y.html');
  assert.deepEqual(b.propDefaults, { as: 'p', tone: 'default' });
});

test('compose: {{base}} in masters, examples and instance props resolves to the page path to design/', () => {
  const root = project();
  const dir = path.join(root, 'design/system/components');
  fs.mkdirSync(dir, { recursive: true });
  fs.writeFileSync(path.join(dir, 'logo.html'), '<dfw-component name="logo" props="img={{base}}assets/logo.png"><template><a href="{{base}}screens/home/a.html"><img src="{{img}}" alt=""></a></template><dfw-examples><dfw-use c="logo"></dfw-use></dfw-examples></dfw-component>');
  fs.writeFileSync(path.join(dir, 'tile.html'), '<dfw-component name="tile" props="base"><template><i data-b="{{base}}"></i></template></dfw-component>');
  const screen = composeHtml(root, '<dfw-use c="logo"></dfw-use><dfw-use c="logo" img="{{base}}assets/alt.png"></dfw-use>', { designBase: '../../' }).html;
  assert.match(screen, /href="\.\.\/\.\.\/screens\/home\/a\.html"/);
  assert.match(screen, /src="\.\.\/\.\.\/assets\/logo\.png"/);
  assert.match(screen, /src="\.\.\/\.\.\/assets\/alt\.png"/);
  const studio = composeHtml(root, '<dfw-use c="logo"></dfw-use>', { designBase: '/design/' }).html;
  assert.match(studio, /src="\/design\/assets\/logo\.png"/);
  const page = composeHtml(root, renderSystemPage(root, { designBase: '/design/' }), { designBase: '/design/' }).html;
  assert.match(page, /src="\/design\/assets\/logo\.png"/, 'Design System page examples resolve too');
  assert.doesNotMatch(page, /\{\{base\}\}/);
  assert.match(composeHtml(root, '<dfw-use c="tile" base="x"></dfw-use>').html, /data-b="x"/, 'a master that declares a "base" prop keeps it');
});

test('tokens: presets ship target-wcag (24, floor) and target-touch (44); no ambiguous target-min', () => {
  for (const name of ['crisp', 'soft', 'industrial']) {
    const t = JSON.parse(fs.readFileSync(path.join(KIT_COMPONENTS, '..', 'presets', `${name}.tokens.json`), 'utf8'));
    assert.equal(t.size['target-wcag'].$value.value, 24);
    assert.equal(t.size['target-touch'].$value.value, 44);
    assert.equal(t.size['target-min'], undefined);
  }
});

test('system choose keeps edits made to the opt-<style> theme during the comparison', () => {
  const root = project();
  systemInit(root, { style: 'playful-bold' });
  systemOptions(root, ['playful-bold', 'warm-craft']);
  const opt = path.join(root, 'design/tokens/themes/opt-playful-bold.json');
  const t = JSON.parse(fs.readFileSync(opt, 'utf8'));
  t.color.accent = { $value: { colorSpace: 'srgb', components: [0.051, 0.4196, 0.4706], hex: '#0d6b78' }, $description: 'Edited during the comparison' };
  fs.writeFileSync(opt, JSON.stringify(t));
  systemChoose(root, 'playful-bold');
  const tok = JSON.parse(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'));
  assert.equal(tok.color.accent.$value.hex, '#0d6b78');
  assert.match(fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8'), /--color-accent: #0d6b78/);
  assert.ok(fs.existsSync(path.join(root, 'design/system/components/motif.html')), 'motif still comes from the guide');
  assert.equal(JSON.parse(fs.readFileSync(path.join(root, 'design/system/style.json'), 'utf8')).id, 'playful-bold');
  assert.ok(!fs.existsSync(opt), 'option themes are removed');
});

test('layout: small tappable controls on touch breakpoints are a touch-target finding (needs Chrome)', { skip: !(await import('../../core/lib/runtime.mjs')).browserStatus().installed }, async () => {
  const { layoutCheck } = await import('../../core/lib/layout.mjs');
  const root = project();
  systemInit(root, { preset: 'crisp' });
  fs.writeFileSync(path.join(root, 'design/design.json'), JSON.stringify({ schema: 1, product: 'T', breakpoints: [{ id: 'mobile', width: 390, height: 844 }, { id: 'desktop', width: 1440, height: 900 }], screens: [{ id: 's', variants: [{ id: 'a', file: 'screens/s/a.html' }] }] }));
  fs.mkdirSync(path.join(root, 'design/screens/s'), { recursive: true });
  fs.writeFileSync(path.join(root, 'design/screens/s/a.html'), '<!doctype html><html lang="en"><head><dfw-head></dfw-head></head><body><main><button data-dfw-id="tiny" style="height:30px;width:120px">Small</button><button data-dfw-id="ok" style="height:48px;width:120px">Big</button><p>Read the <a href="#" data-dfw-id="inline">terms</a> first.</p></main></body></html>');
  const r = await layoutCheck(root, { system: false, stress: false });
  const touch = r.findings.filter((f) => f.rule === 'touch-target');
  assert.equal(touch.length, 1, JSON.stringify(touch));
  assert.match(touch[0].message, /@ mobile/);
  assert.match(touch[0].message, /"tiny" 120x30/);
  assert.doesNotMatch(touch[0].message, /"main/);
  assert.doesNotMatch(touch[0].message, /"ok"|"inline"/);
});
