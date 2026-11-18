// dfw bridge: a Penpot plugin (runs inside Penpot's plugin sandbox, Penpot plugin API).
// Served by the local dfw Studio (127.0.0.1) at /__dfw/penpot/plugin.js; its UI (ui.html) is served
// by the same Studio, so the UI's data calls never leave the machine.
//   Build boards : export (from "dfw penpot export") -> Penpot pages, boards, components, text layers.
//   Update boards: redraw the boards already in the file from a new export, in place (comment threads stay).
//   Live comments: every few seconds the panel asks for the comment threads on dfw boards; the Studio turns
//                  new ones into dfw comments and answers with dfw's replies, posted into the threads here.
//   Send changes : boards built here + main components on the "dfw components" page + Penpot comments
//                  + token sets -> the Studio, which records them as dfw feedback comments (component
//                  edits target the master file). dfw never overwrites its HTML from Penpot.
// API used: penpot.ui, createPage/openPage, createBoard/createRectangle/createText/createShapeFromSvg,
// uploadMediaData, library.local.createComponent / component.instance() / component.mainInstance(),
// library.local.components, library.local.tokens,
// fonts.findByName, Page.findCommentThreads, CommentThread.findComments/reply, Shape.remove, setPluginData/getPluginData (doc.plugins.penpot.app).
/* global penpot */
const STUDIO = '__DFW_STUDIO_ORIGIN__';

penpot.ui.open('dfw bridge', `${STUDIO}/__dfw/penpot/ui.html`, { width: 360, height: 520 });

penpot.ui.onMessage(async (msg) => {
  try {
    if (msg && msg.type === 'build') penpot.ui.sendMessage({ type: 'built', stats: await build(msg.export, msg.images || {}, msg.options || {}) });
    else if (msg && msg.type === 'collect') penpot.ui.sendMessage({ type: 'changes', data: await collect() });
    else if (msg && msg.type === 'threads') penpot.ui.sendMessage({ type: 'threads', data: await liveThreads() });
    else if (msg && msg.type === 'reply') penpot.ui.sendMessage({ type: 'replied', done: await reply(msg.replies || []) });
  } catch (e) {
    penpot.ui.sendMessage({ type: 'error', of: msg && msg.type, message: String((e && e.message) || e) });
  }
});

/* ------------------------------------------------------------ helpers */

function data(shape, key) {
  try {
    const v = shape && shape.getPluginData(key);
    return v ? JSON.parse(v) : null;
  } catch (e) {
    return null;
  }
}

function tokenIndex() {
  const out = new Map();
  try {
    for (const set of penpot.library.local.tokens.sets) for (const t of set.tokens) if (!out.has(t.name)) out.set(t.name, t);
  } catch (e) {}
  return out;
}

function applyToken(tokens, name, shape, stats) {
  if (!name) return;
  const t = tokens.get(name);
  if (!t) return;
  try {
    t.applyToShapes([shape]);
    stats.tokenLinks++;
  } catch (e) {
    stats.warnings.push(`token ${name}: ${e.message}`);
  }
}

function setFont(text, st) {
  try {
    const font = penpot.fonts.findByName(st.fontFamily);
    if (font) {
      const v = (font.variants || []).find((x) => String(x.fontWeight) === String(st.fontWeight) && (x.fontStyle || 'normal') === (st.fontStyle || 'normal'));
      font.applyToText(text, v);
      return;
    }
  } catch (e) {}
  text.fontFamily = st.fontFamily;
  text.fontWeight = String(st.fontWeight);
}

