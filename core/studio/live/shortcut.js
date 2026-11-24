// @ts-check
// The review shortcut of live.js: parsed from data-shortcut ("alt+c", "ctrl+shift+k", "off"), shown
// in the button's title, and matched on keydown by event.code (Option+C types "ç" on macOS).
// Functions of values only, tested in tests/unit/studio-live-shortcut.test.mjs.

import { TEXT } from '#studio/shared/text.js';

/** @typedef {{ key: string, alt: boolean, ctrl: boolean, shift: boolean, meta: boolean }} Shortcut */

/**
 * @param {string | null | undefined} spec "alt+c" by default; "off" means no shortcut (button only)
 * @returns {Shortcut | null}
 */
export function parseShortcut(spec) {
  const s = spec || 'alt+c';
  if (s === 'off') return null;
  const parts = s.toLowerCase().split('+');
  const key = parts.pop();
  return { key, alt: parts.includes('alt'), ctrl: parts.includes('ctrl'), shift: parts.includes('shift'), meta: parts.includes('meta') };
}

/**
 * "Option+C" on a Mac, "Alt+C" elsewhere; '' without a shortcut.
 * @param {Shortcut | null} sc
 * @param {boolean} mac
 */
export function shortcutLabel(sc, mac) {
  if (!sc) return '';
  const names = mac ? TEXT.keyNames.mac : TEXT.keyNames.other;
  return [...['ctrl', 'alt', 'shift', 'meta'].filter((m) => sc[m]).map((m) => names[m]), sc.key.toUpperCase()].join('+');
}

/**
 * Does a keydown match the shortcut? Modifiers must match exactly; the key by event.code, else by event.key.
 * @param {Shortcut | null} sc
 * @param {{ altKey: boolean, ctrlKey: boolean, shiftKey: boolean, metaKey: boolean, code: string, key: string }} e
 */
export function isShortcut(sc, e) {
  return (
    !!sc &&
    e.altKey === sc.alt &&
    e.ctrlKey === sc.ctrl &&
    e.shiftKey === sc.shift &&
    e.metaKey === sc.meta &&
    (e.code === (/\d/.test(sc.key) ? 'Digit' : 'Key') + sc.key.toUpperCase() || String(e.key).toLowerCase() === sc.key)
  );
}
