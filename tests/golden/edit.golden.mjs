// Edit group: Studio direct edits with the undo journal, the generated Design System page and the
// layout cache. The binary's hidden `__debug edit|undo|system-page|changed-screens|mark-checked`
// helpers run on a copy of a project; the result JSON (or the refusal) and every file under
// design/ and .dfw/ that a call changed must be the recorded ones, which are what the Node
// functions of 0.5.2 returned and wrote (where Node had a known defect, the binary's own result).
// The hostile sources (comments holding tags, quotes, unclosed tags, script and style, CRLF, BOM)
// run every op and undo back to the original bytes.
// Run: node --test tests/golden/
import { describe, after } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { spawnSync } from 'node:child_process';
import { nativeBinary, skipReason, REPO, NOW, suite, expected } from './harness.mjs';

const test = suite('edit');

const CORE = path.join(REPO, 'core');
/** Undo depth of the journal (MAX_JOURNAL of edit.mjs in 0.5.2). */
const MAX_JOURNAL = 200;

const FIXTURE = path.join(REPO, 'tests', 'fixtures', 'penpot', 'project');
const KIT = path.join(CORE, 'kits', 'base', 'components');
const TMP = fs.realpathSync(fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-golden-edit-')));
after(() => fs.rmSync(TMP, { recursive: true, force: true }));
/** False on a file system that ignores letter case (the default on macOS and Windows), else why to skip. */
const CASE_SENSITIVE = (() => {
  fs.writeFileSync(path.join(TMP, 'probe-Case'), '');
  return fs.existsSync(path.join(TMP, 'probe-case'))
    ? false
    : 'case-sensitive file system: the letter-case check of design/approved/ does not apply';
})();

const write = (root, rel, text) => {
  fs.mkdirSync(path.dirname(path.join(root, rel)), { recursive: true });
  fs.writeFileSync(path.join(root, rel), text);
};

let n = 0;
/** The files of each copy as last compared, so a recorded result holds only what a call changed. */
const seen = new WeakMap();
/** A copy of the fixture project for the binary (`roots.native`), prepared by setup. */
function pair(setup) {
  const roots = { native: path.join(TMP, `p${n++}`, 'native', 'project') };
  fs.cpSync(FIXTURE, roots.native, { recursive: true });
  setup?.(roots.native);
  seen.set(roots, tree(roots.native));
  return roots;
}

function native(args, stdin = '') {
  const env = { ...process.env, DFW_PLUGIN_ROOT: CORE, DFW_NOW: NOW };
  const r = spawnSync(nativeBinary(), ['__debug', ...args], {
    env,
    input: stdin,
    encoding: 'utf8',
    timeout: 180_000,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (r.error) throw r.error;
  return r;
}

/** A call as what a caller sees: the result as JSON, or the refusal. */
const viaNative = (args, stdin) => {
  const r = native(args, stdin);
  return r.status === 0 ? { ok: JSON.parse(r.stdout) } : { error: r.stderr.replace(/\n$/, '') };
};

/** Every file under design/ and .dfw/, journal records named by their position (ids differ per run). */
function tree(root) {
  const files = {};
  const walk = (dir) => {
    for (const ent of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => (a.name < b.name ? -1 : 1))) {
      const abs = path.join(dir, ent.name);
      if (ent.isDirectory()) walk(abs);
      else if (ent.isFile()) files[path.relative(root, abs).split(path.sep).join('/')] = fs.readFileSync(abs).toString('latin1');
    }
  };
  for (const top of ['.dfw', 'design']) if (fs.existsSync(path.join(root, top))) walk(path.join(root, top));
  const out = {};
  let rec = 0;
  for (const [rel, bytes] of Object.entries(files)) {
    if (!rel.startsWith('.dfw/edits/')) out[rel] = bytes;
    else {
      assert.match(rel, /^\.dfw\/edits\/\d{14}-\d{4}\.json$/, 'journal record name');
      out[`.dfw/edits/#${rec++}`] = bytes;
    }
  }
  return out;
}

/** The files that changed since the last call on this pair: new content, or null when deleted. */
function changes(roots, now) {
  const last = seen.get(roots);
  seen.set(roots, now);
  const out = {};
  for (const rel of [...new Set([...Object.keys(last), ...Object.keys(now)])].sort()) {
    if (last[rel] !== now[rel]) out[rel] = now[rel] ?? null;
  }
  return out;
}

/** The files a call changed, against the recorded ones. */
function sameChanges(roots, label) {
  const got = changes(roots, tree(roots.native));
  const want = expected(got);
  assert.deepEqual(Object.keys(got), Object.keys(want), `${label}: files written`);
  for (const rel of Object.keys(want))
    assert.ok(
      want[rel] === got[rel],
      `${label}: ${rel} differs\n  expected: ${JSON.stringify(want[rel]?.slice(0, 600))}\n  binary:   ${JSON.stringify(got[rel]?.slice(0, 600))}`,
    );
}
/** A result and the files the call changed, against the recorded ones. */
function sameCall(roots, result, label) {
  const files = changes(roots, tree(roots.native));
  const want = expected({ result, files });
  assert.deepEqual(result, want.result, label);
  assert.deepEqual(Object.keys(files), Object.keys(want.files), `${label}: files written`);
  for (const rel of Object.keys(want.files))
    assert.ok(
      want.files[rel] === files[rel],
      `${label}: ${rel} differs\n  expected: ${JSON.stringify(want.files[rel]?.slice(0, 600))}\n  binary:   ${JSON.stringify(files[rel]?.slice(0, 600))}`,
    );
}
/** One edit: the recorded result or refusal, the recorded files. Returns the result. */
function edit(roots, op) {
  const b = viaNative(['edit', roots.native], JSON.stringify(op));
  sameCall(roots, b, `edit ${JSON.stringify(op)}`);
  return b;
}
function undo(roots) {
  const b = viaNative(['undo', roots.native]);
  sameCall(roots, b, 'undo');
  return b;
}
const ok = (r) => {
  assert.ok(r.ok, `expected an edit, got: ${r.error}`);
  return r.ok;
};
const refused = (r, re) => {
  assert.ok(r.error !== undefined, `expected a refusal, got: ${JSON.stringify(r.ok)}`);
  assert.match(r.error, re);
};

const A = 'screens/items/a.html';
const LAB = 'screens/lab/a.html';
const LAB_HTML = `<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <style>.cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-4) }</style>
</head>
<body>
  <main data-dfw-id="lab" style="color: var(--color-ink); padding: 4px; --colW: 3; GAP: 2px">
    <section class="cards" data-dfw-id="lab.cards">
      <div data-dfw-id="lab.a">A</div>
      <article data-dfw-id="lab.b">
        <h2 data-dfw-id="lab.b.title">B</h2>
      </article>
      <dfw-use c="chip" id="lab.c" tone="warm">C</dfw-use>
    </section>
    <section class="cards" data-dfw-id="lab.empty">
    </section>
    <div class="cols" data-dfw-id="lab.cols"></div>
    <div class="row" data-dfw-id="lab.row"><span data-dfw-id="lab.x">x</span><span data-dfw-id="lab.y">y</span></div>
    <input data-dfw-id="lab.search">
    <dfw-use c="shell" id="lab.shell">
      <dfw-fill slot="main">
        <p data-dfw-id="lab.one">One</p>
        <p data-dfw-id="lab.two">Two</p>
      </dfw-fill>
    </dfw-use>
    <dfw-use c="nope" id="lab.unknown">?</dfw-use>
    <div data-dfw-id="dup">a</div><div data-dfw-id="dup">b</div>
  </main>
</body>
</html>
`;
const CHIP = `<dfw-component name="chip" variants="solid soft" sizes="sm md" props="tone icon=star" description="Chip. solid = strong; soft = quiet.">
<style>.chip { padding: var(--space-2); }</style>
<template><span class="chip chip--{{variant}}"><dfw-slot></dfw-slot></span></template>
</dfw-component>
`;
const SHELL = `<dfw-component name="shell" preview="frame" description="App frame.">
<template><div class="shell" data-dfw-id="shell.frame"><nav data-dfw-id="shell.nav">Nav</nav><main data-dfw-id="shell.main"><dfw-slot name="main"></dfw-slot></main></div></template>
</dfw-component>
`;
function lab(root) {
  write(root, `design/${LAB}`, LAB_HTML);
  write(root, 'design/system/components/chip.html', CHIP);
  write(root, 'design/system/components/shell.html', SHELL);
  const tokens = JSON.parse(fs.readFileSync(path.join(root, 'design/tokens/tokens.json'), 'utf8'));
  tokens.size = { $type: 'dimension', sidebar: { $value: { value: 240, unit: 'px' } } };
  write(root, 'design/tokens/tokens.json', JSON.stringify(tokens, null, 2));
}

describe('edit: every op', { skip: skipReason() }, () => {
  test('text: element text, instance slot text, white space kept, HTML escaped, values that are not strings', () => {
    const roots = pair(lab);
    assert.deepEqual(ok(edit(roots, { file: A, op: 'text', id: 'items.title', value: 'Stock <now> & "then"' })), {
      file: `design/${A}`,
      changed: true,
      line: 18,
    });
    ok(edit(roots, { file: `design/${A}`, op: 'text', id: 'items.add', value: 'New\n   item' }));
    ok(edit(roots, { file: `/design/${A}`, op: 'text', id: 'items.note', value: 42 }));
    ok(edit(roots, { file: A, op: 'text', id: 'items.note', value: true }));
    ok(edit(roots, { file: LAB, op: 'text', id: 'lab.b.title', value: ' Tiêu đề  ' }));
    ok(edit(roots, { file: LAB, op: 'text', id: 'lab.cols', value: 'filled' }));
    assert.equal(ok(edit(roots, { file: A, op: 'text', id: 'items.title', value: 'Stock <now> & "then"' })).changed, false);
    refused(edit(roots, { file: A, op: 'text', id: 'items.summary', value: 'x' }), /child elements/);
    refused(edit(roots, { file: LAB, op: 'text', id: 'lab.search', value: 'x' }), /no text content/);
    refused(edit(roots, { file: A, op: 'text', id: 'items.title', value: ' \n ' }), /cannot be empty/);
    refused(edit(roots, { file: A, op: 'text', id: 'items.title' }), /cannot be empty/);
    refused(edit(roots, { file: A, op: 'text', id: 'items.title', value: null }), /cannot be empty/);
  });

  test('variant, size and prop of a component instance', () => {
    const roots = pair(lab);
    ok(edit(roots, { file: A, op: 'variant', id: 'items.add', value: 'ghost' }));
    assert.equal(ok(edit(roots, { file: A, op: 'variant', id: 'items.add', value: 'ghost' })).changed, false);
    ok(edit(roots, { file: LAB, op: 'variant', id: 'lab.c', value: 'soft' }));
    ok(edit(roots, { file: LAB, op: 'size', id: 'lab.c', value: 'sm' }));
    ok(edit(roots, { file: LAB, op: 'prop', id: 'lab.c', prop: 'tone', value: 'cool "x" <y> & z' }));
    ok(edit(roots, { file: LAB, op: 'prop', id: 'lab.c', prop: 'icon', value: 7 }));
    ok(edit(roots, { file: LAB, op: 'prop', id: 'lab.c', prop: 'tone', value: '' }));
    ok(edit(roots, { file: LAB, op: 'prop', id: 'lab.c', prop: 'icon', value: null }));
    ok(edit(roots, { file: LAB, op: 'prop', id: 'lab.c', prop: 'icon' }));
    refused(edit(roots, { file: A, op: 'size', id: 'items.add', value: 'lg' }), /has no sizes/);
    refused(edit(roots, { file: A, op: 'variant', id: 'items.add', value: 'huge' }), /no variant "huge" \(primary, ghost\)/);
    refused(edit(roots, { file: A, op: 'variant', id: 'items.add', value: 3 }), /no variant "3"/);
    refused(edit(roots, { file: A, op: 'variant', id: 'items.add' }), /no variant "undefined"/);
    refused(edit(roots, { file: A, op: 'prop', id: 'items.add', prop: 'onclick', value: 'x' }), /no prop "onclick" \(none\)/);
    refused(edit(roots, { file: LAB, op: 'prop', id: 'lab.c', prop: 'href', value: 'x' }), /no prop "href" \(tone, icon\)/);
    refused(edit(roots, { file: A, op: 'variant', id: 'items.title', value: 'primary' }), /not a component instance/);
    refused(edit(roots, { file: A, op: 'prop', id: 'items.title', prop: 'x', value: 'y' }), /not a component instance/);
    refused(edit(roots, { file: LAB, op: 'variant', id: 'lab.unknown', value: 'x' }), /Unknown component "nope"/);
  });

  test('spacing: space tokens in the inline style, families replaced, custom-property case kept', () => {
    const roots = pair(lab);
    ok(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding', value: 'space.4' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'gap', value: '--space-2' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding-top', value: 'space.2' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding', value: 'space.2' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'gap', value: '' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding', value: null }));
    assert.equal(
      fs.readFileSync(path.join(roots.native, 'design', A), 'utf8'),
      fs.readFileSync(path.join(FIXTURE, 'design', A), 'utf8'),
      'style attribute removed again',
    );
    ok(edit(roots, { file: LAB, op: 'spacing', id: 'lab', prop: 'gap', value: 'space.4' }));
    assert.match(
      fs.readFileSync(path.join(roots.native, 'design', LAB), 'utf8'),
      /style="color: var\(--color-ink\); padding: 4px; --colW: 3; gap: var\(--space-4\)"/,
    );
    ok(edit(roots, { file: LAB, op: 'spacing', id: 'lab', prop: 'margin-inline', value: 'space.2' }));
    refused(
      edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding', value: 'color.ink' }),
      /"color\.ink" is not a space\.\* token/,
    );
    refused(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding', value: '--space-9' }), /not a space\.\* token/);
    refused(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'padding', value: 4 }), /"4" is not a space/);
    refused(edit(roots, { file: A, op: 'spacing', id: 'items.panel', prop: 'color', value: 'space.2' }), /not editable here/);
    refused(edit(roots, { file: A, op: 'spacing', id: 'items.panel', value: 'space.2' }), /"undefined" is not editable/);
    refused(edit(roots, { file: A, op: 'spacing', id: 'items.add', prop: 'padding', value: 'space.2' }), /component instance/);
  });

  test('move: before, after, inside; whole lines re-indent, inline stays inline', () => {
    const roots = pair(lab);
    ok(edit(roots, { file: A, op: 'move', id: 'items.note', ref: 'items.summary', position: 'before' }));
    ok(edit(roots, { file: A, op: 'move', id: 'items.export', ref: 'items.title', position: 'before' }));
    ok(edit(roots, { file: A, op: 'move', id: 'items.panel', ref: 'items.title', position: 'after' }));
    ok(edit(roots, { file: A, op: 'move', id: 'items.title', ref: 'items.note', position: 'after' }));
    ok(edit(roots, { file: A, op: 'move', id: 'items.add', ref: 'items.panel', position: 'inside' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.c', ref: 'lab.a', position: 'before' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.b', ref: 'lab.row', position: 'after' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.y', ref: 'lab.x', position: 'before' }));
    assert.equal(ok(edit(roots, { file: LAB, op: 'move', id: 'lab.y', ref: 'lab.x', position: 'before' })).changed, false);
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.b', ref: 'lab.empty', position: 'inside' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.a', ref: 'lab.cols', position: 'inside' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.x', ref: 'lab.cols', position: 'inside' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.two', ref: 'lab.one', position: 'before' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.y', ref: 'lab.b', position: 'after' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.a', ref: 'lab.y', position: 'before' }));
    ok(edit(roots, { file: LAB, op: 'move', id: 'lab.search', ref: 'lab.cards', position: 'inside' }));
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.cards', ref: 'lab.c', position: 'after' }), /into itself/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.b.title', ref: 'lab.b', position: 'after' }), /parent/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.cards', ref: 'lab.c', position: 'inside' }), /into itself/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'lab.c', position: 'inside' }), /component instance/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'lab.search', position: 'inside' }), /<input> cannot contain/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'lab.two', position: 'over' }), /"before", "after" or "inside"/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'lab.two' }), /"before", "after" or "inside"/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'lab.one', position: 'after' }), /needs another element/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', position: 'after' }), /needs another element/);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'nope', position: 'after' }), /"nope" was not found/);
  });

  test('layout: the data-dfw-layout block, @media scopes widest first, :not(#dfw), resets to auto', () => {
    const roots = pair(lab);
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: 'span 2' }));
    assert.match(
      fs.readFileSync(path.join(roots.native, 'design', A), 'utf8'),
      / {2}<style data-dfw-layout>\n {4}\/\* dfw Studio layout edits[^\n]*\*\/\n {4}\[data-dfw-id='items.panel'\]:not\(#dfw\) \{ grid-column: span 2; \}\n {2}<\/style>\n<\/head>/,
    );
    assert.equal(ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: 'span 2' })).changed, false);
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: '03' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title', prop: 'width', value: '33.333%' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title', prop: 'min-height', value: 'space.4' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.add', prop: 'width', value: ' --space-2 ' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.add', prop: 'grid-column', value: 'span 1', maxWidth: 834 }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: 'span 1', maxWidth: '390' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '100%', maxWidth: 834 }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%', maxWidth: 1200 }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title', prop: 'width', value: 'auto' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title', prop: 'min-height', value: '' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', maxWidth: 834 }));
    assert.equal(ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', maxWidth: 834 })).changed, false);
    ok(edit(roots, { file: A, op: 'layout', id: 'items.add', prop: 'grid-column', value: null, maxWidth: 834 }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: null, maxWidth: 1200 }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel', maxWidth: 390 }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.add', maxWidth: '' }));
    assert.equal(
      fs.readFileSync(path.join(roots.native, 'design', A), 'utf8'),
      fs.readFileSync(path.join(FIXTURE, 'design', A), 'utf8'),
      'last rule removed: the block is gone',
    );
    ok(edit(roots, { file: LAB, op: 'layout', id: 'lab.c', prop: 'width', value: 'size.sidebar' }));
    ok(edit(roots, { file: LAB, op: 'layout', id: 'lab.one', prop: 'min-height', value: '--size-sidebar', maxWidth: 0 }));
    refused(
      edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '37%' }),
      /snap to 25%, 33\.333%, 50%, 66\.667%, 75%, 100%/,
    );
    refused(
      edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '300px' }),
      /"300px" is not a size\.\* or space\.\* token/,
    );
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'min-height', value: 'color.ink' }), /token/);
    refused(
      edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: 'span 99' }),
      /"span N" with N from 1 to 24/,
    );
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: 'span 0' }), /span N/);
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: '2 / 4' }), /span N/);
    refused(
      edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'color', value: 'x' }),
      /"color" is not resizable here; only grid-column, width, min-height/,
    );
    refused(
      edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%', maxWidth: 'wide' }),
      /Invalid breakpoint width "wide"/,
    );
    refused(
      edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%', maxWidth: 12.5 }),
      /Invalid breakpoint width "12\.5"/,
    );
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%', maxWidth: -1 }), /Invalid breakpoint/);
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%', maxWidth: 10001 }), /Invalid breakpoint/);
  });

  test('layout: blocks written by hand: older rules without the suffix, double quotes, comments; unsafe forms refused', () => {
    const head = (block) => (root) =>
      write(root, `design/${A}`, fs.readFileSync(path.join(FIXTURE, 'design', A), 'utf8').replace('</head>', `${block}</head>`));
    let roots = pair(
      head(
        '  <style data-dfw-layout>\n    [data-dfw-id=\'items.panel\'] { grid-column: span 2; WIDTH: 50% }\n    /* note */ @media (max-width: 390px) { [data-dfw-id="items.add"]:not(#dfw){width:25%} }\n    @media (max-width:834px){}\n  </style>\n',
      ),
    );
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title', prop: 'grid-column', value: 'span 1' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.add', prop: 'width', value: '75%', maxWidth: 390 }));
    roots = pair(head("<style data-dfw-layout>[data-dfw-id='items.panel'] { width: 50%; }</style>"));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title', prop: 'grid-column', value: 'span 1' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.title' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'items.panel' }));
    roots = pair(head('<style data-dfw-layout>.cards > * { width: 1px }</style>'));
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%' }), /edited by hand/);
    roots = pair(head("<style data-dfw-layout>@media (max-width: 390px) { [data-dfw-id='a'] { width: 50%; } b { } }</style>"));
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%' }), /edited by hand/);
    roots = pair(head('<style data-dfw-layout></style>\n<style data-dfw-layout></style>'));
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%' }), /more than one/);
    roots = pair((root) => write(root, `design/${A}`, '<main data-dfw-id="m"><p data-dfw-id="p">x</p></main>\n'));
    refused(edit(roots, { file: A, op: 'layout', id: 'p', prop: 'width', value: '50%' }), /no <\/head>/);
    assert.equal(ok(edit(roots, { file: A, op: 'layout', id: 'p' })).changed, false, 'nothing to reset needs no head');
    roots = pair((root) => write(root, `design/${A}`, '<html><head><title>x</title></head><body><p data-dfw-id="p">x</p></body></html>'));
    ok(edit(roots, { file: A, op: 'layout', id: 'p', prop: 'width', value: '50%' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'p', prop: 'width', value: 'auto' }));
  });
});

