// @ts-check
// dfw Studio client: a Figma-like canvas over the design workspace. Plain JavaScript modules loaded by
// the page as written (javascript.md J2), one per feature under app/. Frames are real-size iframes
// inside one transformed "world" element; pan/zoom only change that transform.
import { S, toast, view, viewport } from './app/state.js';
import { initCanvas } from './app/canvas.js';
import { updateHud } from './app/hud.js';
import { initLayers } from './app/layers.js';
import { initInspector } from './app/inspector.js';
import { initComments } from './app/comments.js';
import { initPresent, layoutPresent } from './app/present.js';
import { initKeys, setMode } from './app/keys.js';
import { initToolbar } from './app/toolbar.js';
import { initShoot } from './app/shoot.js';
import { initEvents, loadAll } from './app/events.js';

initLayers();
initInspector();
initPresent();
initCanvas();
initKeys();
initToolbar();
initShoot();
initComments();
addEventListener('resize', () => {
  view.rect = null;
  layoutPresent();
  updateHud();
});
new ResizeObserver(() => {
  view.rect = null;
  updateHud();
}).observe(viewport);
initEvents();

window.__dfwStudio = S; // for debugging and e2e tests
setMode('select');
loadAll().catch((e) => toast(e.message));
