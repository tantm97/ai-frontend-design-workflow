//! The review proxy server of `core/lib/live.mjs` (`createLiveServer`, `startLiveServer`): the
//! routes of its own (overlay, live UI, info, comments with a screenshot queue), the pass-through
//! to the app with HTML injection, and the WebSocket upgrade splice.
//!
//! The upstream request asks for `Connection: close`, so a passed-through body is copied until
//! the app closes (or for its Content-Length) without re-framing. An https app is reached
//! through `ureq` (no TLS crate of its own here); its WebSocket upgrades are not passed through.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Cursor, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use super::{
    assert_local_target, configured_shortcut, fix_set_cookie, inject_live, nearest_breakpoint,
    patch_csp, patch_meta_csp, proxy_host, Target,
};
use crate::assets::studio_file;
use crate::browser::cdp::encoding::base64_encode;
use crate::browser::screenshot::{screenshot, Job};
use crate::core::js;

use crate::core::error::DfwError;
use crate::core::paths::{project_paths, to_posix};
use crate::core::util::to_json_compact;
use crate::design::tokens::relative_path;
use crate::review::comments::{add_comment, set_comment_screenshot};
use crate::studio::http::{
    drain, http_version, no_timeouts, node_headers, read_body, read_head, url_pathname, write_head,
    write_response, BodyError, Head, READ_TIMEOUT,
};
use crate::tools::inspector::parse_insp_path;

/// Largest comment a page may post (`readBody(req, 1_000_000)`).
const MAX_COMMENT: usize = 1_000_000;
/// Largest body passed to or read from the app in memory (request bodies, HTML to inject into).
const MAX_PASS: usize = 256 * 1024 * 1024;

/// What `createLiveServer` takes.
#[derive(Debug, Clone)]
pub struct LiveOptions {
    pub root: PathBuf,
    /// The app, e.g. `http://localhost:5173`.
    pub target: String,
    /// Screenshot the app for each comment.
    pub shots: bool,
    /// `--shortcut`; the config's `review.shortcut` when `None`.
    pub shortcut: Option<String>,
}

/// A screenshot to take after a comment: id, route, viewport.
type Shot = (String, String, u32, u32);

/// A review proxy, ready to serve.
pub struct LiveServer {
    root: PathBuf,
    t: Target,
    keys: String,
    shots: bool,
    target_origins: Vec<String>,
    /// The host of the review URL: `localhost` for a localhost app, else `127.0.0.1`.
    pub public_host: String,
    shot_queue: Option<Sender<Shot>>,
}

fn log(msg: &str) {
    eprintln!("dfw review: {msg}");
}

/// Matches `^(localhost|127\.0\.0\.1|\[::1\]):\d+$`/i.
fn loopback(host: &str) -> bool {
    let low = host.to_ascii_lowercase();
    ["localhost:", "127.0.0.1:", "[::1]:"].iter().any(|p| {
        low.strip_prefix(p)
            .is_some_and(|port| !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()))
    })
}

/// A fresh CSP nonce: 16 bytes, base64. The keys of `RandomState` come from the operating
/// system's random source, so the hashes are unpredictable without a random-number crate.
fn nonce() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let mut bytes = Vec::with_capacity(16);
    for i in 0..2u8 {
        let mut h = RandomState::new().build_hasher();
        h.write_u8(i);
        h.write_u128(now);
        bytes.extend_from_slice(&h.finish().to_le_bytes());
    }
    base64_encode(&bytes)
}

impl LiveServer {
    /// `createLiveServer(opts)`: check the target and the shortcut, start the screenshot queue.
    pub fn new(opts: &LiveOptions) -> Result<Self, DfwError> {
        let t = assert_local_target(&opts.target)?;
        let keys = configured_shortcut(&opts.root, opts.shortcut.as_deref())?;
        let origin = t.origin();
        let target_origins = vec![
            origin.clone(),
            origin.replacen("localhost", "127.0.0.1", 1),
            origin.replacen("127.0.0.1", "localhost", 1),
        ];
        let shot_queue = opts
            .shots
            .then(|| spawn_shots(opts.root.clone(), t.origin()));
        Ok(LiveServer {
            root: opts.root.clone(),
            public_host: proxy_host(&t).to_string(),
            t,
            keys,
            shots: opts.shots,
            target_origins,
            shot_queue,
        })
    }

