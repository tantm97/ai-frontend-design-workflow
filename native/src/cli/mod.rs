//! The command table. Each `cli/<group>.rs` lists the commands of its group with their handlers.
//!
//! `help.txt` is the `dfw help` text, one line per public command; the golden tests record it.

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
pub use crate::core::word;
mod workflow;

use std::path::PathBuf;

use crate::core::error::DfwError;
use crate::core::paths::find_project_root;
use crate::core::platform::dfw_version;
use crate::core::util::{out, Args};
use word::{command, sub};

/// What a command handler receives.
pub struct Ctx {
    /// The arguments as typed, without the program name.
    pub argv: Vec<String>,
    /// The same, parsed (`args.pos(0)` is the command).
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
pub const INTERNAL: [&str; 2] = [command::STUDIO_SERVER, command::REVIEW_SERVER];

/// Every command of the CLI.
pub fn commands() -> Vec<(&'static str, Handler)> {
    let mut all: Vec<(&'static str, Handler)> = vec![
        (command::VERSION, version as Handler),
        (command::HELP, help as Handler),
    ];
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

/// The `dfw help` text.
const HELP: &str = include_str!("help.txt");

/// `dfw help`, `dfw` alone, or `--help`, `-h` or a lone `help` after a command: that command's
/// lines of the help, or all of it for a word the help does not name.
fn help(ctx: &Ctx) -> Result<i32, DfwError> {
    let cmd = ctx
        .args
        .pos(0)
        .filter(|c| !["", command::HELP, "-h"].contains(c));
    match cmd.and_then(command_help) {
        Some(text) => out(&text),
        None => out(HELP),
    }
    Ok(0)
}

/// The lines of the help about one command: its usage lines and the lines that continue them.
/// `None` for a word the help does not name.
pub fn command_help(cmd: &str) -> Option<String> {
    let mut lines = Vec::new();
    let mut taking = false;
    for line in HELP.lines() {
        if let Some(usage) = line.strip_prefix("  dfw ") {
            let words: Vec<&str> = usage.split_whitespace().collect();
            // "dfw pause | resume" names two commands.
            taking = words.first() == Some(&cmd)
                || (words.get(1) == Some(&"|") && words.get(2) == Some(&cmd));
        } else if !line.starts_with("        ") {
            taking = false;
        }
        if taking {
            lines.push(line);
        }
    }
    (!lines.is_empty()).then(|| {
        format!(
            "{}\n\nRun \"dfw help\" for every command.",
            lines.join("\n")
        )
    })
}

/// Whether the arguments ask for a command's help instead of running it: `--help`, `-h`
/// anywhere (also where `parse_args` would read it as a flag's value: `setup --browser -h`), or
/// `help` as the only word after the command.
pub fn wants_help(argv: &[String], args: &Args) -> bool {
    let words = args.positionals.get(1..).unwrap_or_default();
    args.truthy("help") || argv.iter().any(|w| w == "-h") || words == [command::HELP]
}

/// The flags and the number of words after the command name that a command with effects outside
/// the project takes; `None` for every other command.
fn strict_form(args: &Args) -> Option<(&'static [&'static str], usize)> {
    let form: (&[&str], usize) = match (args.pos(0)?, args.pos(1)) {
        (command::INSTALL, _) => (
            &[
                "host",
                "scope",
                "source",
                "no-runtime",
                "no-binary",
                "quiet",
            ],
            0,
        ),
        (command::UNINSTALL, _) => (&["host", "scope", "quiet"], 0),
        (command::SETUP, _) => (
            &[
                "browser",
                "playwright",
                "a11y",
                "binary",
                "marketplace-repo",
            ],
            0,
        ),
        (command::MCP, Some(sub::INSTALL | sub::UNINSTALL)) => (&["print"], 2),
        (command::TELEMETRY, None) => (&[], 0),
        (command::TELEMETRY, Some(sub::ENDPOINT)) => (&[], 2),
        (command::TELEMETRY, Some(sub::SHOW)) => (&["raw"], 1),
        (command::TELEMETRY, Some(_)) => (&[], 1),
        (command::CONFIG, _) if args.get("global").is_some() => {
            (&["global", "project", "local", "root"], 3)
        }
        _ => return None,
    };
    Some(form)
}

/// The names of the flags as typed (`--k`, `--k=v`, `--k v`), read as `parse_args` reads them.
fn typed_flags(argv: &[String]) -> Vec<&str> {
    let mut flags = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        if let Some(body) = argv[i].strip_prefix("--") {
            match body.split_once('=') {
                Some((key, _)) => flags.push(key),
                None => {
                    flags.push(body);
                    if argv.get(i + 1).is_some_and(|next| !next.starts_with("--")) {
                        i += 1;
                    }
                }
            }
        }
        i += 1;
    }
    flags
}

