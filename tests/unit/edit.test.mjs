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
