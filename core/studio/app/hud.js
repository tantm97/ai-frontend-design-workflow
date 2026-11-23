// @ts-check
// HUD: the screen-space chrome drawn over the frames (hover and selection boxes, pins, the agent's
// boxes and cursor), and agent presence (what the agent is editing or just changed).
import { frameToScreen } from './geometry.js';
import { $, $$, P, S, frameByKey, hud, viewportRect, visibleFrame, ui } from './state.js';
import { MODE, AGENT_PHASE } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

/**
 * Frame-viewport rect -> client (window) coordinates.
 * @param {any} f a frame or the presenter
 * @param {import('./geometry.js').Rect} r
 * @returns {import('./geometry.js').Rect}
 */
export function toScreen(f, r) {
  if (f.present) {
    const b = P.iframe.getBoundingClientRect();
    return [b.left + r[0] * P.scale, b.top + r[1] * P.scale, r[2] * P.scale, r[3] * P.scale];
  }
  return frameToScreen(f, r, S.cam, viewportRect());
}

/**
 * @param {HTMLElement} el
 * @param {import('./geometry.js').Rect} rect
 */
export function place(el, [x, y, w, h]) {
  el.style.transform = `translate(${x}px, ${y}px)`;
  el.style.width = `${w}px`;
  el.style.height = `${h}px`;
}

/** Redraw the HUD after anything moved: camera, selection, hover, pins, agent marks. */
export function updateHud() {
  // Clip the HUD to the canvas (or to the presenter stage).
  const clip = P.open ? $('#p-stage').getBoundingClientRect() : viewportRect();
  hud.style.clipPath = `inset(${clip.top}px ${innerWidth - clip.right}px ${innerHeight - clip.bottom}px ${clip.left}px)`;
  const hb = $('#hover-box');
  const hf = S.hover && frameByKey(S.hover.key);
  const same = S.sel && S.hover && S.sel.key === S.hover.key && S.sel.rect && S.hover.rect && S.sel.rect.join() === S.hover.rect.join();
  if (visibleFrame(hf) && S.hover.rect && !same && S.mode !== MODE.HAND) {
    hb.hidden = false;
    hb.classList.toggle('comp', !!S.hover.comp);
    place(hb, toScreen(hf, S.hover.rect));
  } else hb.hidden = true;
  const sb = $('#sel-box');
  const sf = S.sel && frameByKey(S.sel.key);
  if (visibleFrame(sf) && S.sel.rect) {
    sb.hidden = false;
    sb.classList.toggle('comp', !!S.sel.own?.comp);
    const r = toScreen(sf, S.sel.rect);
    place(sb, [r[0] - 1, r[1] - 1, r[2] + 2, r[3] + 2]);
    $('.tag', sb).textContent = S.sel.label;
    $('.dims', sb).textContent = TEXT.dims(S.sel.rect[2], S.sel.rect[3]);
  } else sb.hidden = true;
  for (const b of $$('#pins button')) {
    const f = frameByKey(b.dataset.key);
    b.hidden = !visibleFrame(f);
    if (b.hidden) continue;
    const r = toScreen(f, JSON.parse(b.dataset.rect));
    b.style.left = `${r[0] + r[2]}px`;
    b.style.top = `${r[1]}px`;
  }
  drawAgent();
  ui.placePop();
}

/**
 * Mark what the agent is editing (dashed) or just changed (solid, fades) in a frame.
 * @param {any} f
 * @param {'editing' | 'changed'} phase
 * @param {{ rect: import('./geometry.js').Rect, label?: string, whole?: boolean }[]} items
 * @param {number} [count] changed elements, when more than the items shown
 */
export function showAgent(f, phase, items, count = items.length) {
  const cur = S.agent.get(f.key);
  clearTimeout(cur?.timer);
  S.agent.set(f.key, { phase, items, count, timer: setTimeout(() => clearAgent(f.key), phase === AGENT_PHASE.EDITING ? 6000 : 2600) });
  S.agentLast = f.key;
  updateHud();
}

export function clearAgent(/** @type {string} */ key) {
  clearTimeout(S.agent.get(key)?.timer);
  S.agent.delete(key);
  updateHud();
}

/** The agent's boxes and its cursor, which glides to the last frame it touched. */
function drawAgent() {
  const host = $('#agent');
  const boxes = [];
  /** @type {[number, number] | null} */
  let cursorAt = null;
  for (const [key, a] of S.agent) {
    const f = frameByKey(key);
    if (!visibleFrame(f)) continue;
    a.items.forEach((/** @type {any} */ it, /** @type {number} */ i) => {
      const r = toScreen(f, it.rect);
      const more = i === 0 && a.count > a.items.length ? ` +${a.count - a.items.length}` : '';
      boxes.push({
        r,
        phase: a.phase,
        label: i === 0 ? TEXT.agentLabel(a.phase === AGENT_PHASE.EDITING, it.whole ? TEXT.agentWholeFrame : it.label || '', more) : '',
      });
      if (i === 0 && (key === S.agentLast || !cursorAt)) cursorAt = [r[0] + Math.min(r[2] - 4, 14), r[1] + Math.min(r[3] - 4, 14)];
    });
  }
  while (host.children.length - 1 < boxes.length) {
    const b = document.createElement('div');
    b.className = 'box agent';
    b.append(Object.assign(document.createElement('span'), { className: 'tag' }));
    host.append(b);
  }
  [...host.children].slice(1).forEach((b, i) => {
    const x = boxes[i];
    b.hidden = !x;
    if (!x) return;
    b.classList.toggle('changed', x.phase === AGENT_PHASE.CHANGED);
    place(b, [x.r[0] - 2, x.r[1] - 2, x.r[2] + 4, x.r[3] + 4]);
    b.firstChild.textContent = x.label;
    b.firstChild.hidden = !x.label;
  });
  const cursor = $('#agent-cursor');
  cursor.style.display = cursorAt ? '' : 'none'; // an SVG element has no .hidden property
  if (cursorAt) cursor.style.transform = `translate(${cursorAt[0]}px, ${cursorAt[1]}px)`;
}
