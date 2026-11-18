//! Tooling commands of `core/lib/cli.mjs`: inspector, mcp, install, uninstall, setup, doctor, stack, handoff
//! (`cmdInspector`, `cmdDoctor`, `cmdHandoff`). Lighthouse has no command of its own: it runs inside `check`.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

use regex::Regex;
use serde_json::{json, Value};

use super::{Ctx, Handler};
use crate::browser::download::install_browser;
use crate::browser::launch::{browser_status, BrowserStatus};
use crate::core::config::{js_truthy, set_config_value, Scope};
use crate::core::error::DfwError;
use crate::core::host::{Status, HOSTS};
use crate::core::paths::{
    claude_dir, global_dir, manifest_path, plugin_root, project_paths, resolve,
};
use crate::core::util::{
    list_files, out, read_json, sha256, to_json_compact, to_json_pretty, ArgValue, Args,
    MARKETPLACE_NAME,
};
use crate::tools::binary::{binary_status, install_binary, RELEASE_REPO};
use crate::tools::inspector::{format_plan, inspector_setup, parse_insp_path, PACKAGE, VERSION};
use crate::tools::install::{axe_file, install, uninstall, InstallOptions};

/// The commands of this group. `mcp`, `stack` and `handoff` are at the end of the file.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("inspector", Handler::Native(inspector)),
        ("mcp", Handler::Native(mcp)),
        ("install", Handler::Native(cmd_install)),
        ("uninstall", Handler::Native(cmd_uninstall)),
        ("setup", Handler::Native(setup)),
        ("doctor", Handler::Native(doctor)),
        ("stack", Handler::Native(stack)),
        ("handoff", Handler::Native(handoff)),
    ]
}

/// `a.<key> || fallback` for a flag used as text ("true" when it stood alone).
fn flag_or(a: &Args, key: &str, fallback: &str) -> String {
    match a.get(key) {
        Some(ArgValue::Flag) => "true".to_string(),
        Some(ArgValue::Text(t)) if !t.is_empty() => t.clone(),
        _ => fallback.to_string(),
    }
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

/// `dfw install [--host ...] [--scope ...] [--source ...] [--no-runtime] [--no-binary] [--quiet]`.
/// `--no-binary` is accepted and changes nothing: only the Node CLI downloads the binary.
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

/// `spawnSync(bin, ['--version'])`: whether it ran and succeeded, and what it printed.
fn version_of(bin: &str) -> (bool, String, String) {
    let mut probe = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/d", "/s", "/c", bin, "--version"]);
        c
    } else {
        let mut c = Command::new(bin);
        c.arg("--version");
        c
    };
    match probe.stdin(Stdio::null()).output() {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(_) => (false, String::new(), String::new()),
    }
}

/// `newestDir(dir)`: the most recently changed folder in `dir`.
fn newest_dir(dir: &Path) -> Option<PathBuf> {
    let mut subs: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((fs::metadata(e.path()).ok()?.modified().ok()?, e.path())))
        .collect();
    subs.sort_by_key(|sub| std::cmp::Reverse(sub.0));
    subs.into_iter().next().map(|(_, path)| path)
}

