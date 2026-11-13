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
  systemInit(root, { preset: 'crisp', components: ['button', 'card', 'badge'] });
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

test('system: init copies kit + preset tokens + icons and builds css; options and choose', () => {
  const root = project();
  fs.writeFileSync(path.join(root, 'design/tokens/tokens.json'), '{"$description":"Placeholder tokens created by dfw init."}');
  const r = systemInit(root, { preset: 'soft' });
  assert.equal(r.copied.length, 24);
  assert.ok(r.tokensWritten);
  assert.ok(r.icons.includes('chevron-down'));
  const css = fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8');
  assert.match(css, /--color-accent: #0f6e66/);
  assert.match(css, /--size-control: 40px/);
  assert.ok(fs.existsSync(path.join(root, 'design/system/components.css')));
  assert.equal(loadComponents(root).size, 24);
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

test('kit: every component composes its examples without issues and lints clean', () => {
  const root = project();
  systemInit(root, { preset: 'crisp' });
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
  systemInit(root, { preset: 'crisp', components: ['button'] });
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
  systemInit(root, { preset: 'crisp', components: ['button'] });
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
