// @ts-check
// Comment pins on the frames and the pin popover (read, verify or reopen a comment where it was made).
import { PIN_STATUSES, esc, own, pinsOf, commentLabel, isDraft } from './model.js';
import { $, $$, P, S, post, store, toast } from './state.js';
import { updateHud } from './hud.js';
import { commentActions, renderComments, setTab } from './comments.js';
import { MSG } from '../shared/protocol.js';
import { MODE, PIN_FILTER, TAB, SYSTEM_SCREEN } from '../shared/enums.js';
import { TEXT } from '../shared/text.js';

const STATUS_COLOR = { open: '#c2410c', acknowledged: '#c2410c', reopened: '#c2410c', fixed: '#a16207', resolved: '#15803d' };
const PIN_LABEL = { active: 'Pins: open', all: 'Pins: all', off: 'Pins: off' };

/** Presenter: pins only when toggled on, or while commenting. Canvas: the Pins setting. */
export const pinStatuses = (/** @type {any} */ f) =>
  f?.present ? (P.pins || P.mode === MODE.COMMENT ? PIN_STATUSES.active : []) : own(PIN_STATUSES, S.pins) || PIN_STATUSES.active;

/** The pins a frame shows, as its overlay needs them. */
export function pinsFor(/** @type {any} */ f) {
  if (!f) return [];
  return pinsOf(S.comments, f, pinStatuses(f));
}

/** Draw a button per pin rect the frames reported. */
export function renderPins() {
  const wrap = $('#pins');
  wrap.innerHTML = '';
  for (const f of [...S.frames.values(), ...(P.open ? [P] : [])]) {
    for (const p of f.pinRects || []) {
      const c = S.comments.find((x) => x.id === p.id);
      if (!c) continue;
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = String(Number(c.id.replace('fb_', '')));
      b.title = TEXT.pinTitle(b.textContent || '', commentLabel(c));
      b.setAttribute('aria-label', TEXT.pinLabel(b.textContent || '', c.comment.slice(0, 80)));
      b.dataset.s = c.status;
      if (S.pop === c.id) b.classList.add('on');
      b.dataset.key = f.key;
      b.dataset.id = c.id;
      b.dataset.rect = JSON.stringify(p.rect);
      b.style.background = own(STATUS_COLOR, c.status) || '#78716c';
      b.onclick = (e) => {
        e.stopPropagation();
        if (S.pop === c.id) return closePop();
        S.selectedComment = c.id;
        if (!P.open) {
          setTab(TAB.COMMENTS);
          renderComments();
          document.querySelector(`.comment[data-id="${CSS.escape(c.id)}"]`)?.scrollIntoView({ block: 'nearest' });
        }
        openPop(c.id);
      };
      wrap.append(b);
    }
  }
  updateHud();
}

/** Send every loaded frame its pins (after comments or the Pins setting changed). */
export function syncPins() {
  for (const f of S.frames.values()) if (f.loaded) post(f, { type: MSG.PINS, pins: pinsFor(f) });
  if (P.open && P.loaded) post(P, { type: MSG.PINS, pins: pinsFor(P) });
}

/** Pins setting of the canvas: open -> all -> off (Shift+C). */
export function cyclePins() {
  S.pins = own({ active: 'all', all: 'off', off: 'active' }, S.pins) || 'active';
  store.set('pins', S.pins);
  if (S.pins === PIN_FILTER.OFF) closePop();
  renderPinsToggle();
  syncPins();
  toast(S.pins === PIN_FILTER.OFF ? TEXT.pinsHidden : S.pins === PIN_FILTER.ALL ? TEXT.pinsAll : TEXT.pinsActive);
}

export function renderPinsToggle() {
  const b = $('#pins-toggle');
  b.textContent = own(PIN_LABEL, S.pins) || PIN_LABEL.active;
  b.setAttribute('aria-pressed', String(S.pins !== PIN_FILTER.OFF));
}

export function openPop(/** @type {string} */ id) {
  S.pop = id;
  renderPop();
  renderPins();
}

export function closePop() {
  if (!S.pop) return;
  S.pop = null;
  $('#pin-pop').hidden = true;
  for (const b of $$('#pins button.on')) b.classList.remove('on');
}

export function renderPop() {
  const c = S.comments.find((x) => x.id === S.pop);
  const pop = $('#pin-pop');
  if (!c) return closePop();
  const where = c.screen === SYSTEM_SCREEN ? 'Design System' : [c.screen, c.variant, c.breakpoint, c.state].filter(Boolean).join(' / ');
  const thread = (c.thread || []).slice(-3);
  pop.innerHTML = `
    <div class="pop-head"><span class="num">#${esc(c.id.replace('fb_', ''))}</span><span class="status" data-s="${esc(isDraft(c) ? 'draft' : c.status)}">${esc(commentLabel(c))}</span><button type="button" class="pop-x" aria-label="Close">✕</button></div>
    <p class="pop-text">${esc(c.comment)}</p>
    <p class="pop-where">${esc(where)} · <code>${esc(c.target?.dfwId || c.target?.selector || 'area')}</code></p>
    ${thread.length ? `<ul class="thread">${thread.map((/** @type {any} */ t) => `<li><span class="by">${esc(t.by)} · ${esc(new Date(t.at).toLocaleString())}</span>${esc(t.message)}</li>`).join('')}</ul>` : ''}
    <div class="comment-actions"></div>`;
  $('.pop-x', pop).onclick = closePop;
  commentActions(c, $('.comment-actions', pop));
  pop.hidden = false;
  placePop();
}

/** Keep the popover next to its pin (or close it when the pin is gone). */
export function placePop() {
  const pop = $('#pin-pop');
  if (!S.pop || pop.hidden) return;
  const b = $(`#pins button[data-id="${CSS.escape(S.pop)}"]`);
  if (!b || b.hidden) return closePop();
  const r = b.getBoundingClientRect();
  const w = pop.offsetWidth;
  const h = pop.offsetHeight;
  let x = r.right + 8;
  if (x + w > innerWidth - 8) x = Math.max(8, r.left - w - 8);
  pop.style.left = `${x}px`;
  pop.style.top = `${Math.min(Math.max(48, r.top), innerHeight - h - 8)}px`;
}
