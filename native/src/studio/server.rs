//! Ports `core/studio/server.mjs`: the local Design Studio. A web server on 127.0.0.1 that
//! renders the project's `design/` as a multi-screen canvas, takes direct edits and collects
//! element-level review comments. Split by job: `api` (the data it serves), `pages` (canvas
//! pages), `sse` (events), `watch` (live reload), `penpot_routes`, `http` (the wire).

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{json, Value};

use super::api::{self, DesignFile};
use super::http::{
    decode_uri_component, read_body, read_head, url_pathname, write_response, BodyError, Head,
    READ_TIMEOUT,
};
use super::sse::Clients;
use super::{penpot_routes, watch};
use crate::assets::studio_file;
use crate::core::config::{load_config, studio_port};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::core::util::to_json_compact;
use crate::design::compose::build_system;
use crate::design::edit::{apply_edit, undo_edit};

use crate::review::comments::{
    add_comment, all_comments, current_session, new_session, update_comment, Update,
};
use crate::workflow::state::{load_state, summarize};

/// Largest request body the Studio reads (edits, comments and Penpot changes are far smaller).
const MAX_BODY: usize = 64 * 1024 * 1024;

/// A running Studio: the project, its config and the connected pages.
pub struct Studio {
    pub root: PathBuf,
    pub config: Value,
    pub port: u16,
    pub clients: Clients,
    shooting: AtomicBool,
    /// Held while a Penpot export runs.
    pub penpot_exporting: Mutex<()>,
}

/// A request as the routes see it.
pub struct Request {
    pub head: Head,
    /// `decodeURIComponent(url.pathname)`.
    pub path: String,
    pub body: Vec<u8>,
}

impl Request {
    /// `body(req)`: the JSON body, `{}` when empty.
    pub fn json(&self) -> Result<Value, DfwError> {
        let text = String::from_utf8_lossy(&self.body);
        if text.is_empty() {
            return Ok(json!({}));
        }
        Ok(serde_json::from_str(&text)?)
    }
}

/// A response as the routes build it.
#[derive(Debug, Clone, PartialEq)]
pub struct Response {
    pub code: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    /// `send(res, code, body, type)`: content type and `Cache-Control: no-store`.
    pub fn send(code: u16, body: impl Into<Vec<u8>>, content_type: &str) -> Self {
        Response {
            code,
            headers: vec![
                ("Content-Type".into(), content_type.into()),
                ("Cache-Control".into(), "no-store".into()),
            ],
            body: body.into(),
        }
    }

    /// `send(res, code, object)`: JSON as `JSON.stringify` writes it.
    pub fn json(code: u16, body: &Value) -> Self {
        Self::send(code, to_json_compact(body), JSON)
    }

    /// `{ error: 'not found' }` with 404.
    pub fn not_found() -> Self {
        Self::json(404, &json!({ "error": "not found" }))
    }
}

/// Content type of JSON answers.
pub const JSON: &str = "application/json; charset=utf-8";
const HTML: &str = "text/html; charset=utf-8";
const JS: &str = "text/javascript; charset=utf-8";

impl Studio {
    /// A Studio for `root` on `port` (`--port`, else the config's `studio.port`, else 4600).
    pub fn new(root: PathBuf, port: Option<u16>) -> Result<Self, DfwError> {
        let config = load_config(Some(&root))?;
        let port = port.unwrap_or_else(|| studio_port(&config));
        Ok(Studio {
            root,
            config,
            port,
            clients: Clients::default(),
            shooting: AtomicBool::new(false),
            penpot_exporting: Mutex::new(()),
        })
    }

    /// `foreign(req)`: the Studio is for the local user only. A web page the user visits could
    /// otherwise post comments (which the agent reads as the user's words) or edits, and a
    /// DNS-rebinding page could read it: the Host must be this server, and a request that
    /// changes something must not come from another origin.
    pub fn foreign(&self, head: &Head) -> Option<String> {
        let selves = [
            format!("127.0.0.1:{}", self.port),
            format!("localhost:{}", self.port),
        ];
        let host = head.header("host").unwrap_or_default();
        if !selves.contains(&host.to_lowercase()) {
            let shown = if host.is_empty() { "(none)" } else { host };
            return Some(format!("host {shown} not allowed"));
        }
        let writes = !matches!(head.method(), "GET" | "HEAD" | "OPTIONS");
        if let Some(from) = head.header("origin").filter(|_| writes) {
            if !selves
                .iter()
                .any(|s| s == from.strip_prefix("http://").unwrap_or(from))
            {
                return Some(format!("origin {from} not allowed"));
            }
        }
        None
    }

