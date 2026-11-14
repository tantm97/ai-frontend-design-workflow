// Minimal Chrome DevTools Protocol driver for dfw's renders (layout check, screenshots).
// It drives a Chrome / Edge / Chromium that is already on the machine, or Google's
// "chrome-headless-shell" downloaded once by "dfw setup --browser". No npm packages:
// one browser-level WebSocket (Node's built-in WebSocket) with flattened sessions.
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import crypto from 'node:crypto';
import { spawn } from 'node:child_process';
import { globalDir, DfwError } from './util.mjs';

/* ---------------------------------------------------------------- finding a browser */

export function browserDir() {
  return path.join(globalDir(), 'browser');
}

/** The headless shell downloaded by "dfw setup --browser", if any. */
function downloadedBrowser() {
  try {
    const cur = JSON.parse(fs.readFileSync(path.join(browserDir(), 'current.json'), 'utf8'));
    if (cur.path && fs.existsSync(cur.path)) return { path: cur.path, source: 'downloaded', version: cur.version };
  } catch {}
  return null;
}

function systemCandidates() {
  const home = os.homedir();
  if (process.platform === 'darwin') {
    const apps = ['Google Chrome', 'Google Chrome Beta', 'Google Chrome Dev', 'Google Chrome Canary', 'Chromium', 'Microsoft Edge', 'Brave Browser'];
    return apps.flatMap((a) => [`/Applications/${a}.app/Contents/MacOS/${a}`, path.join(home, `Applications/${a}.app/Contents/MacOS/${a}`)]);
  }
  if (process.platform === 'win32') {
    const roots = [process.env['PROGRAMFILES'], process.env['PROGRAMFILES(X86)'], process.env.LOCALAPPDATA].filter(Boolean);
    const rels = ['Google\\Chrome\\Application\\chrome.exe', 'Microsoft\\Edge\\Application\\msedge.exe', 'Chromium\\Application\\chrome.exe', 'BraveSoftware\\Brave-Browser\\Application\\brave.exe'];
    return roots.flatMap((r) => rels.map((x) => path.join(r, x)));
  }
  const names = ['google-chrome', 'google-chrome-stable', 'chromium', 'chromium-browser', 'microsoft-edge', 'microsoft-edge-stable', 'brave-browser'];
  const dirs = (process.env.PATH || '').split(path.delimiter).concat(['/usr/bin', '/usr/local/bin', '/snap/bin', '/opt/google/chrome']);
  return names.flatMap((n) => dirs.map((d) => path.join(d, n)));
}

/** First usable browser: $DFW_BROWSER, then the downloaded headless shell, then an installed browser. */
export function findBrowser() {
  const env = process.env.DFW_BROWSER || process.env.CHROME_PATH;
  if (env && fs.existsSync(env)) return { path: env, source: 'env' };
  const dl = downloadedBrowser();
  if (dl) return dl;
  for (const p of systemCandidates()) {
    try {
      if (fs.statSync(p).isFile()) return { path: p, source: 'system' };
    } catch {}
  }
  return null;
}

export function browserStatus() {
  const b = findBrowser();
  return b ? { installed: true, ...b } : { installed: false, detail: 'no Chrome, Edge or Chromium found (dfw setup --browser downloads a headless one)' };
}

/* ---------------------------------------------------------------- protocol */

class Connection {
  constructor(ws) {
    this.ws = ws;
    this.seq = 0;
    this.pending = new Map();
    this.listeners = new Map(); // sessionId ('' = browser) -> Set(fn(method, params))
    this.closed = false;
    ws.addEventListener('message', (e) => this.onMessage(typeof e.data === 'string' ? e.data : Buffer.from(e.data).toString('utf8')));
    ws.addEventListener('close', () => {
      this.closed = true;
      for (const { reject } of this.pending.values()) reject(new Error('browser connection closed'));
      this.pending.clear();
    });
  }

  onMessage(raw) {
    const m = JSON.parse(raw);
    if (m.id != null) {
      const p = this.pending.get(m.id);
      if (!p) return;
      this.pending.delete(m.id);
      if (m.error) p.reject(new Error(`${p.method}: ${m.error.message}`));
      else p.resolve(m.result || {});
      return;
    }
    for (const fn of this.listeners.get(m.sessionId || '') || []) fn(m.method, m.params || {});
  }

  send(method, params = {}, sessionId) {
    if (this.closed) return Promise.reject(new Error('browser connection closed'));
    const id = ++this.seq;
    return new Promise((resolve, reject) => {
      this.pending.set(id, { resolve, reject, method });
      this.ws.send(JSON.stringify(sessionId ? { id, method, params, sessionId } : { id, method, params }));
    });
  }

