// dfw review: click-to-comment on the running app. A local reverse proxy (127.0.0.1 only) in
// front of the app's dev server: it injects the Studio overlay helpers (overlay.js) and the
// live review UI (studio/live.js) into HTML responses, passes everything else through
// (including WebSocket upgrades, so Vite/webpack/Next HMR keeps working) and writes comments
// to the same feedback contract as the Studio (.dfw/reviews/), so "dfw comments wait", the
// inbox and the hooks deliver them unchanged. Source file:line comes from code-inspector-plugin's
// data-insp-path attribute when the project has it (dfw inspector setup).
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import tls from 'node:tls';
import zlib from 'node:zlib';
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, readJson, DfwError, toPosix } from './util.mjs';
import { loadConfig } from './config.mjs';
import { addComment, setCommentScreenshot } from './comments.mjs';
import { parseInspPath } from './inspector.mjs';
import { screenshot } from './runtime.mjs';

const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1', '::1', '[::1]', '0.0.0.0']);
export const LIVE_TAGS = '<script defer src="/__dfw/overlay.js" data-dfw-live></script><script defer src="/__dfw/live.js"></script>';

/** The app URL must be a local http(s) server: dfw review never proxies to the network. */
export function assertLocalTarget(raw) {
  let u;
  try {
    u = new URL(raw);
  } catch {
    throw new DfwError(`Not a URL: ${raw}`);
  }
  if (!/^https?:$/.test(u.protocol)) throw new DfwError(`dfw review needs an http(s) URL, got ${u.protocol}`);
  const h = u.hostname.toLowerCase();
  if (!LOCAL_HOSTS.has(h) && !h.endsWith('.localhost') && !/^127\./.test(h)) {
    throw new DfwError(`dfw review only proxies a local dev server (localhost / 127.0.0.1), not ${u.hostname}.`);
  }
  return u;
}

/** Add the overlay + live review scripts to an HTML document (head first: React 19 tolerates extra head/body tags on hydration). */
export function injectLive(html) {
  if (html.includes('data-dfw-live')) return html;
  if (/<\/head>/i.test(html)) return html.replace(/<\/head>/i, `${LIVE_TAGS}</head>`);
  if (/<body[^>]*>/i.test(html)) return html.replace(/<body[^>]*>/i, (m) => m + LIVE_TAGS);
  return LIVE_TAGS + html;
}

function nearestBreakpoint(root, width) {
  const bps = readJson(projectPaths(root).manifest, {})?.breakpoints || loadConfig(root).studio.breakpoints || [];
  if (!bps.length || !width) return null;
  return bps.reduce((a, b) => (Math.abs(b.width - width) < Math.abs(a.width - width) ? b : a)).id;
}

function decode(buf, enc) {
  if (!enc || enc === 'identity') return buf;
  if (enc === 'gzip' || enc === 'x-gzip') return zlib.gunzipSync(buf);
  if (enc === 'br') return zlib.brotliDecompressSync(buf);
  if (enc === 'deflate') return zlib.inflateSync(buf);
  return null; // unknown: pass through untouched
}

async function readBody(req, limit = 1_000_000) {
  const chunks = [];
  let n = 0;
  for await (const c of req) {
    n += c.length;
    if (n > limit) throw new DfwError('Request too large.');
    chunks.push(c);
  }
  const s = Buffer.concat(chunks).toString('utf8');
  return s ? JSON.parse(s) : {};
}

/**
 * Create (not start) the review proxy.
 * opts: { root, target: 'http://localhost:5173', port, shots: true|false, log }
 */
