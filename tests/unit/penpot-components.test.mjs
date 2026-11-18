// Penpot bridge: edits on the "dfw components" page (main components = dfw masters) come back as
// comments on the master file and token suggestions; never as edits of design/.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildExport, diffChanges, importChanges, dimensionTokenIndex, masterSource, formatImportChanges } from '../../core/lib/penpot.mjs';
import { buildTokens } from '../../core/lib/tokens.mjs';
import { allComments } from '../../core/lib/comments.mjs';
import { listFiles, sha256 } from '../../core/lib/util.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIX = path.join(HERE, '..', 'fixtures', 'penpot');
const read = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));
const base = read('project/design/tokens/tokens.json');
const dims = dimensionTokenIndex(base);

function project(state = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-cmp-'));
  fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ design: { version: 1, status: 'draft', locked: false }, ...state }));
  buildTokens(root);
  return root;
}

function fixtureExport() {
  return buildExport({ renders: read('renders.json'), tokens: { base, themes: {} }, product: 'Fixture', generatedAt: '2026-11-16T10:00:00.000Z' });
}

function withExport(root) {
  const doc = fixtureExport();
  const dir = path.join(root, '.dfw', 'penpot');
  fs.mkdirSync(path.join(dir, 'exports'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'exports', `${doc.id}.json`), JSON.stringify(doc));
  fs.writeFileSync(path.join(dir, 'export.json'), JSON.stringify(doc));
  return { doc, changes: { ...read('changes-components.json'), exportId: doc.id } };
}

const designHash = (root) => listFiles(path.join(root, 'design')).map((f) => `${f}:${sha256(fs.readFileSync(path.join(root, 'design', f)))}`).join('\n');

test('penpot components: dimension tokens by role (radius, space), font sizes left out', () => {
  assert.deepEqual(dims.map((d) => `${d.role}:${d.name}=${d.px}`).sort(), ['radius:radius.md=6', 'space:space.2=8', 'space:space.4=16']);
});

test('penpot components: main component edits map to the master, variant and element, with tokens', () => {
  const doc = fixtureExport();
  const { changes, warnings } = diffChanges(doc, { ...read('changes-components.json'), exportId: doc.id }, { dims });
  assert.deepEqual(warnings, []);
  const brief = changes.map((c) => `${c.type}:${c.component?.key}:${c.path ?? '-'}`).sort();
  assert.deepEqual(
    brief,
    [
      'added:button|primary|:0.1',
      'comment:button|primary|:-',
      'fill:button|primary|:0.1',
      'radius:button|primary|:0.1',
      'rename:button|ghost|:0.2',
      'rename:button|ghost|:0.2.0',
      'size:button|primary|:0.1',
      'spacing:button|ghost|:0.2',
      'spacing:button|primary|:0.1',
      'stroke:button|ghost|:0.2',
      'text:button|primary|:0.1.0',
    ].sort(),
  );
  const pick = (type, key) => changes.find((c) => c.type === type && c.component.key === key);
  assert.deepEqual(pick('fill', 'button|primary|').component, { key: 'button|primary|', name: 'button', variant: 'primary', size: null, title: 'button / primary' });
  assert.deepEqual([pick('fill', 'button|primary|').from.token, pick('fill', 'button|primary|').to.token], ['color.accent', 'color.ink']);
  assert.deepEqual([pick('stroke', 'button|ghost|').from.token, pick('stroke', 'button|ghost|').to.token], ['color.border', 'color.ink']);
  assert.deepEqual(pick('radius', 'button|primary|').to, { radius: [8, 8, 8, 8], token: null }); // space.2 is 8px but not a radius
  assert.deepEqual(pick('size', 'button|primary|').to, [97.14, 48]);
  assert.deepEqual(pick('spacing', 'button|primary|').sides, { top: { from: 11, to: 16, token: 'space.4' } });
  // Ghost has a 1px border: a 9px inset is space.2 + the border.
  assert.deepEqual(pick('spacing', 'button|ghost|').sides, { left: { from: 17, to: 9, token: 'space.2' } });
  assert.deepEqual([pick('text', 'button|primary|').from, pick('text', 'button|primary|').to], ['Add item', 'Add to stock']);
  const renames = changes.filter((c) => c.type === 'rename').map((c) => [c.isRoot, c.from, c.to]);
  assert.deepEqual(renames, [[true, 'button / ghost', 'button / outline'], [false, 'text', 'label']]);
  assert.equal(pick('added', 'button|primary|').layer, 'icon');
  // No screen board was touched: no screen changes.
  assert.ok(changes.every((c) => c.board === null && c.screen === null));
});

test('penpot components: removed layers and deleted main components', () => {
  const doc = fixtureExport();
  const ch = read('changes-components.json');
  ch.components = [{ ...ch.components[1], layers: [ch.components[1].layers[0]] }];
  ch.missingComponents = ['button|primary|'];
  const { changes } = diffChanges(doc, ch, { dims });
  assert.ok(changes.some((c) => c.type === 'removed' && c.path === '0.2.0' && c.text === 'Export'));
  assert.ok(changes.some((c) => c.type === 'component-removed' && c.component.key === 'button|primary|'));
  // Unknown keys are reported, not guessed.
  const { warnings } = diffChanges(doc, { ...ch, components: [{ key: 'card||', layers: [] }], missingComponents: [] });
  assert.match(warnings[0], /Component "card\|\|" is not in export/);
});

