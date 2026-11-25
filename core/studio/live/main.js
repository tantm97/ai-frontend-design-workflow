// @ts-check
// dfw review: click-to-comment on the running app (injected by the live-review proxy next to overlay.js,
// which provides window.__dfwTargetInfo). Click the "Comment" button or press the shortcut
// (default Alt/Option+C, set by data-shortcut: "dfw config set review.shortcut <combo>" or
// --shortcut; "off" = button only), click an element, write, Cmd/Ctrl+Enter. The shortcut never
// fires while the app has focus in a field. The comment carries the element's source
// (data-insp-path from code-inspector-plugin) when the app has it, otherwise selector + text. All
// UI lives in a shadow root on <html> so the app's styles and framework never touch it; its CSS is
// a constructed stylesheet, so the app's Content-Security-Policy style-src does not block it.
// Bundled into core/studio/live.js (one classic script) by scripts/build-overlay.mjs.
import { isShortcut, parseShortcut, shortcutLabel } from './shortcut.js';
import { isTypingTarget } from '#studio/overlay/fields.js';
import { PATH, METHOD } from '#studio/shared/paths.js';
import { LIVE_COMMENT_EVENT } from '#studio/shared/protocol.js';
import { KEY } from '#studio/shared/enums.js';
import { TEXT } from '#studio/shared/text.js';

