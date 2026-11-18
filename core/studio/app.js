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
  activeBps: store.get('bps3', null), // null = all
  activeStates: store.get('states3', null), // null = all
  fullHeight: store.get('full2', true), // frames grow to their content height by default; the toggle crops to the viewport
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
  // Comment pins on the canvas: 'active' (open + to verify), 'all' (also closed) or 'off'.
  pins: store.get('pins', 'active'),
  pop: null, // comment id shown in the pin popover
};

const ACTIVE = ['open', 'acknowledged', 'reopened'];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);
const viewport = $('#viewport');
const world = $('#world');
const hud = $('#hud');
const ALL = '__all';
const PKEY = '__present';
// Presenter state: a pseudo-frame shown full-window.
const P = { present: true, key: PKEY, open: false, loaded: false, pins: store.get('ppins', false), fit: store.get('pfit', true), scale: 1, screen: null, variant: null, bp: null, state: null, file: null, system: false, pinRects: [] };

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

const L = { FLOW_GAP: 480, PAD: 96, HEAD_GAP: 72, ROW_GAP: 240, FRAME_GAP: 80, GUTTER_NOTES: 420, GUTTER: 300, GUTTER_GAP: 80, SEC_GAP: 480 };
const bpShort = (bp) => (bp ? bp.short || String(bp.label || bp.id).charAt(0).toUpperCase() : '');
const primaryVariant = (s) => (s.variants || []).find((v) => v.recommended) || (s.variants || [])[0];
const bpShown = (id) => !S.activeBps || S.activeBps.includes(id);
const activeBps = () => S.manifest.breakpoints.filter((b) => bpShown(b.id));
const statesOf = (s) => (s.states && s.states.length ? s.states : S.manifest.states || []);
const shownStates = (list) => (S.activeStates ? list.filter((x) => S.activeStates.includes(x)) : list);
const allStates = () => [...new Set([...(S.manifest.states || []), ...(S.manifest.screens || []).flatMap((s) => s.states || [])])];

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
  if (page.flow && primary) {
    // Rows of 4 at the first shown breakpoint, in link order (sources before targets).
    const specs = screens.map((s) => makeSpec(s, primaryVariant(s), primary, null));
    for (let i = 0; i < specs.length; i += 4) rows.push({ title: i ? null : `Flow at ${primary.label || primary.id} ${primary.width}`, sub: i ? null : `${(S.manifest.flows || []).length} link(s)`, frames: specs.slice(i, i + 4) });
    return { id: page.id, title: page.title, description: page.description || '', rows, flow: true };
  }
  if (page.sections && page.sections.length) {
    for (const sec of page.sections) {
      const kind = sec.kind || (sec.breakpoint ? 'breakpoint' : sec.states ? 'states' : 'custom');
      const bpDecl = sec.breakpoint && S.manifest.breakpoints.find((b) => b.id === sec.breakpoint);
      if (kind === 'breakpoint' && bpDecl && !bpShown(bpDecl.id)) continue;
      const bp = bpDecl || primary;
      if (!bp) continue;
      const list = sec.screens ? refs(sec.screens) : screens.map((s) => ({ s, v: null }));
      const frames = [];
      for (const { s, v } of list) {
        if (kind === 'variants') {
          const vs = v ? [v] : (s.variants || []).filter((x) => x !== primaryVariant(s));
          for (const x of vs) frames.push(makeSpec(s, x, bp, null));
        } else if (kind === 'states') {
          for (const st of shownStates(sec.states || statesOf(s))) frames.push(makeSpec(s, v || primaryVariant(s), bp, st));
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
    if (allStates().length) {
      const st = screens.flatMap((s) => shownStates(statesOf(s)).map((x) => makeSpec(s, primaryVariant(s), primary, x)));
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
  const pages = S.page === ALL ? S.pages.filter((p) => !p.flow) : [currentPage()].filter(Boolean);
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
        x += f.w + (sec.model.flow ? L.FLOW_GAP : L.FRAME_GAP);
        h = Math.max(h, f.h);
      }
      maxW = Math.max(maxW, x - (sec.model.flow ? L.FLOW_GAP : L.FRAME_GAP) - x0 - L.PAD);
      y += h;
    });
    const w = maxW + L.PAD * 2;
    const h = y + L.PAD - secY;
    sec.el.style.cssText = `left:${x0}px;top:${secY}px;width:${w}px;height:${h}px`;
    S.secBounds[sec.id] = [x0, secY, w, h];
    secY += h + L.SEC_GAP;
  }
  drawFlow();
}

