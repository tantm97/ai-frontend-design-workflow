//! Ports the Penpot routes of `core/studio/server.mjs` (`/__dfw/penpot/*`). Penpot (in the user's
//! browser) fetches the plugin manifest and code cross-origin; the plugin UI is served from here,
//! so its data calls are same-origin. Writes accept only that origin (the Studio's `foreign`
//! check runs before these routes).
//!
//! The panel (`ui.html`) relays what Penpot's plugin sends into Studio writes, so a page that
//! framed it could make it write comments the agent reads as the user's words. Three locks:
//! the panel may be framed only by the Penpot origins of the user's global config
//! (`penpot.origins`, default the Penpot cloud; `frame-ancestors`), it talks only to those
//! origins (`postMessage`), and every write it makes carries the token it read from the Studio
//! (`session.json`, same origin only), which a page elsewhere cannot read.

use serde_json::{json, Value};

use super::api::mime;
use super::http::{method, query_param, status};
use super::server::{Request, Response, Studio, JSON};
use crate::assets::{studio_file, studio_text};
use crate::core::config::global_config_path;
use crate::core::error::DfwError;
use crate::core::paths::{file, project_paths};
use crate::core::util::{get_path, read_json, to_json_pretty};
use crate::penpot::export::{export_penpot, ExportOptions};
use crate::penpot::import::{format_import_changes, import_changes, ImportOptions};
use crate::penpot::live::sync_threads;
use crate::studio::paths;
use crate::workflow::state::load_state;

/// `PENPOT_CORS`: Penpot runs on another origin, and on a public one (private network access).
const CORS: [(&str, &str); 4] = [
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Methods", method::GET),
    ("Access-Control-Allow-Headers", "content-type"),
    ("Access-Control-Allow-Private-Network", "true"),
];

/// The Penpot the panel works with when the user's config names none: the Penpot cloud.
pub const DEFAULT_PENPOT_ORIGIN: &str = "https://design.penpot.app";

/// The answer to a GET of the export before one exists.
const NO_EXPORT: &str =
    "No Penpot export yet: run \"dfw penpot export\", or tick \"Render now\" in the dfw bridge panel.";

/// The header that carries the panel's token.
pub const TOKEN_HEADER: &str = "x-dfw-bridge";

/// `penpot.origins` of the user's global config (`~/.config/dfw/config.json`; a project cannot
/// choose who may drive its Studio): a list of origins such as `https://penpot.example.com`
/// or `http://localhost:9001`, or one origin as text. Unset: the Penpot cloud. An entry that
/// is not an http(s) origin is left out with one line on `warn`.
pub fn allowed_origins(warn: &mut dyn FnMut(String)) -> Vec<String> {
    let global = read_json(&global_config_path(), Some(json!({}))).unwrap_or_else(|err| {
        warn(format!("dfw studio: {}", err.message()));
        json!({})
    });
    origins_from(get_path(&global, "penpot.origins"), warn)
}

/// [`allowed_origins`] of a config value.
pub fn origins_from(value: Option<&Value>, warn: &mut dyn FnMut(String)) -> Vec<String> {
    let items: Vec<&Value> = match value {
        None | Some(Value::Null) => return vec![DEFAULT_PENPOT_ORIGIN.to_string()],
        Some(Value::Array(list)) => list.iter().collect(),
        Some(one) => vec![one],
    };
    let mut out: Vec<String> = Vec::new();
    for item in items {
        match item.as_str().and_then(normalize_origin) {
            Some(o) if !out.contains(&o) => out.push(o),
            Some(_) => {}
            None => warn(format!(
                "dfw studio: penpot.origins: {item} is not an origin like \"https://penpot.example.com\"; left out. Fix it with: dfw config set penpot.origins '[\"https://penpot.example.com\"]' --global"
            )),
        }
    }
    out
}