describe('edit: refusals', { skip: skipReason() }, () => {
  const locked = {
    schema: 1,
    mode: 'new-product',
    phase: 'stack',
    active: true,
    design: { version: 3, status: 'approved', locked: true, approved: [] },
  };
  const state = (s) => (root) => write(root, '.dfw/state.json', JSON.stringify(s));
  const T = { file: A, op: 'text', id: 'items.title', value: 'X' };

  test('a frozen design, review-only mode; an inactive workflow edits', () => {
    let roots = pair(state(locked));
    refused(edit(roots, T), /Design v3 is approved and locked \(G2\)/);
    refused(edit(roots, { file: A, op: 'layout', id: 'items.panel', prop: 'width', value: '50%' }), /locked/);
    refused(undo(roots), /locked/);
    roots = pair(state({ schema: 1, mode: 'review-only', design: { version: 1, locked: false } }));
    refused(edit(roots, T), /review-only mode/);
    refused(undo(roots), /review-only/);
    roots = pair(state({ ...locked, active: false }));
    ok(edit(roots, T));
    roots = pair(state({ schema: 1, mode: 'new-product', design: { version: 2, locked: false } }));
    ok(edit(roots, T));
    ok(undo(roots));
  });

  test('the boundary: approved/, symlinks, outside design/, not HTML, missing', () => {
    const frozen = '<html><head></head><body><p data-dfw-id="t">frozen</p></body></html>';
    const roots = pair((root) => {
      write(root, 'design/approved/v1/index.html', frozen);
      write(root, 'outside/x.html', frozen);
      write(root, 'src/app.html', frozen);
      fs.symlinkSync(path.join(root, 'outside'), path.join(root, 'design', 'link'));
      fs.symlinkSync(path.join(root, 'design', 'approved'), path.join(root, 'design', 'alias'));
      fs.symlinkSync(path.join(root, 'design', 'screens'), path.join(root, 'design', 'inside'));
    });
    const op = (file) => ({ file, op: 'text', id: 't', value: 'X' });
    refused(edit(roots, op('approved/v1/index.html')), /design\/approved\/ is frozen/);
    refused(edit(roots, op('design/approved/v1/index.html')), /frozen/);
    refused(edit(roots, op('screens/../approved/v1/index.html')), /frozen/);
    refused(edit(roots, op('alias/v1/index.html')), /frozen/);
    refused(edit(roots, op('link/x.html')), /limited to files under design/);
    refused(edit(roots, op('../src/app.html')), /limited to files under design/);
    refused(edit(roots, op('../outside/x.html')), /limited to files under design/);
    refused(edit(roots, op('')), /No source file/);
    refused(edit(roots, { op: 'text', id: 't', value: 'X' }), /No source file/);
    refused(edit(roots, op(7)), /No source file/);
    refused(edit(roots, op('.')), /limited to files under design/);
    refused(edit(roots, op('tokens/tokens.json')), /Only HTML design files/);
    refused(edit(roots, op('screens/nope/index.html')), /design\/screens\/nope\/index\.html does not exist/);
    // A symlink that stays inside design/ edits the real file and reports its real path.
    assert.equal(ok(edit(roots, { file: 'inside/items/a.html', op: 'text', id: 'items.title', value: 'Via link' })).file, `design/${A}`);
    for (const root of Object.values(roots)) {
      assert.equal(fs.readFileSync(path.join(root, 'design/approved/v1/index.html'), 'utf8'), frozen);
      assert.equal(fs.readFileSync(path.join(root, 'outside/x.html'), 'utf8'), frozen);
      assert.equal(fs.readFileSync(path.join(root, 'src/app.html'), 'utf8'), frozen);
    }
  });

  test('the boundary: approved/ in other letter case', { skip: CASE_SENSITIVE }, () => {
    const frozen = '<html><head></head><body><p data-dfw-id="t">frozen</p></body></html>';
    const roots = pair((root) => write(root, 'design/approved/v1/index.html', frozen));
    refused(edit(roots, { file: 'Approved/v1/index.html', op: 'text', id: 't', value: 'X' }), /frozen/);
    refused(edit(roots, { file: 'design/APPROVED/v1/index.html', op: 'text', id: 't', value: 'X' }), /frozen/);
    for (const root of Object.values(roots))
      assert.equal(fs.readFileSync(path.join(root, 'design/approved/v1/index.html'), 'utf8'), frozen);
  });

  test('ids: only in a master, unknown, duplicate, missing, unsafe in a rule; an unknown op', () => {
    const roots = pair(lab);
    const master =
      /"shell\.nav" is part of the component master "shell" \(design\/system\/components\/shell\.html\), not of this screen.*Edit the master/;
    refused(edit(roots, { file: LAB, op: 'move', id: 'shell.nav', ref: 'lab.one', position: 'after' }), master);
    refused(edit(roots, { file: LAB, op: 'move', id: 'lab.one', ref: 'shell.nav', position: 'before' }), master);
    refused(edit(roots, { file: LAB, op: 'layout', id: 'shell.nav', prop: 'width', value: '25%' }), master);
    refused(edit(roots, { file: LAB, op: 'text', id: 'shell.nav', value: 'x' }), master);
    refused(edit(roots, { file: LAB, op: 'text', id: 'nope', value: 'x' }), /"nope" was not found in the source file/);
    refused(edit(roots, { file: LAB, op: 'text', id: 'dup', value: 'x' }), /"dup" appears 2 times/);
    refused(edit(roots, { file: LAB, op: 'text', value: 'x' }), /has no data-dfw-id/);
    refused(edit(roots, { file: LAB, op: 'text', id: '', value: 'x' }), /has no data-dfw-id/);
    refused(edit(roots, { file: LAB, op: 'text', id: null, value: 'x' }), /has no data-dfw-id/);
    refused(edit(roots, { file: LAB, op: 'text', id: 12, value: 'x' }), /"12" was not found/);
    refused(edit(roots, { file: LAB, op: 'paint', id: 'lab.a', value: 'x' }), /Unknown edit op "paint"/);
    refused(edit(roots, { file: LAB, id: 'lab.a' }), /Unknown edit op "undefined"/);
    const quoted = pair((root) => write(root, `design/${A}`, '<html><head></head><body><p data-dfw-id="it\'s">x</p></body></html>'));
    refused(edit(quoted, { file: A, op: 'layout', id: "it's", prop: 'width', value: '50%' }), /cannot be used in a layout rule/);
  });
});