test('penpot components: source line = CSS rule of the variant, base rule, or template element', () => {
  const root = project();
  const c = { key: 'button|primary|', name: 'button', variant: 'primary', size: null, title: 'button / primary' };
  const src = (change) => masterSource(root, { component: c, ...change }).source;
  assert.equal(src({ type: 'fill', kind: 'frame', isRoot: true }), 'design/system/components/button.html:4'); // .btn--primary background
  assert.equal(src({ type: 'radius', kind: 'frame', isRoot: true }), 'design/system/components/button.html:3'); // .btn border-radius
  assert.equal(src({ type: 'spacing', kind: 'frame', isRoot: true }), 'design/system/components/button.html:3');
  assert.equal(src({ type: 'stroke', kind: 'frame', isRoot: true, component: { ...c, variant: 'ghost' } }), 'design/system/components/button.html:5'); // not the :focus-visible outline
  assert.equal(src({ type: 'text', kind: 'text', tag: 'span' }), 'design/system/components/button.html:9'); // <dfw-slot>
  assert.equal(src({ type: 'rename', kind: 'text', tag: 'span', name: 'text "Save"' }), 'design/system/components/button.html:9');
  assert.equal(masterSource(root, { component: { ...c, name: 'nope' }, type: 'fill' }).source, null);
});

test('penpot components: import writes master comments (target.component, source, note), never touches design/, idempotent', () => {
  const root = project();
  const { changes } = withExport(root);
  const before = designHash(root);
  const r = importChanges(root, changes, { state: { design: { version: 1, locked: false } } });
  assert.equal(designHash(root), before);
  const list = allComments(root);
  assert.equal(list.length, 5); // primary root, primary text, ghost root, ghost label, Penpot comment
  assert.ok(list.every((c) => c.provider === 'penpot' && c.file === 'design/system/components/button.html' && c.screen === null));
  assert.ok(list.every((c) => /Change the master design\/system\/components\/button\.html \(component "button", variant "(primary|ghost)"\): it affects every screen using it/.test(c.comment)));
  const root1 = list.find((c) => c.target.component?.variant === 'primary' && c.target.layer === '0.1');
  assert.deepEqual(root1.target.component, { name: 'button', variant: 'primary', size: null });
  assert.equal(root1.target.source, 'design/system/components/button.html:4');
  assert.match(root1.comment, /^\[Penpot master\] button \/ primary:/);
  assert.match(root1.comment, /Fill changed: #0066ff \(color\.accent\) -> #1c1b1a \(color\.ink\)/);
  assert.match(root1.comment, /Corner radius changed: 6px 6px 6px 6px \(radius\.md\) -> 8px 8px 8px 8px \(no radius token has this value/);
  assert.match(root1.comment, /Size changed: 97x40 -> 97x48 px/);
  assert.match(root1.comment, /Spacing \(padding\) changed: top 11px -> 16px \(space\.4\)/);
  assert.match(root1.comment, /Added in Penpot inside "button \/ primary": rect "icon"/);
  assert.match(root1.comment, /\[design\/system\/components\/button\.html:3\]/); // other rules than the target's
  const text = list.find((c) => c.target.layer === '0.1.0');
  assert.match(text.comment, /Example text changed: "Add item" -> "Add to stock"/);
  assert.equal(text.target.source, 'design/system/components/button.html:9');
  assert.equal(text.target.text, 'Add item');
  const ghost = list.find((c) => c.target.layer === '0.2');
  assert.match(ghost.comment, /Component renamed "button \/ ghost" -> "button \/ outline"/);
  assert.match(ghost.comment, /Border changed: 1px #e4e4e7 \(color\.border\) -> 1px #1c1b1a \(color\.ink\)/);
  assert.match(ghost.comment, /left 17px -> 9px \(space\.2\).*1px border/);
  assert.match(list.find((c) => c.comment.startsWith('[Penpot comment on master button / primary]')).comment, /Designer: Primary should feel heavier\./);
  assert.match(formatImportChanges(r, root), /1 component fill[\s\S]*master -> button\/primary/);
  // Re-sending the same changes adds nothing; a new edit on one element adds one comment.
  assert.equal(importChanges(root, changes, {}).comments.length, 0);
  assert.equal(allComments(root).length, 5);
  const more = structuredClone(changes);
  more.components[0].layers[1].characters = 'Add';
  const again = importChanges(root, more, {});
  assert.equal(again.comments.length, 1);
  assert.match(again.comments[0].comment, /"Add item" -> "Add"/);
});

test('penpot components: frozen design -> master comments say dfw reopen is needed', () => {
  const root = project({ design: { version: 2, status: 'approved', locked: true } });
  const { changes } = withExport(root);
  const r = importChanges(root, changes, { state: { design: { version: 2, locked: true } } });
  assert.ok(r.frozen);
  assert.equal(r.comments.length, 5);
  assert.ok(allComments(root).every((c) => /Change the master/.test(c.comment) || c.comment.startsWith('[Penpot comment')));
  assert.ok(allComments(root).every((c) => /frozen: discuss with the user; applying this needs "dfw reopen"/.test(c.comment)));
});
