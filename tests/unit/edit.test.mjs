import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { applyEdit, undoEdit, transform, setAttr, setStyleProp, parseStyle } from '../../core/lib/edit.mjs';
import { studioPages } from '../../core/studio/pages.mjs';

const SCREEN = `<!doctype html><html><head><style>.x[data-dfw-id="home.title"] { color: red }</style></head><body>
<!-- <h1 data-dfw-id="home.title">commented out</h1> -->
<main data-dfw-id="home.main" class="m" style="color: var(--color-ink); padding: 4px">
  <h1 data-dfw-id="home.title">
    Welcome back
  </h1>
  <p data-dfw-id="home.lede">Orders &amp; stock <b>today</b></p>
  <dfw-use c="button" variant="primary" id="home.cta" kbd="Enter">New order</dfw-use>
  <dfw-use c="button" id="home.more"><dfw-fill slot="default"><b>More</b></dfw-fill></dfw-use>
  <input data-dfw-id="home.search">
  <div data-dfw-id="dup">a</div><div data-dfw-id="dup">b</div>
</main>
<script>const s = '<p data-dfw-id="home.lede">x</p>';</script>
</body></html>`;

const BUTTON = `<dfw-component name="button" variants="primary secondary ghost" sizes="md lg" props="kbd">
<style>.btn{}</style>
<template><button class="btn btn--{{variant}}"><dfw-slot></dfw-slot><kbd data-dfw-if="kbd">{{kbd}}</kbd></button></template>
</dfw-component>`;

const TOKENS = {
  space: { $type: 'dimension', 1: { $value: { value: 4, unit: 'px' } }, 4: { $value: { value: 16, unit: 'px' } } },
  color: { $type: 'color', ink: { $value: '#111111' } },
};

function project() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-edit-'));
  const w = (rel, text) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  w('design/screens/home/a.html', SCREEN);
  w('design/system/components/button.html', BUTTON);
  w('design/tokens/tokens.json', JSON.stringify(TOKENS));
  w('design/approved/v1/screens/home/a.html', SCREEN);
  w('src/app.html', SCREEN);
  return root;
}
const read = (root, rel = 'design/screens/home/a.html') => fs.readFileSync(path.join(root, rel), 'utf8');

test('edit text: replaces only the element text, keeps whitespace, escapes HTML', () => {
  const root = project();
  const r = applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.title', value: 'Hi <you> & co' });
  assert.equal(r.changed, true);
  assert.equal(r.file, 'design/screens/home/a.html');
  const after = read(root);
  assert.match(after, /<h1 data-dfw-id="home.title">\n    Hi &lt;you&gt; &amp; co\n  <\/h1>/);
  assert.match(after, /commented out/, 'comment untouched');
  assert.match(after, /color: red/, 'style block untouched');
  assert.equal(after.replace('Hi &lt;you&gt; &amp; co', 'Welcome back'), SCREEN, 'nothing else changed');
});

test('edit text: component instance slot text via <dfw-use id>', () => {
  const root = project();
  applyEdit(root, { file: 'design/screens/home/a.html', op: 'text', id: 'home.cta', value: 'Create order' });
  assert.match(read(root), /<dfw-use c="button" variant="primary" id="home.cta" kbd="Enter">Create order<\/dfw-use>/);
});

test('edit text: refusals (child elements, void, unknown, duplicate ids)', () => {
  const root = project();
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.lede', value: 'x' }), /child elements/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.more', value: 'x' }), /child elements/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.search', value: 'x' }), /no text/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'nope', value: 'x' }), /not found/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'dup', value: 'x' }), /2 times/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.title', value: '   ' }), /empty/);
  assert.equal(read(root), SCREEN, 'file unchanged after refusals');
});

