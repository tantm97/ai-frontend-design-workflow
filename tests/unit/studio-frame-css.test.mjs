// A frame on the canvas is marked (selected, flash) with box-shadow, never outline. An outline on a
// frame made Chrome exceed its tile memory while the canvas moved: with a component selected, parts
// of the window flashed blank on a Retina Mac (Chrome logged "tile memory limits exceeded").
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const CSS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'core/studio/app.css');

/** Lines of a stylesheet that set an outline on a frame (.frame, not .frame-title) or in a frame animation. */
export function frameOutlines(css) {
  const animations = new Set();
  for (const m of css.matchAll(/\.frame(?![\w-])[^{]*\{[^}]*\banimation:\s*([\w-]+)/g)) animations.add(m[1]);
  const bad = [];
  css.split('\n').forEach((line, i) => {
    if (!/\boutline\s*:/.test(line)) return;
    const selector = line.split('{')[0];
    const keyframes = /^\s*@keyframes\s+([\w-]+)/.exec(line);
    if (/\.frame(?![\w-])/.test(selector) || (keyframes && animations.has(keyframes[1]))) bad.push(`${i + 1}: ${line.trim()}`);
  });
  return bad;
}

test('frame css: the checker finds an outline on a frame and in its animation', () => {
  const css =
    '.frame.x { outline: 2px solid red; animation: pulse 1s; }\n.frame-title:focus { outline: 1px solid; }\n@keyframes pulse { 0% { outline: 4px solid; } }';
  assert.deepEqual(frameOutlines(css), [
    '1: .frame.x { outline: 2px solid red; animation: pulse 1s; }',
    '3: @keyframes pulse { 0% { outline: 4px solid; } }',
  ]);
});

test('frame css: frames are marked without outline', () => {
  assert.deepEqual(frameOutlines(fs.readFileSync(CSS, 'utf8')), [], 'mark a frame with box-shadow (see the comment on .frame.selected)');
});
