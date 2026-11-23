//! The checks of `dfw doctor` (`cmdDoctor` of `core/lib/cli.mjs`): the environment, the
//! installed plugins and tools, the project's approved snapshots and the hosts. The command
//! handler in `cli/tools.rs` prints them; this module only finds them out, in report order.

use std::env;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use crate::browser::a11y::{axe_status, AxeStatus, AXE_VERSION};
use crate::browser::launch::{browser_status, BrowserStatus};
use crate::core::error::DfwError;
use crate::core::host::{Host, Status, HOSTS};
use crate::core::js;
use crate::core::paths::{claude_dir, global_dir};
use crate::core::process::{find_executable, plan_for, run_timed};
use crate::core::util::read_json;
use crate::review::locate::{
    impeccable_engine, plugin_root_of, sloplint_script, taste_copy_lint_script,
};
use crate::tools::binary::binary_status;
use crate::tools::install::copilot_plugin_state;
use crate::tools::mcp_install::mcp_installed;
use crate::workflow::freeze::verify_snapshot;
use crate::workflow::state::load_state;

/// One line of the report: what was checked, whether it is fine, and what was found.
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// The vendored design skills whose plugins the report looks for.
const DESIGN_PLUGINS: [&str; 7] = [
    "frontend-design",
    "ux-research",
    "ui-ux-pro-max",
    "impeccable",
    "emil-design-eng",
    "hallmark",
    "taste-reference",
];

/// Upstream plugin names that also exist in dfw-marketplace: installed from elsewhere, they load twice.
const UPSTREAM_PLUGINS: [&str; 8] = [
    "frontend-design",
    "impeccable",
    "ui-ux-pro-max",
    "hallmark",
    "taste-skill",
    "taste",
    "emil-design-eng",
    "example-skills",
];

/// The checks, in the order the report prints them.
#[derive(Default)]
struct Report {
    checks: Vec<Check>,
}

impl Report {
    fn add(&mut self, name: &str, ok: bool, detail: &str) {
        self.checks.push(Check {
            name: name.to_string(),
            ok,
            detail: detail.to_string(),
        });
    }
}

/// Every check of `dfw doctor` for the project at `root`, in report order.
///
/// # Errors
/// When a plugin lookup, the installed plugins file, the state or a snapshot cannot be read.
pub fn doctor_checks(root: &Path) -> Result<Vec<Check>, DfwError> {
    let mut report = Report::default();
    check_runtime(&mut report);
    check_programs(&mut report);
    check_plugins(&mut report)?;
    check_engines(&mut report)?;
    check_browser_tools(&mut report);
    check_duplicate_installs(&mut report)?;
    check_project(&mut report, root)?;
    for host in HOSTS.iter().filter(|h| h.status != Status::Internal) {
        check_host(&mut report, host)?;
    }
    report.add(
        "global config dir",
        true,
        &global_dir().display().to_string(),
    );
    Ok(report.checks)
}

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

/// Claude Code's list of installed plugins.
fn installed_plugins_file() -> PathBuf {
    claude_dir().join("plugins").join("installed_plugins.json")
}

/// The running binary: its version and where it is.
fn check_runtime(report: &mut Report) {
    let bin = binary_status();
    let exe = env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_default();
    report.add(
        "dfw runtime",
        true,
        &format!("binary {} ({exe})", bin.version),
    );
}

/// python3 (for the UI UX Pro Max search) and git on PATH.
fn check_programs(report: &mut Report) {
    let (py, py_out, py_err) = version_of("python3");
    let py_text = [py_out, py_err]
        .into_iter()
        .find(|t| !t.is_empty())
        .unwrap_or_else(|| "not found".into());
    report.add("python3 (UI UX Pro Max search)", py, py_text.trim());
    report.add("git", version_of("git").0, "");
}

/// Where each vendored design skill's plugin is installed.
fn check_plugins(report: &mut Report) -> Result<(), DfwError> {
    for name in DESIGN_PLUGINS {
        let r = plugin_root_of(name)?;
        report.add(
            &format!("plugin {name}"),
            r.is_some(),
            &r.map_or_else(
                || "not found (install from dfw-marketplace)".to_string(),
                |p| p.display().to_string(),
            ),
        );
    }
    Ok(())
}

/// The Impeccable engine (and whether its checksum matches), sloplint and Taste's copy-lint.
fn check_engines(report: &mut Report) -> Result<(), DfwError> {
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
            report.add(label, true, &detail);
        }
        Some(e) => report.add(label, false, &e.detail.unwrap_or_default()),
        None => report.add(label, false, "impeccable plugin not found"),
    }
    report.add("sloplint", sloplint_script()?.is_some(), "");
    report.add(
        "taste copy-lint",
        taste_copy_lint_script()?.is_some(),
        "optional",
    );
    Ok(())
}

