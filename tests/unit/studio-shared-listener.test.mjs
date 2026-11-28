// What the Studio's send bar and the Penpot bridge panel say about the AI
// (core/studio/shared/listener.js): one state from `{ listening, working }`, one text per state.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { listenerHtml, listenerState } from '#studio/shared/listener.js';
import { LISTENER } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

test('studio-shared-listener: the state of a listening answer, anything else is off', () => {
  assert.equal(listenerState({ listening: true, working: true }), LISTENER.LISTENING);
  assert.equal(listenerState({ listening: false, working: true }), LISTENER.WORKING);
  assert.equal(listenerState({ listening: false, working: false }), LISTENER.OFF);
  assert.equal(listenerState({ listening: true }), LISTENER.LISTENING, 'an older server sends no working');
  for (const odd of [null, undefined, 'listening', { listening: 'true' }, []]) assert.equal(listenerState(odd), LISTENER.OFF);
});

test('studio-shared-listener: each state has its own text; an unknown state reads as not listening', () => {
  assert.equal(listenerHtml(LISTENER.LISTENING), TEXT.listeningHtml);
  assert.equal(listenerHtml(LISTENER.WORKING), TEXT.workingHtml);
  assert.equal(listenerHtml(LISTENER.OFF), TEXT.notListeningHtml);
  assert.equal(listenerHtml('nope'), TEXT.notListeningHtml);
  assert.equal(new Set(Object.values(LISTENER).map(listenerHtml)).size, Object.values(LISTENER).length);
});
