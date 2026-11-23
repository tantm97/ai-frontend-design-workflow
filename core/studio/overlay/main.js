// @ts-check
// dfw Studio in-frame agent, injected into every design document served by the Studio. It reports
// hover/selection geometry, layers, design-token usage and text edits to the parent Studio via
// postMessage, and forwards wheel/keys so the canvas can pan and zoom over frames. The Studio draws
// all chrome (selection boxes, pins) itself, in screen space. Framework-agnostic.
// Bundled into core/studio/overlay.js (one classic script) by scripts/build-overlay.mjs.
import { info } from './target.js';
import { listenInput } from './events.js';
import { listenStudio } from './messages.js';
import { ready } from './update.js';
import { reportPins, reportSelection, reportSize } from './select.js';
import { st, ui } from './state.js';
import { editTarget } from './edit.js';
import { placeHandles } from './layout.js';

// Higher modules answer the calls of lower ones (state.js `ui`); wired before anything runs.
Object.assign(ui, { editTarget, placeHandles });

function main() {
  if (window.__dfwOverlay) return;
  window.__dfwOverlay = true;
  // dfw review (running app, the live-review proxy): only share the comment-target helpers with live.js.
  const LIVE = Boolean(document.currentScript && document.currentScript.hasAttribute('data-dfw-live'));
  const params = new URLSearchParams(location.search);
  if (!LIVE && params.get('theme')) document.documentElement.dataset.theme = params.get('theme');
  if (!LIVE && params.get('state')) document.documentElement.dataset.state = params.get('state');
  if (window.parent === window && !LIVE) return; // opened directly: plain page
  if (LIVE) {
    window.__dfwTargetInfo = info;
    return;
  }
  listenInput();
  listenStudio();
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready);
  else ready();
  addEventListener('load', reportSize);
  if (document.fonts)
    document.fonts.ready.then(() => {
      reportSize();
      reportSelection();
      if (st.pins.length) reportPins();
    });
  if (window.ResizeObserver && document.documentElement) {
    /** @type {ReturnType<typeof setTimeout> | undefined} */
    let t;
    new ResizeObserver(() => {
      clearTimeout(t);
      t = setTimeout(reportSize, 100);
    }).observe(document.documentElement);
  }
}

main();