function paint(shape, l, ctx) {
  if (l.fills) {
    shape.fills = l.fills.map((f) => ({ fillColor: f.color, fillOpacity: f.opacity }));
    if (l.fills[0].token) applyToken(ctx.tokens, l.fills[0].token, shape, ctx.stats);
  } else if (shape.type !== 'text') shape.fills = [];
  if (l.strokes) shape.strokes = l.strokes.map((s) => ({ strokeColor: s.color, strokeOpacity: s.opacity, strokeWidth: s.width, strokeStyle: s.style, strokeAlignment: 'inner' }));
  if (l.radius) {
    shape.borderRadiusTopLeft = l.radius[0];
    shape.borderRadiusTopRight = l.radius[1];
    shape.borderRadiusBottomRight = l.radius[2];
    shape.borderRadiusBottomLeft = l.radius[3];
    if (l.radiusToken) applyToken(ctx.tokens, l.radiusToken, shape, ctx.stats);
  }
  if (l.shadows) {
    shape.shadows = l.shadows.map((s) => ({ style: s.inset ? 'inner-shadow' : 'drop-shadow', offsetX: s.x, offsetY: s.y, blur: s.blur, spread: s.spread, hidden: false, color: { color: s.color.hex, opacity: s.color.opacity } }));
  }
  if (l.opacity !== undefined) shape.opacity = l.opacity;
}

function place(shape, parent, l, ox, oy) {
  if (parent) parent.appendChild(shape);
  shape.x = ox + l.x;
  shape.y = oy + l.y;
  if (shape.type !== 'group') shape.resize(Math.max(0.01, l.w), Math.max(0.01, l.h));
}

function mark(shape, l, prefix) {
  shape.name = l.name;
  shape.setPluginData('dfw', JSON.stringify({ path: prefix + l.path, kind: l.kind, dfwId: l.dfwId, anchor: l.anchor }));
}

function textLayer(l) {
  const t = penpot.createText(l.characters || ' ');
  const st = l.text || {};
  setFont(t, st);
  if (st.fontSize) t.fontSize = String(st.fontSize);
  if (st.lineHeight) t.lineHeight = String(st.lineHeight);
  t.letterSpacing = String(st.letterSpacing || 0);
  if (st.transform) t.textTransform = st.transform;
  if (st.decoration) t.textDecoration = st.decoration;
  if (st.align) t.align = st.align;
  t.growType = 'fixed';
  if (st.color) t.fills = [{ fillColor: st.color.hex, fillOpacity: st.color.opacity }];
  return t;
}

function sideBorders(frame, l, ox, oy) {
  const b = l.sideBorders;
  const add = (s, x, y, w, h) => {
    if (!s) return;
    const r = penpot.createRectangle();
    frame.appendChild(r);
    r.name = 'border';
    r.x = ox + x;
    r.y = oy + y;
    r.resize(Math.max(0.01, w), Math.max(0.01, h));
    r.fills = [{ fillColor: s.color.hex, fillOpacity: s.color.opacity }];
    r.setPluginData('dfw', JSON.stringify({ deco: true }));
  };
  add(b.top, l.x, l.y, l.w, b.top && b.top.w);
  add(b.bottom, l.x, l.y + l.h - (b.bottom ? b.bottom.w : 0), l.w, b.bottom && b.bottom.w);
  add(b.left, l.x, l.y, b.left && b.left.w, l.h);
  add(b.right, l.x + l.w - (b.right ? b.right.w : 0), l.y, b.right && b.right.w, l.h);
}