  on(sessionId, fn) {
    const k = sessionId || '';
    if (!this.listeners.has(k)) this.listeners.set(k, new Set());
    this.listeners.get(k).add(fn);
    return () => this.listeners.get(k)?.delete(fn);
  }
}

/* ---------------------------------------------------------------- web font disk cache */

// Google Fonts CSS and font files are immutable per URL: serve them from disk so a render
// never waits on the network after the first one (each page paid ~2 s for fonts before).
const FONT_PATTERNS = [{ urlPattern: 'https://fonts.googleapis.com/*' }, { urlPattern: 'https://fonts.gstatic.com/*' }];

export function fontCacheDir() {
  return path.join(globalDir(), 'cache', 'fonts');
}

async function serveFont(conn, sessionId, { requestId, request }) {
  const key = crypto.createHash('sha1').update(request.url).digest('hex');
  const dir = fontCacheDir();
  const body = path.join(dir, key);
  const meta = `${body}.json`;
  const fulfill = (buf, contentType) =>
    conn.send(
      'Fetch.fulfillRequest',
      {
        requestId,
        responseCode: 200,
        responseHeaders: [
          { name: 'Content-Type', value: contentType },
          { name: 'Access-Control-Allow-Origin', value: '*' },
          { name: 'Cache-Control', value: 'public, max-age=31536000' },
        ],
        body: buf.toString('base64'),
      },
      sessionId,
    );
  try {
    if (fs.existsSync(body) && fs.existsSync(meta)) {
      const { contentType } = JSON.parse(fs.readFileSync(meta, 'utf8'));
      return await fulfill(fs.readFileSync(body), contentType);
    }
    // Same headers as the browser (Google Fonts picks the font format from the User-Agent).
    const headers = Object.fromEntries(Object.entries(request.headers || {}).filter(([k]) => !/^(host|connection|content-length|accept-encoding)$/i.test(k)));
    const res = await fetch(request.url, { headers, signal: AbortSignal.timeout(15000) });
    const buf = Buffer.from(await res.arrayBuffer());
    const contentType = res.headers.get('content-type') || 'application/octet-stream';
    if (res.ok) {
      fs.mkdirSync(dir, { recursive: true });
      fs.writeFileSync(body, buf);
      fs.writeFileSync(meta, JSON.stringify({ url: request.url, contentType }));
    }
    return await fulfill(buf, contentType);
  } catch {
    // Offline or blocked: the page falls back to its system font stack.
    return conn.send('Fetch.failRequest', { requestId, errorReason: 'InternetDisconnected' }, sessionId).catch(() => {});
  }
}

/* ---------------------------------------------------------------- browser / context / page */

class Page {
  constructor(conn, targetId, sessionId, scale) {
    this.conn = conn;
    this.targetId = targetId;
    this.sessionId = sessionId;
    this.scale = scale;
    this.width = 1280;
    this.height = 800;
  }

  send(method, params) {
    return this.conn.send(method, params, this.sessionId);
  }

  waitFor(event, timeoutMs) {
    return new Promise((resolve, reject) => {
      const t = setTimeout(() => {
        off();
        reject(new Error(`timeout ${timeoutMs}ms waiting for ${event}`));
      }, timeoutMs);
      const off = this.conn.on(this.sessionId, (method, params) => {
        if (method !== event) return;
        clearTimeout(t);
        off();
        resolve(params);
      });
    });
  }

  async setViewportSize({ width, height }) {
    this.width = width;
    this.height = height;
    await this.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: this.scale, mobile: false });
  }

  async goto(url, { timeout = 30000 } = {}) {
    const loaded = this.waitFor('Page.loadEventFired', timeout);
    const r = await this.send('Page.navigate', { url });
    if (r.errorText) {
      loaded.catch(() => {});
      throw new Error(`cannot open ${url}: ${r.errorText}`);
    }
    await loaded;
  }

  /** Run a self-contained function in the page (like Playwright's page.evaluate). */
  async evaluate(fn, arg) {
    const expression = `(${typeof fn === 'function' ? fn.toString() : `() => (${fn})`})(${arg === undefined ? '' : JSON.stringify(arg)})`;
    const r = await this.send('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || r.exceptionDetails.text || 'evaluation failed');
    return r.result?.value;
  }

  waitForTimeout(ms) {
    return new Promise((r) => setTimeout(r, ms));
  }

  async screenshot({ path: out, fullPage = false, selector } = {}) {
    let clip = null;
    if (selector) {
      const r = await this.evaluate((sel) => {
        const el = document.querySelector(sel);
        if (!el) return null;
        el.scrollIntoView({ block: 'center', inline: 'nearest' });
        const b = el.getBoundingClientRect();
        return { x: b.left + scrollX, y: b.top + scrollY, width: b.width, height: b.height };
      }, selector);
      if (!r || !r.width || !r.height) throw new Error(`selector not found: ${selector}`);
      clip = { ...r, scale: 1 };
    } else if (fullPage) {
      const m = await this.send('Page.getLayoutMetrics');
      const size = m.cssContentSize || m.contentSize;
      clip = { x: 0, y: 0, width: this.width, height: Math.max(this.height, Math.ceil(size.height)), scale: 1 };
    }
    const shot = await this.send('Page.captureScreenshot', clip ? { format: 'png', clip, captureBeyondViewport: true } : { format: 'png' });
    const buf = Buffer.from(shot.data, 'base64');
    if (out) {
      fs.mkdirSync(path.dirname(out), { recursive: true });
      fs.writeFileSync(out, buf);
    }
    return buf;
  }

  async close() {
    await this.conn.send('Target.closeTarget', { targetId: this.targetId }).catch(() => {});
  }
}

