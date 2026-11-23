// @ts-check
// Canvas and camera: pan and zoom only change the transform of the "world" element that holds every
// frame (real-size iframes). Wheel, pinch, middle-drag and the hand tool move the camera.
import { L, fitRect, nextZoom, wheelCamera, zoomAtPoint } from './geometry.js';
import { $, S, viewport, world, ui } from './state.js';
import { clearSelection, markSection, noteCameraMove, pageBounds, scheduleLoads } from './frames.js';
import { updateHud } from './hud.js';
import { MODE } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

let rafCam = 0;
/** @type {ReturnType<typeof setTimeout> | undefined} */
let movingT;
/** Apply S.cam to the world on the next frame (and remember it for the page). */
export function applyCam() {
  cancelAnimationFrame(rafCam);
  rafCam = requestAnimationFrame(() => {
    const { x, y, z } = S.cam;
    world.style.transform = `translate(${x}px, ${y}px) scale(${z})`;
    world.style.setProperty('--iz', String(1 / z));
    world.classList.toggle('tiny', z < 0.075);
    world.classList.add('moving');
    clearTimeout(movingT);
    movingT = setTimeout(() => world.classList.remove('moving'), 160);
    $('#zoom-label').textContent = TEXT.zoom(z);
    S.cams[S.page] = { ...S.cam };
    updateHud();
    scheduleLoads();
  });
}

/** Zoom to z keeping the viewport point (px, py) in place. */
export function zoomAt(/** @type {number} */ px, /** @type {number} */ py, /** @type {number} */ z) {
  S.cam = zoomAtPoint(S.cam, px, py, z);
  applyCam();
}
/** @returns {[number, number]} */
export const vpSize = () => [viewport.clientWidth, viewport.clientHeight];
export const zoomCenter = (/** @type {number} */ z) => {
  const [w, h] = vpSize();
  zoomAt(w / 2, h / 2, z);
};
export const zoomStep = (/** @type {number} */ dir) => zoomCenter(nextZoom(S.cam.z, dir));

/**
 * @param {import('./geometry.js').Rect} rect
 * @param {{ max?: number, pad?: number }} [opts]
 */
export function zoomToRect(rect, opts = {}) {
  const [vw, vh] = vpSize();
  const cam = fitRect(rect, vw, vh, opts);
  if (!cam) return;
  S.cam = cam;
  applyCam();
}

export function zoomToFit({ initial = false } = {}) {
  const b = pageBounds();
  if (!b) return;
  // Leave room for titles above the first row.
  zoomToRect([b[0], b[1] - 60, b[2], b[3] + 60], { max: 1 });
  if (initial && S.cam.z < 0.2) {
    // Too small to read: like opening a design file, start at the top-left of
    // the first section with its header and first row filling the height.
    const [, vh] = vpSize();
    const sec = S.sections[0];
    const firstH = sec
      ? L.PAD +
        sec.headH +
        L.HEAD_GAP +
        Math.max(0, ...(sec.rows[0]?.frames.map((/** @type {any} */ f) => f.h) || [0]), sec.rows[0]?.gutH || 0) +
        40
      : b[3];
    const z = Math.min(0.5, Math.max(0.12, (vh - 48) / firstH));
    S.cam = { z, x: 32 - b[0] * z, y: 24 - b[1] * z };
    applyCam();
  }
}

export function zoomToSelection() {
  const f = S.frames.get(S.sel?.key || S.selKey);
  if (!f) return zoomToFit();
  if (S.sel?.rect) return zoomToRect([f.x + S.sel.rect[0], f.y + S.sel.rect[1], S.sel.rect[2], S.sel.rect[3]], { max: 4, pad: 120 });
  zoomToRect([f.x, f.y, f.w, f.h], { max: 2 });
}

/**
 * Bring the selected element into view without changing the zoom (a layer picked in the Layers
 * pane may be outside the viewport, e.g. low in a tall frame). Nothing moves when it is visible.
 */
