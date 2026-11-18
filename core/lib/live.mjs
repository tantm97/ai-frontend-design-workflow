// dfw review: click-to-comment on the running app. A local reverse proxy (loopback only) in
// front of the app's dev server: it injects the Studio overlay helpers (overlay.js) and the
// live review UI (studio/live.js) into HTML responses, passes everything else through
// (including WebSocket upgrades, so Vite/webpack/Next HMR keeps working) and writes comments
// to the same feedback contract as the Studio (.dfw/reviews/), so "dfw comments wait", the
// inbox and the hooks deliver them unchanged. Source file:line comes from code-inspector-plugin's
// data-insp-path attribute when the project has it (dfw inspector setup).
// A strict Content-Security-Policy gets the proxy origin + a per-response nonce for the injected
// scripts only; for a localhost app the proxy is served as http://localhost:<port>/ so the app's
// localhost cookies (host-scoped, not port-scoped) reach it.
import http from 'node:http';
import https from 'node:https';
import net from 'node:net';
import tls from 'node:tls';
import zlib from 'node:zlib';
import crypto from 'node:crypto';
import fs from 'node:fs';
import path from 'node:path';
import { PLUGIN_ROOT, projectPaths, readJson, DfwError, toPosix } from './util.mjs';
import { loadConfig } from './config.mjs';
import { addComment, setCommentScreenshot } from './comments.mjs';
import { parseInspPath } from './inspector.mjs';
import { screenshot } from './runtime.mjs';

const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1', '::1', '[::1]', '0.0.0.0']);
export const DEFAULT_SHORTCUT = 'alt+c';

/**
 * Normalise a review shortcut ("Alt+C", "option+c", "ctrl+shift+k", "c", "off") to
 * "ctrl+alt+shift+meta+<key>" order. One letter or digit plus optional modifiers; "off" disables it.
 */
export function normalizeShortcut(raw) {
  const s = String(raw ?? '').trim().toLowerCase();
  if (!s || s === 'off' || s === 'none' || s === 'false') return 'off';
  const MODS = { alt: 'alt', option: 'alt', opt: 'alt', ctrl: 'ctrl', control: 'ctrl', meta: 'meta', cmd: 'meta', command: 'meta', shift: 'shift' };
  const mods = new Set();
  let key = null;
  let bad = false;
  for (const part of s.split('+').map((x) => x.trim())) {
    if (MODS[part]) mods.add(MODS[part]);
    else if (/^[a-z0-9]$/.test(part) && !key) key = part;
    else bad = true;
  }
  if (!key || bad) throw new DfwError(`Invalid review shortcut "${raw}": use one letter or digit with optional modifiers (alt, ctrl, shift, meta), e.g. alt+c, ctrl+shift+k, or off.`);
  return [...['ctrl', 'alt', 'shift', 'meta'].filter((m) => mods.has(m)), key].join('+');
}

const attr = (v) => String(v).replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;');

/** The script tags injected into the app's HTML (nonce: the per-response CSP nonce, if any). */
export function liveTags({ nonce = null, shortcut = DEFAULT_SHORTCUT } = {}) {
  const n = nonce ? ` nonce="${attr(nonce)}"` : '';
  return `<script defer src="/__dfw/overlay.js" data-dfw-live${n}></script><script defer src="/__dfw/live.js" data-shortcut="${attr(shortcut)}"${n}></script>`;
}
export const LIVE_TAGS = liveTags();

/** Host the review URL is served with: localhost for a localhost app (cookies are host-scoped, not port-scoped). */
export function proxyHost(target) {
  return new URL(target).hostname.toLowerCase() === 'localhost' ? 'localhost' : '127.0.0.1';
}

/**
 * Let the injected scripts through a Content-Security-Policy value (header or meta; possibly several
 * comma-separated policies) without weakening anything else: the proxy origin, and a per-response
 * nonce where host sources are not enough ('strict-dynamic', nonce/hash lists), are added to the
 * directive that governs <script src> and to the one that governs fetch() (copied from default-src
 * when absent). Nothing is removed, except a lone 'none' that now gets a source.
 */
