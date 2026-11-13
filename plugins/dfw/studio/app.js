// dfw Studio client: a Figma-like canvas over the design workspace.
// Vanilla JS, no build step. Frames are real-size iframes inside one
// transformed "world" element; pan/zoom only change that transform.
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];

const store = {
  get(k, d) {
    try {
      const v = localStorage.getItem(`dfw:${k}`);
      return v == null ? d : JSON.parse(v);
    } catch {
      return d;
    }
  },
  set(k, v) {
    try {
      localStorage.setItem(`dfw:${k}`, JSON.stringify(v));
    } catch {}
  },
};

const ZMIN = 0.02;
const ZMAX = 8;

const S = {
  manifest: null,
  pages: [],
  state: null,
  comments: [],
  tokens: [],
  components: new Map(),
  page: null,
  mode: 'select',
  cam: { x: 0, y: 0, z: 0.25 },
  cams: {},
  activeBps: store.get('bps2', null),
  showStates: store.get('states', false),
  fullHeight: store.get('full', false),
  theme: store.get('theme', ''),
  frames: new Map(), // key -> frame
  byWin: new Map(), // contentWindow -> frame
  rows: [],
  selKey: null, // selected frame
  sel: null, // selected element payload {key, ...}
  hover: null,
  layers: null, // {key, tree, parents}
  layerOpen: new Map(),
  filter: 'active',
  selectedComment: null,
  pending: null,
  space: false,
  panning: null,
  zoomOnSelect: false,
};

const ACTIVE = ['open', 'acknowledged', 'reopened'];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const viewport = $('#viewport');
const world = $('#world');
const hud = $('#hud');
const ALL = '__all';
const PKEY = '__present';
// Presenter state: a pseudo-frame shown full-window.
const P = { present: true, key: PKEY, open: false, loaded: false, fit: store.get('pfit', true), scale: 1, screen: null, variant: null, bp: null, state: null, file: null, system: false, pinRects: [] };

async function api(path, opts = {}) {
  const res = await fetch(path, { headers: { 'Content-Type': 'application/json' }, ...opts });
  const data = await res.json();
  if (!res.ok) throw new Error(data.error || res.statusText);
  return data;
}

function toast(msg) {
  const t = $('#toast');
  t.textContent = msg;
  t.hidden = false;
  clearTimeout(toast.t);
  toast.t = setTimeout(() => (t.hidden = true), 3200);
}

const screenById = (id) => (S.manifest.screens || []).find((s) => s.id === id);
const currentPage = () => (S.page === ALL ? { id: ALL, title: 'All pages', screens: [], all: true } : S.pages.find((p) => p.id === S.page) || S.pages[0]);
const post = (f, msg) => f?.iframe?.contentWindow?.postMessage(msg, '*');
const postAll = (msg) => S.frames.forEach((f) => f.loaded && post(f, msg));

/* ---------------------------------------------------------------- frames model */
// A canvas page is laid out like a design file: a section with a page header
// (title + description), then ROWS of frames (one per breakpoint, a variants
// row, a states row, or manifest-declared sections). A row may carry an
// annotations panel (spec notes) at its left.

const L = { PAD: 96, HEAD_GAP: 72, ROW_GAP: 240, FRAME_GAP: 80, GUTTER_NOTES: 420, GUTTER: 300, GUTTER_GAP: 80, SEC_GAP: 480 };
const bpShort = (bp) => (bp ? bp.short || String(bp.label || bp.id).charAt(0).toUpperCase() : '');
const primaryVariant = (s) => (s.variants || []).find((v) => v.recommended) || (s.variants || [])[0];
const activeBps = () => S.manifest.breakpoints.filter((b) => S.activeBps.includes(b.id));
const statesOf = (s) => (s.states && s.states.length ? s.states : S.manifest.states || []);

function frameSrc(file, state) {
  const q = new URLSearchParams();
  if (S.theme) q.set('theme', S.theme);
  if (state) q.set('state', state);
  const qs = q.toString();
  return `/design/${file}${qs ? `?${qs}` : ''}`;
}

function makeSpec(s, v, bp, state) {
  const multi = (s.variants || []).length > 1;
  return {
    key: [s.id, v.id, bp.id, state].filter(Boolean).join('::'),
    screen: s,
    variant: v,
    bp,
    state: state || null,
    file: v.file,
    title: [bpShort(bp), s.title || s.id, multi ? v.label || v.id : null, state].filter(Boolean).join(' · '),
    src: frameSrc(v.file, state),
    w: bp.width,
    h: bp.height,
    auto: S.fullHeight,
  };
}

function systemSpec() {
  return { key: 'system', system: true, screen: null, title: 'Design System', src: '/__dfw/system.html', w: 1440, h: 900, auto: true, bp: null };
}

/** Resolve "screen" or "screen/variant" refs. */
function refs(list) {
  return (list || [])
    .map((r) => {
      const [sid, vid] = String(r).split('/');
      const s = screenById(sid);
      if (!s) return null;
      return { s, v: vid ? (s.variants || []).find((x) => x.id === vid) || null : null };
    })
    .filter(Boolean);
}

/** Page model: { id, title, description, rows: [{ title, sub, notes, frames: [spec] }] }. */
function pageModel(page) {
  if (page.system) {
    return {
      id: page.id,
      title: page.title,
      description: page.description || 'Foundations and components, generated from design/tokens and design/system/components.',
      rows: [{ title: null, notes: page.notes || null, frames: [systemSpec()] }],
    };
  }
  const bps = activeBps();
  const primary = bps[0];
  const screens = page.screens.map(screenById).filter(Boolean);
  const rows = [];
  if (page.sections && page.sections.length) {
    for (const sec of page.sections) {
      const kind = sec.kind || (sec.breakpoint ? 'breakpoint' : sec.states ? 'states' : 'custom');
      const bpDecl = sec.breakpoint && S.manifest.breakpoints.find((b) => b.id === sec.breakpoint);
      if (kind === 'breakpoint' && bpDecl && !S.activeBps.includes(bpDecl.id)) continue;
      const bp = bpDecl || primary;
      if (!bp) continue;
      const list = sec.screens ? refs(sec.screens) : screens.map((s) => ({ s, v: null }));
      const frames = [];
      for (const { s, v } of list) {
        if (kind === 'variants') {
          const vs = v ? [v] : (s.variants || []).filter((x) => x !== primaryVariant(s));
          for (const x of vs) frames.push(makeSpec(s, x, bp, null));
        } else if (kind === 'states') {
          for (const st of sec.states || statesOf(s)) frames.push(makeSpec(s, v || primaryVariant(s), bp, st));
        } else frames.push(makeSpec(s, v || primaryVariant(s), bp, null));
      }
      const defTitle = { breakpoint: `${bp.label || bp.id} ${bp.width}`, variants: 'Variants', states: 'States' }[kind] || '';
      rows.push({ title: sec.title || defTitle, sub: kind === 'breakpoint' ? `${bp.width} × ${bp.height}` : `at ${bp.label || bp.id} ${bp.width}`, notes: sec.notes || null, frames });
    }
  } else if (primary) {
    for (const bp of bps) {
      rows.push({ title: `${bp.label || bp.id} ${bp.width}`, sub: `${bp.width} × ${bp.height}`, frames: screens.map((s) => makeSpec(s, primaryVariant(s), bp, null)) });
    }
    const alt = screens.flatMap((s) => (s.variants || []).filter((v) => v !== primaryVariant(s)).map((v) => makeSpec(s, v, primary, null)));
    if (alt.length) rows.push({ title: 'Variants', sub: `Alternatives at ${primary.label || primary.id} ${primary.width}`, frames: alt });
    if (S.showStates) {
      const st = screens.flatMap((s) => statesOf(s).map((x) => makeSpec(s, primaryVariant(s), primary, x)));
      const names = [...new Set(st.map((f) => f.state))];
      if (st.length) rows.push({ title: `States (${names.join(' · ')})`, sub: `at ${primary.label || primary.id} ${primary.width}`, frames: st });
    }
  }
  if (page.notes?.length) {
    if (!rows.length) rows.push({ title: null, frames: [] });
    rows[0].notes = [...page.notes, ...(rows[0].notes || [])];
  }
  return { id: page.id, title: page.title, description: page.description || '', rows: rows.filter((r) => r.frames.length || r.notes?.length) };
}

const frameCount = (page) => pageModel(page).rows.reduce((n, r) => n + r.frames.length, 0);

function notesHtml(title, sub, notes) {
  return `<div class="notes">${title ? `<h3>${esc(title)}</h3>` : ''}${sub ? `<p class="nsub">${esc(sub)}</p>` : ''}${notes
    .map((g) => `<div class="ng">${g.title ? `<h4>${esc(g.title)}</h4>` : ''}<ul>${(g.items || []).map((i) => `<li>${esc(i)}</li>`).join('')}</ul></div>`)
    .join('')}</div>`;
}

