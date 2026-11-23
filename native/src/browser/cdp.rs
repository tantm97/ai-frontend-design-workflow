//! Ports the protocol half of `core/lib/cdp.mjs`: one browser-level WebSocket with flattened
//! sessions (`Connection`), browser contexts and pages, and the web font disk cache.
//! Finding and starting the browser is `browser/launch`; the one-time download is `browser/download`.
//!
//! Synchronous design, no async runtime: one reader thread per connection reads every message
//! and hands it over a channel to whoever waits for it. A reply goes to the command that asked,
//! an event to every listener of its session. Writes go through a second protocol state on a
//! clone of the socket, so a caller never waits for the reader.
//!
//! Over 800 lines: it is the whole protocol layer of cdp.mjs (connection, font cache, context,
//! page) and also holds the base64 and SHA-1 the crate list lacks (`encoding`), which move out
//! when the integrator gives them a shared home.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};
use tungstenite::protocol::{Role, WebSocketConfig, WebSocketContext};
use tungstenite::Message;

use self::encoding::{base64_decode, base64_encode, sha1_hex};
use crate::core::error::DfwError;
use crate::core::http;
use crate::core::paths::global_dir;
use crate::core::util::to_json_compact;

/// Default per-command timeout: a page stuck in a loop or a server that never answers must not
/// hang dfw for ever.
pub const COMMAND_TIMEOUT_MS: u64 = 120_000;

const CLOSED: &str = "browser connection closed";

/// A poisoned lock only means another thread panicked; the data is still what we want.
pub(crate) fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/* ---------------------------------------------------------------- protocol */

/// What the reader thread delivers to a waiting caller.
#[derive(Debug)]
pub enum Incoming {
    /// The answer to command `id`: its result, or `<method>: <error message>`.
    Reply {
        id: u64,
        result: Result<Value, String>,
    },
    /// An event of a session the receiver listens to.
    Event { method: String, params: Value },
}

struct Pending {
    method: String,
    tx: Sender<Incoming>,
}

struct Listener {
    id: u64,
    session: String,
    tx: Sender<Incoming>,
}

/// State the reader thread shares with the callers.
struct Shared {
    pending: Mutex<HashMap<u64, Pending>>,
    listeners: Mutex<Vec<Listener>>,
    closed: AtomicBool,
}

impl Shared {
    /// `onMessage(raw)`: route one message from the browser.
    fn on_message(&self, raw: &str) {
        let Ok(m) = serde_json::from_str::<Value>(raw) else {
            return;
        };
        if let Some(id) = m.get("id").filter(|id| !id.is_null()) {
            let id = id.as_u64().unwrap_or_default();
            let Some(p) = lock(&self.pending).remove(&id) else {
                return;
            };
            let result = match m.get("error").filter(|e| !e.is_null()) {
                Some(e) => {
                    let msg = e.get("message").and_then(Value::as_str).unwrap_or_default();
                    Err(format!("{}: {msg}", p.method))
                }
                None => Ok(object_or_empty(m.get("result"))),
            };
            // The caller may have given up (timeout): nobody to tell.
            let _ = p.tx.send(Incoming::Reply { id, result });
            return;
        }
        let session = m.get("sessionId").and_then(Value::as_str).unwrap_or("");
        let method = m.get("method").and_then(Value::as_str).unwrap_or("");
        let params = object_or_empty(m.get("params"));
        // A listener whose receiver is gone is removed on the way.
        lock(&self.listeners).retain(|l| {
            l.session != session
                || l.tx
                    .send(Incoming::Event {
                        method: method.to_string(),
                        params: params.clone(),
                    })
                    .is_ok()
        });
    }

    /// The socket is gone: every waiting command fails, every listener is released.
    fn fail_all(&self) {
        self.closed.store(true, Ordering::SeqCst);
        for (id, p) in lock(&self.pending).drain() {
            let _ = p.tx.send(Incoming::Reply {
                id,
                result: Err(CLOSED.to_string()),
            });
        }
        lock(&self.listeners).clear();
    }
}

/// `value || {}` for a result or an event's params.
fn object_or_empty(v: Option<&Value>) -> Value {
    match v {
        Some(v) if !v.is_null() => v.clone(),
        _ => Value::Object(Map::new()),
    }
}