describe('edit: journal and undo', { skip: skipReason() }, () => {
  test('5 edits then 5 undos return the file to its original bytes', () => {
    const roots = pair(lab);
    const before = { [A]: fs.readFileSync(path.join(FIXTURE, 'design', A)), [LAB]: Buffer.from(LAB_HTML) };
    const ops = [
      { file: A, op: 'text', id: 'items.title', value: 'One' },
      { file: A, op: 'move', id: 'items.note', ref: 'items.summary', position: 'before' },
      { file: A, op: 'layout', id: 'items.panel', prop: 'grid-column', value: 'span 2', maxWidth: 834 },
      { file: LAB, op: 'spacing', id: 'lab', prop: 'padding', value: 'space.2' },
      { file: A, op: 'variant', id: 'items.add', value: 'ghost' },
    ];
    for (const op of ops) ok(edit(roots, op));
    for (let i = ops.length - 1; i >= 0; i--) {
      const r = ok(undo(roots));
      assert.equal(r.remaining, i);
      assert.equal(r.file, `design/${ops[i].file}`);
      assert.deepEqual(r.op, Object.fromEntries(Object.entries(ops[i]).filter(([k]) => k !== 'file')));
    }
    for (const root of Object.values(roots))
      for (const [rel, bytes] of Object.entries(before))
        assert.ok(fs.readFileSync(path.join(root, 'design', rel)).equals(bytes), `${rel} is back to its original bytes`);
    refused(undo(roots), /^Nothing to undo\.$/);
  });

  test('undo refuses a file that changed since, and skips the edit of a deleted file', () => {
    const roots = pair(lab);
    ok(edit(roots, { file: A, op: 'text', id: 'items.title', value: 'One' }));
    ok(edit(roots, { file: LAB, op: 'text', id: 'lab.a', value: 'Two' }));
    ok(edit(roots, { file: A, op: 'text', id: 'items.title', value: 'Three' }));
    for (const root of Object.values(roots)) fs.appendFileSync(path.join(root, 'design', A), '<!-- edited by an editor -->');
    refused(undo(roots), /design\/screens\/items\/a\.html changed after that edit/);
    for (const root of Object.values(roots)) fs.rmSync(path.join(root, 'design', 'screens', 'items'), { recursive: true });
    assert.deepEqual(ok(undo(roots)), { file: `design/${LAB}`, op: { op: 'text', id: 'lab.a', value: 'Two' }, remaining: 1 });
    refused(undo(roots), /^Nothing to undo\.$/);
  });

  test(`the journal keeps the newest ${MAX_JOURNAL} records`, () => {
    const roots = pair();
    for (let i = 0; i < MAX_JOURNAL + 3; i++) {
      assert.equal(native(['edit', roots.native], JSON.stringify({ file: A, op: 'text', id: 'items.title', value: `v${i}` })).status, 0);
    }
    sameChanges(roots, 'after the cap');
    assert.equal(fs.readdirSync(path.join(roots.native, '.dfw', 'edits')).length, MAX_JOURNAL);
    ok(undo(roots));
    assert.match(fs.readFileSync(path.join(roots.native, 'design', A), 'utf8'), new RegExp(`>v${MAX_JOURNAL + 1}<`));
  });
});

