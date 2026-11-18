// Penpot bridge: files that come back from Penpot are untrusted input, and a second "Send changes"
// must not duplicate or lose anything.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { buildExport, importChanges, tokensFromPlugin, pruneExports } from '../../core/lib/penpot.mjs';
import { importPenpotTokens, parsePenpotJson, mapSets, mergeSet, dfwToPenpot, formatImport } from '../../core/lib/penpot-tokens.mjs';
import { buildTokens } from '../../core/lib/tokens.mjs';
import { allComments } from '../../core/lib/comments.mjs';

const FIX = path.join(path.dirname(fileURLToPath(import.meta.url)), '..', 'fixtures', 'penpot');
const read = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));

/** The fixture project with its export on disk, and the change file built from it. */
function project() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-hard-'));
  fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ design: { version: 1, status: 'draft', locked: false } }));
  buildTokens(root);
  const doc = buildExport({ renders: read('renders.json'), tokens: { base: read('project/design/tokens/tokens.json'), themes: {} }, product: 'Fixture', generatedAt: '2026-11-16T10:00:00.000Z' });
  const dir = path.join(root, '.dfw', 'penpot');
  fs.mkdirSync(path.join(dir, 'exports'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'exports', `${doc.id}.json`), JSON.stringify(doc));
  fs.writeFileSync(path.join(dir, 'export.json'), JSON.stringify(doc));
  return { root, doc, dir, changes: { ...read('changes.json'), exportId: doc.id } };
}

test('penpot tokens: "__proto__" in a token or set name never reaches Object.prototype', () => {
  try {
    const doc = tokensFromPlugin({ sets: [{ name: 'base', tokens: [{ name: '__proto__.polluted', type: 'color', value: '#fff' }, { name: 'color.ok', type: 'color', value: '#000' }] }, { name: '__proto__', tokens: [] }] });
    assert.equal({}.polluted, undefined);
    assert.deepEqual(doc.base, { color: { ok: { $value: '#000', $type: 'color' } } });
    assert.deepEqual(doc.$metadata.tokenSetOrder, ['base']);

    const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-proto-'));
    fs.mkdirSync(path.join(root, 'design', 'tokens'), { recursive: true });
    fs.writeFileSync(path.join(root, 'design/tokens/tokens.json'), JSON.stringify({ color: { $type: 'color', a: { $value: '#ffffff' } } }));
    const file = path.join(root, 'in.json');
    // Raw text: JSON.parse makes "__proto__" an own key, which is what a hostile file relies on.
    fs.writeFileSync(file, '{"__proto__":{"zzz":{"$value":"#000000","$type":"color"}},"color":{"a":{"$value":"#ffffff","$type":"color"},"constructor":{"prototype":{"yyy":{"$value":"#111111","$type":"color"}}}}}');
    const r = importPenpotTokens(root, file, {});
    assert.equal({}.zzz, undefined);
    assert.equal(Object.prototype.yyy, undefined);
    assert.deepEqual(r.diff.in.added, []);
    assert.deepEqual(r.unsupported.map((u) => u.token).sort(), ['__proto__', 'color.constructor']);
    assert.match(r.unsupported[0].reason, /reserved name/);
  } finally {
    delete Object.prototype.polluted;
    delete Object.prototype.zzz;
    delete Object.prototype.yyy;
  }
});

