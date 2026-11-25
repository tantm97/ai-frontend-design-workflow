// @ts-check
// Inspector: the selected frame or element, its component, and the design tokens its styles use.
// Spacing, component variant/size/props and text are editable here; colours and typography are
// design-system decisions, so they get an "Ask" button that opens a comment instead.
import { esc, own as ownKey } from './model.js';
import { $, S, currentPage, post } from './state.js';
import { edit, goToMaster, layoutScopeOf } from './edits.js';
import { openComposer } from './comments.js';
import { MSG } from '#studio/shared/protocol.js';
import { UI_ACTION, INSTANCE_FIELD, EDIT_OP, LAYOUT_KEY } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

const COLORISH = new Set(['color', 'background-color', 'border-color']);
const TYPE = new Set(['font-size', 'font-weight', 'line-height']);
const SPACING = new Set(['padding', 'gap', 'margin']);
const spaceTokens = () =>
  S.tokens
    .filter((t) => t.name.startsWith('space.'))
    .sort((a, b) => parseFloat(a.name.split('.')[1]) - parseFloat(b.name.split('.')[1]) || a.name.localeCompare(b.name));

function frameInfoHtml(/** @type {any} */ f) {
  if (f.system)
    return `<div class="isec"><div class="ihead">${TEXT.label.designSystem}</div><p class="note">${TEXT.systemFrameHtml}</p></div>`;
  const s = f.screen;
  const openStyle = 'display:inline-flex;align-items:center;text-decoration:none;color:inherit';
  const open = `<a class="btn" href="${esc(f.src)}" target="_blank" rel="noopener" style="${openStyle}">${TEXT.openInNewTab} ↗</a>`;
  return `<div class="isec"><div class="ihead">${esc(s.title || s.id)}</div>${s.description ? `<p class="note">${esc(s.description)}</p>` : ''}</div>
  <div class="isec"><h3>${TEXT.label.frame}</h3><dl class="kv">
    <dt>${TEXT.label.screen}</dt><dd>${esc(s.id)}</dd>
    <dt>${TEXT.label.variant}</dt><dd>${esc(f.variant.label || f.variant.id)}</dd>
    <dt>${TEXT.label.breakpoint}</dt><dd>${esc(f.bp.label || f.bp.id)} · ${esc(f.bp.width)}×${esc(f.bp.height)}</dd>
    <dt>${TEXT.label.state}</dt><dd>${esc(f.state || TEXT.label.default)}</dd>
    <dt>${TEXT.label.file}</dt><dd><code>design/${esc(f.file)}</code></dd>
  </dl><div class="btn-row">${open}</div></div>`;
}

/** The element section: id, tag, size, role, and the text and layout edit hints. */
function elementHtml(/** @type {any} */ f, /** @type {any} */ sel, /** @type {boolean} */ editable) {
  const own = sel.own || {};
  const scope = layoutScopeOf(f).scope;
  const editText =
    sel.textEditable && editable
      ? `<div class="btn-row"><button type="button" class="btn" data-act="edit-text">${TEXT.editText}</button>` +
        `<span class="note" style="margin:0;align-self:center">${TEXT.orDoubleClick}</span></div>`
      : '';
  const reset = `<button type="button" class="btn small" data-act="layout-reset" title="${esc(TEXT.layoutResetTitle(scope))}">`;
  const layout =
    editable && own.id
      ? `<p class="note">${TEXT.layoutHelpHtml(scope ? esc(scope) : '')}</p><div class="btn-row">${reset}${esc(TEXT.layoutReset(scope))}</button></div>`
      : '';
  return `<div class="isec"><h3>${TEXT.label.element}</h3><dl class="kv">
    <dt>dfw-id</dt><dd>${own.id ? `<code>${esc(own.id)}</code>` : `<span class="muted">${TEXT.label.none}${sel.target.dfwId ? ` (${TEXT.inside} ${esc(sel.target.dfwId)})` : ''}</span>`}</dd>
    <dt>${TEXT.label.tag}</dt><dd><code>&lt;${esc(sel.target.tag)}&gt;</code></dd>
    <dt>${TEXT.label.size}</dt><dd>${Math.round(sel.rect[2])} × ${Math.round(sel.rect[3])}</dd>
    ${sel.target.a11y?.role ? `<dt>${TEXT.label.role}</dt><dd>${esc(sel.target.a11y.role)}${sel.target.a11y.name ? ` “${esc(sel.target.a11y.name.slice(0, 60))}”` : ''}</dd>` : ''}
  </dl>${editText}${layout}</div>`;
}

