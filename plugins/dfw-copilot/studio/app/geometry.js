// @ts-check
// Pure geometry of the Studio canvas: where page sections, rows and frames sit in world units, the
// Flow arrows between frames, and the camera maths (zoom at a point, fit a rectangle, zoom steps).
// Functions of values only (javascript.md J3), tested in tests/unit/studio-app-geometry.test.mjs.

/** @typedef {[number, number, number, number]} Rect x, y, width, height */
/** @typedef {{ x: number, y: number, z: number }} Camera screen = world * z + (x, y) */

/** Canvas layout constants, in world pixels. */
export const L = {
  FLOW_GAP: 480,
  PAD: 96,
  HEAD_GAP: 72,
  ROW_GAP: 240,
  FRAME_GAP: 80,
  GUTTER_NOTES: 420,
  GUTTER: 300,
  GUTTER_GAP: 80,
  SEC_GAP: 480,
};

export const ZMIN = 0.02;
export const ZMAX = 8;
/** Zoom levels of the +/- keys and buttons. */
export const STEPS = [0.02, 0.05, 0.1, 0.15, 0.25, 0.33, 0.5, 0.75, 1, 1.5, 2, 3, 4, 8];

/**
 * @typedef {{ headH: number, headW: number, gutter: number, flow: boolean, rows: { gutH: number, frames: { w: number, h: number }[] }[] }} SectionSize
 * @typedef {{ head: { x: number, y: number }, rows: { gut: { x: number, y: number, w: number }, frames: { x: number, y: number }[] }[], bounds: Rect }} SectionPlace
 */

/**
 * Place page sections top to bottom: a header, then rows of frames left to right after the row gutter.
 * @param {SectionSize[]} sections measured sizes (header, gutters) and frame sizes
 * @returns {SectionPlace[]}
 */
export function layoutSections(sections) {
  let secY = 0;
  return sections.map((sec) => {
    const x0 = 0;
    let y = secY + L.PAD;
    const head = { x: x0 + L.PAD, y };
    y += sec.headH + L.HEAD_GAP;
    let maxW = sec.headW;
    const gap = sec.flow ? L.FLOW_GAP : L.FRAME_GAP;
    const rows = sec.rows.map((r, i) => {
      if (i) y += L.ROW_GAP;
      const fx0 = x0 + L.PAD + (sec.gutter ? sec.gutter + L.GUTTER_GAP : 0);
      const gut = { x: x0 + L.PAD, y, w: sec.gutter };
      let x = fx0;
      let h = r.gutH;
      const frames = r.frames.map((f) => {
        const at = { x, y };
        x += f.w + gap;
        h = Math.max(h, f.h);
        return at;
      });
      maxW = Math.max(maxW, x - gap - x0 - L.PAD);
      y += h;
      return { gut, frames };
    });
    const w = maxW + L.PAD * 2;
    const h = y + L.PAD - secY;
    /** @type {Rect} */
    const bounds = [x0, secY, w, h];
    secY += h + L.SEC_GAP;
    return { head, rows, bounds };
  });
}

/**
 * The rectangle around a list of rectangles, or null for none.
 * @param {Rect[]} rects
 * @returns {Rect | null}
 */
export function boundsOf(rects) {
  if (!rects.length) return null;
  const x0 = Math.min(...rects.map((r) => r[0]));
  const y0 = Math.min(...rects.map((r) => r[1]));
  const x1 = Math.max(...rects.map((r) => r[0] + r[2]));
  const y1 = Math.max(...rects.map((r) => r[1] + r[3]));
  return [x0, y0, x1 - x0, y1 - y0];
}

/**
 * Arrows of the Flow page: one cubic path per link between the frames of two screens. A link back
 * along the same pair runs below the forward one instead of on top of it.
 * @param {{ from: string, to: string, label?: string }[]} flows
 * @param {Map<string, { x: number, y: number, w: number, h: number }>} byScreen first frame of each screen
 * @returns {{ d: string, label?: string, lx: number, ly: number }[]}
 */
