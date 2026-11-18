function snapshotPage() {
  const round = (n) => Math.round(n * 100) / 100;
  const px = (v) => parseFloat(v) || 0;
  const cv = document.createElement('canvas');
  cv.width = cv.height = 1;
  const ctx = cv.getContext('2d', { willReadFrequently: true });
  const cache = new Map();
  const color = (c) => {
    if (!c || c === 'transparent') return null;
    if (cache.has(c)) return cache.get(c);
    let out = null;
    const m = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)(?:\s*[,/]\s*([\d.]+%?))?\s*\)$/.exec(c);
    let rgba;
    if (m) rgba = [+m[1], +m[2], +m[3], m[4] === undefined ? 1 : m[4].endsWith('%') ? parseFloat(m[4]) / 100 : +m[4]];
    else {
      ctx.clearRect(0, 0, 1, 1);
      ctx.fillStyle = '#000';
      ctx.fillStyle = c;
      ctx.fillRect(0, 0, 1, 1);
      const d = ctx.getImageData(0, 0, 1, 1).data;
      rgba = [d[0], d[1], d[2], d[3] / 255];
    }
    if (rgba[3] > 0) out = { hex: '#' + rgba.slice(0, 3).map((x) => Math.round(x).toString(16).padStart(2, '0')).join(''), opacity: round(rgba[3]) };
    cache.set(c, out);
    return out;
  };
  const splitTop = (s) => {
    const parts = [];
    let depth = 0;
    let cur = '';
    for (const ch of s) {
      if (ch === '(') depth++;
      if (ch === ')') depth--;
      if (ch === ',' && depth === 0) {
        parts.push(cur.trim());
        cur = '';
      } else cur += ch;
    }
    if (cur.trim()) parts.push(cur.trim());
    return parts;
  };
  const shadows = (s) => {
    if (!s || s === 'none') return null;
    return splitTop(s).map((part) => {
      const inset = /\binset\b/.test(part);
      const p = part.replace(/\binset\b/, '').trim();
      const cm = /([a-z-]+\([^()]*(?:\([^()]*\)[^()]*)*\)|#[0-9a-f]{3,8}\b)/i.exec(p);
      const nums = (cm ? p.replace(cm[0], '') : p).trim().split(/\s+/).filter(Boolean).map(px);
      return { inset, x: nums[0] || 0, y: nums[1] || 0, blur: nums[2] || 0, spread: nums[3] || 0, color: color(cm ? cm[0] : 'rgba(0,0,0,0.25)') };
    });
  };
  const visible = (cs) => cs.display !== 'none' && cs.visibility !== 'hidden' && Number(cs.opacity) > 0;
  const rect = (r) => ({ x: round(r.left + scrollX), y: round(r.top + scrollY), w: round(r.width), h: round(r.height) });
  const textStyle = (cs) => ({
    fontFamily: cs.fontFamily.split(',')[0].trim().replace(/^["']|["']$/g, ''),
    fontSize: px(cs.fontSize),
    fontWeight: String(cs.fontWeight),
    fontStyle: cs.fontStyle === 'italic' ? 'italic' : 'normal',
    lineHeight: cs.lineHeight === 'normal' ? 1.2 : round(px(cs.lineHeight) / px(cs.fontSize)),
    letterSpacing: cs.letterSpacing === 'normal' ? 0 : round(px(cs.letterSpacing)),
    color: color(cs.color),
    align: ['center', 'right', 'justify'].includes(cs.textAlign) ? cs.textAlign : cs.textAlign === 'end' ? 'right' : 'left',
    transform: ['uppercase', 'lowercase', 'capitalize'].includes(cs.textTransform) ? cs.textTransform : null,
    decoration: /underline/.test(cs.textDecorationLine) ? 'underline' : /line-through/.test(cs.textDecorationLine) ? 'line-through' : null,
  });
  const visual = (cs) => {
    const v = {};
    const bg = color(cs.backgroundColor);
    if (bg) v.fill = bg;
    if (cs.backgroundImage && cs.backgroundImage !== 'none') v.backgroundImage = cs.backgroundImage.slice(0, 160);
    const sides = ['Top', 'Right', 'Bottom', 'Left'].map((s) => {
      const w = px(cs[`border${s}Width`]);
      const c = color(cs[`border${s}Color`]);
      return w > 0 && c && !/none|hidden/.test(cs[`border${s}Style`]) ? { w, color: c, style: cs[`border${s}Style`] } : null;
    });
    if (sides.some(Boolean)) v.borders = sides;
    const radii = ['TopLeft', 'TopRight', 'BottomRight', 'BottomLeft'].map((c) => px(cs[`border${c}Radius`]));
    if (radii.some(Boolean)) v.radius = radii;
    const sh = shadows(cs.boxShadow);
    if (sh) v.shadows = sh;
    if (Number(cs.opacity) < 1) v.opacity = Number(cs.opacity);
    if (cs.overflowX !== 'visible' || cs.overflowY !== 'visible') v.clip = true;
    return v;
  };
  const hasVisual = (v) => Boolean(v.fill || v.borders || v.shadows || v.backgroundImage);
  const BLOCKERS = /^(svg|img|input|textarea|select|button|video|canvas|picture|iframe)$/i;
  const isTextLeaf = (el) => {
    if (!el.textContent.trim()) return false;
    for (const d of el.querySelectorAll('*')) {
      const c = getComputedStyle(d);
      if (c.display === 'none') continue;
      if (!/^(inline|contents)$/.test(c.display) || BLOCKERS.test(d.tagName) || d.hasAttribute('data-dfw-id') || d.hasAttribute('data-dfw-component')) return false;
      if (hasVisual(visual(c))) return false;
    }
    return true;
  };
  const contentRect = (el) => {
    const r = document.createRange();
    r.selectNodeContents(el);
    return rect(r.getBoundingClientRect());
  };
  const label = (el) => {
    const comp = el.getAttribute('data-dfw-component');
    if (comp) return [comp, el.getAttribute('data-dfw-variant'), el.getAttribute('data-dfw-size')].filter(Boolean).join('/');
    const cls = typeof el.className === 'string' && el.className.trim() ? '.' + el.className.trim().split(/\s+/)[0] : '';
    return el.tagName.toLowerCase() + cls;
  };
  const textNode = (characters, r, cs, anchor, name) => ({ kind: 'text', name: name || 'text', dfwId: null, anchor, ...r, characters, text: textStyle(cs) });

  const walk = (el, anchor) => {
    const cs = getComputedStyle(el);
    if (!visible(cs) || el.hasAttribute('data-dfw-overlay')) return null;
    const tag = el.tagName.toLowerCase();
    if (/^(script|style|template|noscript|link|meta|head)$/.test(tag)) return null;
    const id = el.getAttribute('data-dfw-id');
    const a = id || anchor || null;
    const comp = el.getAttribute('data-dfw-component');
    const node = { kind: 'frame', name: id || label(el), tag, dfwId: id || null, anchor: a, ...rect(el.getBoundingClientRect()), style: visual(cs), children: [] };
    if (comp) node.component = { name: comp, variant: el.getAttribute('data-dfw-variant') || null, size: el.getAttribute('data-dfw-size') || null };
    if (tag === 'svg') {
      const clone = el.cloneNode(true);
      clone.setAttribute('width', String(node.w));
      clone.setAttribute('height', String(node.h));
      if (!clone.getAttribute('xmlns')) clone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');
      const c = color(cs.color);
      return { ...node, kind: 'svg', svg: clone.outerHTML.replace(/currentColor/g, c ? c.hex : '#000000'), icon: el.getAttribute('data-dfw-icon') || null, style: {}, children: undefined };
    }
    if (tag === 'img') return { ...node, kind: 'image', src: el.currentSrc || el.src || null, alt: el.alt || '', children: undefined };
    if (/^(input|textarea|select)$/.test(tag)) {
      const value = tag === 'select' ? el.options[el.selectedIndex]?.text || '' : el.type === 'checkbox' || el.type === 'radio' ? '' : el.value || el.placeholder || '';
      if (value.trim()) {
        const pad = { l: px(cs.paddingLeft) + px(cs.borderLeftWidth), t: px(cs.borderTopWidth) };
        const t = textNode(value.trim(), { x: node.x + pad.l, y: node.y + pad.t, w: Math.max(1, node.w - pad.l - px(cs.paddingRight)), h: Math.max(1, node.h - 2 * pad.t) }, cs, a, 'value');
        t.placeholder = !el.value;
        node.children.push(t);
      }
      return node;
    }
    if (isTextLeaf(el)) {
      const t = textNode(el.innerText.trim(), contentRect(el), cs, a, null);
      if (hasVisual(node.style) || comp || node.style.clip || node.style.opacity !== undefined) {
        node.children.push(t);
        return node;
      }
      // A plain text element: the text layer itself carries the element's id and name.
      return { ...t, name: id || 'text', dfwId: id || null, tag };
    }
    for (const child of el.childNodes) {
      if (child.nodeType === 3 && child.textContent.trim()) {
        const r = document.createRange();
        r.selectNodeContents(child);
        node.children.push(textNode(child.textContent.trim().replace(/\s+/g, ' '), rect(r.getBoundingClientRect()), cs, a, null));
      } else if (child.nodeType === 1) {
        const c = walk(child, a);
        if (c) node.children.push(c);
      }
    }
    return node;
  };
  const html = getComputedStyle(document.documentElement);
  const body = getComputedStyle(document.body);
  const bg = color(body.backgroundColor) || color(html.backgroundColor) || { hex: '#ffffff', opacity: 1 };
  const root = walk(document.body, null) || { children: [] };
  return {
    width: Math.max(innerWidth, document.documentElement.scrollWidth),
    height: Math.max(innerHeight, document.documentElement.scrollHeight),
    background: bg,
    children: root.children,
    title: document.title,
  };
}
