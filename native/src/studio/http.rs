//! A minimal HTTP/1.1 layer on `std::net` for the Studio and the review proxy (new; Node had
//! `node:http`). One thread per connection, one response per connection (`Connection: close`).
//!
//! Why not `tiny_http`: the Studio streams server-sent events that must reach the page as soon
//! as they are written, and the review proxy needs the raw socket for WebSocket upgrades, the
//! raw header list (several `Set-Cookie`, header case for the replayed upgrade) and a body that
//! streams through untouched. Both are a few lines on a `TcpStream` and awkward through
//! `tiny_http`'s buffered response writer; with this layer the two servers share one parser.
//!
//! Over 800 lines only with its tests: every cap and deadline of this layer is tested against a real
//! socket next to the code.

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// Largest request or response head accepted (Node's default is 16 KiB; dev servers send big
/// cookies, so a little more).
/// HTTP methods the servers read (as `net/http`'s `MethodGet`, ...).
pub mod method {
    pub const GET: &str = "GET";
    pub const HEAD: &str = "HEAD";
    pub const POST: &str = "POST";
    pub const PATCH: &str = "PATCH";
    pub const OPTIONS: &str = "OPTIONS";

    /// A method that only reads: nothing is written or started for it (`security-privacy.md`
    /// S7), and it needs no `Origin` check.
    pub fn is_read_only(m: &str) -> bool {
        matches!(m, GET | HEAD | OPTIONS)
    }
}

/// The status codes the servers send (as `net/http`'s `StatusOK`, ...).
pub mod status {
    pub const OK: u16 = 200;
    pub const CREATED: u16 = 201;
    pub const NO_CONTENT: u16 = 204;
    pub const NOT_MODIFIED: u16 = 304;
    pub const BAD_REQUEST: u16 = 400;
    pub const FORBIDDEN: u16 = 403;
    pub const NOT_FOUND: u16 = 404;
    pub const PAYLOAD_TOO_LARGE: u16 = 413;
    pub const INTERNAL_SERVER_ERROR: u16 = 500;
    pub const SERVICE_UNAVAILABLE: u16 = 503;

    /// The status of a failed route: 400 for an error the user can fix, 500 for the others.
    pub fn of(err: &crate::core::error::DfwError) -> u16 {
        if matches!(err, crate::core::error::DfwError::User { .. }) {
            BAD_REQUEST
        } else {
            INTERNAL_SERVER_ERROR
        }
    }
}

/// The error texts the servers answer with, defined once (`{"error": <text>}`).
pub mod message {
    pub const NOT_FOUND: &str = "not found";
    pub const URI_MALFORMED: &str = "URI malformed";
    pub const TOO_LARGE: &str = "Request too large.";
    pub const MALFORMED: &str = "Malformed request.";
    pub const BUSY: &str = "Too many connections; try again in a moment.";
}

/// The body of every error answer: `{"error": message}`.
pub fn error_body(message: &str) -> serde_json::Value {
    serde_json::json!({ "error": message })
}

pub const MAX_HEAD: usize = 64 * 1024;

/// How long one read of a request (head or body) may wait.
pub const READ_TIMEOUT: Duration = Duration::from_secs(30);

/// How long a client may take to send its whole request head: a client that sends one byte at
/// a time must not hold a connection for ever.
pub const HEAD_DEADLINE: Duration = Duration::from_secs(10);

/// How long one write of an answer may wait for a client that does not read.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

/// Most connections one server handles at once; above it a client gets a 503. One thread per
/// connection: without a cap a few thousand slow clients exhaust the threads and the process.
pub const MAX_CONNECTIONS: usize = 512;

/// The marker of a head over [`MAX_HEAD`] inside an `io::Error`, so callers answer 413.
#[derive(Debug)]
struct HeadTooLarge;

impl std::fmt::Display for HeadTooLarge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("message head too large")
    }
}

impl std::error::Error for HeadTooLarge {}