describe('edit: hostile sources', { skip: skipReason() }, () => {
  const BODY = [
    '<main data-dfw-id="m" class="a">',
    '  <!-- <p data-dfw-id="t">commented out</p> </div> </main> -->',
    '  <div data-dfw-id="box">',
    '    <!-- </div> old wrapper -->',
    '    <p data-dfw-id="t">Text</p>',
    '  </div>',
    "  <section data-dfw-id='quoted' class='a > b' style='--Gap: 1px; background: url(\"a;b\")' title=\"it's\">",
    '    <span data-dfw-id="s1">one</span>',
    '    <span data-dfw-id="s2">two</span>',
    '  </section>',
    '  <div data-dfw-id="empty"></div>',
    '</main>',
  ];
  const page = (nl, tail) =>
    [
      '<!doctype html>',
      '<html lang="en">',
      '<head>',
      '  <meta charset="utf-8">',
      '  <!-- <head> <style> p { } -->',
      '</head>',
      '<body>',
      ...BODY,
      '</body>',
      '</html>',
    ].join(nl) + tail;
  const nested = `<html><head></head><body>\n${'<div>'.repeat(150)}<div data-dfw-id="deep">${'<div>'.repeat(149)}<b data-dfw-id="leaf">x</b>${'</div>'.repeat(300)}\n<p data-dfw-id="after">after</p>\n</body></html>\n`;
  const corpus = {
    'comments and single quotes': page('\n', '\n'),
    'CRLF line endings': page('\r\n', '\r\n'),
    'no trailing newline': page('\n', ''),
    'byte order mark': `\uFEFF${page('\n', '\n')}`,
    'BOM and CRLF, no trailing newline': `\uFEFF${page('\r\n', '')}`,
  };
  const ops = [
    { op: 'text', id: 't', value: 'New <text> & more' },
    { op: 'spacing', id: 'quoted', prop: 'gap', value: 'space.4' },
    { op: 'spacing', id: 'm', prop: 'padding', value: 'space.2' },
    { op: 'move', id: 's2', ref: 's1', position: 'before' },
    { op: 'move', id: 'box', ref: 'quoted', position: 'after' },
    { op: 'move', id: 's1', ref: 'empty', position: 'inside' },
    { op: 'move', id: 'quoted', ref: 'box', position: 'inside' },
    { op: 'layout', id: 'box', prop: 'grid-column', value: 'span 2' },
    { op: 'layout', id: 'quoted', prop: 'width', value: '50%', maxWidth: 390 },
    { op: 'text', id: 's2', value: 'zwei' },
    { op: 'layout', id: 'box' },
  ];

  for (const [name, source] of Object.entries(corpus)) {
    test(`${name}: every op, then undo back to the original bytes`, () => {
      const roots = pair((root) => write(root, `design/${A}`, source));
      let done = 0;
      for (const op of ops) if (edit(roots, { file: A, ...op }).ok?.changed) done++;
      assert.ok(done >= 8, `${done} edits applied`);
      for (const root of Object.values(roots)) assert.notEqual(fs.readFileSync(path.join(root, 'design', A), 'utf8'), source);
      for (; done > 0; done--) ok(undo(roots));
      for (const root of Object.values(roots))
        assert.ok(fs.readFileSync(path.join(root, 'design', A)).equals(Buffer.from(source)), 'original bytes');
    });
  }

  test('300 nested elements', () => {
    const roots = pair((root) => write(root, `design/${A}`, nested));
    ok(edit(roots, { file: A, op: 'text', id: 'leaf', value: 'y' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'deep', prop: 'padding', value: 'space.2' }));
    ok(edit(roots, { file: A, op: 'move', id: 'after', ref: 'leaf', position: 'before' }));
    refused(edit(roots, { file: A, op: 'move', id: 'deep', ref: 'after', position: 'inside' }), /into itself/);
    ok(edit(roots, { file: A, op: 'layout', id: 'deep', prop: 'min-height', value: 'space.4' }));
    for (let i = 0; i < 4; i++) ok(undo(roots));
    for (const root of Object.values(roots)) assert.equal(fs.readFileSync(path.join(root, 'design', A), 'utf8'), nested);
  });

  test('tags in comments, scripts and attribute values are not elements', () => {
    const source = `<html><head><style>.x[data-dfw-id="t"] { color: red }</style></head><body>
<!-- <h1 data-dfw-id="t">commented out</h1> -->
<main data-dfw-id="m" title="<p data-dfw-id='t'>" data-x='a > b'>
  <h1 data-dfw-id="t">
    Welcome back
  </h1>
  <p data-dfw-id="u">a &amp; b</p>
</main>
<script>const s = '<p data-dfw-id="t">x</p>';</script>
</body></html>`;
    const roots = pair((root) => write(root, `design/${A}`, source));
    ok(edit(roots, { file: A, op: 'text', id: 't', value: 'Hi' }));
    ok(edit(roots, { file: A, op: 'text', id: 'u', value: 'c & d' }));
    ok(edit(roots, { file: A, op: 'spacing', id: 'm', prop: 'gap', value: 'space.2' }));
    ok(edit(roots, { file: A, op: 'move', id: 'u', ref: 't', position: 'before' }));
    ok(edit(roots, { file: A, op: 'layout', id: 'u', prop: 'width', value: '25%' }));
    for (let i = 0; i < 5; i++) ok(undo(roots));
    for (const root of Object.values(roots)) assert.equal(fs.readFileSync(path.join(root, 'design', A), 'utf8'), source);
  });

  test('an element that never closes is refused', () => {
    const roots = pair((root) =>
      write(root, `design/${A}`, '<html><head></head><body><div data-dfw-id="open"><p data-dfw-id="p">x</p></body></html>'),
    );
    refused(edit(roots, { file: A, op: 'text', id: 'open', value: 'x' }), /Could not find the end of <div> for "open"/);
    ok(edit(roots, { file: A, op: 'text', id: 'p', value: 'y' }));
  });
});