/// `https://Host:8080/` -> `https://host:8080`; `None` for anything that is not an http(s)
/// origin (a path, a wildcard, a quote or space that would break the CSP header).
fn normalize_origin(raw: &str) -> Option<String> {
    let low = raw.trim().trim_end_matches('/').to_ascii_lowercase();
    let rest = low
        .strip_prefix("https://")
        .or_else(|| low.strip_prefix("http://"))?;
    let ok = !rest.is_empty()
        && rest
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b':' | b'[' | b']'));
    ok.then_some(low)
}

/// `Content-Security-Policy: frame-ancestors <origins>` for the panel: only Penpot may frame it.
pub fn frame_ancestors(origins: &[String]) -> (String, String) {
    let list = if origins.is_empty() {
        "'none'".to_string()
    } else {
        origins.join(" ")
    };
    (
        "Content-Security-Policy".into(),
        format!("frame-ancestors {list}"),
    )
}

/// Why a panel request is refused: a browser that says it comes from another site, or a write
/// without the Studio's token.
fn bridge_refusal(studio: &Studio, req: &Request, needs_token: bool) -> Option<Response> {
    let refuse = |msg: &str| Some(Response::error(status::FORBIDDEN, msg));
    if req
        .head
        .header("sec-fetch-site")
        .is_some_and(|s| !s.eq_ignore_ascii_case("same-origin"))
    {
        return refuse("Only the dfw bridge panel served by this Studio may call this route.");
    }
    if needs_token && req.head.header(TOKEN_HEADER) != Some(studio.penpot_token.as_str()) {
        return refuse("This request needs the dfw bridge token: close the dfw bridge plugin in Penpot and open it again (it reads the token from the Studio).");
    }
    None
}

