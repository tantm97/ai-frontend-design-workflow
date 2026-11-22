// @ts-check
// Comments: the list in the right panel, the composer (click an element, write, save), status
// actions (resolve, reopen, dismiss) and live status updates from the agent.
import { ACTIVE, commentLabel, esc, inFilter, isDraft } from './model.js';
import { $, $$, ALL, P, PKEY, S, api, post, postAll, toast } from './state.js';
import { renderPages, selectFrame, setPage } from './frames.js';
import { zoomToRect } from './canvas.js';
import { toScreen, updateHud } from './hud.js';
import { closePop, pinStatuses, placePop, renderPop, syncPins } from './pins.js';
import { closePresent, flash } from './present.js';

export function renderComments() {
  const list = $('#comment-list');
  const items = S.comments.filter((c) => inFilter(S.filter, c)).sort((a, b) => b.id.localeCompare(a.id));
  $('#open-count').textContent = S.comments.filter((c) => ACTIVE.includes(c.status) || c.status === 'fixed').length;
  // A re-render (a status change over SSE, a click) must not drop the keyboard focus inside the list.
  const had = /** @type {HTMLElement | null} */ (list.contains(document.activeElement) ? document.activeElement : null);
  const focusId = /** @type {HTMLElement | null} */ (had?.closest('.comment'))?.dataset.id;
  const focusOpen = !!had?.matches('.open');
  list.innerHTML = items.length ? '' : `<li class="muted" style="padding:12px">Nothing here.</li>`;
  for (const c of items) {
    const li = document.createElement('li');
    li.className = `comment${S.selectedComment === c.id ? ' selected' : ''}`;
    li.dataset.id = c.id;
    const where = [c.screen, c.variant, c.breakpoint, c.state].filter(Boolean).join(' / ');
    const target = c.target?.dfwId || c.target?.selector || '(area)';
    const last = c.thread?.slice(-2) || [];
    li.innerHTML = `
      <div class="comment-top"><button type="button" class="num open" aria-label="Show comment #${esc(c.id.replace('fb_', ''))} on the canvas">#${esc(c.id.replace('fb_', ''))}</button><span>${esc(where)}</span><span class="status" data-s="${esc(isDraft(c) ? 'draft' : c.status)}">${esc(commentLabel(c))}</span></div>
      <div class="comment-target">${esc(target)}${c.target?.source ? ` · ${esc(c.target.source)}` : ''}</div>
      <p class="comment-text">${esc(c.comment)}</p>
      ${last.length ? `<ul class="thread">${last.map((/** @type {any} */ t) => `<li><span class="by">${esc(t.by)} · ${esc(new Date(t.at).toLocaleString())}</span>${esc(t.message)}</li>`).join('')}</ul>` : ''}
      <div class="comment-actions"></div>`;
    commentActions(c, $('.comment-actions', li));
    // The whole card is the mouse target; the "#n" button is the same action from the keyboard (studio-ui.md U7).
    li.onclick = () => focusComment(c);
    list.append(li);
    if (focusOpen && c.id === focusId) $('.open', li).focus();
  }
}

/**
 * The status buttons of a comment (in the list and in the pin popover).
 * @param {any} c
 * @param {HTMLElement} actions
 */
export function commentActions(c, actions) {
  const btn = (/** @type {string} */ label, /** @type {string} */ cls, /** @type {() => void} */ fn) => {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = label;
    if (cls) b.className = cls;
    b.onclick = (e) => {
      e.stopPropagation();
      fn();
    };
    actions.append(b);
  };
  if (c.status === 'fixed') {
    btn('Looks right · resolve', 'ok', () => patch(c.id, { status: 'resolved', message: 'Verified in studio.' }));
    btn('Not yet · reopen', '', () => askReason(actions, (msg) => patch(c.id, { status: 'reopened', message: msg })));
  } else if (ACTIVE.includes(c.status)) {
    btn('Dismiss', '', () => askReason(actions, (msg) => patch(c.id, { status: 'dismissed', message: msg })));
  } else {
    btn('Reopen', '', () => askReason(actions, (msg) => patch(c.id, { status: 'reopened', message: msg })));
  }
}

/**
 * A reason field under the actions; done() gets the text.
 * @param {HTMLElement} container
 * @param {(msg: string) => void} done
 */