export function patchCsp(value, { origin, nonce }) {
  if (!value) return value;
  return String(value)
    .split(',')
    .map((policy) => {
      const dirs = policy
        .split(';')
        .map((d) => d.trim())
        .filter(Boolean)
        .map((d) => {
          const [name, ...list] = d.split(/\s+/);
          return { name: name.toLowerCase(), list };
        });
      if (!dirs.length) return policy;
      const get = (n) => dirs.find((d) => d.name === n);
      const def = get('default-src');
      const ensure = (n) => {
        if (get(n)) return [get(n)];
        if (!def) return []; // no directive and no default-src: not restricted
        const d = { name: n, list: [...def.list] };
        dirs.push(d);
        return [d];
      };
      const grow = (d, withNonce) => {
        const low = d.list.map((x) => x.toLowerCase());
        if (low.length === 1 && low[0] === "'none'") d.list = [];
        const nonceOrHash = low.some((x) => /^'(nonce|sha256|sha384|sha512)-/.test(x));
        if (!low.includes("'self'") && !low.includes(origin.toLowerCase())) d.list.push(origin);
        // A nonce makes browsers ignore 'unsafe-inline': only add one where that changes nothing for the app.
        if (withNonce && nonce && (nonceOrHash || !low.includes("'unsafe-inline'"))) d.list.push(`'nonce-${nonce}'`);
      };
      const scriptDirs = ['script-src-elem', 'script-src'].map(get).filter(Boolean);
      for (const d of scriptDirs.length ? scriptDirs : ensure('script-src')) grow(d, true);
      for (const d of ensure('connect-src')) grow(d, false);
      return dirs.map((d) => [d.name, ...d.list].join(' ')).join('; ');
    })
    .join(', ');
}

/** Same for <meta http-equiv="Content-Security-Policy" content="..."> in the HTML. */
export function patchMetaCsp(html, opts) {
  return html.replace(/<meta\b[^>]*http-equiv\s*=\s*["']?content-security-policy["']?[^>]*>/gi, (tag) =>
    tag.replace(/\bcontent\s*=\s*(?:"([^"]*)"|'([^']*)')/i, (_, dq, sq) => {
      const v = (dq ?? sq).replace(/&#39;|&apos;/g, "'").replace(/&quot;/g, '"').replace(/&amp;/g, '&');
      return `content="${attr(patchCsp(v, opts))}"`;
    }),
  );
}

/** Drop a Set-Cookie Domain that names a different local host than the one the browser uses (127.0.0.1 vs localhost). */
export function fixSetCookie(cookies, host) {
  const h = host.toLowerCase();
  return [].concat(cookies).map((c) =>
    c.replace(/;\s*domain\s*=\s*\.?([^;]*)/gi, (m, d) => {
      const dom = d.trim().toLowerCase();
      return LOCAL_HOSTS.has(dom) && dom !== h ? '' : m;
    }),
  );
}

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
export function injectLive(html, opts = {}) {
  if (html.includes('data-dfw-live')) return html;
  const tags = liveTags(opts);
  if (/<\/head>/i.test(html)) return html.replace(/<\/head>/i, `${tags}</head>`);
  if (/<body[^>]*>/i.test(html)) return html.replace(/<body[^>]*>/i, (m) => m + tags);
  return tags + html;
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
 * opts: { root, target: 'http://localhost:5173', shots: true|false, shortcut: 'alt+c', log }
 * shortcut defaults to the "review.shortcut" config value.
 */
export function createLiveServer({ root, target, shots = true, shortcut, log = () => {} }) {
  const t = assertLocalTarget(target);
  const P = projectPaths(root);
  const keys = normalizeShortcut(shortcut ?? loadConfig(root).review?.shortcut ?? DEFAULT_SHORTCUT);
  const targetOrigins = new Set([t.origin, t.origin.replace('localhost', '127.0.0.1'), t.origin.replace('127.0.0.1', 'localhost')]);
  const agent = t.protocol === 'https:' ? https : http;
  const tport = Number(t.port) || (t.protocol === 'https:' ? 443 : 80);
  let shotChain = Promise.resolve();

  // The origin the browser sees: the request's Host when it is a loopback name, so redirects, cookies
  // and the CSP source match whether the page was opened as localhost or 127.0.0.1.
  const selfOf = (req, port) => {
    const h = String(req.headers.host || '');
    return /^(localhost|127\.0\.0\.1|\[::1\]):\d+$/i.test(h) ? `http://${h.toLowerCase()}` : `http://${server.publicHost}:${port}`;
  };

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
    const self = selfOf(req, req.socket.localPort);
    if (headers.referer?.startsWith(self)) headers.referer = t.origin + headers.referer.slice(self.length);
    const preq = agent.request(
      { hostname: t.hostname.replace(/^\[|\]$/g, ''), port: tport, path: req.url, method: req.method, headers, rejectUnauthorized: false },
      (pres) => {
        const h = { ...pres.headers };
        if (h.location) for (const o of targetOrigins) if (h.location.startsWith(o)) h.location = self + h.location.slice(o.length);
        if (h['set-cookie']) h['set-cookie'] = fixSetCookie(h['set-cookie'], new URL(self).hostname);
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
          const csp = { origin: self, nonce: crypto.randomBytes(16).toString('base64') };
          for (const k of ['content-security-policy', 'content-security-policy-report-only']) if (h[k]) h[k] = patchCsp([].concat(h[k]).join(', '), csp);
          const out = Buffer.from(injectLive(patchMetaCsp(raw.toString('utf8'), csp), { nonce: csp.nonce, shortcut: keys }));
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

  // Local user only: a rebinding page must not reach the app through the proxy (the proxy rewrites Host,
  // which would defeat the dev server's own host check), and another site must not post comments.
  const LOOPBACK = /^(localhost|127\.0\.0\.1|\[::1\]):\d+$/i;
  const foreign = (req) => {
    if (!LOOPBACK.test(String(req.headers.host || ''))) return `host ${req.headers.host || '(none)'} not allowed`;
    const from = req.headers.origin;
    if (!['GET', 'HEAD', 'OPTIONS'].includes(req.method) && from && !LOOPBACK.test(from.replace(/^https?:\/\//, ''))) return `origin ${from} not allowed`;
    return null;
  };

  async function onRequest(req, res) {
    try {
      const refused = foreign(req);
      if (refused) return send(res, 403, { error: refused });
      const p = new URL(req.url, 'http://127.0.0.1').pathname;
      if (p === '/__dfw/overlay.js') return send(res, 200, fs.readFileSync(path.join(PLUGIN_ROOT, 'studio', 'overlay.js')), 'text/javascript; charset=utf-8');
      if (p === '/__dfw/live.js') return send(res, 200, fs.readFileSync(path.join(PLUGIN_ROOT, 'studio', 'live.js')), 'text/javascript; charset=utf-8');
      if (p === '/__dfw/api/live') return send(res, 200, { target: t.origin, url: `http://${server.publicHost}:${req.socket.localPort}/`, shots, shortcut: keys });
      if (p === '/__dfw/api/comments' && req.method === 'POST') return await postComment(req, res);
      return proxy(req, res);
    } catch (e) {
      if (!res.headersSent) send(res, e.exitCode ? 400 : 500, { error: e.message });
    }
  }

  // WebSocket (HMR) passthrough: replay the upgrade request to the app and splice the sockets.
  function onUpgrade(req, socket, head) {
    if (!LOOPBACK.test(String(req.headers.host || ''))) return socket.destroy();
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
  }

  const make = () => http.createServer(onRequest).on('upgrade', onUpgrade);
  const server = make();
  server.twin = make; // a second listener (IPv6 loopback) with the same handlers
  server.publicHost = proxyHost(t.href);
  return server;
}

const listen = (srv, port, host) =>
  new Promise((resolve, reject) => {
    srv.once('error', reject);
    srv.listen(port, host, () => {
      srv.off('error', reject);
      resolve();
    });
  });

/**
 * Start the proxy on 127.0.0.1:<port> (never 0.0.0.0). For a localhost app it is served as
 * http://localhost:<port>/ and also listens on [::1]:<port>, since "localhost" may resolve to ::1
 * first (macOS). Resolves to { server, url }.
 */
export async function startLiveServer(opts) {
  const server = createLiveServer(opts);
  const log = opts.log || (() => {});
  await listen(server, opts.port, '127.0.0.1');
  const port = server.address().port;
  let host = server.publicHost;
  if (host === 'localhost') {
    const v6 = server.twin();
    try {
      await listen(v6, port, '::1');
      server.on('close', () => v6.close());
    } catch (e) {
      // No IPv6 loopback: localhost resolves to 127.0.0.1 only, fine. Anything else on [::1]:port
      // could answer "localhost" instead of us: fall back to the unambiguous 127.0.0.1 URL.
      if (e.code === 'EADDRINUSE') {
        host = server.publicHost = '127.0.0.1';
        log(`[::1]:${port} is in use; serving http://127.0.0.1:${port}/ (cookies set by the app on localhost will not reach it)`);
      }
    }
  }
  return { server, url: `http://${host}:${port}/` };
}
