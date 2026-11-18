// @ts-check
// Text editing in place: double-click a text-only element with a data-dfw-id (or the text of a
// component instance); Enter saves to the source through the Studio, Escape cancels.
import { post, st } from './state.js';
import { placeHandles } from './layout.js';
import { reportSelection } from './select.js';

/** Text with runs of white space as one space, trimmed. */
export const norm = (/** @type {string | null | undefined} */ s) => (s || '').replace(/\s+/g, ' ').trim();

/**
 * Can this element's text be edited here, and under which id?
 * @param {any} el
 * @returns {{ el?: HTMLElement, id?: string, error?: string }}
 */
export function editTarget(el) {
  if (!el || el.children.length) return { error: 'Only text-only elements can be edited in place.' };
  if (/^(INPUT|TEXTAREA|SELECT|IMG|SVG|svg)$/.test(el.tagName) || !norm(el.textContent))
    return { error: 'This element has no text to edit.' };
  if (el.dataset.dfwId) return { el, id: el.dataset.dfwId };
  const host = el.closest('[data-dfw-id]');
  if (host && host.dataset.dfwComponent && norm(host.textContent) === norm(el.textContent)) return { el, id: host.dataset.dfwId };
  return { error: 'Give this element a data-dfw-id to edit it in place, or leave a comment.' };
}

export function startEdit(/** @type {HTMLElement} */ el) {
  const t = editTarget(el);
  if (t.error) return post('dfw:toast', { message: t.error });
  st.editing = { el: t.el, id: t.id, orig: t.el.textContent };
  t.el.setAttribute('contenteditable', 'plaintext-only');
  t.el.focus();
  const range = document.createRange();
  range.selectNodeContents(t.el);
  const sel = getSelection();
  sel.removeAllRanges();
  sel.addRange(range);
  t.el.addEventListener('blur', onEditBlur);
  placeHandles();
  post('dfw:editing', { on: true, id: t.id });
}

function onEditBlur() {
  endEdit(true);
}

/** End the text edit; save sends the new text to the Studio when it changed. */
export function endEdit(/** @type {boolean} */ save) {
  if (!st.editing) return;
  const { el, id, orig } = st.editing;
  st.editing = null;
  el.removeEventListener('blur', onEditBlur);
  el.removeAttribute('contenteditable');
  const text = el.textContent;
  if (!save) el.textContent = orig;
  getSelection()?.removeAllRanges();
  post('dfw:editing', { on: false, id });
  if (save && norm(text) !== norm(orig)) post('dfw:edit-text', { id, text: norm(text) });
  reportSelection();
}