/// The answer to a request that could not be read: 413 for a head or body over its limit, 400
/// for a malformed one; `None` when the client is gone or too slow (nothing to answer).
pub fn refusal(err: &io::Error) -> Option<(u16, &'static str)> {
    if err
        .get_ref()
        .is_some_and(|inner| inner.is::<HeadTooLarge>())
    {
        return Some((status::PAYLOAD_TOO_LARGE, message::TOO_LARGE));
    }
    (err.kind() == io::ErrorKind::InvalidData).then_some((status::BAD_REQUEST, message::MALFORMED))
}

/// A reader over a socket that gives up at `end`: every read waits at most until then (and
/// never longer than [`READ_TIMEOUT`]).
pub struct Deadline<'a> {
    pub stream: &'a TcpStream,
    pub end: Instant,
}

impl Read for Deadline<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let left = self.end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "request head deadline passed",
            ));
        }
        self.stream.set_read_timeout(Some(left.min(READ_TIMEOUT)))?;
        let mut s = self.stream;
        s.read(out)
    }
}

/// Read a request head from a client socket within [`HEAD_DEADLINE`]; the socket keeps
/// [`READ_TIMEOUT`] for the body and [`WRITE_TIMEOUT`] for the answer.
pub fn read_request_head(stream: &mut TcpStream) -> io::Result<Option<Head>> {
    stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let mut reader = Deadline {
        stream: &*stream,
        end: Instant::now() + HEAD_DEADLINE,
    };
    let head = read_head(&mut reader);
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    head
}

/// A short JSON error answer (`{"error": message}`) for a request that was refused before any
/// route saw it.
pub fn write_error(stream: &mut impl Write, method: &str, code: u16, message: &str) {
    let body = crate::core::util::to_json_compact(&error_body(message));
    let headers = [
        (
            "Content-Type".to_string(),
            "application/json; charset=utf-8".to_string(),
        ),
        ("Cache-Control".to_string(), "no-store".to_string()),
    ];
    // The client may be gone; there is nobody left to tell.
    let _ = write_response(stream, method, code, &headers, body.as_bytes());
}

/// The count of open connections of one server, shared by its threads.
#[derive(Debug, Clone, Default)]
pub struct Connections(Arc<AtomicUsize>);

/// One counted connection; dropping it frees the place.
pub struct Slot(Arc<AtomicUsize>);

impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl Connections {
    /// A place for one more connection, or `None` at `max`.
    pub fn enter(&self, max: usize) -> Option<Slot> {
        let taken = self.0.fetch_add(1, Ordering::SeqCst);
        let slot = Slot(Arc::clone(&self.0));
        (taken < max).then_some(slot)
    }
}

/// Handle `stream` on a thread of its own when there is a place for it: over the cap the
/// client gets a 503, and when no thread can be started the connection is dropped (the server
/// keeps running either way).
pub fn spawn_connection(
    conns: &Connections,
    max: usize,
    mut stream: TcpStream,
    handle: impl FnOnce(TcpStream) + Send + 'static,
) {
    let Some(slot) = conns.enter(max) else {
        // A short answer that fits the socket buffer: the accept loop must not wait on it.
        let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));
        write_error(
            &mut stream,
            method::GET,
            status::SERVICE_UNAVAILABLE,
            message::BUSY,
        );
        return;
    };
    // No thread (the system is out of them): the closure, the stream and the slot are dropped,
    // which closes the connection and frees the place.
    let _ = std::thread::Builder::new().spawn(move || {
        let _slot = slot;
        handle(stream);
    });
}

/// 16 random bytes in base64 (a CSP nonce). The keys of `RandomState` come from the operating
/// system's random source, so the hashes are unpredictable without a random-number crate.
pub fn random_base64() -> String {
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
    crate::core::encoding::base64_encode(&bytes)
}

/// A parsed message head (request or response) and the bytes read past it.
#[derive(Debug, Clone, Default)]
pub struct Head {
    /// Request: method, target, version (`GET`, `/x?y`, `1.1`). Response: version, status,
    /// reason (`1.1`, `200`, `OK`).
    pub start: [String; 3],
    /// Header lines as sent: name with its case, value trimmed.
    pub headers: Vec<(String, String)>,
    /// Bytes after the blank line that were already read (the start of the body).
    pub rest: Vec<u8>,
}