/// A command with effects outside the project (install, uninstall, setup, mcp install and
/// uninstall, telemetry, config --global) refuses a flag it does not know and a stray word: a
/// typo must not run it with defaults.
pub fn check_strict(ctx: &Ctx) -> Result<(), DfwError> {
    let Some((flags, words)) = strict_form(&ctx.args) else {
        return Ok(());
    };
    let cmd = ctx.args.pos(0).unwrap_or_default();
    let usage = || command_help(cmd).unwrap_or_default();
    if let Some(flag) = typed_flags(&ctx.argv)
        .into_iter()
        .find(|f| !flags.contains(f))
    {
        return Err(DfwError::new(format!(
            "Unknown option --{flag} for \"dfw {cmd}\".\n{}",
            usage()
        )));
    }
    if let Some(word) = ctx.args.positionals.get(1 + words) {
        return Err(DfwError::new(format!(
            "Unexpected word \"{word}\" for \"dfw {cmd}\".\n{}",
            usage()
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::host::id;
    use std::collections::HashSet;
    use word::flag;

    #[test]
    fn every_command_is_listed_once() {
        let names: Vec<&str> = commands().iter().map(|(n, _)| *n).collect();
        let unique: HashSet<&str> = names.iter().copied().collect();
        assert_eq!(names.len(), unique.len(), "{names:?}");
        // Every public command.
        use command::*;
        for cmd in [
            VERSION,
            HELP,
            HOOK,
            MCP,
            INSTALL,
            UNINSTALL,
            STUDIO_SERVER,
            REVIEW_SERVER,
            INIT,
            STATUS,
            MODE,
            PHASE,
            APPROVE,
            REOPEN,
            PAUSE,
            RESUME,
            DECIDE,
            DECISIONS,
            BUILD,
            TOKENS,
            SYSTEM,
            STYLE,
            TYPE,
            PALETTE,
            BLUEPRINT,
            LAYOUT,
            ICONS,
            FONTS,
            LINT,
            CHECK,
            STUDIO,
            SHOOT,
            REVIEW,
            INSPECTOR,
            DIFF,
            VERIFY_SNAPSHOT,
            PENPOT,
            HANDOFF,
            COMMENTS,
            STACK,
            LANG,
            CONFIG,
            DOCTOR,
            SETUP,
        ] {
            assert!(
                unique.contains(cmd),
                "{cmd} is missing from the command table"
            );
        }
        // Besides those: `__debug` (hidden; see cli/debug.rs) and `telemetry`.
        assert!(unique.contains(DEBUG) && !include_str!("help.txt").contains(DEBUG));
        assert!(unique.contains(TELEMETRY));
        assert_eq!(names.len(), 46, "a command missing from the list above");
        for name in INTERNAL.iter().chain([MCP, INSTALL, UNINSTALL].iter()) {
            assert!(unique.contains(name));
        }
    }

    fn ctx(argv: &[&str]) -> Ctx {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        let args = crate::core::util::parse_args(&argv);
        Ctx { argv, args }
    }

    #[test]
    fn command_help_is_the_lines_of_that_command() {
        let install = command_help(command::INSTALL).unwrap();
        assert!(install.starts_with("  dfw install [--host auto|claude|copilot|all]"));
        assert!(install.contains("\n                                                Register dfw"));
        assert!(install.ends_with("\n\nRun \"dfw help\" for every command."));
        assert!(!install.contains(command::UNINSTALL));
        let approve = command_help(command::APPROVE).unwrap();
        assert!(
            approve.contains("[--accept-findings]") && approve.contains("[--accept-unverified]")
        );
        assert!(command_help("resume")
            .unwrap()
            .starts_with("  dfw pause | resume"));
        assert_eq!(
            command_help("comments")
                .unwrap()
                .matches("  dfw comments")
                .count(),
            11
        );
        assert_eq!(command_help("nope"), None);
        // Every command of the table has help lines, except the hidden ones.
        for (name, _) in commands() {
            if !name.starts_with("__") && name != command::HELP {
                assert!(command_help(name).is_some(), "{name}");
            }
        }
    }

    /// TR-3: commands with effects outside the project refuse unknown flags and stray words.
    #[test]
    fn commands_outside_the_project_refuse_unknown_flags_and_words() {
        use command::{CONFIG, INSTALL, MCP, SETUP, TELEMETRY, UNINSTALL};
        use flag::{GLOBAL, HOST, RAW};
        use sub::{ENDPOINT, SET};
        /// A stray word after a command.
        const NOW: &str = "now";
        /// A collector address.
        const ENDPOINT_URL: &str = "https://a.example";
        /// A misspelt flag of `config set --global`.
        const TYPO: &str = "--forse";
        let refused = |argv: &[&str]| check_strict(&ctx(argv)).unwrap_err().message().to_string();
        let ok = |argv: &[&str]| check_strict(&ctx(argv)).is_ok();
        assert!(refused(&[INSTALL, "--hots", id::CLAUDE])
            .starts_with("Unknown option --hots for \"dfw install\".\n  dfw install ["));
        assert!(
            refused(&[INSTALL, NOW]).starts_with("Unexpected word \"now\" for \"dfw install\".")
        );
        assert!(refused(&[UNINSTALL, "--source", "x"]).starts_with("Unknown option --source"));
        assert!(refused(&[SETUP, "--a11y", flag::FORCE]).starts_with("Unknown option --force"));
        assert!(refused(&[SETUP, "browser"]).starts_with("Unexpected word \"browser\""));
        assert!(refused(&[MCP, sub::INSTALL, id::CODEX, id::CURSOR])
            .starts_with("Unexpected word \"cursor\""));
        assert!(refused(&[MCP, sub::UNINSTALL, id::CODEX, flag::DRY_RUN])
            .starts_with("Unknown option --dry-run"));
        assert!(refused(&[TELEMETRY, sub::ON, NOW]).starts_with("Unexpected word \"now\""));
        assert!(
            refused(&[TELEMETRY, ENDPOINT, ENDPOINT_URL, "x"]).starts_with("Unexpected word \"x\"")
        );
        assert!(refused(&[TELEMETRY, sub::OFF, RAW]).starts_with("Unknown option --raw"));
        assert!(refused(&[CONFIG, SET, "a", "1", "2", GLOBAL]).starts_with("Unexpected word \"2\""));
        assert!(
            refused(&[CONFIG, SET, "a", "1", GLOBAL, TYPO]).starts_with("Unknown option --forse")
        );
        for good in [
            &[
                INSTALL,
                HOST,
                id::CLAUDE,
                "--scope=project",
                "--no-runtime",
                "--quiet",
            ][..],
            &[INSTALL, "--no-binary"],
            &[UNINSTALL, HOST, "all", "--scope", "user"],
            &[SETUP, "--marketplace-repo", "/x"],
            &[MCP, sub::INSTALL, id::CODEX, flag::PRINT],
            &[MCP],
            &[TELEMETRY],
            &[TELEMETRY, ENDPOINT, ENDPOINT_URL],
            &[TELEMETRY, sub::SHOW, RAW],
            &[CONFIG, SET, "a", "1", GLOBAL],
            // Inside the project: not checked here.
            &[CONFIG, SET, "a", "1", "2", TYPO],
            &[command::STATUS, "--whatever", "x", "y"],
        ] {
            assert!(ok(good), "{good:?}");
        }
    }

    #[test]
    fn help_text_is_the_node_help() {
        let help = HELP;
        assert!(help.starts_with("dfw - design-first workflow CLI\n"));
        assert!(help.ends_with("  dfw version\n"));
    }
}