function askReason(container, done) {
  if ($('textarea', container)) return;
  const ta = document.createElement('textarea');
  ta.rows = 2;
  ta.placeholder = 'Why? (sent to Claude)';
  const ok = document.createElement('button');
  ok.type = 'button';
  ok.textContent = 'Send';
  ok.className = 'ok';
  ok.onclick = (e) => {
    e.stopPropagation();
    if (ta.value.trim()) done(ta.value.trim());
  };
  ta.onclick = (e) => e.stopPropagation();
  container.append(ta, ok);
  placePop();
  ta.focus();
}

async function patch(/** @type {string} */ id, /** @type {Record<string, any>} */ body) {
  try {
    await api(`/__dfw/api/comments/${id}`, { method: 'PATCH', body: JSON.stringify(body) });
    await loadComments();
  } catch (e) {
    toast(e.message);
  }
}

/** Show the frame and element a comment is about. */
function focusComment(/** @type {any} */ c) {
  S.selectedComment = c.id;
  renderComments();
  const page = c.screen === 'system' ? S.pages.find((p) => p.system) : S.pages.find((p) => p.screens.includes(c.screen));
  if (!page) return toast('That screen is not on any page.');
  if (P.open) closePresent();
  if (S.page !== ALL && page.id !== S.page) setPage(page.id);
  const fs = [...S.frames.values()].filter((f) =>
    c.screen === 'system' ? f.system : !f.system && f.screen.id === c.screen && f.variant.id === c.variant,
  );
  const f =
    fs.find((x) => x.bp?.id === c.breakpoint && (x.state || null) === (c.state || null)) ||
    fs.find((x) => (x.state || null) === (c.state || null)) ||
    fs[0];
  if (!f) return toast('That frame is not visible (check breakpoints and States).');
  selectFrame(f.key);
  zoomToRect([f.x, f.y, f.w, f.h], { max: 1 });
  flash(f);
  const sel = { dfwId: c.target?.dfwId, selector: c.target?.selector };
  if (f.loaded) post(f, { type: 'dfw:select-id', ...sel });
  else f.pendingSelect = sel;
}

/**
 * Open the composer next to the selected element.
 * @param {any} sel the selection payload of the overlay
 * @param {any} f the frame (or the presenter)
 * @param {string} prefill
 */
export function openComposer(sel, f, prefill) {
  const s = f.screen;
  S.pending = f.system
    ? { screen: 'system', variant: null, breakpoint: null, file: null, state: null, theme: S.theme || null, target: sel.target }
    : {
        screen: s?.id || null,
        variant: f.variant?.id || null,
        breakpoint: f.bp?.id || null,
        file: f.file,
        state: f.state || null,
        theme: S.theme || null,
        target: sel.target,
      };
  const where = f.system ? 'Design System' : [s?.id || f.file, f.variant?.id, f.bp?.id, f.state].filter(Boolean).join(' / ');
  $('#composer-target').textContent = `${where} → ${sel.target.dfwId || sel.target.selector}`;
  const r = toScreen(f, sel.rect || [0, 0, 0, 0]);
  const comp = $('#composer');
  comp.hidden = false;
  comp.style.left = `${Math.min(Math.max(8, r[0] + r[2] + 12), innerWidth - 336)}px`;
  comp.style.top = `${Math.min(Math.max(48, r[1]), innerHeight - 200)}px`;
  const ta = $('#composer-text');
  ta.value = prefill || '';
  ta.focus();
  ta.setSelectionRange(ta.value.length, ta.value.length);
}

export function closeComposer() {
  if ($('#composer').hidden) return;
  $('#composer').hidden = true;
  S.pending = null;
  if (S.sel?.key === PKEY) {
    post(P, { type: 'dfw:clear-selection' });
    S.sel = null;
    updateHud();
  } else if (S.mode === 'comment') {
    postAll({ type: 'dfw:clear-selection' });
    S.sel = null;
    updateHud();
  }
}

export async function saveComment() {
  const text = $('#composer-text').value.trim();
  if (!text || !S.pending) return;
  try {
    // Saved as a draft: Claude gets the drafts together when the user presses "Send to Claude".
    const c = await api('/__dfw/api/comments', { method: 'POST', body: JSON.stringify({ ...S.pending, comment: text, draft: true }) });
    closeComposer();
    S.selectedComment = c.id;
    S.filter = 'active';
    setFilterTabs();
    setTab('comments');
    await loadComments();
    toast(`Draft #${c.id.replace('fb_', '')} saved · press Send to Claude when you are done.`);
  } catch (e) {
    toast(e.message);
  }
}

