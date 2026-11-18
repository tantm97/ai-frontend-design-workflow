// @ts-check
// Events into the Studio: messages from the frames' overlay (postMessage) and the server's
// Server-Sent Events (design files changed, the agent is about to edit, comments changed), and the
// initial load of the manifest, state, tokens and components.
import { $, P, S, api, post, store, toast } from './state.js';
import { loadPresent, handleGoto, renderPresentBar } from './present.js';
import { positionFrames, rebuildKeepCam, selectFrame, setPage, softReload } from './frames.js';
import { onWheel, startPan, movePan, endPan, zoomToSelection } from './canvas.js';
import { clearAgent, showAgent, updateHud } from './hud.js';
import { setLayers, renderLayers } from './layers.js';
import { renderInspector } from './inspector.js';
import { edit, layoutOp, layoutScopeOf } from './edits.js';
import { closeComposer, loadComments, openComposer, setTab } from './comments.js';
import { pinsFor, renderPins } from './pins.js';
import { handleKey } from './keys.js';
import { renderToolbar } from './toolbar.js';

/** A frame (or the presenter) loaded its overlay: send it the tool, tokens, pins and selection. */
function onReady(/** @type {any} */ f) {
  if (f.present) {
    P.loaded = true;
    post(P, { type: 'dfw:mode', mode: P.mode });
    post(P, { type: 'dfw:tokens', vars: S.tokens });
    post(P, { type: 'dfw:pins', pins: pinsFor(P) });
    renderPresentBar();
    return;
  }
  clearTimeout(f.softTimer);
  f.loaded = true;
  f.sizeCount = 0;
  f.lastStep = 0;
  f.sizeStop = false;
  post(f, { type: 'dfw:mode', mode: S.mode });
  post(f, { type: 'dfw:tokens', vars: S.tokens });
  post(f, { type: 'dfw:pins', pins: pinsFor(f) });
  post(f, { type: 'dfw:layout', enabled: !f.system && !!f.file, scope: layoutScopeOf(f).scope });
  if (f.pendingVerify) {
    post(f, { type: 'dfw:layout-verify', ...f.pendingVerify });
    f.pendingVerify = null;
  }
  if (f.pendingSelect) {
    post(f, { type: 'dfw:select-id', ...f.pendingSelect });
    f.pendingSelect = null;
  } else if (S.sel?.key === f.key)
    post(f, { type: 'dfw:select-id', dfwId: S.sel.own?.id || S.sel.target?.dfwId, selector: S.sel.target?.selector });
  if (S.selKey === f.key) post(f, { type: 'dfw:layers' });
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
  const h = Math.min(20000, Math.max(f.system ? 900 : f.bp.height, Math.ceil(d.h)));
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
  positionFrames();
  updateHud();
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
  if (d.comment && S.mode === 'comment') {
    S.sel = { key: f.key, ...d };
    updateHud();
    openComposer(S.sel, f, '');
    return;
  }
  S.sel = { key: f.key, ...d };
  if (S.selKey !== f.key) selectFrame(f.key, { keepSel: true });
  if (S.layers?.key !== f.key) post(f, { type: 'dfw:layers' });
  renderLayers();
  renderInspector();
  setTab('inspect', { quiet: true });
  updateHud();
  if (S.zoomOnSelect) {
    S.zoomOnSelect = false;
    zoomToSelection();
  }
}

/**
 * What each overlay message does, by type.
 * @type {Record<string, (f: any, d: any) => void>}
 */