/** The component section of an instance: variant, size and props (editable when it has an id). */
function componentHtml(/** @type {any} */ own, /** @type {boolean} */ editable) {
  const def = S.components.get(own.comp);
  const can = editable && own.id && def;
  const opt = (/** @type {string[]} */ list, /** @type {string} */ cur) =>
    list.map((x) => `<option ${x === cur ? 'selected' : ''}>${esc(x)}</option>`).join('');
  // The value written on the <dfw-use> (data-dfw-props, read by the overlay); '' when unset.
  const val = (/** @type {string} */ p) => {
    const v = own.props ? ownKey(own.props, p) : undefined;
    return esc(typeof v === 'string' ? v : '');
  };
  const input = (/** @type {string} */ p) =>
    `<input data-edit="prop" data-prop="${esc(p)}" value="${val(p)}" placeholder="${TEXT.emptyValue}">`;
  const props = (def?.props || []).map((/** @type {string} */ p) => `<dt>${esc(p)}</dt><dd>${can ? input(p) : val(p)}</dd>`).join('');
  return `<div class="isec"><h3>${TEXT.label.component} <span class="sp"></span><button type="button" class="btn small comp" data-act="master">${TEXT.goToMaster}</button></h3><dl class="kv">
      <dt>${TEXT.label.component}</dt><dd>${esc(own.comp)}</dd>
      ${def?.variants?.length ? `<dt>${TEXT.label.variant}</dt><dd>${can ? `<select data-edit="variant">${opt(def.variants, own.variant)}</select>` : esc(own.variant)}</dd>` : ''}
      ${def?.sizes?.length ? `<dt>${TEXT.label.size}</dt><dd>${can ? `<select data-edit="size">${opt(def.sizes, own.size)}</select>` : esc(own.size)}</dd>` : ''}
      ${props}
    </dl>${!own.id && editable ? '<p class="note">Give the instance an id to edit it here.</p>' : ''}${def?.description ? `<p class="note">${esc(def.description)}</p>` : ''}</div>`;
}

/** One row per style the overlay reported, with the token it maps to (or "raw value"). */
function tokensHtml(/** @type {any} */ sel, /** @type {boolean} */ canSpace) {
  const st = spaceTokens();
  return (sel.inspect || [])
    .map((/** @type {any} */ r, /** @type {number} */ i) => {
      const has = r.tokens.length > 0;
      const sw = COLORISH.has(r.group) ? `<span class="sw" style="background:${esc(r.value)}"></span>` : '';
      let v;
      if (SPACING.has(r.group) && canSpace && st.length) {
        const cur = has ? r.tokens.find((/** @type {string} */ t) => t.startsWith('space.')) : null;
        const title = `Set ${esc(r.prop)} to a space token (writes var(--space-*) into the element's inline style)`;
        const raw = cur ? '' : `<option value="" selected>raw ${esc(r.value)}</option>`;
        const options = st
          .map((t) => `<option value="${esc(t.name)}" ${t.name === cur ? 'selected' : ''}>${esc(t.name)}</option>`)
          .join('');
        v = `<select data-space="${i}" title="${title}">${raw}${options}</select>${has ? '' : '<span class="none">raw value (no token)</span>'}`;
      } else {
        v = has
          ? `<span class="name" title="${esc(r.tokens.join(', '))}">${sw}${esc(r.tokens[0])}${r.tokens.length > 1 ? ` <span class="muted">+${r.tokens.length - 1}</span>` : ''}</span>`
          : `<span class="name">${sw}<span class="none">raw value (no token)</span></span>`;
      }
      const ask =
        COLORISH.has(r.group) || TYPE.has(r.group) || r.group === 'box-shadow' || r.group === 'border-radius'
          ? `<button type="button" class="ask" data-ask="${i}" title="Design-system decision: ask the AI to change it">Ask AI</button>`
          : '';
      return `<div class="tok"><span class="p">${esc(r.prop)}</span><span class="v">${v}<span class="raw" title="${esc(r.value)}">${esc(r.value)}</span></span>${ask || '<span></span>'}</div>`;
    })
    .join('');
}

