// dfw Design Studio: a local, dependency-free web server that renders the
// project's design workspace (design/) as a multi-screen canvas and collects
// element-level review comments. Implementation detail only: the studio is
// plain HTML/CSS/JS and does not influence the product's frontend stack.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { projectPaths, readJson, parseArgs, listFiles, PLUGIN_ROOT } from '../lib/util.mjs';
import { loadConfig } from '../lib/config.mjs';
import { loadState, summarize } from '../lib/state.mjs';
import { addComment, allComments, updateComment, newSession, currentSession } from '../lib/comments.mjs';
import { composeHtml, loadComponents, buildSystem, escapeHtml } from '../lib/compose.mjs';
import { applyEdit, undoEdit, loadTokenNames } from '../lib/edit.mjs';
import { studioPages, screenFlows } from './pages.mjs';
import { screenshot } from '../lib/runtime.mjs';
import { exportPenpot, importChanges, formatImportChanges } from '../lib/penpot.mjs';

// Started as "dfw __studio-server --root <dir> --port <n>" (works the same from source and in the binary).
const HERE = path.join(PLUGIN_ROOT, 'studio');
const args = parseArgs(globalThis.__DFW_STUDIO_ARGV || process.argv.slice(2));
const root = path.resolve(args.root || process.cwd());
const P = projectPaths(root);
const config = loadConfig(root);
const port = Number(args.port || config.studio.port || 4600);
const host = '127.0.0.1';

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.webp': 'image/webp',
  '.avif': 'image/avif',
  '.gif': 'image/gif',
  '.ico': 'image/x-icon',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
  '.ttf': 'font/ttf',
  '.otf': 'font/otf',
};

const clients = new Set();
function broadcast(event, data) {
  const msg = `event: ${event}\ndata: ${JSON.stringify(data)}\n\n`;
  for (const res of clients) res.write(msg);
}

/** Manifest; when design.json is missing, infer screens from design/screens/**. */
function manifest() {
  const m = readJson(P.manifest, null);
  const base = {
    breakpoints: config.studio.breakpoints,
    themes: [],
    states: [],
  };
  if (m) {
    const out = { ...base, ...m, breakpoints: m.breakpoints || base.breakpoints };
    const files = new Map();
    for (const s of out.screens || []) for (const v of s.variants || []) {
      const f = path.join(P.design, v.file || '');
      if (v.file && fs.existsSync(f)) files.set(v.file, fs.readFileSync(f, 'utf8'));
    }
    out.flows = screenFlows(out, files);
    return { ...out, studioPages: studioPages(out) };
  }
  const screens = new Map();
  for (const rel of listFiles(path.join(P.design, 'screens'))) {
    if (!rel.endsWith('.html')) continue;
    const parts = rel.split('/');
    const id = parts.length > 1 ? parts.slice(0, -1).join('/') : parts[0].replace(/\.html$/, '');
    const variant = parts.length > 1 ? parts.at(-1).replace(/\.html$/, '') : 'a';
    if (!screens.has(id)) screens.set(id, { id, title: id, variants: [] });
    screens.get(id).variants.push({ id: variant, label: variant, file: `screens/${rel}` });
  }
  const out = { ...base, product: path.basename(root), inferred: true, screens: [...screens.values()] };
  return { ...out, studioPages: studioPages(out) };
}

let shooting = false;

/**
 * Screenshot Studio frames to .dfw/shots/studio/<time>/<name>.png with the same headless renderer as "dfw shoot".
 * frames: [{ src: '/design/...' | '/__dfw/system.html', w, h, name }]. Returns [{ name, ok, file, url, error }].
 */