const ON = {
  'dfw:ready': onReady,
  'dfw:size': onSize,
  'dfw:presence': (f, d) => {
    // The agent is about to edit this element (or, without a rect, something in this frame).
    if (d.rect) showAgent(f, 'editing', [{ rect: d.rect, label: d.label }]);
    else if (d.fallback) showAgent(f, 'editing', [{ rect: [0, 0, f.w, f.h], whole: true }]);
  },
  'dfw:changed': (f, d) => {
    if (d.agent && d.items?.length) showAgent(f, 'changed', d.items, d.count);
    else if (S.agent.get(f.key)?.phase === 'editing') clearAgent(f.key);
  },
  'dfw:hover': (f, d) => {
    S.hover = d.rect ? { key: f.key, rect: d.rect, label: d.label, comp: d.comp } : null;
    updateHud();
  },
  'dfw:select': onSelect,
  'dfw:sel-rect': (f, d) => {
    if (S.sel?.key !== f.key) return;
    S.sel.rect = d.rect;
    updateHud();
  },
  'dfw:layers': setLayers,
  'dfw:pin-rects': (f, d) => {
    f.pinRects = d.pins;
    renderPins();
  },
  'dfw:wheel': (f, d) => {
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
  'dfw:pan': (f, d) => {
    if (d.phase === 'start') startPan(d.sx, d.sy);
    else if (d.phase === 'move') movePan(d.sx, d.sy);
    else endPan();
  },
  'dfw:key': (f, d) => handleKey(d),
  'dfw:goto': handleGoto,
  'dfw:edit-text': (f, d) => edit(f, { op: 'text', id: d.id, value: d.text }),
  'dfw:layout-op': layoutOp,
  'dfw:toast': (f, d) => toast(d.message),
  'dfw:layout-conflict': (f, d) => {
    S.layoutConflict = { key: f.key, ...d }; // for tests and the inspector note
  },
  'dfw:focus': () => {
    if (!$('#composer').hidden && S.mode !== 'comment') closeComposer();
  },
};

/** Fetch the manifest, state, tokens and components, then show the page (or rebuild it in place). */
export async function loadAll({ keepCam = false } = {}) {
  const [manifest, st, tokens, comps] = await Promise.all([
    api('/__dfw/api/manifest'),
    api('/__dfw/api/state'),
    api('/__dfw/api/tokens').catch(() => []),
    api('/__dfw/api/components').catch(() => []),
  ]);
  S.manifest = manifest;
  S.pages = manifest.studioPages || [{ id: 'system', title: 'Design System', system: true, screens: [] }];
  S.state = st.state;
  S.tokens = tokens;
  S.components = new Map(comps.map((/** @type {any} */ c) => [c.name, c]));
  const ids = manifest.breakpoints.map((/** @type {any} */ b) => b.id);
  if (S.activeBps) S.activeBps = S.activeBps.filter((/** @type {string} */ id) => ids.includes(id));
  if (!S.activeBps?.length) S.activeBps = null;
  renderToolbar();
  if (keepCam && S.page) rebuildKeepCam();
  else setPage(store.get('page', null) || S.pages.find((p) => !p.system && !p.flow && p.screens.length)?.id || 'system');
  await loadComments();
}

/** A design file changed: reload the manifest, or update the frames that show the file. */
async function onDesign(/** @type {MessageEvent} */ e) {
  const { files = [] } = JSON.parse(e.data);
  if (files.some((/** @type {string} */ f) => /(^|\/)design\.json$/.test(f))) return loadAll({ keepCam: true });
  if (files.some((/** @type {string} */ f) => f.startsWith('tokens/'))) S.tokens = await api('/__dfw/api/tokens').catch(() => S.tokens);
  if (files.some((/** @type {string} */ f) => f.startsWith('system/')))
    S.components = new Map((await api('/__dfw/api/components').catch(() => [])).map((/** @type {any} */ c) => [c.name, c]));
  const screenOnly = files.every(
    (/** @type {string} */ f) => f.endsWith('.html') && !f.startsWith('system/') && !f.startsWith('components/'),
  );
  for (const f of S.frames.values()) {
    if (!f.iframe) continue;
    if (!screenOnly || (f.file && files.includes(f.file))) softReload(f, files);
  }
  if (P.open && (!screenOnly || (P.file && files.includes(P.file)))) loadPresent();
}

/** The agent is about to edit a design file: point at the element in the frames that show it. */
function onPresence(/** @type {MessageEvent} */ e) {
  const { file = '', dfwId = null } = JSON.parse(e.data);
  const component = /^system\/components\/([\w-]+)\.html$/.exec(file)?.[1] || null;
  for (const f of S.frames.values()) {
    if (!f.iframe || !f.loaded) continue;
    if (f.system) {
      if (component) post(f, { type: 'dfw:presence', dfwId: `system.component.${component}` });
      else if (file.startsWith('tokens/')) post(f, { type: 'dfw:presence', dfwId: 'system.colors' });
    } else if (f.file === file) post(f, { type: 'dfw:presence', dfwId, fallback: true });
    else if (component) post(f, { type: 'dfw:presence', component });
  }
}

/** Listen to the frames and to the server. */
export function initEvents() {
  window.addEventListener('message', (e) => {
    const f = S.byWin.get(e.source);
    if (!f) return;
    const d = e.data || {};
    if (Object.hasOwn(ON, d.type)) ON[d.type](f, d);
  });
  const es = new EventSource('/__dfw/events');
  es.addEventListener('design', onDesign);
  es.addEventListener('presence', onPresence);
  es.addEventListener('comments', () => loadComments());
}