fn cors(code: u16, body: impl Into<Vec<u8>>, content_type: &str) -> Response {
    let mut r = Response::send(code, body, content_type);
    r.headers
        .extend(CORS.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    r
}

/// The plugin manifest Penpot installs.
fn manifest() -> Value {
    json!({
        "name": "dfw bridge",
        "description": "Build boards from a local dfw design and send designer changes back as dfw comments (local Studio on 127.0.0.1).",
        "version": 3,
        "code": "plugin.js",
        "icon": "icon.svg",
        "permissions": ["content:read", "content:write", "library:read", "library:write", "comment:write", "allow:downloads"],
    })
}

/// `penpotRoute(req, res, p, url)`.
pub fn route(studio: &Studio, req: &Request) -> Result<Response, DfwError> {
    let method = req.head.method();
    let rel = req.path.strip_prefix(paths::PENPOT).unwrap_or_default();
    if method == method::OPTIONS {
        return Ok(cors(status::NO_CONTENT, "", "text/plain"));
    }
    let json_type = mime(".json").unwrap_or(JSON);
    Ok(match rel {
        "manifest.json" => cors(status::OK, to_json_pretty(&manifest()), json_type),
        "plugin.js" => match studio_text("penpot/plugin.js") {
            Some(code) => {
                let origin = format!("http://127.0.0.1:{}", studio.port);
                cors(
                    status::OK,
                    code.replace("__DFW_STUDIO_ORIGIN__", &origin),
                    mime(".js").unwrap_or(JSON),
                )
            }
            None => Response::not_found(),
        },
        "icon.svg" => match studio_file("penpot/icon.svg") {
            Some(svg) => cors(status::OK, svg.into_owned(), mime(".svg").unwrap_or(JSON)),
            None => Response::not_found(),
        },
        "ui.html" => match studio_file("penpot/ui.html") {
            Some(html) => {
                let mut r =
                    Response::send(status::OK, html.into_owned(), mime(".html").unwrap_or(JSON));
                r.headers.push(frame_ancestors(&studio.penpot_origins));
                r
            }
            None => Response::not_found(),
        },
        // What the panel needs before it talks to Penpot or writes: the token and the origins.
        "session.json" if method == method::GET => {
            if let Some(refused) = bridge_refusal(studio, req, false) {
                return Ok(refused);
            }
            Response::json(
                status::OK,
                &json!({ "token": studio.penpot_token, "origins": studio.penpot_origins }),
            )
        }
        // A GET only reads the last export: rendering (files written, a browser started) is a
        // POST, so a page elsewhere cannot start it with an <img>.
        "export.json" if method == method::GET => {
            let file = project_paths(&studio.root).penpot.join(file::PENPOT_EXPORT);
            if !file.exists() {
                return Ok(Response::error(status::NOT_FOUND, NO_EXPORT));
            }
            Response::send(status::OK, std::fs::read(&file)?, json_type)
        }
        "export.json" if method == method::POST => {
            if let Some(refused) = bridge_refusal(studio, req, true) {
                return Ok(refused);
            }
            // One export at a time (Node shared the running export's promise).
            let _one = studio.penpot_exporting.lock();
            let opts = ExportOptions {
                page: query_param(req.head.target(), "page").filter(|p| !p.is_empty()),
                bp: query_param(req.head.target(), "bp").filter(|b| !b.is_empty()),
                out: None,
            };
            Response::json(status::OK, &export_penpot(&studio.root, &opts)?.1)
        }
        "changes" if method == method::POST => {
            if let Some(refused) = bridge_refusal(studio, req, true) {
                return Ok(refused);
            }
            let state = load_state(&studio.root)?;
            let mut changes = req.json()?;
            let opts = ImportOptions {
                dry_run: false,
                state: state.as_ref(),
                live: false,
            };
            let r = import_changes(&studio.root, &mut changes, opts)?;
            let ids: Vec<Value> = r
                .comments
                .iter()
                .map(|c| c.get("id").cloned().unwrap_or(Value::Null))
                .collect();
            Response::json(
                status::OK,
                &json!({ "summary": format_import_changes(&r, false), "comments": ids, "changes": r.changes.len() }),
            )
        }
        // Live comments: the plugin panel sends the file's threads every few seconds and posts
        // dfw's replies back; a sync that adds or updates comments broadcasts `comments`.
        "sync" if method == method::POST => {
            if let Some(refused) = bridge_refusal(studio, req, true) {
                return Ok(refused);
            }
            let state = load_state(&studio.root)?;
            let r = sync_threads(&studio.root, &req.json()?, state.as_ref())?;
            let count = |k: &str| r.get(k).and_then(Value::as_array).map_or(0, Vec::len);
            if count("comments") > 0 || count("updated") > 0 {
                studio.clients.broadcast("comments", &json!({}));
            }
            Response::json(status::OK, &r)
        }
        _ => Response::not_found(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// penpot.origins: a list or one origin, normalised; anything else is left out with a line
    #[test]
    fn penpot_origins_are_origins_only() {
        let mut lines = Vec::new();
        let mut warn = |m: String| lines.push(m);
        assert_eq!(origins_from(None, &mut warn), [DEFAULT_PENPOT_ORIGIN]);
        assert_eq!(
            origins_from(Some(&json!("https://Penpot.Example.com/")), &mut warn),
            ["https://penpot.example.com"]
        );
        let list = json!([
            "http://localhost:9001",
            "*",
            "https://a.example/path",
            "https://x y",
            7,
            "javascript:alert(1)",
            "http://localhost:9001"
        ]);
        assert_eq!(
            origins_from(Some(&list), &mut warn),
            ["http://localhost:9001"]
        );
        assert_eq!(lines.len(), 5, "{lines:?}");
        assert!(lines[0].contains("dfw config set penpot.origins"));
        assert_eq!(
            origins_from(Some(&json!([])), &mut |_| {}),
            Vec::<String>::new()
        );
        assert_eq!(frame_ancestors(&[]).1, "frame-ancestors 'none'");
        assert_eq!(
            frame_ancestors(&["https://a.example".into(), "http://localhost:9001".into()]).1,
            "frame-ancestors https://a.example http://localhost:9001"
        );
    }
}