export function renderInspector() {
  const box = $('#inspector');
  const f = S.frames.get(S.sel?.key || S.selKey);
  if (!f) {
    const p = currentPage();
    box.innerHTML = `<div class="isec"><div class="ihead">${esc(p?.title || '')}</div><p class="note">${TEXT.inspectorHelpHtml}</p></div>`;
    return;
  }
  if (!S.sel || S.sel.key !== f.key) return (box.innerHTML = frameInfoHtml(f));
  const sel = S.sel;
  const own = sel.own || {};
  const editable = !f.system && !!f.file;
  const parts = [];
  parts.push(
    `<div class="isec"><div class="ihead${own.comp ? ' comp' : ''}">${own.comp ? '◆' : '#'} ${esc(sel.label)}</div><p class="note">${esc(f.title)}${f.state ? ` · ${esc(f.state)}` : ''}</p></div>`,
  );
  parts.push(elementHtml(f, sel, editable));
  if (own.comp) parts.push(componentHtml(own, editable));
  const canSpace = editable && own.id && !own.comp;
  const tokRows = tokensHtml(sel, canSpace);
  parts.push(
    `<div class="isec"><h3>Design tokens in use</h3>${tokRows || '<p class="note">No styles to report.</p>'}` +
      `${canSpace ? '<p class="note">Spacing is editable here; colours and typography are design-system decisions.</p>' : ''}</div>`,
  );
  parts.push(
    `<div class="isec"><div class="btn-row" style="margin:0"><button type="button" class="btn" data-act="comment">Comment on this</button></div></div>`,
  );
  box.innerHTML = parts.join('');
}

/** Buttons (master, edit text, reset layout, comment, ask) and the edit fields of the inspector. */
export function initInspector() {
  $('#inspector').addEventListener('click', (/** @type {MouseEvent} */ e) => {
    const b = /** @type {HTMLElement} */ (e.target).closest('button');
    if (!b) return;
    const sel = S.sel;
    const f = S.frames.get(sel?.key);
    if (b.dataset.act === UI_ACTION.MASTER) return goToMaster(sel.own.comp);
    if (b.dataset.act === UI_ACTION.EDIT_TEXT) return post(f, { type: MSG.EDIT });
    if (b.dataset.act === UI_ACTION.LAYOUT_RESET) return post(f, { type: MSG.LAYOUT_KEY, key: LAYOUT_KEY.RESET });
    if (b.dataset.act === UI_ACTION.COMMENT) return openComposer(sel, f, '');
    if (b.dataset.ask != null) {
      const r = sel.inspect[Number(b.dataset.ask)];
      const what = r.tokens.length ? `currently ${r.tokens[0]} (${r.value})` : `currently ${r.value}, raw value with no token`;
      return openComposer(sel, f, `Change ${r.prop} of ${sel.own.id || sel.target.dfwId || sel.target.tag} (${what}) to `);
    }
  });
  $('#inspector').addEventListener('change', async (/** @type {Event} */ e) => {
    const t = /** @type {HTMLInputElement} */ (e.target);
    const sel = S.sel;
    const f = S.frames.get(sel?.key);
    if (!f) return;
    if (t.dataset.space != null) {
      const r = sel.inspect[Number(t.dataset.space)];
      return edit(f, { op: EDIT_OP.SPACING, id: sel.own.id, prop: r.prop, value: t.value || null });
    }
    if (t.dataset.edit === INSTANCE_FIELD.VARIANT || t.dataset.edit === INSTANCE_FIELD.SIZE)
      return edit(f, { op: t.dataset.edit, id: sel.own.id, value: t.value });
    if (t.dataset.edit === INSTANCE_FIELD.PROP) return edit(f, { op: EDIT_OP.PROP, id: sel.own.id, prop: t.dataset.prop, value: t.value });
  });
}