/** Arrows between the frames of the Flow page (world coordinates, so they pan and zoom with the canvas). */
function drawFlow() {
  world.querySelector('svg.flow')?.remove();
  if (!S.sections.some((s) => s.model.flow)) return;
  const byScreen = new Map();
  for (const f of S.frames.values()) if (f.screen && !byScreen.has(f.screen.id)) byScreen.set(f.screen.id, f);
  const b = pageBounds();
  if (!b) return;
  const ns = 'http://www.w3.org/2000/svg';
  const svg = document.createElementNS(ns, 'svg');
  svg.setAttribute('class', 'flow');
  svg.setAttribute('width', b[0] + b[2]);
  svg.setAttribute('height', b[1] + b[3]);
  svg.innerHTML = '<defs><marker id="flow-arrow" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="8" markerHeight="8" orient="auto-start-reverse"><path d="M0 0L10 5L0 10z"/></marker></defs>';
  for (const e of S.manifest.flows || []) {
    const a = byScreen.get(e.from);
    const z = byScreen.get(e.to);
    if (!a || !z) continue;
    const forward = z.x > a.x || (z.x === a.x && z.y > a.y);
    // A link back along the same pair runs below the forward one instead of on top of it.
    const lane = (S.manifest.flows || []).some((x) => x.from === e.to && x.to === e.from) ? (forward ? -60 : 60) : 0;
    const x1 = forward ? a.x + a.w : a.x;
    const y1 = a.y + Math.min(a.h, 600) / 2 + lane;
    const x2 = forward ? z.x : z.x + z.w;
    const y2 = z.y + Math.min(z.h, 600) / 2 + lane;
    const bend = Math.max(120, Math.abs(x2 - x1) / 2) * (forward ? 1 : -1);
    const path = document.createElementNS(ns, 'path');
    path.setAttribute('d', `M${x1} ${y1} C${x1 + bend} ${y1} ${x2 - bend} ${y2} ${x2} ${y2}`);
    path.setAttribute('marker-end', 'url(#flow-arrow)');
    svg.append(path);
    if (e.label) {
      const t = document.createElementNS(ns, 'text');
      t.setAttribute('x', (x1 + x2) / 2);
      t.setAttribute('y', (y1 + y2) / 2 - 14);
      t.textContent = e.label;
      svg.append(t);
    }
  }
  world.prepend(svg);
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
  placePop();
}

/* ---------------------------------------------------------------- pin popover (read a comment in place) */

function openPop(id) {
  S.pop = id;
  renderPop();
  renderPins();
}

function closePop() {
  if (!S.pop) return;
  S.pop = null;
  $('#pin-pop').hidden = true;
  for (const b of $$('#pins button.on')) b.classList.remove('on');
}

function renderPop() {
  const c = S.comments.find((x) => x.id === S.pop);
  const pop = $('#pin-pop');
  if (!c) return closePop();
  const where = c.screen === 'system' ? 'Design System' : [c.screen, c.variant, c.breakpoint, c.state].filter(Boolean).join(' / ');
  const thread = (c.thread || []).slice(-3);
  pop.innerHTML = `
    <div class="pop-head"><span class="num">#${esc(c.id.replace('fb_', ''))}</span><span class="status" data-s="${esc(c.status)}">${statusLabel(c.status)}</span><button type="button" class="pop-x" aria-label="Close">✕</button></div>
    <p class="pop-text">${esc(c.comment)}</p>
    <p class="pop-where">${esc(where)} · <code>${esc(c.target?.dfwId || c.target?.selector || 'area')}</code></p>
    ${thread.length ? `<ul class="thread">${thread.map((t) => `<li><span class="by">${esc(t.by)} · ${esc(new Date(t.at).toLocaleString())}</span>${esc(t.message)}</li>`).join('')}</ul>` : ''}
    <div class="comment-actions"></div>`;
  $('.pop-x', pop).onclick = closePop;
  commentActions(c, $('.comment-actions', pop));
  pop.hidden = false;
  placePop();
}

