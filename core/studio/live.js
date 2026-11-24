// GENERATED from core/studio/live/ by scripts/build-overlay.mjs: edit the modules there, then run npm run build:overlay.
(() => {
  // core/studio/shared/enums.js
  var MODE = Object.freeze({
    SELECT: "select",
    HAND: "hand",
    COMMENT: "comment",
    INTERACT: "interact",
    /** Presenter only: click through the prototype. */
    PRESENT: "present"
  });
  var STATUS = Object.freeze({
    OPEN: "open",
    ACKNOWLEDGED: "acknowledged",
    REOPENED: "reopened",
    FIXED: "fixed",
    RESOLVED: "resolved",
    DISMISSED: "dismissed"
  });
  var COMMENT_FILTER = Object.freeze({ ACTIVE: "active", FIXED: "fixed", CLOSED: "closed", ALL: "all" });
  var PIN_FILTER = Object.freeze({ ACTIVE: "active", ALL: "all", OFF: "off" });
  var TAB = Object.freeze({ INSPECT: "inspect", COMMENTS: "comments" });
  var SECTION_KIND = Object.freeze({ BREAKPOINT: "breakpoint", VARIANTS: "variants", STATES: "states", CUSTOM: "custom" });
  var UI_ACTION = Object.freeze({
    MASTER: "master",
    EDIT_TEXT: "edit-text",
    LAYOUT_RESET: "layout-reset",
    COMMENT: "comment"
  });
  var INSTANCE_FIELD = Object.freeze({ VARIANT: "variant", SIZE: "size", PROP: "prop" });
  var EDIT_OP = Object.freeze({
    TEXT: "text",
    PROP: "prop",
    VARIANT: "variant",
    SIZE: "size",
    SPACING: "spacing",
    LAYOUT: "layout",
    MOVE: "move"
  });
  var LAYOUT_PROP = Object.freeze({ GRID_COLUMN: "grid-column", WIDTH: "width", MIN_HEIGHT: "min-height" });
  var LAYOUT_KEY = Object.freeze({ RESET: "reset" });
  var DROP = Object.freeze({ BEFORE: "before", AFTER: "after", INSIDE: "inside" });
  var PHASE = Object.freeze({ START: "start", MOVE: "move", END: "end", DOWN: "down", UP: "up" });
  var AGENT_PHASE = Object.freeze({ EDITING: "editing", CHANGED: "changed" });
  var NAV_DIR = Object.freeze({ UP: "up", DOWN: "down", LEFT: "left", RIGHT: "right" });
  var KEY = Object.freeze({
    ESCAPE: "Escape",
    ENTER: "Enter",
    SPACE: " ",
    UP: "ArrowUp",
    DOWN: "ArrowDown",
    LEFT: "ArrowLeft",
    RIGHT: "ArrowRight",
    HOME: "Home",
    END: "End",
    HELP: "?",
    TAB: "Tab"
  });

  // core/studio/shared/text.js
  var plural = (n) => n === 1 ? "" : "s";
  var TEXT = Object.freeze({
    // Comments
    draftSaved: (n) => `Draft #${n} saved · press Send to AI when you are done.`,
    draftLabel: "Draft · not sent yet",
    sendButton: (drafts) => drafts ? `Send to AI (${drafts})` : "Send to AI",
    sentListening: (n) => `Sent ${n} comment${plural(n)} to the AI.`,
    sentNotListening: (n) => `Sent ${n} comment${plural(n)}. The AI gets them with your next message in the terminal.`,
    noDrafts: "No drafts to send.",
    /** Markup: `<b>` names the state (studio-ui.md U1). */
    listeningHtml: "<b>The AI is listening.</b> Sent comments wake it at once.",
    notListeningHtml: "<b>The AI is not listening.</b> Sent comments reach it with your next message in the terminal.",
    agentWorking: (n) => `The AI is working on #${n}.`,
    agentFixed: (n) => `The AI fixed #${n}: please verify (Comments → To verify).`,
    newRound: "New review round started.",
    screenNotOnPage: "That screen is not on any page.",
    frameNotVisible: "That frame is not visible (check breakpoints and States).",
    statusLabel: Object.freeze({
      [STATUS.OPEN]: "Open · waiting for the AI",
      [STATUS.ACKNOWLEDGED]: "AI is working",
      [STATUS.REOPENED]: "Reopened · waiting for the AI",
      [STATUS.FIXED]: "Fixed · please verify",
      [STATUS.RESOLVED]: "Resolved",
      [STATUS.DISMISSED]: "Dismissed"
    }),
    resolveButton: "Looks right · resolve",
    reopenButton: "Not yet · reopen",
    dismissButton: "Dismiss",
    reopenAgainButton: "Reopen",
    verifiedInStudio: "Verified in studio.",
    reasonPlaceholder: "Why? (sent to the AI)",
    reasonSend: "Send",
    composerTarget: (where, target) => `${where} → ${target}`,
    // Canvas and toolbar
    zoom: (z) => `${Math.round(z * 100)}%`,
    dims: (w, h) => `${Math.round(w)} × ${Math.round(h)}`,
    /** @param {boolean} all @param {string[]} on */
    fitWidth: "Fit width",
    allBreakpoints: "All breakpoints",
    breakpointCount: (n) => `${n} breakpoints`,
    manifestInferred: "manifest inferred",
    /** The product name when design.json names none. */
    defaultProduct: "Design Studio",
    // Canvas rows; `at` is a breakpoint name and width ("Desktop 1440")
    flowRow: (at) => `Flow at ${at}`,
    linkCount: (n) => `${n} link(s)`,
    atBreakpoint: (at) => `at ${at}`,
    variantsSub: (at) => `Alternatives at ${at}`,
    statesRow: (names) => `States (${names})`,
    statesSummary: (all, on) => `States: ${all ? "all" : !on.length ? "off" : on.length <= 2 ? on.join(", ") : on.length}`,
    // Pins
    pinTitle: (n, status) => `Comment #${n} · ${status}`,
    pinLabel: (n, text) => `Comment ${n}: ${text}`,
    pinsHidden: "Comment pins hidden (Shift+C shows them).",
    pinsAll: "Showing every pin, closed comments too.",
    pinsActive: "Showing pins of open and to-verify comments.",
    pinsMenu: Object.freeze({ active: "Pins: open", all: "Pins: all", off: "Pins: off" }),
    // Agent presence
    agentLabel: (editing, what, more) => `AI ${editing ? "is editing" : "changed"} ${what}${more}`.trim(),
    agentWholeFrame: "this frame",
    // Present
    nothingToPresent: "Nothing to present on this page.",
    noScreen: (id) => `No screen "${id}".`,
    linkNotAScreen: (to) => `Link target is not a screen in design.json: ${to}`,
    screenNotShown: (name) => `${name} is not shown with the current breakpoints.`,
    noFullscreen: "Fullscreen is not available here.",
    // Screenshots
    shootCount: (n) => `${n} frame${plural(n)} · shown breakpoints/states`,
    allScreens: "All screens",
    shootButton: (n) => `Screenshot ${n}`,
    noScreenToShoot: "No screen to screenshot.",
    shooting: (n) => `Screenshotting ${n} screen${plural(n)}…`,
    shot: (ok, all, dir, failed) => `${ok}/${all} PNG saved${dir ? ` in ${dir}` : ""}${failed ? ` · failed: ${failed}` : ""}`,
    shootNeedsNewServer: 'Screenshot needs a newer Studio server: run "dfw studio stop" then "dfw studio start", and reload.',
    shootFailed: (why) => `Screenshot failed: ${why}`,
    // Edits
    saved: (label, file, line) => `${label ? `${label}. ` : ""}Saved to ${file}:${line}  (⌘Z to undo)`,
    noChange: "No change.",
    undid: (op, file) => `Undid ${op} edit in ${file}.`,
    onlyTextOnly: "Only text-only elements can be edited in place.",
    noText: "This element has no text to edit.",
    needsIdToEdit: "Give this element a data-dfw-id to edit it in place, or leave a comment.",
    // Layout edits (inside a frame)
    widthHandle: "Drag to resize the width (snaps to grid columns, size tokens or percentages). Alt+Shift+Left/Right. Double-click: back to auto",
    minHeightHandle: "Drag to set a min-height (snaps to spacing/size tokens). Alt+Shift+Up/Down. Double-click: back to auto",
    noSizeTokens: "No size tokens to snap to.",
    needsIdToMove: "Select an element with a data-dfw-id to move or resize it.",
    onlyFlexGridChildren: "Only children of a flex or grid container can be reordered.",
    alreadyFirst: "Already the first item.",
    alreadyLast: "Already the last item.",
    spanOf: (span, of) => `span ${span} / ${of}`,
    tokenSize: (name, px) => `${name} · ${px}px`,
    movedInto: (id) => `Moved into ${id}`,
    moved: (position, id) => `Moved ${position} ${id}`,
    resetToAuto: (prop) => prop ? `${prop} reset to auto` : "Size reset to auto",
    alreadyAt: (label) => `Already ${label}.`,
    elementNotFound: (what) => `Element ${what} not found in this frame.`,
    layoutConflict: (id, prop, by) => `Saved, but ${id} did not change: ${by ? `"${by}"` : "another rule"} still sets ${prop}${by === "style attribute" ? "" : " with higher priority"}. Remove ${prop} there (or ask the AI), then resize again.`,
    // Live review (in the running app)
    liveToggleTitle: (shortcut) => `Comment on an element${shortcut ? ` (${shortcut})` : ""}`,
    liveSent: (n) => `Sent to AI · comment #${n}`,
    liveNotSent: (why) => `Not sent: ${why}`,
    liveBarLabel: "dfw review",
    liveCommentLabel: "Comment for the AI",
    liveCommentPlaceholder: "What should change?",
    noSourceMap: (tag) => `${tag} (no source map: dfw inspector setup)`,
    /** Modifier key names of a shortcut label, on a Mac and elsewhere. */
    keyNames: Object.freeze({
      mac: Object.freeze({ ctrl: "Ctrl", alt: "Option", shift: "Shift", meta: "Cmd" }),
      other: Object.freeze({ ctrl: "Ctrl", alt: "Alt", shift: "Shift", meta: "Meta" })
    }),
    // Comments list and pins
    nothingHere: "Nothing here.",
    showOnCanvas: (n) => `Show comment #${n} on the canvas`,
    areaTarget: "(area)",
    openComments: "Open comments",
    // Frames and pages
    frameTitleHint: (title) => `${title} (double-click to present)`,
    openInNewTab: "Open in new tab",
    pageInAllHint: "Click: go to this section · Double-click: open the page alone",
    // Layers
    layersNoSelection: "Select a frame or an element to see its layers.",
    layersLoading: "Loading layers…",
    layersNone: "No layers (add data-dfw-id to elements).",
    // Inspector (markup: the caller puts it in innerHTML as is)
    inspectorHelpHtml: "Click a frame title to select a frame, click inside a frame to select an element. Double-click text to edit it. Press <kbd>?</kbd> for shortcuts.",
    systemFrameHtml: "Generated from design/system/ and the tokens. Edit masters in <code>design/system/components/</code>.",
    /** @param {string} scopeHtml the breakpoint scope, already escaped, or '' */
    layoutHelpHtml: (scopeHtml) => `Layout: drag to reorder among flex/grid siblings or into an empty flex/grid container (<kbd>Alt</kbd>+arrows); drag the handles to resize (<kbd>Alt</kbd><kbd>Shift</kbd>+arrows), snapped to grid columns or tokens${scopeHtml ? `. Resizes here apply at ${scopeHtml}` : ""}. Double-click a handle or press <kbd>Alt</kbd><kbd>Shift</kbd><kbd>0</kbd> to reset.`,
    layoutResetTitle: (scope) => `Remove this element's Studio layout rule${scope ? ` at ${scope}` : ""} (back to the screen's own layout)`,
    layoutReset: (scope) => `Reset size to auto${scope ? ` (${scope})` : ""}`,
    editText: "Edit text",
    orDoubleClick: "or double-click it",
    goToMaster: "Go to master",
    inside: "inside",
    emptyValue: "(empty)",
    /** Short labels: field names, menu rows, buttons. */
    label: Object.freeze({
      designSystem: "Design System",
      prototype: "Prototype",
      variants: "Variants",
      states: "States",
      frame: "Frame",
      screen: "Screen",
      variant: "Variant",
      breakpoint: "Breakpoint",
      state: "State",
      file: "File",
      element: "Element",
      tag: "Tag",
      size: "Size",
      role: "Role",
      component: "Component",
      none: "none",
      default: "default",
      all: "All",
      allPages: "All pages",
      close: "Close",
      comment: "Comment",
      cancel: "Cancel"
    })
  });

  // core/studio/live/shortcut.js
  function parseShortcut(spec) {
    const s = spec || "alt+c";
    if (s === "off") return null;
    const parts = s.toLowerCase().split("+");
    const key = parts.pop();
    return { key, alt: parts.includes("alt"), ctrl: parts.includes("ctrl"), shift: parts.includes("shift"), meta: parts.includes("meta") };
  }
  function shortcutLabel(sc, mac) {
    if (!sc) return "";
    const names = mac ? TEXT.keyNames.mac : TEXT.keyNames.other;
    return [...["ctrl", "alt", "shift", "meta"].filter((m) => sc[m]).map((m) => names[m]), sc.key.toUpperCase()].join("+");
  }
  function isShortcut(sc, e) {
    return !!sc && e.altKey === sc.alt && e.ctrlKey === sc.ctrl && e.shiftKey === sc.shift && e.metaKey === sc.meta && (e.code === (/\d/.test(sc.key) ? "Digit" : "Key") + sc.key.toUpperCase() || String(e.key).toLowerCase() === sc.key);
  }

  // core/studio/overlay/fields.js
  var EDITABLE_FIELD = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';
  var isTypingTarget = (t) => !!t && (t.isContentEditable === true || typeof t.closest === "function" && !!t.closest(EDITABLE_FIELD));

  // core/studio/shared/paths.js
  var PATH = Object.freeze({
    /** The Design System page shown in its own frame. */
    SYSTEM_PAGE: "/__dfw/system.html",
    /** Server-Sent Events: `design`, `presence`, `comments`. */
    EVENTS: "/__dfw/events",
    MANIFEST: "/__dfw/api/manifest",
    STATE: "/__dfw/api/state",
    TOKENS: "/__dfw/api/tokens",
    COMPONENTS: "/__dfw/api/components",
    COMMENTS: "/__dfw/api/comments",
    /** Hand every draft comment to the agent (POST). */
    COMMENTS_SEND: "/__dfw/api/comments/send",
    /** Whether a `dfw comments wait` listens for this project (GET). */
    LISTENING: "/__dfw/api/listening",
    SESSIONS: "/__dfw/api/sessions",
    SHOOT: "/__dfw/api/shoot",
    EDIT: "/__dfw/api/edit",
    UNDO: "/__dfw/api/undo"
  });
  var METHOD = Object.freeze({ GET: "GET", POST: "POST", PATCH: "PATCH" });

  // core/studio/shared/protocol.js
  var MSG = Object.freeze({
    // to frame
    /** to frame: the tool changed `{ mode }`. */
    MODE: "dfw:mode",
    /** to frame: the design tokens `{ vars }`, for the inspector's token names. */
    TOKENS: "dfw:tokens",
    /** to frame: the comment pins of this frame `{ pins }`. */
    PINS: "dfw:pins",
    /** to frame: whether drag/resize editing is on `{ enabled, scope }`. */
    LAYOUT: "dfw:layout",
    /** to frame: a key for the layout editor `{ key, shift }`. */
    LAYOUT_KEY: "dfw:layout-key",
    /** to frame: check a saved layout edit `{ id, ... }`. */
    LAYOUT_VERIFY: "dfw:layout-verify",
    /** to frame: hover the layer at index `{ i }`. */
    LAYER_HOVER: "dfw:layer-hover",
    /** to frame: select the layer at index `{ i }`. */
    LAYER_SELECT: "dfw:layer-select",
    /** to frame: select an element `{ dfwId, selector }`. */
    SELECT_ID: "dfw:select-id",
    /** to frame: drop the selection. */
    CLEAR_SELECTION: "dfw:clear-selection",
    /** to frame: re-read the source and patch the page in place `{ files, agent }`. */
    SOFT_RELOAD: "dfw:soft-reload",
    /** to frame: start editing the selected text. */
    EDIT: "dfw:edit",
    /** to frame: move the selection `{ dir }`. */
    NAV: "dfw:nav",
    // both ways
    /** to frame: send the layer tree; to studio: the tree `{ tree, selected }`. */
    LAYERS: "dfw:layers",
    /** to frame: where the agent is about to edit `{ dfwId | component, fallback }`; to studio: its box `{ rect, label }`. */
    PRESENCE: "dfw:presence",
    // to studio
    /** to studio: the overlay is ready (after a load or a soft reload). */
    READY: "dfw:ready",
    /** to studio: the document height `{ h }`. */
    SIZE: "dfw:size",
    /** to studio: what a soft reload changed `{ agent, count, items }`. */
    CHANGED: "dfw:changed",
    /** to studio: the hovered element `{ rect, label, comp }`. */
    HOVER: "dfw:hover",
    /** to studio: an element was clicked `{ target, rect, own, comment, ... }`. */
    SELECT: "dfw:select",
    /** to studio: the selected element moved `{ rect }`. */
    SEL_RECT: "dfw:sel-rect",
    /** to studio: where the pins are drawn `{ pins }`. */
    PIN_RECTS: "dfw:pin-rects",
    /** to studio: a wheel over the frame `{ dx, dy, dm, ctrl, shift, x, y }`. */
    WHEEL: "dfw:wheel",
    /** to studio: a pan gesture `{ phase, sx, sy }`. */
    PAN: "dfw:pan",
    /** to studio: a key pressed in the frame (Studio shortcuts). */
    KEY: "dfw:key",
    /** to studio: a prototype link `{ to }`. */
    GOTO: "dfw:goto",
    /** to studio: edited text to save `{ id, text }`. */
    EDIT_TEXT: "dfw:edit-text",
    /** to studio: a drag/resize result to save. */
    LAYOUT_OP: "dfw:layout-op",
    /** to studio: a layout edit conflicts with the source. */
    LAYOUT_CONFLICT: "dfw:layout-conflict",
    /** to studio: show a short message `{ message }`. */
    TOAST: "dfw:toast",
    /** to studio: the frame took the keyboard focus. */
    FOCUS: "dfw:focus"
  });
  var SSE = Object.freeze({
    /** Design files changed `{ files }`. */
    DESIGN: "design",
    /** The agent is about to edit `{ file, dfwId }`. */
    PRESENCE: "presence",
    /** Review comments changed. */
    COMMENTS: "comments"
  });
  var LIVE_COMMENT_EVENT = "dfw:comment";

  // core/studio/live/main.js
  function main() {
    if (window.__dfwLive || window.top !== window) return;
    window.__dfwLive = true;
    const ATTR = "data-insp-path";
    const MAC = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);
    const SHORTCUT = parseShortcut(document.currentScript && document.currentScript.dataset.shortcut);
    const label = shortcutLabel(SHORTCUT, MAC);
    let on = false;
    let picked = null;
    const host = document.createElement("dfw-live");
    host.setAttribute("data-dfw-ui", "");
    host.style.cssText = "all:initial;position:fixed;inset:0;pointer-events:none;z-index:2147483647";
    const sh = host.attachShadow({ mode: "open" });
    const css = `:host{font:13px/1.4 system-ui,-apple-system,"Segoe UI",sans-serif;color:#111}
*{box-sizing:border-box}
.box{position:fixed;border:2px solid #2f6fed;background:rgba(47,111,237,.08);border-radius:2px;pointer-events:none}
.tag{position:absolute;left:-2px;top:-22px;background:#2f6fed;color:#fff;font-size:11px;padding:2px 6px;border-radius:3px;white-space:nowrap;max-width:60vw;overflow:hidden;text-overflow:ellipsis}
.bar{position:fixed;right:16px;bottom:16px;display:flex;gap:6px;align-items:center;pointer-events:auto;background:#111;color:#fff;border-radius:999px;padding:4px 4px 4px 12px;box-shadow:0 4px 16px rgba(0,0,0,.25)}
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
    if (sh.adoptedStyleSheets && typeof CSSStyleSheet === "function" && "replaceSync" in CSSStyleSheet.prototype) {
      const sheet = new CSSStyleSheet();
      sheet.replaceSync(css);
      sh.adoptedStyleSheets = [sheet];
    } else {
      const st = document.createElement("style");
      st.textContent = css;
      sh.prepend(st);
    }
    const $ = (s) => sh.querySelector(s);
    const box = $(".box");
    const toggle = $(".toggle");
    toggle.title = TEXT.liveToggleTitle(label);
    const pop = $(".pop");
    const text = $("textarea");
    const toastEl = $(".toast");
    const mount = () => document.documentElement.append(host);
    mount();
    new MutationObserver(() => host.isConnected || mount()).observe(document.documentElement, { childList: true });
    let tt;
    function toast(msg) {
      toastEl.textContent = msg;
      toastEl.hidden = false;
      clearTimeout(tt);
      tt = setTimeout(() => toastEl.hidden = true, 3500);
    }
    const inspOf = (el) => el.closest(`[${ATTR}]`)?.getAttribute(ATTR) || null;
    const shortSrc = (v) => v ? v.replace(/:[^:\d][^:]*$/, "") : null;
    const ours = (e) => e.composedPath().includes(host);
    function setOn(v) {
      on = v;
      toggle.setAttribute("aria-pressed", String(on));
      document.documentElement.style.cursor = on ? "crosshair" : "";
      if (!on) box.hidden = true;
    }
    function outline(el) {
      if (!el) return box.hidden = true;
      const r = el.getBoundingClientRect();
      Object.assign(box.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${r.height}px` });
      box.firstChild.textContent = shortSrc(inspOf(el)) || el.tagName.toLowerCase();
      box.hidden = false;
    }
    function openComposer(el) {
      picked = el;
      outline(el);
      const src = shortSrc(inspOf(el));
      $(".src").textContent = src || TEXT.noSourceMap(el.tagName.toLowerCase());
      const r = el.getBoundingClientRect();
      pop.hidden = false;
      const w = pop.offsetWidth;
      const h = pop.offsetHeight;
      const left = Math.max(16, Math.min(innerWidth - w - 16, r.left));
      const top = r.bottom + 8 + h < innerHeight ? r.bottom + 8 : Math.max(16, r.top - h - 8);
      Object.assign(pop.style, { left: `${left}px`, top: `${top}px` });
      text.value = "";
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
      const base = typeof window.__dfwTargetInfo === "function" ? window.__dfwTargetInfo(el) : { tag: el.tagName.toLowerCase(), text: (el.innerText || "").trim().slice(0, 160) };
      const body = {
        comment,
        screen: location.pathname + location.search,
        url: location.pathname + location.search + location.hash,
        viewport: [innerWidth, innerHeight],
        target: { ...base, insp: inspOf(el) }
      };
      try {
        const res = await fetch(PATH.COMMENTS, {
          method: METHOD.POST,
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify(body)
        });
        const c = await res.json();
        if (!res.ok) throw new Error(c.error || res.statusText);
        closeComposer();
        setOn(false);
        toast(TEXT.liveSent(String(c.id).replace("fb_", "")));
        window.dispatchEvent(new CustomEvent(LIVE_COMMENT_EVENT, { detail: c }));
      } catch (e) {
        toast(TEXT.liveNotSent(e.message));
      }
    }
    toggle.addEventListener("click", () => {
      closeComposer();
      setOn(!on);
    });
    $(".cancel").addEventListener("click", closeComposer);
    pop.addEventListener("submit", (e) => {
      e.preventDefault();
      sendComment();
    });
    text.addEventListener("keydown", (e) => {
      e.stopPropagation();
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
    for (const type of ["pointerdown", "mousedown", "pointerup", "mouseup", "dblclick", "auxclick"]) addEventListener(type, block, true);
    addEventListener(
      "click",
      (e) => {
        if (!on || ours(e)) return;
        e.preventDefault();
        e.stopPropagation();
        const el = (
          /** @type {any} */
          e.target
        );
        if (el && el.nodeType === 1 && el !== document.documentElement) openComposer(el);
      },
      true
    );
    addEventListener(
      "mousemove",
      (e) => {
        const t = (
          /** @type {any} */
          e.target
        );
        if (on && pop.hidden && !ours(e)) outline(t && t.nodeType === 1 ? t : null);
      },
      true
    );
    addEventListener(
      "keydown",
      (e) => {
        if (ours(e)) return;
        const t = (
          /** @type {any} */
          e.composedPath()[0] || e.target
        );
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
      true
    );
    addEventListener("scroll", () => picked && outline(picked), true);
  }
  main();
})();