/// The browser for screenshots and axe-core for the accessibility check.
fn check_browser_tools(report: &mut Report) {
    match browser_status() {
        BrowserStatus::Installed(b) => report.add(
            "browser for screenshots and layout check",
            true,
            &format!("{}: {}", b.source, b.path.display()),
        ),
        BrowserStatus::Missing { detail } => {
            report.add("browser for screenshots and layout check", false, detail);
        }
    }
    match axe_status() {
        AxeStatus::Installed { path } => report.add(
            "axe-core for the accessibility check",
            true,
            &format!("{AXE_VERSION}: {}", path.display()),
        ),
        AxeStatus::Missing { detail } => {
            report.add("axe-core for the accessibility check", false, detail);
        }
    }
}

/// Duplicate installs of the same upstream skills from other marketplaces load twice.
fn check_duplicate_installs(report: &mut Report) -> Result<(), DfwError> {
    let installed = read_json(&installed_plugins_file(), Some(Value::Null))?;
    let Some(plugins) = installed.get("plugins").filter(|p| js::truthy(p)) else {
        report.add(
            "duplicate install check",
            true,
            "skipped (no installed_plugins.json; plugins may be loaded with --plugin-dir)",
        );
        return Ok(());
    };
    let dups: Vec<&str> = plugins
        .as_object()
        .into_iter()
        .flat_map(|map| map.keys())
        .map(String::as_str)
        .filter(|n| {
            UPSTREAM_PLUGINS.contains(&n.split('@').next().unwrap_or_default())
                && !n.ends_with("@dfw-marketplace")
        })
        .collect();
    let detail = if dups.is_empty() {
        String::new()
    } else {
        format!("also installed outside dfw-marketplace: {} (skills would load twice; consider disabling them)", dups.join(", "))
    };
    report.add("no duplicate upstream installs", dups.is_empty(), &detail);
    Ok(())
}

/// The project's mode and phase, and whether each approved snapshot is intact.
fn check_project(report: &mut Report, root: &Path) -> Result<(), DfwError> {
    let state = load_state(root)?;
    let detail = state
        .as_ref()
        .map_or("not initialized here".to_string(), |s| {
            format!(
                "{}/{}",
                js::to_string(s.get("mode")),
                js::to_string(s.get("phase"))
            )
        });
    report.add("project workflow", true, &detail);
    let Some(state) = &state else {
        return Ok(());
    };
    let approved = state
        .get("design")
        .and_then(|d| d.get("approved"))
        .and_then(Value::as_array);
    for ap in approved.into_iter().flatten() {
        let version = ap
            .get("version")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let problems = verify_snapshot(root, version)?;
        let first: Vec<&str> = problems.iter().take(5).map(String::as_str).collect();
        report.add(
            &format!("approved snapshot {version} intact"),
            problems.is_empty(),
            &first.join("; "),
        );
    }
    Ok(())
}

/// One host: its CLI and whether the dfw plugin is installed in it, or its MCP setup when
/// its plugin is still planned.
fn check_host(report: &mut Report, h: &Host) -> Result<(), DfwError> {
    let name = format!("host {}", h.name);
    let cli = h.cli.unwrap_or_default();
    if h.status != Status::Supported {
        let detail = if mcp_installed(h.id) {
            "plugin planned; dfw MCP server configured".to_string()
        } else {
            format!("plugin planned; use \"dfw mcp install {}\"", h.id)
        };
        report.add(&name, true, &detail);
        return Ok(());
    }
    let (found, printed, _) = version_of(cli);
    if !found {
        report.add(
            &name,
            true,
            &format!(
                "\"{cli}\" not found (skip, or install it and run \"dfw install --host {}\")",
                h.id
            ),
        );
        return Ok(());
    }
    let (plugin, state) = if h.id == "claude" {
        let installed = read_json(&installed_plugins_file(), Some(json!({})))?
            .get("plugins")
            .and_then(|p| p.get("dfw@dfw-marketplace"))
            .is_some_and(js::truthy);
        let state = if installed {
            "installed".to_string()
        } else {
            format!("missing (dfw install --host {})", h.id)
        };
        (installed, state)
    } else {
        // Copilot: also says when the installed folder still has the two-folder layout.
        copilot_plugin_state(&h.dfw_plugin_dir().unwrap_or_default())
    };
    report.add(
        &name,
        plugin,
        &format!(
            "{}; dfw plugin {state}",
            printed.trim().split('\n').next().unwrap_or_default()
        ),
    );
    Ok(())
}