let io = null;
function buildPage() {
  world.innerHTML = '';
  S.frames.clear();
  for (const [w, f] of S.byWin) if (!f.present) S.byWin.delete(w);
  S.hover = null;
  S.layers = null;
  io?.disconnect();
  io = new IntersectionObserver(
    (entries) => {
      for (const e of entries) if (e.isIntersecting) loadFrame(S.frames.get(e.target.dataset.key));
    },
    { root: viewport, rootMargin: '400px' },
  );
  const pages = S.page === ALL ? S.pages : [currentPage()].filter(Boolean);
  S.sections = [];
  for (const page of pages) {
    const model = pageModel(page);
    const sec = { id: page.id, model, rows: [] };
    sec.el = document.createElement('section');
    sec.el.className = 'psec';
    sec.el.dataset.page = page.id;
    sec.el.innerHTML = `<div class="psec-label">${esc(model.title)}</div>`;
    sec.head = document.createElement('header');
    sec.head.className = 'psec-head';
    sec.head.innerHTML = `<h1>${esc(model.title)}</h1>${model.description ? `<p>${esc(model.description)}</p>` : ''}`;
    world.append(sec.el, sec.head);
    const hasNotes = model.rows.some((r) => r.notes?.length);
    sec.gutter = model.rows.some((r) => r.title || r.notes?.length) ? (hasNotes ? L.GUTTER_NOTES : L.GUTTER) : 0;
    for (const r of model.rows) {
      const row = { model: r, frames: [] };
      if (sec.gutter && (r.title || r.notes?.length)) {
        row.gut = document.createElement('div');
        row.gut.className = 'prow-gutter';
        row.gut.style.width = `${sec.gutter}px`;
        row.gut.innerHTML = r.notes?.length ? notesHtml(r.title, r.sub, r.notes) : `<h2>${esc(r.title)}</h2>${r.sub ? `<p>${esc(r.sub)}</p>` : ''}`;
        world.append(row.gut);
      }
      for (const spec of r.frames) {
        let key = spec.key;
        for (let n = 2; S.frames.has(key); n++) key = `${spec.key}~${n}`;
        const f = { ...spec, key, page: page.id, x: 0, y: 0, loaded: false, sizeCount: 0 };
        const el = document.createElement('div');
        el.className = 'frame';
        el.dataset.key = f.key;
        el.innerHTML = `<div class="frame-title" title="${esc(f.title)} (double-click to present)">${esc(f.title)}<a href="${esc(f.src)}" target="_blank" rel="noopener" title="Open in a new tab">↗</a></div><div class="placeholder">${esc(f.title)}</div>`;
        const t = $('.frame-title', el);
        t.addEventListener('mousedown', (e) => {
          if (e.target.tagName === 'A' || e.button !== 0 || S.mode === 'hand' || S.space) return;
          e.stopPropagation();
          selectFrame(f.key);
        });
        t.addEventListener('dblclick', (e) => {
          if (e.target.tagName === 'A') return;
          openPresent(f);
        });
        f.el = el;
        S.frames.set(f.key, f);
        row.frames.push(f);
        world.append(el);
      }
      sec.rows.push(row);
    }
    S.sections.push(sec);
  }
  // Measure text blocks once (offset sizes ignore the world transform).
  for (const sec of S.sections) {
    sec.headH = sec.head.offsetHeight;
    for (const r of sec.rows) r.gutH = r.gut ? r.gut.offsetHeight : 0;
  }
  positionFrames();
  for (const f of S.frames.values()) io.observe(f.el);
  $('#empty').hidden = S.frames.size > 0;
  renderPages();
  renderLayers();
  renderInspector();
  updateHud();
}

function positionFrames() {
  let secY = 0;
  S.secBounds = {};
  for (const sec of S.sections) {
    const x0 = 0;
    let y = secY + L.PAD;
    sec.head.style.cssText = `left:${x0 + L.PAD}px;top:${y}px`;
    y += sec.headH + L.HEAD_GAP;
    let maxW = sec.head.offsetWidth;
    sec.rows.forEach((r, i) => {
      if (i) y += L.ROW_GAP;
      const fx0 = x0 + L.PAD + (sec.gutter ? sec.gutter + L.GUTTER_GAP : 0);
      if (r.gut) r.gut.style.cssText = `left:${x0 + L.PAD}px;top:${y}px;width:${sec.gutter}px`;
      let x = fx0;
      let h = r.gutH;
      for (const f of r.frames) {
        f.x = x;
        f.y = y;
        f.el.style.cssText = `left:${f.x}px;top:${f.y}px;width:${f.w}px;height:${f.h}px`;
        x += f.w + L.FRAME_GAP;
        h = Math.max(h, f.h);
      }
      maxW = Math.max(maxW, x - L.FRAME_GAP - x0 - L.PAD);
      y += h;
    });
    const w = maxW + L.PAD * 2;
    const h = y + L.PAD - secY;
    sec.el.style.cssText = `left:${x0}px;top:${secY}px;width:${w}px;height:${h}px`;
    S.secBounds[sec.id] = [x0, secY, w, h];
    secY += h + L.SEC_GAP;
  }
}

function loadFrame(f) {
  if (!f || f.iframe) return;
  const iframe = document.createElement('iframe');
  iframe.name = f.key;
  iframe.title = f.title;
  iframe.src = f.src;
  f.iframe = iframe;
  $('.placeholder', f.el)?.remove();
  f.el.append(iframe);
  S.byWin.set(iframe.contentWindow, f);
}

function pageBounds() {
  const b = Object.values(S.secBounds || {});
  if (!b.length) return null;
  const x0 = Math.min(...b.map((r) => r[0]));
  const y0 = Math.min(...b.map((r) => r[1]));
  const x1 = Math.max(...b.map((r) => r[0] + r[2]));
  const y1 = Math.max(...b.map((r) => r[1] + r[3]));
  return [x0, y0, x1 - x0, y1 - y0];
}

/* ---------------------------------------------------------------- camera */

let rafCam = 0;
let movingT = 0;
function applyCam() {
  cancelAnimationFrame(rafCam);
  rafCam = requestAnimationFrame(() => {
    const { x, y, z } = S.cam;
    world.style.transform = `translate(${x}px, ${y}px) scale(${z})`;
    world.style.setProperty('--iz', String(1 / z));
    world.classList.toggle('tiny', z < 0.075);
    world.classList.add('moving');
    clearTimeout(movingT);
    movingT = setTimeout(() => world.classList.remove('moving'), 160);
    $('#zoom-label').textContent = `${Math.round(z * 100)}%`;
    S.cams[S.page] = { ...S.cam };
    updateHud();
  });
}

function zoomAt(px, py, z) {
  z = Math.min(ZMAX, Math.max(ZMIN, z));
  const wx = (px - S.cam.x) / S.cam.z;
  const wy = (py - S.cam.y) / S.cam.z;
  S.cam = { x: px - wx * z, y: py - wy * z, z };
  applyCam();
}
const vpSize = () => [viewport.clientWidth, viewport.clientHeight];
const zoomCenter = (z) => {
  const [w, h] = vpSize();
  zoomAt(w / 2, h / 2, z);
};
const STEPS = [0.02, 0.05, 0.1, 0.15, 0.25, 0.33, 0.5, 0.75, 1, 1.5, 2, 3, 4, 8];
const zoomStep = (dir) => {
  const z = S.cam.z;
  const next = dir > 0 ? STEPS.find((s) => s > z + 0.001) || ZMAX : [...STEPS].reverse().find((s) => s < z - 0.001) || ZMIN;
  zoomCenter(next);
};

function zoomToRect([x, y, w, h], { max = 1, pad = 64 } = {}) {
  const [vw, vh] = vpSize();
  if (!vw || !vh || !w || !h) return;
  const z = Math.min(max, Math.max(ZMIN, Math.min((vw - pad * 2) / w, (vh - pad * 2) / h)));
  S.cam = { z, x: (vw - w * z) / 2 - x * z, y: (vh - h * z) / 2 - y * z };
  applyCam();
}

function zoomToFit({ initial = false } = {}) {
  const b = pageBounds();
  if (!b) return;
  // Leave room for titles above the first row.
  zoomToRect([b[0], b[1] - 60, b[2], b[3] + 60], { max: 1 });
  if (initial && S.cam.z < 0.2) {
    // Too small to read: like opening a design file, start at the top-left of
    // the first section with its header and first row filling the height.
    const [, vh] = vpSize();
    const sec = S.sections[0];
    const firstH = sec ? L.PAD + sec.headH + L.HEAD_GAP + Math.max(0, ...sec.rows[0]?.frames.map((f) => f.h) || [0], sec.rows[0]?.gutH || 0) + 40 : b[3];
    const z = Math.min(0.5, Math.max(0.12, (vh - 48) / firstH));
    S.cam = { z, x: 32 - b[0] * z, y: 24 - b[1] * z };
    applyCam();
  }
}

function zoomToSelection() {
  const f = S.frames.get(S.sel?.key || S.selKey);
  if (!f) return zoomToFit();
  if (S.sel?.rect) return zoomToRect([f.x + S.sel.rect[0], f.y + S.sel.rect[1], S.sel.rect[2], S.sel.rect[3]], { max: 4, pad: 120 });
  zoomToRect([f.x, f.y, f.w, f.h], { max: 2 });
}