class Context {
  constructor(conn, id, { deviceScaleFactor = 1, cacheFonts = true } = {}) {
    this.conn = conn;
    this.id = id;
    this.scale = deviceScaleFactor;
    this.cacheFonts = cacheFonts;
  }

  async newPage() {
    const { targetId } = await this.conn.send('Target.createTarget', { url: 'about:blank', browserContextId: this.id });
    const { sessionId } = await this.conn.send('Target.attachToTarget', { targetId, flatten: true });
    const page = new Page(this.conn, targetId, sessionId, this.scale);
    if (this.cacheFonts) {
      this.conn.on(sessionId, (method, params) => {
        if (method === 'Fetch.requestPaused') serveFont(this.conn, sessionId, params);
      });
      await page.send('Fetch.enable', { patterns: FONT_PATTERNS });
    }
    await page.send('Page.enable');
    return page;
  }
}

class Browser {
  constructor(proc, conn, dataDir) {
    this.proc = proc;
    this.conn = conn;
    this.dataDir = dataDir;
  }

  async newContext(opts = {}) {
    const { browserContextId } = await this.conn.send('Target.createBrowserContext', { disposeOnDetach: true });
    return new Context(this.conn, browserContextId, opts);
  }

  async close() {
    if (this.proc.exitCode == null) {
      // Browser.close often gets no reply (the socket dies first): wait for the process instead.
      const exited = new Promise((r) => this.proc.once('exit', r));
      this.conn.send('Browser.close').catch(() => {});
      const timer = (ms) => new Promise((r) => setTimeout(r, ms).unref());
      await Promise.race([exited, timer(1500)]);
      if (this.proc.exitCode == null) {
        this.proc.kill();
        await Promise.race([exited, timer(1500)]);
      }
    }
    try {
      this.conn.ws.close();
    } catch {}
    // Our own temporary profile directory (created by mkdtemp below).
    try {
      fs.rmSync(this.dataDir, { recursive: true, force: true, maxRetries: 3 });
    } catch {}
  }
}

