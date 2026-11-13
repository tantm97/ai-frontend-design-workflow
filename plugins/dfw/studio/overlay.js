// dfw Studio in-frame agent, injected into every design document served by the
// Studio. It reports hover/selection geometry, layers, design-token usage and
// text edits to the parent Studio via postMessage, and forwards wheel/keys so
// the canvas can pan and zoom over frames. The Studio draws all chrome
// (selection boxes, pins) itself, in screen space. Framework-agnostic.
(() => {
  if (window.__dfwOverlay) return;
  window.__dfwOverlay = true;
  const params = new URLSearchParams(location.search);
  if (params.get('theme')) document.documentElement.dataset.theme = params.get('theme');
  if (params.get('state')) document.documentElement.dataset.state = params.get('state');
  if (window.parent === window) return; // opened directly: plain page

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
    if (!el) return post('dfw:select', { target: null });
    if (scroll) el.scrollIntoView({ block: 'nearest' });
    post('dfw:select', { comment: mode === 'comment', ...selPayload(el, inspectIt && mode !== 'comment') });
  }

  function reportSelection() {
    if (selected && selected.isConnected) post('dfw:sel-rect', { rect: rectOf(selected) });
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

  /* ---------- events ---------- */

  const capture = { capture: true };
  const interactive = () => mode === 'select' || mode === 'comment';
  const inEdit = (t) => editing && editing.el.contains(t);

  let raf = 0;
  document.addEventListener(
    'mousemove',
    (e) => {
      if (midPan) return post('dfw:pan', { phase: 'move', sx: e.screenX, sy: e.screenY });
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
      if (inEdit(e.target)) return;
      if (editing) endEdit(true);
      if (interactive() && !isUi(e.target)) {
        e.preventDefault();
        e.stopPropagation();
      }
      post('dfw:focus');
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
        break;
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

  const ready = () => {
    reportSize();
    post('dfw:ready', { href: location.pathname + location.search, title: document.title });
  };
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