/* ---------------------------------------------------------------- HUD (screen-space chrome) */

// Frames by key, including the presenter's pseudo-frame.
const frameByKey = (k) => (k === PKEY ? (P.open ? P : null) : S.frames.get(k));
// Frames whose chrome is visible right now: the presenter hides the canvas.
const visibleFrame = (f) => !!f && (P.open ? f === P : !f.present);

let vpRect = null;
/** Frame-viewport rect -> client (window) coordinates. */
function toScreen(f, r) {
  if (f.present) {
    const b = P.iframe.getBoundingClientRect();
    return [b.left + r[0] * P.scale, b.top + r[1] * P.scale, r[2] * P.scale, r[3] * P.scale];
  }
  const vr = vpRect || (vpRect = viewport.getBoundingClientRect());
  const { x, y, z } = S.cam;
  return [(f.x + r[0]) * z + x + vr.left, (f.y + r[1]) * z + y + vr.top, r[2] * z, r[3] * z];
}

function place(el, [x, y, w, h]) {
  el.style.transform = `translate(${x}px, ${y}px)`;
  el.style.width = `${w}px`;
  el.style.height = `${h}px`;
}

function updateHud() {
  // Clip the HUD to the canvas (or to the presenter stage).
  const clip = P.open ? $('#p-stage').getBoundingClientRect() : vpRect || (vpRect = viewport.getBoundingClientRect());
  hud.style.clipPath = `inset(${clip.top}px ${innerWidth - clip.right}px ${innerHeight - clip.bottom}px ${clip.left}px)`;
  const hb = $('#hover-box');
  const hf = S.hover && frameByKey(S.hover.key);
  const same = S.sel && S.hover && S.sel.key === S.hover.key && S.sel.rect && S.hover.rect && S.sel.rect.join() === S.hover.rect.join();
  if (visibleFrame(hf) && S.hover.rect && !same && S.mode !== 'hand') {
    hb.hidden = false;
    hb.classList.toggle('comp', !!S.hover.comp);
    place(hb, toScreen(hf, S.hover.rect));
  } else hb.hidden = true;
  const sb = $('#sel-box');
  const sf = S.sel && frameByKey(S.sel.key);
  if (visibleFrame(sf) && S.sel.rect) {
    sb.hidden = false;
    sb.classList.toggle('comp', !!S.sel.own?.comp);
    const r = toScreen(sf, S.sel.rect);
    place(sb, [r[0] - 1, r[1] - 1, r[2] + 2, r[3] + 2]);
    $('.tag', sb).textContent = S.sel.label;
    $('.dims', sb).textContent = `${Math.round(S.sel.rect[2])} × ${Math.round(S.sel.rect[3])}`;
  } else sb.hidden = true;
  for (const b of $$('#pins button')) {
    const f = frameByKey(b.dataset.key);
    b.hidden = !visibleFrame(f);
    if (b.hidden) continue;
    const r = toScreen(f, JSON.parse(b.dataset.rect));
    b.style.left = `${r[0] + r[2]}px`;
    b.style.top = `${r[1]}px`;
  }
}

/* ---------------------------------------------------------------- pins */

const STATUS_COLOR = { open: '#c2410c', acknowledged: '#c2410c', reopened: '#c2410c', fixed: '#a16207', resolved: '#15803d' };

function pinsFor(f) {
  if (!f) return [];
  return S.comments
    .filter((c) => c.status !== 'dismissed' && (f.system ? c.screen === 'system' : f.screen && c.screen === f.screen.id && c.variant === f.variant?.id && (c.state || null) === (f.state || null)))
    .map((c) => ({ id: c.id, n: Number(c.id.replace('fb_', '')), selector: c.target?.selector, dfwId: c.target?.dfwId, status: c.status }));
}

function renderPins() {
  const wrap = $('#pins');
  wrap.innerHTML = '';
  for (const f of [...S.frames.values(), ...(P.open ? [P] : [])]) {
    for (const p of f.pinRects || []) {
      const c = S.comments.find((x) => x.id === p.id);
      if (!c) continue;
      const b = document.createElement('button');
      b.type = 'button';
      b.textContent = Number(c.id.replace('fb_', ''));
      b.title = `Comment #${b.textContent}: ${c.comment.slice(0, 80)}`;
      b.dataset.key = f.key;
      b.dataset.id = c.id;
      b.dataset.rect = JSON.stringify(p.rect);
      b.style.background = STATUS_COLOR[c.status] || '#78716c';
      b.onclick = () => {
        S.selectedComment = c.id;
        setTab('comments');
        renderComments();
        document.querySelector(`.comment[data-id="${c.id}"]`)?.scrollIntoView({ block: 'nearest' });
      };
      wrap.append(b);
    }
  }
  updateHud();
}

function syncPins() {
  for (const f of S.frames.values()) if (f.loaded) post(f, { type: 'dfw:pins', pins: pinsFor(f) });
  if (P.open && P.loaded) post(P, { type: 'dfw:pins', pins: pinsFor(P) });
}

/* ---------------------------------------------------------------- pages & layers */

function renderPages() {
  const list = $('#page-list');
  list.innerHTML = '';
  const all = document.createElement('li');
  const total = S.pages.reduce((n, p) => n + (p.system ? 0 : frameCount(p)), 0);
  all.innerHTML = `<button type="button" data-page="${ALL}" class="all" ${S.page === ALL ? 'aria-current="page"' : ''}><span>All pages</span><span class="n">${total}</span></button>`;
  $('button', all).onclick = () => setPage(ALL, { fit: S.page === ALL });
  list.append(all);
  for (const p of S.pages) {
    const n = p.system ? '' : frameCount(p);
    const open = S.comments.filter((c) => ACTIVE.includes(c.status) && (p.system ? c.screen === 'system' : p.screens.includes(c.screen))).length;
    const li = document.createElement('li');
    const inAll = S.page === ALL;
    li.innerHTML = `<button type="button" data-page="${esc(p.id)}" ${p.id === S.page ? 'aria-current="page"' : ''} title="${inAll ? 'Click: go to this section · Double-click: open the page alone' : ''}"><span>${esc(p.title)}</span>${open ? `<span class="c" title="Open comments">${open}</span>` : ''}<span class="n">${n}</span></button>`;
    const b = $('button', li);
    b.onclick = () => (S.page === ALL ? zoomToSection(p.id) : setPage(p.id));
    b.ondblclick = () => setPage(p.id);
    list.append(li);
  }
}

function zoomToSection(id) {
  const b = S.secBounds?.[id];
  if (!b) return;
  const [vw] = vpSize();
  const z = Math.min(1, Math.max(0.05, (vw - 96) / b[2]));
  S.cam = { z, x: 48 - b[0] * z, y: 48 - b[1] * z };
  applyCam();
  for (const x of $$('#page-list button')) x.classList.toggle('focus', x.dataset.page === id);
}

function setPage(id, { fit = false } = {}) {
  if (id !== ALL && !S.pages.some((p) => p.id === id)) id = S.pages.find((p) => !p.system && p.screens.length)?.id || 'system';
  S.cams[S.page] = { ...S.cam };
  S.page = id;
  store.set('page', id);
  S.sel = null;
  S.selKey = null;
  buildPage();
  if (S.cams[id] && !fit) {
    S.cam = { ...S.cams[id] };
    applyCam();
  } else zoomToFit({ initial: true });
}

function selectFrame(key, { keepSel = false } = {}) {
  if (S.selKey !== key) S.layers = null;
  S.selKey = key;
  if (!keepSel && S.sel) {
    postAll({ type: 'dfw:clear-selection' });
    S.sel = null;
  }
  for (const f of S.frames.values()) f.el.classList.toggle('selected', f.key === key);
  const f = S.frames.get(key);
  if (f?.loaded && (!S.layers || S.layers.key !== key)) post(f, { type: 'dfw:layers' });
  renderLayers();
  renderInspector();
  updateHud();
}

function clearSelection() {
  postAll({ type: 'dfw:clear-selection' });
  S.sel = null;
  S.selKey = null;
  for (const f of S.frames.values()) f.el.classList.remove('selected');
  renderLayers();
  renderInspector();
  updateHud();
}

