// @ts-check
// The messages between the Studio page (app/) and the overlay inside every frame (overlay/,
// live/), by postMessage. Every name is defined here once and imported by both sides: a name
// written as a string anywhere else is refused by the lint (eslint.config.js,
// docs/conventions/javascript.md). Payloads are described next to each name.

/**
 * Message names. "to frame": the Studio sends it to an overlay; "to studio": an overlay sends it
 * to the Studio.
 */
export const MSG = Object.freeze({
  // to frame
  /** to frame: the tool changed `{ mode }`. */
  MODE: 'dfw:mode',
  /** to frame: the design tokens `{ vars }`, for the inspector's token names. */
  TOKENS: 'dfw:tokens',
  /** to frame: the comment pins of this frame `{ pins }`. */
  PINS: 'dfw:pins',
  /** to frame: whether drag/resize editing is on `{ enabled, scope }`. */
  LAYOUT: 'dfw:layout',
  /** to frame: a key for the layout editor `{ key, shift }`. */
  LAYOUT_KEY: 'dfw:layout-key',
  /** to frame: check a saved layout edit `{ id, ... }`. */
  LAYOUT_VERIFY: 'dfw:layout-verify',
  /** to frame: hover the layer at index `{ i }`. */
  LAYER_HOVER: 'dfw:layer-hover',
  /** to frame: select the layer at index `{ i }`. */
  LAYER_SELECT: 'dfw:layer-select',
  /** to frame: select an element `{ dfwId, selector }`. */
  SELECT_ID: 'dfw:select-id',
  /** to frame: drop the selection. */
  CLEAR_SELECTION: 'dfw:clear-selection',
  /** to frame: re-read the source and patch the page in place `{ files, agent }`. */
  SOFT_RELOAD: 'dfw:soft-reload',
  /** to frame: start editing the selected text. */
  EDIT: 'dfw:edit',
  /** to frame: move the selection `{ dir }`. */
  NAV: 'dfw:nav',
  // both ways
  /** to frame: send the layer tree; to studio: the tree `{ tree, selected }`. */
  LAYERS: 'dfw:layers',
  /** to frame: where the agent is about to edit `{ dfwId | component, fallback }`; to studio: its box `{ rect, label }`. */
  PRESENCE: 'dfw:presence',
  // to studio
  /** to studio: the overlay is ready (after a load or a soft reload). */
  READY: 'dfw:ready',
  /** to studio: the document height `{ h }`. */
  SIZE: 'dfw:size',
  /** to studio: what a soft reload changed `{ agent, count, items }`. */
  CHANGED: 'dfw:changed',
  /** to studio: the hovered element `{ rect, label, comp }`. */
  HOVER: 'dfw:hover',
  /** to studio: an element was clicked `{ target, rect, own, comment, ... }`. */
  SELECT: 'dfw:select',
  /** to studio: the selected element moved `{ rect }`. */
  SEL_RECT: 'dfw:sel-rect',
  /** to studio: where the pins are drawn `{ pins }`. */
  PIN_RECTS: 'dfw:pin-rects',
  /** to studio: a wheel over the frame `{ dx, dy, dm, ctrl, shift, x, y }`. */
  WHEEL: 'dfw:wheel',
  /** to studio: a pan gesture `{ phase, sx, sy }`. */
  PAN: 'dfw:pan',
  /** to studio: a key pressed in the frame (Studio shortcuts). */
  KEY: 'dfw:key',
  /** to studio: a prototype link `{ to }`. */
  GOTO: 'dfw:goto',
  /** to studio: edited text to save `{ id, text }`. */
  EDIT_TEXT: 'dfw:edit-text',
  /** to studio: a drag/resize result to save. */
  LAYOUT_OP: 'dfw:layout-op',
  /** to studio: a layout edit conflicts with the source. */
  LAYOUT_CONFLICT: 'dfw:layout-conflict',
  /** to studio: show a short message `{ message }`. */
  TOAST: 'dfw:toast',
  /** to studio: the frame took the keyboard focus. */
  FOCUS: 'dfw:focus',
});

/** The Server-Sent Events of the Studio server (`PATH.EVENTS`). */
export const SSE = Object.freeze({
  /** Design files changed `{ files }`. */
  DESIGN: 'design',
  /** The agent is about to edit `{ file, dfwId }`. */
  PRESENCE: 'presence',
  /** Review comments changed. */
  COMMENTS: 'comments',
});

/** The DOM event the live-review overlay fires inside the app's own page when a comment is saved. */
export const LIVE_COMMENT_EVENT = 'dfw:comment';
