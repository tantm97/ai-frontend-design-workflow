// @ts-check
// What the frames do next, as a pure choice (no DOM): which frames load, which patch a change they
// missed, and which give their document back so the live frames stay within the budget. Same-origin
// frames run on the Studio's own thread: every live document costs memory and paint time, so only
// the frames near the viewport are worth keeping alive.

/**
 * Most frame documents alive at once, over every kept page. Measured on 196 frames of 30 screens
 * (design.md of studio-many-frames): 48 keeps panning "All pages" smooth and the renderer under
 * about 700 MB.
 */
export const LIVE_BUDGET = 48;
/** Frames loading at once (each load parses and paints a page on the Studio's thread). */
export const MAX_LOADING = 2;
/** Frames patching a changed file at once. */
export const MAX_PATCHING = 3;

/**
 * @typedef {object} FrameFacts
 * @property {string} key for messages and tests (two kept pages can show the same key)
 * @property {boolean} live it has a document (loaded or loading)
 * @property {boolean} near on the page shown, within reach of the viewport
 * @property {boolean} hidden on a kept page that is not shown
 * @property {number} dist distance from the viewport centre (screen px), for frames shown
 * @property {number} age rank of its page among the kept pages, oldest 0 (for frames hidden)
 * @property {boolean} [keep] must stay alive: selected, an edit to verify, a selection to apply
 * @property {boolean} [loading]
 * @property {boolean} [patching]
 * @property {boolean} [stale] a change reached the source while it was off screen or hidden
 */

/**
 * The next work for the frames: nearest the viewport centre first, at most `maxLoading` loads and
 * `maxPatching` patches running at once, and at most `budget` live documents. A load over the
 * budget takes the document of a frame that is not near: a hidden page's first (oldest page
 * first), then the shown frame farthest away. A frame near the viewport is never unloaded to load
 * another, so a viewport with more frames than the budget leaves the rest as placeholders.
 * @template {FrameFacts} F
 * @param {F[]} frames
 * @param {{ budget?: number, maxLoading?: number, maxPatching?: number }} [limits]
 * @returns {{ load: F[], patch: F[], unload: F[] }} the same objects that came in
 */
export function planFrameWork(frames, { budget = LIVE_BUDGET, maxLoading = MAX_LOADING, maxPatching = MAX_PATCHING } = {}) {
  const byDist = (/** @type {F} */ a, /** @type {F} */ b) => a.dist - b.dist;
  const loading = frames.filter((f) => f.loading).length;
  const patching = frames.filter((f) => f.patching).length;
  const patch = frames
    .filter((f) => f.live && f.stale && f.near && !f.hidden && !f.loading && !f.patching)
    .sort(byDist)
    .slice(0, Math.max(0, maxPatching - patching));
  const wanted = frames
    .filter((f) => !f.live && f.near && !f.hidden)
    .sort(byDist)
    .slice(0, Math.max(0, maxLoading - loading));
  const spare = frames
    .filter((f) => f.live && !f.keep && !f.loading && !f.patching && (f.hidden || !f.near))
    .sort((a, b) => Number(b.hidden) - Number(a.hidden) || (a.hidden ? a.age - b.age : b.dist - a.dist));
  let live = frames.filter((f) => f.live).length;
  /** @type {F[]} */
  const load = [];
  /** @type {F[]} */
  const unload = [];
  /** Unload spare documents until at most `limit` are live (or none is spare). */
  const free = (/** @type {number} */ limit) => {
    while (live > limit && spare.length) {
      unload.push(/** @type {F} */ (spare.shift()));
      live--;
    }
  };
  for (const f of wanted) {
    if (live >= budget) free(budget - 1);
    if (live >= budget) break;
    load.push(f);
    live++;
  }
  // Over the budget with nothing to load (after a page switch): the spare documents go too.
  if (live > budget) free(budget);
  return { load, patch, unload };
}

/**
 * Two lists of changed files as one, without repeats (a frame that missed two changes patches once).
 * @param {string[] | null | undefined} a
 * @param {string[]} b
 */
export const mergeFiles = (a, b) => [...new Set([...(a || []), ...b])];