    /// The route table of server.mjs, in its order. `/__dfw/events` is answered by the caller
    /// (it keeps the socket).
    pub fn route(&self, req: &Request) -> Result<Response, DfwError> {
        let p = req.path.as_str();
        let method = req.head.method();
        let root = self.root.as_path();
        if p == "/" || p == "/index.html" {
            return Ok(asset("app.html", HTML).unwrap_or_else(Response::not_found));
        }
        if let Some(rel) = p.strip_prefix("/__dfw/static/") {
            let clean = api::clamp_rel(rel).join("/");
            let ty = api::mime(api::extname(&clean)).unwrap_or("text/plain");
            return Ok(asset(&clean, ty).unwrap_or_else(Response::not_found));
        }
        if p == "/favicon.ico" {
            return Ok(Response::send(204, "", JSON));
        }
        if p == "/__dfw/overlay.js" {
            return Ok(asset("overlay.js", JS).unwrap_or_else(Response::not_found));
        }
        if let Some(rel) = p.strip_prefix("/design/") {
            return Ok(match api::design_file(root, rel)? {
                DesignFile::Found(body, ty) => Response::send(200, body, ty),
                DesignFile::Missing => Response::not_found(),
            });
        }
        if p == "/__dfw/system.html" {
            return Ok(Response::send(200, api::system_page(root)?, HTML));
        }
        if p == "/__dfw/api/tokens" {
            return Ok(Response::json(200, &api::token_list(root)?));
        }
        if p == "/__dfw/api/components" {
            return Ok(Response::json(200, &api::components(root)?));
        }
        if p == "/__dfw/api/edit" && method == "POST" {
            return Ok(Response::json(
                200,
                &apply_edit(root, &req.json()?)?.to_json(),
            ));
        }
        if p == "/__dfw/api/undo" && method == "POST" {
            return Ok(Response::json(200, &undo_edit(root)?.to_json()));
        }
        if p == "/__dfw/api/shoot" && method == "POST" {
            return self.shoot(req);
        }
        if let Some(rel) = p.strip_prefix("/__dfw/shots/") {
            let f = api::safe_join(&project_paths(root).dfw.join("shots"), rel);
            if !f.to_string_lossy().ends_with(".png") || !f.is_file() {
                return Ok(Response::not_found());
            }
            return Ok(Response::send(200, std::fs::read(&f)?, "image/png"));
        }
        if p.starts_with("/__dfw/penpot/") {
            return penpot_routes::route(self, req);
        }
        if p == "/__dfw/api/manifest" {
            return Ok(Response::json(200, &api::manifest(root, &self.config)?));
        }
        if p == "/__dfw/api/state" {
            let state = load_state(root)?;
            let summary = summarize(state.as_ref())?;
            let session = current_session(root, false)?
                .and_then(|s| s.session.get("id").cloned())
                .filter(|v| js::truthy(v))
                .unwrap_or(Value::Null);
            let body = json!({ "state": state, "summary": summary, "session": session });
            return Ok(Response::json(200, &body));
        }
        if p == "/__dfw/api/comments" && method == "GET" {
            return Ok(Response::json(200, &Value::Array(all_comments(root)?)));
        }
        if p == "/__dfw/api/comments" && method == "POST" {
            let c = add_comment(root, &req.json()?)?;
            self.clients
                .broadcast("comments", &json!({ "id": c.get("id") }));
            return Ok(Response::json(201, &c));
        }
        if let Some(id) = comment_id(p).filter(|_| method == "PATCH") {
            let b = req.json()?;
            // Requests from the studio UI are the user's own actions.
            let update = Update {
                status: b
                    .get("status")
                    .filter(|s| js::truthy(s))
                    .map(|s| js::to_string(Some(s))),
                message: b.get("message").cloned(),
                by: Some(json!("user")),
                evidence: None,
            };
            let c = update_comment(root, Some(id), &update)?;
            self.clients
                .broadcast("comments", &json!({ "id": c.get("id") }));
            return Ok(Response::json(200, &c));
        }
        if p == "/__dfw/api/sessions" && method == "POST" {
            let s = new_session(root, None)?;
            self.clients.broadcast("comments", &json!({}));
            return Ok(Response::json(201, &s.session));
        }
        Ok(Response::not_found())
    }