function placePop() {
  const pop = $('#pin-pop');
  if (!S.pop || pop.hidden) return;
  const b = $(`#pins button[data-id="${S.pop}"]`);
  if (!b || b.hidden) return closePop();
  const r = b.getBoundingClientRect();
  const w = pop.offsetWidth;
  const h = pop.offsetHeight;
  let x = r.right + 8;
  if (x + w > innerWidth - 8) x = Math.max(8, r.left - w - 8);
  pop.style.left = `${x}px`;
  pop.style.top = `${Math.min(Math.max(48, r.top), innerHeight - h - 8)}px`;
}

/* ---------------------------------------------------------------- pins */

const STATUS_COLOR = { open: '#c2410c', acknowledged: '#c2410c', reopened: '#c2410c', fixed: '#a16207', resolved: '#15803d' };

const PIN_STATUSES = { active: ['open', 'acknowledged', 'reopened', 'fixed'], all: ['open', 'acknowledged', 'reopened', 'fixed', 'resolved'], off: [] };
/** Presenter: pins only when toggled on, or while commenting. Canvas: the Pins setting. */
const pinStatuses = (f) => (f?.present ? (P.pins || P.mode === 'comment' ? PIN_STATUSES.active : []) : PIN_STATUSES[S.pins] || PIN_STATUSES.active);

