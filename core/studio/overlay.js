// GENERATED from core/studio/overlay/ by scripts/build-overlay.mjs: edit the modules there, then run npm run build:overlay.
(() => {
  // core/studio/overlay/target.js
  var LANDMARK_TAGS = /* @__PURE__ */ new Set(["HEADER", "NAV", "MAIN", "ASIDE", "FOOTER", "FORM", "DIALOG"]);
  var LANDMARK_ROLES = /* @__PURE__ */ new Set([
    "banner",
    "navigation",
    "main",
    "complementary",
    "contentinfo",
    "region",
    "search",
    "form",
    "dialog",
    "alertdialog",
    "toolbar",
    "tablist"
  ]);
  var isUi = (el) => el && el.closest && el.closest("[data-dfw-ui]");
  var isLayer = (el) => el.hasAttribute("data-dfw-id") || el.hasAttribute("data-dfw-component") || LANDMARK_TAGS.has(el.tagName) || LANDMARK_ROLES.has(el.getAttribute("role")) || el.tagName === "SECTION" && (el.hasAttribute("aria-label") || el.hasAttribute("aria-labelledby"));
  var rectOf = (el) => {
    const r = el.getBoundingClientRect();
    return [r.left, r.top, r.width, r.height];
  };
  function pick(el, exact2) {
    if (!el || el.nodeType !== 1 || el === document.documentElement || el === document.body || isUi(el)) return null;
    if (exact2) return el;
    return el.closest("[data-dfw-id],[data-dfw-component]") || el;
  }
  function labelOf(el) {
    if (el.dataset.dfwComponent) return `${el.dataset.dfwComponent}${el.dataset.dfwVariant ? ` · ${el.dataset.dfwVariant}` : ""}`;
    return el.dataset.dfwId || el.tagName.toLowerCase();
  }
  function cssPath(el) {
    if (el.dataset && el.dataset.dfwId) return `[data-dfw-id="${el.dataset.dfwId}"]`;
    const parts2 = [];
    let n = el;
    while (n && n.nodeType === 1 && n !== document.body) {
      if (n.id) {
        parts2.unshift(`#${CSS.escape(n.id)}`);
        break;
      }
      const anc = n.parentElement && n.parentElement.closest("[data-dfw-id]");
      let sel = n.tagName.toLowerCase();
      const sibs = n.parentElement ? [...n.parentElement.children].filter((c) => c.tagName === n.tagName) : [];
      if (sibs.length > 1) sel += `:nth-of-type(${sibs.indexOf(n) + 1})`;
      parts2.unshift(sel);
      if (anc && n.parentElement === anc) {
        parts2.unshift(`[data-dfw-id="${anc.dataset.dfwId}"]`);
        break;
      }
      n = n.parentElement;
    }
    return parts2.join(" > ");
  }
  var IMPLICIT_ROLE = {
    BUTTON: "button",
    A: "link",
    INPUT: "textbox",
    SELECT: "combobox",
    TEXTAREA: "textbox",
    NAV: "navigation",
    HEADER: "banner",
    FOOTER: "contentinfo",
    MAIN: "main",
    FORM: "form",
    TABLE: "table",
    IMG: "img",
    H1: "heading",
    H2: "heading",
    H3: "heading",
    H4: "heading",
    DIALOG: "dialog",
    UL: "list",
    OL: "list",
    LI: "listitem",
    SECTION: "region",
    ASIDE: "complementary"
  };
  function a11y(el) {
    let role = el.getAttribute("role") || IMPLICIT_ROLE[el.tagName] || null;
    if (el.tagName === "INPUT") {
      const t = (el.getAttribute("type") || "text").toLowerCase();
      role = { checkbox: "checkbox", radio: "radio", search: "searchbox", button: "button", submit: "button", range: "slider" }[t] || role;
    }
    let name = el.getAttribute("aria-label") || el.getAttribute("alt") || el.getAttribute("title") || "";
    if (!name && el.getAttribute("aria-labelledby"))
      name = el.getAttribute("aria-labelledby").split(/\s+/).map((id) => document.getElementById(id)?.textContent || "").join(" ");
    if (!name && el.id) name = document.querySelector(`label[for="${CSS.escape(el.id)}"]`)?.textContent || "";
    if (!name && el.getAttribute("placeholder")) name = el.getAttribute("placeholder");
    if (!name) name = el.textContent || "";
    return { role, name: name.replace(/\s+/g, " ").trim().slice(0, 120) };
  }
  function info(el) {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      dfwId: el.dataset?.dfwId || el.closest("[data-dfw-id]")?.dataset.dfwId || null,
      exact: !el.dataset?.dfwId,
      selector: cssPath(el),
      tag: el.tagName.toLowerCase(),
      component: el.dataset.dfwComponent || null,
      bbox: [Math.round(r.left + scrollX), Math.round(r.top + scrollY), Math.round(r.width), Math.round(r.height)],
      viewportBbox: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
      a11y: a11y(el),
      text: (el.innerText || el.value || "").replace(/\s+/g, " ").trim().slice(0, 160),
      styles: {
        color: cs.color,
        background: cs.backgroundColor,
        font: `${cs.fontWeight} ${cs.fontSize}/${cs.lineHeight} ${cs.fontFamily.split(",")[0]}`,
        padding: cs.padding,
        radius: cs.borderRadius
      },
      scroll: [Math.round(scrollX), Math.round(scrollY)]
    };
  }

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

  // core/studio/overlay/state.js
  var st = {
    /** Studio tool: 'select', 'hand', 'comment', 'interact' or 'present'. */
    mode: (
      /** @type {import('../shared/enums.js').Mode} */
      MODE.SELECT
    ),
    /** @type {HTMLElement | null} */
    hoverEl: null,
    /** @type {HTMLElement | null} */
    selected: null,
    /** @type {{ el: HTMLElement, id: string, orig: string } | null} text edit in progress */
    editing: null,
    /** A middle-button drag pans the Studio canvas. */
    midPan: false,
    /** @type {TokenVar[]} */
    tokenVars: [],
    /** @type {{ key: string, maps: Record<string, Map<string, string[]>> } | null} */
    tokenCache: null,
    /** @type {HTMLElement[]} layer index -> element */
    layerEls: [],
    /** @type {Map<Element, { node: any, parent: HTMLElement | null }>} */
    layerNode: /* @__PURE__ */ new Map(),
    /** @type {{ id: string, selector?: string, dfwId?: string }[]} comment pins of this frame */
    pins: [],
    /** Layout edits on (a design file) and the breakpoint scope they apply at. */
    layoutCfg: { enabled: false, scope: "" },
    /** @type {any} drag to reorder in progress: { el, x, y, active, drop, prevOpacity, prevPointer } */
    drag: null,
    /** @type {any} resize in progress: { el, axis, rect, x, y, moved, pick, prop, list, prev } */
    resize: null,
    /** The click that ends a drag must not re-select (reset on the next mousedown). */
    suppressClick: false,
    /** @type {{ key: string, n: number, list: { name: string, var: string, px: number }[] } | null} */
    lengthCache: null
  };
  var post = (type, data = {}) => parent.postMessage({ type, ...data }, "/");

  // core/studio/overlay/fields.js
  var EDITABLE_FIELD = 'input, textarea, select, [contenteditable]:not([contenteditable="false"])';
  var isTypingTarget = (t) => !!t && (t.isContentEditable === true || typeof t.closest === "function" && !!t.closest(EDITABLE_FIELD));

  // core/studio/overlay/layers.js
  function buildLayers() {
    st.layerEls = [];
    st.layerNode = /* @__PURE__ */ new Map();
    const walk = (el, out, parentNode) => {
      for (
        const c of
        /** @type {HTMLCollectionOf<HTMLElement>} */
        el.children
      ) {
        if (c.hasAttribute("data-dfw-ui") || /^(SCRIPT|STYLE|TEMPLATE|LINK|META|NOSCRIPT)$/.test(c.tagName)) continue;
        if (st.layerEls.length > 3e3) return;
        if (isLayer(c)) {
          const node = {
            i: st.layerEls.length,
            id: c.dataset.dfwId || null,
            comp: c.dataset.dfwComponent || null,
            variant: c.dataset.dfwVariant || null,
            size: c.dataset.dfwSize || null,
            tag: c.tagName.toLowerCase(),
            name: c.getAttribute("aria-label") || null,
            hidden: c.getClientRects().length === 0,
            children: []
          };
          st.layerEls.push(c);
          st.layerNode.set(c, { node, parent: parentNode });
          out.push(node);
          walk(c, node.children, c);
        } else walk(c, out, parentNode);
      }
    };
    const tree = [];
    if (document.body) walk(document.body, tree, null);
    return tree;
  }
  function layerAncestor(el) {
    for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) if (st.layerNode.has(n)) return n;
    return null;
  }
  function navLayer(dir) {
    const selected = st.selected;
    if (!st.layerEls.length || !st.layerNode.has(selected)) buildLayers();
    const cur = st.layerNode.has(selected) ? selected : layerAncestor(selected);
    if (!cur) return null;
    const meta = st.layerNode.get(cur);
    const sibs = meta.parent ? st.layerNode.get(meta.parent).node.children : [...st.layerNode.values()].filter((x) => !x.parent).map((x) => x.node);
    const idx = sibs.findIndex((n) => st.layerEls[n.i] === cur);
    let next = null;
    if (dir === NAV_DIR.UP) next = cur !== selected ? cur : meta.parent;
    else if (dir === NAV_DIR.DOWN) next = meta.node.children[0] ? st.layerEls[meta.node.children[0].i] : null;
    else if (dir === NAV_DIR.LEFT) next = sibs[idx - 1] ? st.layerEls[sibs[idx - 1].i] : null;
    else if (dir === NAV_DIR.RIGHT) next = sibs[idx + 1] ? st.layerEls[sibs[idx + 1].i] : null;
    return next;
  }

  // core/studio/overlay/tokens.js
  var BASE = { color: "rgb(1, 2, 3)", paddingLeft: "0px", fontWeight: "100", boxShadow: "none", fontSize: "3.3px" };
  function buildTokenCache() {
    const box = document.createElement("div");
    box.setAttribute("data-dfw-ui", "");
    box.style.cssText = `position:absolute;left:-99999px;top:0;visibility:hidden;pointer-events:none;color:${BASE.color};font-size:${BASE.fontSize};font-weight:${BASE.fontWeight};`;
    const probes = st.tokenVars.map((t) => {
      const d = document.createElement("div");
      const v = `var(${t.var})`;
      d.style.cssText = `color:${v};padding-left:${v};font-weight:${v};box-shadow:${v};font-size:${v};`;
      box.append(d);
      return d;
    });
    (document.body || document.documentElement).append(box);
    const maps = { color: /* @__PURE__ */ new Map(), length: /* @__PURE__ */ new Map(), fontWeight: /* @__PURE__ */ new Map(), shadow: /* @__PURE__ */ new Map(), fontSize: /* @__PURE__ */ new Map() };
    const add = (m, k, name) => (m.get(k) || m.set(k, []).get(k)).push(name);
    probes.forEach((d, i) => {
      const cs = getComputedStyle(d);
      const name = st.tokenVars[i].name;
      if (cs.color !== BASE.color) add(maps.color, cs.color, name);
      if (cs.paddingLeft !== BASE.paddingLeft) add(maps.length, cs.paddingLeft, name);
      if (cs.fontWeight !== BASE.fontWeight) add(maps.fontWeight, cs.fontWeight, name);
      if (cs.boxShadow !== BASE.boxShadow) add(maps.shadow, cs.boxShadow, name);
      if (cs.fontSize !== BASE.fontSize) add(maps.fontSize, cs.fontSize, name);
    });
    box.remove();
    return { key: `${document.documentElement.dataset.theme || ""}`, maps };
  }
  function lineHeightMap(fontSize) {
    const m = /* @__PURE__ */ new Map();
    const box = document.createElement("div");
    box.setAttribute("data-dfw-ui", "");
    box.style.cssText = `position:absolute;left:-99999px;top:0;visibility:hidden;font-size:${fontSize};line-height:1.111;`;
    const base = document.createElement("div");
    box.append(base);
    const cand = st.tokenVars.filter((t) => /lead|line/i.test(t.name));
    const probes = cand.map((t) => {
      const d = document.createElement("div");
      d.style.lineHeight = `var(${t.var})`;
      box.append(d);
      return d;
    });
    (document.body || document.documentElement).append(box);
    const b = getComputedStyle(base).lineHeight;
    probes.forEach((d, i) => {
      const v = getComputedStyle(d).lineHeight;
      if (v !== b) (m.get(v) || m.set(v, []).get(v)).push(cand[i].name);
    });
    box.remove();
    return m;
  }
  var semanticFirst = (names) => [...names].sort((a, b) => Number(/\.\d/.test(a)) - Number(/\.\d/.test(b)) || a.length - b.length);
  var FAMILY = {
    padding: /^space\b|spacing/i,
    gap: /^space\b|spacing/i,
    margin: /^space\b|spacing/i,
    "border-radius": /radius|rounded/i,
    "font-size": /font|text|size/i,
    "font-weight": /weight|font/i,
    "line-height": /lead|line/i,
    "box-shadow": /shadow|elevation/i
  };
  function match(group, value, map) {
    const names = map.get(value) || [];
    const fam = FAMILY[group];
    const ok = fam ? names.filter((n) => fam.test(n)) : names;
    return semanticFirst(ok);
  }
  function inspect(el) {
    if (!st.tokenCache || st.tokenCache.key !== `${document.documentElement.dataset.theme || ""}`) st.tokenCache = buildTokenCache();
    const M = st.tokenCache.maps;
    const cs = getComputedStyle(el);
    const rows = [];
    const row = (group, prop, value, map) => rows.push({ group, prop, value, tokens: match(group, value, map) });
    const transparent = (c) => c === "rgba(0, 0, 0, 0)" || c === "transparent";
    row("color", "color", cs.color, M.color);
    if (!transparent(cs.backgroundColor)) row("background-color", "background-color", cs.backgroundColor, M.color);
    if (parseFloat(cs.borderTopWidth) > 0 && cs.borderTopStyle !== "none" && !transparent(cs.borderTopColor))
      row("border-color", "border-color", cs.borderTopColor, M.color);
    row("font-size", "font-size", cs.fontSize, M.fontSize);
    row("font-weight", "font-weight", cs.fontWeight, M.fontWeight);
    if (cs.lineHeight !== "normal") row("line-height", "line-height", cs.lineHeight, lineHeightMap(cs.fontSize));
    const sides = (group, names) => {
      const v = names.map((n) => cs.getPropertyValue(n));
      if (v.every((x) => x === "0px" || x === "auto")) return;
      if (v.every((x) => x === v[0])) return row(group, group, v[0], M.length);
      if (v[0] === v[2] && v[1] === v[3] && group !== "border-radius") {
        if (v[0] !== "0px") row(group, `${group}-block`, v[0], M.length);
        if (v[1] !== "0px") row(group, `${group}-inline`, v[1], M.length);
        return;
      }
      names.forEach((n, i) => v[i] !== "0px" && v[i] !== "auto" && row(group, n, v[i], M.length));
    };
    sides("padding", ["padding-top", "padding-right", "padding-bottom", "padding-left"]);
    if (/flex|grid/.test(cs.display)) {
      const rg = cs.rowGap;
      const cg = cs.columnGap;
      if (rg === cg) {
        if (rg !== "normal" && rg !== "0px") row("gap", "gap", rg, M.length);
      } else {
        if (rg !== "normal" && rg !== "0px") row("gap", "row-gap", rg, M.length);
        if (cg !== "normal" && cg !== "0px") row("gap", "column-gap", cg, M.length);
      }
    }
    sides("margin", ["margin-top", "margin-right", "margin-bottom", "margin-left"]);
    sides("border-radius", ["border-top-left-radius", "border-top-right-radius", "border-bottom-right-radius", "border-bottom-left-radius"]);
    if (cs.boxShadow !== "none") row("box-shadow", "box-shadow", cs.boxShadow, M.shadow);
    return rows;
  }

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

  // core/studio/overlay/select.js
  function selPayload(el, withInspect) {
    const t = editTarget(el);
    return {
      target: info(el),
      rect: rectOf(el),
      label: labelOf(el),
      layer: st.layerNode.get(el)?.node.i ?? null,
      own: {
        id: el.dataset.dfwId || null,
        comp: el.dataset.dfwComponent || null,
        variant: el.dataset.dfwVariant || null,
        size: el.dataset.dfwSize || null
      },
      textEditable: !t.error,
      inspect: withInspect ? inspect(el) : null
    };
  }
  function select(el, { inspectIt = true, scroll = false } = {}) {
    st.selected = el;
    placeHandles();
    if (!el) return post(MSG.SELECT, { target: null });
    if (scroll) el.scrollIntoView({ block: "nearest" });
    post(MSG.SELECT, { comment: st.mode === MODE.COMMENT, ...selPayload(el, inspectIt && st.mode !== MODE.COMMENT) });
  }
  function reportSelection() {
    if (st.selected && st.selected.isConnected) post(MSG.SEL_RECT, { rect: rectOf(st.selected) });
    placeHandles();
  }
  function reportPins() {
    const out = [];
    for (const p of st.pins) {
      let el = null;
      try {
        el = p.selector && document.querySelector(p.selector) || p.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(p.dfwId)}"]`) || null;
      } catch {
      }
      if (el && el.getClientRects().length) out.push({ id: p.id, rect: rectOf(el) });
    }
    post(MSG.PIN_RECTS, { pins: out });
  }
  function reportSize() {
    const d = document.documentElement;
    post(MSG.SIZE, { h: Math.max(d.scrollHeight, document.body ? document.body.scrollHeight : 0), title: document.title });
  }

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
    statesSummary: (all, on) => `States: ${all ? "all" : !on.length ? "off" : on.length <= 2 ? on.join(", ") : on.length}`,
    // Pins
    pinTitle: (n, status) => `Comment #${n} · ${status}`,
    pinLabel: (n, text) => `Comment ${n}: ${text}`,
    pinsHidden: "Comment pins hidden (Shift+C shows them).",
    pinsAll: "Showing every pin, closed comments too.",
    pinsActive: "Showing pins of open and to-verify comments.",
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
    alreadyAt: (label) => `Already ${label}.`,
    elementNotFound: (what) => `Element ${what} not found in this frame.`,
    layoutConflict: (id, prop, by) => `Saved, but ${id} did not change: ${by ? `"${by}"` : "another rule"} still sets ${prop}${by === "style attribute" ? "" : " with higher priority"}. Remove ${prop} there (or ask the AI), then resize again.`,
    // Live review (in the running app)
    liveToggleTitle: (shortcut) => `Comment on an element${shortcut ? ` (${shortcut})` : ""}`,
    liveSent: (n) => `Sent to AI · comment #${n}`,
    liveNotSent: (why) => `Not sent: ${why}`
  });

  // core/studio/overlay/layout.js
  var FLOW = /^(inline-)?(flex|grid)$/;
  var isFlow = (el) => !!el && el.nodeType === 1 && FLOW.test(getComputedStyle(el).display);
  var isGrid = (el) => /grid$/.test(getComputedStyle(el).display);
  var idKids = (c) => (
    /** @type {HTMLElement[]} */
    [...c.children].filter(
      (x) => x.dataset && x.dataset.dfwId && !x.hasAttribute("data-dfw-ui") && x.getClientRects().length
    )
  );
  var canLayout = (el) => st.layoutCfg.enabled && st.mode === MODE.SELECT && !st.editing && el && el.isConnected && !!el.dataset.dfwId && el !== document.body;
  var canMove = (el) => canLayout(el) && isFlow(el.parentElement);
  var PERCENTS = [25, 33.333, 50, 66.667, 75, 100];
  var HANDLE = "width:10px;height:10px;background:#fff;border:2px solid #0d99ff;border-radius:2px;pointer-events:auto;";
  var parts = null;
  function ui() {
    if (parts) return parts;
    const root = document.createElement("div");
    root.setAttribute("data-dfw-ui", "");
    root.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483646;";
    const mk = (css) => {
      const d = document.createElement("div");
      d.setAttribute("data-dfw-ui", "");
      d.style.cssText = `position:fixed;display:none;box-sizing:border-box;${css}`;
      root.append(d);
      return d;
    };
    const hX = mk(`${HANDLE}cursor:ew-resize;`);
    const hY = mk(`${HANDLE}cursor:ns-resize;`);
    hX.dataset.dfwHandle = "x";
    hY.dataset.dfwHandle = "y";
    hX.title = TEXT.widthHandle;
    hY.title = TEXT.minHeightHandle;
    const line = mk("background:#0d99ff;border-radius:2px;");
    const zone = mk("border:2px dashed #0d99ff;background:rgba(13,153,255,.12);border-radius:4px;");
    const badge = mk(
      "padding:2px 6px;border-radius:4px;background:#0d99ff;color:#fff;font:600 11px/16px system-ui,sans-serif;white-space:nowrap;"
    );
    parts = { ui: root, hX, hY, line, zone, badge };
    return parts;
  }
  var mountUi = () => !ui().ui.isConnected && (document.body || document.documentElement).append(ui().ui);
  function placeHandles() {
    const { hX, hY } = ui();
    const show = !st.drag?.active && !st.resize?.moved && st.selected && canLayout(st.selected) && st.selected.getClientRects().length;
    hX.style.display = hY.style.display = show ? "block" : "none";
    if (!show) return;
    mountUi();
    const r = st.selected.getBoundingClientRect();
    hX.style.left = `${r.right - 5}px`;
    hX.style.top = `${r.top + r.height / 2 - 5}px`;
    hY.style.left = `${r.left + r.width / 2 - 5}px`;
    hY.style.top = `${r.bottom - 5}px`;
  }
  function showBadge(text, x, y) {
    const { badge } = ui();
    mountUi();
    badge.textContent = text;
    badge.style.display = "block";
    badge.style.left = `${Math.min(x + 12, innerWidth - badge.offsetWidth - 4)}px`;
    badge.style.top = `${Math.max(4, y - 28)}px`;
  }
  var hideFeedback = () => {
    const { line, zone, badge } = ui();
    line.style.display = "none";
    zone.style.display = "none";
    badge.style.display = "none";
  };
  var scoped = (t) => st.layoutCfg.scope ? `${t} · ${st.layoutCfg.scope}` : t;
  function tokenLengths() {
    const key = document.documentElement.dataset.theme || "";
    if (st.lengthCache && st.lengthCache.key === key && st.lengthCache.n === st.tokenVars.length) return st.lengthCache.list;
    const box = document.createElement("div");
    box.setAttribute("data-dfw-ui", "");
    box.style.cssText = "position:absolute;left:-99999px;top:0;visibility:hidden;";
    const cand = st.tokenVars.filter((t) => /^(space|size)\./.test(t.name) && t.type !== "typography");
    const probes = cand.map((t) => {
      const d = document.createElement("div");
      d.style.cssText = `width:var(${t.var});height:0;`;
      box.append(d);
      return d;
    });
    (document.body || document.documentElement).append(box);
    const list = probes.map((d, i) => ({ name: cand[i].name, var: cand[i].var, px: d.getBoundingClientRect().width })).filter((t) => t.px > 0);
    box.remove();
    list.sort((a, b) => a.px - b.px || Number(/\.\d/.test(a.name)) - Number(/\.\d/.test(b.name)));
    st.lengthCache = { key, n: st.tokenVars.length, list };
    return list;
  }
  function snapOptions(el, axis) {
    const p = el.parentElement;
    if (axis === "x" && isGrid(p)) {
      const cs = getComputedStyle(p);
      const tracks = cs.gridTemplateColumns.split(/\s+/).filter((t) => /px$/.test(t)).map(parseFloat);
      const gap = parseFloat(cs.columnGap) || 0;
      const n = Math.max(1, tracks.length);
      const track = tracks.length ? tracks.reduce((a, b) => a + b, 0) / tracks.length : p.clientWidth;
      return {
        prop: LAYOUT_PROP.GRID_COLUMN,
        list: Array.from({ length: n }, (_, i) => ({
          value: `span ${i + 1}`,
          css: `span ${i + 1}`,
          label: `span ${i + 1} / ${n}`,
          px: (i + 1) * track + i * gap
        }))
      };
    }
    const toks = tokenLengths();
    if (axis === "x") {
      const pcs = getComputedStyle(p);
      const inner = p.clientWidth - parseFloat(pcs.paddingLeft) - parseFloat(pcs.paddingRight);
      const pct = PERCENTS.map((v) => ({ value: `${v}%`, css: `${v}%`, label: `${Math.round(v)}%`, px: inner * v / 100 }));
      const sizes = toks.filter((t) => t.name.startsWith("size.") && t.px <= inner).map((t) => ({ value: t.name, css: `var(${t.var})`, label: `${t.name} · ${Math.round(t.px)}px`, px: t.px }));
      return { prop: LAYOUT_PROP.WIDTH, list: [...pct, ...sizes].sort((a, b) => a.px - b.px) };
    }
    return {
      prop: LAYOUT_PROP.MIN_HEIGHT,
      list: toks.map((t) => ({ value: t.name, css: `var(${t.var})`, label: `${t.name} · ${Math.round(t.px)}px`, px: t.px }))
    };
  }
  var nearest = (list, px) => list.reduce((best, o) => Math.abs(o.px - px) < Math.abs(best.px - px) ? o : best, list[0]);
  var styleProp = { [LAYOUT_PROP.GRID_COLUMN]: "gridColumn", [LAYOUT_PROP.WIDTH]: "width", [LAYOUT_PROP.MIN_HEIGHT]: "minHeight" };
  function startResize(axis, e) {
    const el = st.selected;
    const opts = snapOptions(el, axis);
    if (!opts.list.length) return post(MSG.TOAST, { message: TEXT.noSizeTokens });
    st.resize = {
      el,
      axis,
      rect: el.getBoundingClientRect(),
      x: e.clientX,
      y: e.clientY,
      moved: false,
      ...opts,
      pick: null,
      prev: el.style[styleProp[opts.prop]]
    };
    placeHandles();
  }
  function moveResize(e) {
    const r = st.resize;
    if (!r.moved && Math.hypot(e.clientX - r.x, e.clientY - r.y) < 3) return;
    if (!r.moved) {
      r.moved = true;
      placeHandles();
    }
    const px = r.axis === "x" ? e.clientX - r.rect.left : e.clientY - r.rect.top;
    const o = nearest(r.list, px);
    if (o !== r.pick) {
      r.pick = o;
      r.el.style[styleProp[r.prop]] = o.css;
      reportSelection();
    }
    showBadge(scoped(r.prop === LAYOUT_PROP.GRID_COLUMN ? o.label : `${r.prop} ${o.label}`), e.clientX, e.clientY);
  }
  function endResize(commit) {
    const r = st.resize;
    st.resize = null;
    hideFeedback();
    if (commit && r.moved && r.pick)
      post(MSG.LAYOUT_OP, { op: EDIT_OP.LAYOUT, id: r.el.dataset.dfwId, prop: r.prop, value: r.pick.value, label: r.pick.label });
    else r.el.style[styleProp[r.prop]] = r.prev;
    placeHandles();
    reportSelection();
  }
  var isEmptyBox = (n) => n.dataset && n.dataset.dfwId && isFlow(n) && ![...n.children].some((k) => !k.hasAttribute("data-dfw-ui")) && !n.textContent.trim();
  function dropAt(x, y) {
    const d = st.drag.el;
    let n = (
      /** @type {any} */
      document.elementFromPoint(x, y)
    );
    let c = null;
    const home = d.parentElement;
    for (let e = n; e && e !== home && e !== document.documentElement; e = e.parentElement) {
      if (isUi(e) || d.contains(e)) break;
      if (isEmptyBox(e)) return { ref: e, position: DROP.INSIDE, r: e.getBoundingClientRect() };
      if (isFlow(e) && idKids(e).length) break;
    }
    const hr = home.getBoundingClientRect();
    if (x >= hr.left && x <= hr.right && y >= hr.top && y <= hr.bottom) c = home;
    for (; !c && n && n !== document.documentElement; n = n.parentElement) {
      if (isUi(n) || d.contains(n)) return null;
      if (isFlow(n) && idKids(n).some((k2) => k2 !== d)) c = n;
    }
    if (!c) return null;
    const kids = idKids(c).filter((k2) => k2 !== d);
    if (!kids.length) return null;
    const dist = (r2) => Math.hypot(Math.max(r2.left - x, 0, x - r2.right), Math.max(r2.top - y, 0, y - r2.bottom));
    let best = null;
    for (const k2 of kids) {
      const r2 = k2.getBoundingClientRect();
      const dd = dist(r2);
      if (!best || dd < best.dd) best = { k: k2, r: r2, dd };
    }
    const { k, r } = best;
    let horizontal;
    if (isGrid(c)) horizontal = y >= r.top && y <= r.bottom;
    else horizontal = getComputedStyle(c).flexDirection.startsWith("row");
    const before = horizontal ? x < r.left + r.width / 2 : y < r.top + r.height / 2;
    return { ref: k, position: before ? DROP.BEFORE : DROP.AFTER, vertical: horizontal, r };
  }
  function armDrag(e) {
    st.drag = { el: st.selected, x: e.clientX, y: e.clientY, active: false };
  }
  function moveDrag(e) {
    const drag = st.drag;
    const { line, zone } = ui();
    if (!drag.active) {
      if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
      drag.active = true;
      drag.prevOpacity = drag.el.style.opacity;
      drag.prevPointer = drag.el.style.pointerEvents;
      drag.el.style.opacity = "0.4";
      drag.el.style.pointerEvents = "none";
      placeHandles();
      post(MSG.HOVER, { rect: null });
    }
    const t = drag.drop = dropAt(e.clientX, e.clientY);
    mountUi();
    if (!t) {
      line.style.display = zone.style.display = "none";
      return showBadge("Drop next to a sibling in a flex or grid container, or into an empty one", e.clientX, e.clientY);
    }
    const { r } = t;
    if (t.position === DROP.INSIDE) {
      line.style.display = "none";
      Object.assign(zone.style, {
        left: `${r.left}px`,
        top: `${r.top}px`,
        width: `${r.width}px`,
        height: `${Math.max(r.height, 8)}px`,
        display: "block"
      });
      return showBadge(`Into ${t.ref.dataset.dfwId}`, e.clientX, e.clientY);
    }
    zone.style.display = "none";
    const edge = t.position === DROP.BEFORE ? t.vertical ? r.left : r.top : t.vertical ? r.right : r.bottom;
    Object.assign(
      line.style,
      t.vertical ? { left: `${edge - 2}px`, top: `${r.top}px`, width: "3px", height: `${r.height}px` } : { left: `${r.left}px`, top: `${edge - 2}px`, width: `${r.width}px`, height: "3px" }
    );
    line.style.display = "block";
    showBadge(`${t.position === DROP.BEFORE ? "Before" : "After"} ${t.ref.dataset.dfwId}`, e.clientX, e.clientY);
  }
  function endDrag(commit) {
    const g = st.drag;
    st.drag = null;
    hideFeedback();
    if (!g?.active) return placeHandles();
    g.el.style.opacity = g.prevOpacity;
    g.el.style.pointerEvents = g.prevPointer;
    st.suppressClick = true;
    const t = g.drop;
    if (commit && t && t.position === DROP.INSIDE) {
      t.ref.append(g.el);
      post(MSG.LAYOUT_OP, {
        op: EDIT_OP.MOVE,
        id: g.el.dataset.dfwId,
        ref: t.ref.dataset.dfwId,
        position: DROP.INSIDE,
        label: `Moved into ${t.ref.dataset.dfwId}`
      });
    } else if (commit && t) {
      const next = t.position === DROP.BEFORE ? t.ref : t.ref.nextSibling;
      if (next !== g.el && !(t.position === DROP.BEFORE && g.el.nextElementSibling === t.ref) && !(t.position === DROP.AFTER && t.ref.nextElementSibling === g.el)) {
        t.ref.parentElement.insertBefore(g.el, next);
        post(MSG.LAYOUT_OP, {
          op: EDIT_OP.MOVE,
          id: g.el.dataset.dfwId,
          ref: t.ref.dataset.dfwId,
          position: t.position,
          label: `Moved ${t.position} ${t.ref.dataset.dfwId}`
        });
      }
    }
    placeHandles();
    reportSelection();
  }
  function resetLayout(el, prop = null) {
    for (const p of prop ? [prop] : Object.keys(styleProp)) el.style[styleProp[p]] = "";
    reportSelection();
    placeHandles();
    post(MSG.LAYOUT_OP, {
      op: EDIT_OP.LAYOUT,
      id: el.dataset.dfwId,
      prop,
      value: null,
      label: prop ? `${prop} reset to auto` : "Size reset to auto"
    });
  }
  function layoutKey(key, shift) {
    const el = st.selected;
    if (!canLayout(el)) return post(MSG.TOAST, { message: TEXT.needsIdToMove });
    if (key === LAYOUT_KEY.RESET) return resetLayout(el);
    const back = key === KEY.LEFT || key === KEY.UP;
    if (!shift) {
      if (!canMove(el)) return post(MSG.TOAST, { message: TEXT.onlyFlexGridChildren });
      const sibs = idKids(el.parentElement);
      const i = sibs.indexOf(el);
      const ref = sibs[back ? i - 1 : i + 1];
      if (!ref) return post(MSG.TOAST, { message: back ? TEXT.alreadyFirst : TEXT.alreadyLast });
      const position = back ? DROP.BEFORE : DROP.AFTER;
      ref.parentElement.insertBefore(el, back ? ref : ref.nextSibling);
      reportSelection();
      return post(MSG.LAYOUT_OP, {
        op: EDIT_OP.MOVE,
        id: el.dataset.dfwId,
        ref: ref.dataset.dfwId,
        position,
        label: `Moved ${position} ${ref.dataset.dfwId}`
      });
    }
    const axis = key === KEY.LEFT || key === KEY.RIGHT ? "x" : "y";
    const { prop, list } = snapOptions(el, axis);
    if (!list.length) return post(MSG.TOAST, { message: TEXT.noSizeTokens });
    let cur;
    if (prop === LAYOUT_PROP.GRID_COLUMN) {
      const m = /span\s+(\d+)/.exec(getComputedStyle(el).gridColumnEnd) || /span\s+(\d+)/.exec(getComputedStyle(el).gridColumnStart);
      cur = list.findIndex((o2) => o2.value === `span ${m ? m[1] : 1}`);
    } else {
      const now = axis === "x" ? el.getBoundingClientRect().width : parseFloat(getComputedStyle(el).minHeight) || 0;
      cur = list.indexOf(nearest(list, now));
      if (axis === "y" && !now) cur = -1;
    }
    const o = list[Math.max(0, Math.min(list.length - 1, cur + (back ? -1 : 1)))];
    if (cur >= 0 && o === list[cur]) return post(MSG.TOAST, { message: TEXT.alreadyAt(o.label) });
    el.style[styleProp[prop]] = o.css;
    reportSelection();
    post(MSG.LAYOUT_OP, { op: EDIT_OP.LAYOUT, id: el.dataset.dfwId, prop, value: o.value, label: o.label });
  }

  // core/studio/overlay/edit.js
  var norm = (s) => (s || "").replace(/\s+/g, " ").trim();
  function editTarget(el) {
    if (!el || el.children.length) return { error: TEXT.onlyTextOnly };
    if (/^(INPUT|TEXTAREA|SELECT|IMG|SVG|svg)$/.test(el.tagName) || !norm(el.textContent)) return { error: TEXT.noText };
    if (el.dataset.dfwId) return { el, id: el.dataset.dfwId };
    const host = el.closest("[data-dfw-id]");
    if (host && host.dataset.dfwComponent && norm(host.textContent) === norm(el.textContent)) return { el, id: host.dataset.dfwId };
    return { error: TEXT.needsIdToEdit };
  }
  function startEdit(el) {
    const t = editTarget(el);
    if (t.error) return post(MSG.TOAST, { message: t.error });
    st.editing = { el: t.el, id: t.id, orig: t.el.textContent };
    t.el.setAttribute("contenteditable", "plaintext-only");
    t.el.focus();
    const range = document.createRange();
    range.selectNodeContents(t.el);
    const sel = getSelection();
    sel.removeAllRanges();
    sel.addRange(range);
    t.el.addEventListener("blur", onEditBlur);
    placeHandles();
  }
  function onEditBlur() {
    endEdit(true);
  }
  function endEdit(save) {
    if (!st.editing) return;
    const { el, id, orig } = st.editing;
    st.editing = null;
    el.removeEventListener("blur", onEditBlur);
    el.removeAttribute("contenteditable");
    const text = el.textContent;
    if (!save) el.textContent = orig;
    getSelection()?.removeAllRanges();
    if (save && norm(text) !== norm(orig)) post(MSG.EDIT_TEXT, { id, text: norm(text) });
    reportSelection();
  }

  // core/studio/overlay/events.js
  var capture = { capture: true };
  var interactive = () => st.mode === MODE.SELECT || st.mode === MODE.COMMENT;
  var inEdit = (t) => st.editing && st.editing.el.contains(
    /** @type {Node} */
    t
  );
  var exact = (e) => e.metaKey || e.ctrlKey || e.altKey;
  var hoverPayload = (t) => ({ rect: rectOf(t), label: labelOf(t), comp: !!t.dataset.dfwComponent });
  var raf = 0;
  function onMouseMove(e) {
    if (st.midPan) return post(MSG.PAN, { phase: PHASE.MOVE, sx: e.screenX, sy: e.screenY });
    if (st.resize || st.drag) {
      if (!(e.buttons & 1)) return st.resize ? endResize(false) : endDrag(false);
      return st.resize ? moveResize(e) : moveDrag(e);
    }
    if (!interactive() || st.editing) return;
    const t = pick(e.target, exact(e));
    if (t === st.hoverEl) return;
    st.hoverEl = t;
    cancelAnimationFrame(raf);
    raf = requestAnimationFrame(() => post(MSG.HOVER, t ? hoverPayload(t) : { rect: null }));
  }
  function onMouseDown(e) {
    const t = (
      /** @type {HTMLElement} */
      e.target
    );
    if (e.button === 1 && st.mode !== MODE.PRESENT) {
      e.preventDefault();
      st.midPan = true;
      return post(MSG.PAN, { phase: PHASE.START, sx: e.screenX, sy: e.screenY });
    }
    st.suppressClick = false;
    if (inEdit(t)) return;
    if (st.editing) endEdit(true);
    if (interactive() && !isUi(t)) {
      e.preventDefault();
      e.stopPropagation();
    }
    post(MSG.FOCUS);
    if (e.button !== 0 || st.mode !== MODE.SELECT) return;
    if (t.dataset && t.dataset.dfwHandle && canLayout(st.selected)) {
      e.preventDefault();
      e.stopPropagation();
      return startResize(t.dataset.dfwHandle, e);
    }
    if (st.selected && st.selected.contains(t) && canMove(st.selected) && !exact(e)) armDrag(e);
  }
  function onMouseUp(e) {
    if (st.midPan) {
      st.midPan = false;
      e.preventDefault();
      post(MSG.PAN, { phase: PHASE.END });
    }
    if (st.resize) endResize(true);
    else if (st.drag) endDrag(true);
  }
  function onClick(e) {
    if (st.suppressClick) {
      st.suppressClick = false;
      e.preventDefault();
      e.stopPropagation();
      return;
    }
    if (inEdit(e.target) || isUi(e.target)) return;
    if (!interactive()) return prototypeClick(e);
    e.preventDefault();
    e.stopPropagation();
    const t = pick(e.target, exact(e));
    if (!st.layerEls.length) buildLayers();
    select(t);
  }
  function onDblClick(e) {
    if (st.mode !== MODE.SELECT || inEdit(e.target)) return;
    e.preventDefault();
    e.stopPropagation();
    const t = (
      /** @type {HTMLElement} */
      e.target
    );
    if (t.dataset && t.dataset.dfwHandle && canLayout(st.selected))
      return resetLayout(st.selected, snapOptions(st.selected, t.dataset.dfwHandle).prop);
    if (t.nodeType === 1 && !t.children.length && norm(t.textContent)) {
      select(t, { inspectIt: true });
      startEdit(t);
    } else if (t.nodeType === 1) select(t);
  }
  var fwd = (e, type) => post(MSG.KEY, { phase: type, key: e.key, code: e.code, meta: e.metaKey, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey });
  function onKeyDown(e) {
    if ((st.resize || st.drag) && e.key === KEY.ESCAPE) {
      e.preventDefault();
      return st.resize ? endResize(false) : endDrag(false);
    }
    if (st.editing) {
      if (e.key === KEY.ENTER && !e.shiftKey) {
        e.preventDefault();
        endEdit(true);
      } else if (e.key === KEY.ESCAPE) {
        e.preventDefault();
        endEdit(false);
      }
      return;
    }
    const target = (
      /** @type {any} */
      e.target
    );
    const typing = isTypingTarget(target);
    if (st.mode === MODE.PRESENT || st.mode === MODE.INTERACT) {
      if (typing) return e.key === KEY.ESCAPE && fwd(e, PHASE.DOWN);
      if (st.mode === MODE.PRESENT && (e.key === KEY.LEFT || e.key === KEY.RIGHT) && !e.metaKey && !e.ctrlKey) e.preventDefault();
      if (st.mode === MODE.INTERACT && e.key === KEY.SPACE) e.preventDefault();
      return fwd(e, PHASE.DOWN);
    }
    if (typing && !interactive()) return;
    if (
      /** @type {string[]} */
      [KEY.SPACE, KEY.UP, KEY.DOWN, KEY.LEFT, KEY.RIGHT, KEY.TAB].includes(e.key) || e.metaKey || e.ctrlKey
    ) {
      if (!(e.metaKey && ["c", "v", "x", "a", "r", "q", "w", "t"].includes(e.key.toLowerCase()))) e.preventDefault();
    }
    fwd(e, PHASE.DOWN);
  }
  function prototypeClick(e) {
    const target = (
      /** @type {any} */
      e.target
    );
    const goto = target.closest && target.closest("[data-dfw-goto]");
    if (goto) {
      e.preventDefault();
      e.stopPropagation();
      return post(MSG.GOTO, { goto: goto.getAttribute("data-dfw-goto") });
    }
    const a = target.closest && target.closest("a[href]");
    if (!a || e.defaultPrevented) return;
    const href = a.getAttribute("href");
    if (!href || href.startsWith("#") || /^(javascript|mailto|tel):/i.test(href)) return;
    const url = new URL(href, location.href);
    e.preventDefault();
    if (url.origin !== location.origin) return void window.open(url.href, "_blank", "noopener");
    post(MSG.GOTO, { href: url.pathname + url.search });
  }
  function listenInput() {
    document.addEventListener("mousemove", onMouseMove, capture);
    document.addEventListener("mouseleave", () => {
      st.hoverEl = null;
      post(MSG.HOVER, { rect: null });
    });
    document.addEventListener("mousedown", onMouseDown, capture);
    document.addEventListener("mouseup", onMouseUp, capture);
    document.addEventListener("auxclick", (e) => e.button === 1 && e.preventDefault(), capture);
    document.addEventListener("click", onClick, capture);
    document.addEventListener("dblclick", onDblClick, capture);
    document.addEventListener("submit", (e) => e.cancelable && e.preventDefault(), capture);
    addEventListener(
      "wheel",
      (e) => {
        if (st.mode === MODE.PRESENT) return;
        e.preventDefault();
        post(MSG.WHEEL, {
          dx: e.deltaX,
          dy: e.deltaY,
          dm: e.deltaMode,
          ctrl: e.ctrlKey || e.metaKey,
          shift: e.shiftKey,
          x: e.clientX,
          y: e.clientY
        });
      },
      { passive: false, capture: true }
    );
    document.addEventListener("keydown", onKeyDown, capture);
    document.addEventListener("keyup", (e) => !st.editing && st.mode !== MODE.PRESENT && e.key === KEY.SPACE && fwd(e, PHASE.UP), capture);
    addEventListener(
      "scroll",
      () => {
        reportSelection();
        if (st.pins.length) reportPins();
        if (st.hoverEl) post(MSG.HOVER, hoverPayload(st.hoverEl));
      },
      true
    );
  }

  // core/studio/overlay/verify.js
  function rulesOf(rules, pick2, out = []) {
    for (
      const r of
      /** @type {any[]} */
      [...rules]
    ) {
      if (r.cssRules && !r.selectorText) {
        if (!r.media || matchMedia(r.media.mediaText).matches) rulesOf(r.cssRules, pick2, out);
      } else if (r.selectorText && pick2(r)) out.push(r);
    }
    return out;
  }
  function verifyLayout(d) {
    const el = (
      /** @type {HTMLElement | null} */
      document.querySelector(`[data-dfw-id="${CSS.escape(d.id)}"]`)
    );
    const sheet = (
      /** @type {HTMLStyleElement | null} */
      document.querySelector("style[data-dfw-layout]")?.sheet
    );
    if (!el || !sheet || !styleProp[d.prop]) return;
    const own = rulesOf(sheet.cssRules, (r) => r.style.getPropertyValue(d.prop) && el.matches(r.selectorText));
    if (!own.length) return;
    const read = () => (d.prop === LAYOUT_PROP.GRID_COLUMN ? ["gridColumnStart", "gridColumnEnd"] : [styleProp[d.prop]]).map((k) => getComputedStyle(el)[k]).join(" / ");
    const prev = el.style.getPropertyValue(d.prop);
    const prevPrio = el.style.getPropertyPriority(d.prop);
    const now = read();
    el.style.setProperty(d.prop, own.at(-1).style.getPropertyValue(d.prop), "important");
    const ideal = read();
    el.style.setProperty(d.prop, prev, prevPrio);
    if (now === ideal) return;
    const by = conflictFor(el, d.prop, sheet);
    post(MSG.LAYOUT_CONFLICT, { id: d.id, prop: d.prop, selector: by });
    post(MSG.TOAST, {
      message: TEXT.layoutConflict(d.id, d.prop, by)
    });
  }
  var LONGHANDS = {
    [LAYOUT_PROP.GRID_COLUMN]: ["grid-column", "grid-column-start", "grid-column-end", "grid-area"],
    [LAYOUT_PROP.WIDTH]: ["width"],
    [LAYOUT_PROP.MIN_HEIGHT]: ["min-height"]
  };
  function specificity(sel) {
    const s = sel.replace(/:where\([^)]*\)/g, "").replace(/:(?:not|is|has)\(/g, "(").replace(/"[^"]*"|'[^']*'/g, "");
    return [
      (s.match(/#[\w-]/g) || []).length,
      (s.match(/\.[\w-]|\[|:(?!:)[\w-]/g) || []).length,
      (s.replace(/[#.:][\w-]+|\[[^\]]*\]/g, " ").match(/(^|[\s>+~(,])[a-z][\w-]*/gi) || []).length
    ];
  }
  function conflictFor(el, prop, dfwSheet) {
    const names = LONGHANDS[prop];
    if (names.some((n) => el.style.getPropertyValue(n))) return "style attribute";
    const hits = [];
    const matches = (x) => {
      try {
        return el.matches(x);
      } catch {
        return false;
      }
    };
    for (const sh of document.styleSheets) {
      if (sh === dfwSheet) continue;
      let rules = [];
      try {
        rules = rulesOf(sh.cssRules, (r) => names.some((n) => r.style.getPropertyValue(n)));
      } catch {
      }
      for (const r of rules) {
        const sel = r.selectorText.split(",").map((x) => x.trim()).find(matches);
        const n = names.find((x) => r.style.getPropertyValue(x));
        if (sel) hits.push({ sel, imp: r.style.getPropertyPriority(n) === "important", sp: specificity(sel) });
      }
    }
    hits.sort((a, b) => Number(b.imp) - Number(a.imp) || b.sp[0] - a.sp[0] || b.sp[1] - a.sp[1] || b.sp[2] - a.sp[2]);
    return hits[0]?.sel || null;
  }

  // core/studio/overlay/morph.js
  var keyOf = (n) => n.nodeType === 1 ? (
    /** @type {Element} */
    n.getAttribute("data-dfw-id") || /** @type {Element} */
    n.id || null
  ) : null;
  var sameKind = (a, b) => a.nodeType === b.nodeType && a.nodeName === b.nodeName;
  var sheetPath = (href) => {
    try {
      return new URL(href, location.href).pathname;
    } catch {
      return href;
    }
  };
  var NO_NAMES = /* @__PURE__ */ new Set();
  var ROOT_OWNED = /* @__PURE__ */ new Set(["data-theme", "data-state", "style"]);
  var hasScript = (n) => n.nodeType === 1 && (n.nodeName === "SCRIPT" || /** @type {Element} */
  n.querySelector("script"));
  function syncAttrs(from, to, changed, keepNames = NO_NAMES) {
    if (from.nodeName === "LINK" && sheetPath(from.getAttribute("href") || "") === sheetPath(to.getAttribute("href") || "")) return;
    let diff = false;
    for (const a of [...from.attributes]) {
      if (a.name === "contenteditable" || keepNames.has(a.name) || to.hasAttribute(a.name)) continue;
      from.removeAttribute(a.name);
      diff = true;
    }
    for (const a of [...to.attributes]) {
      if (keepNames.has(a.name) || from.getAttribute(a.name) === a.value) continue;
      from.setAttribute(a.name, a.value);
      diff = true;
    }
    if (diff) changed.add(from);
  }
  function morph(from, to, changed, keep = null) {
    if (keep && from === keep) return;
    syncAttrs(from, to, changed);
    const olds = [...from.childNodes].filter((n) => !(n.nodeType === 1 && /** @type {Element} */
    n.hasAttribute("data-dfw-ui")));
    const keyed = /* @__PURE__ */ new Map();
    for (const o of olds) {
      const k = keyOf(o);
      if (k && !keyed.has(k)) keyed.set(k, o);
    }
    const used = /* @__PURE__ */ new Set();
    const plan = [];
    let i = 0;
    for (const n of [...to.childNodes]) {
      const k = keyOf(n);
      let match2 = null;
      if (k) {
        const o = keyed.get(k);
        if (o && !used.has(o) && sameKind(o, n)) match2 = o;
      } else {
        while (i < olds.length && used.has(olds[i])) i++;
        for (let j = i; j < Math.min(olds.length, i + 8) && !match2; j++)
          if (!used.has(olds[j]) && !keyOf(olds[j]) && olds[j].isEqualNode(n)) match2 = olds[j];
        if (!match2 && olds[i] && sameKind(olds[i], n) && !keyOf(olds[i])) match2 = olds[i];
      }
      if (match2) used.add(match2);
      plan.push([n, match2]);
    }
    for (const o of olds) {
      if (used.has(o)) continue;
      if (hasScript(o)) throw new Error("script removed");
      o.remove();
      if (o.nodeType === 1 || (o.nodeValue || "").trim()) changed.add(from);
    }
    let prev = null;
    for (const [n, match2] of plan) {
      const want = prev ? prev.nextSibling : from.firstChild;
      let node = match2;
      if (!node) {
        if (hasScript(n)) throw new Error("script added");
        node = /** @type {ChildNode} */
        from.ownerDocument.importNode(n, true);
        from.insertBefore(node, want);
        if (node.nodeType === 1) changed.add(node);
        else if ((node.nodeValue || "").trim()) changed.add(from);
      } else {
        if (node !== want) from.insertBefore(node, want);
        if (node.nodeType !== 1) {
          if (node.nodeValue !== n.nodeValue) {
            node.nodeValue = n.nodeValue;
            if (node.nodeType === 3) changed.add(from);
          }
        } else if (node.nodeName === "SCRIPT") {
          if (
            /** @type {Element} */
            node.outerHTML !== /** @type {Element} */
            n.outerHTML
          ) throw new Error("script changed");
        } else if (!node.isEqualNode(n)) morph(
          /** @type {Element} */
          node,
          /** @type {Element} */
          n,
          changed,
          keep
        );
      }
      prev = node;
    }
  }

  // core/studio/overlay/update.js
  var ready = (soft) => {
    reportSize();
    post(MSG.READY, { href: location.pathname + location.search, title: document.title, soft: soft === true });
  };
  function swapSheets(files) {
    for (
      const link of
      /** @type {HTMLLinkElement[]} */
      [...document.querySelectorAll('link[rel~="stylesheet"][href]')]
    ) {
      const p = sheetPath(link.getAttribute("href"));
      if (!files.some((f) => p.endsWith(`/${f}`))) continue;
      const next = (
        /** @type {HTMLLinkElement} */
        link.cloneNode()
      );
      next.setAttribute("href", `${p}?v=${Date.now()}`);
      const done = () => {
        link.remove();
        reportSize();
        reportSelection();
        if (st.pins.length) reportPins();
      };
      next.addEventListener("load", done);
      next.addEventListener("error", done);
      link.after(next);
    }
  }
  async function softReload(d) {
    try {
      const res = await fetch(location.href, { cache: "no-store" });
      if (!res.ok) throw new Error(String(res.status));
      const doc = new DOMParser().parseFromString(await res.text(), "text/html");
      if ([...doc.scripts].some((s) => !s.hasAttribute("data-dfw-overlay"))) throw new Error("page script");
      const changed = /* @__PURE__ */ new Set();
      const keep = st.editing ? st.editing.el : null;
      syncAttrs(document.documentElement, doc.documentElement, changed, ROOT_OWNED);
      morph(document.head, doc.head, changed, keep);
      morph(document.body, doc.body, changed, keep);
      swapSheets(d.files || []);
      st.hoverEl = null;
      st.layerEls = [];
      st.layerNode = /* @__PURE__ */ new Map();
      st.tokenCache = null;
      if (st.selected && !st.selected.isConnected) st.selected = null;
      ready(true);
      const els = (
        /** @type {HTMLElement[]} */
        [...changed].filter(
          (el) => el.nodeType === 1 && el.isConnected && el !== document.documentElement && el !== document.body && !document.head.contains(el) && !isUi(el) && el.getClientRects().length
        )
      );
      const tops = els.filter((el) => !els.some((o) => o !== el && o.contains(el)));
      post(MSG.CHANGED, {
        agent: !!d.agent,
        count: tops.length,
        items: tops.slice(0, 4).map((el) => ({ rect: rectOf(el), label: labelOf(pick(el, false) || el) }))
      });
    } catch {
      location.reload();
    }
  }

  // core/studio/overlay/messages.js
  var interactive2 = () => st.mode === MODE.SELECT || st.mode === MODE.COMMENT;
  function find(d) {
    try {
      return d.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(d.dfwId)}"]`) || d.selector && document.querySelector(d.selector) || d.component && document.querySelector(`[data-dfw-component="${CSS.escape(d.component)}"]`) || null;
    } catch {
      return null;
    }
  }
  var ON = {
    [MSG.MODE]: (d) => {
      st.mode = d.mode;
      if (!interactive2()) {
        st.hoverEl = null;
        post(MSG.HOVER, { rect: null });
      }
      document.documentElement.style.cursor = st.mode === MODE.COMMENT ? "crosshair" : "";
      placeHandles();
    },
    [MSG.LAYOUT]: (d) => {
      st.layoutCfg = { enabled: !!d.enabled, scope: d.scope || "" };
      placeHandles();
    },
    [MSG.LAYOUT_KEY]: (d) => layoutKey(d.key, !!d.shift),
    [MSG.LAYOUT_VERIFY]: verifyLayout,
    [MSG.TOKENS]: (d) => {
      st.tokenVars = d.vars || [];
      st.tokenCache = null;
    },
    [MSG.PINS]: (d) => {
      st.pins = d.pins || [];
      reportPins();
    },
    [MSG.LAYERS]: () => post(MSG.LAYERS, { tree: buildLayers(), selected: st.selected ? st.layerNode.get(st.selected)?.node.i ?? null : null }),
    [MSG.LAYER_HOVER]: (d) => {
      const el = st.layerEls[d.i];
      post(MSG.HOVER, el ? { rect: rectOf(el), label: labelOf(el), comp: !!el.dataset.dfwComponent } : { rect: null });
    },
    [MSG.LAYER_SELECT]: (d) => {
      if (st.layerEls[d.i]) select(st.layerEls[d.i], { scroll: true });
    },
    [MSG.SELECT_ID]: (d) => {
      if (!st.layerEls.length) buildLayers();
      const el = (
        /** @type {HTMLElement | null} */
        find({ dfwId: d.dfwId, selector: d.selector })
      );
      if (el) select(el, { scroll: !!d.scroll });
      else if (d.required) post(MSG.TOAST, { message: TEXT.elementNotFound(d.dfwId || d.selector) });
    },
    [MSG.CLEAR_SELECTION]: () => {
      st.selected = null;
      if (st.editing) endEdit(false);
      placeHandles();
    },
    [MSG.SOFT_RELOAD]: softReload,
    [MSG.PRESENCE]: (d) => {
      const el = (
        /** @type {HTMLElement | null} */
        find({ dfwId: d.dfwId, component: d.component })
      );
      post(
        MSG.PRESENCE,
        el && el.getClientRects().length ? { rect: rectOf(el), label: labelOf(el) } : { rect: null, fallback: !!d.fallback }
      );
    },
    [MSG.EDIT]: () => {
      if (st.selected) startEdit(st.selected);
    },
    [MSG.NAV]: (d) => {
      if (!st.selected) return;
      const next = navLayer(d.dir);
      if (next) select(next, { scroll: true });
    }
  };
  function listenStudio() {
    addEventListener("message", (e) => {
      if (e.source !== parent || e.origin !== location.origin) return;
      const d = e.data || {};
      if (Object.hasOwn(ON, d.type)) ON[d.type](d);
    });
  }

  // core/studio/overlay/main.js
  function main() {
    if (window.__dfwOverlay) return;
    window.__dfwOverlay = true;
    const LIVE = Boolean(document.currentScript && document.currentScript.hasAttribute("data-dfw-live"));
    const params = new URLSearchParams(location.search);
    if (!LIVE && params.get("theme")) document.documentElement.dataset.theme = params.get("theme");
    if (!LIVE && params.get("state")) document.documentElement.dataset.state = params.get("state");
    if (window.parent === window && !LIVE) return;
    if (LIVE) {
      window.__dfwTargetInfo = info;
      return;
    }
    listenInput();
    listenStudio();
    if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", ready);
    else ready();
    addEventListener("load", reportSize);
    if (document.fonts)
      document.fonts.ready.then(() => {
        reportSize();
        reportSelection();
        if (st.pins.length) reportPins();
      });
    if (window.ResizeObserver && document.documentElement) {
      let t;
      new ResizeObserver(() => {
        clearTimeout(t);
        t = setTimeout(reportSize, 100);
      }).observe(document.documentElement);
    }
  }
  main();
})();
