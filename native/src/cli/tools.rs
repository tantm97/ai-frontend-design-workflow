//! Tooling commands of `core/lib/cli.mjs`: inspector, mcp, install, uninstall, setup, doctor, stack, handoff
//! (`cmdInspector`, `cmdDoctor`, `cmdHandoff`). Lighthouse has no command of its own: it runs inside `check`.

use std::env;
use std::time::Duration;

use serde_json::{json, Value};

use super::{Ctx, Handler};
use crate::browser::a11y::{axe_status, AxeStatus, AXE_VERSION};
use crate::browser::download::install_browser;
use crate::browser::launch::{browser_status, BrowserStatus};
use crate::core::config::{set_config_value, Scope};
use crate::core::error::DfwError;
use crate::core::host::{Status, HOSTS};
use crate::core::js;
use crate::core::paths::{claude_dir, global_dir, project_paths, resolve};
use crate::core::process::{find_executable, plan_for, run_timed};
use crate::core::util::{out, read_json, to_json_compact, to_json_pretty, ArgValue, Args};
use crate::review::locate::{
    impeccable_engine, plugin_root_of, sloplint_script, taste_copy_lint_script,
};
use crate::tools::binary::{binary_status, install_binary, RELEASE_REPO};
use crate::tools::inspector::{format_plan, inspector_setup, parse_insp_path, PACKAGE, VERSION};
use crate::tools::install::{install, uninstall, InstallOptions};
use crate::tools::mcp_install::mcp_installed;
use crate::workflow::freeze::verify_snapshot;

/// The commands of this group. `mcp`, `stack` and `handoff` are at the end of the file.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("inspector", inspector as Handler),
        ("mcp", mcp as Handler),
        ("install", cmd_install as Handler),
        ("uninstall", cmd_uninstall as Handler),
        ("setup", setup as Handler),
        ("doctor", doctor as Handler),
        ("stack", stack as Handler),
        ("handoff", handoff as Handler),
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

/// `spawnSync(bin, ['--version'])`: whether it ran and succeeded, and what it printed. The
/// program is looked up on PATH, never in the current folder (`claude.bat` in a cloned
/// repository must not run on `dfw doctor`), and gets 15 seconds.
fn version_of(bin: &str) -> (bool, String, String) {
    let Some(exe) = find_executable(bin) else {
        return (false, String::new(), String::new());
    };
    let plan = plan_for(
        &exe.display().to_string(),
        vec!["--version".into()],
        cfg!(windows),
    );
    let ran = run_timed(&plan, None, &[], Duration::from_secs(15));
    (ran.status == Some(0), ran.stdout, ran.stderr)
}