test('edit boundary: refuses design/approved, files outside design/, traversal and non-html', () => {
  const root = project();
  assert.throws(() => applyEdit(root, { file: 'approved/v1/screens/home/a.html', op: 'text', id: 'home.title', value: 'x' }), /approved/);
  assert.throws(() => applyEdit(root, { file: 'design/approved/v1/screens/home/a.html', op: 'text', id: 'home.title', value: 'x' }), /approved/);
  assert.throws(() => applyEdit(root, { file: '../src/app.html', op: 'text', id: 'home.title', value: 'x' }), /under design/);
  assert.throws(() => applyEdit(root, { file: 'tokens/tokens.json', op: 'text', id: 'x', value: 'x' }), /HTML/);
  assert.equal(read(root, 'src/app.html'), SCREEN);
});

test('edit variant / size / prop on a component instance', () => {
  const root = project();
  applyEdit(root, { file: 'screens/home/a.html', op: 'variant', id: 'home.cta', value: 'secondary' });
  applyEdit(root, { file: 'screens/home/a.html', op: 'size', id: 'home.cta', value: 'lg' });
  applyEdit(root, { file: 'screens/home/a.html', op: 'prop', id: 'home.cta', prop: 'kbd', value: 'F2' });
  assert.match(read(root), /<dfw-use c="button" variant="secondary" id="home.cta" kbd="F2" size="lg">New order<\/dfw-use>/);
  applyEdit(root, { file: 'screens/home/a.html', op: 'prop', id: 'home.cta', prop: 'kbd', value: '' });
  assert.match(read(root), /<dfw-use c="button" variant="secondary" id="home.cta" size="lg">New order<\/dfw-use>/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'variant', id: 'home.cta', value: 'huge' }), /no variant "huge"/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'prop', id: 'home.cta', prop: 'onclick', value: 'x' }), /no prop/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'variant', id: 'home.title', value: 'primary' }), /not a component instance/);
});

test('edit spacing: sets/replaces inline style with space tokens only', () => {
  const root = project();
  applyEdit(root, { file: 'screens/home/a.html', op: 'spacing', id: 'home.main', prop: 'padding', value: 'space.4' });
  assert.match(read(root), /style="color: var\(--color-ink\); padding: var\(--space-4\)"/);
  applyEdit(root, { file: 'screens/home/a.html', op: 'spacing', id: 'home.main', prop: 'gap', value: '--space-1' });
  assert.match(read(root), /style="color: var\(--color-ink\); padding: var\(--space-4\); gap: var\(--space-1\)"/);
  applyEdit(root, { file: 'screens/home/a.html', op: 'spacing', id: 'home.title', prop: 'margin-top', value: 'space.1' });
  assert.match(read(root), /<h1 data-dfw-id="home.title" style="margin-top: var\(--space-1\)">/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'spacing', id: 'home.main', prop: 'padding', value: 'color.ink' }), /space\.\* token/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'spacing', id: 'home.main', prop: 'color', value: 'space.1' }), /not editable/);
  assert.throws(() => applyEdit(root, { file: 'screens/home/a.html', op: 'spacing', id: 'home.cta', prop: 'padding', value: 'space.1' }), /component instance/);
});

test('undo restores edits in reverse order and refuses when the file changed since', () => {
  const root = project();
  applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.title', value: 'One' });
  applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.title', value: 'Two' });
  const recs = fs.readdirSync(path.join(root, '.dfw/edits'));
  assert.equal(recs.length, 2);
  const rec = JSON.parse(fs.readFileSync(path.join(root, '.dfw/edits', recs.sort()[1]), 'utf8'));
  assert.deepEqual(Object.keys(rec).sort(), ['after', 'at', 'before', 'file', 'op']);
  assert.equal(undoEdit(root).remaining, 1);
  assert.match(read(root), />\n    One\n  </);
  undoEdit(root);
  assert.equal(read(root), SCREEN);
  assert.throws(() => undoEdit(root), /Nothing to undo/);

  applyEdit(root, { file: 'screens/home/a.html', op: 'text', id: 'home.title', value: 'Three' });
  fs.writeFileSync(path.join(root, 'design/screens/home/a.html'), read(root) + '\n<!-- edited by Claude -->');
  assert.throws(() => undoEdit(root), /changed after that edit/);
});

