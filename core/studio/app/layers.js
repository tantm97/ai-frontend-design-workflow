// @ts-check
// Layers pane: the layer tree of the selected frame, as its overlay reports it.
import { esc, layerParents } from './model.js';
import { $, S, post } from './state.js';
import { updateHud } from './hud.js';

const LANDMARKS = ['header', 'nav', 'main', 'aside', 'footer', 'section', 'form', 'dialog'];
/** Layer index of the row that has the roving tabindex (the selected row wins). */
let focusI = null;

/**
 * Store the tree a frame reported (and the selected layer in it).
 * @param {any} f
 * @param {{ tree: any[], selected?: number | null }} d
 */
export function setLayers(f, d) {
  S.layers = { key: f.key, tree: d.tree, parents: layerParents(d.tree) };
  if (S.sel?.key === f.key && d.selected != null) S.sel.layer = d.selected;
  renderLayers();
}

export function renderLayers() {
  const box = $('#layers');
  const f = S.frames.get(S.selKey);
  $('#layers-for').textContent = f ? f.title : '';
  if (!f) return (box.innerHTML = '<p class="hint muted">Select a frame or an element to see its layers.</p>');
  if (!S.layers || S.layers.key !== f.key) return (box.innerHTML = '<p class="hint muted">Loading layers…</p>');
  const selI = S.sel?.key === f.key ? S.sel.layer : null;
  const hadFocus = box.contains(document.activeElement);
  // Open the ancestors of the selected layer.
  for (let p = S.layers.parents.get(selI); p != null; p = S.layers.parents.get(p)) S.layerOpen.set(`${f.key}:${p}`, true);
  /** @type {string[]} */
  const rows = [];
  const walk = (/** @type {any[]} */ nodes, /** @type {number} */ depth) => {
    for (const n of nodes) {
      const k = `${f.key}:${n.i}`;
      const open = S.layerOpen.has(k) ? S.layerOpen.get(k) : depth < 2;
      const name = n.comp ? n.id || n.comp : n.id || (n.name ? `${n.tag} “${n.name}”` : n.tag);
      const sub = n.comp ? `${n.comp}${n.variant ? ` · ${n.variant}` : ''}` : n.id ? n.tag : '';
      const icon = n.comp ? '◆' : LANDMARKS.includes(n.tag) ? '▭' : '#';
      rows.push(
        `<div class="row${n.comp ? ' comp' : ''}${n.hidden ? ' hidden-layer' : ''}" role="treeitem" tabindex="-1" data-i="${esc(n.i)}" aria-selected="${n.i === selI}" ${n.children.length ? `aria-expanded="${open}"` : ''} style="padding-left:${4 + depth * 12}px">` +
          `<button type="button" class="tw" tabindex="-1" ${n.children.length ? '' : 'style="visibility:hidden"'}><svg viewBox="0 0 8 8" width="8" height="8" aria-hidden="true" style="transform:rotate(${open ? 90 : 0}deg)"><path d="M2 1l4 3-4 3z" fill="currentColor"/></svg></button>` +
          `<span class="ic">${icon}</span><span class="nm">${esc(name)}</span>${sub ? `<span class="sub">${esc(sub)}</span>` : ''}</div>`,
      );
      if (open && n.children.length) walk(n.children, depth + 1);
    }
  };
  walk(S.layers.tree, 0);
  box.innerHTML = rows.join('') || '<p class="hint muted">No layers (add data-dfw-id to elements).</p>';
  // Roving tabindex (studio-ui.md U7): one row is in the tab order, the arrows move between rows.
  const rowEls = /** @type {HTMLElement[]} */ ([...box.querySelectorAll('.row')]);
  const find = (/** @type {number | null} */ i) => (i == null ? undefined : rowEls.find((r) => r.dataset.i === String(i)));
  // Focus in the tree stays on its row; a selection made elsewhere moves the tab stop to it.
  const cur = (hadFocus ? find(focusI) || find(selI) : find(selI) || find(focusI)) || rowEls[0];
  if (cur) {
    cur.tabIndex = 0;
    focusI = Number(cur.dataset.i);
    if (hadFocus) cur.focus();
  }
  box.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
}