// The binary finds elements with the tokenizer of design/html.rs (design decision 2 of the
// rust-core change), the Node code of 0.5.2 with regular expressions. On the sources below the
// Node result was a known defect, so the recorded result is the binary's own; the Node outputs
// stay in the cases as a record of what the defect did.
describe('edit: sources where the tokenizer and the Node tag scanner differ', { skip: skipReason() }, () => {
  const H = (body, head = '') => `<html><head>${head}</head><body>${body}</body></html>`;
  const BLOCK = (rule) =>
    `\n<style data-dfw-layout>\n  /* dfw Studio layout edits (drag/resize): layout properties only, one rule per element; :not(#dfw) lifts specificity over screen rules. */\n  ${rule}\n</style>\n`;
  const P = '<p data-dfw-id="p">x</p>';
  const W50 = { op: 'layout', id: 'p', prop: 'width', value: '50%' };
  const RULE = "[data-dfw-id='p']:not(#dfw) { width: 50%; }";
  const cases = [
    {
      name: 'a closing tag inside a <script> string of the moved element: Node cuts the element there',
      source: H('<div data-dfw-id="a"><script>var s = "</div>";</script><p data-dfw-id="p">x</p></div><i data-dfw-id="b">b</i>'),
      op: { op: 'move', id: 'a', ref: 'b', position: 'after' },
      native: H('<i data-dfw-id="b">b</i><div data-dfw-id="a"><script>var s = "</div>";</script><p data-dfw-id="p">x</p></div>'),
      node: H('";</script><p data-dfw-id="p">x</p></div><i data-dfw-id="b">b</i><div data-dfw-id="a"><script>var s = "</div>'),
    },
    {
      name: 'a commented-out </head>: Node writes the layout block into the comment',
      source: H(P, '<!-- </head> -->'),
      op: W50,
      native: H(P, `<!-- </head> -->${BLOCK(RULE)}`),
      node: H(P, `<!-- ${BLOCK(RULE)}</head> -->`),
    },
    {
      name: 'a commented-out layout block: Node rewrites it inside the comment',
      source: H(P, "<!-- <style data-dfw-layout>[data-dfw-id='q'] { width: 25%; }</style> -->"),
      op: W50,
      native: H(P, `<!-- <style data-dfw-layout>[data-dfw-id='q'] { width: 25%; }</style> -->${BLOCK(RULE)}`),
      node: H(P, `<!-- ${BLOCK(`[data-dfw-id='q']:not(#dfw) { width: 25%; }\n  ${RULE}`).trim()} -->`),
    },
    {
      name: 'data-dfw-layout inside an attribute value is not the marker: Node takes the style for the layout block',
      source: H(P, '<style class="data-dfw-layout">a{}</style>'),
      op: W50,
      native: H(P, `<style class="data-dfw-layout">a{}</style>${BLOCK(RULE)}`),
      nodeError: /edited by hand/,
    },
    {
      name: 'an id inside a <script> that never closes: Node edits the script text',
      source: H(`<script>${P}`),
      op: { op: 'text', id: 'p', value: 'y' },
      nativeError: /"p" was not found/,
      node: H('<script><p data-dfw-id="p">y</p>'),
    },
    {
      name: 'an attribute with no white space before it: Node does not see it and adds a second one',
      source: H('<p data-dfw-id="p" class="c"style="gap: 1px">x</p>'),
      op: { op: 'spacing', id: 'p', prop: 'gap', value: 'space.2' },
      native: H('<p data-dfw-id="p" class="c" style="gap: var(--space-2)">x</p>'),
      node: H('<p data-dfw-id="p" class="c"style="gap: 1px" style="gap: var(--space-2)">x</p>'),
    },
  ];
  for (const c of cases) {
    test(c.name, () => {
      const roots = pair((root) => write(root, `design/${A}`, c.source));
      const op = { file: A, ...c.op };
      const b = viaNative(['edit', roots.native], JSON.stringify(op));
      const file = (root) => fs.readFileSync(path.join(root, 'design', A), 'utf8');
      const got = { result: b, file: file(roots.native) };
      assert.deepEqual(got, expected(got));
      if (c.nativeError) refused(b, c.nativeError);
      else ok(b);
      assert.equal(file(roots.native), c.native ?? c.source, 'native');
    });
  }
});

describe('design system page', { skip: skipReason() }, () => {
  /** Temporary and checkout paths as <tmp> and <repo>, so a result can be recorded. */
  const mask = (v) =>
    JSON.parse(
      JSON.stringify(v).split(JSON.stringify(TMP).slice(1, -1)).join('<tmp>').split(JSON.stringify(REPO).slice(1, -1)).join('<repo>'),
    );
  /** The page of a project against the recorded one (rendering writes nothing). */
  function page(root, base) {
    const r = native(['system-page', root, ...(base === undefined ? [] : ['--base', base])]);
    const b = mask(r.status === 0 ? { ok: r.stdout } : { error: r.stderr.replace(/\n$/, '') });
    const a = expected(b);
    if (a.ok !== undefined && b.ok !== undefined && a.ok !== b.ok) {
      const [x, y] = [a.ok.split('\n'), b.ok.split('\n')];
      const i = x.findIndex((l, k) => l !== y[k]);
      let c = 0;
      while (x[i]?.[c] === y[i]?.[c]) c++;
      assert.fail(
        `page differs at line ${i + 1}, column ${c + 1}\n  expected: ${JSON.stringify(x[i]?.slice(Math.max(0, c - 80), c + 160))}\n  binary:   ${JSON.stringify(y[i]?.slice(Math.max(0, c - 80), c + 160))}`,
      );
    }
    assert.deepEqual(b, a);
    return b;
  }
  const project = (setup) => {
    const root = path.join(TMP, `s${n++}`, 'project');
    fs.cpSync(FIXTURE, root, { recursive: true });
    setup?.(root);
    return root;
  };
  const useKit = (root) => {
    for (const f of fs.readdirSync(KIT)) fs.copyFileSync(path.join(KIT, f), path.join(root, 'design/system/components', f));
  };
  /** `dfw <args>` with the binary (setup only: the project is made as when recorded). Returns the exit code. */
  const cli = (root, args) => {
    const env = {
      ...process.env,
      DFW_PLUGIN_ROOT: CORE,
      DFW_PROJECT_DIR: root,
      CLAUDE_PROJECT_DIR: root,
      DFW_HOME: path.join(root, '..', 'home'),
    };
    const r = spawnSync(nativeBinary(), args, { cwd: root, env, encoding: 'utf8', timeout: 180_000 });
    if (r.error) throw r.error;
    return r.status;
  };
  const must = (root, args) => assert.equal(cli(root, args), 0, `dfw ${args.join(' ')}`);
  const DESIGN_MD = `# Fixture

## Direction

A **calm** ledger for \`stock\` counts, <not> a dashboard.
Second line of the first paragraph.

Second paragraph is not shown.

## Rationale
<!-- why each decision was made -->

- Accent is one blue step because operators scan for the single action per view
- Neutrals lean warm - because the paper forms they replace are cream
* Type scale is 1.2 – Because tables need many sizes close together
- No because here, just a rule that spans the two columns
  - a nested bullet because indentation is allowed
-not a bullet

## Other

- unrelated bullet
`;

  test('the fixture project as it is, with every base', () => {
    const root = project();
    for (const base of [undefined, '../', '/design/', '']) {
      const r = page(root, base);
      assert.match(r.ok, /^<!doctype html>\n<html lang="en">/);
      assert.match(r.ok, /data-dfw-id="system\.component\.button"/);
      assert.match(r.ok, /<\/html>\n$/);
    }
    assert.doesNotMatch(page(root).ok, /<h2>Concept<\/h2>/, 'no style.json: no concept section');
  });

  test('a style guide with DESIGN.md, themes, icons, assets and the kit masters', () => {
    for (const style of ['heritage-signboard', 'ops-dense', 'editorial-luxe']) {
      const root = project((r) => {
        fs.rmSync(path.join(r, 'design'), { recursive: true });
        write(
          r,
          'design/design.json',
          JSON.stringify({
            schema: 1,
            product: 'Fixture & <Co>',
            lang: 'vi',
            sampleText: 'Kho hàng · 0123',
            surface: 'product',
            screens: [],
          }),
        );
        must(r, ['system', 'init', '--style', style]);
        useKit(r);
        must(r, ['icons', 'add', 'plus', 'search', 'chevron-down']);
        write(
          r,
          'design/tokens/themes/dark.json',
          JSON.stringify({
            color: {
              $type: 'color',
              ink: { $value: '#fafafa', $description: 'Dark ink. Used for: text' },
              extra: { $value: '{color.ink}' },
            },
          }),
        );
        write(r, 'design/assets/logo mark (1).svg', '<svg xmlns="http://www.w3.org/2000/svg"/>');
        write(r, 'design/assets/photo.JPG', 'x');
        write(r, 'design/assets/notes.txt', 'x');
        write(r, 'design/assets/.hidden', 'x');
        write(r, 'design/assets/ảnh <1>.png', 'x');
        write(r, 'DESIGN.md', DESIGN_MD);
        must(r, ['tokens', 'build']);
      });
      const r = page(root, '../');
      assert.match(r.ok, /<h2>Concept<\/h2>/);
      assert.match(r.ok, /Why \(DESIGN\.md\)/);
      assert.match(r.ok, /A <b>calm<\/b> ledger for <code>stock<\/code> counts, &lt;not&gt; a dashboard\.\nSecond line/);
      assert.match(r.ok, /class="ds-ramp"/);
      assert.match(r.ok, /class="ds-spec"/);
      assert.match(r.ok, /<img src="\.\.\/assets\/logo%20mark%20\(1\)\.svg" alt="">/);
      assert.match(r.ok, /<h2>Icons · 3 \(Lucide\)<\/h2>/);
      page(root, '/design/');
      // Without DESIGN.md the style guide's own reasons are shown.
      fs.rmSync(path.join(root, 'DESIGN.md'));
      assert.match(page(root, '../').ok, /Why \(style guide\)/);
      // CRLF and a TODO direction fall back to the style's concept.
      write(root, 'DESIGN.md', DESIGN_MD.replace('A **calm**', 'TODO: A **calm**').replaceAll('\n', '\r\n'));
      page(root, '../');
    }
  });

  test('presets with the kit: every component section', () => {
    for (const preset of ['crisp', 'warm']) {
      const root = project((r) => {
        fs.rmSync(path.join(r, 'design'), { recursive: true });
        if (cli(r, ['system', 'init', '--preset', preset]) !== 0) must(r, ['system', 'init']);
        useKit(r);
      });
      const r = page(root, '../');
      assert.match(r.ok, /data-dfw-id="system\.component\.app-shell"/);
      assert.match(r.ok, /class="ds-frames"/);
      assert.match(r.ok, /<table class="ds-table ds-types">/);
    }
  });

  test('odd projects: no tokens, no design folder, short hex, alias chains, descriptions with notes', () => {
    page(project((r) => fs.rmSync(path.join(r, 'design/tokens'), { recursive: true })));
    page(project((r) => fs.rmSync(path.join(r, 'design'), { recursive: true })));
    const tokens = {
      $extensions: { dfw: { exceptions: ['contrast'] } },
      color: {
        $type: 'color',
        palette: {
          'blue-500': { $value: '#36c', $description: 'Brand blue.  Used for: accent. Reserve step 500 for actions' },
          'blue-50': { $value: { hex: '#eef' } },
          'blue-950': { $value: '{color.palette.blue-500}' },
          plain: { $value: 'rebeccapurple' },
          'loop-1': { $value: '{color.palette.loop-1}' },
        },
        ink: { $value: '#ccc', $description: 'Text.\nUsed for: body' },
        surface: { $value: '#fff' },
        canvas: { $value: '#fff' },
        accent: { $value: '{color.palette.blue-500}', $description: 'Action colour. Used for: buttons' },
        'accent-soft': { $value: '{color.palette.blue-50}' },
        'accent-text': { $value: '{color.palette.blue-950}' },
        'on-accent': { $value: '#ffffff' },
        muted: { $value: '#eee' },
        signature: { $value: '{color.missing}' },
        status: { ok: { $value: '#0a0' }, warn: { $value: '#fa0' }, 'ok-tint': { $value: '#efe' } },
      },
      space: {
        $type: 'dimension',
        10: { $value: { value: 40, unit: 'px' } },
        2: { $value: { value: 8, unit: 'px' } },
        odd: { $value: { value: 7, unit: 'px' } },
        raw: { $value: '1rem' },
        none: { $value: { unit: 'px' } },
      },
      radius: { $type: 'dimension', sm: { $value: { value: 2, unit: 'px' } }, pill: { $value: { value: 999, unit: 'px' } } },
      shadow: {
        $type: 'shadow',
        sm: {
          $value: {
            color: { colorSpace: 'srgb', components: [0, 0, 0], alpha: 0.2 },
            offsetX: { value: 0, unit: 'px' },
            offsetY: { value: 1, unit: 'px' },
            blur: { value: 2, unit: 'px' },
            spread: { value: 0, unit: 'px' },
          },
        },
      },
      font: {
        family: { $type: 'fontFamily', display: { $value: 'Georgia' }, body: { $value: ['Arial', 'sans-serif'] }, mono: { $value: [] } },
        size: {
          $type: 'dimension',
          md: { $value: { value: 16, unit: 'px' }, $description: 'Scale 1.2 <minor third>' },
          sm: { $value: { value: 14, unit: 'px' } },
          'display-xl': { $value: { value: 60, unit: 'px' } },
        },
      },
    };
    const text =
      '<dfw-component name="text" variants="display-xl title body mono" description="Text styles. as = the tag to render; display-xl = one per screen (hero); mono = ids. Never bold a mono line! Use sparingly?  yes: it is loud." props="as" group="Type">\n<style>\n.text { font: 400 16px/1.5 "Inter", system-ui; padding: var(--space-2) var(--space-10); gap: var(--space-odd); color: red; border-radius: var(--radius-unknown) }\n</style>\n<template><span class="text text--{{variant}}"><dfw-slot></dfw-slot></span></template>\n</dfw-component>\n';
    const root = project((r) => {
      write(r, 'design/tokens/tokens.json', JSON.stringify(tokens));
      write(r, 'design/system/components/text.html', text);
      write(
        r,
        'design/system/components/bare.html',
        '<dfw-component name="bare_thing-x" sizes="sm lg" variants="a b"><template><i></i></template></dfw-component>',
      );
      write(r, 'design/system/head.html', '<link rel="stylesheet" href="{{base}}fonts/a.css"><!-- {{base}} -->');
      write(
        r,
        'design/system/style.json',
        JSON.stringify({
          id: 'x',
          name: 'Odd & <style>',
          concept: 'A concept with <tags> & **stars**',
          rationale: ['One thing Because of another', 'A rule'],
          do: ['Label: explain it', 'no label here.', '**Bold** and `code`'],
          dont: [],
          layout: '12 columns',
          motifs: [{ name: 'Rule', use: 'Under titles' }, {}],
          components: ['Buttons: one primary'],
        }),
      );
    });
    const r = page(root, '../');
    assert.match(r.ok, /1\.6:1/);
    assert.match(r.ok, /<h2>Concept<\/h2>/);
    assert.match(r.ok, /Why \(style guide\)/);
    assert.match(page(project((r2) => write(r2, 'design/system/style.json', 'null'))).ok, /<h1>Design System<\/h1><p>Fixture<\/p>/);
    const broken = page(project((r2) => write(r2, 'design/tokens/tokens.json', JSON.stringify({ a: { $value: null } }))));
    assert.match(broken.error, /Token "a" has no value/);
  });
});

describe('layout cache', { skip: skipReason() }, () => {
  /** The recorded answer of changedScreens, the recorded cache file from markChecked. */
  function changed(roots, label) {
    const b = viaNative(['changed-screens', roots.native]);
    assert.deepEqual(b, expected(b), label);
    return b.ok;
  }
  function mark(roots) {
    const r = native(['mark-checked', roots.native]);
    assert.equal(r.status, 0, r.stderr);
    assert.equal(r.stdout, '');
    sameChanges(roots, 'after markChecked');
  }

  test('nothing checked yet, checked, then a master, the base styles, the tokens and a screen change', () => {
    const roots = pair((root) => {
      write(root, 'design/styles/base.css', 'body{margin:0}');
      write(
        root,
        'design/system/components/card.html',
        '<dfw-component name="card"><template><div class="card"><dfw-use c="button">Go</dfw-use></div></template></dfw-component>',
      );
      write(root, 'design/system/components/notes.txt', 'not a master');
      write(root, 'design/screens/items/b.html', '<html><body><dfw-use c="card"></dfw-use></body></html>');
      const m = JSON.parse(fs.readFileSync(path.join(root, 'design/design.json'), 'utf8'));
      m.screens[0].variants.push({ id: 'b', file: 'screens/items/b.html' }, { id: 'gone', file: 'screens/items/gone.html' });
      m.screens.push({ id: 'other', variants: [{ id: 'a', file: 'screens/other/a.html' }] }, { id: 'empty' });
      write(root, 'design/screens/other/a.html', '<html><body></body></html>');
      write(root, 'design/design.json', JSON.stringify(m));
    });
    const each = (fn) => Object.values(roots).forEach(fn);
    assert.deepEqual(changed(roots, 'first run'), {
      screens: ['items', 'other'],
      changedComps: ['button', 'card', 'notes.txt'],
      tokensChanged: true,
    });
    mark(roots);
    assert.deepEqual(changed(roots, 'just checked'), { screens: [], changedComps: [], tokensChanged: false });
    each((root) =>
      write(
        root,
        'design/system/components/button.html',
        fs.readFileSync(path.join(root, 'design/system/components/button.html'), 'utf8').replace('40px', '44px'),
      ),
    );
    assert.deepEqual(changed(roots, 'a master').screens, ['items', 'other']);
    mark(roots);
    each((root) => write(root, 'design/styles/base.css', 'body{margin:0;width:3000px}'));
    assert.deepEqual(changed(roots, 'base.css'), { screens: ['items', 'other'], changedComps: [], tokensChanged: true });
    mark(roots);
    each((root) => write(root, 'design/system/head.html', '<link rel="stylesheet" href="x.css">'));
    assert.equal(changed(roots, 'head.html appears').tokensChanged, true);
    mark(roots);
    each((root) => write(root, 'design/screens/other/a.html', '<html><body><p>x</p></body></html>'));
    assert.deepEqual(changed(roots, 'one screen'), { screens: ['other'], changedComps: [], tokensChanged: false });
    each((root) => fs.rmSync(path.join(root, 'design/tokens/tokens.json')));
    assert.equal(changed(roots, 'tokens removed').tokensChanged, true);
    mark(roots);
    assert.deepEqual(changed(roots, 'checked without tokens'), { screens: [], changedComps: [], tokensChanged: false });
    each((root) => write(root, 'design/screens/items/gone.html', '<html></html>'));
    assert.deepEqual(changed(roots, 'a variant appears').screens, ['items']);
  });

  test('a project without a manifest or components', () => {
    const roots = pair((root) => {
      fs.rmSync(path.join(root, 'design/design.json'));
      fs.rmSync(path.join(root, 'design/system'), { recursive: true });
    });
    assert.deepEqual(changed(roots, 'bare'), { screens: [], changedComps: [], tokensChanged: true });
    mark(roots);
    assert.deepEqual(changed(roots, 'bare, checked'), { screens: [], changedComps: [], tokensChanged: false });
  });
});