/// The socket as tungstenite sees it: reads from one handle, writes whole buffers through a
/// lock shared by the reader's and the writer's protocol state, so frames never interleave
/// (the reader only writes to answer a ping or a close).
struct Duplex {
    read: TcpStream,
    write: Arc<Mutex<TcpStream>>,
}

impl Duplex {
    fn new(socket: &TcpStream, write: &Arc<Mutex<TcpStream>>) -> io::Result<Duplex> {
        Ok(Duplex {
            read: socket.try_clone()?,
            write: Arc::clone(write),
        })
    }
}

impl Read for Duplex {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read.read(buf)
    }
}

impl Write for Duplex {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // tungstenite hands over complete frames; writing all of them under the lock keeps
        // each frame in one piece.
        lock(&self.write).write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        lock(&self.write).flush()
    }
}

/// Removes its listener when dropped.
pub struct ListenerGuard {
    id: u64,
    shared: Arc<Shared>,
}

impl Drop for ListenerGuard {
    fn drop(&mut self) {
        lock(&self.shared.listeners).retain(|l| l.id != self.id);
    }
}

/// The events of one session from the moment it was created (`conn.on(sessionId, fn)`).
pub struct Subscription {
    rx: Receiver<Incoming>,
    guard: ListenerGuard,
}

impl Subscription {
    /// Block until the next event; `None` when the connection closed or the listener was removed.
    pub fn next(&self) -> Option<(String, Value)> {
        loop {
            match self.rx.recv() {
                Ok(Incoming::Event { method, params }) => return Some((method, params)),
                Ok(Incoming::Reply { .. }) => {}
                Err(_) => return None,
            }
        }
    }

    fn id(&self) -> u64 {
        self.guard.id
    }
}

/// One DevTools WebSocket (`class Connection`).
pub struct Connection {
    shared: Arc<Shared>,
    writer: Mutex<(WebSocketContext, Duplex)>,
    socket: TcpStream,
    command_timeout_ms: u64,
    seq: AtomicU64,
    next_listener: AtomicU64,
}

impl Connection {
    /// Open `ws://host:port/path` within `timeout_ms` and start the reader thread.
    pub fn connect(
        ws_url: &str,
        timeout_ms: u64,
        command_timeout_ms: Option<u64>,
    ) -> Result<Connection, DfwError> {
        let started = Instant::now();
        let timeout = Duration::from_millis(timeout_ms);
        // Node told the two apart by which fired first, its timer or the socket error.
        let failed = || {
            DfwError::internal(if started.elapsed() >= timeout {
                format!("cannot connect to the browser within {timeout_ms} ms")
            } else {
                "cannot connect to the browser".to_string()
            })
        };
        let authority = ws_url
            .strip_prefix("ws://")
            .and_then(|rest| rest.split('/').next())
            .unwrap_or_default();
        let socket = authority
            .to_socket_addrs()
            .ok()
            .and_then(|mut addrs| addrs.find_map(|a| TcpStream::connect_timeout(&a, timeout).ok()))
            .ok_or_else(failed)?;
        let left = timeout
            .saturating_sub(started.elapsed())
            .max(Duration::from_millis(1));
        let _ = socket.set_nodelay(true);
        let write = Arc::new(Mutex::new(socket.try_clone()?));
        socket.set_read_timeout(Some(left))?;
        // A full-page screenshot is one message of many megabytes: no size limit on our own browser.
        let config = WebSocketConfig::default()
            .max_message_size(None)
            .max_frame_size(None);
        let (mut ws, _) = tungstenite::client::client_with_config(
            ws_url,
            Duplex::new(&socket, &write)?,
            Some(config),
        )
        .map_err(|_| failed())?;
        socket.set_read_timeout(None)?;

        let shared = Arc::new(Shared {
            pending: Mutex::new(HashMap::new()),
            listeners: Mutex::new(Vec::new()),
            closed: AtomicBool::new(false),
        });
        let for_reader = Arc::clone(&shared);
        thread::Builder::new()
            .name("dfw-cdp-reader".into())
            .spawn(move || {
                loop {
                    match ws.read() {
                        Ok(Message::Text(text)) => for_reader.on_message(text.as_str()),
                        Ok(Message::Binary(bytes)) => {
                            for_reader.on_message(&String::from_utf8_lossy(&bytes))
                        }
                        Ok(_) => {}
                        Err(_) => break,
                    }
                }
                for_reader.fail_all();
            })?;
        Ok(Connection {
            shared,
            writer: Mutex::new((
                WebSocketContext::new(Role::Client, Some(config)),
                Duplex::new(&socket, &write)?,
            )),
            socket,
            command_timeout_ms: command_timeout_ms.unwrap_or(COMMAND_TIMEOUT_MS),
            seq: AtomicU64::new(0),
            next_listener: AtomicU64::new(0),
        })
    }

