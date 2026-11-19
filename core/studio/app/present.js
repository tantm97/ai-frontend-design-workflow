// @ts-check
// Present mode: one frame full-window as a clickable prototype (links and data-dfw-goto navigate),
// with breakpoint, state and theme pickers, pins and comments.
import { esc, frameSrc, primaryVariant, statesOf } from './model.js';
import { $, ALL, P, PKEY, S, post, screenById, store, toast } from './state.js';
import { selectFrame, setPage } from './frames.js';
import { zoomToRect } from './canvas.js';
import { updateHud } from './hud.js';
import { closePop, pinsFor, renderPins, syncPins } from './pins.js';
import { closeComposer } from './comments.js';

/** @param {string} href */
function screenForHref(href) {
  const path = decodeURIComponent(String(href).split('?')[0]).replace(/^\/design\//, '');
  for (const s of S.manifest.screens || []) for (const v of s.variants || []) if (v.file === path) return { s, v };
  return null;
}

/** The screen and variant a link (href) or a data-dfw-goto ("screen" or "screen/variant") points at. */
function resolveGoto(/** @type {{ href?: string, goto?: string }} */ d) {
  if (d.href) return screenForHref(d.href);
  const [sid, vid] = String(d.goto || '').split('/');
  const s = screenById(sid);
  if (!s) return null;
  return { s, v: (vid && (s.variants || []).find((x) => x.id === vid)) || primaryVariant(s) };
}

/** Frames of the current canvas in layout order (prev/next in the presenter). */
const presentList = () => [...S.frames.values()];

/** Present a frame (the selected one, else the first). */
export function openPresent(/** @type {any} */ f) {
  f = f || S.frames.get(S.sel?.key || S.selKey) || presentList()[0];
  if (!f) return toast('Nothing to present on this page.');
  P.open = true;
  P.mode = 'present';
  P.system = !!f.system;
  P.screen = f.screen;
  P.variant = f.variant;
  P.bp = f.bp || S.manifest.breakpoints[0];
  P.state = f.state;
  P.file = f.file || null;
  P.key0 = f.key;
  closeComposer();
  $('#present').hidden = false;
  document.body.classList.add('presenting');
  loadPresent();
}

export function closePresent() {
  if (!P.open) return;
  closeComposer();
  closePop();
  P.open = false;
  P.loaded = false;
  P.pinRects = [];
  $('#present').hidden = true;
  document.body.classList.remove('presenting');
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  if (S.hover?.key === PKEY) S.hover = null;
  if (S.sel?.key === PKEY) S.sel = null;
  renderPins();
}

/** URL of what the presenter shows. */
export function presentSrc() {
  if (P.system) return '/__dfw/system.html';
  return frameSrc(P.file, P.state, S.theme);
}

/** (Re)load the presenter's iframe. */
export function loadPresent() {
  P.loaded = false;
  P.pinRects = [];
  const old = P.iframe;
  const iframe = document.createElement('iframe');
  iframe.id = 'p-frame';
  iframe.name = PKEY;
  iframe.title = 'Prototype';
  iframe.src = presentSrc();
  if (old) {
    S.byWin.delete(old.contentWindow);
    old.replaceWith(iframe);
  } else $('#p-wrap').append(iframe);
  P.iframe = iframe;
  S.byWin.set(iframe.contentWindow, P);
  layoutPresent();
  renderPresentBar();
  renderPins();
}

/** Scale the presented frame to the stage width ("Fit width") or show it at 100%. */
export function layoutPresent() {
  if (!P.open) return;
  const stage = $('#p-stage');
  const sw = stage.clientWidth;
  const sh = stage.clientHeight;
  const w = P.system ? 1440 : P.bp.width;
  P.scale = P.fit ? Math.min(1, (sw - 48) / w) : 1;
  const wrap = $('#p-wrap');
  wrap.style.width = `${Math.round(w * P.scale)}px`;
  wrap.style.height = `${sh}px`;
  P.iframe.style.width = `${w}px`;
  P.iframe.style.height = `${Math.round(sh / P.scale)}px`;
  P.iframe.style.transform = P.scale === 1 ? '' : `scale(${P.scale})`;
  $('#p-fit').textContent = P.fit ? 'Fit width' : '100%';
  $('#p-fit').setAttribute('aria-pressed', String(P.fit));
  updateHud();
}

/** Does canvas frame f show what the presenter shows now? (ids, not objects: a live reload rebuilds them) */
const presentMatches = (/** @type {any} */ f) =>
  f.system
    ? P.system
    : !P.system &&
      f.screen?.id === P.screen?.id &&
      f.variant?.id === P.variant?.id &&
      f.bp?.id === P.bp?.id &&
      (f.state || null) === (P.state || null);

function presentIndex() {
  const list = presentList();
  // The same screen can sit on several rows (sections, All pages): the key decides, attributes only
  // when the user switched breakpoint/state in the presenter bar.
  const k = list.findIndex((f) => f.key === P.key0);
  if (k >= 0 && presentMatches(list[k])) return k;
  const i = list.findIndex(presentMatches);
  return i >= 0 ? i : k;
}

export function renderPresentBar() {
  const list = presentList();
  const i = presentIndex();
  const s = P.screen;
  $('#p-title').textContent = P.system
    ? 'Design System'
    : s
      ? `${s.title || s.id}${(s.variants || []).length > 1 ? ` · ${P.variant?.label || P.variant?.id}` : ''}`
      : P.file || '';
  $('#p-count').textContent = i >= 0 ? `${i + 1} / ${list.length}` : `– / ${list.length}`;
  const bpSel = $('#p-bp');
  bpSel.innerHTML = S.manifest.breakpoints
    .map(
      (/** @type {any} */ b) =>
        `<option value="${esc(b.id)}" ${b.id === P.bp?.id ? 'selected' : ''}>${esc(b.label || b.id)} ${esc(b.width)}</option>`,
    )
    .join('');
  bpSel.disabled = P.system;
  const states = s ? statesOf(S.manifest, s) : [];
  $('#p-state').innerHTML =
    `<option value="">default</option>` + states.map((x) => `<option ${x === P.state ? 'selected' : ''}>${esc(x)}</option>`).join('');
  $('#p-state').disabled = !states.length || P.system;
  const themes = S.manifest.themes || [];
  $('#p-theme-wrap').hidden = !themes.length;
  $('#p-theme').innerHTML =
    `<option value="">default</option>` +
    themes.map((/** @type {string} */ t) => `<option ${t === S.theme ? 'selected' : ''}>${esc(t)}</option>`).join('');
  $('#p-open').href = presentSrc();
  $('#p-comment').setAttribute('aria-pressed', String(P.mode === 'comment'));
  $('#p-pins').setAttribute('aria-pressed', String(P.pins));
}

/** Previous (-1) or next (+1) frame of the canvas, wrapping. */
export function presentStep(/** @type {number} */ dir) {
  const list = presentList();
  if (!list.length) return;
  closePop();
  const i = presentIndex();
  const f = list[(Math.max(0, i) + dir + list.length) % list.length];
  P.system = !!f.system;
  P.screen = f.screen;
  P.variant = f.variant;
  P.bp = f.bp || P.bp;
  P.state = f.state;
  P.file = f.file || null;
  P.key0 = f.key;
  loadPresent();
}

/** 'present' (click through the prototype) or 'comment'. */
export function setPresentMode(/** @type {string} */ mode) {
  P.mode = mode;
  if (mode !== 'comment') closeComposer();
  if (mode !== 'comment' && !P.pins) closePop();
  if (P.loaded) post(P, { type: 'dfw:pins', pins: pinsFor(P) });
  if (P.loaded) post(P, { type: 'dfw:mode', mode });
  renderPresentBar();
}

export function togglePresentPins() {
  P.pins = !P.pins;
  store.set('ppins', P.pins);
  if (!P.pins) closePop();
  renderPresentBar();
  syncPins();
}

/**
 * Prototype navigation from a frame (link to another design file or data-dfw-goto): the presenter
 * follows it; on the canvas the target screen's frame is selected and shown.
 * @param {any} f
 * @param {{ href?: string, goto?: string }} d
 */
export function handleGoto(f, d) {
  const t = resolveGoto(d);
  if (f.present) {
    if (!t) {
      if (d.href) {
        P.screen = null;
        P.variant = null;
        P.file = decodeURIComponent(d.href.split('?')[0]).replace(/^\/design\//, '');
        P.state = null;
        P.system = false;
        return loadPresent();
      }
      return toast(`No screen "${d.goto}".`);
    }
    P.system = false;
    P.screen = t.s;
    P.variant = t.v;
    P.file = t.v.file;
    P.state = null;
    return loadPresent();
  }
  // On the canvas: jump to the target screen's frame (same breakpoint when possible).
  if (!t) return toast(`Link target is not a screen in design.json: ${d.href || d.goto}`);
  const pick = () => {
    const cands = [...S.frames.values()].filter((x) => x.screen === t.s && !x.state);
    return cands.find((x) => x.variant === t.v && x.bp?.id === f.bp?.id) || cands.find((x) => x.variant === t.v) || cands[0];
  };
  let target = pick();
  if (!target && S.page !== ALL) {
    const page = S.pages.find((p) => p.screens.includes(t.s.id));
    if (page) {
      setPage(page.id);
      target = pick();
    }
  }
  if (!target) return toast(`${t.s.title || t.s.id} is not shown with the current breakpoints.`);
  selectFrame(target.key);
  zoomToRect([target.x, target.y, target.w, target.h], { max: S.cam.z > 0.5 ? S.cam.z : 0.5 });
  flash(target);
}

/** Flash a frame's outline (after jumping to it). */
export function flash(/** @type {any} */ f) {
  f.el.classList.remove('flash');
  void f.el.offsetWidth;
  f.el.classList.add('flash');
}

export function toggleFullscreen() {
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  else document.documentElement.requestFullscreen?.().catch(() => toast('Fullscreen is not available here.'));
}

/** Keep the presenter's position in the frame list after a breakpoint or state change. */
function syncPresentKey() {
  const f = presentList().find(presentMatches);
  if (f) P.key0 = f.key;
}

/** The presenter bar's buttons and pickers. */
export function initPresent() {
  $('#p-close').onclick = closePresent;
  $('#p-prev').onclick = () => presentStep(-1);
  $('#p-next').onclick = () => presentStep(1);
  $('#p-fit').onclick = () => {
    P.fit = !P.fit;
    store.set('pfit', P.fit);
    layoutPresent();
  };
  $('#p-full').onclick = toggleFullscreen;
  $('#p-comment').onclick = () => setPresentMode(P.mode === 'comment' ? 'present' : 'comment');
  $('#p-pins').onclick = () => togglePresentPins();
  $('#p-bp').onchange = (/** @type {any} */ e) => {
    P.bp = S.manifest.breakpoints.find((/** @type {any} */ b) => b.id === e.target.value) || P.bp;
    syncPresentKey();
    loadPresent();
  };
  $('#p-state').onchange = (/** @type {any} */ e) => {
    P.state = e.target.value || null;
    syncPresentKey();
    loadPresent();
  };
  $('#p-theme').onchange = (/** @type {any} */ e) => {
    S.theme = e.target.value;
    store.set('theme', S.theme);
    loadPresent();
  };
  $('#present-btn').onclick = () => openPresent();
  $('#p-stage').addEventListener('scroll', updateHud);
}
