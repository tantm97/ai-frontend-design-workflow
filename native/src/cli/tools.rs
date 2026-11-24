//! Tooling commands of `core/lib/cli.mjs`: inspector, mcp, install, uninstall, setup, doctor, stack, handoff
//! (`cmdInspector`, `cmdDoctor`, `cmdHandoff`). Lighthouse has no command of its own: it runs inside `check`.

use serde_json::{json, Value};

use super::word::{self, command};
use super::{Ctx, Handler};
use crate::browser::download::install_browser;
use crate::core::config::{set_config_value, Scope};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, rel, resolve};
use crate::core::util::{out, read_json, to_json_compact, to_json_pretty, ArgValue, Args};
use crate::tools::binary::{install_binary, RELEASE_REPO};
use crate::tools::doctor::doctor_checks;
use crate::tools::inspector::{format_plan, inspector_setup, parse_insp_path, PACKAGE, VERSION};
use crate::tools::install::{install, uninstall, InstallOptions};

/// The commands of this group. `mcp`, `stack` and `handoff` are at the end of the file.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        (command::INSPECTOR, inspector as Handler),
        (command::MCP, mcp as Handler),
        (command::INSTALL, cmd_install as Handler),
        (command::UNINSTALL, cmd_uninstall as Handler),
        (command::SETUP, setup as Handler),
        (command::DOCTOR, doctor as Handler),
        (command::STACK, stack as Handler),
        (command::HANDOFF, handoff as Handler),
    ]
}