test('helpers: setAttr, parseStyle, setStyleProp', () => {
  assert.equal(setAttr('<a href="x" data-q=\'1 > 2\'>', 'data-q', 'y'), '<a href="x" data-q="y">');
  assert.equal(setAttr('<img src="a.png"/>', 'alt', 'A "b"'), '<img src="a.png" alt="A &quot;b&quot;"/>');
  assert.equal(setAttr('<p title="variant=x" class="c">', 'variant', 'v'), '<p title="variant=x" class="c" variant="v">', 'does not match inside values');
  assert.deepEqual(parseStyle('background: url("a;b"); color: red'), [
    { prop: 'background', value: 'url("a;b")' },
    { prop: 'color', value: 'red' },
  ]);
  assert.equal(setStyleProp('padding-top: 1px; padding: 2px; color: red', 'padding', 'var(--space-1)'), 'color: red; padding: var(--space-1)');
  assert.equal(setStyleProp('gap: 1px', 'gap', null), '');
  assert.equal(transform(project(), SCREEN, { op: 'text', id: 'home.title', value: 'Welcome back' }), SCREEN);
});

test('studio pages: system first, declared pages, Screens + Explorations fallback', () => {
  const screens = [{ id: 'a', variants: [] }, { id: 'b', page: 'flow', variants: [] }, { id: '_dir', variants: [] }, { id: 'c', variants: [] }];
  const p1 = studioPages({ screens });
  assert.deepEqual(p1.map((p) => [p.id, p.screens]), [['system', []], ['flow', ['b']], ['screens', ['a', 'c']], ['explorations', ['_dir']]]);
  const p2 = studioPages({ screens, pages: [{ id: 'main', title: 'Main', screens: ['c', 'a'] }] });
  assert.deepEqual(p2.map((p) => [p.id, p.title, p.screens]), [['system', 'Design System', []], ['main', 'Main', ['c', 'a']], ['flow', 'flow', ['b']], ['explorations', 'Explorations', ['_dir']]]);
  assert.deepEqual(studioPages({ screens: [] }).map((p) => p.id), ['system', 'screens']);
  // Page metadata and section-only screen refs ("id/variant") pass through.
  const notes = [{ title: 'Frame', items: ['Rail 72'] }];
  const sections = [{ kind: 'states', screens: ['c/x'], states: ['empty'] }];
  const p3 = studioPages({ screens, pages: [{ id: 'system', notes }, { id: 'm', title: 'M', description: 'Desc', notes, sections }] });
  assert.deepEqual(p3[0].notes, notes, 'system page notes');
  assert.equal(p3[1].description, 'Desc');
  assert.deepEqual(p3[1].sections, sections);
  assert.deepEqual(p3[1].screens, ['c'], 'screen referenced only from a section belongs to the page');
});

/* ---------------------------------------------------------------- layout: reorder + resize */

const GRID = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <style>.cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-4) }</style>
</head>
<body>
  <main data-dfw-id="dash">
    <section class="cards" data-dfw-id="dash.cards">
      <div data-dfw-id="dash.a">A</div>
      <article data-dfw-id="dash.b">
        <h2 data-dfw-id="dash.b.title">B</h2>
      </article>
      <dfw-use c="button" id="dash.c">C</dfw-use>
    </section>
    <div class="row" data-dfw-id="dash.row"><span data-dfw-id="dash.x">x</span><span data-dfw-id="dash.y">y</span></div>
  </main>