/** Start a headless browser and connect to it. */
export async function launch({ timeoutMs = 20000 } = {}) {
  if (typeof WebSocket !== 'function') throw new DfwError(`dfw renders need Node.js 22 or newer (found ${process.versions.node}), or the dfw binary (dfw setup --binary).`);
  const b = findBrowser();
  if (!b) throw new DfwError('No Chrome, Edge or Chromium found. Install one, set DFW_BROWSER=/path/to/chrome, or run: dfw setup --browser');
  const dataDir = fs.mkdtempSync(path.join(os.tmpdir(), 'dfw-browser-'));
  const args = [
    '--headless',
    '--remote-debugging-port=0',
    `--user-data-dir=${dataDir}`,
    '--no-first-run',
    '--no-default-browser-check',
    '--disable-extensions',
    '--disable-background-networking',
    '--disable-component-update',
    '--disable-default-apps',
    '--disable-sync',
    '--disable-features=Translate,MediaRouter,OptimizationHints,DialMediaRouteProvider,CalculateNativeWinOcclusion,InterestFeedContentSuggestions',
    // Renders run in parallel tabs: none of them is "in the background".
    '--disable-background-timer-throttling',
    '--disable-backgrounding-occluded-windows',
    '--disable-renderer-backgrounding',
    '--disable-ipc-flooding-protection',
    '--disable-hang-monitor',
    '--disable-breakpad',
    '--disable-client-side-phishing-detection',
    '--disable-popup-blocking',
    '--no-service-autorun',
    // A throwaway profile must never touch the user's keychain / password store.
    '--use-mock-keychain',
    '--password-store=basic',
    '--mute-audio',
    '--hide-scrollbars',
    '--force-color-profile=srgb',
    '--allow-file-access-from-files',
    ...(process.platform === 'linux' && process.getuid?.() === 0 ? ['--no-sandbox'] : []),
    'about:blank',
  ];
  const proc = spawn(b.path, args, { stdio: ['ignore', 'ignore', 'pipe'] });
  const wsUrl = await new Promise((resolve, reject) => {
    let buf = '';
    const t = setTimeout(() => done(new Error(`browser did not start within ${timeoutMs} ms (${b.path})`)), timeoutMs);
    const done = (err, url) => {
      clearTimeout(t);
      proc.stderr.off('data', onData);
      proc.off('exit', onExit);
      proc.off('error', onErr);
      if (err) {
        proc.kill();
        reject(err);
      } else resolve(url);
    };
    const onData = (d) => {
      buf += d;
      const m = /DevTools listening on (ws:\/\/\S+)/.exec(buf);
      if (m) done(null, m[1]);
    };
    const onExit = (code) => done(new Error(`browser exited (${code}) before it was ready: ${buf.trim().split('\n').slice(-3).join(' ')}`));
    const onErr = (e) => done(e);
    proc.stderr.on('data', onData);
    proc.once('exit', onExit);
    proc.once('error', onErr);
  });
  proc.stderr.resume(); // keep draining so the browser never blocks on a full pipe
  const ws = new WebSocket(wsUrl);
  await new Promise((resolve, reject) => {
    ws.addEventListener('open', resolve, { once: true });
    ws.addEventListener('error', () => reject(new Error('cannot connect to the browser')), { once: true });
  });
  return new Browser(proc, new Connection(ws), dataDir);
}

/** Wait until web fonts are in (bounded: a missing font must not stall a check). */
export async function fontsReady(page, ms = 3000) {
  await page.evaluate((t) => Promise.race([document.fonts ? document.fonts.ready.then(() => true) : true, new Promise((r) => setTimeout(r, t))]), ms);
}

/* ---------------------------------------------------------------- one-time headless browser download */

const CFT_URL = 'https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json';

function cftPlatform() {
  const { platform: p, arch: a } = process;
  if (p === 'darwin') return a === 'arm64' ? 'mac-arm64' : 'mac-x64';
  if (p === 'win32') return a === 'ia32' ? 'win32' : 'win64';
  if (p === 'linux') return a === 'arm64' ? 'linux-arm64' : a === 'x64' ? 'linux64' : null;
  return null;
}

/** Download Google's chrome-headless-shell (Chrome for Testing, stable) into ~/.config/dfw/browser. */
export async function installBrowser({ log = console.error } = {}) {
  const plat = cftPlatform();
  if (!plat) throw new DfwError(`No headless Chrome build for ${process.platform}-${process.arch}; install Chromium with your package manager (it is found automatically) or set DFW_BROWSER.`);
  const index = await (await fetch(CFT_URL, { signal: AbortSignal.timeout(30000) })).json();
  const stable = index.channels?.Stable;
  const dl = stable?.downloads?.['chrome-headless-shell']?.find((d) => d.platform === plat);
  if (!dl) throw new DfwError(`Chrome for Testing has no headless shell for ${plat}.`);
  const dir = path.join(browserDir(), stable.version);
  const exe = path.join(dir, `chrome-headless-shell-${plat}`, plat.startsWith('win') ? 'chrome-headless-shell.exe' : 'chrome-headless-shell');
  if (!fs.existsSync(exe)) {
    fs.mkdirSync(dir, { recursive: true });
    const zip = path.join(dir, 'download.zip');
    log(`Downloading headless Chrome ${stable.version} (${plat}) from ${new URL(dl.url).host} ...`);
    const res = await fetch(dl.url);
    if (!res.ok) throw new DfwError(`download failed: HTTP ${res.status}`);
    fs.writeFileSync(zip, Buffer.from(await res.arrayBuffer()));
    const { spawnSync } = await import('node:child_process');
    const r = plat.startsWith('win') ? spawnSync('tar', ['-xf', zip, '-C', dir], { stdio: 'inherit' }) : spawnSync('unzip', ['-q', '-o', zip, '-d', dir], { stdio: 'inherit' });
    fs.unlinkSync(zip);
    if (r.status !== 0 || !fs.existsSync(exe)) throw new DfwError('could not extract the headless Chrome archive (needs unzip, or tar on Windows).');
  }
  fs.writeFileSync(path.join(browserDir(), 'current.json'), JSON.stringify({ version: stable.version, platform: plat, path: exe }, null, 2));
  return { installed: true, path: exe, source: 'downloaded', version: stable.version };
}