/** Draw one exported layer (and its children) into `parent`; ox/oy = board origin on the page. */
async function draw(l, parent, ox, oy, ctx, prefix, useInstances) {
  ctx.stats.layers++;
  if (useInstances && l.component && ctx.mains.has(l.component.key)) {
    const main = ctx.mains.get(l.component.key);
    if (main.sig === l.sig) {
      const inst = main.comp.instance();
      place(inst, parent, l, ox, oy);
      mark(inst, l, prefix);
      override(inst, l, ox, oy, ctx, prefix);
      ctx.stats.instances++;
      return inst;
    }
  }
  let shape;
  if (l.kind === 'text') {
    shape = textLayer(l);
    place(shape, parent, l, ox, oy);
    // Penpot's font metrics differ from Chrome's: let one-line text grow sideways, longer text downwards.
    const lineH = ((l.text && l.text.fontSize) || 16) * ((l.text && l.text.lineHeight) || 1.2);
    shape.growType = l.h < lineH * 1.6 ? 'auto-width' : 'auto-height';
    if (l.text && l.text.colorToken) applyToken(ctx.tokens, l.text.colorToken, shape, ctx.stats);
  } else if (l.kind === 'svg') {
    shape = penpot.createShapeFromSvg(l.svg);
    if (!shape) return null;
    if (parent) parent.appendChild(shape);
    shape.x = ox + l.x;
    shape.y = oy + l.y;
  } else if (l.kind === 'image') {
    shape = penpot.createRectangle();
    place(shape, parent, l, ox, oy);
    const bytes = l.src && ctx.images[l.src];
    if (bytes) {
      try {
        const img = await penpot.uploadMediaData(l.name, bytes.data, bytes.type);
        shape.fills = [{ fillOpacity: 1, fillImage: img }];
      } catch (e) {
        ctx.stats.warnings.push(`image ${l.src}: ${e.message}`);
      }
    } else shape.fills = [{ fillColor: '#d4d4d8', fillOpacity: 1 }];
  } else if (l.children && l.children.length) {
    shape = penpot.createBoard();
    place(shape, parent, l, ox, oy);
    shape.clipContent = Boolean(l.clip);
    paint(shape, l, ctx);
    for (const c of l.children) await draw(c, shape, ox, oy, ctx, prefix, useInstances);
    if (l.sideBorders) sideBorders(shape, l, ox, oy);
  } else {
    shape = penpot.createRectangle();
    place(shape, parent, l, ox, oy);
    paint(shape, l, ctx);
    if (l.sideBorders && parent) sideBorders(parent, l, ox, oy);
  }
  mark(shape, l, prefix);
  return shape;
}

/** Instance children are matched to exported layers by name (n-th occurrence), then overridden. */
function override(shape, l, ox, oy, ctx, prefix) {
  const kids = shape.children || [];
  const used = new Map();
  for (const c of l.children || []) {
    const k = used.get(c.name) || 0;
    used.set(c.name, k + 1);
    const match = kids.filter((s) => s.name === c.name)[k];
    if (!match) continue;
    match.setPluginData('dfw', JSON.stringify({ path: prefix + c.path, kind: c.kind, dfwId: c.dfwId, anchor: c.anchor }));
    try {
      if (c.kind === 'text' && match.characters !== c.characters) match.characters = c.characters;
      if (Math.abs(match.x - (ox + c.x)) > 0.5 || Math.abs(match.y - (oy + c.y)) > 0.5) {
        match.x = ox + c.x;
        match.y = oy + c.y;
      }
      if (c.kind !== 'svg' && (Math.abs(match.width - c.w) > 0.5 || Math.abs(match.height - c.h) > 0.5)) match.resize(Math.max(0.01, c.w), Math.max(0.01, c.h));
      if (c.fills && c.kind !== 'text') match.fills = c.fills.map((f) => ({ fillColor: f.color, fillOpacity: f.opacity }));
    } catch (e) {
      ctx.stats.warnings.push(`override ${c.name}: ${e.message}`);
    }
    if (c.kind !== 'svg') override(match, c, ox, oy, ctx, prefix);
  }
  // Border strips drawn for one-sided borders are decoration, not designer additions.
  for (const s of kids) if (s.name === 'border' && !data(s, 'dfw')) s.setPluginData('dfw', JSON.stringify({ deco: true }));
}

/* ------------------------------------------------------------ build */

