// @ts-check
// What counts as "the user is typing in a field" for the injected scripts: shortcuts never fire
// there (studio-ui.md U7). One definition for the overlay of the design frames and for live.js.

/** Elements that take typed text. `contenteditable="false"` switches editing off, so it is not one. */
export const EDITABLE_FIELD = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';

/**
 * Is a key event's target a field the user types in (itself or inside one, or editable by inheritance)?
 * @param {any} t the event target (in a shadow root: the first node of the composed path)
 */
export const isTypingTarget = (t) =>
  !!t && (t.isContentEditable === true || (typeof t.closest === 'function' && !!t.closest(EDITABLE_FIELD)));
