function collect() {
  const hex = (c) => {
    const m = /rgba?\(([\d.]+),\s*([\d.]+),\s*([\d.]+)(?:,\s*([\d.]+))?\)/.exec(c);
    if (!m || (m[4] !== undefined && Number(m[4]) < 0.5)) return null;
    return '#' + [m[1], m[2], m[3]].map((v) => Math.round(Number(v)).toString(16).padStart(2, '0')).join('');
  };
  const out = { colors: {}, families: {}, sizes: {}, radii: {}, spaces: {} };
  const bump = (bag, k, n = 1, role) => {
    if (!k) return;
    bag[k] = bag[k] || { n: 0, roles: {} };
    bag[k].n += n;
    if (role) bag[k].roles[role] = 1;
  };
  for (const el of [...document.querySelectorAll('body, body *')].slice(0, 4000)) {
    const r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) continue;
    const cs = getComputedStyle(el);
    if (cs.visibility === 'hidden' || cs.display === 'none') continue;
    const area = Math.min(50, (r.width * r.height) / 20000);
    bump(out.colors, hex(cs.backgroundColor), 1 + area, 'background');
    const text = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (text) {
      bump(out.colors, hex(cs.color), 1, 'text');
      bump(out.families, cs.fontFamily, 1);
      bump(out.sizes, cs.fontSize, 1);
    }
    if (parseFloat(cs.borderTopWidth) > 0) bump(out.colors, hex(cs.borderTopColor), 1, 'border');
    if (parseFloat(cs.borderTopLeftRadius) > 0) bump(out.radii, cs.borderTopLeftRadius, 1);
    for (const k of ['paddingTop', 'paddingLeft', 'rowGap', 'columnGap']) if (parseFloat(cs[k]) > 0) bump(out.spaces, cs[k], 1);
  }
  return out;
}
