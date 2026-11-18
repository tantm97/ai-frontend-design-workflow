function measure(opts) {
  const vw = innerWidth;
  const vh = innerHeight;
  const vis = (el) => {
    const cs = getComputedStyle(el);
    if (cs.display === 'none' || cs.visibility === 'hidden' || Number(cs.opacity) === 0) return false;
    const r = el.getBoundingClientRect();
    return r.width > 1 && r.height > 1;
  };
  // Stress: grow lists so overflow behaviour is tested, not assumed.
  if (opts.stress) {
    for (const list of document.querySelectorAll('[data-dfw-grow]')) {
      const kids = [...list.children];
      if (!kids.length) continue;
      const target = Number(list.getAttribute('data-dfw-grow')) || 12;
      let i = 0;
      while (list.children.length < target) list.appendChild(kids[i++ % kids.length].cloneNode(true));
    }
  }
  const out = { vw, vh, overlaps: [], clipped: [], anchors: [], hOverflow: 0, blocks: [], components: [], content: null };
  out.hOverflow = Math.max(0, document.documentElement.scrollWidth - vw);
  const label = (el) => el.getAttribute('data-dfw-id') || (el.getAttribute('data-dfw-component') ? `${el.getAttribute('data-dfw-component')}${el.getAttribute('data-dfw-variant') ? `/${el.getAttribute('data-dfw-variant')}` : ''}` : el.tagName.toLowerCase() + (el.className && typeof el.className === 'string' ? '.' + el.className.split(' ')[0] : ''));

  // Content blocks: elements that paint something meaningful (text, media, controls, filled/bordered boxes),
  // excluding page-sized containers.
  // Visible rectangle: the element box clipped by every scrolling/clipping ancestor.
  const visibleRect = (el) => {
    const b = el.getBoundingClientRect();
    let r = { left: b.left, top: b.top, right: b.right, bottom: b.bottom };
    for (let a = el.parentElement; a && a !== document.documentElement; a = a.parentElement) {
      const cs = getComputedStyle(a);
      if (cs.overflowX === 'visible' && cs.overflowY === 'visible') continue;
      const ab = a.getBoundingClientRect();
      r = { left: Math.max(r.left, ab.left), top: Math.max(r.top, ab.top), right: Math.min(r.right, ab.right), bottom: Math.min(r.bottom, ab.bottom) };
      if (r.right <= r.left || r.bottom <= r.top) return null;
    }
    return { ...r, width: r.right - r.left, height: r.bottom - r.top };
  };
  const painted = [];
  for (const el of document.body.querySelectorAll('*')) {
    if (!vis(el)) continue;
    const r = visibleRect(el);
    if (!r || r.width < 1 || r.height < 1) continue;
    // Page-sized wrappers are not content; a large component surface (a card filling a task page) is.
    const comp = el.getAttribute('data-dfw-component');
    if (r.width * r.height > vw * vh * 0.6 && (!comp || /^(app-shell|page|nav)$/.test(comp))) continue;
    const cs = getComputedStyle(el);
    const hasText = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    const replaced = /^(IMG|SVG|INPUT|SELECT|TEXTAREA|BUTTON|CANVAS|VIDEO|svg)$/.test(el.tagName);
    const boxed = (cs.backgroundColor !== 'rgba(0, 0, 0, 0)' && cs.backgroundColor !== 'transparent') || parseFloat(cs.borderTopWidth) > 0 || cs.boxShadow !== 'none';
    if (hasText || replaced || boxed) painted.push({ el, r, hasText, replaced, boxed, cs });
  }
  out.blocks = painted.map((p) => ({ x: p.r.left, y: p.r.top + scrollY, w: p.r.width, h: p.r.height }));

  // Overlap between leaf content (text/media/controls) that are not nested.
  const leaves = painted.filter((p) => (p.hasText || p.replaced) && p.cs.position !== 'fixed');
  for (let i = 0; i < leaves.length; i++) {
    for (let j = i + 1; j < leaves.length; j++) {
      const a = leaves[i];
      const b = leaves[j];
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      // Overlap designed inside one component master (e.g. an icon over a select) is intentional.
      const ra = a.el.closest('[data-dfw-component]');
      if (ra && ra === b.el.closest('[data-dfw-component]')) continue;
      const x = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const y = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (x > 2 && y > 2 && x * y > 0.15 * Math.min(a.r.width * a.r.height, b.r.width * b.r.height)) {
        if (out.overlaps.length < 20) out.overlaps.push({ a: label(a.el.closest('[data-dfw-id],[data-dfw-component]') || a.el), b: label(b.el.closest('[data-dfw-id],[data-dfw-component]') || b.el), area: Math.round(x * y) });
      }
    }
  }
  // Boxes: two component instances with a visible box (card, stat, banner, table...) that are not nested
  // must not intrude into each other, even if their texts do not touch yet.
  const boxes = painted.filter((p) => p.boxed && p.el.hasAttribute('data-dfw-component') && !/absolute|fixed/.test(p.cs.position));
  for (let i = 0; i < boxes.length; i++) {
    for (let j = i + 1; j < boxes.length; j++) {
      const a = boxes[i];
      const b = boxes[j];
      if (a.el.contains(b.el) || b.el.contains(a.el)) continue;
      const x = Math.min(a.r.right, b.r.right) - Math.max(a.r.left, b.r.left);
      const y = Math.min(a.r.bottom, b.r.bottom) - Math.max(a.r.top, b.r.top);
      if (x > 2 && y > 2 && out.overlaps.length < 20) out.overlaps.push({ a: label(a.el), b: label(b.el), area: Math.round(x * y), boxes: true });
    }
  }
  // Clipped text: by its own box, or cut off horizontally by a clipping ancestor (vertical cuts are
  // normal inside scroll areas).
  for (const p of painted) {
    if (!p.hasText) continue;
    const full = p.el.getBoundingClientRect();
    let scroller = false;
    for (let a = p.el.parentElement; a && a !== document.body; a = a.parentElement) if (/auto|scroll/.test(getComputedStyle(a).overflowX)) { scroller = true; break; }
    if (!scroller && full.width - p.r.width > 2 && p.cs.position !== 'fixed' && out.clipped.length < 20) {
      out.clipped.push({ el: label(p.el.closest('[data-dfw-id],[data-dfw-component]') || p.el), text: p.el.textContent.trim().slice(0, 40) });
      continue;
    }
    const cs = p.cs;
    const clipsX = /hidden|clip/.test(cs.overflowX) && p.el.scrollWidth > p.el.clientWidth + 1 && cs.textOverflow !== 'ellipsis';
    const clipsY = /hidden|clip/.test(cs.overflowY) && p.el.scrollHeight > p.el.clientHeight + 1 && cs.webkitLineClamp === 'none';
    if ((clipsX || clipsY) && out.clipped.length < 20) out.clipped.push({ el: label(p.el), text: p.el.textContent.trim().slice(0, 40) });
  }
  // Anchors must be fully visible in the first viewport.
  for (const el of document.querySelectorAll('[data-dfw-anchor]')) {
    if (!vis(el)) continue;
    const r = el.getBoundingClientRect();
    out.anchors.push({ name: el.getAttribute('data-dfw-anchor') || label(el), top: Math.round(r.top), bottom: Math.round(r.bottom), left: Math.round(r.left), right: Math.round(r.right), inView: r.top >= -1 && r.bottom <= vh + 1 && r.left >= -1 && r.right <= vw + 1 });
  }
  // Main content region (excluding navigation) for whitespace analysis.
  const main = document.querySelector('main, [role="main"]') || document.body;
  const mr = main.getBoundingClientRect();
  out.content = { x: Math.max(0, mr.left), w: Math.min(vw, mr.right) - Math.max(0, mr.left), top: Math.max(0, mr.top + scrollY), docH: document.documentElement.scrollHeight };
  // Component signatures for cross-screen consistency.
  for (const el of document.querySelectorAll('[data-dfw-component]')) {
    if (!vis(el)) continue;
    const cs = getComputedStyle(el);
    out.components.push({ c: el.getAttribute('data-dfw-component'), v: el.getAttribute('data-dfw-variant') || '', s: el.getAttribute('data-dfw-size') || '', sig: [cs.fontSize, cs.fontWeight, cs.letterSpacing, cs.paddingTop, cs.paddingLeft, cs.borderTopLeftRadius].join(' ') });
  }
  // Touch targets: tappable controls under 44x44 at touch breakpoints. A checkbox/radio counts as its label;
  // links inside running text are exempt (WCAG 2.5.8 inline exception).
  out.small = [];
  if (opts.touch) {
    for (const el of document.querySelectorAll('a[href], button, input:not([type="hidden"]), select, textarea, summary, [role="button"], [role="link"], [role="tab"], [role="checkbox"], [role="radio"]')) {
      if (!vis(el) || el.closest('[aria-hidden="true"]')) continue;
      let box = el;
      if (/^(checkbox|radio)$/.test(el.type) && el.closest('label')) box = el.closest('label');
      const cs = getComputedStyle(box);
      if (el.tagName === 'A' && cs.display === 'inline' && [...el.parentElement.childNodes].some((n) => n !== el && n.nodeType === 3 && n.textContent.trim())) continue;
      const r = box.getBoundingClientRect();
      if ((r.width < 44 || r.height < 44) && out.small.length < 40) {
        const text = (el.getAttribute('aria-label') || el.textContent || el.value || '').trim().replace(/\s+/g, ' ').slice(0, 24);
        const own = el.getAttribute('data-dfw-id');
        out.small.push({ el: own || `${label(el.closest('[data-dfw-id],[data-dfw-component]') || el)}${text ? ` › ${text}` : ''}`, w: Math.round(r.width), h: Math.round(r.height) });
      }
    }
  }
  const firstBlock = main.querySelector('[data-dfw-id],[data-dfw-component]');
  out.contentLeft = firstBlock ? Math.round(firstBlock.getBoundingClientRect().left - mr.left) : null;
  return out;
}