impl Head {
    /// First header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// Request method.
    pub fn method(&self) -> &str {
        &self.start[0]
    }

    /// Request target as sent (`req.url`).
    pub fn target(&self) -> &str {
        &self.start[1]
    }

    /// An upgrade request (`Connection: upgrade` and an `Upgrade` header), which Node hands to
    /// the server's `upgrade` event instead of the request handler.
    pub fn is_upgrade(&self) -> bool {
        self.header("upgrade").is_some()
            && self.header("connection").is_some_and(|c| {
                c.split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
            })
    }
}

/// [`random_base64`] as a token safe in a header and a URL (base64url, no padding).
pub fn random_token() -> String {
    random_base64()
        .trim_end_matches('=')
        .replace('+', "-")
        .replace('/', "_")
}

/// Read a message head. `Ok(None)` when the peer closed before sending anything.
pub fn read_head(stream: &mut impl Read) -> io::Result<Option<Head>> {
    let mut buf: Vec<u8> = Vec::with_capacity(2048);
    let mut chunk = [0u8; 4096];
    let end = loop {
        if let Some(i) = find(&buf, b"\r\n\r\n") {
            break i;
        }
        if buf.len() > MAX_HEAD {
            return Err(io::Error::new(io::ErrorKind::InvalidData, HeadTooLarge));
        }
        let n = stream.read(&mut chunk)?;
        if n == 0 {
            return if buf.is_empty() {
                Ok(None)
            } else {
                Err(invalid("connection closed inside the message head"))
            };
        }
        buf.extend_from_slice(&chunk[..n]);
    };
    let text = String::from_utf8_lossy(&buf[..end]).into_owned();
    let mut lines = text.split("\r\n");
    let first = lines.next().unwrap_or_default();
    let mut parts = first.splitn(3, ' ');
    let mut start: [String; 3] = Default::default();
    for slot in start.iter_mut() {
        *slot = parts.next().unwrap_or_default().to_string();
    }
    if start[0].is_empty() || start[1].is_empty() {
        return Err(invalid("bad start line"));
    }
    let mut headers = Vec::new();
    for line in lines {
        let Some((name, value)) = line.split_once(':') else {
            return Err(invalid("bad header line"));
        };
        if name.is_empty() || name.contains(' ') {
            return Err(invalid("bad header name"));
        }
        headers.push((name.to_string(), value.trim().to_string()));
    }
    Ok(Some(Head {
        start,
        headers,
        rest: buf[end + 4..].to_vec(),
    }))
}

/// Strip the `HTTP/` of a request's version (`req.httpVersion`).
pub fn http_version(head: &Head) -> &str {
    head.start[2].strip_prefix("HTTP/").unwrap_or("1.1")
}

/// Why a body could not be read.
#[derive(Debug)]
pub enum BodyError {
    /// More than the limit.
    TooLarge,
    /// The socket failed or the framing was broken.
    Io(io::Error),
}

impl From<io::Error> for BodyError {
    fn from(err: io::Error) -> Self {
        BodyError::Io(err)
    }
}

/// Read the whole body of a message whose head is `head` (Content-Length, chunked, or, for a
/// response only, until the peer closes).
pub fn read_body(
    stream: &mut impl Read,
    head: &Head,
    limit: usize,
    is_response: bool,
) -> Result<Vec<u8>, BodyError> {
    let mut reader = Prefixed {
        first: &head.rest,
        at: 0,
        inner: stream,
    };
    let chunked = head
        .header("transfer-encoding")
        .is_some_and(|t| t.to_ascii_lowercase().contains("chunked"));
    if chunked {
        return read_chunked(&mut reader, limit);
    }
    if let Some(len) = head.header("content-length") {
        // Digits only: `-1`, `+5` or an empty value is a malformed request (400), and a number
        // too big for memory is too large (413).
        let len = len.trim();
        if len.is_empty() || !len.bytes().all(|b| b.is_ascii_digit()) {
            return Err(invalid("bad content-length").into());
        }
        let len: usize = len.parse().map_err(|_| BodyError::TooLarge)?;
        if len > limit {
            return Err(BodyError::TooLarge);
        }
        let mut body = vec![0u8; len];
        reader.read_exact(&mut body)?;
        return Ok(body);
    }
    if !is_response {
        return Ok(Vec::new());
    }
    let mut body = Vec::new();
    reader
        .by_ref()
        .take(limit as u64 + 1)
        .read_to_end(&mut body)?;
    if body.len() > limit {
        return Err(BodyError::TooLarge);
    }
    Ok(body)
}

