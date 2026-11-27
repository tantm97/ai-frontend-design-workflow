//! Concurrent writes through the Studio's routes (§0 row 38, `process-and-io.md` P7): one Studio
//! of the built binary gets many requests at once on every writing route a fixture can serve, and
//! no update is lost. The routes come from `src/studio/paths.rs`, so a new one has to be sorted
//! into the lists below.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[path = "support/product.rs"]
mod product;
use layout::rel;
use product::{fixture, layout, word};

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");
const LOOPBACK: &str = "127.0.0.1";
/// The methods of the requests (`studio::http::method`).
const GET: &str = "GET";
const POST: &str = "POST";
const PATCH: &str = "PATCH";

/// The text of the `i`th comment.
fn comment_text(i: usize) -> String {
    format!("c{i}")
}

/// The writing routes this test sends concurrent requests to.
const COVERED: [&str; 6] = [
    "COMMENTS",
    "COMMENT",
    "COMMENTS_SEND",
    "SESSIONS",
    "EDIT",
    "UNDO",
];

/// The writing routes it leaves out, each with the reason.
const LEFT_OUT: [(&str, &str); 2] = [
    (
        "SHOOT",
        "renders with Chrome and refuses a second shot while one runs (A screenshot is already running)",
    ),
    (
        "PENPOT",
        "the bridge writes need a rendered export and the panel's token; the threads ledger lock is its own change",
    ),
];

/// The routes that only read (or stream): the GET routes, pages and files.
const READ_ONLY: [&str; 18] = [
    "ROOT",
    "INDEX",
    "FAVICON",
    "DESIGN_BASE",
    "PREFIX",
    "SYSTEM_PAGE",
    "EVENTS",
    "OVERLAY_JS",
    "LIVE_JS",
    "STATIC",
    "SHOTS",
    "PING",
    "LIVE",
    "MANIFEST",
    "STATE",
    "TOKENS",
    "COMPONENTS",
    "LISTENING",
];

/// How many requests are in flight at once on each route.
const AT_ONCE: usize = 16;

/// `(name, value)` of every top-level `pub const NAME: &str = "value";` of
/// `src/studio/paths.rs` (the Penpot routes are one prefix here).
fn studio_paths() -> Vec<(String, String)> {
    let source =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/studio/paths.rs"))
            .unwrap();
    let top = source.split("pub mod penpot {").next().unwrap();
    top.lines()
        .filter_map(|l| l.strip_prefix("pub const "))
        .filter_map(|l| {
            let (name, rest) = l.split_once(": &str = \"")?;
            Some((name.to_string(), rest.strip_suffix("\";")?.to_string()))
        })
        .collect()
}

/// The value of the path named `name`.
fn path(name: &str) -> String {
    studio_paths()
        .into_iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .unwrap_or_else(|| panic!("no path {name} in src/studio/paths.rs"))
}