    /// Accept connections until the listener fails.
    pub fn serve(self: Arc<Self>, listener: TcpListener) {
        for stream in listener.incoming().flatten() {
            let me = Arc::clone(&self);
            std::thread::spawn(move || me.handle(stream));
        }
    }

    /// The origin the browser sees: the request's Host when it is a loopback name, so
    /// redirects, cookies and the CSP source match whether the page was opened as localhost or
    /// 127.0.0.1.
    fn self_of(&self, head: &Head, port: u16) -> String {
        let h = head.header("host").unwrap_or_default();
        if loopback(h) {
            format!("http://{}", h.to_lowercase())
        } else {
            format!("http://{}:{port}", self.public_host)
        }
    }

    /// Local user only: a rebinding page must not reach the app through the proxy (the proxy
    /// rewrites Host, which would defeat the dev server's own host check), and another site
    /// must not post comments.
    fn foreign(&self, head: &Head) -> Option<String> {
        let host = head.header("host").unwrap_or_default();
        if !loopback(host) {
            let shown = if host.is_empty() { "(none)" } else { host };
            return Some(format!("host {shown} not allowed"));
        }
        let writes = !matches!(head.method(), "GET" | "HEAD" | "OPTIONS");
        if let Some(from) = head.header("origin").filter(|_| writes) {
            let bare = from
                .strip_prefix("http://")
                .or_else(|| from.strip_prefix("https://"))
                .unwrap_or(from);
            if !loopback(bare) {
                return Some(format!("origin {from} not allowed"));
            }
        }
        None
    }

    fn handle(&self, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let Ok(Some(head)) = read_head(&mut stream) else {
            return;
        };
        if head.is_upgrade() {
            return self.upgrade(stream, head);
        }
        let method = head.method().to_string();
        if let Err(err) = self.request(&mut stream, head) {
            let code = if matches!(err, DfwError::User { .. }) {
                400
            } else {
                500
            };
            let body = to_json_compact(&json!({ "error": err.message() }));
            let _ = write_response(&mut stream, &method, code, &json_headers(), body.as_bytes());
        }
    }

    fn request(&self, stream: &mut TcpStream, head: Head) -> Result<(), DfwError> {
        let method = head.method().to_string();
        let send = |stream: &mut TcpStream, code: u16, body: &[u8], ty: &str| {
            let headers = vec![
                ("Content-Type".to_string(), ty.to_string()),
                ("Cache-Control".to_string(), "no-store".to_string()),
            ];
            let _ = write_response(stream, &method, code, &headers, body);
        };
        if let Some(refused) = self.foreign(&head) {
            let body = to_json_compact(&json!({ "error": refused }));
            send(stream, 403, body.as_bytes(), JSON);
            return Ok(());
        }
        let p = url_pathname(head.target());
        if p == "/__dfw/overlay.js" || p == "/__dfw/live.js" {
            let file = studio_file(&p["/__dfw/".len()..])
                .ok_or_else(|| DfwError::internal(format!("ENOENT: {p}")))?;
            send(stream, 200, &file, "text/javascript; charset=utf-8");
            return Ok(());
        }
        let port = stream.local_addr().map_or(0, |a| a.port());
        if p == "/__dfw/api/live" {
            let info = json!({
                "target": self.t.origin(),
                "url": format!("http://{}:{port}/", self.public_host),
                "shots": self.shots,
                "shortcut": self.keys,
            });
            send(stream, 200, to_json_compact(&info).as_bytes(), JSON);
            return Ok(());
        }
        if p == "/__dfw/api/comments" && method == "POST" {
            let body = match read_body(stream, &head, MAX_COMMENT, false) {
                Ok(b) => b,
                Err(BodyError::TooLarge) => {
                    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                    drain(stream, &head);
                    return Err(DfwError::new("Request too large."));
                }
                Err(BodyError::Io(e)) => return Err(e.into()),
            };
            let text = String::from_utf8_lossy(&body);
            let b: Value = if text.is_empty() {
                json!({})
            } else {
                serde_json::from_str(&text)?
            };
            let c = self.post_comment(&b)?;
            send(stream, 201, to_json_compact(&c).as_bytes(), JSON);
            return Ok(());
        }
        self.proxy(stream, head, port);
        Ok(())
    }

