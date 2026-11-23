// @ts-check
// Top bar: product name, breakpoint and state menus, full height, pins, theme, zoom and help.
import { activeBps, allStates, esc, shownStates } from './model.js';
import { $, $$, S, store, viewContext } from './state.js';
import { rebuildKeepCam } from './frames.js';
import { zoomStep, zoomToFit } from './canvas.js';
import { cyclePins, renderPinsToggle } from './pins.js';
import { setMode } from './keys.js';
import { TEXT } from '#studio/shared/text.js';

export function renderToolbar() {
  const m = S.manifest;
  const ctx = viewContext();
  $('#product').textContent = m.product || 'Design Studio';
  const st = S.state;
  $('#meta').textContent = [st ? `v${st.design.version} ${st.design.status}` : null, m.inferred ? 'manifest inferred' : null]
    .filter(Boolean)
    .join(' · ');
  const bpName = (/** @type {any} */ bp) => `${bp.label || bp.id} ${bp.width}`;
  const bpOn = activeBps(ctx);
  $('#bp-menu summary').textContent = !S.activeBps
    ? 'All breakpoints'
    : bpOn.length <= 2
      ? bpOn.map(bpName).join(', ')
      : `${bpOn.length} breakpoints`;
  checkList(
    $('#breakpoints'),
    m.breakpoints.map((/** @type {any} */ bp) => ({ id: bp.id, label: bpName(bp) })),
    S.activeBps,
    (ids) => {
      S.activeBps = ids && !ids.length ? [bpOn[0].id] : ids; // keep at least one breakpoint
      store.set('bps3', S.activeBps);
    },
  );
  const states = allStates(m);
  $('#state-menu').hidden = !states.length;
  const stOn = shownStates(ctx, states);
  $('#state-menu summary').textContent = TEXT.statesSummary(!S.activeStates, stOn);
  checkList(
    $('#states'),
    states.map((x) => ({ id: x, label: x })),
    S.activeStates,
    (ids) => {
      S.activeStates = ids;
      store.set('states3', ids);
    },
  );
  $('#full-toggle').setAttribute('aria-pressed', String(S.fullHeight));
  renderPinsToggle();
  const themes = m.themes || [];
  $('#theme-wrap').hidden = !themes.length;
  $('#theme').innerHTML =
    `<option value="">default</option>` +
    themes.map((/** @type {string} */ t) => `<option ${t === S.theme ? 'selected' : ''}>${esc(t)}</option>`).join('');
}

/**
 * Checkbox list with an "All" row for a toolbar menu.
 * @param {HTMLElement} wrap
 * @param {{ id: string, label: string }[]} items
 * @param {string[] | null} checked ids, or null = all
 * @param {(ids: string[] | null) => void} onChange gets ids, or null (all)
 */
function checkList(wrap, items, checked, onChange) {
  const on = (/** @type {string} */ id) => !checked || checked.includes(id);
  const n = items.filter((it) => on(it.id)).length;
  const change = (/** @type {string[] | null} */ ids) => {
    onChange(ids && ids.length === items.length ? null : ids);
    renderToolbar();
    rebuildKeepCam();
  };
  wrap.innerHTML = '';
  const all = document.createElement('label');
  all.className = 'ms-all';
  all.innerHTML = `<input type="checkbox" ${n === items.length ? 'checked' : ''}> All`;
  all.querySelector('input').indeterminate = n > 0 && n < items.length;
  all.querySelector('input').onchange = (e) => change(/** @type {HTMLInputElement} */ (e.target).checked ? null : []);
  wrap.append(all);
  for (const it of items) {
    const l = document.createElement('label');
    l.innerHTML = `<input type="checkbox" ${on(it.id) ? 'checked' : ''}> ${esc(it.label)}`;
    l.querySelector('input').onchange = (e) =>
      change(items.map((x) => x.id).filter((id) => (id === it.id ? /** @type {HTMLInputElement} */ (e.target).checked : on(id))));
    wrap.append(l);
  }
}

/** Tool buttons, zoom, help sheet, menus, full height, pins and theme. */
export function initToolbar() {
  for (const b of $$('.seg button')) b.onclick = () => setMode(b.dataset.mode);
  $('#zoom-in').onclick = () => zoomStep(1);
  $('#zoom-out').onclick = () => zoomStep(-1);
  $('#zoom-label').onclick = () => zoomToFit();
  $('#help').onclick = () => ($('#shortcuts').hidden = false);
  $('#shortcuts-close').onclick = () => ($('#shortcuts').hidden = true);
  $('#shortcuts').onclick = (/** @type {MouseEvent} */ e) =>
    /** @type {HTMLElement} */ (e.target).id === 'shortcuts' && ($('#shortcuts').hidden = true);
  // Close an open toolbar menu on a click outside it.
  document.addEventListener('click', (e) => {
    for (const d of $$('details.ms[open]')) if (!d.contains(e.target)) d.open = false;
  });
  $('#full-toggle').onclick = () => {
    S.fullHeight = !S.fullHeight;
    store.set('full2', S.fullHeight);
    renderToolbar();
    rebuildKeepCam();
  };
  $('#theme').onchange = (/** @type {any} */ e) => {
    S.theme = e.target.value;
    store.set('theme', S.theme);
    rebuildKeepCam();
  };
  $('#pins-toggle').onclick = () => cyclePins();
}
