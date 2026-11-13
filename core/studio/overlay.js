// dfw review overlay, injected into every design screen served by the studio.
// Talks to the parent studio via postMessage. Framework-agnostic: works on any DOM.
(() => {
  if (window.__dfwOverlay) return;
  window.__dfwOverlay = true;
  const params = new URLSearchParams(location.search);
  if (params.get('theme')) document.documentElement.dataset.theme = params.get('theme');
  if (params.get('state')) document.documentElement.dataset.state = params.get('state');
  if (window.parent === window) return; // opened directly: no overlay UI

  let commentMode = false;
  let altKey = false;
  let hoverEl = null;
  let pins = [];
  const ACCENT = '#1f5eff';
  const STATUS = { open: '#c2410c', acknowledged: '#c2410c', reopened: '#c2410c', fixed: '#a16207', resolved: '#15803d' };

  const layer = document.createElement('div');
  layer.setAttribute('data-dfw-ui', '');
  layer.style.cssText = 'position:absolute;left:0;top:0;width:0;height:0;z-index:2147483646;pointer-events:none;';
  const box = document.createElement('div');
  box.style.cssText = `position:absolute;border:2px solid ${ACCENT};background:rgba(31,94,255,.08);border-radius:3px;display:none;transition:none;`;
  const tag = document.createElement('div');
  tag.style.cssText = `position:absolute;transform:translateY(-100%);background:${ACCENT};color:#fff;font:600 11px/1.6 ui-monospace,Menlo,monospace;padding:0 6px;border-radius:3px 3px 0 0;white-space:nowrap;display:none;`;
  layer.append(box, tag);
  const pinLayer = document.createElement('div');
  layer.append(pinLayer);
  const mount = () => document.body && !layer.isConnected && document.body.append(layer);
  mount();
  document.addEventListener('DOMContentLoaded', mount);

  const isOurs = (el) => el && el.closest && el.closest('[data-dfw-ui]');

  function targetOf(el) {
    if (!el || el === document.documentElement || el === document.body) return null;
    if (altKey) return el;
    return el.closest('[data-dfw-id]') || el;
  }

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
    if (!name && el.getAttribute('aria-labelledby')) {
      name = el.getAttribute('aria-labelledby').split(/\s+/).map((id) => document.getElementById(id)?.textContent || '').join(' ');
    }
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
      bbox: [Math.round(r.left + scrollX), Math.round(r.top + scrollY), Math.round(r.width), Math.round(r.height)],
      viewportBbox: [Math.round(r.left), Math.round(r.top), Math.round(r.width), Math.round(r.height)],
      a11y: a11y(el),
      text: (el.innerText || el.value || '').replace(/\s+/g, ' ').trim().slice(0, 160),
      styles: {
        color: cs.color,
        background: cs.backgroundColor,
        font: `${cs.fontWeight} ${cs.fontSize}/${cs.lineHeight} ${cs.fontFamily.split(',')[0]}`,
        padding: cs.padding,
        radius: cs.borderRadius,
      },
      scroll: [Math.round(scrollX), Math.round(scrollY)],
    };
  }

  function show(el, color = ACCENT) {
    if (!el) {
      box.style.display = tag.style.display = 'none';
      return;
    }
    const r = el.getBoundingClientRect();
    Object.assign(box.style, { display: 'block', left: `${r.left + scrollX}px`, top: `${r.top + scrollY}px`, width: `${r.width}px`, height: `${r.height}px`, borderColor: color });
    tag.textContent = el.dataset?.dfwId || (altKey ? cssPath(el) : el.tagName.toLowerCase());
    Object.assign(tag.style, { display: 'block', left: `${r.left + scrollX}px`, top: `${r.top + scrollY}px`, background: color });
  }

  function resolvePin(p) {
    try {
      return (p.selector && document.querySelector(p.selector)) || (p.dfwId && document.querySelector(`[data-dfw-id="${CSS.escape(p.dfwId)}"]`)) || null;
    } catch {
      return null;
    }
  }

  function drawPins() {
    pinLayer.innerHTML = '';
    for (const p of pins) {
      const el = resolvePin(p);
      if (!el) continue;
      const r = el.getBoundingClientRect();
      const d = document.createElement('button');
      d.type = 'button';
      d.textContent = p.n;
      d.title = `Comment #${p.n} (${p.status})`;
      d.style.cssText = `position:absolute;pointer-events:auto;left:${r.right + scrollX - 10}px;top:${r.top + scrollY - 10}px;min-width:22px;height:22px;padding:0 5px;border-radius:11px;border:2px solid #fff;background:${STATUS[p.status] || '#78716c'};color:#fff;font:700 11px/18px ui-sans-serif,system-ui;cursor:pointer;box-shadow:0 1px 4px rgba(0,0,0,.3);`;
      d.addEventListener('click', (e) => {
        e.preventDefault();
        e.stopPropagation();
        parent.postMessage({ type: 'dfw:pin', id: p.id }, '*');
      });
      pinLayer.append(d);
    }
  }

  let selected = null;
  document.addEventListener(
    'mousemove',
    (e) => {
      altKey = e.altKey;
      if (!commentMode || selected) return;
      const t = isOurs(e.target) ? null : targetOf(e.target);
      if (t !== hoverEl) {
        hoverEl = t;
        show(t);
      }
    },
    true,
  );
  document.addEventListener(
    'click',
    (e) => {
      if (!commentMode || isOurs(e.target)) return;
      e.preventDefault();
      e.stopPropagation();
      const t = targetOf(e.target);
      if (!t) return;
      selected = t;
      show(t, '#0b3fd6');
      parent.postMessage({ type: 'dfw:select', target: info(t) }, '*');
    },
    true,
  );
  // Block other interactions while commenting so clicks don't navigate or submit.
  for (const ev of ['mousedown', 'mouseup', 'submit', 'pointerdown']) {
    document.addEventListener(ev, (e) => commentMode && !isOurs(e.target) && (e.preventDefault(), e.stopPropagation()), true);
  }
  document.addEventListener('keydown', (e) => {
    if (e.target.closest && e.target.closest('input, textarea, select, [contenteditable]')) return;
    if (['c', 'C', 'v', 'V', 'Escape', '+', '=', '-', '0'].includes(e.key) && !e.metaKey && !e.ctrlKey) {
      parent.postMessage({ type: 'dfw:key', key: e.key }, '*');
    }
  });
  addEventListener('scroll', () => {
    drawPins();
    if (selected) show(selected, '#0b3fd6');
  });
  addEventListener('resize', drawPins);

  addEventListener('message', (e) => {
    const d = e.data || {};
    if (d.type === 'dfw:mode') {
      commentMode = !!d.comment;
      document.documentElement.style.cursor = commentMode ? 'crosshair' : '';
      if (!commentMode) {
        selected = hoverEl = null;
        show(null);
      }
    } else if (d.type === 'dfw:pins') {
      pins = d.pins || [];
      mount();
      drawPins();
    } else if (d.type === 'dfw:clear-selection') {
      selected = null;
      show(null);
    } else if (d.type === 'dfw:highlight') {
      const el = resolvePin(d);
      if (el) {
        el.scrollIntoView({ block: 'center' });
        show(el, '#0b3fd6');
        setTimeout(() => !selected && show(null), 1800);
      }
    }
  });
  if (document.fonts) document.fonts.ready.then(drawPins);
})();