async function shootFrames({ frames = [], fullPage = true } = {}) {
  if (shooting) throw Object.assign(new Error('A screenshot is already running.'), { exitCode: 1 });
  const list = frames.filter((f) => typeof f.src === 'string' && (f.src.startsWith('/design/') || f.src === '/__dfw/system.html')).slice(0, 200);
  if (!list.length) throw Object.assign(new Error('Nothing to screenshot.'), { exitCode: 1 });
  const dir = path.join(P.dfw, 'shots', 'studio', new Date(Date.now() - new Date().getTimezoneOffset() * 60000).toISOString().replace(/[:.]/g, '-').slice(0, 19)); // local time
  const jobs = list.map((f) => ({
    url: `http://${host}:${port}${f.src}`,
    width: Math.max(200, Math.min(4000, Number(f.w) || 1440)),
    height: Math.max(200, Math.min(4000, Number(f.h) || 900)),
    out: path.join(dir, `${String(f.name || 'frame').replace(/[^\w.-]+/g, '_').slice(0, 120)}.png`),
    fullPage,
  }));
  shooting = true;
  try {
    const res = await screenshot(jobs);
    return res.map((r) => {
      const rel = path.relative(P.dfw, r.out).split(path.sep).join('/');
      return { name: path.basename(r.out), ok: r.ok, error: r.error, file: path.relative(root, r.out), url: r.ok ? `/__dfw/${rel}` : null };
    });
  } finally {
    shooting = false;
  }
}

function send(res, code, body, type = 'application/json; charset=utf-8') {
  res.writeHead(code, { 'Content-Type': type, 'Cache-Control': 'no-store' });
  res.end(typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body));
}

async function body(req) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const s = Buffer.concat(chunks).toString('utf8');
  return s ? JSON.parse(s) : {};
}

function safeJoin(base, rel) {
  const abs = path.resolve(base, '.' + path.posix.normalize('/' + rel));
  return abs === base || abs.startsWith(base + path.sep) ? abs : null;
}

