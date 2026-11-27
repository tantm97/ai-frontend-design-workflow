// @ts-check
// Comments: the list in the right panel, the composer (click an element, write, save), status
// actions (resolve, reopen, dismiss) and live status updates from the agent.
import { ACTIVE, canReply, commentLabel, esc, inFilter, isDraft, listenerState } from './model.js';
import { $, $$, ALL, P, PKEY, S, post, postAll, toast, ui } from './state.js';
import { renderPages, selectFrame, setPage } from './frames.js';
import { zoomToRect } from './canvas.js';
import { toScreen, updateHud } from './hud.js';
import { closePop, pinStatuses, placePop, renderPop, syncPins } from './pins.js';
import { MSG } from '#studio/shared/protocol.js';
import { server } from './api.js';
import { MODE, STATUS, COMMENT_FILTER, TAB, SYSTEM_SCREEN, LISTENER } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

export function renderComments() {
  const list = $('#comment-list');
  const items = S.comments.filter((c) => inFilter(S.filter, c)).sort((a, b) => b.id.localeCompare(a.id));
  $('#open-count').textContent = S.comments.filter((c) => ACTIVE.includes(c.status) || c.status === STATUS.FIXED).length;
  // A re-render (a status change over SSE, a click) must not drop the keyboard focus inside the list.
  const had = /** @type {HTMLElement | null} */ (list.contains(document.activeElement) ? document.activeElement : null);
  const focusId = /** @type {HTMLElement | null} */ (had?.closest('.comment'))?.dataset.id;
  const focusOpen = !!had?.matches('.open');
  list.innerHTML = items.length ? '' : `<li class="muted" style="padding:12px">${TEXT.nothingHere}</li>`;
  for (const c of items) {
    const li = document.createElement('li');
    li.className = `comment${S.selectedComment === c.id ? ' selected' : ''}`;
    li.dataset.id = c.id;
    const where = [c.screen, c.variant, c.breakpoint, c.state].filter(Boolean).join(' / ');
    const target = c.target?.dfwId || c.target?.selector || TEXT.areaTarget;
    const last = c.thread?.slice(-2) || [];
    const num = c.id.replace('fb_', '');
    const open = `<button type="button" class="num open" aria-label="${esc(TEXT.showOnCanvas(num))}">#${esc(num)}</button>`;
    const status = `<span class="status" data-s="${esc(isDraft(c) ? 'draft' : c.status)}">${esc(commentLabel(c))}</span>`;
    const lines = last
      .map(
        (/** @type {any} */ t) =>
          `<li><span class="by">${esc(t.by)} · ${esc(new Date(t.at).toLocaleString())}</span>${esc(t.message)}</li>`,
      )
      .join('');
    li.innerHTML = `
      <div class="comment-top">${open}<span>${esc(where)}</span>${status}</div>
      <div class="comment-target">${esc(target)}${c.target?.source ? ` · ${esc(c.target.source)}` : ''}</div>
      <p class="comment-text">${esc(c.comment)}</p>
      ${last.length ? `<ul class="thread">${lines}</ul>` : ''}
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
  if (c.status === STATUS.FIXED) {
    btn(TEXT.resolveButton, 'ok', () => patch(c.id, { status: STATUS.RESOLVED, message: TEXT.verifiedInStudio }));
    btn(TEXT.reopenButton, '', () => askReason(actions, (msg) => patch(c.id, { status: STATUS.REOPENED, message: msg })));
  } else if (ACTIVE.includes(c.status)) {
    // A reply is a thread line by the user, no status change; the inbox delivers it as a follow-up.
    if (canReply(c)) btn(TEXT.replyButton, '', () => askReason(actions, (msg) => patch(c.id, { message: msg }), TEXT.replyPlaceholder));
    btn(TEXT.dismissButton, '', () => askReason(actions, (msg) => patch(c.id, { status: STATUS.DISMISSED, message: msg })));
  } else {
    btn(TEXT.reopenAgainButton, '', () => askReason(actions, (msg) => patch(c.id, { status: STATUS.REOPENED, message: msg })));
  }
}

/**
 * A reason (or reply) field under the actions; done() gets the text, never an empty one.
 * @param {HTMLElement} container
 * @param {(msg: string) => void} done
 * @param {string} [placeholder]
 */
function askReason(container, done, placeholder = TEXT.reasonPlaceholder) {
  if ($('textarea', container)) return;
  const ta = document.createElement('textarea');
  ta.rows = 2;
  ta.placeholder = placeholder;
  ta.setAttribute('aria-label', placeholder);
  const ok = document.createElement('button');
  ok.type = 'button';
  ok.textContent = TEXT.reasonSend;
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
    await server.patchComment(id, body);
    await loadComments();
  } catch (e) {
    toast(e.message);
  }
}

/** Show the frame and element a comment is about. */
function focusComment(/** @type {any} */ c) {
  S.selectedComment = c.id;
  renderComments();
  const page = c.screen === SYSTEM_SCREEN ? S.pages.find((p) => p.system) : S.pages.find((p) => p.screens.includes(c.screen));
  if (!page) return toast(TEXT.screenNotOnPage);
  if (P.open) ui.closePresent();
  if (S.page !== ALL && page.id !== S.page) setPage(page.id);
  const fs = [...S.frames.values()].filter((f) =>
    c.screen === SYSTEM_SCREEN ? f.system : !f.system && f.screen.id === c.screen && f.variant.id === c.variant,
  );
  const f =
    fs.find((x) => x.bp?.id === c.breakpoint && (x.state || null) === (c.state || null)) ||
    fs.find((x) => (x.state || null) === (c.state || null)) ||
    fs[0];
  if (!f) return toast(TEXT.frameNotVisible);
  selectFrame(f.key);
  zoomToRect([f.x, f.y, f.w, f.h], { max: 1 });
  ui.flash(f);
  const sel = { dfwId: c.target?.dfwId, selector: c.target?.selector };
  if (f.loaded) post(f, { type: MSG.SELECT_ID, ...sel });
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
    ? { screen: SYSTEM_SCREEN, variant: null, breakpoint: null, file: null, state: null, theme: S.theme || null, target: sel.target }
    : {
        screen: s?.id || null,
        variant: f.variant?.id || null,
        breakpoint: f.bp?.id || null,
        file: f.file,
        state: f.state || null,
        theme: S.theme || null,
        target: sel.target,
      };
  const where = f.system ? TEXT.label.designSystem : [s?.id || f.file, f.variant?.id, f.bp?.id, f.state].filter(Boolean).join(' / ');
  $('#composer-target').textContent = TEXT.composerTarget(where, sel.target.dfwId || sel.target.selector);
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
    post(P, { type: MSG.CLEAR_SELECTION });
    S.sel = null;
    updateHud();
  } else if (S.mode === MODE.COMMENT) {
    postAll({ type: MSG.CLEAR_SELECTION });
    S.sel = null;
    updateHud();
  }
}

export async function saveComment() {
  const text = $('#composer-text').value.trim();
  if (!text || !S.pending) return;
  try {
    // Saved as a draft: the agent gets the drafts together when the user presses "Send to AI".
    const c = await server.addComment({ ...S.pending, comment: text, draft: true });
    closeComposer();
    S.selectedComment = c.id;
    S.filter = COMMENT_FILTER.ACTIVE;
    setFilterTabs();
    setTab(TAB.COMMENTS);
    await loadComments();
    toast(TEXT.draftSaved(c.id.replace('fb_', '')));
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
  if (quiet && tab === TAB.INSPECT && !$('#tab-comments').hidden && S.mode === MODE.COMMENT) return;
  for (const b of $$('.ptabs button')) b.setAttribute('aria-selected', String(b.dataset.tab === tab));
  // Keyboard focus inside the panel that goes away moves to the tab of the panel that comes.
  const leaving = $(tab === TAB.INSPECT ? '#tab-comments' : '#tab-inspect');
  if (!leaving.hidden && leaving.contains(document.activeElement))
    $(`.ptabs button[data-tab="${tab === TAB.INSPECT ? TAB.INSPECT : TAB.COMMENTS}"]`).focus();
  $('#tab-inspect').hidden = tab !== TAB.INSPECT;
  $('#tab-comments').hidden = tab !== TAB.COMMENTS;
}

/** What the send bar says in each listener state (markup: `<b>` names the state). */
const LISTENER_HTML = new Map([
  [LISTENER.LISTENING, TEXT.listeningHtml],
  [LISTENER.WORKING, TEXT.workingHtml],
  [LISTENER.OFF, TEXT.notListeningHtml],
]);

/** The drafts count on the Send button and whether the agent listens (dfw comments wait), works, or neither. */
export function renderSendBar() {
  const drafts = S.comments.filter(isDraft).length;
  const send = $('#send-drafts');
  send.disabled = drafts === 0;
  send.textContent = TEXT.sendButton(drafts);
  const listen = $('#listen');
  // Patched only on a change: a poll every few seconds must not re-announce the status line.
  if (listen.dataset.on !== S.listener) {
    listen.dataset.on = S.listener;
    listen.innerHTML = LISTENER_HTML.get(S.listener) || TEXT.notListeningHtml;
  }
  $('#listen-copy').hidden = S.listener !== LISTENER.OFF;
}

/** The toast after "Send to AI": what happens to the sent comments now. */
const sentText = (/** @type {number} */ n) =>
  S.listener === LISTENER.LISTENING ? TEXT.sentListening(n) : S.listener === LISTENER.WORKING ? TEXT.sentWorking(n) : TEXT.sentQueued;

/** Send every draft to the agent at once. */
async function sendDrafts() {
  try {
    const r = await server.sendDrafts();
    S.listener = listenerState(r);
    toast(r.sent ? sentText(r.sent) : TEXT.noDrafts);
    await loadComments();
  } catch (e) {
    toast(e.message);
  }
}

/** Put the request to listen on the clipboard, for the user to paste to the agent. */
async function copyRequest() {
  try {
    await navigator.clipboard.writeText(TEXT.listenRequest);
    toast(TEXT.copiedRequest);
  } catch {
    toast(TEXT.copyFailed(TEXT.listenRequest));
  }
}

/** How often the send bar re-checks the listener while the page is visible: a wait that ends or is killed sends no event. */
const LISTENER_POLL_MS = 3000;

/** Ask the server whether the agent listens; re-render the send bar when that changed. */
async function refreshListener() {
  const state = listenerState(await server.listening().catch(() => null));
  if (state === S.listener) return;
  S.listener = state;
  renderSendBar();
}

/** Fetch the comments; announce what the agent changed since the last fetch. */
export async function loadComments() {
  const prev = new Map(S.comments.map((c) => [c.id, c.status]));
  const [comments, listening] = await Promise.all([server.comments(), server.listening().catch(() => null)]);
  S.comments = comments;
  S.listener = listenerState(listening);
  // Live updates from the agent (via .dfw/reviews/ + SSE): announce status changes.
  if (prev.size) {
    for (const c of S.comments) {
      const was = prev.get(c.id);
      if (!was || was === c.status) continue;
      const n = c.id.replace('fb_', '');
      if (c.status === STATUS.ACKNOWLEDGED) toast(TEXT.agentWorking(n));
      else if (c.status === STATUS.FIXED) toast(TEXT.agentFixed(n));
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
  $('#listen-copy').onclick = copyRequest;
  setInterval(() => {
    if (document.visibilityState === 'visible') refreshListener();
  }, LISTENER_POLL_MS);
  document.addEventListener('visibilitychange', () => {
    if (document.visibilityState === 'visible') refreshListener();
  });
  $('#new-session').onclick = async () => {
    await server.newSession();
    toast(TEXT.newRound);
    await loadComments();
  };
}
