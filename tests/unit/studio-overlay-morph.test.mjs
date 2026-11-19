// The DOM diff of the overlay's in-place update (core/studio/overlay/morph.js, studio-ui.md U8), on
// a small DOM (linkedom): what is kept, what moves, what is marked changed, and when it gives up.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { parseHTML } from 'linkedom';
import { morph } from '../../core/studio/overlay/morph.js';

const page = (body, head = '') => parseHTML(`<!doctype html><html><head>${head}</head><body>${body}</body></html>`).document;

/** Morph the live document's body into `next`; returns the live document and the changed set. */
function update(liveBody, nextBody, { liveHead = '', nextHead = '' } = {}) {
  const live = page(liveBody, liveHead);
  const next = page(nextBody, nextHead);
  const changed = new Set();
  const before = [...live.querySelectorAll('[data-dfw-id]')];
  morph(live.head, next.head, changed);
  morph(live.body, next.body, changed);
  return { live, changed, before };
}

test('studio-overlay-morph: an identical document is left alone and nothing is marked', () => {
  const html = '<main data-dfw-id="m"><h1 data-dfw-id="t">Hi</h1><p>Text</p></main>';
  const { live, changed, before } = update(html, html);
  assert.equal(changed.size, 0);
  assert.equal(live.querySelector('[data-dfw-id="t"]'), before[1], 'same node kept');
});

test('studio-overlay-morph: a text change keeps the element and marks its parent', () => {
  const { live, changed, before } = update(
    '<main data-dfw-id="m"><h1 data-dfw-id="t">Welcome back</h1></main>',
    '<main data-dfw-id="m"><h1 data-dfw-id="t">Good morning</h1></main>',
  );
  const h1 = live.querySelector('[data-dfw-id="t"]');
  assert.equal(h1, before[1], 'the h1 is the same node (scroll and selection stay)');
  assert.equal(h1.textContent, 'Good morning');
  assert.ok(changed.has(h1), 'the element whose text changed is marked');
});

test('studio-overlay-morph: keyed children are moved, not rebuilt', () => {
  const { live, changed, before } = update(
    '<ul data-dfw-id="l"><li data-dfw-id="a">A</li><li data-dfw-id="b">B</li><li data-dfw-id="c">C</li></ul>',
    '<ul data-dfw-id="l"><li data-dfw-id="c">C</li><li data-dfw-id="a">A</li><li data-dfw-id="b">B</li></ul>',
  );
  const ids = [...live.querySelectorAll('li')].map((x) => x.getAttribute('data-dfw-id'));
  assert.deepEqual(ids, ['c', 'a', 'b']);
  const byId = (id) => live.querySelector(`[data-dfw-id="${id}"]`);
  for (const id of ['a', 'b', 'c']) assert.ok(before.includes(byId(id)), `${id} kept its node`);
  assert.equal(changed.size, 0, 'a pure reorder marks nothing');
});

test('studio-overlay-morph: an inserted element is imported and marked; a removed one goes', () => {
  const { live, changed } = update(
    '<main data-dfw-id="m"><p data-dfw-id="old">Old</p><p data-dfw-id="keep">Keep</p></main>',
    '<main data-dfw-id="m"><p data-dfw-id="keep">Keep</p><p data-dfw-id="new">New</p></main>',
  );
  assert.equal(live.querySelector('[data-dfw-id="old"]'), null);
  const added = live.querySelector('[data-dfw-id="new"]');
  assert.ok(added);
  assert.equal(added.ownerDocument, live, 'imported into the live document');
  assert.ok(changed.has(added));
  assert.ok(changed.has(live.querySelector('[data-dfw-id="m"]')), 'the parent of a removed element is marked');
});

test('studio-overlay-morph: unkeyed nodes match by position or a little further on', () => {
  const { live, before } = update(
    '<div data-dfw-id="d"><p>One</p><p>Two</p></div>',
    '<div data-dfw-id="d"><span>New</span><p>One</p><p>Two</p></div>',
  );
  const ps = [...live.querySelectorAll('p')];
  assert.deepEqual(
    ps.map((p) => p.textContent),
    ['One', 'Two'],
  );
  assert.equal(live.querySelector('span').textContent, 'New');
  assert.equal(before[0], live.querySelector('[data-dfw-id="d"]'));
});

test('studio-overlay-morph: attributes follow the source; contenteditable of an edit in progress stays', () => {
  const { live, changed } = update(
    '<p data-dfw-id="p" class="a" title="x" contenteditable="plaintext-only">T</p>',
    '<p data-dfw-id="p" class="b">T</p>',
  );
  const p = live.querySelector('p');
  assert.equal(p.getAttribute('class'), 'b');
  assert.equal(p.hasAttribute('title'), false);
  assert.equal(p.getAttribute('contenteditable'), 'plaintext-only');
  assert.ok(changed.has(p));
});

test("studio-overlay-morph: the overlay's own UI (data-dfw-ui) is kept", () => {
  const live = page('<main data-dfw-id="m">A</main><div data-dfw-ui="">handles</div>');
  const next = page('<main data-dfw-id="m">B</main>');
  morph(live.body, next.body, new Set());
  assert.ok(live.querySelector('[data-dfw-ui]'), 'UI layer still there');
  assert.equal(live.querySelector('main').textContent, 'B');
});

test('studio-overlay-morph: scripts are never added, removed or changed in place', () => {
  const run = (a, b) => () => morph(page(a).body, page(b).body, new Set());
  assert.throws(run('<p>x</p>', '<p>x</p><script>1</script>'), /script added/);
  assert.throws(run('<p>x</p><script>1</script>', '<p>x</p>'), /script removed/);
  assert.throws(run('<script>1</script>', '<script>2</script>'), /script changed/);
  assert.doesNotThrow(run('<p>x</p><script>1</script>', '<p>y</p><script>1</script>'));
});

test('studio-overlay-morph: a stylesheet link that only differs by the cache key is left for the sheet swap', () => {
  globalThis.location = /** @type {any} */ ({ href: 'http://127.0.0.1:4700/design/screens/home/a.html' });
  try {
    const { live, changed } = update('<p>x</p>', '<p>x</p>', {
      liveHead: '<link rel="stylesheet" href="/design/a.css?v=1">',
      nextHead: '<link rel="stylesheet" href="/design/a.css">',
    });
    assert.equal(live.querySelector('link').getAttribute('href'), '/design/a.css?v=1', 'the swapped link keeps its cache key');
    assert.equal(changed.size, 0);
  } finally {
    delete globalThis.location;
  }
});

test('studio-overlay-morph: an element the user is editing is left exactly as it is', () => {
  const live = page('<main data-dfw-id="m"><h1 data-dfw-id="t">Typing her</h1><p data-dfw-id="p">Old</p></main>');
  const next = page('<main data-dfw-id="m"><h1 data-dfw-id="t">Agent text</h1><p data-dfw-id="p">New</p></main>');
  const h1 = live.querySelector('[data-dfw-id="t"]');
  h1.setAttribute('contenteditable', 'plaintext-only');
  const changed = new Set();
  morph(live.body, next.body, changed, h1);
  assert.equal(h1.textContent, 'Typing her', 'the text being typed is kept');
  assert.ok(!changed.has(h1), 'the edited element is not marked');
  assert.equal(live.querySelector('[data-dfw-id="p"]').textContent, 'New', 'the rest of the page is updated');
});