function renderLayers() {
  const box = $('#layers');
  const f = S.frames.get(S.selKey);
  $('#layers-for').textContent = f ? f.title : '';
  if (!f) return (box.innerHTML = '<p class="hint muted">Select a frame or an element to see its layers.</p>');
  if (!S.layers || S.layers.key !== f.key) return (box.innerHTML = '<p class="hint muted">Loading layers…</p>');
  const selI = S.sel?.key === f.key ? S.sel.layer : null;
  // Open the ancestors of the selected layer.
  for (let p = S.layers.parents.get(selI); p != null; p = S.layers.parents.get(p)) S.layerOpen.set(`${f.key}:${p}`, true);
  const rows = [];
  const walk = (nodes, depth) => {
    for (const n of nodes) {
      const k = `${f.key}:${n.i}`;
      const open = S.layerOpen.has(k) ? S.layerOpen.get(k) : depth < 2;
      const name = n.comp ? n.id || n.comp : n.id || (n.name ? `${n.tag} “${n.name}”` : n.tag);
      const sub = n.comp ? `${n.comp}${n.variant ? ` · ${n.variant}` : ''}` : n.id ? n.tag : '';
      const icon = n.comp ? '◆' : ['header', 'nav', 'main', 'aside', 'footer', 'section', 'form', 'dialog'].includes(n.tag) ? '▭' : '#';
      rows.push(
        `<div class="row${n.comp ? ' comp' : ''}${n.hidden ? ' hidden-layer' : ''}" role="treeitem" data-i="${n.i}" aria-selected="${n.i === selI}" ${n.children.length ? `aria-expanded="${open}"` : ''} style="padding-left:${4 + depth * 12}px">` +
          `<button type="button" class="tw" tabindex="-1" ${n.children.length ? '' : 'style="visibility:hidden"'}><svg viewBox="0 0 8 8" width="8" height="8" aria-hidden="true" style="transform:rotate(${open ? 90 : 0}deg)"><path d="M2 1l4 3-4 3z" fill="currentColor"/></svg></button>` +
          `<span class="ic">${icon}</span><span class="nm">${esc(name)}</span>${sub ? `<span class="sub">${esc(sub)}</span>` : ''}</div>`,
      );
      if (open && n.children.length) walk(n.children, depth + 1);
    }
  };
  walk(S.layers.tree, 0);
  box.innerHTML = rows.join('') || '<p class="hint muted">No layers (add data-dfw-id to elements).</p>';
  box.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
}

$('#layers').addEventListener('click', (e) => {
  const row = e.target.closest('.row');
  if (!row) return;
  const f = S.frames.get(S.selKey);
  const i = Number(row.dataset.i);
  if (e.target.closest('.tw')) {
    const k = `${f.key}:${i}`;
    S.layerOpen.set(k, row.getAttribute('aria-expanded') !== 'true');
    return renderLayers();
  }
  post(f, { type: 'dfw:layer-select', i });
});
$('#layers').addEventListener('mouseover', (e) => {
  const row = e.target.closest('.row');
  const f = S.frames.get(S.selKey);
  if (row && f) post(f, { type: 'dfw:layer-hover', i: Number(row.dataset.i) });
});
$('#layers').addEventListener('mouseleave', () => {
  S.hover = null;
  updateHud();
});

/* ---------------------------------------------------------------- inspector */

const COLORISH = new Set(['color', 'background-color', 'border-color']);
const TYPE = new Set(['font-size', 'font-weight', 'line-height']);
const SPACING = new Set(['padding', 'gap', 'margin']);
const spaceTokens = () =>
  S.tokens
    .filter((t) => t.name.startsWith('space.'))
    .sort((a, b) => parseFloat(a.name.split('.')[1]) - parseFloat(b.name.split('.')[1]) || a.name.localeCompare(b.name));

function frameInfoHtml(f) {
  if (f.system) return `<div class="isec"><div class="ihead">Design System</div><p class="note">Generated from design/system/ and the tokens. Edit masters in <code>design/system/components/</code>.</p></div>`;
  const s = f.screen;
  return `<div class="isec"><div class="ihead">${esc(s.title || s.id)}</div>${s.description ? `<p class="note">${esc(s.description)}</p>` : ''}</div>
  <div class="isec"><h3>Frame</h3><dl class="kv">
    <dt>Screen</dt><dd>${esc(s.id)}</dd>
    <dt>Variant</dt><dd>${esc(f.variant.label || f.variant.id)}</dd>
    <dt>Breakpoint</dt><dd>${esc(f.bp.label || f.bp.id)} · ${f.bp.width}×${f.bp.height}</dd>
    <dt>State</dt><dd>${esc(f.state || 'default')}</dd>
    <dt>File</dt><dd><code>design/${esc(f.file)}</code></dd>
  </dl><div class="btn-row"><a class="btn" href="${esc(f.src)}" target="_blank" rel="noopener" style="display:inline-flex;align-items:center;text-decoration:none;color:inherit">Open in new tab ↗</a></div></div>`;
}

function renderInspector() {
  const box = $('#inspector');
  const f = S.frames.get(S.sel?.key || S.selKey);
  if (!f) {
    const p = currentPage();
    box.innerHTML = `<div class="isec"><div class="ihead">${esc(p?.title || '')}</div><p class="note">Click a frame title to select a frame, click inside a frame to select an element. Double-click text to edit it. Press <kbd>?</kbd> for shortcuts.</p></div>`;
    return;
  }
  if (!S.sel || S.sel.key !== f.key) return (box.innerHTML = frameInfoHtml(f));
  const sel = S.sel;
  const own = sel.own || {};
  const editable = !f.system && !!f.file;
  const parts = [];
  parts.push(`<div class="isec"><div class="ihead${own.comp ? ' comp' : ''}">${own.comp ? '◆' : '#'} ${esc(sel.label)}</div><p class="note">${esc(f.title)}${f.state ? ` · ${esc(f.state)}` : ''}</p></div>`);
  parts.push(`<div class="isec"><h3>Element</h3><dl class="kv">
    <dt>dfw-id</dt><dd>${own.id ? `<code>${esc(own.id)}</code>` : `<span class="muted">none${sel.target.dfwId ? ` (inside ${esc(sel.target.dfwId)})` : ''}</span>`}</dd>
    <dt>Tag</dt><dd><code>&lt;${esc(sel.target.tag)}&gt;</code></dd>
    <dt>Size</dt><dd>${Math.round(sel.rect[2])} × ${Math.round(sel.rect[3])}</dd>
    ${sel.target.a11y?.role ? `<dt>Role</dt><dd>${esc(sel.target.a11y.role)}${sel.target.a11y.name ? ` “${esc(sel.target.a11y.name.slice(0, 60))}”` : ''}</dd>` : ''}
  </dl>${sel.textEditable && editable ? '<div class="btn-row"><button type="button" class="btn" data-act="edit-text">Edit text</button><span class="note" style="margin:0;align-self:center">or double-click it</span></div>' : ''}</div>`);
  if (own.comp) {
    const def = S.components.get(own.comp);
    const can = editable && own.id && def;
    const opt = (list, cur) => list.map((x) => `<option ${x === cur ? 'selected' : ''}>${esc(x)}</option>`).join('');
    parts.push(`<div class="isec"><h3>Component <span class="sp"></span><button type="button" class="btn small comp" data-act="master">Go to master</button></h3><dl class="kv">
      <dt>Component</dt><dd>${esc(own.comp)}</dd>
      ${def?.variants?.length ? `<dt>Variant</dt><dd>${can ? `<select data-edit="variant">${opt(def.variants, own.variant)}</select>` : esc(own.variant)}</dd>` : ''}
      ${def?.sizes?.length ? `<dt>Size</dt><dd>${can ? `<select data-edit="size">${opt(def.sizes, own.size)}</select>` : esc(own.size)}</dd>` : ''}
      ${(def?.props || []).map((p) => `<dt>${esc(p)}</dt><dd>${can ? `<input data-edit="prop" data-prop="${esc(p)}" placeholder="(empty)">` : ''}</dd>`).join('')}
    </dl>${!own.id && editable ? '<p class="note">Give the instance an id to edit it here.</p>' : ''}${def?.description ? `<p class="note">${esc(def.description)}</p>` : ''}</div>`);
  }
  const rows = sel.inspect || [];
  const canSpace = editable && own.id && !own.comp;
  const st = spaceTokens();
  const tokRows = rows
    .map((r, i) => {
      const has = r.tokens.length > 0;
      const sw = COLORISH.has(r.group) ? `<span class="sw" style="background:${esc(r.value)}"></span>` : '';
      let v;
      if (SPACING.has(r.group) && canSpace && st.length) {
        const cur = has ? r.tokens.find((t) => t.startsWith('space.')) : null;
        v = `<select data-space="${i}" title="Set ${esc(r.prop)} to a space token (writes var(--space-*) into the element's inline style)">${cur ? '' : `<option value="" selected>raw ${esc(r.value)}</option>`}${st.map((t) => `<option value="${esc(t.name)}" ${t.name === cur ? 'selected' : ''}>${esc(t.name)}</option>`).join('')}</select>${has ? '' : '<span class="none">raw value (no token)</span>'}`;
      } else {
        v = has ? `<span class="name" title="${esc(r.tokens.join(', '))}">${sw}${esc(r.tokens[0])}${r.tokens.length > 1 ? ` <span class="muted">+${r.tokens.length - 1}</span>` : ''}</span>` : `<span class="name">${sw}<span class="none">raw value (no token)</span></span>`;
      }
      const ask = COLORISH.has(r.group) || TYPE.has(r.group) || r.group === 'box-shadow' || r.group === 'border-radius' ? `<button type="button" class="ask" data-ask="${i}" title="Design-system decision: ask Claude to change it">Ask Claude</button>` : '';
      return `<div class="tok"><span class="p">${esc(r.prop)}</span><span class="v">${v}<span class="raw" title="${esc(r.value)}">${esc(r.value)}</span></span>${ask || '<span></span>'}</div>`;
    })
    .join('');
  parts.push(`<div class="isec"><h3>Design tokens in use</h3>${tokRows || '<p class="note">No styles to report.</p>'}${canSpace ? '<p class="note">Spacing is editable here; colours and typography are design-system decisions.</p>' : ''}</div>`);
  parts.push(`<div class="isec"><div class="btn-row" style="margin:0"><button type="button" class="btn" data-act="comment">Comment on this</button></div></div>`);
  box.innerHTML = parts.join('');
}