    /// Send a command without waiting: its reply arrives on `tx` as `Incoming::Reply` with the
    /// returned id. Drop the receiver to ignore the reply.
    pub fn post(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
        tx: Sender<Incoming>,
    ) -> Result<u64, DfwError> {
        let closed = || DfwError::internal(CLOSED);
        if self.shared.closed.load(Ordering::SeqCst) {
            return Err(closed());
        }
        let id = self.seq.fetch_add(1, Ordering::SeqCst) + 1;
        let mut message = Map::new();
        message.insert("id".into(), json!(id));
        message.insert("method".into(), json!(method));
        message.insert("params".into(), params);
        if let Some(session) = session.filter(|s| !s.is_empty()) {
            message.insert("sessionId".into(), json!(session));
        }
        lock(&self.shared.pending).insert(
            id,
            Pending {
                method: method.to_string(),
                tx,
            },
        );
        let text = to_json_compact(&Value::Object(message));
        let sent = {
            let mut writer = lock(&self.writer);
            let (ctx, stream) = &mut *writer;
            ctx.write(stream, Message::text(text))
                .and_then(|()| ctx.flush(stream))
        };
        // The reader may have failed every pending command just before this one was added.
        if sent.is_err() || self.shared.closed.load(Ordering::SeqCst) {
            self.forget(id);
            return Err(closed());
        }
        Ok(id)
    }

    /// Stop waiting for the reply to command `id`.
    pub fn forget(&self, id: u64) {
        lock(&self.shared.pending).remove(&id);
    }