export function createLiveServer({ root, target, shots = true, log = () => {} }) {
  const t = assertLocalTarget(target);
  const P = projectPaths(root);
  const targetOrigins = new Set([t.origin, t.origin.replace('localhost', '127.0.0.1'), t.origin.replace('127.0.0.1', 'localhost')]);
  const agent = t.protocol === 'https:' ? https : http;
  const tport = Number(t.port) || (t.protocol === 'https:' ? 443 : 80);
  let shotChain = Promise.resolve();

  const send = (res, code, body, type = 'application/json; charset=utf-8') => {
    res.writeHead(code, { 'Content-Type': type, 'Cache-Control': 'no-store' });
    res.end(typeof body === 'string' || Buffer.isBuffer(body) ? body : JSON.stringify(body));
  };

  function queueShot(c, route, vw, vh) {
    if (!shots) return;
    shotChain = shotChain
      .then(async () => {
        const out = path.join(P.dfw, 'shots', 'live', `${c.id}.png`);
        // The app itself (not the proxy): what the route renders, without the review UI.
        const [r] = await screenshot([{ url: new URL(route || '/', t.origin).href, width: vw, height: vh, out, fullPage: true }], { concurrency: 1 });
        if (r.ok) setCommentScreenshot(root, c.id, toPosix(path.relative(root, out)));
        else log(`screenshot for ${c.id} failed: ${r.error}`);
      })
      .catch((e) => log(`screenshot for ${c.id} failed: ${e.message}`));
  }

  async function postComment(req, res) {
    const b = await readBody(req);
    const target = { ...(b.target || {}) };
    const insp = typeof target.insp === 'string' ? target.insp : null;
    delete target.insp;
    // Only a parsed data-insp-path becomes the source; without it the selector + text identify the element.
    target.source = parseInspPath(insp, root)?.source || null;
    if (insp) target.inspPath = insp;
    const vw = Math.max(200, Math.min(4000, Math.round(Number(b.viewport?.[0]) || 1440)));
    const vh = Math.max(200, Math.min(4000, Math.round(Number(b.viewport?.[1]) || 900)));
    const route = typeof b.url === 'string' && b.url.startsWith('/') ? b.url : '/';
    const c = addComment(root, {
      comment: b.comment,
      screen: typeof b.screen === 'string' ? b.screen : route.split('#')[0],
      variant: 'live',
      breakpoint: nearestBreakpoint(root, vw),
      url: route,
      target,
      provider: 'dfw-live',
    });
    queueShot(c, route, vw, vh);
    return send(res, 201, c);
  }

  function proxy(req, res) {
    const headers = { ...req.headers, host: t.host };
    delete headers['accept-encoding']; // keep HTML readable so the overlay can be injected
    if (headers.origin) headers.origin = t.origin;
    const self = `http://127.0.0.1:${server.address().port}`;
    if (headers.referer) headers.referer = headers.referer.replace(self, t.origin);
    const preq = agent.request(
      { hostname: t.hostname.replace(/^\[|\]$/g, ''), port: tport, path: req.url, method: req.method, headers, rejectUnauthorized: false },
      (pres) => {
        const h = { ...pres.headers };
        if (h.location) for (const o of targetOrigins) if (h.location.startsWith(o)) h.location = self + h.location.slice(o.length);
        const html = /text\/html/i.test(h['content-type'] || '') && req.method === 'GET' && pres.statusCode === 200;
        if (!html) {
          res.writeHead(pres.statusCode, h);
          return pres.pipe(res);
        }
        const chunks = [];
        pres.on('data', (c) => chunks.push(c));
        pres.on('end', () => {
          const raw = decode(Buffer.concat(chunks), h['content-encoding']);
          if (!raw) {
            res.writeHead(pres.statusCode, h);
            return res.end(Buffer.concat(chunks));
          }
          const out = Buffer.from(injectLive(raw.toString('utf8')));
          for (const k of ['content-length', 'content-encoding', 'transfer-encoding', 'etag', 'last-modified']) delete h[k];
          h['content-length'] = out.length;
          h['cache-control'] = 'no-store';
          res.writeHead(pres.statusCode, h);
          res.end(out);
        });
        pres.on('error', () => res.destroy());
      },
    );
    preq.on('error', (e) => {
      if (res.headersSent) return res.destroy();
      send(res, 502, `<!doctype html><meta charset="utf-8"><title>dfw review</title><body style="font:15px system-ui;padding:40px"><h1>dfw review: the app is not reachable</h1><p>${t.origin}: ${String(e.message).replace(/[<&]/g, '')}</p><p>Start the dev server, then reload this page.</p>`, 'text/html; charset=utf-8');
    });
    req.pipe(preq);
  }

  const server = http.createServer(async (req, res) => {
    try {
      const p = new URL(req.url, 'http://127.0.0.1').pathname;
      if (p === '/__dfw/overlay.js') return send(res, 200, fs.readFileSync(path.join(PLUGIN_ROOT, 'studio', 'overlay.js')), 'text/javascript; charset=utf-8');
      if (p === '/__dfw/live.js') return send(res, 200, fs.readFileSync(path.join(PLUGIN_ROOT, 'studio', 'live.js')), 'text/javascript; charset=utf-8');
      if (p === '/__dfw/api/live') return send(res, 200, { target: t.origin, shots });
      if (p === '/__dfw/api/comments' && req.method === 'POST') return await postComment(req, res);
      return proxy(req, res);
    } catch (e) {
      if (!res.headersSent) send(res, e.exitCode ? 400 : 500, { error: e.message });
    }
  });

  // WebSocket (HMR) passthrough: replay the upgrade request to the app and splice the sockets.
  server.on('upgrade', (req, socket, head) => {
    const connect = t.protocol === 'https:' ? () => tls.connect({ host: t.hostname, port: tport, servername: t.hostname, rejectUnauthorized: false }) : () => net.connect(tport, t.hostname.replace(/^\[|\]$/g, ''));
    const up = connect();
    up.once(t.protocol === 'https:' ? 'secureConnect' : 'connect', () => {
      const lines = [`${req.method} ${req.url} HTTP/${req.httpVersion}`];
      for (let i = 0; i < req.rawHeaders.length; i += 2) {
        const k = req.rawHeaders[i];
        let v = req.rawHeaders[i + 1];
        if (/^host$/i.test(k)) v = t.host;
        else if (/^origin$/i.test(k)) v = t.origin;
        lines.push(`${k}: ${v}`);
      }
      up.write(`${lines.join('\r\n')}\r\n\r\n`);
      if (head?.length) up.write(head);
      up.pipe(socket);
      socket.pipe(up);
    });
    up.on('error', () => socket.destroy());
    socket.on('error', () => up.destroy());
    up.on('close', () => socket.destroy());
    socket.on('close', () => up.destroy());
  });
  return server;
}

/** Start the proxy on 127.0.0.1:<port>. Resolves to { server, url }. */
export function startLiveServer(opts) {
  const server = createLiveServer(opts);
  return new Promise((resolve, reject) => {
    server.once('error', reject);
    server.listen(opts.port, '127.0.0.1', () => resolve({ server, url: `http://127.0.0.1:${server.address().port}/` }));
  });
}