    /// `postComment`: a comment as live.js posts it, stored like a Studio comment.
    fn post_comment(&self, b: &Value) -> Result<Value, DfwError> {
        let mut target = js::spread(b.get("target").filter(|t| js::truthy(t)));
        let insp = target
            .get("insp")
            .and_then(Value::as_str)
            .map(str::to_string);
        target.shift_remove("insp");
        // Only a parsed data-insp-path becomes the source; without it the selector and text
        // identify the element.
        let root = self.root.to_string_lossy();
        let source = insp
            .as_deref()
            .and_then(|i| parse_insp_path(i, Some(&root)))
            .map_or(Value::Null, |p| json!(p.source));
        target.insert("source".into(), source);
        if let Some(i) = insp.as_deref().filter(|i| !i.is_empty()) {
            target.insert("inspPath".into(), json!(i));
        }
        let size = |i: usize, fallback: f64| -> f64 {
            let n = js::to_number(b.get("viewport").and_then(|v| v.get(i)));
            let n = if n.is_nan() || n == 0.0 { fallback } else { n };
            js::round(n).clamp(200.0, 4000.0)
        };
        let (vw, vh) = (size(0, 1440.0), size(1, 900.0));
        let route = b
            .get("url")
            .and_then(Value::as_str)
            .filter(|u| u.starts_with('/'))
            .unwrap_or("/")
            .to_string();
        let screen = match b.get("screen").and_then(Value::as_str) {
            Some(s) => s.to_string(),
            None => route.split('#').next().unwrap_or_default().to_string(),
        };
        let mut input = Map::new();
        if let Some(c) = b.get("comment") {
            input.insert("comment".into(), c.clone());
        }
        input.insert("screen".into(), json!(screen));
        input.insert("variant".into(), json!("live"));
        match nearest_breakpoint(&self.root, vw)? {
            Some(id) => input.insert("breakpoint".into(), id),
            None => input.insert("breakpoint".into(), Value::Null),
        };
        input.insert("url".into(), json!(route));
        input.insert("target".into(), Value::Object(target));
        input.insert("provider".into(), json!("dfw-live"));
        let c = add_comment(&self.root, &Value::Object(input))?;
        if let Some(queue) = &self.shot_queue {
            let id = js::to_string(c.get("id"));
            let _ = queue.send((id, route, vw as u32, vh as u32));
        }
        Ok(c)
    }