async function build(doc, images, options) {
  if (!doc || doc.schema !== 'dfw-penpot-export/1') throw new Error('Not a dfw Penpot export (run "dfw penpot export").');
  const stats = { pages: 0, boards: 0, updated: 0, layers: 0, components: 0, instances: 0, tokenLinks: 0, warnings: [] };
  const ctx = { tokens: tokenIndex(), images, mains: new Map(), stats };
  if (!ctx.tokens.size) stats.warnings.push('No tokens in this file: import .dfw/penpot/tokens.penpot.json in the Tokens panel first to link colours to tokens.');
  const useInstances = options.components !== false;
  const update = Boolean(options.update);
  const old = update ? existing() : { boards: new Map(), pages: new Map(), mains: new Map() };
  // Update: reuse the main components already in the file when their structure is unchanged;
  // a new or changed master is drawn as plain layers (no second main is created).
  if (update) for (const [k, m] of old.mains) ctx.mains.set(k, m);
  if (!update && useInstances && doc.components.length) {
    const page = penpot.createPage();
    page.name = 'dfw components';
    await penpot.openPage(page);
    let x = 0;
    let y = 0;
    let rowH = 0;
    for (const c of doc.components) {
      const ox = x - c.root.x;
      const oy = y - c.root.y;
      const shape = await draw({ ...c.root, component: null }, null, ox, oy, ctx, `main:${c.key}#`, false);
      if (!shape) continue;
      try {
        const comp = penpot.library.local.createComponent([shape]);
        comp.name = c.name;
        // Key of the dfw master (component|variant|size): on the main shape and on the library component,
        // so "Send changes" finds the main again whatever Penpot does with the shape on createComponent.
        const mainMeta = JSON.stringify({ key: c.key, exportId: doc.id, sig: c.sig });
        shape.setPluginData('dfw-main', mainMeta);
        try {
          comp.setPluginData('dfw-main', mainMeta);
          const m = comp.mainInstance();
          if (m && m.id !== shape.id) m.setPluginData('dfw-main', mainMeta);
        } catch (e) {}
        ctx.mains.set(c.key, { comp, sig: c.sig });
        stats.components++;
      } catch (e) {
        stats.warnings.push(`component ${c.name}: ${e.message}`);
      }
      x += c.root.w + 80;
      rowH = Math.max(rowH, c.root.h);
      if (x > 2400) {
        x = 0;
        y += rowH + 80;
        rowH = 0;
      }
    }
  }
  const keys = [];
  for (const pg of doc.pages) {
    let page = old.pages.get(pg.title);
    if (!page) {
      page = penpot.createPage();
      page.name = pg.title;
    }
    await penpot.openPage(page);
    stats.pages++;
    for (const b of pg.boards) {
      // An existing board keeps its place and identity, so comment threads stay on it.
      let board = old.boards.get(b.key);
      if (board) {
        for (const child of [...(board.children || [])]) child.remove();
        stats.updated++;
      } else {
        board = penpot.createBoard();
        board.x = b.x;
        board.y = b.y;
      }
      board.name = b.name;
      board.resize(b.width, b.height);
      board.clipContent = true;
      board.fills = [{ fillColor: b.background.color, fillOpacity: b.background.opacity }];
      if (b.background.token) applyToken(ctx.tokens, b.background.token, board, stats);
      board.setPluginData('dfw-board', JSON.stringify({ key: b.key, exportId: doc.id, screen: b.screen, variant: b.variant, breakpoint: b.breakpoint }));
      for (const l of b.layers) await draw(l, board, board.x, board.y, ctx, '', useInstances);
      keys.push(b.key);
      stats.boards++;
    }
  }
  try {
    const before = data(penpot.currentFile, 'dfw-export');
    const allKeys = update && before && before.keys ? [...new Set([...before.keys, ...keys])] : keys;
    penpot.currentFile.setPluginData('dfw-export', JSON.stringify({ id: doc.id, keys: allKeys, components: [...ctx.mains.keys()] }));
  } catch (e) {}
  return stats;
}