// The Studio is for the local user only. A web page the user visits could otherwise post comments
// (which the agent reads as the user's words) or edits to it, and a DNS-rebinding page could read it:
// the Host must be this server, and a request that changes something must not come from another origin.
const SELF_HOSTS = new Set([`${host}:${port}`, `localhost:${port}`]);
function foreign(req) {
  if (!SELF_HOSTS.has(String(req.headers.host || '').toLowerCase())) return `host ${req.headers.host || '(none)'} not allowed`;
  const from = req.headers.origin;
  if (!['GET', 'HEAD', 'OPTIONS'].includes(req.method) && from && !SELF_HOSTS.has(from.replace(/^http:\/\//, ''))) return `origin ${from} not allowed`;
  return null;
}

const OVERLAY_TAG = '<script src="/__dfw/overlay.js" data-dfw-overlay></script>';
const DESIGN_BASE = '/design/';

/** Compose <dfw-use>/<dfw-icon>/<dfw-head>, make sure component CSS is linked, inject the overlay. */
function prepareHtml(html) {
  const r = composeHtml(root, html, { designBase: DESIGN_BASE });
  let out = r.html;
  if (r.instances.length && !/system\/components\.css/.test(out)) {
    const link = `<link rel="stylesheet" href="${DESIGN_BASE}system/components.css">`;
    out = /<\/head>/i.test(out) ? out.replace(/<\/head>/i, `${link}</head>`) : link + out;
  }
  return /<\/body>/i.test(out) ? out.replace(/<\/body>(?![\s\S]*<\/body>)/i, `${OVERLAY_TAG}</body>`) : out + OVERLAY_TAG;
}

function serveDesignFile(res, rel) {
  const abs = safeJoin(P.design, rel);
  // Pages link the generated component CSS even before any component exists.
  if (rel === 'system/components.css' && abs && !fs.existsSync(abs)) return send(res, 200, '/* no components yet */', MIME['.css']);
  if (!abs || !fs.existsSync(abs) || fs.statSync(abs).isDirectory()) return send(res, 404, { error: 'not found' });
  const ext = path.extname(abs).toLowerCase();
  if (ext === '.html') return send(res, 200, prepareHtml(fs.readFileSync(abs, 'utf8')), MIME['.html']);
  return send(res, 200, fs.readFileSync(abs), MIME[ext] || 'application/octet-stream');
}

/** Fallback Design System page, used until/unless core/lib/system-page.mjs is available. */
function fallbackSystemPage() {
  const lib = loadComponents(root);
  const sections = [...lib.values()]
    .map(
      (d) => `<section id="c-${escapeHtml(d.name)}" data-dfw-id="system.component.${escapeHtml(d.name)}" style="padding:32px 0;border-top:1px solid #ddd">
  <h2 style="margin:0 0 4px">${escapeHtml(d.name)}</h2><p style="margin:0 0 16px;color:#666">${escapeHtml(d.description)}</p>
  <div style="display:flex;flex-wrap:wrap;gap:16px;align-items:center">${d.examples || ''}</div></section>`,
    )
    .join('\n');
  return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Design System</title>
<link rel="stylesheet" href="${DESIGN_BASE}tokens/tokens.css"><link rel="stylesheet" href="${DESIGN_BASE}system/components.css">
</head><body style="margin:0;padding:48px;font-family:system-ui,sans-serif">
<h1 data-dfw-id="system.title">Design System</h1>
${sections || '<p>No components yet. Add masters to <code>design/system/components/</code>.</p>'}
</body></html>`;
}

async function systemPage() {
  let html = null;
  try {
    const mod = await import('../lib/system-page.mjs');
    if (typeof mod.renderSystemPage === 'function') {
      const r = await mod.renderSystemPage(root, { designBase: DESIGN_BASE });
      html = typeof r === 'string' ? r : r?.html || null;
    }
  } catch (e) {
    if (e.code !== 'ERR_MODULE_NOT_FOUND') console.error(`dfw studio: system page: ${e.message}`);
  }
  return prepareHtml(html || fallbackSystemPage());
}

/** Design tokens for the inspector: [{name, var, type}] from tokens.json (+ any extra vars in tokens.css). */
function tokenList() {
  const out = [];
  const seen = new Set();
  const SUB = { fontFamily: 'font-family', fontSize: 'font-size', fontWeight: 'font-weight', letterSpacing: 'letter-spacing', lineHeight: 'line-height' };
  for (const [name, t] of loadTokenNames(root)) {
    if (t.type === 'typography' && t.value && typeof t.value === 'object') {
      for (const [k, sub] of Object.entries(SUB)) if (t.value[k] !== undefined) out.push({ name: `${name}.${sub}`, var: `${t.var}-${sub}`, type: 'typography' });
    } else out.push({ name, var: t.var, type: t.type || null });
  }
  for (const x of out) seen.add(x.var);
  const css = path.join(P.tokens, 'tokens.css');
  if (fs.existsSync(css)) {
    for (const m of fs.readFileSync(css, 'utf8').matchAll(/(--[\w-]+)\s*:/g)) {
      if (seen.has(m[1])) continue;
      seen.add(m[1]);
      out.push({ name: m[1].slice(2).replace(/-/g, '.'), var: m[1], type: null });
    }
  }
  return out;
}

// Penpot bridge: Penpot (in the user's browser) fetches the plugin manifest and code cross-origin;
// the plugin UI is served from here, so its data calls are same-origin. Writes accept only that origin.
const PENPOT_STATIC = path.join(HERE, 'penpot');
const PENPOT_CORS = { 'Access-Control-Allow-Origin': '*', 'Access-Control-Allow-Methods': 'GET', 'Access-Control-Allow-Headers': 'content-type', 'Access-Control-Allow-Private-Network': 'true' };
let penpotExporting = null;
async function penpotRoute(req, res, p, url) {
  const origin = `http://${host}:${port}`;
  const rel = p.slice('/__dfw/penpot/'.length);
  const cors = (code, body, type) => {
    res.writeHead(code, { 'Content-Type': type, 'Cache-Control': 'no-store', ...PENPOT_CORS });
    res.end(body);
  };
  if (req.method === 'OPTIONS') return cors(204, '', 'text/plain');
  if (rel === 'manifest.json') {
    const m = { name: 'dfw bridge', description: 'Build boards from a local dfw design and send designer changes back as dfw comments (local Studio on 127.0.0.1).', version: 2, code: 'plugin.js', icon: 'icon.svg', permissions: ['content:read', 'content:write', 'library:read', 'library:write', 'comment:read', 'allow:downloads'] };
    return cors(200, JSON.stringify(m, null, 2), MIME['.json']);
  }
  if (rel === 'plugin.js') return cors(200, fs.readFileSync(path.join(PENPOT_STATIC, 'plugin.js'), 'utf8').replaceAll('__DFW_STUDIO_ORIGIN__', origin), MIME['.js']);
  if (rel === 'icon.svg') return cors(200, fs.readFileSync(path.join(PENPOT_STATIC, 'icon.svg')), MIME['.svg']);
  if (rel === 'ui.html') return send(res, 200, fs.readFileSync(path.join(PENPOT_STATIC, 'ui.html')), MIME['.html']);
  if (rel === 'export.json' && req.method === 'GET') {
    const file = path.join(P.dfw, 'penpot', 'export.json');
    if (url.searchParams.get('refresh') === '1' || !fs.existsSync(file)) {
      penpotExporting ||= exportPenpot(root, { page: url.searchParams.get('page') || null, bp: url.searchParams.get('bp') || null }).finally(() => (penpotExporting = null));
      return send(res, 200, (await penpotExporting).doc);
    }
    return send(res, 200, fs.readFileSync(file), MIME['.json']);
  }
  if (rel === 'changes' && req.method === 'POST') {
    const r = importChanges(root, await body(req), { state: loadState(root) });
    return send(res, 200, { summary: formatImportChanges(r, root), comments: r.comments.map((c) => c.id), changes: r.changes.length });
  }
  return send(res, 404, { error: 'not found' });
}

const server = http.createServer(async (req, res) => {
  try {
    const refused = foreign(req);
    if (refused) return send(res, 403, { error: refused });
    const url = new URL(req.url, `http://${host}:${port}`);
    const p = decodeURIComponent(url.pathname);
    if (p === '/' || p === '/index.html') return send(res, 200, fs.readFileSync(path.join(HERE, 'app.html')), MIME['.html']);
    if (p.startsWith('/__dfw/static/')) {
      const f = safeJoin(HERE, p.slice('/__dfw/static/'.length));
      if (!f || !fs.existsSync(f)) return send(res, 404, { error: 'not found' });
      return send(res, 200, fs.readFileSync(f), MIME[path.extname(f)] || 'text/plain');
    }
    if (p === '/favicon.ico') return send(res, 204, '');
    if (p === '/__dfw/overlay.js') return send(res, 200, fs.readFileSync(path.join(HERE, 'overlay.js')), MIME['.js']);
    if (p.startsWith('/design/')) return serveDesignFile(res, p.slice('/design/'.length));
    if (p === '/__dfw/system.html') return send(res, 200, await systemPage(), MIME['.html']);
    if (p === '/__dfw/api/tokens') return send(res, 200, tokenList());
    if (p === '/__dfw/api/components') {
      return send(res, 200, [...loadComponents(root).values()].map(({ styles, template, examples, ...d }) => d));
    }
    if (p === '/__dfw/api/edit' && req.method === 'POST') return send(res, 200, applyEdit(root, await body(req)));
    if (p === '/__dfw/api/undo' && req.method === 'POST') return send(res, 200, undoEdit(root));

    if (p === '/__dfw/api/shoot' && req.method === 'POST') return send(res, 200, await shootFrames(await body(req)));
    if (p.startsWith('/__dfw/shots/')) {
      const f = safeJoin(path.join(P.dfw, 'shots'), p.slice('/__dfw/shots/'.length));
      if (!f || !f.endsWith('.png') || !fs.existsSync(f)) return send(res, 404, { error: 'not found' });
      return send(res, 200, fs.readFileSync(f), 'image/png');
    }
    if (p.startsWith('/__dfw/penpot/')) return await penpotRoute(req, res, p, url);
    if (p === '/__dfw/api/manifest') return send(res, 200, manifest());
    if (p === '/__dfw/api/state') {
      const state = loadState(root);
      return send(res, 200, { state, summary: summarize(state), session: currentSession(root, { create: false })?.session?.id || null });
    }
    if (p === '/__dfw/api/comments' && req.method === 'GET') return send(res, 200, allComments(root));
    if (p === '/__dfw/api/comments' && req.method === 'POST') {
      const c = addComment(root, await body(req));
      broadcast('comments', { id: c.id });
      return send(res, 201, c);
    }
    const m = p.match(/^\/__dfw\/api\/comments\/(fb_\d+)$/);
    if (m && req.method === 'PATCH') {
      const b = await body(req);
      // Requests from the studio UI are the user's own actions.
      const c = updateComment(root, m[1], { status: b.status, message: b.message, by: 'user' });
      broadcast('comments', { id: c.id });
      return send(res, 200, c);
    }
    if (p === '/__dfw/api/sessions' && req.method === 'POST') {
      const s = newSession(root);
      broadcast('comments', {});
      return send(res, 201, s.session);
    }
    if (p === '/__dfw/events') {
      res.writeHead(200, { 'Content-Type': 'text/event-stream', 'Cache-Control': 'no-store', Connection: 'keep-alive' });
      res.write('event: hello\ndata: {}\n\n');
      clients.add(res);
      req.on('close', () => clients.delete(res));
      return;
    }
    return send(res, 404, { error: 'not found' });
  } catch (err) {
    return send(res, err.exitCode ? 400 : 500, { error: err.message });
  }
});

// Live reload: watch design/ and .dfw/reviews/ (debounced).
// fs.watch misses events on some filesystems (CI runners, network drives, WSL, bind mounts), so a
// light mtime poll backs it up: a file whose mtime changed without a watch event is queued the same way.
let timer = null;
const pending = new Set();
const ignored = (file) => !file || String(file).startsWith('approved') || String(file).endsWith('.tmp') || String(file) === '.lock';
const seen = { design: new Map(), reviews: new Map() };
function mtime(file) {
  try {
    return fs.statSync(file).mtimeMs;
  } catch {
    return -1;
  }
}
function snapshot(dir, rel = '', out = new Map()) {
  let entries = [];
  try {
    entries = fs.readdirSync(path.join(dir, rel), { withFileTypes: true });
  } catch {
    return out;
  }
  for (const e of entries) {
    const r = rel ? path.join(rel, e.name) : e.name;
    if (ignored(r)) continue;
    if (e.isDirectory()) snapshot(dir, r, out);
    else if (e.isFile()) out.set(r, mtime(path.join(dir, r)));
  }
  return out;
}
function poll(dir, kind) {
  const now = snapshot(dir);
  const before = seen[kind];
  for (const [file, m] of now) if (before.get(file) !== m) queue(kind, file);
  for (const file of before.keys()) if (!now.has(file)) queue(kind, file);
  seen[kind] = now;
}
function queue(kind, file) {
  pending.add(`${kind}:${file}`);
  clearTimeout(timer);
  timer = setTimeout(flush, 150);
}
function watch(dir, kind) {
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  seen[kind] = snapshot(dir);
  setInterval(() => poll(dir, kind), 1000).unref();
  if (process.env.DFW_STUDIO_WATCH === 'poll') return; // polling only (slow or unreliable filesystems)
  try {
    fs.watch(dir, { recursive: true }, (_e, file) => {
      if (ignored(file)) return;
      seen[kind].set(String(file), mtime(path.join(dir, String(file)))); // the poll will not report it again
      queue(kind, file);
    });
  } catch (e) {
    console.error(`dfw studio: cannot watch ${dir} (polling only): ${e.message}`);
  }
}
function flush() {
  const files = [...pending];
  pending.clear();
  if (files.some((f) => /^design:system[\\/]components[\\/]/.test(f))) {
    try {
      buildSystem(root);
    } catch (e) {
      console.error(`dfw studio: buildSystem: ${e.message}`);
    }
  }
  if (files.some((f) => f.startsWith('design:'))) broadcast('design', { files: [...new Set(files.filter((f) => f.startsWith('design:')).map((f) => f.slice(7).split(path.sep).join('/')))] });
  if (files.some((f) => f.startsWith('reviews:'))) broadcast('comments', {});
}
try {
  buildSystem(root);
} catch (e) {
  console.error(`dfw studio: buildSystem: ${e.message}`);
}
watch(P.design, 'design');
watch(P.reviews, 'reviews');
setInterval(() => broadcast('ping', {}), 25000).unref();

server.listen(port, host, () => {
  console.log(`dfw studio: http://${host}:${port}/  (project: ${root})`);
});
server.on('error', (e) => {
  console.error(`dfw studio: ${e.message}`);
  process.exit(1);
});