export function flowArrows(flows, byScreen) {
  const out = [];
  for (const e of flows) {
    const a = byScreen.get(e.from);
    const z = byScreen.get(e.to);
    if (!a || !z) continue;
    const forward = z.x > a.x || (z.x === a.x && z.y > a.y);
    const lane = flows.some((x) => x.from === e.to && x.to === e.from) ? (forward ? -60 : 60) : 0;
    const x1 = forward ? a.x + a.w : a.x;
    const y1 = a.y + Math.min(a.h, 600) / 2 + lane;
    const x2 = forward ? z.x : z.x + z.w;
    const y2 = z.y + Math.min(z.h, 600) / 2 + lane;
    const bend = Math.max(120, Math.abs(x2 - x1) / 2) * (forward ? 1 : -1);
    out.push({
      d: `M${x1} ${y1} C${x1 + bend} ${y1} ${x2 - bend} ${y2} ${x2} ${y2}`,
      label: e.label,
      lx: (x1 + x2) / 2,
      ly: (y1 + y2) / 2 - 14,
    });
  }
  return out;
}

export const clampZoom = (/** @type {number} */ z) => Math.min(ZMAX, Math.max(ZMIN, z));

/**
 * Camera after zooming to z with the point (px, py) of the viewport kept in place.
 * @param {Camera} cam
 * @param {number} px
 * @param {number} py
 * @param {number} z
 * @returns {Camera}
 */
export function zoomAtPoint(cam, px, py, z) {
  z = clampZoom(z);
  const wx = (px - cam.x) / cam.z;
  const wy = (py - cam.y) / cam.z;
  return { x: px - wx * z, y: py - wy * z, z };
}

/** The next zoom step in a direction (+1 in, -1 out). */
export function nextZoom(/** @type {number} */ z, /** @type {number} */ dir) {
  return dir > 0 ? STEPS.find((s) => s > z + 0.001) || ZMAX : [...STEPS].reverse().find((s) => s < z - 0.001) || ZMIN;
}

/**
 * Camera that centres a world rectangle in a viewport of vw x vh, at most at zoom `max`; null when
 * either has no size.
 * @param {Rect} rect
 * @param {number} vw
 * @param {number} vh
 * @param {{ max?: number, pad?: number }} [opts]
 * @returns {Camera | null}
 */
export function fitRect([x, y, w, h], vw, vh, { max = 1, pad = 64 } = {}) {
  if (!vw || !vh || !w || !h) return null;
  const z = Math.min(max, Math.max(ZMIN, Math.min((vw - pad * 2) / w, (vh - pad * 2) / h)));
  return { z, x: (vw - w * z) / 2 - x * z, y: (vh - h * z) / 2 - y * z };
}

/**
 * Wheel input to a camera change: zoom at the pointer with Ctrl/Cmd (or a pinch), else pan
 * (Shift turns a vertical wheel into a horizontal pan). deltaMode 1 is lines, 2 pages.
 * @param {Camera} cam
 * @param {{ dx: number, dy: number, dm: number, ctrl: boolean, shift: boolean, x: number, y: number }} w
 * @returns {Camera}
 */
export function wheelCamera(cam, { dx, dy, dm, ctrl, shift, x, y }) {
  const k = dm === 1 ? 16 : dm === 2 ? 400 : 1;
  if (ctrl) {
    const d = Math.max(-30, Math.min(30, dy * k));
    return zoomAtPoint(cam, x, y, cam.z * Math.pow(2, -d * 0.012));
  }
  if (shift && !dx) [dx, dy] = [dy, 0];
  return { x: cam.x - dx * k, y: cam.y - dy * k, z: cam.z };
}

/**
 * A rectangle inside a canvas frame (frame viewport pixels) in window coordinates.
 * @param {{ x: number, y: number }} f frame position in the world
 * @param {Rect} r
 * @param {Camera} cam
 * @param {{ left: number, top: number }} vr the canvas element's client rect
 * @returns {Rect}
 */
export function frameToScreen(f, r, cam, vr) {
  const { x, y, z } = cam;
  return [(f.x + r[0]) * z + x + vr.left, (f.y + r[1]) * z + y + vr.top, r[2] * z, r[3] * z];
}
