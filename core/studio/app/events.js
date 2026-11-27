// @ts-check
// Events into the Studio: messages from the frames' overlay (postMessage) and the server's
// Server-Sent Events (design files changed, the agent is about to edit, comments changed), and the
// initial load of the manifest, state, tokens and components.
import { $, P, S, post, store, toast } from './state.js';
import { loadPresent, handleGoto, renderPresentBar } from './present.js';
import { isShown, liveFrames, patchDone, queuePatches, rebuildKeepCam, schedulePositions, selectFrame, setPage } from './frames.js';
import { onWheel, startPan, movePan, endPan, revealSelection, zoomToSelection } from './canvas.js';
import { clearAgent, showAgent, updateHud } from './hud.js';
import { setLayers, renderLayers } from './layers.js';
import { renderInspector } from './inspector.js';
import { edit, layoutOp, layoutScopeOf } from './edits.js';
import { closeComposer, loadComments, openComposer, setTab } from './comments.js';
import { pinsFor, renderPins } from './pins.js';
import { handleKey } from './keys.js';
import { renderToolbar } from './toolbar.js';
import { frameHeight } from './geometry.js';
import { MSG, SSE } from '#studio/shared/protocol.js';
import { events, server } from './api.js';
import { MODE, TAB, EDIT_OP, PHASE, AGENT_PHASE, SYSTEM_SCREEN } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

/** A frame (or the presenter) loaded its overlay: send it the tool, tokens, pins and selection. */
function onReady(/** @type {any} */ f) {
  if (f.present) {
    P.loaded = true;
    post(P, { type: MSG.MODE, mode: P.mode });
    post(P, { type: MSG.TOKENS, vars: S.tokens });
    post(P, { type: MSG.PINS, pins: pinsFor(P) });
    renderPresentBar();
    return;
  }
  clearTimeout(f.softTimer);
  patchDone(f);
  f.loaded = true;
  f.sizeCount = 0;
  f.lastStep = 0;
  f.sizeStop = false;
  post(f, { type: MSG.MODE, mode: S.mode });
  post(f, { type: MSG.TOKENS, vars: S.tokens });
  post(f, { type: MSG.PINS, pins: pinsFor(f) });
  post(f, { type: MSG.LAYOUT, enabled: !f.system && !!f.file, scope: layoutScopeOf(f).scope });
  if (f.pendingVerify) {
    post(f, { type: MSG.LAYOUT_VERIFY, ...f.pendingVerify });
    f.pendingVerify = null;
  }
  if (f.pendingSelect) {
    post(f, { type: MSG.SELECT_ID, ...f.pendingSelect });
    f.pendingSelect = null;
  } else if (S.sel?.key === f.key)
    post(f, { type: MSG.SELECT_ID, dfwId: S.sel.own?.id || S.sel.target?.dfwId, selector: S.sel.target?.selector });
  if (S.selKey === f.key) post(f, { type: MSG.LAYERS });
}

/**
 * Grow the frame to its content. Only real changes count toward the limit (fonts, images and the
 * ResizeObserver report many times). A section sized in vh grows with the frame by the same amount
 * every round: two equal growth steps in a row mean a vh feedback loop, so the frame stops there.
 * @param {any} f
 * @param {{ h?: number }} d
 */
function onSize(f, d) {
  if (!f.auto || !d.h || f.sizeStop) return;
  const h = frameHeight(d.h, f.system ? 900 : f.bp.height);
  if (Math.abs(h - f.h) <= 1) return;
  const step = h - f.h;
  if (step > 0 && f.lastStep && Math.abs(step - f.lastStep) <= 2) {
    f.sizeStop = true;
    return;
  }
  f.lastStep = step > 0 ? step : 0;
  if (++f.sizeCount > 24) {
    f.sizeStop = true;
    return;
  }
  f.h = h;
  // A frame of a kept, hidden page is placed when that page shows again.
  if (!isShown(f)) return;
  // While a page loads, many frames report a size in one animation frame: place them once.
  schedulePositions();
}