    fn shoot(&self, req: &Request) -> Result<Response, DfwError> {
        if self.shooting.swap(true, Ordering::SeqCst) {
            return Err(DfwError::new("A screenshot is already running."));
        }
        let result = req
            .json()
            .and_then(|body| api::shoot_frames(&self.root, self.port, &body));
        self.shooting.store(false, Ordering::SeqCst);
        Ok(Response::json(200, &result?))
    }

    /// One connection: read the request, answer it.
    pub fn handle(&self, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(Some(READ_TIMEOUT));
        let Ok(Some(head)) = read_head(&mut stream) else {
            return;
        };
        let method = head.method().to_string();
        let reply = |stream: &mut TcpStream, r: Response| {
            let _ = write_response(stream, &method, r.code, &r.headers, &r.body);
        };
        let body = match read_body(&mut stream, &head, MAX_BODY, false) {
            Ok(body) => body,
            Err(BodyError::TooLarge) => {
                let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(2)));
                super::http::drain(&mut stream, &head);
                return reply(
                    &mut stream,
                    Response::json(413, &json!({ "error": "Request too large." })),
                );
            }
            Err(BodyError::Io(_)) => return,
        };
        if let Some(refused) = self.foreign(&head) {
            return reply(
                &mut stream,
                Response::json(403, &json!({ "error": refused })),
            );
        }
        let Some(path) = decode_uri_component(&url_pathname(head.target())) else {
            return reply(
                &mut stream,
                Response::json(500, &json!({ "error": "URI malformed" })),
            );
        };
        if path == "/__dfw/events" {
            return self.clients.connect(stream);
        }
        let req = Request { head, path, body };
        let response = self.route(&req).unwrap_or_else(|err| {
            let code = if matches!(err, DfwError::User { .. }) {
                400
            } else {
                500
            };
            Response::json(code, &json!({ "error": err.message() }))
        });
        reply(&mut stream, response);
    }
}

/// A file of `core/studio` as a 200 response.
fn asset(rel: &str, content_type: &str) -> Option<Response> {
    studio_file(rel).map(|bytes| Response::send(200, bytes.into_owned(), content_type))
}

