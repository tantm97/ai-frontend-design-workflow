import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { execFileSync, spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { buildExport, exportStructure, exportPenpot, diffChanges, importChanges, tokensFromPlugin, simplify, signature } from '../../core/lib/penpot.mjs';
import { buildTokens } from '../../core/lib/tokens.mjs';
import { allComments } from '../../core/lib/comments.mjs';
import { listFiles, sha256 } from '../../core/lib/util.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const FIX = path.join(HERE, '..', 'fixtures', 'penpot');
const DFW = path.resolve(HERE, '../../core/bin/dfw.mjs');
const read = (f) => JSON.parse(fs.readFileSync(path.join(FIX, f), 'utf8'));
const hasChrome = (await import('../../core/lib/runtime.mjs')).browserStatus().installed;

function project(state = {}) {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-penpot-scr-'));
  fs.cpSync(path.join(FIX, 'project'), root, { recursive: true });
  fs.mkdirSync(path.join(root, '.dfw'), { recursive: true });
  fs.writeFileSync(path.join(root, '.dfw', 'state.json'), JSON.stringify({ design: { version: 1, status: 'draft', locked: false }, ...state }));
  buildTokens(root);
  return root;
}

function fixtureExport() {
  const base = read('project/design/tokens/tokens.json');
  return buildExport({ renders: read('renders.json'), tokens: { base, themes: {} }, product: 'Fixture', generatedAt: '2026-11-16T10:00:00.000Z' });
}

/** Put the fixture export in a project as if "dfw penpot export" had written it; returns the change file for it. */
function withExport(root) {
  const doc = fixtureExport();
  const dir = path.join(root, '.dfw', 'penpot');
  fs.mkdirSync(path.join(dir, 'exports'), { recursive: true });
  fs.writeFileSync(path.join(dir, 'exports', `${doc.id}.json`), JSON.stringify(doc));
  fs.writeFileSync(path.join(dir, 'export.json'), JSON.stringify(doc));
  return { doc, changes: { ...read('changes.json'), exportId: doc.id } };
}

const designHash = (root) => listFiles(path.join(root, 'design')).map((f) => `${f}:${sha256(fs.readFileSync(path.join(root, 'design', f)))}`).join('\n');

/* ------------------------------------------------------------ export */

test('penpot export: structure snapshot (boards per screen x breakpoint, ids as layer names, tokens, components)', () => {
  const doc = fixtureExport();
  assert.equal(doc.schema, 'dfw-penpot-export/1');
  assert.match(doc.id, /^px_20261116100000_[0-9a-f]{8}$/);
  assert.deepEqual(exportStructure(doc), fs.readFileSync(path.join(FIX, 'export.structure.txt'), 'utf8').trimEnd().split('\n'));
  // Tokens travel with the export in Penpot's format; colours keep their token names.
  assert.deepEqual(Object.keys(doc.tokens), ['base', '$themes', '$metadata']);
  assert.ok(doc.colors.some((c) => c.name === 'color.accent' && c.hex === '#0066ff'));
  // Instances share their master's structure, so the plugin can use component instances.
  const add = doc.pages[0].boards[0].layers[0].children.find((l) => l.name === 'items.add');
  assert.equal(add.sig, doc.components.find((c) => c.key === add.component.key).sig);
  assert.deepEqual([doc.components[0].root.x, doc.components[0].root.y], [0, 0]);
});

test('penpot export: layout wrappers are hoisted, signatures ignore text', () => {
  const tree = [{ kind: 'frame', name: 'div.wrap', style: {}, children: [{ kind: 'text', name: 'a', characters: 'x', style: {} }, { kind: 'frame', name: 'div.empty', style: {}, children: [] }] }];
  assert.deepEqual(simplify(tree).map((n) => n.name), ['a']);
  assert.equal(signature({ kind: 'frame', name: 'b', children: [{ kind: 'text', name: 'text', characters: 'one' }] }), signature({ kind: 'frame', name: 'b', children: [{ kind: 'text', name: 'text', characters: 'two' }] }));
});

test('penpot export: real render of the fixture project matches the snapshot (needs Chrome)', { skip: !hasChrome }, async () => {
  const root = project();
  const { file, doc } = await exportPenpot(root, { bp: 'desktop' });
  assert.deepEqual(doc.errors, []);
  assert.ok(fs.existsSync(path.join(root, '.dfw/penpot/exports', `${doc.id}.json`)));
  assert.ok(fs.existsSync(path.join(root, '.dfw/penpot/tokens.penpot.json')));
  assert.equal(file, path.join(root, '.dfw/penpot/export.json'));
  assert.deepEqual(exportStructure(doc), fs.readFileSync(path.join(FIX, 'export.structure.txt'), 'utf8').trimEnd().split('\n'));
  const both = await exportPenpot(root, {});
  assert.deepEqual(both.doc.pages[0].boards.map((b) => [b.key, b.x, b.y > 0]), [['items|a|desktop', 0, false], ['items|a|mobile', 0, true]]);
});

/* ------------------------------------------------------------ import */

test('penpot import: change set maps layers to data-dfw-id (text, move, fill, removed, added, comments)', () => {
  const doc = fixtureExport();
  const { changes, warnings } = diffChanges(doc, { ...read('changes.json'), exportId: doc.id });
  assert.deepEqual(warnings, []);
  const brief = changes.map((c) => `${c.type}:${c.anchor || '-'}`).sort();
  assert.deepEqual(brief, ['added:items.panel', 'comment:items.summary', 'comment:items.title', 'fill:items.panel', 'move:items.panel', 'removed:items.note', 'text:items.add'].sort());
  const text = changes.find((c) => c.type === 'text');
  assert.deepEqual([text.from, text.to, text.screen, text.variant, text.breakpoint, text.file], ['Add item', 'Add stock', 'items', 'a', 'desktop', 'design/screens/items/a.html']);
  const fill = changes.find((c) => c.type === 'fill');
  assert.deepEqual([fill.from.token, fill.to.token], ['color.surface', 'color.canvas']);
  assert.deepEqual(changes.find((c) => c.type === 'move').delta, { x: 0, y: 24, w: 0, h: 0 });
  assert.equal(changes.find((c) => c.type === 'added').characters, 'Last audit: 2 days ago');
  assert.deepEqual(changes.find((c) => c.type === 'comment' && !c.resolved).messages.map((m) => m.author), ['Designer', 'Lead']);
  assert.throws(() => diffChanges(doc, { schema: 'x' }), /Not a dfw Penpot change file/);
});

test('penpot import: writes feedback comments (provider penpot) and a token diff, never touches design/', () => {
  const root = project();
  const { changes } = withExport(root);
  const before = designHash(root);
  const r = importChanges(root, changes, { state: JSON.parse(fs.readFileSync(path.join(root, '.dfw/state.json'), 'utf8')) });
  assert.equal(designHash(root), before);
  const list = allComments(root);
  assert.ok(list.length >= 5);
  assert.ok(list.every((c) => c.provider === 'penpot' && c.status === 'open'));
  const add = list.find((c) => c.target.dfwId === 'items.add');
  assert.match(add.comment, /Text changed: "Add item" -> "Add stock"/);
  assert.equal(add.file, 'design/screens/items/a.html');
  assert.equal(add.breakpoint, 'desktop');
  assert.equal(add.target.selector, '[data-dfw-id="items.add"]');
  assert.match(add.target.source, /design\/screens\/items\/a\.html:\d+/);
  const panel = list.find((c) => c.target.dfwId === 'items.panel');
  assert.match(panel.comment, /Moved by 0, 24 px/);
  assert.match(panel.comment, /Colour changed: #ffffff \(color\.surface\) -> #f7f7f8 \(color\.canvas\)/);
  assert.match(panel.comment, /Added in Penpot: text "Last audit"/);
  assert.match(list.find((c) => c.target.dfwId === 'items.note').comment, /Removed in Penpot/);
  assert.match(list.find((c) => c.comment.startsWith('[Penpot comment]')).comment, /Designer: Can low stock be more visible\?\nLead: Maybe a badge\./);
  assert.ok(!list.some((c) => /Old note/.test(c.comment)), 'resolved Penpot threads are not imported');
  const tok = list.find((c) => /Token values changed/.test(c.comment));
  assert.match(tok.comment, /base color\.blue\.600/);
  assert.equal(r.tokenDiff.diff.base.changed.length, 1);
  assert.ok(fs.existsSync(path.join(root, '.dfw/penpot/tokens.from-penpot.json')));
  // Importing the same file again adds nothing.
  const again = importChanges(root, changes, {});
  assert.equal(again.comments.length, 0);
  assert.equal(allComments(root).length, list.length);
});

test('penpot import: frozen design -> comments only, with the gate noted; dry run writes nothing', () => {
  const root = project({ design: { version: 2, status: 'approved', locked: true } });
  const { changes } = withExport(root);
  const dry = importChanges(root, changes, { dryRun: true, state: { design: { version: 2, locked: true } } });
  assert.ok(dry.comments.length > 0);
  assert.equal(allComments(root).length, 0);
  assert.ok(!fs.existsSync(path.join(root, '.dfw/penpot/tokens.from-penpot.json')));
  const r = importChanges(root, changes, { state: { design: { version: 2, locked: true } } });
  assert.ok(r.frozen);
  assert.ok(allComments(root).every((c) => /frozen: discuss with the user; applying this needs "dfw reopen"/.test(c.comment)));
});

test('penpot import: plugin token sets become a Penpot token file', () => {
  const doc = tokensFromPlugin(read('changes.json').tokens);
  assert.deepEqual(doc.base.color.accent, { $value: '{color.blue.600}', $type: 'color' });
  assert.deepEqual(doc.$metadata.tokenSetOrder, ['base']);
  assert.deepEqual(doc.$themes[0].selectedTokenSets, { base: 'enabled' });
  assert.equal(tokensFromPlugin(null), null);
});

test('penpot CLI: import, plugin instructions', () => {
  const root = project();
  const { changes } = withExport(root);
  const f = path.join(root, 'changes.json');
  fs.writeFileSync(f, JSON.stringify(changes));
  const run = (...args) => execFileSync(process.execPath, [DFW, ...args], { cwd: root, env: { ...process.env, DFW_HOME: path.join(root, '.home') }, encoding: 'utf8' });
  assert.match(run('penpot', 'import', f, '--dry-run'), /1 text[\s\S]*Dry run/);
  assert.match(run('penpot', 'import', f), /Comments are in the review loop/);
  assert.match(run('penpot', 'plugin'), /__dfw\/penpot\/manifest\.json[\s\S]*No Penpot API token/);
});

test('penpot studio routes: manifest/plugin over CORS (private network), writes only from the Studio origin', async () => {
  const root = project();
  const { changes } = withExport(root);
  const port = 47000 + Math.floor(Math.random() * 1000);
  const child = spawn(process.execPath, [DFW, '__studio-server', '--root', root, '--port', String(port)], { stdio: 'ignore', env: { ...process.env, DFW_HOME: path.join(root, '.home') } });
  const base = `http://127.0.0.1:${port}`;
  try {
    for (let i = 0; i < 50; i++) {
      try {
        await fetch(`${base}/__dfw/api/manifest`);
        break;
      } catch {
        await new Promise((r) => setTimeout(r, 100));
      }
    }
    const m = await fetch(`${base}/__dfw/penpot/manifest.json`);
    assert.equal(m.headers.get('access-control-allow-origin'), '*');
    const manifest = await m.json();
    assert.deepEqual([manifest.code, manifest.version], ['plugin.js', 3]);
    assert.ok(manifest.permissions.includes('content:write') && manifest.permissions.includes('comment:write'));
    const pre = await fetch(`${base}/__dfw/penpot/plugin.js`, { method: 'OPTIONS' });
    assert.equal(pre.headers.get('access-control-allow-private-network'), 'true');
    const code = await (await fetch(`${base}/__dfw/penpot/plugin.js`)).text();
    assert.ok(code.includes(`'${base}'`) && !code.includes('__DFW_STUDIO_ORIGIN__'));
    assert.match(await (await fetch(`${base}/__dfw/penpot/ui.html`)).text(), /dfw bridge/);
    assert.equal((await (await fetch(`${base}/__dfw/penpot/export.json`)).json()).schema, 'dfw-penpot-export/1');
    const foreign = await fetch(`${base}/__dfw/penpot/changes`, { method: 'POST', headers: { origin: 'https://design.penpot.app', 'content-type': 'application/json' }, body: JSON.stringify(changes) });
    assert.equal(foreign.status, 403);
    const ok = await fetch(`${base}/__dfw/penpot/changes`, { method: 'POST', headers: { origin: base, 'content-type': 'application/json' }, body: JSON.stringify(changes) });
    const res = await ok.json();
    assert.equal(ok.status, 200);
    assert.ok(res.comments.length > 0);
  } finally {
    child.kill();
  }
});
