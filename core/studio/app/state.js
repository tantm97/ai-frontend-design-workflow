// @ts-check
// Shared state of the Studio and the small helpers every feature module uses: DOM lookup, local
// settings, the server API, the toast, and messages to the frames.
import { pageModel as modelOf, screenById as findScreen } from './model.js';

/**
 * querySelector with a loose result type: the Studio's own markup (app.html) is known.
 * @param {string} s
 * @param {ParentNode} [el]
 * @returns {any}
 */
export const $ = (s, el = document) => el.querySelector(s);
/**
 * @param {string} s
 * @param {ParentNode} [el]
 * @returns {any[]}
 */
export const $$ = (s, el = document) => [...el.querySelectorAll(s)];

/** Per-viewer settings in localStorage (`dfw:<key>`); never fails (private windows, blocked storage). */
export const store = {
  /**
   * @param {string} k
   * @param {any} d
   */
  get(k, d) {
    try {
      const v = localStorage.getItem(`dfw:${k}`);
      return v == null ? d : JSON.parse(v);
    } catch {
      return d;
    }
  },
  /**
   * @param {string} k
   * @param {any} v
   */
  set(k, v) {
    try {
      localStorage.setItem(`dfw:${k}`, JSON.stringify(v));
    } catch {}
  },
};

/**
 * The Studio state (also `window.__dfwStudio`, for debugging and the e2e test).
 * @type {Record<string, any>}
 */
export const S = {
  agent: new Map(), // frame key -> what the agent is editing / just changed there
  agentLast: null,
  selfEditUntil: 0,
  manifest: null,
  pages: [],
  state: null,
  comments: [],
  tokens: [],
  components: new Map(),
  page: null,
  mode: 'select',
  cam: { x: 0, y: 0, z: 0.25 },
  cams: {},
  activeBps: store.get('bps3', null), // null = all
  activeStates: store.get('states3', null), // null = all
  fullHeight: store.get('full2', true), // frames grow to their content height by default; the toggle crops to the viewport
  theme: store.get('theme', ''),
  frames: new Map(), // key -> frame
  byWin: new Map(), // contentWindow -> frame
  rows: [],
  selKey: null, // selected frame
  sel: null, // selected element payload {key, ...}
  hover: null,
  layers: null, // {key, tree, parents}
  layerOpen: new Map(),
  filter: 'active',
  selectedComment: null,
  pending: null,
  space: false,
  panning: null,
  zoomOnSelect: false,
  // Comment pins on the canvas: 'active' (open + to verify), 'all' (also closed) or 'off'.
  pins: store.get('pins', 'active'),
  pop: null, // comment id shown in the pin popover
};

export const ALL = '__all';
export const PKEY = '__present';
/**
 * Presenter state: a pseudo-frame shown full-window.
 * @type {Record<string, any>}
 */
export const P = {
  present: true,
  key: PKEY,
  open: false,
  loaded: false,
  pins: store.get('ppins', false),
  fit: store.get('pfit', true),
  scale: 1,
  screen: null,
  variant: null,
  bp: null,
  state: null,
  file: null,
  system: false,
  pinRects: [],
};

export const viewport = $('#viewport');
export const world = $('#world');
export const hud = $('#hud');
/** The canvas element's client rect, cached until a resize. */
export const view = { /** @type {DOMRect | null} */ rect: null };
export const viewportRect = () => view.rect || (view.rect = viewport.getBoundingClientRect());

/**
 * JSON call to the Studio server; a non-2xx answer throws its `error`.
 * @param {string} path
 * @param {RequestInit} [opts]
 * @returns {Promise<any>}
 */
export async function api(path, opts = {}) {
  const res = await fetch(path, { headers: { 'Content-Type': 'application/json' }, ...opts });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

/** @type {ReturnType<typeof setTimeout> | undefined} */
let toastTimer;
/** Show a short message at the bottom of the window. */
export function toast(/** @type {string} */ msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => (t.hidden = true), 3200);
}

/** The toolbar choices the page model depends on. */
export const viewContext = () => ({
  manifest: S.manifest,
  activeBps: S.activeBps,
  activeStates: S.activeStates,
  fullHeight: S.fullHeight,
  theme: S.theme,
});
export const pageModel = (/** @type {any} */ page) => modelOf(viewContext(), page);
export const screenById = (/** @type {string} */ id) => findScreen(S.manifest, id);
export const currentPage = () =>
  S.page === ALL ? { id: ALL, title: 'All pages', screens: [], all: true } : S.pages.find((p) => p.id === S.page) || S.pages[0];

/**
 * Message to the overlay of a frame. Frames show /design/ and /__dfw/ pages of this Studio, so the
 * target origin is ours ('/'): a frame that navigated elsewhere gets nothing (javascript.md J6).
 * @param {any} f a frame, or the presenter
 * @param {Record<string, any>} msg
 */
export const post = (f, msg) => f?.iframe?.contentWindow?.postMessage(msg, '/');
export const postAll = (/** @type {Record<string, any>} */ msg) => S.frames.forEach((f) => f.loaded && post(f, msg));

/** Frames by key, including the presenter's pseudo-frame. */
export const frameByKey = (/** @type {string} */ k) => (k === PKEY ? (P.open ? P : null) : S.frames.get(k));
/** Frames whose chrome is visible right now: the presenter hides the canvas. */
export const visibleFrame = (/** @type {any} */ f) => !!f && (P.open ? f === P : !f.present);
