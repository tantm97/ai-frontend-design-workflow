// @ts-check
// dfw Studio client: a Figma-like canvas over the design workspace. Plain JavaScript modules loaded by
// the page as written (javascript.md J2), one per feature under app/. Frames are real-size iframes
// inside one transformed "world" element; pan/zoom only change that transform.
import { S, toast, ui, view, viewport } from './app/state.js';
import { initCanvas, applyCam, zoomToFit, zoomToSection } from './app/canvas.js';
import { updateHud } from './app/hud.js';
import { initLayers, renderLayers } from './app/layers.js';
import { initInspector, renderInspector } from './app/inspector.js';
import { initComments, closeComposer, commentActions, renderComments, setTab } from './app/comments.js';
import { initPresent, layoutPresent, closePresent, flash, loadPresent, openPresent } from './app/present.js';
import { initKeys, setMode } from './app/keys.js';
import { initToolbar } from './app/toolbar.js';
import { initShoot } from './app/shoot.js';
import { initEvents, loadAll } from './app/events.js';
import { closePop, placePop } from './app/pins.js';

// Higher modules answer the calls of lower ones (state.js `ui`); wired before anything runs.
Object.assign(ui, {
  applyCam,
  closeComposer,
  closePop,
  closePresent,
  commentActions,
  flash,
  loadPresent,
  openPresent,
  placePop,
  renderComments,
  renderInspector,
  renderLayers,
  setTab,
  zoomToFit,
  zoomToSection,
});

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
