// Pure helpers of the overlay: selector specificity (which rule beats a layout edit), the snap
// choice of a resize, and the order of token names in the inspector.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { specificity } from '../../core/studio/overlay/verify.js';
import { nearest } from '../../core/studio/overlay/layout.js';
import { match, semanticFirst } from '../../core/studio/overlay/tokens.js';

test('studio-overlay-pure: specificity counts ids, classes/attributes/pseudo-classes and types', () => {
  assert.deepEqual(specificity('#a'), [1, 0, 0]);
  assert.deepEqual(specificity('.cards > .pinned'), [0, 2, 0]);
  assert.deepEqual(specificity('main .cards > div'), [0, 1, 2]);
  assert.deepEqual(specificity("[data-dfw-id='x']:not(#dfw)"), [1, 1, 0]);
  assert.deepEqual(specificity(':where(.a, #b) p'), [0, 0, 1], ':where() adds nothing');
});

test('studio-overlay-pure: nearest picks the closest snap, the first of equals', () => {
  const list = [{ px: 10 }, { px: 20 }, { px: 30 }];
  assert.equal(nearest(list, 12), list[0]);
  assert.equal(nearest(list, 26), list[2]);
  assert.equal(nearest(list, 15), list[0], 'a tie keeps the first');
  assert.equal(nearest(list, 1000), list[2]);
});

test('studio-overlay-pure: semantic token names come first, then shorter ones', () => {
  assert.deepEqual(semanticFirst(['space.4', 'space.md', 'space.inset.card']), ['space.md', 'space.inset.card', 'space.4']);
});

test('studio-overlay-pure: match keeps the tokens of the style group family', () => {
  const map = new Map([['16px', ['space.4', 'size.icon', 'radius.lg', 'font.size.md']]]);
  assert.deepEqual(match('padding', '16px', map), ['space.4']);
  assert.deepEqual(match('border-radius', '16px', map), ['radius.lg']);
  assert.deepEqual(match('font-size', '16px', map), ['size.icon', 'font.size.md']);
  assert.deepEqual(match('color', '16px', map), ['size.icon', 'radius.lg', 'font.size.md', 'space.4'], 'no family: every name');
  assert.deepEqual(match('padding', '3px', map), []);
});