/// A fresh project folder (its real path).
fn project(label: &str) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("dfw-native-writes-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A Studio of the built binary, stopped when dropped.
struct Studio {
    child: Child,
    port: u16,
}

impl Drop for Studio {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Studio {
    fn start(root: &Path) -> Studio {
        let port = TcpListener::bind((LOOPBACK, 0))
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let home = root.join(".home");
        let mut cmd = home::command(DFW, root, &home, &home.join("dfw"));
        let root_arg = root.to_string_lossy().into_owned();
        cmd.args([
            word::command::STUDIO_SERVER,
            "--root",
            &root_arg,
            "--port",
            &port.to_string(),
        ])
        .env("DFW_TELEMETRY", "off")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
        let studio = Studio {
            child: cmd.spawn().unwrap(),
            port,
        };
        let ping = path("PING");
        let end = Instant::now() + Duration::from_secs(20);
        while studio.send(GET, &ping, "").0 != 200 {
            assert!(Instant::now() < end, "the Studio did not start");
            std::thread::sleep(Duration::from_millis(50));
        }
        studio
    }

    /// `method path` with a JSON `body`: the status (0 when there is no answer) and the body.
    fn send(&self, method: &str, path: &str, body: &str) -> (u16, String) {
        let Ok(mut s) = TcpStream::connect((LOOPBACK, self.port)) else {
            return (0, String::new());
        };
        s.set_read_timeout(Some(Duration::from_secs(30))).unwrap();
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {LOOPBACK}:{}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.port,
            body.len()
        );
        if s.write_all(head.as_bytes())
            .and_then(|()| s.write_all(body.as_bytes()))
            .is_err()
        {
            return (0, String::new());
        }
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        let status = out.get(9..12).and_then(|c| c.parse().ok()).unwrap_or(0);
        let body = out
            .split_once("\r\n\r\n")
            .map_or(String::new(), |(_, b)| b.to_string());
        (status, body)
    }

    /// `AT_ONCE` requests at once, request `i` made by `each(i)`: every answer as JSON, each
    /// with the status `want`.
    fn at_once(
        &self,
        method: &str,
        want: u16,
        each: impl Fn(usize) -> (String, String) + Sync,
    ) -> Vec<Value> {
        std::thread::scope(|s| {
            let sent: Vec<_> = (0..AT_ONCE)
                .map(|i| {
                    let each = &each;
                    s.spawn(move || {
                        let (path, body) = each(i);
                        self.send(method, &path, &body)
                    })
                })
                .collect();
            sent.into_iter()
                .map(|h| {
                    let (status, body) = h.join().unwrap();
                    assert_eq!(status, want, "{method}: {body}");
                    serde_json::from_str(&body).unwrap()
                })
                .collect()
        })
    }

    fn comments(&self) -> Vec<Value> {
        let (status, body) = self.send(GET, &path("COMMENTS"), "");
        assert_eq!(status, 200, "{body}");
        serde_json::from_str::<Value>(&body)
            .unwrap()
            .as_array()
            .unwrap()
            .clone()
    }
}

/// Every route of `src/studio/paths.rs` is sorted: covered here, left out with a reason, or
/// read-only. A new writing route fails until it is one of them.
#[test]
fn every_studio_route_is_sorted() {
    let mut sorted: Vec<&str> = COVERED.to_vec();
    sorted.extend(LEFT_OUT.iter().map(|(n, _)| *n));
    sorted.extend(READ_ONLY);
    let unsorted: Vec<String> = studio_paths()
        .into_iter()
        .map(|(n, _)| n)
        .filter(|n| !sorted.contains(&n.as_str()))
        .collect();
    assert!(
        unsorted.is_empty(),
        "routes of src/studio/paths.rs this test does not know; add each to COVERED (and a concurrent case), LEFT_OUT with the reason, or READ_ONLY: {unsorted:?}"
    );
    assert_eq!(sorted.len(), studio_paths().len(), "a name listed twice");
}

/// Comments added while drafts are sent and sessions start, then many messages on one comment:
/// every comment and every message is kept.
#[test]
fn concurrent_comment_writes_keep_every_update() {
    let root = project("comments");
    let studio = Studio::start(&root);
    let (comments, send, sessions) = (path("COMMENTS"), path("COMMENTS_SEND"), path("SESSIONS"));
    std::thread::scope(|s| {
        s.spawn(|| studio.at_once(POST, 200, |_| (send.clone(), String::new())));
        s.spawn(|| studio.at_once(POST, 201, |_| (sessions.clone(), String::new())));
        studio.at_once(POST, 201, |i| {
            (
                comments.clone(),
                json!({ "comment": comment_text(i) }).to_string(),
            )
        });
    });
    let all = studio.comments();
    let ids: std::collections::HashSet<&str> =
        all.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), AT_ONCE, "comments lost: {all:?}");
    for i in 0..AT_ONCE {
        let text = comment_text(i);
        assert!(all.iter().any(|c| c["comment"] == text), "{text} lost");
    }

    // Many messages on one comment at once: the thread keeps them all.
    let id = all[0]["id"].as_str().unwrap().to_string();
    let one = format!("{}{id}", path("COMMENT"));
    studio.at_once(PATCH, 200, |i| {
        (
            one.clone(),
            json!({ "message": format!("m{i}") }).to_string(),
        )
    });
    let after = studio.comments();
    let comment = after.iter().find(|c| c["id"] == id.as_str()).unwrap();
    let kept = comment.to_string();
    for i in 0..AT_ONCE {
        assert!(
            kept.contains(&format!("\"m{i}\"")),
            "message m{i} lost: {kept}"
        );
    }
    drop(studio);
    let _ = fs::remove_dir_all(&root);
}

/// Edits of one screen at once are all kept, each journaled; as many undos at once take them
/// all back, each once, to the original bytes.
#[test]
fn concurrent_edits_and_undos_keep_every_update() {
    let root = project("edits");
    let paras: String = (0..AT_ONCE)
        .map(|i| format!("<p data-dfw-id=\"p.{i}\">old {i}</p>\n"))
        .collect();
    let source = format!("<!doctype html><html><body>\n{paras}</body></html>\n");
    let screen = root.join(rel::screen(fixture::MANY_EDITS_PAGE));
    fs::create_dir_all(screen.parent().unwrap()).unwrap();
    fs::write(&screen, &source).unwrap();
    let studio = Studio::start(&root);
    let (edit, undo) = (path("EDIT"), path("UNDO"));
    // The screen as the Studio names it: relative to `design/`.
    let file = format!("{}/{}", layout::file::SCREENS, fixture::MANY_EDITS_PAGE);
    for done in studio.at_once(POST, 200, |i| {
        let op = json!({ "file": file, "op": "text", "id": format!("p.{i}"), "value": format!("new {i}") });
        (edit.clone(), op.to_string())
    }) {
        assert_eq!(done["changed"], true, "{done}");
    }
    let text = fs::read_to_string(&screen).unwrap();
    for i in 0..AT_ONCE {
        assert!(text.contains(&format!(">new {i}<")), "edit {i} lost");
    }
    let mut remaining: Vec<u64> = studio
        .at_once(POST, 200, |_| (undo.clone(), String::new()))
        .iter()
        .map(|u| u["remaining"].as_u64().unwrap())
        .collect();
    remaining.sort_unstable();
    assert_eq!(
        remaining,
        (0..AT_ONCE as u64).collect::<Vec<_>>(),
        "each undo once"
    );
    assert_eq!(fs::read_to_string(&screen).unwrap(), source);
    drop(studio);
    let _ = fs::remove_dir_all(&root);
}
