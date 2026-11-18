//! Ports the Penpot routes of `core/studio/server.mjs` (`/__dfw/penpot/*`). Penpot (in the user's
//! browser) fetches the plugin manifest and code cross-origin; the plugin UI is served from here,
//! so its data calls are same-origin. Writes accept only that origin (the Studio's `foreign`
//! check runs before these routes).

use serde_json::{json, Value};

use super::api::mime;
use super::http::query_param;
use super::server::{Request, Response, Studio, JSON};
use crate::assets::{studio_file, studio_text};
use crate::core::error::DfwError;
use crate::core::paths::project_paths;
use crate::core::util::to_json_pretty;
use crate::penpot::export::{export_penpot, ExportOptions};
use crate::penpot::import::{format_import_changes, import_changes, ImportOptions};
use crate::penpot::live::sync_threads;
use crate::workflow::state::load_state;

/// `PENPOT_CORS`: Penpot runs on another origin, and on a public one (private network access).
const CORS: [(&str, &str); 4] = [
    ("Access-Control-Allow-Origin", "*"),
    ("Access-Control-Allow-Methods", "GET"),
    ("Access-Control-Allow-Headers", "content-type"),
    ("Access-Control-Allow-Private-Network", "true"),
];

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
    let rel = req.path.strip_prefix("/__dfw/penpot/").unwrap_or_default();
    if method == "OPTIONS" {
        return Ok(cors(204, "", "text/plain"));
    }
    let json_type = mime(".json").unwrap_or(JSON);
    Ok(match rel {
        "manifest.json" => cors(200, to_json_pretty(&manifest()), json_type),
        "plugin.js" => match studio_text("penpot/plugin.js") {
            Some(code) => {
                let origin = format!("http://127.0.0.1:{}", studio.port);
                cors(
                    200,
                    code.replace("__DFW_STUDIO_ORIGIN__", &origin),
                    mime(".js").unwrap_or(JSON),
                )
            }
            None => Response::not_found(),
        },
        "icon.svg" => match studio_file("penpot/icon.svg") {
            Some(svg) => cors(200, svg.into_owned(), mime(".svg").unwrap_or(JSON)),
            None => Response::not_found(),
        },
        "ui.html" => match studio_file("penpot/ui.html") {
            Some(html) => Response::send(200, html.into_owned(), mime(".html").unwrap_or(JSON)),
            None => Response::not_found(),
        },
        "export.json" if method == "GET" => {
            let file = project_paths(&studio.root)
                .dfw
                .join("penpot")
                .join("export.json");
            let refresh = query_param(req.head.target(), "refresh").as_deref() == Some("1");
            if refresh || !file.exists() {
                // One export at a time (Node shared the running export's promise).
                let _one = studio.penpot_exporting.lock();
                let opts = ExportOptions {
                    page: query_param(req.head.target(), "page").filter(|p| !p.is_empty()),
                    bp: query_param(req.head.target(), "bp").filter(|b| !b.is_empty()),
                    out: None,
                };
                Response::json(200, &export_penpot(&studio.root, &opts)?.1)
            } else {
                Response::send(200, std::fs::read(&file)?, json_type)
            }
        }
        "changes" if method == "POST" => {
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
                200,
                &json!({ "summary": format_import_changes(&r, false), "comments": ids, "changes": r.changes.len() }),
            )
        }
        // Live comments: the plugin panel sends the file's threads every few seconds and posts
        // dfw's replies back; a sync that adds or updates comments broadcasts `comments`.
        "sync" if method == "POST" => {
            let state = load_state(&studio.root)?;
            let r = sync_threads(&studio.root, &req.json()?, state.as_ref())?;
            let count = |k: &str| r.get(k).and_then(Value::as_array).map_or(0, Vec::len);
            if count("comments") > 0 || count("updated") > 0 {
                studio.clients.broadcast("comments", &json!({}));
            }
            Response::json(200, &r)
        }
        _ => Response::not_found(),
    })
}