/// `pluginRoot(name)` of locate.mjs: root directory of a plugin from the dfw marketplace,
/// installed or in the dev repo.
// TODO(merge): use review::locate::plugin_root
fn sibling_plugin(name: &str) -> Result<Option<PathBuf>, DfwError> {
    let is_plugin = |dir: &PathBuf| manifest_path(dir).is_some();
    if let Some(parent) = plugin_root().as_deref().and_then(Path::parent) {
        // Sibling in the same marketplace checkout (dev mode, or plugin cache layout), then
        // the dev checkout: core/ next to plugins/.
        for dir in [parent.join(name), parent.join("plugins").join(name)] {
            if is_plugin(&dir) {
                return Ok(Some(dir));
            }
        }
    }
    // Plugin cache: <claudeDir>/plugins/cache/<marketplace>/<plugin>/<version>/
    let cache = claude_dir()
        .join("plugins")
        .join("cache")
        .join(MARKETPLACE_NAME)
        .join(name);
    if let Some(cached) = newest_dir(&cache).filter(is_plugin) {
        return Ok(Some(cached));
    }
    // Other hosts' install directories (e.g. ~/.copilot/installed-plugins/dfw-marketplace/<plugin>).
    for dir in HOSTS.iter().flat_map(|h| h.installed_plugin_dirs()) {
        let p = dir.join(name);
        if is_plugin(&p) {
            return Ok(Some(p));
        }
    }
    // Marketplace repo recorded in the global config.
    let config = read_json(&global_dir().join("config.json"), Some(json!({})))?;
    Ok(config
        .get("marketplaceRepo")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
        .map(|repo| Path::new(repo).join("plugins").join(name))
        .filter(is_plugin))
}

