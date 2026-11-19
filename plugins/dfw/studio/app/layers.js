// @ts-check
// Layers pane: the layer tree of the selected frame, as its overlay reports it.
import { esc, layerParents } from './model.js';
import { $, S, post } from './state.js';
import { updateHud } from './hud.js';

const LANDMARKS = ['header', 'nav', 'main', 'aside', 'footer', 'section', 'form', 'dialog'];

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
        `<div class="row${n.comp ? ' comp' : ''}${n.hidden ? ' hidden-layer' : ''}" role="treeitem" data-i="${n.i}" aria-selected="${n.i === selI}" ${n.children.length ? `aria-expanded="${open}"` : ''} style="padding-left:${4 + depth * 12}px">` +
          `<button type="button" class="tw" tabindex="-1" ${n.children.length ? '' : 'style="visibility:hidden"'}><svg viewBox="0 0 8 8" width="8" height="8" aria-hidden="true" style="transform:rotate(${open ? 90 : 0}deg)"><path d="M2 1l4 3-4 3z" fill="currentColor"/></svg></button>` +
          `<span class="ic">${icon}</span><span class="nm">${esc(name)}</span>${sub ? `<span class="sub">${esc(sub)}</span>` : ''}</div>`,
      );
      if (open && n.children.length) walk(n.children, depth + 1);
    }
  };
  walk(S.layers.tree, 0);
  box.innerHTML = rows.join('') || '<p class="hint muted">No layers (add data-dfw-id to elements).</p>';
  box.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
}

/** Click a row to select its element (or the twisty to open it); hover a row to outline it. */
export function initLayers() {
  $('#layers').addEventListener('click', (/** @type {MouseEvent} */ e) => {
    const t = /** @type {HTMLElement} */ (e.target);
    const row = /** @type {HTMLElement | null} */ (t.closest('.row'));
    if (!row) return;
    const f = S.frames.get(S.selKey);
    const i = Number(row.dataset.i);
    if (t.closest('.tw')) {
      const k = `${f.key}:${i}`;
      S.layerOpen.set(k, row.getAttribute('aria-expanded') !== 'true');
      return renderLayers();
    }
    post(f, { type: 'dfw:layer-select', i });
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