/** What a previous build left in the file: boards by dfw key, pages by name, main components by key. */
function existing() {
  const out = { boards: new Map(), pages: new Map(), mains: new Map() };
  for (const page of penpot.currentFile.pages) {
    if (!out.pages.has(page.name)) out.pages.set(page.name, page);
    for (const shape of (page.root && page.root.children) || []) {
      const meta = data(shape, 'dfw-board');
      if (meta && !out.boards.has(meta.key)) out.boards.set(meta.key, shape);
    }
  }
  try {
    for (const comp of penpot.library.local.components || []) {
      let main = null;
      try {
        main = comp.mainInstance();
      } catch (e) {}
      const meta = data(main, 'dfw-main') || data(comp, 'dfw-main');
      if (meta && meta.sig && !out.mains.has(meta.key)) out.mains.set(meta.key, { comp, sig: meta.sig });
    }
  } catch (e) {}
  return out;
}

/* ------------------------------------------------------------ comment threads */

/** Comment threads of one page: [{ thread, entry }] (entry is what goes to dfw). */
async function pageThreads(page, warnings) {
  const out = [];
  try {
    const threads = await page.findCommentThreads({ onlyYours: false, showResolved: true });
    for (const t of threads) {
      const bm = t.board ? data(t.board, 'dfw-board') : null;
      const mm = t.board && !bm ? data(t.board, 'dfw-main') : null;
      const comments = await t.findComments();
      out.push({
        thread: t,
        entry: {
          seq: t.seqNumber,
          resolved: Boolean(t.resolved),
          page: page.name,
          boardKey: bm ? bm.key : null,
          componentKey: mm ? mm.key : null,
          x: t.position.x - (t.board ? t.board.x : 0),
          y: t.position.y - (t.board ? t.board.y : 0),
          comments: comments.map((c) => ({ content: c.content, user: c.user ? c.user.name : null, date: c.date ? String(c.date) : null })),
        },
      });
    }
  } catch (e) {
    warnings.push(`comments on page ${page.name}: ${e.message}`);
  }
  return out;
}

/** Live comments: only threads on boards and main components built by dfw (other comments in the file are not dfw's). */
async function liveThreads() {
  const file = penpot.currentFile;
  const ex = data(file, 'dfw-export');
  const out = { fileId: file ? file.id : null, exportId: ex ? ex.id : null, threads: [], warnings: [] };
  for (const page of file.pages) for (const x of await pageThreads(page, out.warnings)) if (x.entry.boardKey || x.entry.componentKey) out.threads.push(x.entry);
  return out;
}

/** Post dfw's replies ("[dfw] fb_012 fixed: ...") into their threads; returns the ones posted. */
async function reply(replies) {
  const bySeq = new Map();
  for (const page of penpot.currentFile.pages) for (const x of await pageThreads(page, [])) bySeq.set(x.entry.seq, x.thread);
  const done = [];
  for (const r of replies) {
    const t = bySeq.get(r.seq);
    if (!t) continue;
    try {
      await t.reply(String(r.text));
      done.push({ seq: r.seq, key: r.key });
    } catch (e) {}
  }
  return done;
}

/* ------------------------------------------------------------ collect (send changes) */

function fillOf(s) {
  const f = Array.isArray(s.fills) ? s.fills[0] : null;
  return f && f.fillColor ? { color: String(f.fillColor).toLowerCase(), opacity: f.fillOpacity === undefined ? 1 : f.fillOpacity } : null;
}

function strokeOf(s) {
  const k = Array.isArray(s.strokes) ? s.strokes[0] : null;
  return k && k.strokeColor ? { color: String(k.strokeColor).toLowerCase(), opacity: k.strokeOpacity === undefined ? 1 : k.strokeOpacity, width: k.strokeWidth === undefined ? 1 : k.strokeWidth } : null;
}