    /// Pass a request to the app; inject into a 200 HTML answer to a GET.
    fn proxy(&self, stream: &mut TcpStream, head: Head, port: u16) {
        let self_origin = self.self_of(&head, port);
        let body = match read_body(stream, &head, MAX_PASS, false) {
            Ok(b) => b,
            Err(_) => return,
        };
        let mut headers = node_headers(&head.headers);
        let had_length = headers
            .iter()
            .any(|(k, _)| k == "content-length" || k == "transfer-encoding");
        headers.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                "accept-encoding"
                    | "content-length"
                    | "transfer-encoding"
                    | "connection"
                    | "keep-alive"
            )
        });
        for (k, v) in headers.iter_mut() {
            match k.as_str() {
                "host" => *v = self.t.host(),
                "origin" => *v = self.t.origin(),
                "referer" if v.starts_with(&self_origin) => {
                    *v = format!("{}{}", self.t.origin(), &v[self_origin.len()..]);
                }
                _ => {}
            }
        }
        if !headers.iter().any(|(k, _)| k == "host") {
            headers.insert(0, ("host".into(), self.t.host()));
        }
        if had_length || !body.is_empty() {
            headers.push(("content-length".into(), body.len().to_string()));
        }
        let method = head.method().to_string();
        let upstream = if self.t.protocol == "https:" {
            https_upstream(&self.t, &method, head.target(), &headers, body)
        } else {
            http_upstream(&self.t, &method, head.target(), &headers, &body)
        };
        let up = match upstream {
            Ok(up) => up,
            Err(e) => {
                let msg = e.replace(['<', '&'], "");
                let page = format!("<!doctype html><meta charset=\"utf-8\"><title>dfw review</title><body style=\"font:15px system-ui;padding:40px\"><h1>dfw review: the app is not reachable</h1><p>{}: {msg}</p><p>Start the dev server, then reload this page.</p>", self.t.origin());
                let headers = vec![
                    (
                        "Content-Type".to_string(),
                        "text/html; charset=utf-8".to_string(),
                    ),
                    ("Cache-Control".to_string(), "no-store".to_string()),
                ];
                let _ = write_response(stream, &method, 502, &headers, page.as_bytes());
                return;
            }
        };
        self.answer(stream, &method, &self_origin, up);
    }

    fn answer(&self, stream: &mut TcpStream, method: &str, self_origin: &str, up: Upstream) {
        let Upstream {
            status,
            headers,
            body,
        } = up;
        let mut h = node_headers(&headers);
        h.retain(|(k, _)| k != "connection" && k != "keep-alive");
        for (k, v) in h.iter_mut().filter(|(k, _)| k == "location") {
            let _ = k;
            for o in &self.target_origins {
                if v.starts_with(o.as_str()) {
                    *v = format!("{self_origin}{}", &v[o.len()..]);
                }
            }
        }
        let host = self_origin.trim_start_matches("http://");
        let host = host.rsplit_once(':').map_or(host, |(h, _)| h);
        let cookies: Vec<String> = h
            .iter()
            .filter(|(k, _)| k == "set-cookie")
            .map(|(_, v)| v.clone())
            .collect();
        if !cookies.is_empty() {
            let mut fixed = fix_set_cookie(&cookies, host).into_iter();
            for (k, v) in h.iter_mut() {
                if k == "set-cookie" {
                    *v = fixed.next().unwrap_or_default();
                }
            }
        }
        let get = |h: &[(String, String)], k: &str| {
            h.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
        };
        let html = get(&h, "content-type")
            .is_some_and(|t| t.to_ascii_lowercase().contains("text/html"))
            && method == "GET"
            && status == 200;
        let no_body =
            method == "HEAD" || status == 204 || status == 304 || (100..200).contains(&status);
        if !html || no_body {
            return pass_through(stream, status, h, body, no_body);
        }
        let Ok(raw) = body.read_all() else {
            return;
        };
        let decoded = decode(&raw, get(&h, "content-encoding").as_deref());
        let Some(text) = decoded else {
            // Unknown encoding (or one that does not decode): the body as the app sent it.
            h.retain(|(k, _)| k != "transfer-encoding" && k != "content-length");
            h.push(("content-length".into(), raw.len().to_string()));
            let _ = write_head(stream, status, &h).and_then(|_| stream.write_all(&raw));
            return;
        };
        let nonce = nonce();
        for (k, v) in h.iter_mut() {
            if k == "content-security-policy" || k == "content-security-policy-report-only" {
                *v = patch_csp(v, self_origin, Some(&nonce));
            }
        }
        let page = String::from_utf8_lossy(&text);
        let out = inject_live(
            &patch_meta_csp(&page, self_origin, Some(&nonce)),
            Some(&nonce),
            &self.keys,
        );
        h.retain(|(k, _)| {
            !matches!(
                k.as_str(),
                "content-length"
                    | "content-encoding"
                    | "transfer-encoding"
                    | "etag"
                    | "last-modified"
            )
        });
        h.push(("content-length".into(), out.len().to_string()));
        match h.iter_mut().find(|(k, _)| k == "cache-control") {
            Some((_, v)) => *v = "no-store".into(),
            None => h.push(("cache-control".into(), "no-store".into())),
        }
        let _ = write_head(stream, status, &h).and_then(|_| stream.write_all(out.as_bytes()));
    }

    /// WebSocket (HMR) passthrough: replay the upgrade request to the app and splice the sockets.
    fn upgrade(&self, client: TcpStream, head: Head) {
        if !loopback(head.header("host").unwrap_or_default()) || self.t.protocol == "https:" {
            return;
        }
        let host = self
            .t
            .hostname
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let Ok(mut up) = TcpStream::connect((host.as_str(), self.t.connect_port())) else {
            return;
        };
        let mut lines = vec![format!(
            "{} {} HTTP/{}",
            head.method(),
            head.target(),
            http_version(&head)
        )];
        for (k, v) in &head.headers {
            let v = if k.eq_ignore_ascii_case("host") {
                self.t.host()
            } else if k.eq_ignore_ascii_case("origin") {
                self.t.origin()
            } else {
                v.clone()
            };
            lines.push(format!("{k}: {v}"));
        }
        let request = format!("{}\r\n\r\n", lines.join("\r\n"));
        if up
            .write_all(request.as_bytes())
            .and_then(|_| up.write_all(&head.rest))
            .is_err()
        {
            return;
        }
        no_timeouts(&client);
        splice(client, up);
    }
}