/** An element was clicked in a frame: select it, or open the composer in comment mode. */
function onSelect(/** @type {any} */ f, /** @type {any} */ d) {
  if (!d.target) return;
  if (f.present) {
    if (d.comment) {
      S.sel = { key: f.key, ...d };
      updateHud();
      openComposer(S.sel, f, '');
    }
    return;
  }
  if (d.comment && S.mode === MODE.COMMENT) {
    S.sel = { key: f.key, ...d };
    updateHud();
    openComposer(S.sel, f, '');
    return;
  }
  S.sel = { key: f.key, ...d };
  if (S.selKey !== f.key) selectFrame(f.key, { keepSel: true });
  if (S.layers?.key !== f.key) post(f, { type: MSG.LAYERS });
  renderLayers();
  renderInspector();
  setTab(TAB.INSPECT, { quiet: true });
  updateHud();
  if (S.zoomOnSelect) {
    S.zoomOnSelect = false;
    S.revealOnSelect = false;
    zoomToSelection();
  } else if (S.revealOnSelect) {
    S.revealOnSelect = false;
    revealSelection();
  }
}

/**
 * What each overlay message does, by type.
 * @type {Record<string, (f: any, d: any) => void>}
 */
const ON = {
  [MSG.READY]: onReady,
  [MSG.SIZE]: onSize,
  [MSG.PRESENCE]: (f, d) => {
    // The agent is about to edit this element (or, without a rect, something in this frame).
    if (d.rect) showAgent(f, AGENT_PHASE.EDITING, [{ rect: d.rect, label: d.label }]);
    else if (d.fallback) showAgent(f, AGENT_PHASE.EDITING, [{ rect: [0, 0, f.w, f.h], whole: true }]);
  },
  [MSG.CHANGED]: (f, d) => {
    if (d.agent && d.items?.length) showAgent(f, AGENT_PHASE.CHANGED, d.items, d.count);
    else if (S.agent.get(f.key)?.phase === AGENT_PHASE.EDITING) clearAgent(f.key);
  },
  [MSG.HOVER]: (f, d) => {
    S.hover = d.rect ? { key: f.key, rect: d.rect, label: d.label, comp: d.comp } : null;
    updateHud();
  },
  [MSG.SELECT]: onSelect,
  [MSG.SEL_RECT]: (f, d) => {
    if (S.sel?.key !== f.key) return;
    S.sel.rect = d.rect;
    updateHud();
  },
  [MSG.LAYERS]: setLayers,
  [MSG.PIN_RECTS]: (f, d) => {
    f.pinRects = d.pins;
    renderPins();
  },
  [MSG.WHEEL]: (f, d) => {
    if (f.present) return;
    onWheel({
      dx: d.dx,
      dy: d.dy,
      dm: d.dm,
      ctrl: d.ctrl,
      shift: d.shift,
      x: (f.x + d.x) * S.cam.z + S.cam.x,
      y: (f.y + d.y) * S.cam.z + S.cam.y,
    });
  },
  [MSG.PAN]: (f, d) => {
    if (d.phase === PHASE.START) startPan(d.sx, d.sy);
    else if (d.phase === PHASE.MOVE) movePan(d.sx, d.sy);
    else endPan();
  },
  [MSG.KEY]: (f, d) => handleKey(d),
  [MSG.GOTO]: handleGoto,
  [MSG.EDIT_TEXT]: (f, d) => edit(f, { op: EDIT_OP.TEXT, id: d.id, value: d.text }),
  [MSG.LAYOUT_OP]: layoutOp,
  [MSG.TOAST]: (f, d) => toast(d.message),
  [MSG.LAYOUT_CONFLICT]: (f, d) => {
    S.layoutConflict = { key: f.key, ...d }; // for tests and the inspector note
  },
  [MSG.FOCUS]: () => {
    if (!$('#composer').hidden && S.mode !== MODE.COMMENT) closeComposer();
  },
};