$('#inspector').addEventListener('click', (e) => {
  const b = e.target.closest('button');
  if (!b) return;
  const sel = S.sel;
  const f = S.frames.get(sel?.key);
  if (b.dataset.act === 'master') return goToMaster(sel.own.comp);
  if (b.dataset.act === 'edit-text') return post(f, { type: 'dfw:edit' });
  if (b.dataset.act === 'comment') return openComposer(sel, f, '');
  if (b.dataset.ask != null) {
    const r = sel.inspect[Number(b.dataset.ask)];
    const what = r.tokens.length ? `currently ${r.tokens[0]} (${r.value})` : `currently ${r.value}, raw value with no token`;
    return openComposer(sel, f, `Change ${r.prop} of ${sel.own.id || sel.target.dfwId || sel.target.tag} (${what}) to `);
  }
});
$('#inspector').addEventListener('change', async (e) => {
  const t = e.target;
  const sel = S.sel;
  const f = S.frames.get(sel?.key);
  if (!f) return;
  if (t.dataset.space != null) {
    const r = sel.inspect[Number(t.dataset.space)];
    return edit(f, { op: 'spacing', id: sel.own.id, prop: r.prop, value: t.value || null });
  }
  if (t.dataset.edit === 'variant' || t.dataset.edit === 'size') return edit(f, { op: t.dataset.edit, id: sel.own.id, value: t.value });
  if (t.dataset.edit === 'prop') return edit(f, { op: 'prop', id: sel.own.id, prop: t.dataset.prop, value: t.value });
});

async function edit(f, op) {
  try {
    const r = await api('/__dfw/api/edit', { method: 'POST', body: JSON.stringify({ file: f.file, ...op }) });
    toast(r.changed ? `Saved to ${r.file}:${r.line}  (⌘Z to undo)` : 'No change.');
  } catch (e) {
    toast(e.message);
    reloadFrame(f);
  }
}

async function undo() {
  try {
    const r = await api('/__dfw/api/undo', { method: 'POST', body: '{}' });
    toast(`Undid ${r.op.op} edit in ${r.file}.`);
  } catch (e) {
    toast(e.message);
  }
}

function goToMaster(name) {
  S.zoomOnSelect = true;
  setPage('system');
  selectFrame('system');
  const f = S.frames.get('system');
  f.pendingSelect = { dfwId: `system.component.${name}`, required: true };
  if (f.loaded) {
    post(f, { type: 'dfw:select-id', ...f.pendingSelect });
    f.pendingSelect = null;
  }
}

/* ---------------------------------------------------------------- frame messages */

window.addEventListener('message', (e) => {
  const f = S.byWin.get(e.source);
  if (!f) return;
  const d = e.data || {};
  switch (d.type) {
    case 'dfw:ready':
      if (f.present) {
        P.loaded = true;
        post(P, { type: 'dfw:mode', mode: P.mode });
        post(P, { type: 'dfw:tokens', vars: S.tokens });
        post(P, { type: 'dfw:pins', pins: pinsFor(P) });
        renderPresentBar();
        break;
      }
      f.loaded = true;
      f.sizeCount = 0;
      post(f, { type: 'dfw:mode', mode: S.mode });
      post(f, { type: 'dfw:tokens', vars: S.tokens });
      post(f, { type: 'dfw:pins', pins: pinsFor(f) });
      if (f.pendingSelect) {
        post(f, { type: 'dfw:select-id', ...f.pendingSelect });
        f.pendingSelect = null;
      } else if (S.sel?.key === f.key) post(f, { type: 'dfw:select-id', dfwId: S.sel.own?.id || S.sel.target?.dfwId, selector: S.sel.target?.selector });
      if (S.selKey === f.key) post(f, { type: 'dfw:layers' });
      break;
    case 'dfw:size':
      if (f.auto && d.h && f.sizeCount++ < 8) {
        const h = Math.min(20000, Math.max(f.system ? 900 : f.bp.height, Math.ceil(d.h)));
        if (Math.abs(h - f.h) > 1) {
          f.h = h;
          positionFrames();
          updateHud();
        }
      }
      break;
    case 'dfw:hover':
      S.hover = d.rect ? { key: f.key, rect: d.rect, label: d.label, comp: d.comp } : null;
      updateHud();
      break;
    case 'dfw:select':
      if (!d.target) break;
      if (f.present) {
        if (d.comment) {
          S.sel = { key: f.key, ...d };
          updateHud();
          openComposer(S.sel, f, '');
        }
        break;
      }
      if (d.comment && S.mode === 'comment') {
        S.sel = { key: f.key, ...d };
        updateHud();
        openComposer(S.sel, f, '');
        break;
      }
      S.sel = { key: f.key, ...d };
      if (S.selKey !== f.key) selectFrame(f.key, { keepSel: true });
      if (S.layers?.key !== f.key) post(f, { type: 'dfw:layers' });
      renderLayers();
      renderInspector();
      setTab('inspect', { quiet: true });
      updateHud();
      if (S.zoomOnSelect) {
        S.zoomOnSelect = false;
        zoomToSelection();
      }
      break;
    case 'dfw:sel-rect':
      if (S.sel?.key === f.key) {
        S.sel.rect = d.rect;
        updateHud();
      }
      break;
    case 'dfw:layers': {
      const parents = new Map();
      const walk = (nodes, p) => nodes.forEach((n) => (parents.set(n.i, p), walk(n.children, n.i)));
      walk(d.tree, null);
      S.layers = { key: f.key, tree: d.tree, parents };
      if (S.sel?.key === f.key && d.selected != null) S.sel.layer = d.selected;
      renderLayers();
      break;
    }
    case 'dfw:pin-rects':
      f.pinRects = d.pins;
      renderPins();
      break;
    case 'dfw:wheel':
      if (f.present) break;
      onWheel({ dx: d.dx, dy: d.dy, dm: d.dm, ctrl: d.ctrl, shift: d.shift, x: (f.x + d.x) * S.cam.z + S.cam.x, y: (f.y + d.y) * S.cam.z + S.cam.y });
      break;
    case 'dfw:pan':
      if (d.phase === 'start') startPan(d.sx, d.sy);
      else if (d.phase === 'move') movePan(d.sx, d.sy);
      else endPan();
      break;
    case 'dfw:key':
      handleKey(d);
      break;
    case 'dfw:goto':
      handleGoto(f, d);
      break;
    case 'dfw:edit-text':
      edit(f, { op: 'text', id: d.id, value: d.text });
      break;
    case 'dfw:toast':
      toast(d.message);
      break;
    case 'dfw:focus':
      if (!$('#composer').hidden && S.mode !== 'comment') closeComposer();
      break;
  }
});

function reloadFrame(f) {
  if (f?.present) return P.open && loadPresent();
  if (!f?.iframe) return;
  f.loaded = false;
  f.pinRects = [];
  try {
    f.iframe.contentWindow.location.reload();
  } catch {
    f.iframe.src = f.src;
  }
}

/* ---------------------------------------------------------------- presenter (full-window prototype) */