test('penpot tokens: a file holding only "themes/dark" is a theme, never the base', () => {
  const only = { 'themes/dark': { color: { a: { $value: '#000000', $type: 'color' } } }, $themes: [], $metadata: {} };
  const m = mapSets(parsePenpotJson(only));
  assert.equal(m.baseName, undefined);
  assert.deepEqual(m.map.get('themes/dark'), { kind: 'theme', name: 'dark' });
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-theme-'));
  fs.mkdirSync(path.join(root, 'design', 'tokens'), { recursive: true });
  const base = JSON.stringify({ color: { $type: 'color', a: { $value: '#ffffff' } } });
  fs.writeFileSync(path.join(root, 'design/tokens/tokens.json'), base);
  fs.writeFileSync(path.join(root, 'dark.json'), JSON.stringify(only));
  assert.throws(() => importPenpotTokens(root, path.join(root, 'dark.json'), {}), /No base token set/);
  assert.equal(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'), base, 'tokens.json untouched');
  // One set with another name in a multi-set file is still the base (as before).
  assert.equal(mapSets(parsePenpotJson({ global: {}, $themes: [] })).baseName, 'global');
});

test('penpot tokens: a description edited in Penpot keeps the exact dfw value (oklch, seconds)', () => {
  const cur = {
    color: { $type: 'color', brand: { $value: { colorSpace: 'oklch', components: [0.6, 0.15, 250] }, $description: 'Brand' } },
    motion: { $type: 'duration', fast: { $value: { value: 0.2, unit: 's' } } },
  };
  const pen = dfwToPenpot({ base: cur }).json.base;
  pen.color.brand.$description = 'Brand colour';
  pen.motion.fast.$description = 'Quick';
  const m = mergeSet(cur, pen, { set: 'base' });
  assert.deepEqual(m.tree.color.brand, { $value: { colorSpace: 'oklch', components: [0.6, 0.15, 250] }, $description: 'Brand colour' });
  assert.deepEqual(m.tree.motion.fast, { $value: { value: 0.2, unit: 's' }, $description: 'Quick' });
  assert.deepEqual(m.diff.changed.map((c) => [c.token, c.descriptionOnly]), [['color.brand', true], ['motion.fast', true]]);
  assert.match(formatImport({ diff: { base: m.diff }, skippedSets: [], unsupported: [], warnings: [], written: [] }), /~ color\.brand: description changed \(value kept\)/);
  // Description cleared in Penpot: removed, value still exact.
  delete pen.color.brand.$description;
  assert.deepEqual(mergeSet(cur, pen, { set: 'base' }).tree.color.brand, { $value: { colorSpace: 'oklch', components: [0.6, 0.15, 250] } });
});

test('penpot import: a comment thread is imported once, even after the layer under it moved', () => {
  const { root, changes } = project();
  importChanges(root, changes, {});
  const threads = () => allComments(root).filter((c) => c.comment.startsWith('[Penpot comment]')).length;
  assert.equal(threads(), 1);
  const again = structuredClone(changes);
  for (const b of again.boards) for (const l of b.layers) l.x += 3;
  for (const t of again.comments) t.x += 3;
  importChanges(root, again, {});
  assert.equal(threads(), 1, 'same thread, same messages: not a new comment');
  // A reply in the thread is news.
  again.comments[0].comments.push({ content: 'Agreed.', user: 'Designer' });
  importChanges(root, again, {});
  assert.equal(threads(), 2);
});

test('penpot import: exportId cannot name a file outside .dfw/penpot/exports', () => {
  const { root, doc, dir, changes } = project();
  const outside = path.join(root, 'outside.json');
  fs.writeFileSync(outside, JSON.stringify({ ...doc, id: 'evil' }));
  const r = importChanges(root, { ...changes, exportId: path.relative(path.join(dir, 'exports'), outside).replace(/\.json$/, '') }, { dryRun: true });
  assert.equal(r.baseline, doc.id, 'falls back to the latest export');
});

test('penpot import: a non-string pushedAt does not fail after the comments were written', () => {
  const { root, dir, changes } = project();
  const r = importChanges(root, { ...changes, pushedAt: 1731750000000 }, {});
  assert.ok(r.comments.length >= 5);
  assert.deepEqual(fs.readdirSync(path.join(dir, 'changes')), ['1731750000000.json']);
  const r2 = importChanges(root, { ...changes, pushedAt: '../../x' }, {});
  assert.equal(r2.comments.length, 0);
  assert.ok(fs.readdirSync(path.join(dir, 'changes')).every((f) => /^[\dTZ]+\.json$|^changes\.json$/.test(f)), 'archive names stay inside changes/');
});

test('penpot export: only the newest exports are kept', () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-prune-'));
  fs.mkdirSync(path.join(dir, 'exports'));
  const ids = Array.from({ length: 25 }, (_, i) => `px_202611${String(i + 1).padStart(2, '0')}100000_abcdef12`);
  for (const id of ids) fs.writeFileSync(path.join(dir, 'exports', `${id}.json`), '{}');
  fs.writeFileSync(path.join(dir, 'exports', 'notes.txt'), 'mine');
  const removed = pruneExports(dir);
  assert.equal(removed.length, 5);
  assert.deepEqual(fs.readdirSync(path.join(dir, 'exports')).sort(), ['notes.txt', ...ids.slice(5).map((id) => `${id}.json`)].sort());
  assert.deepEqual(pruneExports(path.join(dir, 'nope')), []);
});