function setFilterTabs() {
  for (const b of $$('.tabs button')) b.setAttribute('aria-selected', String(b.dataset.filter === S.filter));
}

/**
 * Right panel tab: 'inspect' or 'comments'. Quiet: a selection does not take the panel away from
 * the comments while commenting.
 * @param {string} tab
 * @param {{ quiet?: boolean }} [opts]
 */
export function setTab(tab, { quiet = false } = {}) {
  if (quiet && tab === 'inspect' && !$('#tab-comments').hidden && S.mode === 'comment') return;
  for (const b of $$('.ptabs button')) b.setAttribute('aria-selected', String(b.dataset.tab === tab));
  // Keyboard focus inside the panel that goes away moves to the tab of the panel that comes.
  const leaving = $(tab === 'inspect' ? '#tab-comments' : '#tab-inspect');
  if (!leaving.hidden && leaving.contains(document.activeElement))
    $(`.ptabs button[data-tab="${tab === 'inspect' ? 'inspect' : 'comments'}"]`).focus();
  $('#tab-inspect').hidden = tab !== 'inspect';
  $('#tab-comments').hidden = tab !== 'comments';
}

/** The drafts count on the Send button and whether a Claude session listens (dfw comments wait). */
export function renderSendBar() {
  const drafts = S.comments.filter(isDraft).length;
  const send = $('#send-drafts');
  send.disabled = drafts === 0;
  send.textContent = drafts ? `Send to Claude (${drafts})` : 'Send to Claude';
  const listen = $('#listen');
  listen.dataset.on = String(!!S.listening);
  listen.innerHTML = S.listening
    ? '<b>Claude is listening.</b> Sent comments wake it at once.'
    : '<b>Claude is not listening.</b> Sent comments reach Claude with your next message in the terminal.';
}

/** Send every draft to Claude at once. */
async function sendDrafts() {
  try {
    const r = await api('/__dfw/api/comments/send', { method: 'POST' });
    S.listening = r.listening;
    toast(
      r.sent
        ? r.listening
          ? `Sent ${r.sent} comment(s) to Claude.`
          : `Sent ${r.sent} comment(s). Claude gets them with your next message in the terminal.`
        : 'No drafts to send.',
    );
    await loadComments();
  } catch (e) {
    toast(e.message);
  }
}

/** Fetch the comments; announce what the agent changed since the last fetch. */
export async function loadComments() {
  const prev = new Map(S.comments.map((c) => [c.id, c.status]));
  const [comments, listening] = await Promise.all([
    api('/__dfw/api/comments'),
    api('/__dfw/api/listening').catch(() => ({ listening: false })),
  ]);
  S.comments = comments;
  S.listening = listening.listening;
  // Live updates from Claude (via .dfw/reviews/ + SSE): announce status changes.
  if (prev.size) {
    for (const c of S.comments) {
      const was = prev.get(c.id);
      if (!was || was === c.status) continue;
      const n = c.id.replace('fb_', '');
      if (c.status === 'acknowledged') toast(`Claude is working on #${n}.`);
      else if (c.status === 'fixed') toast(`Claude fixed #${n}: please verify (Comments → To verify).`);
    }
  }
  renderComments();
  renderSendBar();
  renderPages();
  syncPins();
  if (S.pop) {
    const c = S.comments.find((x) => x.id === S.pop);
    if (!c || !pinStatuses(P.open ? P : null).includes(c.status)) closePop();
    else renderPop();
  }
}

/** Panel tabs, filter tabs, composer buttons, "New round". */
export function initComments() {
  for (const b of $$('.ptabs button')) b.onclick = () => setTab(b.dataset.tab);
  $('#composer-cancel').onclick = closeComposer;
  $('#composer-save').onclick = saveComment;
  for (const b of $$('.tabs button')) {
    b.onclick = () => {
      S.filter = b.dataset.filter;
      setFilterTabs();
      renderComments();
    };
  }
  $('#send-drafts').onclick = sendDrafts;
  $('#new-session').onclick = async () => {
    await api('/__dfw/api/sessions', { method: 'POST' });
    toast('New review round started.');
    await loadComments();
  };
}