    /// `send(method, params, sessionId)`: send a command and wait for its result.
    pub fn send(
        &self,
        method: &str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, DfwError> {
        let (tx, rx) = mpsc::channel();
        let id = self.post(method, params, session, tx)?;
        match rx.recv_timeout(Duration::from_millis(self.command_timeout_ms)) {
            Ok(Incoming::Reply { result, .. }) => result.map_err(DfwError::internal),
            // Only the reply of `id` is ever sent on this channel.
            Ok(Incoming::Event { .. }) | Err(RecvTimeoutError::Disconnected) => {
                Err(DfwError::internal(CLOSED))
            }
            Err(RecvTimeoutError::Timeout) => {
                self.forget(id);
                Err(DfwError::internal(format!(
                    "{method}: no reply from the browser within {} ms",
                    self.command_timeout_ms
                )))
            }
        }
    }

    /// Deliver the events of `session` ("" = the browser) to `tx` until the guard is dropped.
    pub fn listen(&self, session: &str, tx: Sender<Incoming>) -> ListenerGuard {
        let id = self.next_listener.fetch_add(1, Ordering::SeqCst);
        lock(&self.shared.listeners).push(Listener {
            id,
            session: session.to_string(),
            tx,
        });
        ListenerGuard {
            id,
            shared: Arc::clone(&self.shared),
        }
    }

    /// `on(sessionId, fn)`: the events of a session from now on.
    pub fn subscribe(&self, session: &str) -> Subscription {
        let (tx, rx) = mpsc::channel();
        Subscription {
            rx,
            guard: self.listen(session, tx),
        }
    }

    fn unlisten(&self, id: u64) {
        lock(&self.shared.listeners).retain(|l| l.id != id);
    }

    /// Per-command timeout of this connection.
    pub fn command_timeout_ms(&self) -> u64 {
        self.command_timeout_ms
    }

    /// Close the socket; waiting commands fail with "browser connection closed".
    pub fn close(&self) {
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

/* ---------------------------------------------------------------- web font disk cache */

// Google Fonts CSS and font files are immutable per URL: serve them from disk so a render
// never waits on the network after the first one (each page paid ~2 s for fonts before).
/// The largest font file a page may fetch through the cache (a font is well under 1 MB).
const FONT_LIMIT: u64 = 20 << 20;

const FONT_PATTERNS: [&str; 2] = [
    "https://fonts.googleapis.com/*",
    "https://fonts.gstatic.com/*",
];

/// `fontCacheDir()`.
fn font_cache_dir() -> PathBuf {
    global_dir().join("cache").join("fonts")
}

/// `serveFont`: answer one paused font request from the disk cache, filling it on a miss.
fn serve_font(conn: &Connection, session: &str, dir: &Path, params: &Value) {
    let request_id = params.get("requestId").cloned().unwrap_or(Value::Null);
    if cached_or_fetched(conn, session, dir, &request_id, params.get("request")).is_err() {
        // Offline or blocked: the page falls back to its system font stack.
        let _ = conn.send(
            "Fetch.failRequest",
            json!({ "requestId": request_id, "errorReason": "InternetDisconnected" }),
            Some(session),
        );
    }
}

fn cached_or_fetched(
    conn: &Connection,
    session: &str,
    dir: &Path,
    request_id: &Value,
    request: Option<&Value>,
) -> Result<(), DfwError> {
    let url = request
        .and_then(|r| r.get("url"))
        .and_then(Value::as_str)
        .ok_or_else(|| DfwError::internal("font request without a url"))?;
    let key = sha1_hex(url.as_bytes());
    let body = dir.join(&key);
    let meta = dir.join(format!("{key}.json"));
    let fulfill = |buf: &[u8], content_type: &str| {
        conn.send(
            "Fetch.fulfillRequest",
            json!({
                "requestId": request_id,
                "responseCode": 200,
                "responseHeaders": [
                    { "name": "Content-Type", "value": content_type },
                    { "name": "Access-Control-Allow-Origin", "value": "*" },
                    { "name": "Cache-Control", "value": "public, max-age=31536000" },
                ],
                "body": base64_encode(buf),
            }),
            Some(session),
        )
        .map(|_| ())
    };
    if body.exists() && meta.exists() {
        let info: Value = serde_json::from_str(&fs::read_to_string(&meta)?)?;
        let content_type = info
            .get("contentType")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return fulfill(&fs::read(&body)?, content_type);
    }
    // Same headers as the browser (Google Fonts picks the font format from the User-Agent).
    let headers = request
        .and_then(|r| r.get("headers"))
        .and_then(Value::as_object);
    let skip = ["host", "connection", "content-length", "accept-encoding"];
    let forwarded: Vec<(&str, &str)> = headers
        .into_iter()
        .flatten()
        .filter(|(name, _)| !skip.contains(&name.to_ascii_lowercase().as_str()))
        .filter_map(|(name, value)| Some((name.as_str(), value.as_str()?)))
        .collect();
    let res = http::get(url, &forwarded, Duration::from_secs(15), FONT_LIMIT)
        .map_err(|e| DfwError::internal(e.to_string()))?;
    let content_type = res
        .content_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());
    let ok = res.ok();
    let buf = res.body;
    if ok {
        fs::create_dir_all(dir)?;
        crate::core::util::write_atomic(&body, &buf)?;
        crate::core::util::write_atomic(
            &meta,
            to_json_compact(&json!({ "url": url, "contentType": content_type })),
        )?;
    }
    // As in Node, a failed upstream answer is still handed to the page as a 200.
    fulfill(&buf, &content_type)
}

/// Answer the paused font requests of a session, each on its own thread (a slow download must
/// not hold up the page's other fonts), until the listener is removed. Returns the listener id.
fn serve_fonts(
    conn: &Arc<Connection>,
    session: &str,
    cache: Option<PathBuf>,
) -> Result<u64, DfwError> {
    let events = conn.subscribe(session);
    let id = events.id();
    let conn = Arc::clone(conn);
    let session = session.to_string();
    thread::Builder::new()
        .name("dfw-cdp-fonts".into())
        .spawn(move || {
            while let Some((method, params)) = events.next() {
                if method != "Fetch.requestPaused" {
                    continue;
                }
                let (conn, session) = (Arc::clone(&conn), session.clone());
                let dir = cache.clone().unwrap_or_else(font_cache_dir);
                let _ = thread::Builder::new()
                    .spawn(move || serve_font(&conn, &session, &dir, &params));
            }
        })?;
    Ok(id)
}

/* ---------------------------------------------------------------- context / page */

/// Options of [`Page::screenshot`].
#[derive(Debug, Default, Clone)]
pub struct ShotOptions<'a> {
    /// Also write the PNG here (folders are created).
    pub path: Option<&'a Path>,
    /// The project `path` must stay inside (see `screenshot::Job::inside`).
    pub inside: Option<&'a Path>,
    /// The whole document, not only the viewport.
    pub full_page: bool,
    /// Only this element (wins over `full_page`).
    pub selector: Option<&'a str>,
}

// Runs in the page: the element's box in document coordinates, scrolled into view.
const SELECTOR_BOX: &str = "(sel) => {
        const el = document.querySelector(sel);
        if (!el) return null;
        el.scrollIntoView({ block: 'center', inline: 'nearest' });
        const b = el.getBoundingClientRect();
        return { x: b.left + scrollX, y: b.top + scrollY, width: b.width, height: b.height };
      }";

