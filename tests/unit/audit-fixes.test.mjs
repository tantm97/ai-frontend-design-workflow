// Regression tests for the code-audit findings (one per fix; each fails without its fix).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

import { composeHtml } from '../../core/lib/compose.mjs';
import { applyEdit, undoEdit, resolveDesignFile, MAX_JOURNAL } from '../../core/lib/edit.mjs';
import { runChecks } from '../../core/lib/check.mjs';
import { loadConfig } from '../../core/lib/config.mjs';
import { systemInit, systemOptions } from '../../core/lib/system.mjs';
import { listStyles } from '../../core/lib/style.mjs';
import { changedScreens, markChecked } from '../../core/lib/layout.mjs';
import { contrast } from '../../core/lib/color.mjs';
import { renderSystemPage } from '../../core/lib/system-page.mjs';
import { auditDesignSystem } from '../../core/lib/dsaudit.mjs';
import { lintDesign, lintScreen } from '../../core/lib/lint.mjs';
import { tokensToDeclarations } from '../../core/lib/tokens.mjs';

function project(files = {}) {
  const root = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-audit-')));
  const w = (rel, text) => {
    fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
    fs.writeFileSync(path.join(root, rel), text);
  };
  for (const [rel, text] of Object.entries(files)) w(rel, text);
  return { root, w, read: (rel) => fs.readFileSync(path.join(root, rel), 'utf8') };
}
const master = (name, tpl, attrs = '') => `<dfw-component name="${name}"${attrs}>\n<template>\n${tpl}\n</template>\n</dfw-component>\n`;

test('1: a tag inside an HTML comment does not end the element (move keeps the source intact)', () => {
  const a = '<div data-dfw-id="a">\n  <!-- </div> old wrapper -->\n  <p data-dfw-id="p">x</p>\n</div>';
  const b = '<div data-dfw-id="b">B</div>';
  const page = (x, y) => `<html><head></head><body>\n${x}\n${y}\n</body></html>\n`;
  const { root, read } = project({ 'design/screens/a/index.html': page(a, b) });
  applyEdit(root, { file: 'screens/a/index.html', op: 'move', id: 'a', ref: 'b', position: 'after' });
  assert.equal(read('design/screens/a/index.html'), page(b, a));
});

test('1: an unclosed <dfw-use> is reported and does not swallow the page; comments are not expanded', () => {
  const { root } = project({ 'design/system/components/divider.html': master('divider', '<hr class="divider">') });
  const unclosed = '<body>\n<dfw-use c="divider">\n<main data-dfw-id="main"><h1>Title</h1></main>\n</body>';
  const r = composeHtml(root, unclosed);
  assert.match(r.html, /<main data-dfw-id="main">/);
  assert.deepEqual(r.issues.map((i) => [i.rule, i.level, i.line]), [['component-unclosed', 'error', 2]]);
  assert.ok(lintScreen(root, 'design/screens/a/index.html', unclosed).some((f) => f.rule === 'component-unclosed'));

  const commented = '<body>\n<!-- TODO replace with <dfw-use c="divider"> later -->\n<main>x</main>\n</body>';
  const c = composeHtml(root, commented);
  assert.equal(c.html, commented);
  assert.deepEqual(c.issues, []);
});

test('2: lint targets written as ./design/..., or absolute, are linted (no false pass)', () => {
  const { root } = project({
    'design/design.json': '{"screens":[]}',
    'design/screens/a/index.html': '<html><head></head><body><img src="x.png"><h1>Lorem ipsum</h1></body></html>',
  });
  const rules = (target) => runChecks(root, [target], loadConfig(root), { tools: ['dfw'] }).flatMap((r) => r.findings).map((f) => f.rule);
  const want = rules('design/screens/a/index.html');
  assert.ok(want.includes('img-alt'));
  assert.deepEqual(rules('./design/screens/a/index.html'), want);
  assert.deepEqual(rules(path.join(root, 'design/screens/a/index.html')), want);
});

test('3: system init --style keeps existing tokens unless --force', () => {
  const style = listStyles()[0].id;
  const mine = '{"$description":"hand-tuned","color":{"$type":"color","ink":{"$value":"#123456"}}}';
  const { root, read } = project({ 'design/tokens/tokens.json': mine, 'design/system/head.html': '<!-- my fonts -->' });
  assert.throws(() => systemInit(root, { style }), /--force/);
  assert.equal(read('design/tokens/tokens.json'), mine);
  assert.equal(read('design/system/head.html'), '<!-- my fonts -->');
  assert.equal(systemInit(root, { style, force: true }).tokensWritten, true);
  assert.notEqual(read('design/tokens/tokens.json'), mine);
  // No tokens yet (or the dfw init placeholder): the style applies without --force.
  assert.equal(systemInit(project().root, { style }).tokensWritten, true);
});