/// Read and drop what is left of a body that was refused (at most `cap` bytes), so the client
/// is not cut off while it still sends and can read the answer. Bytes only: nothing is parsed.
pub fn drain(stream: &mut impl Read, head: &Head, cap: u64) {
    // Without a length a chunked body runs until the client stops; without either there is
    // no body.
    let unframed = if head.header("transfer-encoding").is_some() {
        cap
    } else {
        0
    };
    let left = head
        .header("content-length")
        .and_then(|l| l.trim().parse::<u64>().ok())
        .map_or(unframed, |l| l.saturating_sub(head.rest.len() as u64))
        .min(cap);
    // Cleanup of a refused request: the answer does not depend on it.
    let _ = io::copy(&mut stream.take(left), &mut io::sink());
}

fn read_chunked(reader: &mut impl Read, limit: usize) -> Result<Vec<u8>, BodyError> {
    let mut body = Vec::new();
    loop {
        let line = read_line(reader)?;
        let size = line.split(';').next().unwrap_or_default().trim();
        if size.is_empty() || !size.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(invalid("bad chunk size").into());
        }
        // A size that does not fit, or that passes the limit, is too large; checked so that a
        // chunk of `ffffffffffffffff` cannot overflow (release builds abort on a panic).
        let size = usize::from_str_radix(size, 16).map_err(|_| BodyError::TooLarge)?;
        if size == 0 {
            // Trailers until the blank line.
            while !read_line(reader)?.is_empty() {}
            return Ok(body);
        }
        if size > limit.saturating_sub(body.len()) {
            return Err(BodyError::TooLarge);
        }
        let start = body.len();
        body.resize(start + size, 0);
        reader.read_exact(&mut body[start..])?;
        read_line(reader)?;
    }
}

fn read_line(reader: &mut impl Read) -> io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        if reader.read(&mut byte)? == 0 {
            return Err(invalid("connection closed inside a chunk"));
        }
        if byte[0] == b'\n' {
            break;
        }
        if line.len() > 4096 {
            return Err(invalid("chunk line too long"));
        }
        line.push(byte[0]);
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

/// A reader that first yields bytes already read, then the stream.
pub struct Prefixed<'a, R: Read> {
    pub first: &'a [u8],
    pub at: usize,
    pub inner: &'a mut R,
}

impl<R: Read> Read for Prefixed<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.at < self.first.len() {
            let n = out.len().min(self.first.len() - self.at);
            out[..n].copy_from_slice(&self.first[self.at..self.at + n]);
            self.at += n;
            return Ok(n);
        }
        self.inner.read(out)
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn invalid(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// Reason phrase of a status code (Node's `STATUS_CODES`).
pub fn reason(code: u16) -> &'static str {
    match code {
        101 => "Switching Protocols",
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "",
    }
}

