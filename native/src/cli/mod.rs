//! The command table. Each `cli/<group>.rs` lists the commands of its group with their handlers.
//!
//! `help.txt` is the `dfw help` text (the one of the Node CLI of 0.5.2, with the mode and phase
//! lists filled in, plus the telemetry line); the golden tests record it.

mod browser;
mod comments;
mod debug;
mod design;
mod edit;
mod hook;
mod penpot;
mod studio;
mod telemetry;
mod tools;
mod workflow;

use std::path::PathBuf;

use crate::core::error::DfwError;
use crate::core::paths::find_project_root;
use crate::core::platform::dfw_version;
use crate::core::util::{out, Args};

/// What a command handler receives.
pub struct Ctx {
    /// The arguments as typed, without the program name.
    pub argv: Vec<String>,
    /// The same, parsed (`a` in cli.mjs; `args.pos(0)` is the command).
    pub args: Args,
}

impl Ctx {
    /// The project the command works on: `--root`, else the search of `find_project_root`.
    pub fn root(&self) -> PathBuf {
        find_project_root(
            self.args
                .text("root")
                .filter(|r| !r.is_empty())
                .map(PathBuf::from)
                .as_deref(),
        )
    }
}

/// A command's handler: runs the command and returns its exit code.
pub type Handler = fn(&Ctx) -> Result<i32, DfwError>;

/// Internal commands, recognised only as the first argument ("dfw studio start" and
/// "dfw review start" spawn them).
pub const INTERNAL: [&str; 2] = ["__studio-server", "__review-server"];

/// Commands cli.mjs dispatches before it looks at `--help`.
pub const BEFORE_HELP: [&str; 3] = ["mcp", "install", "uninstall"];

/// Every command of the CLI.
pub fn commands() -> Vec<(&'static str, Handler)> {
    let mut all: Vec<(&'static str, Handler)> =
        vec![("version", version as Handler), ("help", help as Handler)];
    all.extend(workflow::commands());
    all.extend(hook::commands());
    all.extend(comments::commands());
    all.extend(design::commands());
    all.extend(studio::commands());
    all.extend(browser::commands());
    all.extend(penpot::commands());
    all.extend(tools::commands());
    all.extend(telemetry::commands());
    all.extend(debug::commands());
    all
}

/// `dfw version`: `dfw <version> (native)`.
fn version(_ctx: &Ctx) -> Result<i32, DfwError> {
    out(&format!("dfw {} (native)", dfw_version()));
    Ok(0)
}

/// `dfw help`, `dfw` alone, or `--help` on any command.
fn help(_ctx: &Ctx) -> Result<i32, DfwError> {
    out(include_str!("help.txt"));
    Ok(0)
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
            "version",
            "help",
            "hook",
            "mcp",
            "install",
            "uninstall",
            "__studio-server",
            "__review-server",
            "init",
            "status",
            "mode",
            "phase",
            "approve",
            "reopen",
            "pause",
            "resume",
            "decide",
            "decisions",
            "build",
            "tokens",
            "system",
            "style",
            "type",
            "palette",
            "blueprint",
            "layout",
            "icons",
            "fonts",
            "lint",
            "check",
            "studio",
            "shoot",
            "review",
            "inspector",
            "diff",
            "verify-snapshot",
            "penpot",
            "handoff",
            "comments",
            "stack",
            "lang",
            "config",
            "doctor",
            "setup",
        ] {
            assert!(
                unique.contains(cmd),
                "{cmd} is missing from the command table"
            );
        }
        // `__debug` (hidden; see cli/debug.rs) and `telemetry` (new with the telemetry change)
        // are the commands the Node CLI does not have.
        assert!(unique.contains("__debug") && !include_str!("help.txt").contains("__debug"));
        assert!(unique.contains("telemetry"));
        assert_eq!(names.len(), 46, "a command the Node CLI does not have");
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