/// `dfw doctor`: environment and installation checks (`cmdDoctor`).
// Long: one row after the other, in the order the report prints them.
#[allow(clippy::too_many_lines)]
fn doctor(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let mut rows: Vec<String> = Vec::new();
    let mut ok = |name: &str, good: bool, detail: &str| {
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!(" - {detail}")
        };
        rows.push(format!(
            "{} {name}{detail}",
            if good { "ok  " } else { "WARN" }
        ));
    };
    let bin = binary_status();
    let exe = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    ok(
        "dfw runtime",
        true,
        &format!("binary {} ({exe})", bin.version),
    );
    let (py, py_out, py_err) = version_of("python3");
    let py_text = [py_out, py_err]
        .into_iter()
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| "not found".into());
    ok("python3 (UI UX Pro Max search)", py, py_text.trim());
    ok("git", version_of("git").0, "");
    for name in [
        "frontend-design",
        "ux-research",
        "ui-ux-pro-max",
        "impeccable",
        "emil-design-eng",
        "hallmark",
        "taste-reference",
    ] {
        let r = plugin_root_of(name)?;
        ok(
            &format!("plugin {name}"),
            r.is_some(),
            &r.map_or_else(
                || "not found (install from dfw-marketplace)".to_string(),
                |p| p.display().to_string(),
            ),
        );
    }
    let label = "impeccable engine verified";
    match impeccable_engine()? {
        Some(e) if e.ready => {
            let cmd = e.cmd.as_deref().map(|c| c.display().to_string());
            let detail = if e.verified {
                format!("{} (sha256 matches the pinned checksum)", e.version)
            } else {
                format!(
                    "UNVERIFIED {} (impeccable.allowUnverifiedEngine)",
                    cmd.unwrap_or_default()
                )
            };
            ok(label, true, &detail);
        }
        Some(e) => ok(label, false, &e.detail.unwrap_or_default()),
        None => ok(label, false, "impeccable plugin not found"),
    }
    ok("sloplint", sloplint_script()?.is_some(), "");
    ok(
        "taste copy-lint",
        taste_copy_lint_script()?.is_some(),
        "optional",
    );
    match browser_status() {
        BrowserStatus::Installed(b) => ok(
            "browser for screenshots and layout check",
            true,
            &format!("{}: {}", b.source, b.path.display()),
        ),
        BrowserStatus::Missing { detail } => {
            ok("browser for screenshots and layout check", false, detail)
        }
    }
    match axe_status() {
        AxeStatus::Installed { path } => ok(
            "axe-core for the accessibility check",
            true,
            &format!("{AXE_VERSION}: {}", path.display()),
        ),
        AxeStatus::Missing { detail } => ok("axe-core for the accessibility check", false, detail),
    }
    // Duplicate installs of the same upstream skills from other marketplaces load twice.
    let installed_file = claude_dir().join("plugins").join("installed_plugins.json");
    let installed = read_json(&installed_file, Some(Value::Null))?;
    let plugins = installed.get("plugins").filter(|p| js::truthy(p));
    match plugins {
        Some(plugins) => {
            let ours = [
                "frontend-design",
                "impeccable",
                "ui-ux-pro-max",
                "hallmark",
                "taste-skill",
                "taste",
                "emil-design-eng",
                "example-skills",
            ];
            let dups: Vec<&str> = plugins
                .as_object()
                .into_iter()
                .flat_map(|map| map.keys())
                .map(String::as_str)
                .filter(|n| {
                    ours.contains(&n.split('@').next().unwrap_or_default())
                        && !n.ends_with("@dfw-marketplace")
                })
                .collect();
            let detail = if dups.is_empty() {
                String::new()
            } else {
                format!("also installed outside dfw-marketplace: {} (skills would load twice; consider disabling them)", dups.join(", "))
            };
            ok("no duplicate upstream installs", dups.is_empty(), &detail);
        }
        None => ok(
            "duplicate install check",
            true,
            "skipped (no installed_plugins.json; plugins may be loaded with --plugin-dir)",
        ),
    }
    let state = load_state(&root)?;
    let detail = state
        .as_ref()
        .map_or("not initialized here".to_string(), |s| {
            format!(
                "{}/{}",
                js::to_string(s.get("mode")),
                js::to_string(s.get("phase"))
            )
        });
    ok("project workflow", true, &detail);
    if let Some(state) = &state {
        let approved = state
            .get("design")
            .and_then(|d| d.get("approved"))
            .and_then(Value::as_array);
        for ap in approved.into_iter().flatten() {
            let version = ap
                .get("version")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let problems = verify_snapshot(&root, version)?;
            let first: Vec<&str> = problems.iter().take(5).map(String::as_str).collect();
            ok(
                &format!("approved snapshot {version} intact"),
                problems.is_empty(),
                &first.join("; "),
            );
        }
    }
    for h in HOSTS.iter().filter(|h| h.status != Status::Internal) {
        let name = format!("host {}", h.name);
        let cli = h.cli.unwrap_or_default();
        if h.status != Status::Supported {
            let detail = if mcp_installed(h.id) {
                "plugin planned; dfw MCP server configured".to_string()
            } else {
                format!("plugin planned; use \"dfw mcp install {}\"", h.id)
            };
            ok(&name, true, &detail);
            continue;
        }
        let (found, printed, _) = version_of(cli);
        if !found {
            ok(
                &name,
                true,
                &format!(
                    "\"{cli}\" not found (skip, or install it and run \"dfw install --host {}\")",
                    h.id
                ),
            );
            continue;
        }
        let plugin = if h.id == "claude" {
            read_json(&installed_file, Some(json!({})))?
                .get("plugins")
                .and_then(|p| p.get("dfw@dfw-marketplace"))
                .is_some_and(js::truthy)
        } else {
            h.dfw_plugin_dir()
                .is_some_and(|dir| dir.join("bin").join("dfw").exists())
        };
        let state = if plugin {
            "installed".to_string()
        } else {
            format!("missing (dfw install --host {})", h.id)
        };
        ok(
            &name,
            plugin,
            &format!(
                "{}; dfw plugin {state}",
                printed.trim().split('\n').next().unwrap_or_default()
            ),
        );
    }
    ok(
        "global config dir",
        true,
        &global_dir().display().to_string(),
    );
    ok("telemetry", true, &crate::telemetry::status_line());
    out(&rows.join("\n"));
    Ok(0)
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
    if sub != "install" && sub != "uninstall" {
        return Err(DfwError::new(
            "Usage: dfw mcp | dfw mcp install|uninstall <opencode|codex|cursor|all> [--print]",
        ));
    }
    let Some(tool) = a.pos(2).filter(|t| !t.is_empty()) else {
        return Err(DfwError::new(which_tool()));
    };
    out(&mcp_install(tool, sub == "uninstall", a.truthy("print"))?);
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
            out("Stack proposal written to .dfw/stack.json. Approve with \"dfw approve G3 --note ...\" after the user confirms.");
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