/// Write a status line and headers, then `Connection: close` and the blank line.
pub fn write_head(
    stream: &mut impl Write,
    code: u16,
    headers: &[(String, String)],
) -> io::Result<()> {
    let mut head = format!("HTTP/1.1 {code} {}\r\n", reason(code));
    for (k, v) in headers {
        // A value with a line break would split the head: Node refuses such a header too.
        if k.contains(['\r', '\n', ':']) || v.contains(['\r', '\n']) {
            continue;
        }
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    stream.write_all(head.as_bytes())
}

/// A complete response with a `Content-Length` (the body is left out for `HEAD`).
pub fn write_response(
    stream: &mut impl Write,
    method: &str,
    code: u16,
    headers: &[(String, String)],
    body: &[u8],
) -> io::Result<()> {
    let mut all = headers.to_vec();
    if !all
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        all.push(("Content-Length".into(), body.len().to_string()));
    }
    write_head(stream, code, &all)?;
    if method != method::HEAD && code != status::NO_CONTENT && code != status::NOT_MODIFIED {
        stream.write_all(body)?;
    }
    stream.flush()
}

/// `new URL(target, base).pathname`: the path of a request target with `.` and `..` segments
/// resolved (also their `%2e` spellings), `\` read as `/`, and query and fragment cut.
pub fn url_pathname(target: &str) -> String {
    let mut t = target;
    for scheme in ["http://", "https://"] {
        if t.len() >= scheme.len() && t[..scheme.len()].eq_ignore_ascii_case(scheme) {
            let rest = &t[scheme.len()..];
            t = rest.find(['/', '\\', '?', '#']).map_or("", |i| &rest[i..]);
        }
    }
    let t = t
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .replace('\\', "/");
    let mut out: Vec<&str> = Vec::new();
    let segments: Vec<&str> = t.split('/').skip(1).collect();
    let last = segments.len().saturating_sub(1);
    for (i, seg) in segments.iter().enumerate() {
        let low = seg.to_ascii_lowercase();
        let dots = low.replace("%2e", ".");
        if dots == ".." {
            out.pop();
            if i == last {
                out.push("");
            }
        } else if dots == "." {
            if i == last {
                out.push("");
            }
        } else {
            out.push(seg);
        }
    }
    let encoded: Vec<String> = out.iter().map(|s| encode_path_segment(s)).collect();
    format!("/{}", encoded.join("/"))
}

/// The characters the URL parser percent-encodes in a path (space, quotes, `<>`, `` ` ``,
/// `{}`, controls and non-ASCII), so a decoded path reads the same as in Node.
fn encode_path_segment(seg: &str) -> String {
    let mut out = String::with_capacity(seg.len());
    for b in seg.bytes() {
        if b <= 0x20 || b >= 0x7f || matches!(b, b'"' | b'<' | b'>' | b'`' | b'{' | b'}' | b'#') {
            out.push_str(&format!("%{b:02X}"));
        } else {
            out.push(b as char);
        }
    }
    out
}

