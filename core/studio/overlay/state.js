// @ts-check
// State shared by the overlay modules, and the one way they talk to the Studio.
import { MODE } from '#studio/shared/enums.js';

/**
 * @typedef {{ name: string, var: string, type?: string }} TokenVar a design token and its CSS variable
 */

export const st = {
  /** Studio tool: 'select', 'hand', 'comment', 'interact' or 'present'. */
  mode: /** @type {import('../shared/enums.js').Mode} */ (MODE.SELECT),
  /** @type {HTMLElement | null} */
  hoverEl: null,
  /** @type {{ x: number, y: number, exact: boolean } | null} the last pointer point in this frame (null: outside) */
  pointer: null,
  /** @type {HTMLElement | null} */
  selected: null,
  /** @type {{ el: HTMLElement, id: string, orig: string } | null} text edit in progress */
  editing: null,
  /** A middle-button drag pans the Studio canvas. */
  midPan: false,
  /** @type {TokenVar[]} */
  tokenVars: [],
  /** @type {{ key: string, maps: Record<string, Map<string, string[]>> } | null} */
  tokenCache: null,
  /** @type {HTMLElement[]} layer index -> element */
  layerEls: [],
  /** @type {Map<Element, { node: any, parent: HTMLElement | null }>} */
  layerNode: new Map(),
  /** @type {{ id: string, selector?: string, dfwId?: string }[]} comment pins of this frame */
  pins: [],
  /** Layout edits on (a design file) and the breakpoint scope they apply at. */
  layoutCfg: { enabled: false, scope: '' },
  /** @type {any} drag to reorder in progress: { el, x, y, active, drop, prevOpacity, prevPointer } */
  drag: null,
  /** @type {any} resize in progress: { el, axis, rect, x, y, moved, pick, prop, list, prev } */
  resize: null,
  /** The click that ends a drag must not re-select (reset on the next mousedown). */
  suppressClick: false,
  /** @type {{ key: string, n: number, list: { name: string, var: string, px: number }[] } | null} */
  lengthCache: null,
};

/**
 * Message to the Studio. The Studio serves this page, so its origin is ours ('/'): nothing is
 * delivered to a parent from another origin (javascript.md J6).
 * @param {string} type
 * @param {Record<string, any>} [data]
 */
export const post = (type, data = {}) => parent.postMessage({ type, ...data }, '/');

/** A placeholder of `ui` that says which entry was not wired. */
const notWired = (/** @type {string} */ name) => () => {
  throw new Error(`ui.${name} is not wired (app start-up)`);
};

/**
 * Calls from a lower module to a higher one (the layer order is in tests/unit/imports.test.mjs):
 * the lower module calls `ui.<name>()`, and overlay/main.js fills this table at start-up. A call
 * before that throws, so a missing entry shows at once.
 */
export const ui = {
  /** @type {(...args: any[]) => any} */ editTarget: notWired('editTarget'), // edit.js
  /** @type {(...args: any[]) => any} */ placeHandles: notWired('placeHandles'), // layout.js
};