/// `impeccableLauncher()` of locate.mjs, as doctor reads it: `None` without the plugin, else
/// whether its engine is downloaded and which version (none for `IMPECCABLE_BIN`).
// TODO(merge): use review::locate::impeccable_launcher
fn impeccable() -> Result<Option<(bool, Option<String>)>, DfwError> {
    if env::var("IMPECCABLE_BIN").is_ok_and(|bin| !bin.is_empty() && Path::new(&bin).exists()) {
        return Ok(Some((true, None)));
    }
    let Some(root) = sibling_plugin("impeccable")? else {
        return Ok(None);
    };
    let scripts = root.join("skills").join("impeccable").join("scripts");
    if !scripts.join("impeccable").exists() {
        return Ok(None);
    }
    let version = fs::read_to_string(scripts.join("VERSION"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let home = env::var("IMPECCABLE_HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| env::home_dir().unwrap_or_default().join(".impeccable"));
    let ready = !version.is_empty() && home.join("bin").join(&version).join("impeccable").exists();
    Ok(Some((ready, Some(version))))
}

/// A script inside a sibling plugin's skill, when it is there (`sloplintScript`,
/// `tasteCopyLintScript` of locate.mjs).
fn skill_script(plugin: &str, script: &str) -> Result<bool, DfwError> {
    Ok(sibling_plugin(plugin)?.is_some_and(|root| {
        root.join("skills")
            .join(plugin)
            .join("scripts")
            .join(script)
            .exists()
    }))
}

/// `verifySnapshot(root, version)` of freeze.mjs: what differs from the approval lock.
// TODO(merge): use workflow::freeze::verify_snapshot
fn verify_snapshot(root: &Path, version: &str) -> Result<Vec<String>, DfwError> {
    let dir = project_paths(root).approved.join(version);
    let lock = read_json(&dir.join("approval.lock.json"), Some(Value::Null))?;
    if !js_truthy(&lock) {
        return Err(DfwError::new(format!(
            "No approval.lock.json in {}",
            dir.display()
        )));
    }
    let locked: Vec<(&str, &str)> = lock
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|f| {
            let field = |k: &str| f.get(k).and_then(Value::as_str).unwrap_or_default();
            (field("path"), field("sha256"))
        })
        .collect();
    let mut problems = Vec::new();
    for (path, sum) in &locked {
        let abs = dir.join(path);
        if !abs.exists() {
            problems.push(format!("missing: {path}"));
        } else if sha256(fs::read(&abs)?) != *sum {
            problems.push(format!("modified: {path}"));
        }
    }
    for rel in list_files(&dir, &[])? {
        if rel != "approval.lock.json" && !locked.iter().any(|(path, _)| *path == rel) {
            problems.push(format!("added: {rel}"));
        }
    }
    Ok(problems)
}

/// `mcpInstalled(id)` of mcp.mjs: whether a tool's config already has the dfw server.
// TODO(merge): use tools::mcp::mcp_installed
fn mcp_installed(id: &str) -> bool {
    let home = env::home_dir().unwrap_or_default();
    let dir = |var: &str, fallback: PathBuf| {
        env::var(var)
            .ok()
            .filter(|v| !v.is_empty())
            .map_or(fallback, PathBuf::from)
    };
    let (file, key) = match id {
        "opencode" => (
            dir("XDG_CONFIG_HOME", home.join(".config"))
                .join("opencode")
                .join("opencode.json"),
            Some("mcp"),
        ),
        "codex" => (
            dir("CODEX_HOME", home.join(".codex")).join("config.toml"),
            None,
        ),
        "cursor" => (home.join(".cursor").join("mcp.json"), Some("mcpServers")),
        _ => return false,
    };
    let Ok(text) = fs::read_to_string(file) else {
        return false;
    };
    match key {
        // A [mcp_servers.dfw] table header on its own line (LF or CRLF files).
        None => Regex::new(r"(?m)^\[mcp_servers\.dfw\][ \t]*\r?\n")
            .is_ok_and(|header| header.is_match(&text)),
        Some(key) => serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.get(key)?.get("dfw").map(js_truthy))
            .unwrap_or(false),
    }
}

/// `dfw doctor`: environment and installation checks (`cmdDoctor`).
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
    let (py, py_out, py_err) = version_of_plain("python3");
    let py_text = [py_out, py_err]
        .into_iter()
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| "not found".into());
    ok("python3 (UI UX Pro Max search)", py, py_text.trim());
    ok("git", version_of_plain("git").0, "");
    for name in [
        "frontend-design",
        "ux-research",
        "ui-ux-pro-max",
        "impeccable",
        "emil-design-eng",
        "hallmark",
        "taste-reference",
    ] {
        let r = sibling_plugin(name)?;
        ok(
            &format!("plugin {name}"),
            r.is_some(),
            &r.map_or_else(
                || "not found (install from dfw-marketplace)".to_string(),
                |p| p.display().to_string(),
            ),
        );
    }
    match impeccable()? {
        Some((true, version)) => ok(
            "impeccable engine downloaded",
            true,
            &version.unwrap_or_default(),
        ),
        Some((false, _)) => ok(
            "impeccable engine downloaded",
            false,
            "run \"dfw check --setup\" once (downloads a checksum-verified binary from GitHub)",
        ),
        None => ok(
            "impeccable engine downloaded",
            false,
            "impeccable plugin not found",
        ),
    }
    ok("sloplint", skill_script("hallmark", "sloplint.mjs")?, "");
    ok(
        "taste copy-lint",
        skill_script("taste-reference", "copy-lint.mjs")?,
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
    // TODO(merge): use browser::a11y::axe_status
    let axe = axe_file();
    if axe.exists() {
        ok(
            "axe-core for the accessibility check",
            true,
            &format!("4.14.0: {}", axe.display()),
        );
    } else {
        ok(
            "axe-core for the accessibility check",
            false,
            "axe-core not downloaded (dfw setup --a11y, ~550 KB, once)",
        );
    }
    // Duplicate installs of the same upstream skills from other marketplaces load twice.
    let installed_file = claude_dir().join("plugins").join("installed_plugins.json");
    let installed = read_json(&installed_file, Some(Value::Null))?;
    let plugins = installed.get("plugins").filter(|p| js_truthy(p));
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
    // TODO(merge): use workflow::state::load_state
    let state = read_json(&project_paths(&root).state, Some(Value::Null))?;
    let field = |k: &str| match state.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => "undefined".to_string(),
    };
    if js_truthy(&state) {
        ok(
            "project workflow",
            true,
            &format!("{}/{}", field("mode"), field("phase")),
        );
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
    } else {
        ok("project workflow", true, "not initialized here");
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
                .is_some_and(js_truthy)
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
    out(&rows.join("\n"));
    Ok(0)
}

