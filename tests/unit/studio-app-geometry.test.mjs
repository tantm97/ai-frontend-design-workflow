// Canvas geometry of the Studio (core/studio/app/geometry.js): section and frame placement, Flow
// arrows and the camera maths.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import {
  L,
  ZMAX,
  ZMIN,
  boundsOf,
  fitRect,
  flowArrows,
  frameToScreen,
  layoutSections,
  nextZoom,
  wheelCamera,
  zoomAtPoint,
} from '../../core/studio/app/geometry.js';

test('studio-app-geometry: sections stack, rows stack, frames go left to right after the gutter', () => {
  const [a, b] = layoutSections([
    {
      headH: 100,
      headW: 500,
      gutter: 300,
      flow: false,
      rows: [
        {
          gutH: 50,
          frames: [
            { w: 1440, h: 900 },
            { w: 390, h: 1200 },
          ],
        },
        { gutH: 0, frames: [{ w: 800, h: 600 }] },
      ],
    },
    {
      headH: 80,
      headW: 200,
      gutter: 0,
      flow: true,
      rows: [
        {
          gutH: 0,
          frames: [
            { w: 100, h: 100 },
            { w: 100, h: 100 },
          ],
        },
      ],
    },
  ]);
  const fx = L.PAD + 300 + L.GUTTER_GAP;
  const y0 = L.PAD + 100 + L.HEAD_GAP;
  assert.deepEqual(a.head, { x: L.PAD, y: L.PAD });
  assert.deepEqual(a.rows[0].frames, [
    { x: fx, y: y0 },
    { x: fx + 1440 + L.FRAME_GAP, y: y0 },
  ]);
  assert.deepEqual(a.rows[1].frames, [{ x: fx, y: y0 + 1200 + L.ROW_GAP }], 'the next row starts below the tallest frame');
  assert.deepEqual(a.rows[0].gut, { x: L.PAD, y: y0, w: 300 });
  const w = fx + 1440 + L.FRAME_GAP + 390 - L.PAD + L.PAD * 2;
  const h = y0 + 1200 + L.ROW_GAP + 600 + L.PAD;
  assert.deepEqual(a.bounds, [0, 0, w, h]);
  assert.equal(b.bounds[1], h + L.SEC_GAP, 'sections stack with a gap');
  assert.equal(b.rows[0].frames[1].x, L.PAD + 100 + L.FLOW_GAP, 'Flow frames are further apart');
});

test('studio-app-geometry: bounds of rectangles', () => {
  assert.equal(boundsOf([]), null);
  assert.deepEqual(
    boundsOf([
      [0, 0, 10, 10],
      [20, 5, 10, 30],
    ]),
    [0, 0, 30, 35],
  );
});

test('studio-app-geometry: a Flow arrow per link; a link back runs in its own lane', () => {
  const byScreen = new Map([
    ['a', { x: 0, y: 0, w: 100, h: 100 }],
    ['b', { x: 500, y: 0, w: 100, h: 100 }],
  ]);
  const [one] = flowArrows(
    [
      { from: 'a', to: 'b', label: 'Go' },
      { from: 'a', to: 'x' },
    ],
    byScreen,
  );
  assert.equal(one.d, 'M100 50 C300 50 300 50 500 50');
  assert.deepEqual([one.label, one.lx, one.ly], ['Go', 300, 36]);
  const both = flowArrows(
    [
      { from: 'a', to: 'b' },
      { from: 'b', to: 'a' },
    ],
    byScreen,
  );
  assert.equal(both[0].d, 'M100 -10 C300 -10 300 -10 500 -10', 'forward link above');
  assert.equal(both[1].d, 'M500 110 C300 110 300 110 100 110', 'back link below');
});

test('studio-app-geometry: zooming keeps the point under the pointer and stays in bounds', () => {
  const cam = zoomAtPoint({ x: 0, y: 0, z: 1 }, 100, 50, 2);
  assert.deepEqual(cam, { x: -100, y: -50, z: 2 });
  assert.equal(zoomAtPoint(cam, 0, 0, 100).z, ZMAX);
  assert.equal(zoomAtPoint(cam, 0, 0, 0).z, ZMIN);
  assert.equal(nextZoom(1, 1), 1.5);
  assert.equal(nextZoom(1, -1), 0.75);
  assert.equal(nextZoom(8, 1), ZMAX);
  assert.equal(nextZoom(0.02, -1), ZMIN);
});

test('studio-app-geometry: fit a rectangle in the viewport', () => {
  assert.deepEqual(fitRect([0, 0, 1000, 500], 1128, 628), { z: 1, x: 64, y: 64 });
  assert.equal(fitRect([0, 0, 4000, 500], 1128, 628).z, 0.25);
  assert.equal(fitRect([0, 0, 0, 0], 1128, 628), null);
});

test('studio-app-geometry: wheel pans (Shift: sideways), Ctrl/Cmd zooms at the pointer', () => {
  const cam = { x: 0, y: 0, z: 1 };
  assert.deepEqual(wheelCamera(cam, { dx: 0, dy: 10, dm: 0, ctrl: false, shift: false, x: 0, y: 0 }), { x: 0, y: -10, z: 1 });
  assert.deepEqual(wheelCamera(cam, { dx: 0, dy: 1, dm: 1, ctrl: false, shift: true, x: 0, y: 0 }), { x: -16, y: 0, z: 1 });
  const z = wheelCamera(cam, { dx: 0, dy: -100, dm: 0, ctrl: true, shift: false, x: 0, y: 0 }).z;
  assert.ok(Math.abs(z - Math.pow(2, 30 * 0.012)) < 1e-9, 'zoom step is clamped to 30 units');
});

test('studio-app-geometry: a rectangle in a frame to window coordinates', () => {
  assert.deepEqual(frameToScreen({ x: 100, y: 200 }, [10, 20, 30, 40], { x: 5, y: 6, z: 0.5 }, { left: 240, top: 44 }), [300, 160, 15, 20]);
});
