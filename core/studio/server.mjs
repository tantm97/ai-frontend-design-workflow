// dfw Design Studio: a local, dependency-free web server that renders the
// project's design workspace (design/) as a multi-screen canvas and collects
// element-level review comments. Implementation detail only: the studio is
// plain HTML/CSS/JS and does not influence the product's frontend stack.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { projectPaths, readJson, parseArgs, listFiles } from '../lib/util.mjs';
import { loadConfig } from '../lib/config.mjs';
import { loadState, summarize } from '../lib/state.mjs';
import { addComment, allComments, updateComment, newSession, currentSession } from '../lib/comments.mjs';

const HERE = path.dirname(fileURLToPath(import.meta.url));
const args = parseArgs(process.argv.slice(2));
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
  if (m) return { ...base, ...m, breakpoints: m.breakpoints || base.breakpoints };
  const screens = new Map();
  for (const rel of listFiles(path.join(P.design, 'screens'))) {
    if (!rel.endsWith('.html')) continue;
    const parts = rel.split('/');
    const id = parts.length > 1 ? parts.slice(0, -1).join('/') : parts[0].replace(/\.html$/, '');
    const variant = parts.length > 1 ? parts.at(-1).replace(/\.html$/, '') : 'a';
    if (!screens.has(id)) screens.set(id, { id, title: id, variants: [] });
    screens.get(id).variants.push({ id: variant, label: variant, file: `screens/${rel}` });
  }
  return { ...base, product: path.basename(root), inferred: true, screens: [...screens.values()] };
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
  return abs.startsWith(base) ? abs : null;
}

const OVERLAY_TAG = '<script src="/__dfw/overlay.js" data-dfw-overlay></script>';

function serveDesignFile(res, rel) {
  const abs = safeJoin(P.design, rel);
  if (!abs || !fs.existsSync(abs) || fs.statSync(abs).isDirectory()) return send(res, 404, { error: 'not found' });
  const ext = path.extname(abs).toLowerCase();
  if (ext === '.html') {
    let html = fs.readFileSync(abs, 'utf8');
    html = /<\/body>/i.test(html) ? html.replace(/<\/body>/i, `${OVERLAY_TAG}</body>`) : html + OVERLAY_TAG;
    return send(res, 200, html, MIME['.html']);
  }
  return send(res, 200, fs.readFileSync(abs), MIME[ext] || 'application/octet-stream');
}

const server = http.createServer(async (req, res) => {
  try {
    const url = new URL(req.url, `http://${host}:${port}`);
    const p = decodeURIComponent(url.pathname);
    if (p === '/' || p === '/index.html') return send(res, 200, fs.readFileSync(path.join(HERE, 'app.html')), MIME['.html']);
    if (p.startsWith('/__dfw/static/')) {
      const f = safeJoin(HERE, p.slice('/__dfw/static/'.length));
      if (!f || !fs.existsSync(f)) return send(res, 404, { error: 'not found' });
      return send(res, 200, fs.readFileSync(f), MIME[path.extname(f)] || 'text/plain');
    }
    if (p === '/__dfw/overlay.js') return send(res, 200, fs.readFileSync(path.join(HERE, 'overlay.js')), MIME['.js']);
    if (p.startsWith('/design/')) return serveDesignFile(res, p.slice('/design/'.length));

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
let timer = null;
const pending = new Set();
function watch(dir, kind) {
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
  try {
    fs.watch(dir, { recursive: true }, (_e, file) => {
      if (!file || String(file).startsWith('approved')) return;
      pending.add(`${kind}:${file}`);
      clearTimeout(timer);
      timer = setTimeout(() => {
        const files = [...pending];
        pending.clear();
        if (files.some((f) => f.startsWith('design:'))) broadcast('design', { files });
        if (files.some((f) => f.startsWith('reviews:'))) broadcast('comments', {});
      }, 150);
    });
  } catch (e) {
    console.error(`dfw studio: cannot watch ${dir}: ${e.message}`);
  }
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