test('4: a master used through another master, or the shared base.css, marks its screens as changed', () => {
  const button = (pad) => `<dfw-component name="button"><style>.btn{padding:${pad}px}</style><template><button class="btn"><dfw-slot></dfw-slot></button></template></dfw-component>`;
  const { root, w } = project({
    'design/design.json': JSON.stringify({ screens: [{ id: 'home', variants: [{ id: 'a', file: 'screens/home/index.html' }] }] }),
    'design/tokens/tokens.json': '{}',
    'design/system/components/card.html': '<dfw-component name="card"><template><div class="card"><dfw-use c="button">Go</dfw-use></div></template></dfw-component>',
    'design/system/components/button.html': button(4),
    'design/styles/base.css': 'body{margin:0}',
    'design/screens/home/index.html': '<html><body><dfw-use c="card"></dfw-use></body></html>',
  });
  markChecked(root);
  assert.deepEqual(changedScreens(root).screens, []);
  w('design/system/components/button.html', button(400));
  assert.deepEqual(changedScreens(root).screens, ['home'], 'home uses button through card');
  markChecked(root);
  assert.deepEqual(changedScreens(root).screens, []);
  w('design/styles/base.css', 'body{margin:0;width:3000px}');
  assert.deepEqual(changedScreens(root).screens, ['home'], 'base.css is loaded by every screen');
});

test('5: 3-digit hex tokens are audited and do not crash the Design System page', () => {
  assert.equal(contrast('#ccc', '#fff'), contrast('#cccccc', '#ffffff'));
  const tokens = (c) => JSON.stringify({ color: { $type: 'color', ink: { $value: c.ink }, surface: { $value: c.bg }, canvas: { $value: c.bg }, accent: { $value: c.accent }, muted: { $value: c.muted } } });
  const count = (c) => auditDesignSystem(project({ 'design/tokens/tokens.json': tokens(c) }).root).filter((f) => f.rule === 'contrast').length;
  const short = { ink: '#ccc', bg: '#fff', accent: '#ddd', muted: '#eee' };
  const long = { ink: '#cccccc', bg: '#ffffff', accent: '#dddddd', muted: '#eeeeee' };
  assert.ok(count(long) > 0);
  assert.equal(count(short), count(long));
  const { root } = project({ 'design/tokens/tokens.json': tokens(short) });
  assert.match(renderSystemPage(root), /1\.6:1/);
});

test('6: a theme may alias tokens of the base file; unknown aliases are still errors', () => {
  const { root, w } = project({
    'design/design.json': '{"screens":[]}',
    'design/tokens/tokens.json': JSON.stringify({ color: { $type: 'color', palette: { n900: { $value: '#111111' }, n50: { $value: '#fafafa' } }, ink: { $value: '{color.palette.n900}' } } }),
    'design/tokens/themes/dark.json': JSON.stringify({ color: { $type: 'color', ink: { $value: '{color.palette.n50}' } } }),
  });
  const aliasErrors = () => lintDesign(root).filter((f) => f.rule === 'token-alias').map((f) => f.file);
  assert.deepEqual(aliasErrors(), []);
  w('design/tokens/themes/dark.json', JSON.stringify({ color: { $type: 'color', ink: { $value: '{color.palette.nope}' } } }));
  assert.deepEqual(aliasErrors(), ['design/tokens/themes/dark.json']);
});

test('7: undo skips the edit of a deleted file, and the journal is capped', () => {
  const screen = (t) => `<html><head></head><body><p data-dfw-id="t">${t}</p></body></html>`;
  const { root, read } = project({ 'design/screens/c/index.html': screen('c'), 'design/screens/d/index.html': screen('d') });
  applyEdit(root, { file: 'screens/c/index.html', op: 'text', id: 't', value: 'c2' });
  applyEdit(root, { file: 'screens/d/index.html', op: 'text', id: 't', value: 'd2' });
  fs.rmSync(path.join(root, 'design/screens/d'), { recursive: true });
  assert.equal(undoEdit(root).file, 'design/screens/c/index.html');
  assert.equal(read('design/screens/c/index.html'), screen('c'));
  assert.throws(() => undoEdit(root), /Nothing to undo/);

  for (let i = 0; i < MAX_JOURNAL + 5; i++) applyEdit(root, { file: 'screens/c/index.html', op: 'text', id: 't', value: `v${i}` });
  assert.equal(fs.readdirSync(path.join(root, '.dfw/edits')).length, MAX_JOURNAL);
  undoEdit(root); // the newest records are the ones kept
  assert.equal(read('design/screens/c/index.html'), screen(`v${MAX_JOURNAL + 3}`));
});