/** Fetch the manifest, state, tokens and components, then show the page (or rebuild it in place). */
export async function loadAll({ keepCam = false } = {}) {
  const [manifest, st, tokens, comps] = await Promise.all([
    server.manifest(),
    server.state(),
    server.tokens().catch(() => []),
    server.components().catch(() => []),
  ]);
  S.manifest = manifest;
  S.pages = manifest.studioPages || [{ id: SYSTEM_SCREEN, title: TEXT.label.designSystem, system: true, screens: [] }];
  S.state = st.state;
  S.tokens = tokens;
  S.components = new Map(comps.map((/** @type {any} */ c) => [c.name, c]));
  const ids = manifest.breakpoints.map((/** @type {any} */ b) => b.id);
  if (S.activeBps) S.activeBps = S.activeBps.filter((/** @type {string} */ id) => ids.includes(id));
  if (!S.activeBps?.length) S.activeBps = null;
  renderToolbar();
  if (keepCam && S.page) rebuildKeepCam();
  else setPage(store.get('page', null) || S.pages.find((p) => !p.system && !p.flow && p.screens.length)?.id || SYSTEM_SCREEN);
  await loadComments();
}

/** A design file changed: reload the manifest, or update the frames that show the file. */
async function onDesign(/** @type {MessageEvent} */ e) {
  const { files = [] } = JSON.parse(e.data);
  if (files.some((/** @type {string} */ f) => /(^|\/)design\.json$/.test(f))) return loadAll({ keepCam: true });
  if (files.some((/** @type {string} */ f) => f.startsWith('tokens/'))) S.tokens = await server.tokens().catch(() => S.tokens);
  if (files.some((/** @type {string} */ f) => f.startsWith('system/')))
    S.components = new Map((await server.components().catch(() => [])).map((/** @type {any} */ c) => [c.name, c]));
  const screenOnly = files.every(
    (/** @type {string} */ f) => f.endsWith('.html') && !f.startsWith('system/') && !f.startsWith('components/'),
  );
  // Kept pages are patched too (when shown again), so going back to one never shows an old version.
  queuePatches(
    liveFrames().filter((f) => !screenOnly || (f.file && files.includes(f.file))),
    files,
  );
  if (P.open && (!screenOnly || (P.file && files.includes(P.file)))) loadPresent();
}

/** The agent is about to edit a design file: point at the element in the frames that show it. */
function onPresence(/** @type {MessageEvent} */ e) {
  const { file = '', dfwId = null } = JSON.parse(e.data);
  const component = /^system\/components\/([\w-]+)\.html$/.exec(file)?.[1] || null;
  for (const f of S.frames.values()) {
    if (!f.iframe || !f.loaded) continue;
    if (f.system) {
      if (component) post(f, { type: MSG.PRESENCE, dfwId: `system.component.${component}` });
      else if (file.startsWith('tokens/')) post(f, { type: MSG.PRESENCE, dfwId: 'system.colors' });
    } else if (f.file === file) post(f, { type: MSG.PRESENCE, dfwId, fallback: true });
    else if (component) post(f, { type: MSG.PRESENCE, component });
  }
}

/** Listen to the frames and to the server. */
export function initEvents() {
  window.addEventListener('message', (e) => {
    // Only our own frames (same origin, a window we created) are heard (javascript.md J6).
    if (e.origin !== location.origin) return;
    const f = S.byWin.get(e.source);
    if (!f) return;
    const d = e.data || {};
    if (Object.hasOwn(ON, d.type)) ON[d.type](f, d);
  });
  const es = events();
  es.addEventListener(SSE.DESIGN, onDesign);
  es.addEventListener(SSE.PRESENCE, onPresence);
  es.addEventListener(SSE.COMMENTS, () => loadComments());
}
