//! Ports `core/lib/cli.mjs`: the command table. Each `cli/<group>.rs` lists the commands of its
//! group; a command is `Forward` until its group is ported, then `Native`.
//!
//! `help.txt` is a copy of the `HELP` text in cli.mjs (with the mode and phase lists filled in).
//! Until the Node CLI is deleted the text lives in both places: change both.

mod browser;
mod comments;
mod design;
mod hook;
mod penpot;
mod studio;
mod tools;
mod workflow;

use std::io;
use std::path::PathBuf;
use std::process::Command;

use crate::core::error::DfwError;
use crate::core::paths::{env_nonempty, find_project_root, plugin_root};
use crate::core::util::{out, plugin_manifest, Args};

/// What a command handler receives.
// No handler reads it yet: this allow goes with the first ported command group.
#[allow(dead_code)]
pub struct Ctx {
    /// The arguments as typed, without the program name.
    pub argv: Vec<String>,
    /// The same, parsed (`a` in cli.mjs; `args.pos(0)` is the command).
    pub args: Args,
}

// See the allow on `Ctx`.
#[allow(dead_code)]
impl Ctx {
    /// The project the command works on: `--root`, else the search of `find_project_root`.
    pub fn root(&self) -> PathBuf {
        find_project_root(self.args.text("root").filter(|r| !r.is_empty()).map(PathBuf::from).as_deref())
    }
}

/// How a command runs.
pub enum Handler {
    /// Ported: runs here and returns the exit code.
    Native(fn(&Ctx) -> Result<i32, DfwError>),
    /// Not ported yet: the Node CLI runs it.
    Forward,
}

/// Internal commands, recognised only as the first argument ("dfw studio start" and
/// "dfw review start" spawn them).
pub const INTERNAL: [&str; 2] = ["__studio-server", "__review-server"];

/// Commands cli.mjs dispatches before it looks at `--help`.
pub const BEFORE_HELP: [&str; 3] = ["mcp", "install", "uninstall"];

/// Every command of the CLI.
pub fn commands() -> Vec<(&'static str, Handler)> {
    let mut all: Vec<(&'static str, Handler)> = vec![("version", Handler::Native(version)), ("help", Handler::Native(help))];
    all.extend(workflow::commands());
    all.extend(hook::commands());
    all.extend(comments::commands());
    all.extend(design::commands());
    all.extend(studio::commands());
    all.extend(browser::commands());
    all.extend(penpot::commands());
    all.extend(tools::commands());
    all
}

/// `dfwVersion()` of binary.mjs: the plugin manifest's version, else `DFW_VERSION`, else the
/// version this binary was built as.
pub fn dfw_version() -> String {
    let manifest = plugin_manifest(None).ok();
    let from_manifest = manifest.as_ref().and_then(|m| m.get("version")).and_then(|v| v.as_str()).filter(|v| !v.is_empty());
    match from_manifest {
        Some(v) => v.to_string(),
        None => env_nonempty("DFW_VERSION").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string()),
    }
}

/// `dfw version`: the Node CLI printed `node <version>` or `binary` in the brackets.
fn version(_ctx: &Ctx) -> Result<i32, DfwError> {
    out(&format!("dfw {} (native)", dfw_version()));
    Ok(0)
}

/// `dfw help`, `dfw` alone, or `--help` on any command.
fn help(_ctx: &Ctx) -> Result<i32, DfwError> {
    out(include_str!("help.txt"));
    Ok(0)
}

/// Run the Node CLI with the same arguments, stdin, stdout, stderr and environment, and return
/// its exit code.
pub fn forward(argv: &[String]) -> Result<i32, DfwError> {
    let script = plugin_root().map(|root| root.join("bin").join("dfw.mjs")).filter(|f| f.is_file()).ok_or_else(|| {
        DfwError::new("This command still runs on the Node.js CLI, and bin/dfw.mjs was not found. Set DFW_PLUGIN_ROOT to the dfw plugin folder.")
    })?;
    // DFW_FORWARDED marks the child: the Node side never starts this binary for a forwarded command.
    let status = Command::new("node").arg(&script).args(argv).env("DFW_FORWARDED", "1").status().map_err(|err| match err.kind() {
        io::ErrorKind::NotFound => DfwError::new("This command still runs on the Node.js CLI, and node was not found on PATH. Install Node.js 22 or newer."),
        _ => DfwError::internal(format!("Could not start node {}: {err}", script.display())),
    })?;
    Ok(status.code().unwrap_or_else(|| killed_by(&status)))
}

/// Exit code for a child that ended by a signal, as a shell reports it (128 + signal).
#[cfg(unix)]
fn killed_by(status: &std::process::ExitStatus) -> i32 {
    use std::os::unix::process::ExitStatusExt;
    status.signal().map_or(1, |signal| 128 + signal)
}

#[cfg(not(unix))]
fn killed_by(_status: &std::process::ExitStatus) -> i32 {
    1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn every_command_is_listed_once() {
        let names: Vec<&str> = commands().iter().map(|(n, _)| *n).collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "{names:?}");
        // The switch of cli.mjs and the cases before it.
        for cmd in [
            "version", "help", "hook", "mcp", "install", "uninstall", "__studio-server", "__review-server", "init", "status", "mode", "phase",
            "approve", "reopen", "pause", "resume", "decide", "decisions", "build", "tokens", "system", "style", "type", "palette",
            "blueprint", "layout", "icons", "fonts", "lint", "check", "studio", "shoot", "review", "inspector", "diff", "verify-snapshot",
            "penpot", "handoff", "comments", "stack", "lang", "config", "doctor", "setup",
        ] {
            assert!(unique.contains(cmd), "{cmd} is missing from the command table");
        }
        assert_eq!(names.len(), 44, "a command the Node CLI does not have");
        for name in INTERNAL.iter().chain(BEFORE_HELP.iter()) {
            assert!(unique.contains(name));
        }
    }

    #[test]
    fn help_text_is_the_node_help() {
        let help = include_str!("help.txt");
        assert!(help.starts_with("dfw - design-first workflow CLI\n"));
        assert!(help.ends_with("  dfw version\n"));
    }
}
