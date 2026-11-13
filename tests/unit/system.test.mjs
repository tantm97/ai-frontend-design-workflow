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
  assert.equal(r.copied.length, 21);
  assert.ok(r.tokensWritten);
  assert.ok(r.icons.includes('chevron-down'));
  const css = fs.readFileSync(path.join(root, 'design/tokens/tokens.css'), 'utf8');
  assert.match(css, /--color-accent: #0f6e66/);
  assert.match(css, /--size-control: 40px/);
  assert.ok(fs.existsSync(path.join(root, 'design/system/components.css')));
  assert.equal(loadComponents(root).size, 21);
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
  assert.match(page.html, /Style · Heritage signboard/);
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