/// `spawnSync(bin, ['--version'])` without a shell (python3, git).
fn version_of_plain(bin: &str) -> (bool, String, String) {
    match Command::new(bin)
        .arg("--version")
        .stdin(Stdio::null())
        .output()
    {
        Ok(o) => (
            o.status.success(),
            String::from_utf8_lossy(&o.stdout).into_owned(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        ),
        Err(_) => (false, String::new(), String::new()),
    }
}

/* ---------------------------------------------------------------- mcp, stack, handoff */

use crate::core::config::js_spread;
use crate::core::util::{now_iso, write_json};
use crate::tools::handoff::write_handoff;
use crate::tools::mcp::serve_mcp;
use crate::tools::mcp_install::{mcp_install, which_tool};
use crate::workflow::stack::detect_stack;
use crate::workflow::state::{load_state, push_history, save_state, truthy, type_error};

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
                let mut data = js_spread(Some(&detected));
                data.insert("status".into(), json!("proposed"));
                data.insert("source".into(), json!("detected"));
                write_json(&file, &Value::Object(data))?;
            }
            out(&to_json_pretty(&detected));
        }
        Some("set") => {
            let given = match (a.truthy("file"), a.get("file"), a.get("json")) {
                (true, Some(ArgValue::Text(f)), _) => read_json(&resolve(f), None)?,
                // `--file` alone is `true`, which path.resolve refuses.
                (true, ..) => {
                    return Err(DfwError::internal(
                        "The \"paths[0]\" argument must be of type string. Received type boolean (true)",
                    ))
                }
                // `--json` alone is `true`, and JSON.parse(true) is true.
                (_, _, Some(ArgValue::Flag)) => json!(true),
                (_, _, Some(ArgValue::Text(t))) if !t.is_empty() => serde_json::from_str(t)?,
                _ => json!({}),
            };
            let mut data = js_spread(Some(&given));
            data.insert("status".into(), json!("proposed"));
            data.insert("proposedAt".into(), json!(now_iso()));
            write_json(&file, &Value::Object(data))?;
            if let Some(mut state) = load_state(&root)? {
                let gates = state
                    .get_mut("gates")
                    .filter(|g| !g.is_null())
                    .ok_or_else(|| type_error("G3"))?;
                if truthy(gates.get("G3")).is_some() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::{test_dir, write_json};

    #[test]
    fn snapshot_problems_are_missing_modified_and_added() {
        let root = test_dir("doctor-snapshot");
        let dir = project_paths(&root).approved.join("v1");
        fs::create_dir_all(dir.join("screens")).unwrap();
        fs::write(dir.join("kept.html"), "kept").unwrap();
        fs::write(dir.join("screens/changed.html"), "now").unwrap();
        fs::write(dir.join("extra.css"), "x").unwrap();
        assert_eq!(
            verify_snapshot(&root, "v1").unwrap_err().message(),
            format!("No approval.lock.json in {}", dir.display())
        );
        write_json(
            &dir.join("approval.lock.json"),
            &json!({ "files": [
                { "path": "kept.html", "sha256": sha256("kept") },
                { "path": "screens/changed.html", "sha256": sha256("before") },
                { "path": "gone.html", "sha256": sha256("gone") },
            ] }),
        )
        .unwrap();
        assert_eq!(
            verify_snapshot(&root, "v1").unwrap(),
            [
                "modified: screens/changed.html",
                "missing: gone.html",
                "added: extra.css"
            ]
        );
    }

    #[test]
    fn newest_folder_wins() {
        let dir = test_dir("doctor-newest");
        assert_eq!(newest_dir(&dir.join("none")), None);
        fs::create_dir(dir.join("0.4.0")).unwrap();
        fs::write(dir.join("file"), "").unwrap();
        assert_eq!(newest_dir(&dir), Some(dir.join("0.4.0")));
    }
}