/// The id of `/__dfw/api/comments/fb_<digits>`.
fn comment_id(path: &str) -> Option<&str> {
    let id = path.strip_prefix("/__dfw/api/comments/")?;
    let digits = id.strip_prefix("fb_")?;
    (!digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// `dfw __studio-server --root <dir> --port <n>`: build the component CSS, watch the project
/// and serve until the process is stopped. Only returns on a listen error.
pub fn serve(root: &Path, port: Option<u16>) -> Result<i32, DfwError> {
    let studio = Arc::new(Studio::new(root.to_path_buf(), port)?);
    if let Err(err) = build_system(root) {
        eprintln!("dfw studio: buildSystem: {}", err.message());
    }
    let listener = match TcpListener::bind(("127.0.0.1", studio.port)) {
        Ok(l) => l,
        Err(err) => {
            eprintln!("dfw studio: {err}");
            return Ok(1);
        }
    };
    let watcher = Arc::clone(&studio);
    std::thread::spawn(move || watch::run(watcher.root.clone(), &watcher.clients));
    println!(
        "dfw studio: http://127.0.0.1:{}/  (project: {})",
        studio.port,
        root.display()
    );
    for stream in listener.incoming().flatten() {
        let studio = Arc::clone(&studio);
        std::thread::spawn(move || studio.handle(stream));
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::io::{Read, Write};

    fn head(method: &str, headers: &[(&str, &str)]) -> Head {
        Head {
            start: [method.into(), "/".into(), "HTTP/1.1".into()],
            headers: headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            rest: Vec::new(),
        }
    }

    #[test]
    fn foreign_hosts_and_origins_are_refused() {
        let dir = test_dir("studio-foreign");
        let s = Studio::new(dir.clone(), Some(4601)).unwrap();
        assert_eq!(s.foreign(&head("GET", &[("Host", "127.0.0.1:4601")])), None);
        assert_eq!(s.foreign(&head("GET", &[("Host", "LOCALHOST:4601")])), None);
        assert_eq!(
            s.foreign(&head("GET", &[("Host", "evil.example")]))
                .as_deref(),
            Some("host evil.example not allowed")
        );
        assert_eq!(
            s.foreign(&head("GET", &[])).as_deref(),
            Some("host (none) not allowed")
        );
        let post = |origin: &str| {
            s.foreign(&head(
                "POST",
                &[("Host", "127.0.0.1:4601"), ("Origin", origin)],
            ))
        };
        assert_eq!(
            post("https://evil.example").as_deref(),
            Some("origin https://evil.example not allowed")
        );
        assert_eq!(post("http://localhost:4601"), None);
        assert_eq!(post("http://127.0.0.1:4601"), None);
        assert!(post("http://127.0.0.1:4602").is_some());
        // Reads from another origin are fine (the page cannot read the answer without CORS).
        assert_eq!(
            s.foreign(&head(
                "GET",
                &[
                    ("Host", "127.0.0.1:4601"),
                    ("Origin", "https://evil.example")
                ]
            )),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn comment_ids_follow_the_route_pattern() {
        assert_eq!(comment_id("/__dfw/api/comments/fb_001"), Some("fb_001"));
        assert_eq!(comment_id("/__dfw/api/comments/fb_"), None);
        assert_eq!(comment_id("/__dfw/api/comments/fb_1/x"), None);
        assert_eq!(comment_id("/__dfw/api/comments/rv_1"), None);
    }

    /// A raw request to a Studio served on a free port; returns the status and body.
    fn call(port: u16, raw: &str) -> (u16, String) {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(raw.as_bytes()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        let code = out[9..12].parse().unwrap();
        let body = out.split_once("\r\n\r\n").unwrap().1.to_string();
        (code, body)
    }

    fn start(root: &Path) -> (u16, Arc<Studio>) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let studio = Arc::new(Studio::new(root.to_path_buf(), Some(port)).unwrap());
        let s = Arc::clone(&studio);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let s = Arc::clone(&s);
                std::thread::spawn(move || s.handle(stream));
            }
        });
        (port, studio)
    }

    fn fixture() -> PathBuf {
        let dir = test_dir("studio-server");
        let fix = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/penpot/project");
        crate::core::util::copy_dir(&fix, &dir, &[]).unwrap();
        std::fs::create_dir_all(dir.join(".dfw/reviews")).unwrap();
        std::fs::write(
            dir.join(".dfw/state.json"),
            r#"{"design":{"version":1,"status":"draft","locked":false}}"#,
        )
        .unwrap();
        dir
    }

    /// the Studio refuses writes from another origin and requests for another host
    #[test]
    fn the_studio_refuses_foreign_writes_and_hosts() {
        let root = fixture();
        let (port, _studio) = start(&root);
        let comment = r#"{"comment":"from another site","screen":"home"}"#;
        let post = |extra: &str| {
            call(
                port,
                &format!(
                    "POST /__dfw/api/comments HTTP/1.1\r\n{extra}Content-Type: text/plain\r\nContent-Length: {}\r\n\r\n{comment}",
                    comment.len()
                ),
            )
            .0
        };
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        assert_eq!(
            post(&format!("{host}Origin: https://evil.example\r\n")),
            403
        );
        assert_eq!(
            call(
                port,
                "GET /__dfw/api/comments HTTP/1.1\r\nHost: evil.example\r\n\r\n"
            )
            .0,
            403
        );
        assert_eq!(all_comments(&root).unwrap().len(), 0);
        assert_eq!(
            post(&format!("{host}Origin: http://127.0.0.1:{port}\r\n")),
            201
        );
        assert_eq!(
            post(&format!(
                "Host: localhost:{port}\r\nOrigin: http://localhost:{port}\r\n"
            )),
            201
        );
        assert_eq!(post(&host), 201, "curl and the CLI send no Origin");
        assert_eq!(all_comments(&root).unwrap().len(), 3);
        let (code, body) = call(port, &format!("GET /__dfw/nothing HTTP/1.1\r\n{host}\r\n"));
        assert_eq!((code, body.as_str()), (404, r#"{"error":"not found"}"#));
        let (code, body) = call(
            port,
            &format!("GET /design/%E0%A4%A HTTP/1.1\r\n{host}\r\n"),
        );
        assert_eq!((code, body.as_str()), (500, r#"{"error":"URI malformed"}"#));
        let (code, _) = call(
            port,
            &format!("GET /design/../../../etc/passwd HTTP/1.1\r\n{host}\r\n"),
        );
        assert_eq!(code, 404);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Read an event stream until `want` shows up (or 6 s pass); returns what was read.
    fn read_events(port: u16, act: impl FnOnce(), want: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
        s.write_all(
            format!("GET /__dfw/events HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes(),
        )
        .unwrap();
        s.set_read_timeout(Some(std::time::Duration::from_millis(200)))
            .unwrap();
        let mut buf = String::new();
        let mut chunk = [0u8; 4096];
        let mut act = Some(act);
        let end = std::time::Instant::now() + std::time::Duration::from_secs(6);
        while !buf.contains(want) && std::time::Instant::now() < end {
            if let Ok(n) = s.read(&mut chunk) {
                buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
            }
            if buf.contains("event: hello") {
                if let Some(a) = act.take() {
                    a();
                }
            }
        }
        buf
    }

    /// presence: the Studio passes the note on to its pages; studio live reload works by polling
    #[test]
    fn events_carry_presence_design_and_comment_changes() {
        let root = fixture();
        let (port, studio) = start(&root);
        let s = Arc::clone(&studio);
        std::thread::spawn(move || watch::run(s.root.clone(), &s.clients));
        std::thread::sleep(std::time::Duration::from_millis(300));
        let note = root.join(".dfw/presence.json");
        let at = crate::core::util::iso_from_millis(watch::now_ms());
        let got = read_events(
            port,
            || {
                std::fs::write(&note, format!(r#"{{"file":"screens/demo/a.html","dfwId":"demo.title","at":"{at}"}}"#)).unwrap()
            },
            "event: presence\ndata: {\"file\":\"screens/demo/a.html\",\"dfwId\":\"demo.title\"}\n\n",
        );
        assert!(got.contains("event: presence"), "{got}");
        let review = root.join(".dfw/reviews/rv_test.json");
        let got = read_events(
            port,
            || std::fs::write(&review, r#"{"comments":[]}"#).unwrap(),
            "event: comments\n",
        );
        assert!(got.contains("event: comments\ndata: {}"), "{got}");
        let screen = root.join("design/screens/items/a.html");
        let got = read_events(
            port,
            || {
                let mut f = std::fs::OpenOptions::new()
                    .append(true)
                    .open(&screen)
                    .unwrap();
                f.write_all(b"\n<!-- touched -->\n").unwrap();
            },
            "event: design\n",
        );
        assert!(
            got.contains(r#"data: {"files":["screens/items/a.html"]}"#),
            "{got}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// penpot studio routes: manifest/plugin over CORS (private network), writes only from the
    /// Studio origin
    #[test]
    fn penpot_routes_serve_the_plugin_over_cors() {
        let root = fixture();
        let (port, _studio) = start(&root);
        let host = format!("Host: 127.0.0.1:{port}\r\n");
        let raw = |method: &str, path: &str, extra: &str| {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(
                format!("{method} {path} HTTP/1.1\r\n{host}{extra}Content-Length: 2\r\n\r\n{{}}")
                    .as_bytes(),
            )
            .unwrap();
            let mut out = String::new();
            s.read_to_string(&mut out).unwrap();
            out
        };
        let m = raw("GET", "/__dfw/penpot/manifest.json", "");
        assert!(m.contains("Access-Control-Allow-Origin: *\r\n"));
        let manifest: Value = serde_json::from_str(m.split_once("\r\n\r\n").unwrap().1).unwrap();
        assert_eq!(
            (manifest["code"].clone(), manifest["version"].clone()),
            (json!("plugin.js"), json!(3))
        );
        let pre = raw("OPTIONS", "/__dfw/penpot/plugin.js", "");
        assert!(
            pre.starts_with("HTTP/1.1 204")
                && pre.contains("Access-Control-Allow-Private-Network: true")
        );
        let code = call(
            port,
            &format!("GET /__dfw/penpot/plugin.js HTTP/1.1\r\n{host}\r\n"),
        )
        .1;
        assert!(
            code.contains(&format!("'http://127.0.0.1:{port}'"))
                && !code.contains("__DFW_STUDIO_ORIGIN__")
        );
        assert!(call(
            port,
            &format!("GET /__dfw/penpot/ui.html HTTP/1.1\r\n{host}\r\n")
        )
        .1
        .contains("dfw bridge"));
        let foreign = raw(
            "POST",
            "/__dfw/penpot/changes",
            "Origin: https://design.penpot.app\r\n",
        );
        assert!(foreign.starts_with("HTTP/1.1 403"));
        let _ = std::fs::remove_dir_all(&root);
    }
}