/// One tab (`class Page`).
pub struct Page {
    conn: Arc<Connection>,
    target_id: String,
    session_id: String,
    scale: f64,
    width: u32,
    height: u32,
    font_listener: Option<u64>,
}

impl Page {
    /// `page.send(method, params)`: a command in this page's session.
    pub fn send(&self, method: &str, params: Value) -> Result<Value, DfwError> {
        self.conn.send(method, params, Some(&self.session_id))
    }

    /// `setViewportSize({ width, height })`.
    pub fn set_viewport_size(&mut self, width: u32, height: u32) -> Result<(), DfwError> {
        self.width = width;
        self.height = height;
        self.send(
            "Emulation.setDeviceMetricsOverride",
            json!({ "width": width, "height": height, "deviceScaleFactor": self.scale, "mobile": false }),
        )?;
        Ok(())
    }

    /// `goto(url, { timeout })`: navigate and wait for the load event.
    pub fn goto(&self, url: &str, timeout_ms: u64) -> Result<(), DfwError> {
        let started = Instant::now();
        let load_by = started + Duration::from_millis(timeout_ms);
        let reply_by = started + Duration::from_millis(self.conn.command_timeout_ms());
        let (tx, rx) = mpsc::channel();
        let _events = self.conn.listen(&self.session_id, tx.clone());
        let id = self.conn.post(
            "Page.navigate",
            json!({ "url": url }),
            Some(&self.session_id),
            tx,
        )?;
        let mut replied = false;
        loop {
            // Page.navigate only answers once the server responds: the load timeout covers
            // that wait too.
            let until = if replied {
                load_by
            } else {
                load_by.min(reply_by)
            };
            match rx.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(Incoming::Event { method, .. }) if method == "Page.loadEventFired" => {
                    self.conn.forget(id);
                    return Ok(());
                }
                Ok(Incoming::Reply { id: got, result }) if got == id => {
                    let r = result.map_err(DfwError::internal)?;
                    let error = r.get("errorText").and_then(Value::as_str).unwrap_or("");
                    if !error.is_empty() {
                        return Err(DfwError::internal(format!("cannot open {url}: {error}")));
                    }
                    replied = true;
                }
                Ok(_) => {}
                Err(_) => {
                    self.conn.forget(id);
                    return Err(DfwError::internal(if replied || load_by <= reply_by {
                        format!("timeout {timeout_ms}ms waiting for Page.loadEventFired")
                    } else {
                        format!(
                            "Page.navigate: no reply from the browser within {} ms",
                            self.conn.command_timeout_ms()
                        )
                    }));
                }
            }
        }
    }

    /// `evaluate(fn, arg)`: call a self-contained JavaScript function (its source text) in the
    /// page with one JSON argument; a returned promise is awaited. Returns the value (null for
    /// undefined).
    pub fn evaluate(&self, function: &str, arg: Option<&Value>) -> Result<Value, DfwError> {
        let arg = arg.map(to_json_compact).unwrap_or_default();
        let r = self.send(
            "Runtime.evaluate",
            json!({ "expression": format!("({function})({arg})"), "awaitPromise": true, "returnByValue": true, "userGesture": true }),
        )?;
        if let Some(details) = r.get("exceptionDetails").filter(|d| !d.is_null()) {
            let text = |v: Option<&Value>| {
                v.and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
            };
            let message = text(details.get("exception").and_then(|e| e.get("description")))
                .or_else(|| text(details.get("text")))
                .unwrap_or_else(|| "evaluation failed".to_string());
            return Err(DfwError::internal(message));
        }
        Ok(r.get("result")
            .and_then(|res| res.get("value"))
            .cloned()
            .unwrap_or(Value::Null))
    }

    /// `waitForTimeout(ms)`.
    pub fn wait_for_timeout(&self, ms: u64) {
        thread::sleep(Duration::from_millis(ms));
    }

    /// `screenshot({ path, fullPage, selector })`: the PNG bytes, also written to `path`.
    pub fn screenshot(&self, opts: &ShotOptions) -> Result<Vec<u8>, DfwError> {
        let mut clip: Option<Value> = None;
        if let Some(selector) = opts.selector.filter(|s| !s.is_empty()) {
            let r = self.evaluate(SELECTOR_BOX, Some(&json!(selector)))?;
            let size = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
            if size("width") == 0.0 || size("height") == 0.0 {
                return Err(DfwError::internal(format!(
                    "selector not found: {selector}"
                )));
            }
            let mut c = r.as_object().cloned().unwrap_or_default();
            c.insert("scale".into(), json!(1));
            clip = Some(Value::Object(c));
        } else if opts.full_page {
            let m = self.send("Page.getLayoutMetrics", json!({}))?;
            let height = ["cssContentSize", "contentSize"]
                .iter()
                .find_map(|k| m.get(k).filter(|v| !v.is_null()))
                .and_then(|size| size.get("height"))
                .and_then(Value::as_f64)
                .ok_or_else(|| DfwError::internal("Page.getLayoutMetrics: no content size"))?;
            let height = f64::from(self.height).max(height.ceil());
            clip =
                Some(json!({ "x": 0, "y": 0, "width": self.width, "height": height, "scale": 1 }));
        }
        let shot = self.send(
            "Page.captureScreenshot",
            match clip {
                Some(clip) => {
                    json!({ "format": "png", "clip": clip, "captureBeyondViewport": true })
                }
                None => json!({ "format": "png" }),
            },
        )?;
        let buf = shot
            .get("data")
            .and_then(Value::as_str)
            .and_then(base64_decode)
            .ok_or_else(|| DfwError::internal("Page.captureScreenshot: no image data"))?;
        if let Some(out) = opts.path {
            if let Some(dir) = out.parent() {
                fs::create_dir_all(dir)?;
            }
            match opts.inside {
                Some(root) => crate::core::util::write_atomic_inside(root, out, &buf)?,
                None => crate::core::util::write_atomic(out, &buf)?,
            }
        }
        Ok(buf)
    }

    /// `close()`: close the tab; a failure is ignored (the browser may be gone already).
    pub fn close(&self) {
        self.stop_fonts();
        let _ = self.conn.send(
            "Target.closeTarget",
            json!({ "targetId": self.target_id }),
            None,
        );
    }

    fn stop_fonts(&self) {
        if let Some(id) = self.font_listener {
            self.conn.unlisten(id);
        }
    }
}

