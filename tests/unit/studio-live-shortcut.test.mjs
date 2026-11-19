// The live-review shortcut (core/studio/live/shortcut.js): parsing data-shortcut, the label, and
// matching keydown by event.code (Option+C types "ç" on macOS).
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { isShortcut, parseShortcut, shortcutLabel } from '../../core/studio/live/shortcut.js';

const key = (code, key, mods = {}) => ({ altKey: false, ctrlKey: false, shiftKey: false, metaKey: false, code, key, ...mods });

test('studio-live-shortcut: alt+c by default, "off" means none', () => {
  assert.deepEqual(parseShortcut(undefined), { key: 'c', alt: true, ctrl: false, shift: false, meta: false });
  assert.deepEqual(parseShortcut('Ctrl+Shift+K'), { key: 'k', alt: false, ctrl: true, shift: true, meta: false });
  assert.equal(parseShortcut('off'), null);
});

test('studio-live-shortcut: the label names the modifiers per platform', () => {
  assert.equal(shortcutLabel(parseShortcut('alt+c'), true), 'Option+C');
  assert.equal(shortcutLabel(parseShortcut('alt+c'), false), 'Alt+C');
  assert.equal(shortcutLabel(parseShortcut('meta+shift+1'), true), 'Shift+Cmd+1');
  assert.equal(shortcutLabel(null, true), '');
});

test('studio-live-shortcut: matches by code (Option+C types ç), modifiers exactly', () => {
  const sc = parseShortcut('alt+c');
  assert.equal(isShortcut(sc, key('KeyC', 'ç', { altKey: true })), true);
  assert.equal(isShortcut(sc, key('KeyC', 'c', { altKey: true, shiftKey: true })), false, 'extra Shift');
  assert.equal(isShortcut(sc, key('KeyC', 'c')), false, 'no Alt');
  assert.equal(isShortcut(parseShortcut('ctrl+2'), key('Digit2', '2', { ctrlKey: true })), true);
  assert.equal(isShortcut(null, key('KeyC', 'c', { altKey: true })), false);
});