// main is about 190 lines (general.md GE4): the shadow-root UI, its state and its handlers share one
// closure, as the injected script always had; the pure part (the shortcut) lives in shortcut.js.
function main() {
  if (window.__dfwLive || window.top !== window) return;
  window.__dfwLive = true;
  const ATTR = 'data-insp-path';
  const MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
  const SHORTCUT = parseShortcut(document.currentScript && document.currentScript.dataset.shortcut);
  const label = shortcutLabel(SHORTCUT, MAC);
  let on = false;
  let picked = null;

  const host = document.createElement('dfw-live');
  host.setAttribute('data-dfw-ui', '');
  host.style.cssText = 'all:initial;position:fixed;inset:0;pointer-events:none;z-index:2147483647';
  const sh = host.attachShadow({ mode: 'open' });
  const css = `:host{font:13px/1.4 system-ui,-apple-system,"Segoe UI",sans-serif;color:#111}
*{box-sizing:border-box}
.box{position:fixed;border:2px solid #2f6fed;background:rgba(47,111,237,.08);border-radius:2px;pointer-events:none}
.tag{position:absolute;left:-2px;top:-22px;background:#2f6fed;color:#fff;font-size:11px;padding:2px 6px;border-radius:3px;white-space:nowrap;max-width:60vw;overflow:hidden;text-overflow:ellipsis}
.bar{position:fixed;right:16px;bottom:16px;display:flex;gap:6px;align-items:center;pointer-events:auto;
background:#111;color:#fff;border-radius:999px;padding:4px 4px 4px 12px;box-shadow:0 4px 16px rgba(0,0,0,.25)}
.bar b{font-weight:600;letter-spacing:.02em}
button{font:inherit;border:0;border-radius:999px;padding:6px 12px;cursor:pointer;background:#333;color:#fff}
button:focus-visible,textarea:focus-visible{outline:2px solid #7aa7ff;outline-offset:2px}
button[aria-pressed=true],button.primary{background:#2f6fed}
.pop{position:fixed;width:min(340px,calc(100vw - 32px));pointer-events:auto;background:#fff;border:1px solid #d0d4dc;border-radius:8px;padding:10px;box-shadow:0 8px 28px rgba(0,0,0,.2)}
.pop textarea{width:100%;min-height:76px;resize:vertical;font:inherit;border:1px solid #c4c9d4;border-radius:6px;padding:8px}
.src{display:block;margin:0 0 6px;color:#555;font:11px ui-monospace,Menlo,monospace;word-break:break-all}
.row{display:flex;justify-content:flex-end;gap:6px;margin-top:8px}
.row .ghost{background:#eef0f4;color:#111}
.toast{position:fixed;left:50%;bottom:72px;transform:translateX(-50%);background:#111;color:#fff;padding:8px 14px;border-radius:6px;pointer-events:none}
[hidden]{display:none!important}`;
  sh.innerHTML = `<div class="box" hidden><span class="tag"></span></div>
<div class="bar" role="toolbar" aria-label="${TEXT.liveBarLabel}"><b>dfw</b><button type="button" class="toggle" aria-pressed="false">${TEXT.label.comment}</button></div>
<form class="pop" hidden><span class="src"></span><textarea aria-label="${TEXT.liveCommentLabel}" placeholder="${TEXT.liveCommentPlaceholder}"></textarea>
<div class="row"><button type="button" class="ghost cancel">${TEXT.label.cancel}</button><button type="submit" class="primary">${TEXT.sendButton(0)}</button></div></form>
<div class="toast" role="status" hidden></div>`;
  if (sh.adoptedStyleSheets && typeof CSSStyleSheet === 'function' && 'replaceSync' in CSSStyleSheet.prototype) {
    const sheet = new CSSStyleSheet();
    sheet.replaceSync(css);
    sh.adoptedStyleSheets = [sheet];
  } else {
    const st = document.createElement('style');
    st.textContent = css;
    sh.prepend(st);
  }
  const $ = (s) => sh.querySelector(s);
  const box = $('.box');
  const toggle = $('.toggle');
  // The label comes from data-shortcut (config): an attribute, never markup (javascript.md J4).
  toggle.title = TEXT.liveToggleTitle(label);
  const pop = $('.pop');
  const text = $('textarea');
  const toastEl = $('.toast');
  const mount = () => document.documentElement.append(host);
  mount();
  // Frameworks that re-render <html> children (rare) would drop the host: put it back.
  new MutationObserver(() => host.isConnected || mount()).observe(document.documentElement, { childList: true });

  /** @type {ReturnType<typeof setTimeout> | undefined} */
  let tt;
  function toast(msg) {
    toastEl.textContent = msg;
    toastEl.hidden = false;
    clearTimeout(tt);
    tt = setTimeout(() => (toastEl.hidden = true), 3500);
  }

  const inspOf = (el) => el.closest(`[${ATTR}]`)?.getAttribute(ATTR) || null;
  const shortSrc = (v) => (v ? v.replace(/:[^:\d][^:]*$/, '') : null);
  const ours = (e) => e.composedPath().includes(host);

  function setOn(v) {
    on = v;
    toggle.setAttribute('aria-pressed', String(on));
    document.documentElement.style.cursor = on ? 'crosshair' : '';
    if (!on) box.hidden = true;
  }

  function outline(el) {
    if (!el) return (box.hidden = true);
    const r = el.getBoundingClientRect();
    Object.assign(box.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
    box.firstChild.textContent = shortSrc(inspOf(el)) || el.tagName.toLowerCase();
    box.hidden = false;
  }

  function openComposer(el) {
    picked = el;
    outline(el);
    const src = shortSrc(inspOf(el));
    $('.src').textContent = src || TEXT.noSourceMap(el.tagName.toLowerCase());
    const r = el.getBoundingClientRect();
    pop.hidden = false;
    const w = pop.offsetWidth;
    const h = pop.offsetHeight;
    const left = Math.max(16, Math.min(innerWidth - w - 16, r.left));
    const top = r.bottom + 8 + h < innerHeight ? r.bottom + 8 : Math.max(16, r.top - h - 8);
    Object.assign(pop.style, { left: `${left}px`, top: `${top}px` });
    text.value = '';
    text.focus();
  }

  function closeComposer() {
    pop.hidden = true;
    picked = null;
    box.hidden = true;
  }

  async function sendComment() {
    const comment = text.value.trim();
    if (!picked || !comment) return;
    const el = picked;
    const base =
      typeof window.__dfwTargetInfo === 'function'
        ? window.__dfwTargetInfo(el)
        : { tag: el.tagName.toLowerCase(), text: (el.innerText || '').trim().slice(0, 160) };
    const body = {
      comment,
      screen: location.pathname + location.search,
      url: location.pathname + location.search + location.hash,
      viewport: [innerWidth, innerHeight],
      target: { ...base, insp: inspOf(el) },
    };
    try {
      const res = await fetch(PATH.COMMENTS, {
        method: METHOD.POST,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      const c = await res.json();
      if (!res.ok) throw new Error(c.error || res.statusText);
      closeComposer();
      setOn(false);
      toast(TEXT.liveSent(String(c.id).replace('fb_', '')));
      window.dispatchEvent(new CustomEvent(LIVE_COMMENT_EVENT, { detail: c }));
    } catch (e) {
      toast(TEXT.liveNotSent(e.message));
    }
  }

  toggle.addEventListener('click', () => {
    closeComposer();
    setOn(!on);
  });
  $('.cancel').addEventListener('click', closeComposer);
  pop.addEventListener('submit', (e) => {
    e.preventDefault();
    sendComment();
  });
  text.addEventListener('keydown', (e) => {
    e.stopPropagation(); // the app's shortcuts must not fire while typing a comment
    if (e.key === KEY.ENTER && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      sendComment();
    } else if (e.key === KEY.ESCAPE) closeComposer();
  });

  const block = (e) => {
    if (!on || ours(e)) return;
    e.preventDefault();
    e.stopPropagation();
  };
  for (const type of ['pointerdown', 'mousedown', 'pointerup', 'mouseup', 'dblclick', 'auxclick']) addEventListener(type, block, true);
  addEventListener(
    'click',
    (e) => {
      if (!on || ours(e)) return;
      e.preventDefault();
      e.stopPropagation();
      const el = /** @type {any} */ (e.target);
      if (el && el.nodeType === 1 && el !== document.documentElement) openComposer(el);
    },
    true,
  );
  addEventListener(
    'mousemove',
    (e) => {
      const t = /** @type {any} */ (e.target);
      if (on && pop.hidden && !ours(e)) outline(t && t.nodeType === 1 ? t : null);
    },
    true,
  );
  addEventListener(
    'keydown',
    (e) => {
      if (ours(e)) return;
      const t = /** @type {any} */ (e.composedPath()[0] || e.target); // the real target inside the app's own shadow roots
      const typing = isTypingTarget(t);
      if (e.key === KEY.ESCAPE && (on || !pop.hidden)) {
        closeComposer();
        setOn(false);
      } else if (!typing && !e.repeat && pop.hidden && isShortcut(SHORTCUT, e)) {
        e.preventDefault();
        e.stopPropagation();
        setOn(!on);
      }
    },
    true,
  );
  addEventListener('scroll', () => picked && outline(picked), true);
}

main();