/** Give a row the roving tabindex and the focus. */
function focusRow(/** @type {HTMLElement | null | undefined} */ row) {
  if (!row) return;
  for (const r of /** @type {HTMLElement[]} */ ([...$('#layers').querySelectorAll('.row[tabindex="0"]')])) r.tabIndex = -1;
  row.tabIndex = 0;
  focusI = Number(row.dataset.i);
  row.focus();
  row.scrollIntoView({ block: 'nearest' });
}

/** Open or close a row (its twisty). */
function toggleRow(/** @type {any} */ f, /** @type {HTMLElement} */ row) {
  S.layerOpen.set(`${f.key}:${row.dataset.i}`, row.getAttribute('aria-expanded') !== 'true');
  renderLayers();
}

/** Keys of a focused layer row: arrows move, Right/Left open and close, Enter or Space selects. */
function layerKey(/** @type {KeyboardEvent} */ e) {
  const row = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest('.row'));
  const f = S.frames.get(S.selKey);
  if (!row || !f || e.altKey || e.metaKey || e.ctrlKey) return;
  const rowEls = /** @type {HTMLElement[]} */ ([...$('#layers').querySelectorAll('.row')]);
  const at = rowEls.indexOf(row);
  const expanded = row.getAttribute('aria-expanded');
  const byI = (/** @type {number | null | undefined} */ i) => rowEls.find((r) => r.dataset.i === String(i));
  if (e.key === 'ArrowDown') focusRow(rowEls[at + 1]);
  else if (e.key === 'ArrowUp') focusRow(rowEls[at - 1]);
  else if (e.key === 'Home') focusRow(rowEls[0]);
  else if (e.key === 'End') focusRow(rowEls.at(-1));
  else if (e.key === 'ArrowRight') {
    if (expanded === 'false') toggleRow(f, row);
    else if (expanded === 'true') focusRow(rowEls[at + 1]);
  } else if (e.key === 'ArrowLeft') {
    if (expanded === 'true') toggleRow(f, row);
    else focusRow(byI(S.layers?.parents.get(Number(row.dataset.i))));
  } else if (e.key === 'Enter' || e.key === ' ') {
    S.revealOnSelect = true;
    post(f, { type: 'dfw:layer-select', i: Number(row.dataset.i) });
  } else return;
  // Handled here: the canvas shortcuts (arrows, Space to pan, Enter to edit) must not see it.
  e.preventDefault();
  e.stopPropagation();
}

/** Click a row to select its element (or the twisty to open it); hover a row to outline it. */
export function initLayers() {
  $('#layers').addEventListener('click', (/** @type {MouseEvent} */ e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const row = /** @type {HTMLElement | null} */ (t.closest('.row'));
    if (!row) return;
    const f = S.frames.get(S.selKey);
    const i = Number(row.dataset.i);
    focusI = i;
    if (t.closest('.tw')) return toggleRow(f, row);
    // The element may be outside the viewport: the camera follows the selection (canvas.js revealSelection).
    S.revealOnSelect = true;
    post(f, { type: 'dfw:layer-select', i });
  });
  $('#layers').addEventListener('keydown', layerKey);
  // A focused row outlines its element in the frame, as hovering it does.
  $('#layers').addEventListener('focusin', (/** @type {FocusEvent} */ e) => {
    const row = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest('.row'));
    const f = S.frames.get(S.selKey);
    if (row && f) post(f, { type: 'dfw:layer-hover', i: Number(row.dataset.i) });
  });
  $('#layers').addEventListener('mouseover', (/** @type {MouseEvent} */ e) => {
    const row = /** @type {HTMLElement | null} */ (/** @type {HTMLElement} */ (e.target).closest('.row'));
    const f = S.frames.get(S.selKey);
    if (row && f) post(f, { type: 'dfw:layer-hover', i: Number(row.dataset.i) });
  });
  $('#layers').addEventListener('mouseleave', () => {
    S.hover = null;
    updateHud();
  });
}
