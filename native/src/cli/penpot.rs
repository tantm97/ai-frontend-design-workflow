//! The `penpot` command of `core/lib/cli.mjs` (`cmdPenpot`), and the two Penpot branches of the
//! `tokens` command (`tokens export`, `tokens import`), which `cli/design.rs` hands over.

use std::path::Path;

use serde_json::{json, Map, Value};

use super::word::{command, sub};
use super::{Ctx, Handler};
use crate::core::config::load_config;
use crate::core::error::exit;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::process::{is_alive, Pid};
use crate::core::util::{get_path, out, read_json, read_stdin, to_json_compact, to_json_pretty};
use crate::design::compose::build_system;
use crate::design::tokens::build_tokens;
use crate::penpot::export::{
    build_export, export_penpot, export_summary, ExportInput, ExportOptions,
};
use crate::penpot::import::{format_import_changes, import_changes, ImportOptions};
use crate::penpot::live::sync_threads;
use crate::penpot::tokens::{
    export_penpot_tokens, format_export, format_import, import_penpot_tokens, DfwTokens,
};
use crate::workflow::state::load_state;

/// The commands of this group.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![(command::PENPOT, run as Handler)]
}

/// A value flag as the Node CLI read it in `a.flag || null`: its text, nothing when it is
/// missing or empty. Given alone it never gets here (`require_flag_values`).
fn flag_text(ctx: &Ctx, key: &str) -> Option<String> {
    ctx.args
        .text(key)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn run(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    match ctx.args.pos(1) {
        Some(sub::EXPORT) => export(ctx, &root),
        Some(sub::IMPORT) => import(ctx, &root),
        Some(sub::PLUGIN) => plugin(&root),
        _ => Err(DfwError::new(
            "Usage: dfw penpot export [--page <id>] [--bp <ids>] | plugin | import <changes.json> [--dry-run]",
        )),
    }
}

/// `dfw penpot export`: render the screens and write `.dfw/penpot/export.json`. Exit code 2
/// when a render failed.
fn export(ctx: &Ctx, root: &Path) -> Result<i32, DfwError> {
    let opts = ExportOptions {
        page: flag_text(ctx, "page"),
        bp: flag_text(ctx, "bp"),
        out: flag_text(ctx, "out"),
    };
    let (file, doc) = export_penpot(root, &opts)?;
    let errors = doc.get("errors").cloned().unwrap_or(Value::Null);
    if ctx.args.truthy("json") {
        out(&to_json_pretty(&json!({
            "file": file.display().to_string(),
            "id": doc.get("id"),
            "errors": errors,
        })));
    } else {
        out(&export_summary(&doc, &file, root));
    }
    let failed = errors.as_array().is_some_and(|e| !e.is_empty());
    Ok(if failed { exit::PROBLEMS } else { exit::OK })
}

/// `dfw penpot import <changes.json>`: the designer's changes as feedback comments.
fn import(ctx: &Ctx, root: &Path) -> Result<i32, DfwError> {
    let Some(file) = ctx.args.pos(2).filter(|f| !f.is_empty()) else {
        return Err(DfwError::new(
            "Usage: dfw penpot import <changes.json> [--dry-run] [--json]",
        ));
    };
    let mut changes = js::read_json(&resolve(file), Some(Value::Null))?;
    if !js::truthy(Some(&changes)) {
        return Err(DfwError::new(format!("{file}: not found or not JSON.")));
    }
    let dry_run = ctx.args.truthy("dry-run");
    let state = load_state(root)?;
    let r = import_changes(
        root,
        &mut changes,
        ImportOptions {
            dry_run,
            state: state.as_ref(),
            live: false,
        },
    )?;
    if ctx.args.truthy("json") {
        out(&to_json_pretty(&r.to_json()));
    } else {
        out(&format_import_changes(&r, dry_run));
    }
    Ok(0)
}

/// `dfw penpot plugin`: the manifest URL to install in Penpot, and what the plugin does.
fn plugin(root: &Path) -> Result<i32, DfwError> {
    let info = read_json(&project_paths(root).studio, Some(Value::Null))?;
    let url = info
        .get("url")
        .filter(|u| js::truthy(Some(u)) && Pid::of(info.get("pid")).is_some_and(is_alive))
        .map(|u| js::to_string(Some(u)));
    let base = match &url {
        Some(url) => url.clone(),
        None => {
            let config = load_config(Some(root))?;
            let port = get_path(&config, "studio.port")
                .filter(|p| js::truthy(Some(p)))
                .map_or("4600".to_string(), |p| js::to_string(Some(p)));
            format!("http://127.0.0.1:{port}/")
        }
    };
    let hint = if url.is_some() {
        ""
    } else {
        "The Studio is not running: start it with \"dfw studio start\".\n"
    };
    // penpot.origins of the global config: the Penpot pages the panel may run in.
    let origins = crate::studio::penpot_routes::allowed_origins(&mut |m| eprintln!("{m}"));
    let origins = if origins.is_empty() {
        "no Penpot (penpot.origins is empty)".to_string()
    } else {
        origins.join(", ")
    };
    out(&format!(
        "{hint}Penpot plugin manifest: {base}__dfw/penpot/manifest.json
In Penpot: open a file, Plugins (Ctrl+Alt+P), paste the URL, Install, then open \"dfw bridge\".
  Build boards: pulls the latest \"dfw penpot export\" from the Studio and creates pages, boards, components.
  Live comments: while the panel is open, threads on dfw boards reach the agent like Studio comments; its replies come back into the thread.
  Update boards: redraws the boards in place after the design changed (comment threads stay).
  Send changes: sends layer edits and token changes back to the Studio, which records them as dfw comments (provider \"penpot\").
No Penpot API token is used: the plugin runs in your browser and only talks to 127.0.0.1.
The panel works only inside {origins}. Penpot on your own server: dfw config set penpot.origins '[\"https://penpot.example.com\"]' --global, then restart the Studio."
    ));
    Ok(0)
}

/// `dfw tokens export --format penpot [--out <file>] [--opt-themes] [--json]`.
pub(super) fn tokens_export(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    if let Some(format) = flag_text(ctx, "format").filter(|f| f != "penpot") {
        return Err(DfwError::new(format!(
            "Unknown --format \"{format}\" (supported: penpot)."
        )));
    }
    let out_file = flag_text(ctx, "out");
    let r = export_penpot_tokens(&root, out_file.as_deref(), ctx.args.truthy("opt-themes"))?;
    if ctx.args.truthy("json") {
        out(&to_json_pretty(&json!({
            "file": r.file.display().to_string(),
            "report": r.report.to_json(),
        })));
    } else {
        out(&format_export(&r, &root));
    }
    Ok(0)
}

/// `dfw tokens import <file|folder> [--from penpot] [--dry-run] [--prune] [--json]`.
pub(super) fn tokens_import(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let Some(file) = ctx.args.pos(2).filter(|f| !f.is_empty()) else {
        return Err(DfwError::new(
            "Usage: dfw tokens import <penpot-tokens.json|folder> [--from penpot] [--dry-run] [--prune]",
        ));
    };
    if let Some(from) = flag_text(ctx, "from").filter(|f| f != "penpot") {
        return Err(DfwError::new(format!(
            "Unknown --from \"{from}\" (supported: penpot)."
        )));
    }
    let dry_run = ctx.args.truthy("dry-run");
    let state = load_state(&root)?;
    let r = import_penpot_tokens(
        &root,
        Path::new(file),
        state.as_ref(),
        dry_run,
        ctx.args.truthy("prune"),
    )?;
    if !r.written.is_empty() {
        build_tokens(&root)?;
        build_system(&root)?;
    }
    if ctx.args.truthy("json") {
        out(&to_json_pretty(&r.to_json()));
    } else {
        out(&format_import(&r, dry_run));
    }
    Ok(0)
}

/// `__debug penpot-build-export <renders.json> [tokens.json] [generatedAt]`:
/// `JSON.stringify(buildExport({ renders, tokens: { base, themes: {} }, product: 'Fixture',
/// generatedAt }))`, without a browser.
pub(super) fn debug_build_export(ctx: &Ctx) -> Result<i32, DfwError> {
    let renders = js::read_json(Path::new(ctx.args.pos(2).unwrap_or_default()), None)?;
    let tokens = match ctx.args.pos(3).filter(|f| !f.is_empty() && *f != "-") {
        Some(file) => Some(DfwTokens {
            base: js::read_json(Path::new(file), None)?,
            themes: Map::new(),
            skipped: Vec::new(),
        }),
        None => None,
    };
    let doc = build_export(&ExportInput {
        renders: renders.as_array().map_or(&[], Vec::as_slice),
        tokens: tokens.as_ref(),
        product: "Fixture".into(),
        design_version: "v1".to_string(),
        project_root: None,
        generated_at: ctx
            .args
            .pos(4)
            .unwrap_or("2026-11-16T10:00:00.000Z")
            .to_string(),
    });
    out(&to_json_compact(&doc));
    Ok(0)
}

/// `__debug penpot-sync <root>`: a live payload as JSON on stdin,
/// `JSON.stringify(syncThreads(root, payload, { state: loadState(root) }))` on stdout.
pub(super) fn debug_sync(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = resolve(ctx.args.pos(2).unwrap_or_default());
    let payload: Value = serde_json::from_str(&read_stdin()?)?;
    let state = load_state(&root)?;
    out(&to_json_compact(&sync_threads(
        &root,
        &js::key_order(payload),
        state.as_ref(),
    )?));
    Ok(0)
}