const JSON: &str = "application/json; charset=utf-8";

fn json_headers() -> Vec<(String, String)> {
    vec![
        ("Content-Type".into(), JSON.into()),
        ("Cache-Control".into(), "no-store".into()),
    ]
}

/// Copy both ways until either side closes, then close both.
fn splice(a: TcpStream, b: TcpStream) {
    let (Ok(mut a2), Ok(mut b2)) = (a.try_clone(), b.try_clone()) else {
        return;
    };
    let (mut a, mut b) = (a, b);
    let t = std::thread::spawn(move || {
        let _ = io::copy(&mut a2, &mut b2);
        let _ = b2.shutdown(Shutdown::Both);
        let _ = a2.shutdown(Shutdown::Both);
    });
    let _ = io::copy(&mut b, &mut a);
    let _ = a.shutdown(Shutdown::Both);
    let _ = b.shutdown(Shutdown::Both);
    let _ = t.join();
}

/// The app's answer: status, header lines and the body still to read.
struct Upstream {
    status: u16,
    headers: Vec<(String, String)>,
    body: UpBody,
}

enum UpBody {
    /// Straight from the socket: framing (Content-Length, chunked) still on the wire.
    Raw { head: Head, stream: TcpStream },
    /// Already unframed (ureq); read until the end.
    Plain(Box<dyn Read + Send>),
}

impl UpBody {
    fn read_all(self) -> Result<Vec<u8>, BodyError> {
        match self {
            UpBody::Raw { head, mut stream } => read_body(&mut stream, &head, MAX_PASS, true),
            UpBody::Plain(mut r) => {
                let mut out = Vec::new();
                r.read_to_end(&mut out)?;
                Ok(out)
            }
        }
    }
}

/// Write the app's answer as it came (headers already fixed).
fn pass_through(
    stream: &mut TcpStream,
    status: u16,
    mut h: Vec<(String, String)>,
    body: UpBody,
    no_body: bool,
) {
    match body {
        UpBody::Raw {
            head,
            stream: mut up,
        } => {
            if write_head(stream, status, &h).is_err() || no_body {
                return;
            }
            let length = h
                .iter()
                .find(|(k, _)| k == "content-length")
                .and_then(|(_, v)| v.trim().parse::<u64>().ok())
                .filter(|_| !h.iter().any(|(k, _)| k == "transfer-encoding"));
            let mut reader = Cursor::new(head.rest).chain(&mut up);
            let _ = match length {
                Some(n) => io::copy(&mut reader.take(n), stream),
                None => io::copy(&mut reader, stream),
            };
        }
        UpBody::Plain(mut r) => {
            h.retain(|(k, _)| k != "transfer-encoding");
            if write_head(stream, status, &h).is_err() || no_body {
                return;
            }
            let _ = io::copy(&mut r, stream);
        }
    }
    let _ = stream.flush();
}

/// `decode(buf, enc)`: the body without its content encoding; `None` for an unknown encoding
/// or one that does not decode.
fn decode(buf: &[u8], enc: Option<&str>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let ok = match enc.unwrap_or("") {
        "" | "identity" => return Some(buf.to_vec()),
        "gzip" | "x-gzip" => flate2::read::MultiGzDecoder::new(buf).read_to_end(&mut out),
        "br" => brotli_decompressor::Decompressor::new(buf, 4096).read_to_end(&mut out),
        "deflate" => flate2::read::ZlibDecoder::new(buf).read_to_end(&mut out),
        _ => return None,
    };
    ok.ok().map(|_| out)
}

/// The request to an http app, over a plain socket.
fn http_upstream(
    t: &Target,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> Result<Upstream, String> {
    let host = t.hostname.trim_start_matches('[').trim_end_matches(']');
    let mut up = TcpStream::connect((host, t.connect_port()))
        .map_err(|e| format!("connect {e} {host}:{}", t.connect_port()))?;
    let _ = up.set_read_timeout(Some(Duration::from_secs(300)));
    let mut req = format!("{method} {path} HTTP/1.1\r\n");
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("connection: close\r\n\r\n");
    up.write_all(req.as_bytes())
        .and_then(|_| up.write_all(body))
        .map_err(|e| e.to_string())?;
    let head = read_head(&mut up)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "socket hang up".to_string())?;
    let status = head.start[1]
        .parse()
        .map_err(|_| "bad status line".to_string())?;
    Ok(Upstream {
        status,
        headers: head.headers.clone(),
        body: UpBody::Raw { head, stream: up },
    })
}

