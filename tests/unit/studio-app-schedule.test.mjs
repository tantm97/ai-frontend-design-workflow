// What the Studio's frames do next (core/studio/app/schedule.js): which frames load, which patch a
// change they missed, and which give their document back to stay within the live budget.
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { LIVE_BUDGET, MAX_LOADING, MAX_PATCHING, mergeFiles, planFrameWork } from '../../core/studio/app/schedule.js';

/** A frame on the page shown; `over` overrides the defaults. */
const shown = (key, dist, over = {}) => ({ key, live: false, near: true, hidden: false, dist, age: 0, ...over });
/** A live frame on a kept page that is hidden. */
const hidden = (key, age) => ({ key, live: true, near: false, hidden: true, dist: 0, age });
const keys = (list) => list.map((f) => f.key);

test('studio-app-schedule: the frames nearest the centre load first, two at a time', () => {
  const plan = planFrameWork([shown('far', 900), shown('mid', 300), shown('close', 10), shown('off', 50, { near: false })]);
  assert.deepEqual(keys(plan.load), ['close', 'mid']);
  assert.deepEqual(plan.unload, []);
  assert.equal(MAX_LOADING, 2);
  // A slot taken by a frame still loading leaves one.
  const busy = planFrameWork([shown('a', 1, { live: true, loading: true }), shown('b', 2), shown('c', 3)]);
  assert.deepEqual(keys(busy.load), ['b']);
});

test('studio-app-schedule: under the budget nothing is unloaded, hidden pages included', () => {
  const frames = [hidden('h1', 0), hidden('h2', 1), shown('s1', 5, { live: true }), shown('new', 9)];
  const plan = planFrameWork(frames, { budget: 4 });
  assert.deepEqual(keys(plan.load), ['new']);
  assert.deepEqual(plan.unload, []);
});

test('studio-app-schedule: over the budget, the oldest hidden page gives its documents first, then the farthest shown frame', () => {
  const frames = [
    shown('far', 2000, { live: true, near: false }),
    shown('farther', 3000, { live: true, near: false }),
    hidden('newer-page', 1),
    hidden('older-page', 0),
    shown('want1', 10),
    shown('want2', 20),
  ];
  const plan = planFrameWork(frames, { budget: 4 });
  assert.deepEqual(keys(plan.unload), ['older-page', 'newer-page']);
  assert.deepEqual(keys(plan.load), ['want1', 'want2']);
  const tighter = planFrameWork(frames, { budget: 3 });
  assert.deepEqual(keys(tighter.unload), ['older-page', 'newer-page', 'farther']);
  assert.deepEqual(keys(tighter.load), ['want1', 'want2']);
});

test('studio-app-schedule: a frame near the viewport is never unloaded to load another; the rest stay placeholders', () => {
  const frames = [shown('a', 1, { live: true }), shown('b', 2, { live: true }), shown('c', 3)];
  const plan = planFrameWork(frames, { budget: 2 });
  assert.deepEqual(plan.load, []);
  assert.deepEqual(plan.unload, []);
});

test('studio-app-schedule: the selected frame, a frame with an edit to verify, and frames at work keep their documents', () => {
  const frames = [
    shown('selected', 5000, { live: true, near: false, keep: true }),
    shown('loading', 4000, { live: true, near: false, loading: true }),
    shown('patching', 3000, { live: true, near: false, patching: true }),
    shown('spare', 2000, { live: true, near: false }),
    shown('want1', 1),
    shown('want2', 2),
  ];
  const plan = planFrameWork(frames, { budget: 4 });
  assert.deepEqual(keys(plan.unload), ['spare']);
  // One slot was taken by the frame still loading.
  assert.deepEqual(keys(plan.load), ['want1']);
});

test('studio-app-schedule: over the budget with nothing to load (after a page switch), spare documents go', () => {
  const frames = [hidden('old', 0), hidden('older', 0), shown('s', 1, { live: true })];
  assert.deepEqual(keys(planFrameWork(frames, { budget: 1 }).unload), ['old', 'older']);
  assert.deepEqual(planFrameWork(frames, { budget: 3 }).unload, []);
});

test('studio-app-schedule: stale frames near the viewport patch nearest first, three at a time; far and hidden ones wait', () => {
  const stale = (key, dist, over = {}) => shown(key, dist, { live: true, stale: true, ...over });
  const frames = [
    stale('s4', 40),
    stale('s1', 10),
    stale('s3', 30),
    stale('s2', 20),
    stale('far', 5, { near: false }),
    { ...hidden('kept', 0), stale: true },
    stale('loading', 1, { loading: true }),
  ];
  assert.equal(MAX_PATCHING, 3);
  assert.deepEqual(keys(planFrameWork(frames).patch), ['s1', 's2', 's3']);
  const busy = planFrameWork([...frames, stale('busy', 0, { patching: true, stale: false })]);
  assert.deepEqual(keys(busy.patch), ['s1', 's2']);
});

test('studio-app-schedule: the budget is the measured 48', () => {
  assert.equal(LIVE_BUDGET, 48);
  assert.equal(planFrameWork(Array.from({ length: 60 }, (_, i) => shown(`f${i}`, i, { live: i < 48 }))).load.length, 0);
});

test('studio-app-schedule: missed changes are merged without repeats', () => {
  assert.deepEqual(mergeFiles(null, ['styles/app.css']), ['styles/app.css']);
  assert.deepEqual(mergeFiles(['styles/app.css'], ['styles/app.css', 'screens/home/a.html']), ['styles/app.css', 'screens/home/a.html']);
});
