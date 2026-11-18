// dfw Studio in-frame agent, injected into every design document served by the
// Studio. It reports hover/selection geometry, layers, design-token usage and
// text edits to the parent Studio via postMessage, and forwards wheel/keys so
// the canvas can pan and zoom over frames. The Studio draws all chrome
// (selection boxes, pins) itself, in screen space. Framework-agnostic.
(() => {
  if (window.__dfwOverlay) return;
  window.__dfwOverlay = true;
  // dfw review (running app, see lib/live.mjs): only share the comment-target helpers with live.js.
  const LIVE = Boolean(document.currentScript && document.currentScript.hasAttribute('data-dfw-live'));
  const params = new URLSearchParams(location.search);
  if (!LIVE && params.get('theme')) document.documentElement.dataset.theme = params.get('theme');
  if (!LIVE && params.get('state')) document.documentElement.dataset.state = params.get('state');
  if (window.parent === window && !LIVE) return; // opened directly: plain page

  const post = (type, data = {}) => parent.postMessage({ type, ...data }, '*');
  let mode = 'select';
  let hoverEl = null;
  let selected = null;
  let editing = null;
  let midPan = false;
  let tokenVars = [];
  let tokenCache = null;
  let layerEls = [];
  let layerNode = new Map();
  let pins = [];

  const LANDMARK_TAGS = new Set(['HEADER', 'NAV', 'MAIN', 'ASIDE', 'FOOTER', 'FORM', 'DIALOG']);
  const LANDMARK_ROLES = new Set(['banner', 'navigation', 'main', 'complementary', 'contentinfo', 'region', 'search', 'form', 'dialog', 'alertdialog', 'toolbar', 'tablist']);
  const isUi = (el) => el && el.closest && el.closest('[data-dfw-ui]');
  const isLayer = (el) =>
    el.hasAttribute('data-dfw-id') || el.hasAttribute('data-dfw-component') || LANDMARK_TAGS.has(el.tagName) || LANDMARK_ROLES.has(el.getAttribute('role')) || (el.tagName === 'SECTION' && (el.hasAttribute('aria-label') || el.hasAttribute('aria-labelledby')));

  const rectOf = (el) => {
    const r = el.getBoundingClientRect();
    return [r.left, r.top, r.width, r.height];
  };

  function pick(el, exact) {
    if (!el || el.nodeType !== 1 || el === document.documentElement || el === document.body || isUi(el)) return null;
    if (exact) return el;
    return el.closest('[data-dfw-id],[data-dfw-component]') || el;
  }

  function labelOf(el) {
    if (el.dataset.dfwComponent) return `${el.dataset.dfwComponent}${el.dataset.dfwVariant ? ` · ${el.dataset.dfwVariant}` : ''}`;
    return el.dataset.dfwId || el.tagName.toLowerCase();
  }

  /* ---------- comment target info (feedback contract) ---------- */

  function cssPath(el) {
    if (el.dataset && el.dataset.dfwId) return `[data-dfw-id="${el.dataset.dfwId}"]`;
    const parts = [];
    let n = el;
    while (n && n.nodeType === 1 && n !== document.body) {
      if (n.id) {
        parts.unshift(`#${CSS.escape(n.id)}`);
        break;
      }
      const anc = n.parentElement && n.parentElement.closest('[data-dfw-id]');
      let sel = n.tagName.toLowerCase();
      const sibs = n.parentElement ? [...n.parentElement.children].filter((c) => c.tagName === n.tagName) : [];
      if (sibs.length > 1) sel += `:nth-of-type(${sibs.indexOf(n) + 1})`;
      parts.unshift(sel);
      if (anc && n.parentElement === anc) {
        parts.unshift(`[data-dfw-id="${anc.dataset.dfwId}"]`);
        break;
      }
      n = n.parentElement;
    }
    return parts.join(' > ');
  }

  const IMPLICIT_ROLE = { BUTTON: 'button', A: 'link', INPUT: 'textbox', SELECT: 'combobox', TEXTAREA: 'textbox', NAV: 'navigation', HEADER: 'banner', FOOTER: 'contentinfo', MAIN: 'main', FORM: 'form', TABLE: 'table', IMG: 'img', H1: 'heading', H2: 'heading', H3: 'heading', H4: 'heading', DIALOG: 'dialog', UL: 'list', OL: 'list', LI: 'listitem', SECTION: 'region', ASIDE: 'complementary' };

  function a11y(el) {
    let role = el.getAttribute('role') || IMPLICIT_ROLE[el.tagName] || null;
    if (el.tagName === 'INPUT') {
      const t = (el.getAttribute('type') || 'text').toLowerCase();
      role = { checkbox: 'checkbox', radio: 'radio', search: 'searchbox', button: 'button', submit: 'button', range: 'slider' }[t] || role;
    }
    let name = el.getAttribute('aria-label') || el.getAttribute('alt') || el.getAttribute('title') || '';
    if (!name && el.getAttribute('aria-labelledby')) name = el.getAttribute('aria-labelledby').split(/\s+/).map((id) => document.getElementById(id)?.textContent || '').join(' ');
    if (!name && el.id) name = document.querySelector(`label[for="${CSS.escape(el.id)}"]`)?.textContent || '';
    if (!name && el.getAttribute('placeholder')) name = el.getAttribute('placeholder');
    if (!name) name = el.textContent || '';
    return { role, name: name.replace(/\s+/g, ' ').trim().slice(0, 120) };
  }

  function info(el) {
    const r = el.getBoundingClientRect();
    const cs = getComputedStyle(el);
    return {
      dfwId: el.dataset?.dfwId || el.closest('[data-dfw-id]')?.dataset.dfwId || null,
      exact: !el.dataset?.dfwId,
      selector: cssPath(el),
      tag: el.tagName.toLowerCase(),
      component: el.dataset.dfwComponent || null,
      bbox: [Math.round(r.left + scrollX), Math.round(r.top + scrollY), Math.round(r.width), Math.round(r.height)],
      viewportBbox: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
      a11y: a11y(el),
      text: (el.innerText || el.value || '').replace(/\s+/g, ' ').trim().slice(0, 160),
      styles: { color: cs.color, background: cs.backgroundColor, font: `${cs.fontWeight} ${cs.fontSize}/${cs.lineHeight} ${cs.fontFamily.split(',')[0]}`, padding: cs.padding, radius: cs.borderRadius },
      scroll: [Math.round(scrollX), Math.round(scrollY)],
    };
  }

  if (LIVE) {
    window.__dfwTargetInfo = info;
    return;
  }

  /* ---------- design tokens: reverse-map computed values to token names ---------- */

  const BASE = { color: 'rgb(1, 2, 3)', paddingLeft: '0px', fontWeight: '100', boxShadow: 'none', fontSize: '3.3px' };

  function buildTokenCache() {
    const box = document.createElement('div');
    box.setAttribute('data-dfw-ui', '');
    box.style.cssText = `position:absolute;left:-99999px;top:0;visibility:hidden;pointer-events:none;color:${BASE.color};font-size:${BASE.fontSize};font-weight:${BASE.fontWeight};`;
    const probes = tokenVars.map((t) => {
      const d = document.createElement('div');
      const v = `var(${t.var})`;
      d.style.cssText = `color:${v};padding-left:${v};font-weight:${v};box-shadow:${v};font-size:${v};`;
      box.append(d);
      return d;
    });
    (document.body || document.documentElement).append(box);
    const maps = { color: new Map(), length: new Map(), fontWeight: new Map(), shadow: new Map(), fontSize: new Map() };
    const add = (m, k, name) => (m.get(k) || m.set(k, []).get(k)).push(name);
    probes.forEach((d, i) => {
      const cs = getComputedStyle(d);
      const name = tokenVars[i].name;
      if (cs.color !== BASE.color) add(maps.color, cs.color, name);
      if (cs.paddingLeft !== BASE.paddingLeft) add(maps.length, cs.paddingLeft, name);
      if (cs.fontWeight !== BASE.fontWeight) add(maps.fontWeight, cs.fontWeight, name);
      if (cs.boxShadow !== BASE.boxShadow) add(maps.shadow, cs.boxShadow, name);
      if (cs.fontSize !== BASE.fontSize) add(maps.fontSize, cs.fontSize, name);
    });
    box.remove();
    return { key: `${document.documentElement.dataset.theme || ''}`, maps };
  }

  function lineHeightMap(fontSize) {
    const m = new Map();
    const box = document.createElement('div');
    box.setAttribute('data-dfw-ui', '');
    box.style.cssText = `position:absolute;left:-99999px;top:0;visibility:hidden;font-size:${fontSize};line-height:1.111;`;
    const base = document.createElement('div');
    box.append(base);
    const cand = tokenVars.filter((t) => /lead|line/i.test(t.name));
    const probes = cand.map((t) => {
      const d = document.createElement('div');
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

  const semanticFirst = (names) => [...names].sort((a, b) => /\.\d/.test(a) - /\.\d/.test(b) || a.length - b.length);
  const FAMILY = {
    padding: /^space\b|spacing/i,
    gap: /^space\b|spacing/i,
    margin: /^space\b|spacing/i,
    'border-radius': /radius|rounded/i,
    'font-size': /font|text|size/i,
    'font-weight': /weight|font/i,
    'line-height': /lead|line/i,
    'box-shadow': /shadow|elevation/i,
  };

  function match(group, value, map) {
    const names = map.get(value) || [];
    const fam = FAMILY[group];
    const ok = fam ? names.filter((n) => fam.test(n)) : names;
    return semanticFirst(ok);
  }

  function inspect(el) {
    if (!tokenCache || tokenCache.key !== `${document.documentElement.dataset.theme || ''}`) tokenCache = buildTokenCache();
    const M = tokenCache.maps;
    const cs = getComputedStyle(el);
    const rows = [];
    const row = (group, prop, value, map) => rows.push({ group, prop, value, tokens: match(group, value, map) });
    const transparent = (c) => c === 'rgba(0, 0, 0, 0)' || c === 'transparent';
    row('color', 'color', cs.color, M.color);
    if (!transparent(cs.backgroundColor)) row('background-color', 'background-color', cs.backgroundColor, M.color);
    if (parseFloat(cs.borderTopWidth) > 0 && cs.borderTopStyle !== 'none' && !transparent(cs.borderTopColor)) row('border-color', 'border-color', cs.borderTopColor, M.color);
    row('font-size', 'font-size', cs.fontSize, M.fontSize);
    row('font-weight', 'font-weight', cs.fontWeight, M.fontWeight);
    if (cs.lineHeight !== 'normal') row('line-height', 'line-height', cs.lineHeight, lineHeightMap(cs.fontSize));
    const sides = (group, names) => {
      const v = names.map((n) => cs.getPropertyValue(n));
      if (v.every((x) => x === '0px' || x === 'auto')) return;
      if (v.every((x) => x === v[0])) return row(group, group, v[0], M.length);
      if (v[0] === v[2] && v[1] === v[3] && group !== 'border-radius') {
        if (v[0] !== '0px') row(group, `${group}-block`, v[0], M.length);
        if (v[1] !== '0px') row(group, `${group}-inline`, v[1], M.length);
        return;
      }
      names.forEach((n, i) => v[i] !== '0px' && v[i] !== 'auto' && row(group, n, v[i], M.length));
    };
    sides('padding', ['padding-top', 'padding-right', 'padding-bottom', 'padding-left']);
    if (/flex|grid/.test(cs.display)) {
      const rg = cs.rowGap;
      const cg = cs.columnGap;
      if (rg === cg) {
        if (rg !== 'normal' && rg !== '0px') row('gap', 'gap', rg, M.length);
      } else {
        if (rg !== 'normal' && rg !== '0px') row('gap', 'row-gap', rg, M.length);
        if (cg !== 'normal' && cg !== '0px') row('gap', 'column-gap', cg, M.length);
      }
    }
    sides('margin', ['margin-top', 'margin-right', 'margin-bottom', 'margin-left']);
    sides('border-radius', ['border-top-left-radius', 'border-top-right-radius', 'border-bottom-right-radius', 'border-bottom-left-radius']);
    if (cs.boxShadow !== 'none') row('box-shadow', 'box-shadow', cs.boxShadow, M.shadow);
    return rows;
  }

  /* ---------- layers ---------- */

  function buildLayers() {
    layerEls = [];
    layerNode = new Map();
    const walk = (el, out, parentNode) => {
      for (const c of el.children) {
        if (c.hasAttribute('data-dfw-ui') || /^(SCRIPT|STYLE|TEMPLATE|LINK|META|NOSCRIPT)$/.test(c.tagName)) continue;
        if (layerEls.length > 3000) return;
        if (isLayer(c)) {
          const node = {
            i: layerEls.length,
            id: c.dataset.dfwId || null,
            comp: c.dataset.dfwComponent || null,
            variant: c.dataset.dfwVariant || null,
            size: c.dataset.dfwSize || null,
            tag: c.tagName.toLowerCase(),
            name: c.getAttribute('aria-label') || null,
            hidden: c.getClientRects().length === 0,
            children: [],
          };
          layerEls.push(c);
          layerNode.set(c, { node, parent: parentNode });
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
    for (let n = el.parentElement; n && n !== document.body; n = n.parentElement) if (layerNode.has(n)) return n;
    return null;
  }

  /* ---------- text editing ---------- */

  const norm = (s) => (s || '').replace(/\s+/g, ' ').trim();
  function editTarget(el) {
    if (!el || el.children.length) return { error: 'Only text-only elements can be edited in place.' };
    if (/^(INPUT|TEXTAREA|SELECT|IMG|SVG|svg)$/.test(el.tagName) || !norm(el.textContent)) return { error: 'This element has no text to edit.' };
    if (el.dataset.dfwId) return { el, id: el.dataset.dfwId };
    const host = el.closest('[data-dfw-id]');
    if (host && host.dataset.dfwComponent && norm(host.textContent) === norm(el.textContent)) return { el, id: host.dataset.dfwId };
    return { error: 'Give this element a data-dfw-id to edit it in place, or leave a comment.' };
  }

  function startEdit(el) {
    const t = editTarget(el);
    if (t.error) return post('dfw:toast', { message: t.error });
    editing = { el: t.el, id: t.id, orig: t.el.textContent };
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

  function endEdit(save) {
    if (!editing) return;
    const { el, id, orig } = editing;
    editing = null;
    el.removeEventListener('blur', onEditBlur);
    el.removeAttribute('contenteditable');
    const text = el.textContent;
    if (!save) el.textContent = orig;
    getSelection()?.removeAllRanges();
    post('dfw:editing', { on: false, id });
    if (save && norm(text) !== norm(orig)) post('dfw:edit-text', { id, text: norm(text) });
    reportSelection();
  }

  /* ---------- selection & reporting ---------- */

  function selPayload(el, withInspect) {
    const t = editTarget(el);
    return {
      target: info(el),
      rect: rectOf(el),
      label: labelOf(el),
      layer: layerNode.get(el)?.node.i ?? null,
      own: { id: el.dataset.dfwId || null, comp: el.dataset.dfwComponent || null, variant: el.dataset.dfwVariant || null, size: el.dataset.dfwSize || null },
      textEditable: !t.error,
      inspect: withInspect ? inspect(el) : null,
    };
  }

  function select(el, { inspectIt = true, scroll = false } = {}) {
    selected = el;
    placeHandles();
    if (!el) return post('dfw:select', { target: null });
    if (scroll) el.scrollIntoView({ block: 'nearest' });
    post('dfw:select', { comment: mode === 'comment', ...selPayload(el, inspectIt && mode !== 'comment') });
  }

  function reportSelection() {
    if (selected && selected.isConnected) post('dfw:sel-rect', { rect: rectOf(selected) });
    placeHandles();
  }

  function reportPins() {
    const out = [];
    for (const p of pins) {
      let el = null;
      try {
        el = (p.selector && document.querySelector(p.selector)) || (p.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(p.dfwId)}"]`)) || null;
      } catch {}
      if (el && el.getClientRects().length) out.push({ id: p.id, rect: rectOf(el) });
    }
    post('dfw:pin-rects', { pins: out });
  }

  function reportSize() {
    const d = document.documentElement;
    post('dfw:size', { h: Math.max(d.scrollHeight, document.body ? document.body.scrollHeight : 0), title: document.title });
  }

  /* ---------- layout: drag to reorder, resize with snapping (edit mode) ----------
   * Constrained on purpose: no free positioning. Dragging reorders an element among the
   * data-dfw-id children of a flex/grid container; resizing snaps to the grid (column span)
   * or to tokens / fixed percentages. The Studio writes the change to the screen source. */

  let layoutCfg = { enabled: false, scope: '' };
  let drag = null; // { el, x, y, active, drop, prevOpacity, prevPointer }
  let resize = null; // { el, axis, rect, x, y, moved, pick, prop, prev }
  let suppressClick = false;
  let lengthCache = null;
  const FLOW = /^(inline-)?(flex|grid)$/;
  const isFlow = (el) => !!el && el.nodeType === 1 && FLOW.test(getComputedStyle(el).display);
  const isGrid = (el) => /grid$/.test(getComputedStyle(el).display);
  const idKids = (c) => [...c.children].filter((x) => x.dataset && x.dataset.dfwId && !x.hasAttribute('data-dfw-ui') && x.getClientRects().length);
  const canLayout = (el) => layoutCfg.enabled && mode === 'select' && !editing && el && el.isConnected && !!el.dataset.dfwId && el !== document.body;
  const canMove = (el) => canLayout(el) && isFlow(el.parentElement);
  const PERCENTS = [25, 33.333, 50, 66.667, 75, 100];

  const ui = document.createElement('div');
  ui.setAttribute('data-dfw-ui', '');
  ui.style.cssText = 'position:fixed;inset:0;pointer-events:none;z-index:2147483646;';
  const mk = (css) => {
    const d = document.createElement('div');
    d.setAttribute('data-dfw-ui', '');
    d.style.cssText = `position:fixed;display:none;box-sizing:border-box;${css}`;
    ui.append(d);
    return d;
  };
  const HANDLE = 'width:10px;height:10px;background:#fff;border:2px solid #0d99ff;border-radius:2px;pointer-events:auto;';
  const hX = mk(`${HANDLE}cursor:ew-resize;`);
  const hY = mk(`${HANDLE}cursor:ns-resize;`);
  hX.dataset.dfwHandle = 'x';
  hY.dataset.dfwHandle = 'y';
  hX.title = 'Drag to resize the width (snaps to grid columns, size tokens or percentages). Alt+Shift+Left/Right. Double-click: back to auto';
  hY.title = 'Drag to set a min-height (snaps to spacing/size tokens). Alt+Shift+Up/Down. Double-click: back to auto';
  const line = mk('background:#0d99ff;border-radius:2px;');
  const zone = mk('border:2px dashed #0d99ff;background:rgba(13,153,255,.12);border-radius:4px;'); // drop-into-container highlight
  const badge = mk('padding:2px 6px;border-radius:4px;background:#0d99ff;color:#fff;font:600 11px/16px system-ui,sans-serif;white-space:nowrap;');
  const mountUi = () => !ui.isConnected && (document.body || document.documentElement).append(ui);

  function placeHandles() {
    const show = !drag?.active && !resize?.moved && selected && canLayout(selected) && selected.getClientRects().length;
    hX.style.display = hY.style.display = show ? 'block' : 'none';
    if (!show) return;
    mountUi();
    const r = selected.getBoundingClientRect();
    hX.style.left = `${r.right - 5}px`;
    hX.style.top = `${r.top + r.height / 2 - 5}px`;
    hY.style.left = `${r.left + r.width / 2 - 5}px`;
    hY.style.top = `${r.bottom - 5}px`;
  }

  function showBadge(text, x, y) {
    mountUi();
    badge.textContent = text;
    badge.style.display = 'block';
    badge.style.left = `${Math.min(x + 12, innerWidth - badge.offsetWidth - 4)}px`;
    badge.style.top = `${Math.max(4, y - 28)}px`;
  }
  const hideFeedback = () => {
    line.style.display = 'none';
    zone.style.display = 'none';
    badge.style.display = 'none';
  };
  const scoped = (t) => (layoutCfg.scope ? `${t} · ${layoutCfg.scope}` : t);

  /** px value of every space.* / size.* dimension token (resolved in this document). */
  function tokenLengths() {
    const key = document.documentElement.dataset.theme || '';
    if (lengthCache && lengthCache.key === key && lengthCache.n === tokenVars.length) return lengthCache.list;
    const box = document.createElement('div');
    box.setAttribute('data-dfw-ui', '');
    box.style.cssText = 'position:absolute;left:-99999px;top:0;visibility:hidden;';
    const cand = tokenVars.filter((t) => /^(space|size)\./.test(t.name) && t.type !== 'typography');
    const probes = cand.map((t) => {
      const d = document.createElement('div');
      d.style.cssText = `width:var(${t.var});height:0;`;
      box.append(d);
      return d;
    });
    (document.body || document.documentElement).append(box);
    const list = probes.map((d, i) => ({ name: cand[i].name, var: cand[i].var, px: d.getBoundingClientRect().width })).filter((t) => t.px > 0);
    box.remove();
    list.sort((a, b) => a.px - b.px || /\.\d/.test(a.name) - /\.\d/.test(b.name));
    lengthCache = { key, n: tokenVars.length, list };
    return list;
  }

  /** Snap options for a resize: [{ value (sent to the server), css (preview), label, px }] sorted by px. */
  function snapOptions(el, axis) {
    const p = el.parentElement;
    if (axis === 'x' && isGrid(p)) {
      const cs = getComputedStyle(p);
      const tracks = cs.gridTemplateColumns.split(/\s+/).filter((t) => /px$/.test(t)).map(parseFloat);
      const gap = parseFloat(cs.columnGap) || 0;
      const n = Math.max(1, tracks.length);
      const track = tracks.length ? tracks.reduce((a, b) => a + b, 0) / tracks.length : p.clientWidth;
      return { prop: 'grid-column', list: Array.from({ length: n }, (_, i) => ({ value: `span ${i + 1}`, css: `span ${i + 1}`, label: `span ${i + 1} / ${n}`, px: (i + 1) * track + i * gap })) };
    }
    const toks = tokenLengths();
    if (axis === 'x') {
      const pcs = getComputedStyle(p);
      const inner = p.clientWidth - parseFloat(pcs.paddingLeft) - parseFloat(pcs.paddingRight);
      const pct = PERCENTS.map((v) => ({ value: `${v}%`, css: `${v}%`, label: `${Math.round(v)}%`, px: (inner * v) / 100 }));
      const sizes = toks.filter((t) => t.name.startsWith('size.') && t.px <= inner).map((t) => ({ value: t.name, css: `var(${t.var})`, label: `${t.name} · ${Math.round(t.px)}px`, px: t.px }));
      return { prop: 'width', list: [...pct, ...sizes].sort((a, b) => a.px - b.px) };
    }
    return { prop: 'min-height', list: toks.map((t) => ({ value: t.name, css: `var(${t.var})`, label: `${t.name} · ${Math.round(t.px)}px`, px: t.px })) };
  }

  const nearest = (list, px) => list.reduce((best, o) => (Math.abs(o.px - px) < Math.abs(best.px - px) ? o : best), list[0]);
  const styleProp = { 'grid-column': 'gridColumn', width: 'width', 'min-height': 'minHeight' };

  function startResize(axis, e) {
    const el = selected;
    const opts = snapOptions(el, axis);
    if (!opts.list.length) return post('dfw:toast', { message: 'No size tokens to snap to.' });
    resize = { el, axis, rect: el.getBoundingClientRect(), x: e.clientX, y: e.clientY, moved: false, ...opts, pick: null, prev: el.style[styleProp[opts.prop]] };
    placeHandles();
  }

  function moveResize(e) {
    const r = resize;
    // A click (or the double-click that resets) on a handle is not a resize.
    if (!r.moved && Math.hypot(e.clientX - r.x, e.clientY - r.y) < 3) return;
    if (!r.moved) {
      r.moved = true;
      placeHandles();
    }
    const px = r.axis === 'x' ? e.clientX - r.rect.left : e.clientY - r.rect.top;
    const o = nearest(r.list, px);
    if (o !== r.pick) {
      r.pick = o;
      r.el.style[styleProp[r.prop]] = o.css;
      reportSelection();
    }
    showBadge(scoped(r.prop === 'grid-column' ? o.label : `${r.prop} ${o.label}`), e.clientX, e.clientY);
  }

  function endResize(commit) {
    const r = resize;
    resize = null;
    hideFeedback();
    if (commit && r.moved && r.pick) post('dfw:layout-op', { op: 'layout', id: r.el.dataset.dfwId, prop: r.prop, value: r.pick.value, label: r.pick.label });
    else r.el.style[styleProp[r.prop]] = r.prev;
    placeHandles();
    reportSelection();
  }

  /** An empty flex/grid container with a data-dfw-id: nothing in it but whitespace (and dfw UI). */
  const isEmptyBox = (n) => n.dataset && n.dataset.dfwId && isFlow(n) && ![...n.children].some((k) => !k.hasAttribute('data-dfw-ui')) && !n.textContent.trim();

  /**
   * Where a dragged element would land: { ref, position, vertical } next to a data-dfw-id child of a flex/grid
   * container, or { ref: container, position: 'inside' } over an empty flex/grid container that has a data-dfw-id.
   */
  function dropAt(x, y) {
    const d = drag.el;
    let n = document.elementFromPoint(x, y);
    let c = null;
    const home = d.parentElement;
    for (let e = n; e && e !== home && e !== document.documentElement; e = e.parentElement) {
      if (isUi(e) || d.contains(e)) break;
      if (isEmptyBox(e)) return { ref: e, position: 'inside', r: e.getBoundingClientRect() };
      if (isFlow(e) && idKids(e).length) break;
    }
    const hr = home.getBoundingClientRect();
    if (x >= hr.left && x <= hr.right && y >= hr.top && y <= hr.bottom) c = home;
    for (; !c && n && n !== document.documentElement; n = n.parentElement) {
      if (isUi(n) || d.contains(n)) return null;
      if (isFlow(n) && idKids(n).some((k) => k !== d)) c = n;
    }
    if (!c) return null;
    const kids = idKids(c).filter((k) => k !== d);
    if (!kids.length) return null;
    const dist = (r) => Math.hypot(Math.max(r.left - x, 0, x - r.right), Math.max(r.top - y, 0, y - r.bottom));
    let best = null;
    for (const k of kids) {
      const r = k.getBoundingClientRect();
      const dd = dist(r);
      if (!best || dd < best.dd) best = { k, r, dd };
    }
    const { k, r } = best;
    let horizontal; // true: decide by x (items flow left-right)
    if (isGrid(c)) horizontal = y >= r.top && y <= r.bottom;
    else horizontal = getComputedStyle(c).flexDirection.startsWith('row');
    const before = horizontal ? x < r.left + r.width / 2 : y < r.top + r.height / 2;
    return { ref: k, position: before ? 'before' : 'after', vertical: horizontal, r };
  }

  function moveDrag(e) {
    if (!drag.active) {
      if (Math.hypot(e.clientX - drag.x, e.clientY - drag.y) < 5) return;
      drag.active = true;
      drag.prevOpacity = drag.el.style.opacity;
      drag.prevPointer = drag.el.style.pointerEvents;
      drag.el.style.opacity = '0.4';
      drag.el.style.pointerEvents = 'none';
      placeHandles();
      post('dfw:hover', { rect: null });
    }
    const t = (drag.drop = dropAt(e.clientX, e.clientY));
    mountUi();
    if (!t) {
      line.style.display = zone.style.display = 'none';
      return showBadge('Drop next to a sibling in a flex or grid container, or into an empty one', e.clientX, e.clientY);
    }
    const { r } = t;
    if (t.position === 'inside') {
      line.style.display = 'none';
      Object.assign(zone.style, { left: `${r.left}px`, top: `${r.top}px`, width: `${r.width}px`, height: `${Math.max(r.height, 8)}px`, display: 'block' });
      return showBadge(`Into ${t.ref.dataset.dfwId}`, e.clientX, e.clientY);
    }
    zone.style.display = 'none';
    const edge = t.position === 'before' ? (t.vertical ? r.left : r.top) : t.vertical ? r.right : r.bottom;
    Object.assign(line.style, t.vertical ? { left: `${edge - 2}px`, top: `${r.top}px`, width: '3px', height: `${r.height}px` } : { left: `${r.left}px`, top: `${edge - 2}px`, width: `${r.width}px`, height: '3px' });
    line.style.display = 'block';
    showBadge(`${t.position === 'before' ? 'Before' : 'After'} ${t.ref.dataset.dfwId}`, e.clientX, e.clientY);
  }

  function endDrag(commit) {
    const g = drag;
    drag = null;
    hideFeedback();
    if (!g?.active) return placeHandles();
    g.el.style.opacity = g.prevOpacity;
    g.el.style.pointerEvents = g.prevPointer;
    suppressClick = true; // the click that ends the drag must not re-select (reset on the next mousedown)
    const t = g.drop;
    if (commit && t && t.position === 'inside') {
      t.ref.append(g.el); // optimistic; the frame reloads from the source
      post('dfw:layout-op', { op: 'move', id: g.el.dataset.dfwId, ref: t.ref.dataset.dfwId, position: 'inside', label: `Moved into ${t.ref.dataset.dfwId}` });
    } else if (commit && t) {
      const next = t.position === 'before' ? t.ref : t.ref.nextSibling;
      if (next !== g.el && !(t.position === 'before' && g.el.nextElementSibling === t.ref) && !(t.position === 'after' && t.ref.nextElementSibling === g.el)) {
        t.ref.parentElement.insertBefore(g.el, next); // optimistic; the frame reloads from the source
        post('dfw:layout-op', { op: 'move', id: g.el.dataset.dfwId, ref: t.ref.dataset.dfwId, position: t.position, label: `Moved ${t.position} ${t.ref.dataset.dfwId}` });
      }
    }
    placeHandles();
    reportSelection();
  }

  /** Reset to auto: drop the element's layout rule (prop null: the whole rule) at this frame's breakpoint scope. */
  function resetLayout(el, prop = null) {
    for (const p of prop ? [prop] : Object.keys(styleProp)) el.style[styleProp[p]] = '';
    reportSelection();
    placeHandles();
    post('dfw:layout-op', { op: 'layout', id: el.dataset.dfwId, prop, value: null, label: prop ? `${prop} reset to auto` : 'Size reset to auto' });
  }

  /** Style rules of a sheet (recursing into @media that matches now, @layer, @supports) for which pick(rule) is truthy. */
  function rulesOf(rules, pick, out = []) {
    for (const r of rules) {
      if (r.cssRules && !r.selectorText) {
        if (!r.media || matchMedia(r.media.mediaText).matches) rulesOf(r.cssRules, pick, out);
      } else if (r.selectorText && pick(r)) out.push(r);
    }
    return out;
  }

  /**
   * After a resize was written and the frame reloaded: check the element's computed value really changed.
   * If a screen rule still wins (an id selector, inline style or !important), name it instead of failing silently.
   */
  function verifyLayout(d) {
    const el = document.querySelector(`[data-dfw-id="${CSS.escape(d.id)}"]`);
    const sheet = document.querySelector('style[data-dfw-layout]')?.sheet;
    if (!el || !sheet || !styleProp[d.prop]) return;
    const own = rulesOf(sheet.cssRules, (r) => r.style.getPropertyValue(d.prop) && el.matches(r.selectorText));
    if (!own.length) return; // not at this width (or already reset)
    const read = () => (d.prop === 'grid-column' ? ['gridColumnStart', 'gridColumnEnd'] : [styleProp[d.prop]]).map((k) => getComputedStyle(el)[k]).join(' / ');
    const prev = el.style.getPropertyValue(d.prop);
    const prevPrio = el.style.getPropertyPriority(d.prop);
    const now = read();
    el.style.setProperty(d.prop, own.at(-1).style.getPropertyValue(d.prop), 'important');
    const ideal = read();
    el.style.setProperty(d.prop, prev, prevPrio);
    if (now === ideal) return;
    const by = conflictFor(el, d.prop, sheet);
    post('dfw:layout-conflict', { id: d.id, prop: d.prop, selector: by });
    post('dfw:toast', {
      message: `Saved, but ${d.id} did not change: ${by ? `"${by}"` : 'another rule'} still sets ${d.prop}${by === 'style attribute' ? '' : ' with higher priority'}. Remove ${d.prop} there (or ask Claude), then resize again.`,
    });
  }

  const LONGHANDS = { 'grid-column': ['grid-column', 'grid-column-start', 'grid-column-end', 'grid-area'], width: ['width'], 'min-height': ['min-height'] };
  /** Rough selector specificity [ids, classes/attributes/pseudo-classes, types], only to rank conflicting rules. */
  function specificity(sel) {
    const s = sel.replace(/:where\([^)]*\)/g, '').replace(/:(?:not|is|has)\(/g, '(').replace(/"[^"]*"|'[^']*'/g, '');
    return [(s.match(/#[\w-]/g) || []).length, (s.match(/\.[\w-]|\[|:(?!:)[\w-]/g) || []).length, (s.replace(/[#.:][\w-]+|\[[^\]]*\]/g, ' ').match(/(^|[\s>+~(,])[a-z][\w-]*/gi) || []).length];
  }
  /** Selector of the rule that beats the dfw layout rule for prop on el (inline style, then !important, then specificity). */
  function conflictFor(el, prop, dfwSheet) {
    const names = LONGHANDS[prop];
    if (names.some((n) => el.style.getPropertyValue(n))) return 'style attribute';
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
      } catch {} // cross-origin sheet
      for (const r of rules) {
        const sel = r.selectorText.split(',').map((x) => x.trim()).find(matches);
        const n = names.find((x) => r.style.getPropertyValue(x));
        if (sel) hits.push({ sel, imp: r.style.getPropertyPriority(n) === 'important', sp: specificity(sel) });
      }
    }
    hits.sort((a, b) => b.imp - a.imp || b.sp[0] - a.sp[0] || b.sp[1] - a.sp[1] || b.sp[2] - a.sp[2]);
    return hits[0]?.sel || null;
  }

  /** Alt+Arrow: move among siblings; Alt+Shift+Arrow: step the span / width (Left/Right) or min-height (Up/Down); Alt+Shift+0: reset. */
  function layoutKey(key, shift) {
    const el = selected;
    if (!canLayout(el)) return post('dfw:toast', { message: 'Select an element with a data-dfw-id to move or resize it.' });
    if (key === 'reset') return resetLayout(el);
    const back = key === 'ArrowLeft' || key === 'ArrowUp';
    if (!shift) {
      if (!canMove(el)) return post('dfw:toast', { message: 'Only children of a flex or grid container can be reordered.' });
      const sibs = idKids(el.parentElement);
      const i = sibs.indexOf(el);
      const ref = sibs[back ? i - 1 : i + 1];
      if (!ref) return post('dfw:toast', { message: back ? 'Already the first item.' : 'Already the last item.' });
      const position = back ? 'before' : 'after';
      ref.parentElement.insertBefore(el, back ? ref : ref.nextSibling);
      reportSelection();
      return post('dfw:layout-op', { op: 'move', id: el.dataset.dfwId, ref: ref.dataset.dfwId, position, label: `Moved ${position} ${ref.dataset.dfwId}` });
    }
    const axis = key === 'ArrowLeft' || key === 'ArrowRight' ? 'x' : 'y';
    const { prop, list } = snapOptions(el, axis);
    if (!list.length) return post('dfw:toast', { message: 'No size tokens to snap to.' });
    let cur;
    if (prop === 'grid-column') {
      const m = /span\s+(\d+)/.exec(getComputedStyle(el).gridColumnEnd) || /span\s+(\d+)/.exec(getComputedStyle(el).gridColumnStart);
      cur = list.findIndex((o) => o.value === `span ${m ? m[1] : 1}`);
    } else {
      const now = axis === 'x' ? el.getBoundingClientRect().width : parseFloat(getComputedStyle(el).minHeight) || 0;
      cur = list.indexOf(nearest(list, now));
      if (axis === 'y' && !now) cur = -1;
    }
    const o = list[Math.max(0, Math.min(list.length - 1, cur + (back ? -1 : 1)))];
    if (cur >= 0 && o === list[cur]) return post('dfw:toast', { message: `Already ${o.label}.` });
    el.style[styleProp[prop]] = o.css;
    reportSelection();
    post('dfw:layout-op', { op: 'layout', id: el.dataset.dfwId, prop, value: o.value, label: o.label });
  }

  /* ---------- events ---------- */

  const capture = { capture: true };
  const interactive = () => mode === 'select' || mode === 'comment';
  const inEdit = (t) => editing && editing.el.contains(t);

  let raf = 0;
  document.addEventListener(
    'mousemove',
    (e) => {
      if (midPan) return post('dfw:pan', { phase: 'move', sx: e.screenX, sy: e.screenY });
      if (resize || drag) {
        if (!(e.buttons & 1)) return resize ? endResize(false) : endDrag(false); // released outside the frame
        return resize ? moveResize(e) : moveDrag(e);
      }
      if (!interactive() || editing) return;
      const t = pick(e.target, e.metaKey || e.ctrlKey || e.altKey);
      if (t === hoverEl) return;
      hoverEl = t;
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => post('dfw:hover', t ? { rect: rectOf(t), label: labelOf(t), comp: !!t.dataset.dfwComponent } : { rect: null }));
    },
    capture,
  );
  document.addEventListener('mouseleave', () => {
    hoverEl = null;
    post('dfw:hover', { rect: null });
  });
  document.addEventListener(
    'mousedown',
    (e) => {
      if (e.button === 1 && mode !== 'present') {
        e.preventDefault();
        midPan = true;
        return post('dfw:pan', { phase: 'start', sx: e.screenX, sy: e.screenY });
      }
      suppressClick = false;
      if (inEdit(e.target)) return;
      if (editing) endEdit(true);
      if (interactive() && !isUi(e.target)) {
        e.preventDefault();
        e.stopPropagation();
      }
      post('dfw:focus');
      if (e.button !== 0 || mode !== 'select') return;
      if (e.target.dataset && e.target.dataset.dfwHandle && canLayout(selected)) {
        e.preventDefault();
        e.stopPropagation();
        return startResize(e.target.dataset.dfwHandle, e);
      }
      if (selected && selected.contains(e.target) && canMove(selected) && !(e.metaKey || e.ctrlKey || e.altKey)) drag = { el: selected, x: e.clientX, y: e.clientY, active: false };
    },
    capture,
  );
  document.addEventListener(
    'mouseup',
    (e) => {
      if (midPan) {
        midPan = false;
        e.preventDefault();
        post('dfw:pan', { phase: 'end' });
      }
      if (resize) endResize(true);
      else if (drag) endDrag(true);
    },
    capture,
  );
  document.addEventListener(
    'auxclick',
    (e) => e.button === 1 && e.preventDefault(),
    capture,
  );
  document.addEventListener(
    'click',
    (e) => {
      if (suppressClick) {
        suppressClick = false;
        e.preventDefault();
        e.stopPropagation();
        return;
      }
      if (inEdit(e.target) || isUi(e.target)) return;
      if (!interactive()) return prototypeClick(e);
      e.preventDefault();
      e.stopPropagation();
      const t = pick(e.target, e.metaKey || e.ctrlKey || e.altKey);
      if (!layerEls.length) buildLayers();
      select(t);
    },
    capture,
  );
  document.addEventListener(
    'dblclick',
    (e) => {
      if (mode !== 'select' || inEdit(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      const t = e.target;
      if (t.dataset && t.dataset.dfwHandle && canLayout(selected)) return resetLayout(selected, snapOptions(selected, t.dataset.dfwHandle).prop);
      if (t.nodeType === 1 && !t.children.length && norm(t.textContent)) {
        select(t, { inspectIt: true });
        startEdit(t);
      } else if (t.nodeType === 1) select(t);
    },
    capture,
  );
  // Prototype forms have no backend: never submit (and never navigate the frame away).
  document.addEventListener('submit', (e) => e.cancelable && e.preventDefault(), capture);
  addEventListener(
    'wheel',
    (e) => {
      if (mode === 'present') return; // the presented page scrolls normally
      e.preventDefault();
      post('dfw:wheel', { dx: e.deltaX, dy: e.deltaY, dm: e.deltaMode, ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey, x: e.clientX, y: e.clientY });
    },
    { passive: false, capture: true },
  );
  const fwd = (e, type) => post('dfw:key', { phase: type, key: e.key, code: e.code, meta: e.metaKey, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey });
  document.addEventListener(
    'keydown',
    (e) => {
      if ((resize || drag) && e.key === 'Escape') {
        e.preventDefault();
        return resize ? endResize(false) : endDrag(false);
      }
      if (editing) {
        if (e.key === 'Enter' && !e.shiftKey) {
          e.preventDefault();
          endEdit(true);
        } else if (e.key === 'Escape') {
          e.preventDefault();
          endEdit(false);
        }
        return;
      }
      const typing = e.target.closest && e.target.closest('input, textarea, select, [contenteditable]');
      if (mode === 'present' || mode === 'interact') {
        // The page is live: keep its keyboard behaviour; only forward what the Studio needs.
        if (typing) return e.key === 'Escape' && fwd(e, 'down');
        if (mode === 'present' && (e.key === 'ArrowLeft' || e.key === 'ArrowRight') && !e.metaKey && !e.ctrlKey) e.preventDefault();
        if (mode === 'interact' && e.key === ' ') e.preventDefault();
        return fwd(e, 'down');
      }
      if (typing && !interactive()) return;
      if ([' ', 'ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'Tab'].includes(e.key) || e.metaKey || e.ctrlKey) {
        if (!(e.metaKey && ['c', 'v', 'x', 'a', 'r', 'q', 'w', 't'].includes(e.key.toLowerCase()))) e.preventDefault();
      }
      fwd(e, 'down');
    },
    capture,
  );
  document.addEventListener('keyup', (e) => !editing && mode !== 'present' && e.key === ' ' && fwd(e, 'up'), capture);

  /* ---------- prototype navigation (interact / present) ---------- */

  function prototypeClick(e) {
    const goto = e.target.closest && e.target.closest('[data-dfw-goto]');
    if (goto) {
      e.preventDefault();
      e.stopPropagation();
      return post('dfw:goto', { goto: goto.getAttribute('data-dfw-goto') });
    }
    const a = e.target.closest && e.target.closest('a[href]');
    if (!a || e.defaultPrevented) return;
    const href = a.getAttribute('href');
    if (!href || href.startsWith('#') || /^(javascript|mailto|tel):/i.test(href)) return;
    const url = new URL(href, location.href);
    e.preventDefault();
    if (url.origin !== location.origin) return void window.open(url.href, '_blank', 'noopener');
    post('dfw:goto', { href: url.pathname + url.search });
  }
  addEventListener(
    'scroll',
    () => {
      reportSelection();
      if (pins.length) reportPins();
      if (hoverEl) post('dfw:hover', { rect: rectOf(hoverEl), label: labelOf(hoverEl), comp: !!hoverEl.dataset.dfwComponent });
    },
    true,
  );

  addEventListener('message', (e) => {
    if (e.source !== parent) return;
    const d = e.data || {};
    switch (d.type) {
      case 'dfw:env':
        if (d.theme) document.documentElement.dataset.theme = d.theme;
        else delete document.documentElement.dataset.theme;
        if (d.state) document.documentElement.dataset.state = d.state;
        else delete document.documentElement.dataset.state;
        break;
      case 'dfw:mode':
        mode = d.mode;
        if (!interactive()) {
          hoverEl = null;
          post('dfw:hover', { rect: null });
        }
        document.documentElement.style.cursor = mode === 'comment' ? 'crosshair' : '';
        placeHandles();
        break;
      case 'dfw:layout':
        layoutCfg = { enabled: !!d.enabled, scope: d.scope || '' };
        placeHandles();
        break;
      case 'dfw:layout-key':
        layoutKey(d.key, !!d.shift);
        break;
      case 'dfw:layout-verify':
        verifyLayout(d);
        break;
      case 'dfw:tokens':
        tokenVars = d.vars || [];
        tokenCache = null;
        break;
      case 'dfw:pins':
        pins = d.pins || [];
        reportPins();
        break;
      case 'dfw:layers':
        post('dfw:layers', { tree: buildLayers(), selected: selected ? layerNode.get(selected)?.node.i ?? null : null });
        break;
      case 'dfw:layer-hover': {
        const el = layerEls[d.i];
        post('dfw:hover', el ? { rect: rectOf(el), label: labelOf(el), comp: !!el.dataset.dfwComponent } : { rect: null });
        break;
      }
      case 'dfw:layer-select':
        if (layerEls[d.i]) select(layerEls[d.i], { scroll: true });
        break;
      case 'dfw:select-id': {
        if (!layerEls.length) buildLayers();
        let el = null;
        try {
          el = (d.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(d.dfwId)}"]`)) || (d.selector && document.querySelector(d.selector));
        } catch {}
        if (el) select(el, { scroll: !!d.scroll });
        else if (d.required) post('dfw:toast', { message: `Element ${d.dfwId || d.selector} not found in this frame.` });
        break;
      }
      case 'dfw:clear-selection':
        selected = null;
        if (editing) endEdit(false);
        placeHandles();
        break;
      case 'dfw:soft-reload':
        softReload(d);
        break;
      case 'dfw:presence': {
        let el = null;
        try {
          el = (d.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(d.dfwId)}"]`)) || (d.component && document.querySelector(`[data-dfw-component="${CSS.escape(d.component)}"]`)) || null;
        } catch {}
        post('dfw:presence', el && el.getClientRects().length ? { rect: rectOf(el), label: labelOf(el) } : { rect: null, fallback: !!d.fallback });
        break;
      }
      case 'dfw:reinspect':
        if (selected && selected.isConnected) select(selected);
        break;
      case 'dfw:edit':
        if (selected) startEdit(selected);
        break;
      case 'dfw:nav': {
        if (!selected) break;
        if (!layerEls.length || !layerNode.has(selected)) buildLayers();
        const cur = layerNode.has(selected) ? selected : layerAncestor(selected);
        let next = null;
        if (!cur) break;
        const meta = layerNode.get(cur);
        const sibs = meta.parent ? layerNode.get(meta.parent).node.children : [...layerNode.values()].filter((x) => !x.parent).map((x) => x.node);
        const idx = sibs.findIndex((n) => layerEls[n.i] === cur);
        if (d.dir === 'up') next = cur !== selected ? cur : meta.parent;
        else if (d.dir === 'down') next = meta.node.children[0] ? layerEls[meta.node.children[0].i] : null;
        else if (d.dir === 'left') next = sibs[idx - 1] ? layerEls[sibs[idx - 1].i] : null;
        else if (d.dir === 'right') next = sibs[idx + 1] ? layerEls[sibs[idx + 1].i] : null;
        if (next) select(next, { scroll: true });
        break;
      }
    }
  });

  const ready = (soft) => {
    reportSize();
    post('dfw:ready', { href: location.pathname + location.search, title: document.title, soft: soft === true });
  };

  /* ---------- soft update: apply a changed source in place (no reload, no flash, scroll kept) ---------- */

  const keyOf = (n) => (n.nodeType === 1 ? n.getAttribute('data-dfw-id') || n.id || null : null);
  const sameKind = (a, b) => a.nodeType === b.nodeType && a.nodeName === b.nodeName;
  const sheetPath = (href) => {
    try {
      return new URL(href, location.href).pathname;
    } catch {
      return href;
    }
  };
  const hasScript = (n) => n.nodeType === 1 && (n.nodeName === 'SCRIPT' || n.querySelector('script'));

  function syncAttrs(from, to, changed) {
    // A stylesheet link is refreshed by swapSheets (its href only differs by the cache key).
    if (from.nodeName === 'LINK' && sheetPath(from.getAttribute('href') || '') === sheetPath(to.getAttribute('href') || '')) return;
    let diff = false;
    for (const a of [...from.attributes]) {
      if (a.name === 'contenteditable' || to.hasAttribute(a.name)) continue;
      from.removeAttribute(a.name);
      diff = true;
    }
    for (const a of [...to.attributes]) {
      if (from.getAttribute(a.name) === a.value) continue;
      from.setAttribute(a.name, a.value);
      diff = true;
    }
    if (diff) changed.add(from);
  }

  /** Make `from` equal to `to`, touching only what differs. Changed elements are collected. Scripts never change this way. */
  function morph(from, to, changed) {
    if (from === editing) return;
    syncAttrs(from, to, changed);
    const olds = [...from.childNodes].filter((n) => !(n.nodeType === 1 && n.hasAttribute('data-dfw-ui')));
    const keyed = new Map();
    for (const o of olds) {
      const k = keyOf(o);
      if (k && !keyed.has(k)) keyed.set(k, o);
    }
    const used = new Set();
    const plan = [];
    let i = 0;
    for (const n of [...to.childNodes]) {
      const k = keyOf(n);
      let match = null;
      if (k) {
        const o = keyed.get(k);
        if (o && !used.has(o) && sameKind(o, n)) match = o;
      } else {
        while (i < olds.length && used.has(olds[i])) i++;
        // The same node a little further on (something was inserted or removed before it), else the next one of its kind.
        for (let j = i; j < Math.min(olds.length, i + 8) && !match; j++) if (!used.has(olds[j]) && !keyOf(olds[j]) && olds[j].isEqualNode(n)) match = olds[j];
        if (!match && olds[i] && sameKind(olds[i], n) && !keyOf(olds[i])) match = olds[i];
      }
      if (match) used.add(match);
      plan.push([n, match]);
    }
    for (const o of olds) {
      if (used.has(o)) continue;
      if (hasScript(o)) throw new Error('script removed');
      o.remove();
      if (o.nodeType === 1 || (o.nodeValue || '').trim()) changed.add(from);
    }
    let prev = null;
    for (const [n, match] of plan) {
      const want = prev ? prev.nextSibling : from.firstChild;
      let node = match;
      if (!node) {
        if (hasScript(n)) throw new Error('script added');
        node = document.importNode(n, true);
        from.insertBefore(node, want);
        if (node.nodeType === 1) changed.add(node);
        else if ((node.nodeValue || '').trim()) changed.add(from);
      } else {
        if (node !== want) from.insertBefore(node, want);
        if (node.nodeType !== 1) {
          if (node.nodeValue !== n.nodeValue) {
            node.nodeValue = n.nodeValue;
            if (node.nodeType === 3) changed.add(from);
          }
        } else if (node.nodeName === 'SCRIPT') {
          if (node.outerHTML !== n.outerHTML) throw new Error('script changed');
        } else if (!node.isEqualNode(n)) morph(node, n, changed);
      }
      prev = node;
    }
  }

  /** Reload the stylesheets whose file changed: the new one loads next to the old one, which goes when it is ready. */
  function swapSheets(files) {
    for (const link of [...document.querySelectorAll('link[rel~="stylesheet"][href]')]) {
      const p = sheetPath(link.getAttribute('href'));
      if (!files.some((f) => p.endsWith(`/${f}`))) continue;
      const next = link.cloneNode();
      next.setAttribute('href', `${p}?v=${Date.now()}`);
      const done = () => {
        link.remove();
        reportSize();
        reportSelection();
        if (pins.length) reportPins();
      };
      next.addEventListener('load', done);
      next.addEventListener('error', done);
      link.after(next);
    }
  }

  async function softReload(d) {
    try {
      const res = await fetch(location.href, { cache: 'no-store' });
      if (!res.ok) throw new Error(String(res.status));
      const doc = new DOMParser().parseFromString(await res.text(), 'text/html');
      // A page script may have built DOM the source does not have: patching would remove it and not run the script again.
      if ([...doc.scripts].some((s) => !s.hasAttribute('data-dfw-overlay'))) throw new Error('page script');
      const changed = new Set();
      morph(document.head, doc.head, changed);
      morph(document.body, doc.body, changed);
      swapSheets(d.files || []);
      hoverEl = null;
      layerEls = [];
      layerNode = new Map();
      tokenCache = null;
      if (selected && !selected.isConnected) selected = null;
      ready(true);
      // What changed, for the Studio to point at: the outermost changed elements that are on screen.
      const els = [...changed].filter((el) => el.nodeType === 1 && el.isConnected && el !== document.body && !document.head.contains(el) && !isUi(el) && el.getClientRects().length);
      const tops = els.filter((el) => !els.some((o) => o !== el && o.contains(el)));
      post('dfw:changed', { agent: !!d.agent, count: tops.length, items: tops.slice(0, 4).map((el) => ({ rect: rectOf(el), label: labelOf(pick(el) || el) })) });
    } catch {
      location.reload(); // anything unusual (scripts, parse problems): the plain reload is always right
    }
  }
  if (document.readyState === 'loading') document.addEventListener('DOMContentLoaded', ready);
  else ready();
  addEventListener('load', reportSize);
  if (document.fonts) document.fonts.ready.then(() => {
    reportSize();
    reportSelection();
    if (pins.length) reportPins();
  });
  if (window.ResizeObserver && document.documentElement) {
    let t = 0;
    new ResizeObserver(() => {
      clearTimeout(t);
      t = setTimeout(reportSize, 100);
    }).observe(document.documentElement);
  }
})();