/// The request to an https app (self-signed certificates accepted, as `rejectUnauthorized:
/// false` in Node).
fn https_upstream(
    t: &Target,
    method: &str,
    path: &str,
    headers: &[(String, String)],
    body: Vec<u8>,
) -> Result<Upstream, String> {
    let tls = ureq::tls::TlsConfig::builder()
        .disable_verification(true)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .max_redirects(0)
        .http_status_as_error(false)
        .allow_non_standard_methods(true)
        .proxy(None)
        .tls_config(tls)
        .build()
        .into();
    let mut builder = ureq::http::Request::builder()
        .method(method)
        .uri(format!("{}{path}", t.origin()));
    for (k, v) in headers {
        if !matches!(k.as_str(), "host" | "content-length") {
            builder = builder.header(k.as_str(), v.as_str());
        }
    }
    let request = builder.body(body).map_err(|e| e.to_string())?;
    let response = agent.run(request).map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let reader = response
        .into_body()
        .into_with_config()
        .limit(MAX_PASS as u64)
        .reader();
    Ok(Upstream {
        status,
        headers,
        body: UpBody::Plain(Box::new(reader)),
    })
}

/// `queueShot`: one screenshot at a time, in comment order, of the app itself (not the proxy):
/// what the route renders, without the review UI.
fn spawn_shots(root: PathBuf, origin: String) -> Sender<Shot> {
    let (tx, rx) = channel::<Shot>();
    std::thread::spawn(move || {
        for (id, route, vw, vh) in rx {
            let out = project_paths(&root)
                .dfw
                .join("shots")
                .join("live")
                .join(format!("{id}.png"));
            let job = Job {
                url: format!("{origin}{route}"),
                width: vw,
                height: vh,
                out: out.clone(),
                full_page: true,
                selector: None,
                scale: 1.0,
            };
            match screenshot(&[job], 1) {
                Ok(results) => match results.into_iter().next() {
                    Some(r) if r.ok => {
                        let rel = to_posix(&relative_path(&root, &out));
                        if let Err(e) = set_comment_screenshot(&root, &id, &rel) {
                            log(&format!("screenshot for {id} failed: {}", e.message()));
                        }
                    }
                    Some(r) => log(&format!(
                        "screenshot for {id} failed: {}",
                        r.error.unwrap_or_default()
                    )),
                    None => {}
                },
                Err(e) => log(&format!("screenshot for {id} failed: {}", e.message())),
            }
        }
    });
    tx
}

/// `startLiveServer(opts)`: listen on 127.0.0.1:`port` (never 0.0.0.0). For a localhost app
/// the proxy is served as `http://localhost:<port>/` and also listens on `[::1]:<port>`, since
/// "localhost" may resolve to ::1 first (macOS). Returns the server, its listeners and the URL.
pub fn start_live_server(
    opts: &LiveOptions,
    port: u16,
) -> Result<(Arc<LiveServer>, Vec<TcpListener>, String), DfwError> {
    let mut server = LiveServer::new(opts)?;
    let v4 = TcpListener::bind(("127.0.0.1", port))?;
    let port = v4.local_addr()?.port();
    let mut listeners = vec![v4];
    if server.public_host == "localhost" {
        match TcpListener::bind(("::1", port)) {
            Ok(v6) => listeners.push(v6),
            // No IPv6 loopback: localhost resolves to 127.0.0.1 only, fine. Anything else on
            // [::1]:port could answer "localhost" instead of us: use the 127.0.0.1 URL.
            Err(e) if e.kind() == io::ErrorKind::AddrInUse => {
                server.public_host = "127.0.0.1".into();
                log(&format!("[::1]:{port} is in use; serving http://127.0.0.1:{port}/ (cookies set by the app on localhost will not reach it)"));
            }
            Err(_) => {}
        }
    }
    let url = format!("http://{}:{port}/", server.public_host);
    Ok((Arc::new(server), listeners, url))
}