/** Fill, stroke and corner radii of a shape, as sent back (strokes/radius only matter for component masters). */
function paintOf(s) {
  const fill = fillOf(s);
  const stroke = strokeOf(s);
  const out = { fills: fill ? [fill] : Array.isArray(s.fills) ? [] : undefined, strokes: stroke ? [stroke] : Array.isArray(s.strokes) ? [] : undefined };
  if (s.type !== 'text' && typeof s.borderRadiusTopLeft === 'number') out.radius = [s.borderRadiusTopLeft, s.borderRadiusTopRight, s.borderRadiusBottomRight, s.borderRadiusBottomLeft].map((v) => v || 0);
  return out;
}

function walk(shape, board, out) {
  for (const c of shape.children || []) {
    const meta = data(c, 'dfw');
    if (meta && meta.deco) continue;
    const box = { x: c.x - board.x, y: c.y - board.y, w: c.width, h: c.height };
    const fill = fillOf(c);
    if (meta && meta.path !== undefined) {
      out.layers.push({ path: meta.path, name: c.name, kind: meta.kind, ...box, characters: c.type === 'text' ? c.characters : undefined, ...paintOf(c) });
      if (meta.kind === 'svg') continue;
    } else {
      out.added.push({ name: c.name, kind: c.type, ...box, characters: c.type === 'text' ? c.characters : undefined, fills: fill ? [fill] : undefined });
      continue;
    }
    walk(c, board, out);
  }
}

async function collect() {
  const file = penpot.currentFile;
  const ex = data(file, 'dfw-export');
  const out = { schema: 'dfw-penpot-changes/1', fileId: file ? file.id : null, exportId: ex ? ex.id : null, pushedAt: new Date().toISOString(), fileName: file ? file.name : null, boards: [], missingBoards: [], components: [], missingComponents: [], comments: [], tokens: null, warnings: [] };
  const seen = new Set();
  for (const page of file.pages) {
    const boards = new Map();
    for (const shape of (page.root && page.root.children) || []) {
      const meta = data(shape, 'dfw-board');
      if (!meta) continue;
      if (!out.exportId) out.exportId = meta.exportId;
      seen.add(meta.key);
      boards.set(shape.id, { shape, meta });
      const b = { key: meta.key, name: shape.name, layers: [], added: [] };
      walk(shape, shape, b);
      out.boards.push(b);
    }
    for (const x of await pageThreads(page, out.warnings)) out.comments.push(x.entry);
  }
  if (ex && ex.keys) out.missingBoards = ex.keys.filter((k) => !seen.has(k));
  // Main components (the dfw components page): coordinates relative to the main's own origin.
  const mains = new Set();
  try {
    for (const comp of penpot.library.local.components || []) {
      let main = null;
      try {
        main = comp.mainInstance();
      } catch (e) {}
      const meta = data(main, 'dfw-main') || data(comp, 'dfw-main');
      if (!meta || !main || mains.has(meta.key)) continue;
      mains.add(meta.key);
      if (!out.exportId) out.exportId = meta.exportId;
      const rootMeta = data(main, 'dfw');
      const c = { key: meta.key, name: comp.name, layers: [{ root: true, path: rootMeta ? rootMeta.path : null, name: main.name, kind: rootMeta ? rootMeta.kind : 'frame', x: 0, y: 0, w: main.width, h: main.height, ...paintOf(main) }], added: [] };
      walk(main, main, c);
      out.components.push(c);
    }
  } catch (e) {
    out.warnings.push(`components: ${e.message}`);
  }
  if (ex && ex.components) out.missingComponents = ex.components.filter((k) => !mains.has(k));
  try {
    const cat = penpot.library.local.tokens;
    out.tokens = {
      sets: cat.sets.map((s) => ({ name: s.name, active: s.active, tokens: s.tokens.map((t) => ({ name: t.name, type: t.type, value: t.value, description: t.description || undefined })) })),
      themes: cat.themes.map((th) => ({ name: th.name, group: th.group, sets: (th.activeSets || []).map((s) => s.name) })),
    };
  } catch (e) {
    out.warnings.push(`tokens: ${e.message}`);
  }
  return out;
}