function pinsFor(f) {
  if (!f) return [];
  const show = pinStatuses(f);
  return S.comments
    .filter((c) => show.includes(c.status) && (f.system ? c.screen === 'system' : f.screen && c.screen === f.screen.id && c.variant === f.variant?.id && (c.state || null) === (f.state || null)))
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
      b.title = `Comment #${b.textContent} · ${statusLabel(c.status)}`;
      b.setAttribute('aria-label', `Comment ${b.textContent}: ${c.comment.slice(0, 80)}`);
      b.dataset.s = c.status;
      if (S.pop === c.id) b.classList.add('on');
      b.dataset.key = f.key;
      b.dataset.id = c.id;
      b.dataset.rect = JSON.stringify(p.rect);
      b.style.background = STATUS_COLOR[c.status] || '#78716c';
      b.onclick = (e) => {
        e.stopPropagation();
        if (S.pop === c.id) return closePop();
        S.selectedComment = c.id;
        if (!P.open) {
          setTab('comments');
          renderComments();
          document.querySelector(`.comment[data-id="${c.id}"]`)?.scrollIntoView({ block: 'nearest' });
        }
        openPop(c.id);
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
  if (id !== ALL && !S.pages.some((p) => p.id === id)) id = S.pages.find((p) => !p.system && !p.flow && p.screens.length)?.id || 'system';
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
  </dl>${sel.textEditable && editable ? '<div class="btn-row"><button type="button" class="btn" data-act="edit-text">Edit text</button><span class="note" style="margin:0;align-self:center">or double-click it</span></div>' : ''}${
    editable && own.id
      ? `<p class="note">Layout: drag to reorder among flex/grid siblings or into an empty flex/grid container (<kbd>Alt</kbd>+arrows); drag the handles to resize (<kbd>Alt</kbd><kbd>Shift</kbd>+arrows), snapped to grid columns or tokens${layoutScope(f).scope ? `. Resizes here apply at ${esc(layoutScope(f).scope)}` : ''}. Double-click a handle or press <kbd>Alt</kbd><kbd>Shift</kbd><kbd>0</kbd> to reset.</p><div class="btn-row"><button type="button" class="btn small" data-act="layout-reset" title="Remove this element's Studio layout rule${layoutScope(f).scope ? ` at ${esc(layoutScope(f).scope)}` : ''} (back to the screen's own layout)">Reset size to auto${layoutScope(f).scope ? ` (${esc(layoutScope(f).scope)})` : ''}</button></div>`
      : ''
  }</div>`);
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
  if (b.dataset.act === 'layout-reset') return post(f, { type: 'dfw:layout-key', key: 'reset' });
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

/**
 * Layout edit scope of a frame: resizing in the widest breakpoint writes the base rule; in a
 * narrower frame it writes inside @media (max-width: <frame width>px), so it applies there and below.
 */
function layoutScope(f) {
  const widest = Math.max(0, ...(S.manifest?.breakpoints || []).map((b) => b.width || 0));
  if (!f?.bp || f.bp.width >= widest) return { maxWidth: 0, scope: '' };
  return { maxWidth: f.bp.width, scope: `≤${f.bp.width}px` };
}

async function edit(f, op, label = '') {
  // A resize is checked in the frame once it reloads with the new source (a stronger screen rule can still win).
  const verify = op.op === 'layout' && op.prop && op.value != null ? { id: op.id, prop: op.prop } : null;
  f.pendingVerify = verify;
  try {
    const r = await api('/__dfw/api/edit', { method: 'POST', body: JSON.stringify({ file: f.file, ...op }) });
    if (!r.changed && f.pendingVerify === verify) f.pendingVerify = null;
    toast(r.changed ? `${label ? `${label}. ` : ''}Saved to ${r.file}:${r.line}  (⌘Z to undo)` : 'No change.');
  } catch (e) {
    f.pendingVerify = null;
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
      f.lastStep = 0;
      f.sizeStop = false;
      post(f, { type: 'dfw:mode', mode: S.mode });
      post(f, { type: 'dfw:tokens', vars: S.tokens });
      post(f, { type: 'dfw:pins', pins: pinsFor(f) });
      post(f, { type: 'dfw:layout', enabled: !f.system && !!f.file, scope: layoutScope(f).scope });
      if (f.pendingVerify) {
        post(f, { type: 'dfw:layout-verify', ...f.pendingVerify });
        f.pendingVerify = null;
      }
      if (f.pendingSelect) {
        post(f, { type: 'dfw:select-id', ...f.pendingSelect });
        f.pendingSelect = null;
      } else if (S.sel?.key === f.key) post(f, { type: 'dfw:select-id', dfwId: S.sel.own?.id || S.sel.target?.dfwId, selector: S.sel.target?.selector });
      if (S.selKey === f.key) post(f, { type: 'dfw:layers' });
      break;
    case 'dfw:size': {
      // Grow the frame to its content. Only real changes count toward the limit (fonts, images and the
      // ResizeObserver report many times). A section sized in vh grows with the frame by the same amount
      // every round: two equal growth steps in a row mean a vh feedback loop, so the frame stops there.
      if (!f.auto || !d.h || f.sizeStop) break;
      const h = Math.min(20000, Math.max(f.system ? 900 : f.bp.height, Math.ceil(d.h)));
      if (Math.abs(h - f.h) <= 1) break;
      const step = h - f.h;
      if (step > 0 && f.lastStep && Math.abs(step - f.lastStep) <= 2) {
        f.sizeStop = true;
        break;
      }
      f.lastStep = step > 0 ? step : 0;
      if (++f.sizeCount > 24) {
        f.sizeStop = true;
        break;
      }
      f.h = h;
      positionFrames();
      updateHud();
      break;
    }
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
    case 'dfw:layout-op': {
      if (f.present || f.system || !f.file) break;
      const { type, label, ...op } = d;
      // Resizes are breakpoint-aware; reordering changes the source order, so it applies at every width.
      if (op.op === 'layout') op.maxWidth = layoutScope(f).maxWidth;
      if (op.prop == null) delete op.prop; // reset: the whole rule at this scope
      edit(f, op, label + (op.op === 'layout' && layoutScope(f).scope ? ` (${layoutScope(f).scope})` : ''));
      break;
    }
    case 'dfw:toast':
      toast(d.message);
      break;
    case 'dfw:layout-conflict':
      S.layoutConflict = { key: f.key, ...d }; // for tests and the inspector note
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
  closePop();
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

/** Does canvas frame f show what the presenter shows now? (ids, not objects: a live reload rebuilds them) */
const presentMatches = (f) =>
  f.system ? P.system : !P.system && f.screen?.id === P.screen?.id && f.variant?.id === P.variant?.id && f.bp?.id === P.bp?.id && (f.state || null) === (P.state || null);

function presentIndex() {
  const list = presentList();
  // The same screen can sit on several rows (sections, All pages): the key decides, attributes only
  // when the user switched breakpoint/state in the presenter bar.
  const k = list.findIndex((f) => f.key === P.key0);
  if (k >= 0 && presentMatches(list[k])) return k;
  const i = list.findIndex(presentMatches);
  return i >= 0 ? i : k;
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
  $('#p-pins').setAttribute('aria-pressed', String(P.pins));
}

function presentStep(dir) {
  const list = presentList();
  if (!list.length) return;
  closePop();
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
  if (mode !== 'comment' && !P.pins) closePop();
  if (P.loaded) post(P, { type: 'dfw:pins', pins: pinsFor(P) });
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
$('#p-pins').onclick = () => togglePresentPins();
$('#pins-toggle').onclick = () => cyclePins();

function togglePresentPins() {
  P.pins = !P.pins;
  store.set('ppins', P.pins);
  if (!P.pins) closePop();
  renderPresentBar();
  syncPins();
}

const PIN_LABEL = { active: 'Pins: open', all: 'Pins: all', off: 'Pins: off' };
function cyclePins() {
  S.pins = { active: 'all', all: 'off', off: 'active' }[S.pins] || 'active';
  store.set('pins', S.pins);
  if (S.pins === 'off') closePop();
  renderPinsToggle();
  syncPins();
  toast(S.pins === 'off' ? 'Comment pins hidden (Shift+C shows them).' : S.pins === 'all' ? 'Showing every pin, closed comments too.' : 'Showing pins of open and to-verify comments.');
}
function renderPinsToggle() {
  const b = $('#pins-toggle');
  b.textContent = PIN_LABEL[S.pins] || PIN_LABEL.active;
  b.setAttribute('aria-pressed', String(S.pins !== 'off'));
}
function syncPresentKey() {
  const f = presentList().find(presentMatches);
  if (f) P.key0 = f.key;
}
$('#p-bp').onchange = (e) => {
  P.bp = S.manifest.breakpoints.find((b) => b.id === e.target.value) || P.bp;
  syncPresentKey();
  loadPresent();
};
$('#p-state').onchange = (e) => {
  P.state = e.target.value || null;
  syncPresentKey();
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
    closePop();
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
      if (S.pop) return closePop();
      if (P.mode === 'comment') return setPresentMode('present');
      return closePresent();
    }
    if (key === 'ArrowLeft') return presentStep(-1);
    if (key === 'ArrowRight') return presentStep(1);
    if (key === 'C' && d.shift) return togglePresentPins();
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
  if (d.alt && !mod && (key.startsWith('Arrow') || (d.shift && d.code === 'Digit0')) && S.sel && S.mode === 'select') {
    const f = S.frames.get(S.sel.key);
    if (f && !f.system && f.file) post(f, { type: 'dfw:layout-key', key: key.startsWith('Arrow') ? key : 'reset', shift: d.shift });
    return;
  }
  if (mod || d.alt) return;
  if (d.shift && (d.code === 'Digit1' || key === '!')) return zoomToFit();
  if (d.shift && (d.code === 'Digit2' || key === '@')) return zoomToSelection();
  switch (key) {
    case 'Escape':
      if (!$('#composer').hidden) return closeComposer();
      if (S.pop) return closePop();
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
    case 'C':
      if (d.shift) return cyclePins();
    // falls through
    case 'c':
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
    commentActions(c, $('.comment-actions', li));
    li.onclick = () => focusComment(c);
    list.append(li);
  }
}

function commentActions(c, actions) {
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
  placePop();
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
  if (S.pop) {
    const c = S.comments.find((x) => x.id === S.pop);
    if (!c || !pinStatuses(P.open ? P : null).includes(c.status)) closePop();
    else renderPop();
  }
}

function renderToolbar() {
  const m = S.manifest;
  $('#product').textContent = m.product || 'Design Studio';
  const st = S.state;
  $('#meta').textContent = [st ? `v${st.design.version} ${st.design.status}` : null, m.inferred ? 'manifest inferred' : null].filter(Boolean).join(' · ');
  const bpName = (bp) => `${bp.label || bp.id} ${bp.width}`;
  const bpOn = activeBps();
  $('#bp-menu summary').textContent = !S.activeBps ? 'All breakpoints' : bpOn.length <= 2 ? bpOn.map(bpName).join(', ') : `${bpOn.length} breakpoints`;
  checkList($('#breakpoints'), m.breakpoints.map((bp) => ({ id: bp.id, label: bpName(bp) })), S.activeBps, (ids) => {
    S.activeBps = ids && !ids.length ? [bpOn[0].id] : ids; // keep at least one breakpoint
    store.set('bps3', S.activeBps);
  });
  const states = allStates();
  $('#state-menu').hidden = !states.length;
  const stOn = shownStates(states);
  $('#state-menu summary').textContent = `States: ${!S.activeStates ? 'all' : !stOn.length ? 'off' : stOn.length <= 2 ? stOn.join(', ') : stOn.length}`;
  checkList($('#states'), states.map((x) => ({ id: x, label: x })), S.activeStates, (ids) => {
    S.activeStates = ids;
    store.set('states3', ids);
  });
  $('#full-toggle').setAttribute('aria-pressed', String(S.fullHeight));
  renderPinsToggle();
  const themes = m.themes || [];
  $('#theme-wrap').hidden = !themes.length;
  $('#theme').innerHTML = `<option value="">default</option>` + themes.map((t) => `<option ${t === S.theme ? 'selected' : ''}>${esc(t)}</option>`).join('');
}

/** Checkbox list with an "All" row for a toolbar menu. checked: ids, or null = all. onChange gets ids or null (all). */
function checkList(wrap, items, checked, onChange) {
  const on = (id) => !checked || checked.includes(id);
  const n = items.filter((it) => on(it.id)).length;
  const change = (ids) => {
    onChange(ids && ids.length === items.length ? null : ids);
    renderToolbar();
    rebuildKeepCam();
  };
  wrap.innerHTML = '';
  const all = document.createElement('label');
  all.className = 'ms-all';
  all.innerHTML = `<input type="checkbox" ${n === items.length ? 'checked' : ''}> All`;
  all.querySelector('input').indeterminate = n > 0 && n < items.length;
  all.querySelector('input').onchange = (e) => change(e.target.checked ? null : []);
  wrap.append(all);
  for (const it of items) {
    const l = document.createElement('label');
    l.innerHTML = `<input type="checkbox" ${on(it.id) ? 'checked' : ''}> ${esc(it.label)}`;
    l.querySelector('input').onchange = (e) => change(items.map((x) => x.id).filter((id) => (id === it.id ? e.target.checked : on(id))));
    wrap.append(l);
  }
}

/** Every frame of every page with the current breakpoint/state/theme choices (a screen shown on two pages once). */
function allFrameSpecs() {
  const seen = new Map();
  for (const p of S.pages) for (const r of pageModel(p).rows) for (const f of r.frames) seen.set(f.key, f);
  return [...seen.values()];
}

/* Screenshot picker: screens grouped by page; the choice is kept in localStorage (null = all). */
const shootKey = (f) => (f.system ? 'system' : f.screen?.id || f.file);
let shootPicked = store.get('shoot1', null);

/** Screens available to screenshot, grouped by page, each with its frame count under the current toolbar choices. */
function shootGroups() {
  const count = new Map();
  for (const f of allFrameSpecs()) count.set(shootKey(f), (count.get(shootKey(f)) || 0) + 1);
  const seen = new Set();
  const groups = [];
  for (const p of S.pages) {
    if (p.flow) continue;
    const items = [];
    for (const r of pageModel(p).rows)
      for (const f of r.frames) {
        const k = shootKey(f);
        if (seen.has(k)) continue;
        seen.add(k);
        items.push({ id: k, label: f.system ? 'Design System' : f.screen?.title || k, n: count.get(k) || 0 });
      }
    if (items.length) groups.push({ id: p.id, title: p.title, items });
  }
  return groups;
}

function shootSelection() {
  return allFrameSpecs().filter((f) => !shootPicked || shootPicked.includes(shootKey(f)));
}

function renderShootMenu() {
  const groups = shootGroups();
  const ids = groups.flatMap((g) => g.items.map((i) => i.id));
  if (shootPicked) shootPicked = shootPicked.filter((id) => ids.includes(id));
  const on = (id) => !shootPicked || shootPicked.includes(id);
  const set = (next) => {
    shootPicked = next && next.length === ids.length ? null : next;
    store.set('shoot1', shootPicked);
    renderShootMenu();
  };
  const wrap = $('#shoot-items');
  wrap.innerHTML = '';
  const box = (cls, label, checked, mixed, onChange, n) => {
    const l = document.createElement('label');
    l.className = cls;
    l.innerHTML = `<input type="checkbox" ${checked ? 'checked' : ''}> <span>${esc(label)}</span>${n != null ? `<span class="shoot-n">${n}</span>` : ''}`;
    const i = l.querySelector('input');
    i.indeterminate = mixed;
    i.onchange = (e) => onChange(e.target.checked);
    wrap.append(l);
  };
  const nOn = ids.filter(on).length;
  box('ms-all', 'All screens', nOn === ids.length, nOn > 0 && nOn < ids.length, (c) => set(c ? null : []));
  for (const g of groups) {
    const gOn = g.items.filter((i) => on(i.id)).length;
    const gIds = g.items.map((i) => i.id);
    if (g.items.length > 1 || g.items[0].label !== g.title) box('shoot-page', g.title, gOn === g.items.length, gOn > 0 && gOn < g.items.length, (c) => set(c ? [...new Set([...ids.filter(on), ...gIds])] : ids.filter((id) => on(id) && !gIds.includes(id))));
    for (const it of g.items) box(g.items.length > 1 || it.label !== g.title ? 'shoot-screen' : 'shoot-page', it.label, on(it.id), false, (c) => set(c ? [...ids.filter(on), it.id] : ids.filter((id) => on(id) && id !== it.id)), it.n);
  }
  const frames = shootSelection().length;
  $('#shoot-count').textContent = `${frames} frame(s) · shown breakpoints/states`;
  $('#shoot-go').textContent = `Screenshot ${frames}`;
  $('#shoot-go').disabled = !frames;
}

/** Screenshot frame specs ({src, w, h, system, screen, variant, bp, state}) to PNG; one shot is also downloaded. */
async function shootFrames(list, btn) {
  if (btn.disabled) return;
  if (!list.length) return toast('No screen to screenshot.');
  btn.disabled = true;
  toast(`Screenshotting ${list.length} screen(s)…`);
  try {
    const name = (f) => (f.system ? 'design-system' : [f.screen?.id || f.file?.replace(/\.html$/, ''), f.variant?.id, f.bp?.id, S.theme, f.state].filter(Boolean).join('__').replace(/\//g, '__'));
    const res = await api('/__dfw/api/shoot', { method: 'POST', body: JSON.stringify({ frames: list.map((f) => ({ src: f.src, w: f.w, h: f.h, name: name(f) })) }) });
    const ok = res.filter((r) => r.ok);
    if (ok.length === 1) {
      const a = document.createElement('a');
      a.href = ok[0].url;
      a.download = ok[0].name;
      a.click();
    }
    const dir = ok[0]?.file.split('/').slice(0, -1).join('/');
    toast(`${ok.length}/${res.length} PNG saved${dir ? ` in ${dir}` : ''}${ok.length < res.length ? ` · failed: ${res.find((r) => !r.ok).error}` : ''}`);
  } catch (e) {
    // A Studio server started before an update serves the new page but not the new API.
    toast(e.message === 'not found' ? 'Screenshot needs a newer Studio server: run "dfw studio stop" then "dfw studio start", and reload.' : `Screenshot failed: ${e.message}`);
  } finally {
    btn.disabled = false;
  }
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
  if (!S.activeBps?.length) S.activeBps = null;
  renderToolbar();
  if (keepCam && S.page) rebuildKeepCam();
  else setPage(store.get('page', null) || S.pages.find((p) => !p.system && !p.flow && p.screens.length)?.id || 'system');
  await loadComments();
}

/* ---------------------------------------------------------------- wiring */

for (const b of $$('.seg button')) b.onclick = () => setMode(b.dataset.mode);
for (const b of $$('.ptabs button')) b.onclick = () => setTab(b.dataset.tab);
// Toolbar camera: a menu to pick which screens to screenshot (all by default), grouped by page.
$('#shoot-menu').addEventListener('toggle', () => $('#shoot-menu').open && renderShootMenu());
$('#shoot-go').onclick = async () => {
  const list = shootSelection();
  await shootFrames(list, $('#shoot-go'));
  if (list.length) $('#shoot-menu').open = false;
};
// Present: the screen as shown (breakpoint, state, theme), full height.
$('#p-shoot').onclick = () =>
  shootFrames([{ src: presentSrc(), w: P.system ? 1440 : P.bp.width, h: P.system ? 900 : P.bp.height, system: P.system, screen: P.screen, variant: P.variant, file: P.file, bp: P.bp, state: P.state }], $('#p-shoot'));
$('#zoom-in').onclick = () => zoomStep(1);
$('#zoom-out').onclick = () => zoomStep(-1);
$('#zoom-label').onclick = () => zoomToFit();
$('#help').onclick = () => ($('#shortcuts').hidden = false);
$('#shortcuts-close').onclick = () => ($('#shortcuts').hidden = true);
$('#shortcuts').onclick = (e) => e.target.id === 'shortcuts' && ($('#shortcuts').hidden = true);
// Close an open toolbar menu on a click outside it.
document.addEventListener('click', (e) => {
  for (const d of $$('details.ms[open]')) if (!d.contains(e.target)) d.open = false;
});
$('#full-toggle').onclick = () => {
  S.fullHeight = !S.fullHeight;
  store.set('full2', S.fullHeight);
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