function screenForHref(href) {
  const path = decodeURIComponent(String(href).split('?')[0]).replace(/^\/design\//, '');
  for (const s of S.manifest.screens || []) for (const v of s.variants || []) if (v.file === path) return { s, v };
  return null;
}

function resolveGoto(d) {
  if (d.href) return screenForHref(d.href);
  const [sid, vid] = String(d.goto || '').split('/');
  const s = screenById(sid);
  if (!s) return null;
  return { s, v: (vid && (s.variants || []).find((x) => x.id === vid)) || primaryVariant(s) };
}

/** Frames of the current canvas in layout order (prev/next in the presenter). */
const presentList = () => [...S.frames.values()];

function openPresent(f) {
  f = f || S.frames.get(S.sel?.key || S.selKey) || presentList()[0];
  if (!f) return toast('Nothing to present on this page.');
  P.open = true;
  P.mode = 'present';
  P.system = !!f.system;
  P.screen = f.screen;
  P.variant = f.variant;
  P.bp = f.bp || S.manifest.breakpoints[0];
  P.state = f.state;
  P.file = f.file || null;
  P.key0 = f.key;
  closeComposer();
  $('#present').hidden = false;
  document.body.classList.add('presenting');
  loadPresent();
}

function closePresent() {
  if (!P.open) return;
  closeComposer();
  P.open = false;
  P.loaded = false;
  P.pinRects = [];
  $('#present').hidden = true;
  document.body.classList.remove('presenting');
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  if (S.hover?.key === PKEY) S.hover = null;
  if (S.sel?.key === PKEY) S.sel = null;
  renderPins();
}

function presentSrc() {
  if (P.system) return '/__dfw/system.html';
  return frameSrc(P.file, P.state);
}

function loadPresent() {
  P.loaded = false;
  P.pinRects = [];
  const old = P.iframe;
  const iframe = document.createElement('iframe');
  iframe.id = 'p-frame';
  iframe.name = PKEY;
  iframe.title = 'Prototype';
  iframe.src = presentSrc();
  if (old) {
    S.byWin.delete(old.contentWindow);
    old.replaceWith(iframe);
  } else $('#p-wrap').append(iframe);
  P.iframe = iframe;
  S.byWin.set(iframe.contentWindow, P);
  layoutPresent();
  renderPresentBar();
  renderPins();
}

function layoutPresent() {
  if (!P.open) return;
  const stage = $('#p-stage');
  const sw = stage.clientWidth;
  const sh = stage.clientHeight;
  const w = P.system ? 1440 : P.bp.width;
  P.scale = P.fit ? Math.min(1, (sw - 48) / w) : 1;
  const wrap = $('#p-wrap');
  wrap.style.width = `${Math.round(w * P.scale)}px`;
  wrap.style.height = `${sh}px`;
  P.iframe.style.width = `${w}px`;
  P.iframe.style.height = `${Math.round(sh / P.scale)}px`;
  P.iframe.style.transform = P.scale === 1 ? '' : `scale(${P.scale})`;
  $('#p-fit').textContent = P.fit ? 'Fit width' : '100%';
  $('#p-fit').setAttribute('aria-pressed', String(P.fit));
  updateHud();
}

function presentIndex() {
  const list = presentList();
  const i = list.findIndex((f) => (f.system ? P.system : !P.system && f.screen === P.screen && f.variant === P.variant && f.bp?.id === P.bp?.id && (f.state || null) === (P.state || null)));
  return i >= 0 ? i : list.findIndex((f) => f.key === P.key0);
}

function renderPresentBar() {
  const list = presentList();
  const i = presentIndex();
  const s = P.screen;
  $('#p-title').textContent = P.system ? 'Design System' : s ? `${s.title || s.id}${(s.variants || []).length > 1 ? ` · ${P.variant?.label || P.variant?.id}` : ''}` : P.file || '';
  $('#p-count').textContent = i >= 0 ? `${i + 1} / ${list.length}` : `– / ${list.length}`;
  const bpSel = $('#p-bp');
  bpSel.innerHTML = S.manifest.breakpoints.map((b) => `<option value="${esc(b.id)}" ${b.id === P.bp?.id ? 'selected' : ''}>${esc(b.label || b.id)} ${b.width}</option>`).join('');
  bpSel.disabled = P.system;
  const states = s ? statesOf(s) : [];
  $('#p-state').innerHTML = `<option value="">default</option>` + states.map((x) => `<option ${x === P.state ? 'selected' : ''}>${esc(x)}</option>`).join('');
  $('#p-state').disabled = !states.length || P.system;
  const themes = S.manifest.themes || [];
  $('#p-theme-wrap').hidden = !themes.length;
  $('#p-theme').innerHTML = `<option value="">default</option>` + themes.map((t) => `<option ${t === S.theme ? 'selected' : ''}>${esc(t)}</option>`).join('');
  $('#p-open').href = presentSrc();
  $('#p-comment').setAttribute('aria-pressed', String(P.mode === 'comment'));
}

function presentStep(dir) {
  const list = presentList();
  if (!list.length) return;
  const i = presentIndex();
  const f = list[(Math.max(0, i) + dir + list.length) % list.length];
  P.system = !!f.system;
  P.screen = f.screen;
  P.variant = f.variant;
  P.bp = f.bp || P.bp;
  P.state = f.state;
  P.file = f.file || null;
  P.key0 = f.key;
  loadPresent();
}

function setPresentMode(mode) {
  P.mode = mode;
  if (mode !== 'comment') closeComposer();
  if (P.loaded) post(P, { type: 'dfw:mode', mode });
  renderPresentBar();
}

/** Prototype navigation from a frame (link to another design file or data-dfw-goto). */
function handleGoto(f, d) {
  const t = resolveGoto(d);
  if (f.present) {
    if (!t) {
      if (d.href) {
        P.screen = null;
        P.variant = null;
        P.file = decodeURIComponent(d.href.split('?')[0]).replace(/^\/design\//, '');
        P.state = null;
        P.system = false;
        return loadPresent();
      }
      return toast(`No screen "${d.goto}".`);
    }
    P.system = false;
    P.screen = t.s;
    P.variant = t.v;
    P.file = t.v.file;
    P.state = null;
    return loadPresent();
  }
  // On the canvas: jump to the target screen's frame (same breakpoint when possible).
  if (!t) return toast(`Link target is not a screen in design.json: ${d.href || d.goto}`);
  const pick = () => {
    const cands = [...S.frames.values()].filter((x) => x.screen === t.s && !x.state);
    return cands.find((x) => x.variant === t.v && x.bp?.id === f.bp?.id) || cands.find((x) => x.variant === t.v) || cands[0];
  };
  let target = pick();
  if (!target && S.page !== ALL) {
    const page = S.pages.find((p) => p.screens.includes(t.s.id));
    if (page) {
      setPage(page.id);
      target = pick();
    }
  }
  if (!target) return toast(`${t.s.title || t.s.id} is not shown with the current breakpoints.`);
  selectFrame(target.key);
  zoomToRect([target.x, target.y, target.w, target.h], { max: S.cam.z > 0.5 ? S.cam.z : 0.5 });
  target.el.classList.remove('flash');
  void target.el.offsetWidth;
  target.el.classList.add('flash');
}

function toggleFullscreen() {
  if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
  else document.documentElement.requestFullscreen?.().catch(() => toast('Fullscreen is not available here.'));
}

$('#p-close').onclick = closePresent;
$('#p-prev').onclick = () => presentStep(-1);
$('#p-next').onclick = () => presentStep(1);
$('#p-fit').onclick = () => {
  P.fit = !P.fit;
  store.set('pfit', P.fit);
  layoutPresent();
};
$('#p-full').onclick = toggleFullscreen;
$('#p-comment').onclick = () => setPresentMode(P.mode === 'comment' ? 'present' : 'comment');
$('#p-bp').onchange = (e) => {
  P.bp = S.manifest.breakpoints.find((b) => b.id === e.target.value) || P.bp;
  loadPresent();
};
$('#p-state').onchange = (e) => {
  P.state = e.target.value || null;
  loadPresent();
};
$('#p-theme').onchange = (e) => {
  S.theme = e.target.value;
  store.set('theme', S.theme);
  loadPresent();
};
$('#present-btn').onclick = () => openPresent();
addEventListener('resize', () => {
  vpRect = null;
  layoutPresent();
  updateHud();
});
$('#p-stage').addEventListener('scroll', updateHud);

/* ---------------------------------------------------------------- input: wheel, pan, keys */

function onWheel({ dx, dy, dm, ctrl, shift, x, y }) {
  const k = dm === 1 ? 16 : dm === 2 ? 400 : 1;
  if (ctrl) {
    const d = Math.max(-30, Math.min(30, dy * k));
    zoomAt(x, y, S.cam.z * Math.pow(2, -d * 0.012));
  } else {
    if (shift && !dx) [dx, dy] = [dy, 0];
    S.cam.x -= dx * k;
    S.cam.y -= dy * k;
    applyCam();
  }
}

viewport.addEventListener(
  'wheel',
  (e) => {
    e.preventDefault();
    const r = viewport.getBoundingClientRect();
    onWheel({ dx: e.deltaX, dy: e.deltaY, dm: e.deltaMode, ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey, x: e.clientX - r.left, y: e.clientY - r.top });
  },
  { passive: false },
);
// Safari pinch
let gestureZ = 1;
viewport.addEventListener('gesturestart', (e) => {
  e.preventDefault();
  gestureZ = S.cam.z;
});
viewport.addEventListener('gesturechange', (e) => {
  e.preventDefault();
  const r = viewport.getBoundingClientRect();
  zoomAt(e.clientX - r.left, e.clientY - r.top, gestureZ * e.scale);
});

function startPan(sx, sy) {
  S.panning = { sx, sy, x: S.cam.x, y: S.cam.y };
  document.body.classList.add('panning');
}
function movePan(sx, sy) {
  if (!S.panning) return;
  S.cam.x = S.panning.x + (sx - S.panning.sx);
  S.cam.y = S.panning.y + (sy - S.panning.sy);
  applyCam();
}
function endPan() {
  S.panning = null;
  document.body.classList.remove('panning');
}

viewport.addEventListener('mousedown', (e) => {
  if (e.button === 1 || S.mode === 'hand' || S.space) {
    e.preventDefault();
    startPan(e.screenX, e.screenY);
    return;
  }
  if (e.button === 0 && (e.target === viewport || e.target === world)) {
    closeComposer();
    clearSelection();
  }
});
window.addEventListener('mousemove', (e) => S.panning && movePan(e.screenX, e.screenY));
window.addEventListener('mouseup', () => S.panning && endPan());

function setSpace(on) {
  if (S.space === on) return;
  S.space = on;
  document.body.classList.toggle('space', on);
}

function handleKey(d) {
  const key = d.key;
  if (d.phase === 'up') {
    if (key === ' ') setSpace(false);
    return;
  }
  const mod = d.meta || d.ctrl;
  if (!$('#shortcuts').hidden) {
    if (key === 'Escape' || key === '?') $('#shortcuts').hidden = true;
    return;
  }
  if (P.open) {
    if (mod || d.alt) return;
    if (key === 'Escape') {
      if (!$('#composer').hidden) return closeComposer();
      if (P.mode === 'comment') return setPresentMode('present');
      return closePresent();
    }
    if (key === 'ArrowLeft') return presentStep(-1);
    if (key === 'ArrowRight') return presentStep(1);
    if (key === 'c' || key === 'C') return setPresentMode(P.mode === 'comment' ? 'present' : 'comment');
    if (key === 'f' || key === 'F') return toggleFullscreen();
    if (key === 'p' || key === 'P') return closePresent();
    if (key === '?') $('#shortcuts').hidden = false;
    return;
  }
  if (key === ' ') return setSpace(true);
  if (mod && (key === 'z' || key === 'Z') && !d.shift) return undo();
  if (mod && (key === '=' || key === '+')) return zoomStep(1);
  if (mod && key === '-') return zoomStep(-1);
  if (mod && key === '0') return zoomCenter(1);
  if (mod || d.alt) return;
  if (d.shift && (d.code === 'Digit1' || key === '!')) return zoomToFit();
  if (d.shift && (d.code === 'Digit2' || key === '@')) return zoomToSelection();
  switch (key) {
    case 'Escape':
      if (!$('#composer').hidden) return closeComposer();
      if (S.sel) {
        postAll({ type: 'dfw:clear-selection' });
        S.sel = null;
        renderLayers();
        renderInspector();
        return updateHud();
      }
      if (S.selKey) return clearSelection();
      if (S.mode !== 'select') setMode('select');
      return;
    case 'v':
    case 'V':
      return setMode('select');
    case 'h':
    case 'H':
      return setMode('hand');
    case 'i':
    case 'I':
      return setMode('interact');
    case 'p':
    case 'P':
      return openPresent();
    case 'f':
    case 'F':
      return toggleFullscreen();
    case 'c':
    case 'C':
      return setMode(S.mode === 'comment' ? 'select' : 'comment');
    case '+':
    case '=':
      return zoomStep(1);
    case '-':
    case '_':
      return zoomStep(-1);
    case '0':
      return zoomCenter(1);
    case '?':
      $('#shortcuts').hidden = false;
      return;
    case 'ArrowUp':
    case 'ArrowDown':
    case 'ArrowLeft':
    case 'ArrowRight':
      if (S.sel) post(S.frames.get(S.sel.key), { type: 'dfw:nav', dir: key.slice(5).toLowerCase() });
      return;
    case 'Enter':
      if (S.sel?.textEditable) post(S.frames.get(S.sel.key), { type: 'dfw:edit' });
  }
}

document.addEventListener('keydown', (e) => {
  if (e.target.closest('textarea, input, select')) {
    if (e.target.id === 'composer-text' && e.key === 'Enter' && (e.metaKey || e.ctrlKey)) saveComment();
    if (e.key === 'Escape') {
      closeComposer();
      e.target.blur();
    }
    return;
  }
  const d = { phase: 'down', key: e.key, code: e.code, meta: e.metaKey, ctrl: e.ctrlKey, shift: e.shiftKey, alt: e.altKey };
  if (e.key === ' ' || e.key.startsWith('Arrow') || ((e.metaKey || e.ctrlKey) && ['z', '=', '-', '0', '+'].includes(e.key))) e.preventDefault();
  handleKey(d);
});
document.addEventListener('keyup', (e) => {
  if (e.key !== ' ') return;
  // Space is the hand tool: never let it activate a focused toolbar button.
  if (!e.target.closest('textarea, input, select')) e.preventDefault();
  setSpace(false);
});
// Toolbar buttons should not keep keyboard focus after a click (Space/Enter belong to the canvas).
document.querySelector('.topbar').addEventListener('mousedown', (e) => e.target.closest('button') && e.preventDefault());
window.addEventListener('blur', () => setSpace(false));

function setMode(mode) {
  S.mode = mode;
  document.body.dataset.mode = mode;
  for (const b of $$('.seg button')) b.setAttribute('aria-checked', String(b.dataset.mode === mode));
  if (mode !== 'comment') closeComposer();
  S.hover = null;
  postAll({ type: 'dfw:mode', mode });
  updateHud();
}

/* ---------------------------------------------------------------- comments */

function statusLabel(s) {
  return { open: 'Open · waiting for Claude', acknowledged: 'Claude is working', reopened: 'Reopened · waiting for Claude', fixed: 'Fixed · please verify', resolved: 'Resolved', dismissed: 'Dismissed' }[s] || s;
}

function renderComments() {
  const list = $('#comment-list');
  const f = S.filter;
  const items = S.comments
    .filter((c) => (f === 'active' ? ACTIVE.includes(c.status) : f === 'fixed' ? c.status === 'fixed' : f === 'closed' ? ['resolved', 'dismissed'].includes(c.status) : true))
    .sort((a, b) => b.id.localeCompare(a.id));
  $('#open-count').textContent = S.comments.filter((c) => ACTIVE.includes(c.status) || c.status === 'fixed').length;
  list.innerHTML = items.length ? '' : `<li class="muted" style="padding:12px">Nothing here.</li>`;
  for (const c of items) {
    const li = document.createElement('li');
    li.className = `comment${S.selectedComment === c.id ? ' selected' : ''}`;
    li.dataset.id = c.id;
    const where = [c.screen, c.variant, c.breakpoint, c.state].filter(Boolean).join(' / ');
    const target = c.target?.dfwId || c.target?.selector || '(area)';
    const last = c.thread?.slice(-2) || [];
    li.innerHTML = `
      <div class="comment-top"><span class="num">#${esc(c.id.replace('fb_', ''))}</span><span>${esc(where)}</span><span class="status" data-s="${esc(c.status)}">${statusLabel(c.status)}</span></div>
      <div class="comment-target">${esc(target)}${c.target?.source ? ` · ${esc(c.target.source)}` : ''}</div>
      <p class="comment-text">${esc(c.comment)}</p>
      ${last.length ? `<ul class="thread">${last.map((t) => `<li><span class="by">${esc(t.by)} · ${esc(new Date(t.at).toLocaleString())}</span>${esc(t.message)}</li>`).join('')}</ul>` : ''}
      <div class="comment-actions"></div>`;
    const actions = $('.comment-actions', li);
    const btn = (label, cls, fn) => {
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
    if (c.status === 'fixed') {
      btn('Looks right · resolve', 'ok', () => patch(c.id, { status: 'resolved', message: 'Verified in studio.' }));
      btn('Not yet · reopen', '', () => askReason(actions, (msg) => patch(c.id, { status: 'reopened', message: msg })));
    } else if (ACTIVE.includes(c.status)) {
      btn('Dismiss', '', () => askReason(actions, (msg) => patch(c.id, { status: 'dismissed', message: msg })));
    } else {
      btn('Reopen', '', () => askReason(actions, (msg) => patch(c.id, { status: 'reopened', message: msg })));
    }
    li.onclick = () => focusComment(c);
    list.append(li);
  }
}

function askReason(container, done) {
  if ($('textarea', container)) return;
  const ta = document.createElement('textarea');
  ta.rows = 2;
  ta.placeholder = 'Why? (sent to Claude)';
  const ok = document.createElement('button');
  ok.type = 'button';
  ok.textContent = 'Send';
  ok.className = 'ok';
  ok.onclick = (e) => {
    e.stopPropagation();
    if (ta.value.trim()) done(ta.value.trim());
  };
  ta.onclick = (e) => e.stopPropagation();
  container.append(ta, ok);
  ta.focus();
}

async function patch(id, body) {
  try {
    await api(`/__dfw/api/comments/${id}`, { method: 'PATCH', body: JSON.stringify(body) });
    await loadComments();
  } catch (e) {
    toast(e.message);
  }
}

function focusComment(c) {
  S.selectedComment = c.id;
  renderComments();
  const page = c.screen === 'system' ? S.pages.find((p) => p.system) : S.pages.find((p) => p.screens.includes(c.screen));
  if (!page) return toast('That screen is not on any page.');
  if (P.open) closePresent();
  if (S.page !== ALL && page.id !== S.page) setPage(page.id);
  const fs = [...S.frames.values()].filter((f) => (c.screen === 'system' ? f.system : !f.system && f.screen.id === c.screen && f.variant.id === c.variant));
  const f = fs.find((x) => x.bp?.id === c.breakpoint && (x.state || null) === (c.state || null)) || fs.find((x) => (x.state || null) === (c.state || null)) || fs[0];
  if (!f) return toast('That frame is not visible (check breakpoints and States).');
  selectFrame(f.key);
  zoomToRect([f.x, f.y, f.w, f.h], { max: 1 });
  f.el.classList.remove('flash');
  void f.el.offsetWidth;
  f.el.classList.add('flash');
  const sel = { dfwId: c.target?.dfwId, selector: c.target?.selector };
  if (f.loaded) post(f, { type: 'dfw:select-id', ...sel });
  else f.pendingSelect = sel;
}

function openComposer(sel, f, prefill) {
  const s = f.screen;
  S.pending = f.system
    ? { screen: 'system', variant: null, breakpoint: null, file: null, state: null, theme: S.theme || null, target: sel.target }
    : { screen: s?.id || null, variant: f.variant?.id || null, breakpoint: f.bp?.id || null, file: f.file, state: f.state || null, theme: S.theme || null, target: sel.target };
  const where = f.system ? 'Design System' : [s?.id || f.file, f.variant?.id, f.bp?.id, f.state].filter(Boolean).join(' / ');
  $('#composer-target').textContent = `${where} → ${sel.target.dfwId || sel.target.selector}`;
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

function closeComposer() {
  if ($('#composer').hidden) return;
  $('#composer').hidden = true;
  S.pending = null;
  if (S.sel?.key === PKEY) {
    post(P, { type: 'dfw:clear-selection' });
    S.sel = null;
    updateHud();
  } else if (S.mode === 'comment') {
    postAll({ type: 'dfw:clear-selection' });
    S.sel = null;
    updateHud();
  }
}

async function saveComment() {
  const text = $('#composer-text').value.trim();
  if (!text || !S.pending) return;
  try {
    const c = await api('/__dfw/api/comments', { method: 'POST', body: JSON.stringify({ ...S.pending, comment: text }) });
    closeComposer();
    S.selectedComment = c.id;
    S.filter = 'active';
    setFilterTabs();
    setTab('comments');
    await loadComments();
    toast(`Sent to Claude · comment #${c.id.replace('fb_', '')}`);
  } catch (e) {
    toast(e.message);
  }
}

function setFilterTabs() {
  for (const b of $$('.tabs button')) b.setAttribute('aria-selected', String(b.dataset.filter === S.filter));
}

function setTab(tab, { quiet = false } = {}) {
  if (quiet && tab === 'inspect' && !$('#tab-comments').hidden && S.mode === 'comment') return;
  for (const b of $$('.ptabs button')) b.setAttribute('aria-selected', String(b.dataset.tab === tab));
  $('#tab-inspect').hidden = tab !== 'inspect';
  $('#tab-comments').hidden = tab !== 'comments';
}

/* ---------------------------------------------------------------- data */

async function loadComments() {
  const prev = new Map(S.comments.map((c) => [c.id, c.status]));
  S.comments = await api('/__dfw/api/comments');
  // Live updates from Claude (via .dfw/reviews/ + SSE): announce status changes.
  if (prev.size) {
    for (const c of S.comments) {
      const was = prev.get(c.id);
      if (!was || was === c.status) continue;
      const n = c.id.replace('fb_', '');
      if (c.status === 'acknowledged') toast(`Claude is working on #${n}.`);
      else if (c.status === 'fixed') toast(`Claude fixed #${n}: please verify (Comments → To verify).`);
    }
  }
  renderComments();
  renderPages();
  syncPins();
}

function renderToolbar() {
  const m = S.manifest;
  $('#product').textContent = m.product || 'Design Studio';
  const st = S.state;
  $('#meta').textContent = [st ? `v${st.design.version} ${st.design.status}` : null, m.inferred ? 'manifest inferred' : null].filter(Boolean).join(' · ');
  const wrap = $('#breakpoints');
  wrap.innerHTML = '';
  for (const bp of m.breakpoints) {
    const b = document.createElement('button');
    b.type = 'button';
    b.textContent = `${bp.label || bp.id} ${bp.width}`;
    b.setAttribute('aria-pressed', String(S.activeBps.includes(bp.id)));
    b.onclick = () => {
      const on = S.activeBps.includes(bp.id);
      if (on && S.activeBps.length === 1) return;
      S.activeBps = on ? S.activeBps.filter((x) => x !== bp.id) : m.breakpoints.map((x) => x.id).filter((x) => x === bp.id || S.activeBps.includes(x));
      store.set('bps2', S.activeBps);
      renderToolbar();
      rebuildKeepCam();
    };
    wrap.append(b);
  }
  $('#states-toggle').setAttribute('aria-pressed', String(S.showStates));
  $('#full-toggle').setAttribute('aria-pressed', String(S.fullHeight));
  const themes = m.themes || [];
  $('#theme-wrap').hidden = !themes.length;
  $('#theme').innerHTML = `<option value="">default</option>` + themes.map((t) => `<option ${t === S.theme ? 'selected' : ''}>${esc(t)}</option>`).join('');
}

function rebuildKeepCam() {
  const selKey = S.selKey;
  buildPage();
  applyCam();
  if (selKey && S.frames.has(selKey)) selectFrame(selKey);
}

async function loadAll({ keepCam = false } = {}) {
  const [manifest, st, tokens, comps] = await Promise.all([api('/__dfw/api/manifest'), api('/__dfw/api/state'), api('/__dfw/api/tokens').catch(() => []), api('/__dfw/api/components').catch(() => [])]);
  S.manifest = manifest;
  S.pages = manifest.studioPages || [{ id: 'system', title: 'Design System', system: true, screens: [] }];
  S.state = st.state;
  S.tokens = tokens;
  S.components = new Map(comps.map((c) => [c.name, c]));
  const ids = manifest.breakpoints.map((b) => b.id);
  if (S.activeBps) S.activeBps = S.activeBps.filter((id) => ids.includes(id));
  if (!S.activeBps?.length) S.activeBps = ids.slice(0, 2);
  renderToolbar();
  if (keepCam && S.page) rebuildKeepCam();
  else setPage(store.get('page', null) || S.pages.find((p) => !p.system && p.screens.length)?.id || 'system');
  await loadComments();
}

/* ---------------------------------------------------------------- wiring */

for (const b of $$('.seg button')) b.onclick = () => setMode(b.dataset.mode);
for (const b of $$('.ptabs button')) b.onclick = () => setTab(b.dataset.tab);
$('#zoom-in').onclick = () => zoomStep(1);
$('#zoom-out').onclick = () => zoomStep(-1);
$('#zoom-label').onclick = () => zoomToFit();
$('#help').onclick = () => ($('#shortcuts').hidden = false);
$('#shortcuts-close').onclick = () => ($('#shortcuts').hidden = true);
$('#shortcuts').onclick = (e) => e.target.id === 'shortcuts' && ($('#shortcuts').hidden = true);
$('#states-toggle').onclick = () => {
  S.showStates = !S.showStates;
  store.set('states', S.showStates);
  renderToolbar();
  rebuildKeepCam();
};
$('#full-toggle').onclick = () => {
  S.fullHeight = !S.fullHeight;
  store.set('full', S.fullHeight);
  renderToolbar();
  rebuildKeepCam();
};
$('#theme').onchange = (e) => {
  S.theme = e.target.value;
  store.set('theme', S.theme);
  rebuildKeepCam();
};
$('#composer-cancel').onclick = closeComposer;
$('#composer-save').onclick = saveComment;
for (const b of $$('.tabs button')) {
  b.onclick = () => {
    S.filter = b.dataset.filter;
    setFilterTabs();
    renderComments();
  };
}
$('#new-session').onclick = async () => {
  await api('/__dfw/api/sessions', { method: 'POST' });
  toast('New review round started.');
  await loadComments();
};
new ResizeObserver(() => {
  vpRect = null;
  updateHud();
}).observe(viewport);

const es = new EventSource('/__dfw/events');
es.addEventListener('design', async (e) => {
  const { files = [] } = JSON.parse(e.data);
  if (files.some((f) => /(^|\/)design\.json$/.test(f))) return loadAll({ keepCam: true });
  if (files.some((f) => f.startsWith('tokens/'))) S.tokens = await api('/__dfw/api/tokens').catch(() => S.tokens);
  if (files.some((f) => f.startsWith('system/'))) S.components = new Map((await api('/__dfw/api/components').catch(() => [])).map((c) => [c.name, c]));
  const screenOnly = files.every((f) => f.endsWith('.html') && !f.startsWith('system/') && !f.startsWith('components/'));
  for (const f of S.frames.values()) {
    if (!f.iframe) continue;
    if (!screenOnly || (f.file && files.includes(f.file))) reloadFrame(f);
  }
  if (P.open && (!screenOnly || (P.file && files.includes(P.file)))) loadPresent();
});
es.addEventListener('comments', () => loadComments());

window.__dfwStudio = S; // for debugging and e2e tests
setMode('select');
loadAll().catch((e) => toast(e.message));