test('8: data-dfw-if with single quotes is evaluated; a template may start with a comment', () => {
  const { root } = project({
    'design/system/components/c1.html': master('c1', `<div class="c1"><span data-dfw-if='a'>A</span><b data-dfw-if="b">B</b><i data-dfw-if=a>I</i></div>`, ' props="a b"'),
    'design/system/components/c2.html': master('c2', '<!-- root -->\n<div class="c2"><dfw-slot></dfw-slot></div>'),
  });
  assert.equal(composeHtml(root, '<dfw-use c="c1" a="1"></dfw-use>').html, '<div class="c1" data-dfw-component="c1"><span>A</span><i>I</i></div>');
  assert.equal(composeHtml(root, '<dfw-use c="c2" id="x.y" class="wide">hi</dfw-use>').html, '<div class="c2 wide" data-dfw-id="x.y" data-dfw-component="c2">hi</div>');
});

test('9: a spacing edit keeps the case of custom properties in the inline style', () => {
  const { root, read } = project({
    'design/tokens/tokens.json': JSON.stringify({ space: { $type: 'dimension', 4: { $value: { value: 16, unit: 'px' } } } }),
    'design/screens/c/index.html': '<html><head></head><body><div data-dfw-id="g" style="--colW: 3; GAP: 4px">x</div></body></html>',
  });
  applyEdit(root, { file: 'screens/c/index.html', op: 'spacing', id: 'g', prop: 'gap', value: 'space.4' });
  assert.match(read('design/screens/c/index.html'), /style="--colW: 3; gap: var\(--space-4\)"/);
});

test('10: the design/approved boundary holds for other letter case and for symlinks; missing files still get a clear error', (t) => {
  const frozen = '<html><body><p data-dfw-id="t">frozen</p></body></html>';
  const { root, read } = project({ 'design/approved/v1/index.html': frozen, 'design/screens/a/index.html': frozen, 'outside/x.html': frozen });
  assert.throws(() => resolveDesignFile(root, 'screens/nope/index.html'), /design\/screens\/nope\/index\.html does not exist/);
  assert.equal(resolveDesignFile(root, 'design/screens/a/index.html').rel, 'design/screens/a/index.html');
  try {
    fs.symlinkSync(path.join(root, 'outside'), path.join(root, 'design/link'));
    assert.throws(() => applyEdit(root, { file: 'link/x.html', op: 'text', id: 't', value: 'X' }), /limited to files under design/);
    fs.symlinkSync(path.join(root, 'design/approved'), path.join(root, 'design/alias'));
    assert.throws(() => applyEdit(root, { file: 'alias/v1/index.html', op: 'text', id: 't', value: 'X' }), /frozen/);
  } catch (e) {
    if (e.code !== 'EPERM') throw e; // Windows without the symlink privilege
  }
  if (fs.existsSync(path.join(root, 'design/APPROVED'))) {
    // Case-insensitive file system (macOS, Windows): "Approved/..." is the frozen file.
    assert.throws(() => applyEdit(root, { file: 'Approved/v1/index.html', op: 'text', id: 't', value: 'X' }), /frozen/);
  } else t.diagnostic('case-sensitive file system: letter-case check not exercised');
  assert.equal(read('design/approved/v1/index.html'), frozen);
  assert.equal(read('outside/x.html'), frozen);
});

test('tokens: a shadow without blur/spread prints valid CSS; a null $value is a clear error', () => {
  const px = (value) => ({ value, unit: 'px' });
  const decl = (shadow) => tokensToDeclarations({ shadow: { $type: 'shadow', sm: { $value: shadow } } })[0][1];
  assert.equal(decl({ offsetX: px(0), offsetY: px(1), blur: px(2), color: '#000000' }), '0px 1px 2px #000000');
  assert.equal(decl({ offsetX: px(0), offsetY: px(1), spread: px(3), color: '#000000' }), '0px 1px 0px 3px #000000');
  assert.throws(() => tokensToDeclarations({ space: { $type: 'dimension', a: { $value: null } } }), /Token "space\.a" has no value/);
});

test('system options: a mistyped name leaves the current options untouched', () => {
  const { root } = project({ 'design/design.json': '{"screens":[]}' });
  systemInit(root, { preset: 'crisp' });
  systemOptions(root, ['crisp', 'soft']);
  const themes = () => fs.readdirSync(path.join(root, 'design/tokens/themes')).sort();
  assert.deepEqual(themes(), ['opt-crisp.json', 'opt-soft.json']);
  assert.throws(() => systemOptions(root, ['industrial', 'sotf']), /Unknown preset "sotf"/);
  assert.deepEqual(themes(), ['opt-crisp.json', 'opt-soft.json']);
});

test('lint: a malformed design.json is a finding, not a crash', () => {
  const { root } = project({
    'design/design.json': '{"screens": [',
    'design/tokens/tokens.json': '{"color":{"$type":"color","ink":{"$value":"#111111"}}}',
    'design/screens/a/index.html': '<html lang="en"><head><dfw-head></dfw-head></head><body><main data-dfw-id="m">x</main></body></html>',
  });
  assert.ok(lintDesign(root).some((f) => f.rule === 'manifest-json'));
  assert.doesNotThrow(() => auditDesignSystem(root));
});