/// `a.<key> || fallback` for a value flag (alone it never gets here: `require_flag_values`).
fn flag_or(a: &Args, key: &str, fallback: &str) -> String {
    a.text(key)
        .filter(|t| !t.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// `dfw inspector setup [--write] [--json]` (`cmdInspector`).
fn inspector(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    if a.pos(1).filter(|s| !s.is_empty()).unwrap_or("setup") != "setup" {
        return Err(DfwError::new(
            "Usage: dfw inspector setup [--write] [--json]",
        ));
    }
    let write = a.truthy("write");
    let (mut plan, written) = inspector_setup(&ctx.root(), write)?;
    if a.truthy("json") {
        if let Some(map) = plan.as_object_mut() {
            map.insert("written".into(), json!(written));
        }
        out(&to_json_pretty(&plan));
    } else if !write {
        out(&format_plan(&plan));
    } else if written.is_empty() {
        out(&format!("{PACKAGE}@{VERSION} is already set up."));
    } else {
        let pm = plan
            .get("packageManager")
            .and_then(Value::as_str)
            .filter(|p| !p.is_empty())
            .unwrap_or("npm");
        out(&format!(
            "Wrote {} ({PACKAGE}@{VERSION}, dev-only). Now run: {pm} install, restart the dev server, then \"dfw review start --url <dev server>\".",
            written.join(", ")
        ));
    }
    Ok(0)
}

/// `dfw install [--host ...] [--scope ...] [--source ...] [--no-runtime] [--quiet]`.
/// `--no-binary` (an option of the 0.5 installers) is still accepted and changes nothing.
fn cmd_install(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    install(&InstallOptions {
        host: flag_or(a, "host", "auto"),
        scope: flag_or(a, "scope", "user"),
        source: flag_or(a, "source", RELEASE_REPO),
        runtime: !a.truthy("no-runtime"),
        quiet: a.truthy("quiet"),
    })?;
    Ok(0)
}

/// `dfw uninstall [--host ...] [--scope ...] [--quiet]`.
fn cmd_uninstall(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    uninstall(
        &flag_or(a, "host", "auto"),
        &flag_or(a, "scope", "user"),
        a.truthy("quiet"),
    )?;
    Ok(0)
}

/// `dfw setup --browser | --a11y | --binary | --marketplace-repo <path>`.
fn setup(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let log = |m: &str| eprintln!("{m}");
    if a.truthy("browser") || a.truthy("playwright") {
        out(&to_json_pretty(&install_browser(&log)?));
    } else if a.truthy("a11y") {
        out(&to_json_pretty(
            &crate::browser::a11y::install_axe(&log)?.to_json(),
        ));
    } else if a.truthy("binary") {
        out(&to_json_pretty(&install_binary(&log)?));
    } else if let Some(repo) = a.text("marketplace-repo").filter(|r| !r.is_empty()) {
        let repo = resolve(repo).display().to_string();
        let file = set_config_value(Scope::Global, &ctx.root(), "marketplaceRepo", json!(repo))?;
        out(&format!("Recorded marketplace repo in {}", file.display()));
    } else {
        return Err(DfwError::new(
            "Usage: dfw setup --browser | --a11y | --binary | --marketplace-repo <path>",
        ));
    }
    Ok(0)
}

/// `__debug insp-path <value> [root]`: `JSON.stringify(parseInspPath(value, root))`.
pub(super) fn debug_insp_path(ctx: &Ctx) -> Result<i32, DfwError> {
    let parsed = parse_insp_path(ctx.args.pos(2).unwrap_or_default(), ctx.args.pos(3));
    out(&to_json_compact(&parsed.map_or(Value::Null, |p| {
        json!({ "file": p.file, "line": p.line, "column": p.column, "tag": p.tag, "source": p.source })
    })));
    Ok(0)
}

/* ---------------------------------------------------------------- doctor */

/// `dfw doctor`: environment and installation checks (`cmdDoctor`), one line each.
fn doctor(ctx: &Ctx) -> Result<i32, DfwError> {
    let mut lines: Vec<String> = doctor_checks(&ctx.root())?
        .iter()
        .map(|c| doctor_line(&c.name, c.ok, &c.detail))
        .collect();
    lines.push(doctor_line(
        "telemetry",
        true,
        &crate::telemetry::status_line(),
    ));
    out(&lines.join("\n"));
    Ok(0)
}

/// `ok  <name> - <detail>` or `WARN <name> - <detail>`; no dash without a detail.
fn doctor_line(name: &str, good: bool, detail: &str) -> String {
    let detail = if detail.is_empty() {
        String::new()
    } else {
        format!(" - {detail}")
    };
    format!("{} {name}{detail}", if good { "ok  " } else { "WARN" })
}

/* ---------------------------------------------------------------- mcp, stack, handoff */

use crate::core::util::{now_iso, write_json_inside};
use crate::tools::handoff::write_handoff;
use crate::tools::mcp::serve_mcp;
use crate::tools::mcp_install::{mcp_install, which_tool};
use crate::workflow::stack::detect_stack;
use crate::workflow::state::{load_state, push_history, save_state, state_field_error};

/// `dfw mcp`: the server on stdio; `dfw mcp install|uninstall <tool|all> [--print]`.
fn mcp(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let Some(sub) = a.pos(1).filter(|s| !s.is_empty()) else {
        serve_mcp()?;
        return Ok(0);
    };
    if sub != word::sub::INSTALL && sub != word::sub::UNINSTALL {
        return Err(DfwError::new(
            "Usage: dfw mcp | dfw mcp install|uninstall <opencode|codex|cursor|all> [--print]",
        ));
    }
    let Some(tool) = a.pos(2).filter(|t| !t.is_empty()) else {
        return Err(DfwError::new(which_tool()));
    };
    out(&mcp_install(
        tool,
        sub == word::sub::UNINSTALL,
        a.truthy("print"),
    )?);
    Ok(0)
}

/// `dfw stack detect [--write] | dfw stack set --json <json>|--file <f>`.
fn stack(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let file = project_paths(&root).stack;
    match a.pos(1) {
        Some("detect") => {
            let detected = detect_stack(&root)?;
            if a.truthy("write") {
                let mut data = js::spread(Some(&detected));
                data.insert("status".into(), json!("proposed"));
                data.insert("source".into(), json!("detected"));
                write_json_inside(&root, &file, &Value::Object(data))?;
            }
            out(&to_json_pretty(&detected));
        }
        Some("set") => {
            // `--file` alone never gets here (`require_flag_values`).
            let given = match (a.text("file").filter(|f| !f.is_empty()), a.get("json")) {
                (Some(f), _) => read_json(&resolve(f), None)?,
                // `--json` alone is `true`, and JSON.parse(true) is true.
                (None, Some(ArgValue::Flag)) => json!(true),
                (None, Some(ArgValue::Text(t))) if !t.is_empty() => serde_json::from_str(t)?,
                _ => json!({}),
            };
            let mut data = js::spread(Some(&given));
            data.insert("status".into(), json!("proposed"));
            data.insert("proposedAt".into(), json!(now_iso()));
            write_json_inside(&root, &file, &Value::Object(data))?;
            if let Some(mut state) = load_state(&root)? {
                let gates = state
                    .get_mut("gates")
                    .filter(|g| !g.is_null())
                    .ok_or_else(|| state_field_error("gates", "an object"))?;
                if js::truthy_value(gates.get("G3")).is_some() {
                    gates["G3"] = Value::Null;
                    push_history(
                        &mut state,
                        json!({ "at": now_iso(), "event": "stack-changed", "note": "G3 invalidated by a new stack proposal" }),
                    )?;
                    save_state(&root, &mut state)?;
                }
            }
            out(&format!("Stack proposal written to {}. Approve with \"dfw approve G3 --note ...\" after the user confirms.", rel::STACK));
        }
        _ => {
            return Err(DfwError::new(
                "Usage: dfw stack detect [--write] | dfw stack set --json <json>|--file <f>",
            ))
        }
    }
    Ok(0)
}

/// `dfw handoff` (`cmdHandoff`).
fn handoff(ctx: &Ctx) -> Result<i32, DfwError> {
    out(&write_handoff(&ctx.root())?);
    Ok(0)
}