</body>
</html>`;

function gridProject() {
  const root = project();
  fs.writeFileSync(path.join(root, 'design/screens/home/grid.html'), GRID);
  const tokens = { ...TOKENS, size: { $type: 'dimension', sidebar: { $value: { value: 240, unit: 'px' } } } };
  fs.writeFileSync(path.join(root, 'design/tokens/tokens.json'), JSON.stringify(tokens));
  return root;
}
const G = 'screens/home/grid.html';
const readG = (root) => read(root, `design/${G}`);

test('move: reorders whole-line siblings by id and re-indents multi-line elements', () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'move', id: 'dash.c', ref: 'dash.a', position: 'before' });
  assert.match(readG(root), /<section class="cards" data-dfw-id="dash.cards">\n      <dfw-use c="button" id="dash.c">C<\/dfw-use>\n      <div data-dfw-id="dash.a">A<\/div>\n      <article/);
  applyEdit(root, { file: G, op: 'move', id: 'dash.b', ref: 'dash.c', position: 'before' });
  assert.match(readG(root), /cards">\n      <article data-dfw-id="dash.b">\n        <h2 data-dfw-id="dash.b.title">B<\/h2>\n      <\/article>\n      <dfw-use c="button" id="dash.c">C<\/dfw-use>\n      <div data-dfw-id="dash.a">A<\/div>\n    <\/section>/);
  // Back to the original order: the file is byte-identical.
  applyEdit(root, { file: G, op: 'move', id: 'dash.a', ref: 'dash.b', position: 'before' });
  applyEdit(root, { file: G, op: 'move', id: 'dash.c', ref: 'dash.b', position: 'after' });
  assert.equal(readG(root), GRID);
  // Moving into another container re-indents to the new place.
  applyEdit(root, { file: G, op: 'move', id: 'dash.b', ref: 'dash.row', position: 'after' });
  assert.match(readG(root), /<\/div>\n    <article data-dfw-id="dash.b">\n      <h2 data-dfw-id="dash.b.title">B<\/h2>\n    <\/article>\n  <\/main>/);
});

test('move: inline siblings, no-op, and refusals', () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'move', id: 'dash.y', ref: 'dash.x', position: 'before' });
  assert.match(readG(root), /<span data-dfw-id="dash.y">y<\/span><span data-dfw-id="dash.x">x<\/span>/);
  const same = applyEdit(root, { file: G, op: 'move', id: 'dash.y', ref: 'dash.x', position: 'before' });
  assert.equal(same.changed, false, 'already there: idempotent');
  const before = readG(root);
  assert.throws(() => applyEdit(root, { file: G, op: 'move', id: 'dash.b', ref: 'dash.b.title', position: 'after' }), /into itself/);
  assert.throws(() => applyEdit(root, { file: G, op: 'move', id: 'dash.b.title', ref: 'dash.b', position: 'after' }), /parent/);
  assert.throws(() => applyEdit(root, { file: G, op: 'move', id: 'dash.a', ref: 'nope', position: 'after' }), /not found/);
  assert.throws(() => applyEdit(root, { file: G, op: 'move', id: 'dash.a', ref: 'dash.b', position: 'over' }), /before.*after.*inside/);
  assert.equal(readG(root), before);
});

test('layout: grid-column span rule in a dfw layout block; update, media placement, removal', () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 2' });
  let t = readG(root);
  assert.match(t, / {2}<style data-dfw-layout>\n {4}\/\* dfw Studio layout edits[^\n]*\*\/\n {4}\[data-dfw-id='dash.b'\]:not\(#dfw\) \{ grid-column: span 2; \}\n {2}<\/style>\n<\/head>/);
  assert.equal(t.replace(/ {2}<style data-dfw-layout>[\s\S]*?<\/style>\n/, ''), GRID, 'nothing else changed');
  // Same value again: no change (idempotent).
  assert.equal(applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 2' }).changed, false);
  applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: '3' });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.c', prop: 'grid-column', value: 'span 1', maxWidth: 834 });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 1', maxWidth: 390 });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 2', maxWidth: 834 });
  t = readG(root);
  const block = /<style data-dfw-layout>([\s\S]*?)<\/style>/.exec(t)[1];
  assert.equal(
    block.replace(/\/\*[^*]*\*\//, '/**/'),
    `
    /**/
    [data-dfw-id='dash.b']:not(#dfw) { grid-column: span 3; }
    @media (max-width: 834px) {
      [data-dfw-id='dash.c']:not(#dfw) { grid-column: span 1; }
      [data-dfw-id='dash.b']:not(#dfw) { grid-column: span 2; }
    }
    @media (max-width: 390px) {
      [data-dfw-id='dash.b']:not(#dfw) { grid-column: span 1; }
    }
  `,
    'base first, then media blocks widest first so narrower ones win',
  );
  // Removing every rule removes the block and restores the file.
  for (const [id, mw] of [['dash.b', 0], ['dash.c', 834], ['dash.b', 390], ['dash.b', 834]]) applyEdit(root, { file: G, op: 'layout', id, prop: 'grid-column', value: null, maxWidth: mw });
  assert.equal(readG(root), GRID);
});

test('layout: width / min-height snap to tokens or percentages; refusals; lint stays clean', async () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '50%' });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'min-height', value: 'space.4' });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.c', prop: 'width', value: '--size-sidebar' });
  const t = readG(root);
  assert.match(t, /\[data-dfw-id='dash.a'\]:not\(#dfw\) \{ width: 50%; min-height: var\(--space-4\); \}/);
  assert.match(t, /\[data-dfw-id='dash.c'\]:not\(#dfw\) \{ width: var\(--size-sidebar\); \}/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '37%' }), /snap/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '300px' }), /token/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'min-height', value: 'color.ink' }), /token/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'grid-column', value: 'span 99' }), /span N/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'color', value: 'x' }), /not resizable/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '50%', maxWidth: 'wide' }), /breakpoint/);
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'nope', prop: 'width', value: '50%' }), /not found/);
  const { lintScreen, lintHtml } = await import('../../core/lib/lint.mjs');
  const errors = [...lintHtml(`design/${G}`, t), ...lintScreen(root, `design/${G}`, t)].filter((i) => i.level === 'error');
  assert.deepEqual(errors, [], 'no MUST findings (screen-visual-style clean; the rule selector is not a duplicate data-dfw-id)');
  const { locateSource } = await import('../../core/lib/comments.mjs');
  assert.equal(locateSource(root, G, 'dash.a'), `design/${G}:${t.split('\n').findIndex((l) => l.includes('<div data-dfw-id="dash.a"')) + 1}`, 'comment source line points at the element, not its layout rule');
});

test('layout: refuses to rewrite a hand-edited layout block; undo restores layout edits', () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 2' });
  applyEdit(root, { file: G, op: 'move', id: 'dash.c', ref: 'dash.a', position: 'before' });
  undoEdit(root);
  undoEdit(root);
  assert.equal(readG(root), GRID);
  fs.writeFileSync(path.join(root, 'design', G), GRID.replace('</head>', '<style data-dfw-layout>.cards > * { width: 1px }</style></head>'));
  assert.throws(() => applyEdit(root, { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 2' }), /edited by hand/);
});

test('edits are refused once the design is approved and locked (G2) or in review-only mode', () => {
  const root = gridProject();
  const stateFile = path.join(root, '.dfw/state.json');
  fs.mkdirSync(path.dirname(stateFile), { recursive: true });
  fs.writeFileSync(stateFile, JSON.stringify({ schema: 1, mode: 'new-product', phase: 'stack', design: { version: 1, status: 'approved', locked: true, approved: [] } }));
  for (const op of [
    { file: G, op: 'move', id: 'dash.c', ref: 'dash.a', position: 'before' },
    { file: G, op: 'layout', id: 'dash.b', prop: 'grid-column', value: 'span 2' },
    { file: G, op: 'text', id: 'dash.a', value: 'Z' },
  ]) assert.throws(() => applyEdit(root, op), /approved and locked \(G2\)/);
  assert.throws(() => undoEdit(root), /locked/);
  fs.writeFileSync(stateFile, JSON.stringify({ schema: 1, mode: 'review-only', design: { version: 1, locked: false } }));
  assert.throws(() => applyEdit(root, { file: G, op: 'text', id: 'dash.a', value: 'Z' }), /review-only/);
  fs.writeFileSync(stateFile, JSON.stringify({ schema: 1, mode: 'new-product', design: { version: 2, locked: false } }));
  assert.equal(applyEdit(root, { file: G, op: 'text', id: 'dash.a', value: 'Z' }).changed, true, 'editable again after reopen');
  assert.equal(readG(root), GRID.replace('>A<', '>Z<'));
});

test('layout: rules outrank class-based screen rules via :not(#dfw); older unsuffixed blocks still parse', () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '50%' });
  const rule = /(\[data-dfw-id='dash.a'\][^{]*)\{/.exec(readG(root))[1].trim();
  assert.equal(rule, "[data-dfw-id='dash.a']:not(#dfw)", 'one id of specificity: (1,1,0) beats e.g. ".dash .card" (0,2,0) without !important');
  // A block written before the suffix existed is read and rewritten in the current form.
  fs.writeFileSync(path.join(root, 'design', G), GRID.replace('</head>', "  <style data-dfw-layout>\n    [data-dfw-id='dash.b'] { grid-column: span 2; }\n  </style>\n</head>"));
  applyEdit(root, { file: G, op: 'layout', id: 'dash.c', prop: 'grid-column', value: 'span 1' });
  assert.match(readG(root), /\[data-dfw-id='dash.b'\]:not\(#dfw\) \{ grid-column: span 2; \}\n {4}\[data-dfw-id='dash.c'\]:not\(#dfw\) \{ grid-column: span 1; \}/);
});

test('layout reset: value "auto" removes one property; no prop removes the rule at that scope only', () => {
  const root = gridProject();
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '50%' });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'min-height', value: 'space.4' });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: '25%', maxWidth: 390 });
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: 'width', value: 'auto' });
  assert.match(readG(root), /\[data-dfw-id='dash.a'\]:not\(#dfw\) \{ min-height: var\(--space-4\); \}\n {4}@media/);
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', maxWidth: 390 });
  const t = readG(root);
  assert.doesNotMatch(t, /@media/, 'empty media block removed');
  assert.match(t, /min-height: var\(--space-4\)/, 'base rule kept');
  assert.equal(applyEdit(root, { file: G, op: 'layout', id: 'dash.a', maxWidth: 390 }).changed, false, 'nothing to reset');
  applyEdit(root, { file: G, op: 'layout', id: 'dash.a', prop: null, value: null });
  assert.equal(readG(root), GRID, 'last rule removed: block removed, file restored');
});

test('move inside: drops into an empty container (own-line close tag, one-line empty, inline); refusals', () => {
  const root = gridProject();
  const withEmpty = GRID.replace(
    '    <div class="row"',
    '    <section class="cards" data-dfw-id="dash.empty">\n    </section>\n    <div class="cols" data-dfw-id="dash.cols"></div>\n    <div class="row"',
  );
  fs.writeFileSync(path.join(root, 'design', G), withEmpty);
  applyEdit(root, { file: G, op: 'move', id: 'dash.b', ref: 'dash.empty', position: 'inside' });
  assert.match(readG(root), /<section class="cards" data-dfw-id="dash.empty">\n      <article data-dfw-id="dash.b">\n        <h2 data-dfw-id="dash.b.title">B<\/h2>\n      <\/article>\n    <\/section>/);
  applyEdit(root, { file: G, op: 'move', id: 'dash.a', ref: 'dash.cols', position: 'inside' });
  assert.match(readG(root), /<div class="cols" data-dfw-id="dash.cols">\n      <div data-dfw-id="dash.a">A<\/div>\n    <\/div>/);
  undoEdit(root);
  undoEdit(root);
  assert.equal(readG(root), withEmpty, 'undo restores the source byte for byte');
  applyEdit(root, { file: G, op: 'move', id: 'dash.x', ref: 'dash.cols', position: 'inside' });
  assert.match(readG(root), /<div class="cols" data-dfw-id="dash.cols"><span data-dfw-id="dash.x">x<\/span><\/div>/, 'inline element into an inline container');
  assert.throws(() => applyEdit(root, { file: G, op: 'move', id: 'dash.cards', ref: 'dash.b.title', position: 'inside' }), /into itself/);
  assert.throws(() => applyEdit(root, { file: G, op: 'move', id: 'dash.a', ref: 'dash.c', position: 'inside' }), /component instance/);
});

/* ---------------------------------------------------------------- containers built by a component template */

const SHELL = `<dfw-component name="app-shell" description="App frame with a nav and a main slot">
<template><div class="shell" data-dfw-id="shell.frame"><nav data-dfw-id="shell.nav">Nav</nav><main class="shell-main" data-dfw-id="shell.main"><dfw-slot name="main"></dfw-slot></main></div></template>
</dfw-component>`;
const SLOTTED = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
</head>
<body>
  <dfw-use c="app-shell" id="orders.shell">
    <dfw-fill slot="main">
      <section data-dfw-id="orders.summary">Summary</section>
      <section data-dfw-id="orders.table">Table</section>
      <aside data-dfw-id="orders.help">Help</aside>
    </dfw-fill>
  </dfw-use>
</body>
</html>`;

test('slot content: screen elements reorder among slot siblings; master-only ids are refused with a hint to edit the master', async () => {
  const root = gridProject();
  fs.writeFileSync(path.join(root, 'design/system/components/app-shell.html'), SHELL);
  const S = 'screens/home/slotted.html';
  fs.writeFileSync(path.join(root, 'design', S), SLOTTED);
  const { composeHtml } = await import('../../core/lib/compose.mjs');
  assert.match(composeHtml(root, SLOTTED).html, /<main class="shell-main" data-dfw-id="shell.main">\s*<section data-dfw-id="orders.summary">/, 'slot children render inside the master-built container');
  applyEdit(root, { file: S, op: 'move', id: 'orders.help', ref: 'orders.summary', position: 'before' });
  assert.match(read(root, `design/${S}`), /<dfw-fill slot="main">\n      <aside data-dfw-id="orders.help">Help<\/aside>\n      <section data-dfw-id="orders.summary">/);
  applyEdit(root, { file: S, op: 'layout', id: 'orders.table', prop: 'min-height', value: 'space.4' });
  assert.match(read(root, `design/${S}`), /\[data-dfw-id='orders.table'\]:not\(#dfw\) \{ min-height: var\(--space-4\); \}/);
  const before = read(root, `design/${S}`);
  for (const op of [
    { op: 'move', id: 'shell.nav', ref: 'orders.summary', position: 'after' },
    { op: 'move', id: 'orders.table', ref: 'shell.nav', position: 'before' },
    { op: 'move', id: 'orders.table', ref: 'shell.main', position: 'inside' },
    { op: 'layout', id: 'shell.nav', prop: 'width', value: '25%' },
  ]) assert.throws(() => applyEdit(root, { file: S, ...op }), /part of the component master "app-shell" \(design\/system\/components\/app-shell\.html\).*Edit the master/);
  assert.throws(() => applyEdit(root, { file: S, op: 'move', id: 'orders.table', ref: 'nope', position: 'before' }), /was not found/);
  assert.equal(read(root, `design/${S}`), before, 'refusals change nothing');
});