impl Drop for Page {
    fn drop(&mut self) {
        // Ends the font thread of a page that was never closed.
        self.stop_fonts();
    }
}

/// Options of a browser context (`newContext(opts)`).
#[derive(Debug, Clone)]
pub struct ContextOptions {
    pub device_scale_factor: f64,
    /// Serve Google Fonts from the disk cache.
    pub cache_fonts: bool,
    /// Where that cache is (default: `cache/fonts` in the global dfw folder).
    pub font_cache_dir: Option<PathBuf>,
}

impl Default for ContextOptions {
    fn default() -> Self {
        ContextOptions {
            device_scale_factor: 1.0,
            cache_fonts: true,
            font_cache_dir: None,
        }
    }
}

/// An isolated browser context (`class Context`): its pages share cookies and the memory cache.
pub struct Context {
    conn: Arc<Connection>,
    id: String,
    opts: ContextOptions,
}

impl Context {
    /// `browser.newContext(opts)`.
    pub fn create(conn: &Arc<Connection>, opts: ContextOptions) -> Result<Context, DfwError> {
        let r = conn.send(
            "Target.createBrowserContext",
            json!({ "disposeOnDetach": true }),
            None,
        )?;
        Ok(Context {
            conn: Arc::clone(conn),
            id: text(&r, "browserContextId"),
            opts,
        })
    }