export function revealSelection() {
  const f = S.frames.get(S.sel?.key);
  const r = S.sel?.rect;
  if (!f || !r) return;
  const [vw, vh] = vpSize();
  const { z } = S.cam;
  const m = 48;
  const sx = (f.x + r[0]) * z + S.cam.x;
  const sy = (f.y + r[1]) * z + S.cam.y;
  const sw = r[2] * z;
  const sh = r[3] * z;
  const inside = sx >= m && sy >= m && sx + sw <= vw - m && sy + sh <= vh - m;
  if (inside) return;
  // Centre it on each axis that is out of view; one larger than the viewport starts at the margin.
  const fit = (/** @type {number} */ s, /** @type {number} */ size, /** @type {number} */ room) =>
    s >= m && s + size <= room - m ? 0 : size > room - 2 * m ? m - s : (room - size) / 2 - s;
  S.cam = { ...S.cam, x: S.cam.x + fit(sx, sw, vw), y: S.cam.y + fit(sy, sh, vh) };
  applyCam();
}

/** "All pages": bring a page's section to the top-left. */
export function zoomToSection(/** @type {string} */ id) {
  const b = S.secBounds?.[id];
  if (!b) return;
  const [vw] = vpSize();
  const z = Math.min(1, Math.max(0.05, (vw - 96) / b[2]));
  S.cam = { z, x: 48 - b[0] * z, y: 48 - b[1] * z };
  applyCam();
  markSection(id);
}

/** Wheel from the canvas or forwarded by a frame (x, y in viewport pixels). */
export function onWheel(/** @type {Parameters<typeof wheelCamera>[1]} */ w) {
  S.cam = wheelCamera(S.cam, w);
  noteCameraMove();
  applyCam();
}

export function startPan(/** @type {number} */ sx, /** @type {number} */ sy) {
  S.panning = { sx, sy, x: S.cam.x, y: S.cam.y };
  document.body.classList.add('panning');
}
export function movePan(/** @type {number} */ sx, /** @type {number} */ sy) {
  if (!S.panning) return;
  S.cam.x = S.panning.x + (sx - S.panning.sx);
  S.cam.y = S.panning.y + (sy - S.panning.sy);
  noteCameraMove();
  applyCam();
}
export function endPan() {
  S.panning = null;
  document.body.classList.remove('panning');
}

/** Wheel, pinch (Safari gesture events), middle-drag and hand-tool panning, click on empty canvas. */
export function initCanvas() {
  viewport.addEventListener(
    'wheel',
    (/** @type {WheelEvent} */ e) => {
      e.preventDefault();
      const r = viewport.getBoundingClientRect();
      onWheel({
        dx: e.deltaX,
        dy: e.deltaY,
        dm: e.deltaMode,
        ctrl: e.ctrlKey || e.metaKey,
        shift: e.shiftKey,
        x: e.clientX - r.left,
        y: e.clientY - r.top,
      });
    },
    { passive: false },
  );
  // Safari pinch
  let gestureZ = 1;
  viewport.addEventListener('gesturestart', (/** @type {any} */ e) => {
    e.preventDefault();
    gestureZ = S.cam.z;
  });
  viewport.addEventListener('gesturechange', (/** @type {any} */ e) => {
    e.preventDefault();
    const r = viewport.getBoundingClientRect();
    noteCameraMove();
    zoomAt(e.clientX - r.left, e.clientY - r.top, gestureZ * e.scale);
  });
  viewport.addEventListener('mousedown', (/** @type {MouseEvent} */ e) => {
    if (e.button === 1 || S.mode === MODE.HAND || S.space) {
      e.preventDefault();
      startPan(e.screenX, e.screenY);
      return;
    }
    if (
      e.button === 0 &&
      (e.target === viewport ||
        e.target === world ||
        /** @type {HTMLElement} */ (e.target).matches?.('.page-canvas, .page-deco, .page-frames'))
    ) {
      ui.closeComposer();
      ui.closePop();
      clearSelection();
    }
  });
  window.addEventListener('mousemove', (e) => S.panning && movePan(e.screenX, e.screenY));
  window.addEventListener('mouseup', () => S.panning && endPan());
}