/// `decodeURIComponent`; `None` for a malformed escape ("URI malformed" in Node).
pub fn decode_uri_component(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// The query parameter `name` of a request target (`url.searchParams.get(name)`).
pub fn query_param(target: &str, name: &str) -> Option<String> {
    let query = target.split('#').next()?.split_once('?')?.1;
    query.split('&').find_map(|pair| {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let decode = |s: &str| decode_uri_component(&s.replace('+', " "));
        (decode(k)? == name).then(|| decode(v)).flatten()
    })
}

/// Node's `req.headers` / `res.headers`: names in lower case, repeated headers merged
/// (`set-cookie` kept as a list, `cookie` joined with `; `, the single-value headers keep the
/// first, the rest joined with `, `).
pub fn node_headers(raw: &[(String, String)]) -> Vec<(String, String)> {
    const SINGLE: [&str; 17] = [
        "age",
        "authorization",
        "content-length",
        "content-type",
        "etag",
        "expires",
        "from",
        "host",
        "if-modified-since",
        "if-unmodified-since",
        "last-modified",
        "location",
        "max-forwards",
        "proxy-authorization",
        "referer",
        "retry-after",
        "server",
    ];
    let mut out: Vec<(String, String)> = Vec::new();
    for (k, v) in raw {
        let name = k.to_ascii_lowercase();
        if name == "set-cookie" {
            out.push((name, v.clone()));
            continue;
        }
        match out.iter_mut().find(|(n, _)| *n == name) {
            Some(_) if SINGLE.contains(&name.as_str()) || name == "user-agent" => {}
            Some((_, value)) => {
                value.push_str(if name == "cookie" { "; " } else { ", " });
                value.push_str(v);
            }
            None => out.push((name, v.clone())),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_resolve_like_the_url_parser() {
        assert_eq!(url_pathname("/design/../x"), "/x");
        assert_eq!(url_pathname("/a/%2e%2E/b?q=1#f"), "/b");
        assert_eq!(url_pathname("/a/./b/."), "/a/b/");
        assert_eq!(url_pathname("/a/.."), "/");
        assert_eq!(url_pathname("/../../etc/passwd"), "/etc/passwd");
        assert_eq!(url_pathname("\\design\\x"), "/design/x");
        assert_eq!(url_pathname("http://h:1/p/q"), "/p/q");
        assert_eq!(url_pathname("/a b"), "/a%20b");
        assert_eq!(url_pathname("/..%2F..%2Fx"), "/..%2F..%2Fx");
    }

    #[test]
    fn decode_uri_component_refuses_malformed_escapes() {
        assert_eq!(
            decode_uri_component("/a%20b%2Fc").as_deref(),
            Some("/a b/c")
        );
        assert_eq!(decode_uri_component("%zz"), None);
        assert_eq!(decode_uri_component("%e2%82"), None);
        assert_eq!(decode_uri_component("%"), None);
    }

    #[test]
    fn heads_and_bodies_are_read() {
        let raw = b"POST /x HTTP/1.1\r\nHost: a\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n";
        let mut r: &[u8] = raw;
        let head = read_head(&mut r).unwrap().unwrap();
        assert_eq!((head.method(), head.target()), ("POST", "/x"));
        assert_eq!(head.header("HOST"), Some("a"));
        let body = read_body(&mut r, &head, 100, false).unwrap();
        assert_eq!(body, b"abcde");
        let raw = b"GET / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello";
        let mut r: &[u8] = raw;
        let head = read_head(&mut r).unwrap().unwrap();
        assert!(matches!(
            read_body(&mut r, &head, 4, false),
            Err(BodyError::TooLarge)
        ));
    }

    fn head_of(raw: &[u8]) -> (Head, &[u8]) {
        let mut r: &[u8] = raw;
        let head = read_head(&mut r).unwrap().unwrap();
        (head, r)
    }

    /// a chunk size that does not fit in memory is too large, never an overflow (release
    /// builds abort on a panic)
    #[test]
    fn huge_chunk_sizes_are_too_large_not_a_panic() {
        for size in ["ffffffffffffffff", "fffffffffffffffffffff", "10000"] {
            let raw = format!(
                "POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n{size}\r\n"
            );
            let (head, mut r) = head_of(raw.as_bytes());
            assert!(
                matches!(
                    read_body(&mut r, &head, 100, false),
                    Err(BodyError::TooLarge)
                ),
                "{size}"
            );
        }
        let (head, mut r) = head_of(
            b"POST / HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n+5\r\nabcde\r\n0\r\n\r\n",
        );
        match read_body(&mut r, &head, 100, false) {
            Err(BodyError::Io(e)) => assert_eq!(refusal(&e), Some((400, message::MALFORMED))),
            other => panic!("{other:?}"),
        }
    }

    /// a length that is not a number is malformed (400); one too big for memory is too large
    #[test]
    fn content_lengths_are_digits_only() {
        for bad in ["-1", "+5", "", "5 5", "0x10"] {
            let raw = format!("POST / HTTP/1.1\r\nContent-Length: {bad}\r\n\r\n");
            let (head, mut r) = head_of(raw.as_bytes());
            match read_body(&mut r, &head, 100, false) {
                Err(BodyError::Io(e)) => assert_eq!(refusal(&e).map(|r| r.0), Some(400), "{bad}"),
                other => panic!("{bad}: {other:?}"),
            }
        }
        let (head, mut r) =
            head_of(b"POST / HTTP/1.1\r\nContent-Length: 99999999999999999999999\r\n\r\n");
        assert!(matches!(
            read_body(&mut r, &head, 100, false),
            Err(BodyError::TooLarge)
        ));
    }

    /// a head over MAX_HEAD is answered 413, a broken one 400
    #[test]
    fn oversized_and_broken_heads_have_answers() {
        let mut big = b"GET / HTTP/1.1\r\nX: ".to_vec();
        big.extend(std::iter::repeat_n(b'a', MAX_HEAD + 10));
        let mut r: &[u8] = &big;
        let err = read_head(&mut r).unwrap_err();
        assert_eq!(refusal(&err), Some((413, message::TOO_LARGE)));
        let mut r: &[u8] = b"GET / HTTP/1.1\r\nno colon\r\n\r\n";
        let err = read_head(&mut r).unwrap_err();
        assert_eq!(refusal(&err), Some((400, message::MALFORMED)));
        let timed_out = io::Error::new(io::ErrorKind::TimedOut, "slow");
        assert_eq!(refusal(&timed_out), None);
    }

    /// a client that sends its head one byte at a time is cut off at the head deadline
    #[test]
    fn the_head_deadline_stops_a_slow_client() {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let slow = std::thread::spawn(move || {
            let mut c = TcpStream::connect(("127.0.0.1", port)).unwrap();
            for _ in 0..40 {
                if c.write_all(b"a").is_err() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });
        let (server, _) = listener.accept().unwrap();
        let started = Instant::now();
        let mut reader = Deadline {
            stream: &server,
            end: Instant::now() + Duration::from_millis(300),
        };
        let err = read_head(&mut reader).unwrap_err();
        // The last read times out (WouldBlock on Unix) or the deadline is already gone.
        assert!(
            matches!(
                err.kind(),
                io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
            ),
            "{err}"
        );
        assert_eq!(
            refusal(&err),
            None,
            "nothing to answer a client that is too slow"
        );
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "{:?}",
            started.elapsed()
        );
        drop(server);
        slow.join().unwrap();
    }

    /// over the connection cap a client gets a 503 and the place comes back when one closes
    #[test]
    fn the_connection_cap_answers_503() {
        let conns = Connections::default();
        let first = conns.enter(1).unwrap();
        assert!(conns.enter(1).is_none());
        drop(first);
        assert!(conns.enter(1).is_some());

        let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let (release, wait) = std::sync::mpsc::channel::<()>();
        let held = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (a, _) = listener.accept().unwrap();
        spawn_connection(&conns, 1, a, move |_s| {
            let _ = wait.recv();
        });
        let mut second = TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (b, _) = listener.accept().unwrap();
        spawn_connection(&conns, 1, b, |_s| panic!("over the cap"));
        let mut answer = String::new();
        second.read_to_string(&mut answer).unwrap();
        assert!(answer.starts_with("HTTP/1.1 503"), "{answer}");
        release.send(()).unwrap();
        drop(held);
    }

    #[test]
    fn headers_merge_like_node() {
        let raw: Vec<(String, String)> = [
            ("Set-Cookie", "a=1"),
            ("set-cookie", "b=2"),
            ("Cookie", "x=1"),
            ("cookie", "y=2"),
            ("Accept", "a"),
            ("accept", "b"),
            ("Content-Type", "text/html"),
            ("content-type", "text/plain"),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let merged = node_headers(&raw);
        let get = |k: &str| -> Vec<&str> {
            merged
                .iter()
                .filter(|(n, _)| n == k)
                .map(|(_, v)| v.as_str())
                .collect()
        };
        assert_eq!(get("set-cookie"), ["a=1", "b=2"]);
        assert_eq!(get("cookie"), ["x=1; y=2"]);
        assert_eq!(get("accept"), ["a, b"]);
        assert_eq!(get("content-type"), ["text/html"]);
    }

    #[test]
    fn query_params_are_decoded() {
        assert_eq!(
            query_param("/x?refresh=1&page=a%20b", "page").as_deref(),
            Some("a b")
        );
        assert_eq!(query_param("/x?refresh=1", "page"), None);
    }
}