    /// `newPage()`: a blank tab, 1280x800 until `set_viewport_size`.
    pub fn new_page(&self) -> Result<Page, DfwError> {
        let target = self.conn.send(
            "Target.createTarget",
            json!({ "url": "about:blank", "browserContextId": self.id }),
            None,
        )?;
        let target_id = text(&target, "targetId");
        let attached = self.conn.send(
            "Target.attachToTarget",
            json!({ "targetId": target_id, "flatten": true }),
            None,
        )?;
        let mut page = Page {
            conn: Arc::clone(&self.conn),
            target_id,
            session_id: text(&attached, "sessionId"),
            scale: self.opts.device_scale_factor,
            width: 1280,
            height: 800,
            font_listener: None,
        };
        if self.opts.cache_fonts {
            let cache = self.opts.font_cache_dir.clone();
            page.font_listener = Some(serve_fonts(&self.conn, &page.session_id, cache)?);
            page.send(
                "Fetch.enable",
                json!({ "patterns": FONT_PATTERNS.map(|p| json!({ "urlPattern": p })) }),
            )?;
        }
        page.send("Page.enable", json!({}))?;
        Ok(page)
    }
}

fn text(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/* ---------------------------------------------------------------- base64, SHA-1 */

/// Base64 (protocol payloads) and SHA-1 (font cache keys, shared with the Node cache on disk).
/// Both crates are in the lock file only through tungstenite, so they cannot be used from here.
/// Moved to `core::encoding` (layer 0); kept here as a path so the CDP code reads as before.
pub(crate) use crate::core::encoding;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::net::TcpListener;

    const FONT: &[u8] = b"wOF2 bytes \x00\xff";

    /// A scripted DevTools endpoint: answers every command with `{}` (and the ids a new page
    /// needs), pauses one "font request" per URL once Fetch is enabled, and reports how the
    /// client resolved each of them.
    fn font_endpoint(urls: Vec<String>) -> (String, Receiver<Value>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}/devtools/browser/x", listener.local_addr().unwrap());
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let mut ws = tungstenite::accept(listener.accept().unwrap().0).unwrap();
            let mut urls = urls.into_iter();
            let mut pause = |ws: &mut tungstenite::WebSocket<TcpStream>| {
                if let Some(url) = urls.next() {
                    let headers = json!({ "User-Agent": "TestBrowser/1", "Accept-Encoding": "gzip", "Host": "fonts.example" });
                    let params = json!({ "requestId": format!("r-{url}"), "request": { "url": url, "headers": headers } });
                    let event = json!({ "method": "Fetch.requestPaused", "sessionId": "s1", "params": params });
                    ws.send(Message::text(event.to_string())).unwrap();
                }
            };
            while let Ok(Message::Text(text)) = ws.read() {
                let m: Value = serde_json::from_str(text.as_str()).unwrap();
                let method = m["method"].as_str().unwrap();
                let result = match method {
                    "Target.createBrowserContext" => json!({ "browserContextId": "c1" }),
                    "Target.createTarget" => json!({ "targetId": "t1" }),
                    "Target.attachToTarget" => json!({ "sessionId": "s1" }),
                    _ => json!({}),
                };
                let reply = json!({ "id": m["id"], "result": result });
                ws.send(Message::text(reply.to_string())).unwrap();
                match method {
                    "Fetch.enable" => pause(&mut ws),
                    "Fetch.fulfillRequest" | "Fetch.failRequest" => {
                        let mut seen = m["params"].clone();
                        seen["method"] = json!(method);
                        tx.send(seen).unwrap();
                        pause(&mut ws);
                    }
                    _ => {}
                }
            }
        });
        (url, rx)
    }

    /// The web font disk cache: a miss is fetched once (with the browser's headers) and
    /// stored, a hit is served from disk, a failure lets the page fall back. The "font host"
    /// is a local server: nothing leaves the machine.
    #[test]
    fn fonts_are_fetched_once_then_served_from_disk() {
        let cache = test_dir("fonts").join("cache");
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let host = format!("http://{}", server.server_addr());
        let (seen_tx, seen_rx) = mpsc::channel();
        thread::spawn(move || {
            for request in server.incoming_requests() {
                let header = |name: &'static str| {
                    let found = request.headers().iter().find(|h| h.field.equiv(name));
                    found.map(|h| h.value.to_string())
                };
                let seen = (
                    request.url().to_string(),
                    header("User-Agent"),
                    header("Accept-Encoding"),
                    header("Host"),
                );
                seen_tx.send(seen).unwrap();
                let (status, body, kind) = match request.url() {
                    "/font.woff2" => (200, FONT.to_vec(), "font/woff2"),
                    _ => (404, b"no such font".to_vec(), "text/plain"),
                };
                let kind = tiny_http::Header::from_bytes("Content-Type", kind).unwrap();
                let response = tiny_http::Response::from_data(body)
                    .with_status_code(status)
                    .with_header(kind);
                let _ = request.respond(response);
            }
        });
        let font = format!("{host}/font.woff2");
        let missing = format!("{host}/missing.woff2");
        let unreachable = "http://127.0.0.1:1/unreachable.woff2".to_string();
        let (ws_url, resolved) = font_endpoint(vec![
            font.clone(),
            font.clone(),
            missing.clone(),
            unreachable,
        ]);
        let conn = Arc::new(Connection::connect(&ws_url, 5000, Some(5000)).unwrap());
        let opts = ContextOptions {
            font_cache_dir: Some(cache.clone()),
            ..ContextOptions::default()
        };
        let page = Context::create(&conn, opts).unwrap().new_page().unwrap();
        let next = || resolved.recv_timeout(Duration::from_secs(10)).unwrap();

        let first = next();
        assert_eq!(first["method"], "Fetch.fulfillRequest");
        assert_eq!(first["requestId"], json!(format!("r-{font}")));
        assert_eq!(first["responseCode"], 200);
        assert_eq!(first["body"], json!(base64_encode(FONT)));
        assert_eq!(
            first["responseHeaders"],
            json!([
                { "name": "Content-Type", "value": "font/woff2" },
                { "name": "Access-Control-Allow-Origin", "value": "*" },
                { "name": "Cache-Control", "value": "public, max-age=31536000" },
            ])
        );
        // The browser's own headers go upstream, except the ones that describe the connection.
        let (path, agent, encoding, host_header) = seen_rx.recv().unwrap();
        assert_eq!(
            (path.as_str(), agent.as_deref(), encoding),
            ("/font.woff2", Some("TestBrowser/1"), None)
        );
        assert_eq!(
            host_header.as_deref(),
            Some(host.trim_start_matches("http://"))
        );
        let key = sha1_hex(font.as_bytes());
        assert_eq!(fs::read(cache.join(&key)).unwrap(), FONT);
        assert_eq!(
            fs::read_to_string(cache.join(format!("{key}.json"))).unwrap(),
            json!({ "url": font, "contentType": "font/woff2" }).to_string()
        );

        // Second request for the same URL: from disk, the server is not asked again.
        let second = next();
        assert_eq!(
            (&second["method"], &second["body"]),
            (&json!("Fetch.fulfillRequest"), &first["body"])
        );
        // A 404 is passed on (as in Node, with code 200) but never cached.
        let third = next();
        assert_eq!(
            (&third["responseCode"], &third["body"]),
            (&json!(200), &json!(base64_encode(b"no such font")))
        );
        assert_eq!(seen_rx.recv().unwrap().0, "/missing.woff2");
        assert!(seen_rx.try_recv().is_err(), "one upstream request per URL");
        assert!(!cache.join(sha1_hex(missing.as_bytes())).exists());
        // Nothing answers: the request fails and the page falls back to its system fonts.
        let fourth = next();
        assert_eq!(
            (&fourth["method"], &fourth["errorReason"]),
            (&json!("Fetch.failRequest"), &json!("InternetDisconnected"))
        );
        page.close();
        conn.close();
    }

    #[test]
    fn base64_round_trip_matches_node() {
        for (plain, encoded) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(base64_encode(plain.as_bytes()), encoded);
            assert_eq!(base64_decode(encoded).unwrap(), plain.as_bytes());
        }
        assert_eq!(base64_decode("Zm9v\nYmFy").unwrap(), b"foobar");
        assert_eq!(base64_decode("_-8=").unwrap(), [0xff, 0xef]);
        assert_eq!(base64_decode("Zm9v!"), None);
        let bytes: Vec<u8> = (0..=255).collect();
        assert_eq!(base64_decode(&base64_encode(&bytes)).unwrap(), bytes);
    }

    #[test]
    fn sha1_matches_known_digests() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        // Longer than one block.
        assert_eq!(
            sha1_hex(b"The quick brown fox jumps over the lazy dog, twice: The quick brown fox jumps over the lazy dog"),
            "23f3b45ba0f15cefec989acea071257e995190b8"
        );
    }
}
