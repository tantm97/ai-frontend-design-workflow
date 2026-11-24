//! dfw: design-first frontend workflow CLI (native core).
//!
//! The command line: the arguments select one command of the table (`cli`), its handler runs
//! and its exit code is the process's. The golden tests (tests/golden) hold the dispatch order.
//! The end of every command is a record point of `telemetry` (off unless a collector address exists).

// Tests build their fixtures with std::fs and std::process freely; the product build (without
// cfg(test)) is the one clippy.toml's disallowed-methods checks.
#![cfg_attr(
    test,
    allow(
        clippy::disallowed_methods,
        reason = "test fixtures use std::fs and std::process directly"
    )
)]

mod assets;
mod browser;
mod cli;
mod core;
mod design;
mod hooks;
mod penpot;
mod review;
mod studio;
mod telemetry;
mod tools;
mod workflow;

use crate::cli::word::command;
use crate::cli::Ctx;
use crate::core::error::DfwError;
use crate::core::util::{parse_args, require_flag_values, Args};

fn main() {
    let started = std::time::Instant::now();
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&argv);
    let name = command_name(&argv, &args);
    let ctx = Ctx { argv, args };
    let code = match run(&ctx, &name) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{}", err.report());
            err.exit_code()
        }
    };
    // After everything the command prints, and unable to change its exit code.
    telemetry::command_finished(&name, &ctx, started, code);
    std::process::exit(code);
}

fn run(ctx: &Ctx, name: &str) -> Result<i32, DfwError> {
    let argv = &ctx.argv;
    // The internal server commands exist only as the first argument; anywhere else the command
    // table does not know them.
    let misplaced = cli::INTERNAL.contains(&name) && argv.first().map(String::as_str) != Some(name);
    let internal = cli::INTERNAL.contains(&name) && !misplaced;
    // A hook checks its flags inside the handler: it fails open.
    if !internal && ctx.args.pos(0) != Some(command::HOOK) {
        require_flag_values(&ctx.args)?;
    }
    if !internal && name != command::HELP && name != command::VERSION {
        cli::check_strict(ctx)?;
    }
    let table = cli::commands();
    let handler = table
        .iter()
        .find(|(n, _)| *n == name && !misplaced)
        .map(|(_, h)| h);
    match handler {
        Some(handler) => handler(ctx),
        None => Err(DfwError::new(format!(
            "Unknown command \"{name}\". Run \"dfw help\"."
        ))),
    }
}

/// Which command the arguments select: an internal server command as first argument,
/// `version`, `help` (also `--help`, `-h` or a lone `help` after any command: the help handler
/// prints that command's help), then the first positional. No command runs when its help is
/// asked for: `dfw install -h` installs nothing.
fn command_name(argv: &[String], args: &Args) -> String {
    let first = argv.first().map(String::as_str).unwrap_or_default();
    if cli::INTERNAL.contains(&first) {
        return first.into();
    }
    let cmd = args.pos(0).unwrap_or_default();
    if cmd == command::VERSION || args.truthy("version") {
        command::VERSION.into()
    } else if cmd.is_empty() || cmd == command::HELP || cmd == "-h" || cli::wants_help(argv, args) {
        command::HELP.into()
    } else {
        cmd.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::word::{command, flag, hook_name, sub};
    use crate::core::host::id;

    /// A word that is no command.
    const NOPE: &str = "nope";

    fn name(argv: &[&str]) -> String {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        command_name(&argv, &parse_args(&argv))
    }

    #[test]
    fn command_selection() {
        use command::{HELP, STATUS, VERSION};
        assert_eq!(name(&[]), HELP);
        assert_eq!(name(&[""]), HELP);
        assert_eq!(name(&[HELP]), HELP);
        assert_eq!(name(&[STATUS, flag::HELP]), HELP);
        assert_eq!(name(&[STATUS, flag::JSON]), STATUS);
        assert_eq!(
            name(&[flag::ROOT, "/p", STATUS]),
            STATUS,
            "a flag's value is not the command"
        );
        assert_eq!(name(&[STATUS, flag::VERSION]), VERSION);
        assert_eq!(name(&[flag::VERSION]), VERSION);
        assert_eq!(name(&["--version="]), HELP, "an empty value is falsy");
        // `dfw install --help` must never install (the Node CLI did), nor -h or a lone help.
        for cmd in [
            command::MCP,
            command::INSTALL,
            command::UNINSTALL,
            command::SETUP,
            command::TELEMETRY,
            STATUS,
        ] {
            for asked in [flag::HELP, "-h", HELP] {
                assert_eq!(name(&[cmd, asked]), HELP, "{cmd} {asked}");
            }
        }
        assert_eq!(name(&[command::MCP, sub::INSTALL, id::CODEX, "-h"]), HELP);
        assert_eq!(name(&["-h"]), HELP);
        assert_eq!(
            name(&[command::SETUP, "--browser", "-h"]),
            HELP,
            "-h as a flag's value"
        );
        assert_eq!(
            name(&[command::COMMENTS, sub::SHOW, HELP]),
            command::COMMENTS,
            "help is a word there"
        );
        assert_eq!(
            name(&[command::INSTALL, flag::HOST, id::CLAUDE]),
            command::INSTALL
        );
        assert_eq!(name(&[command::HOOK, hook_name::STOP, flag::HELP]), HELP);
        assert_eq!(
            name(&[command::STUDIO_SERVER, flag::HELP]),
            command::STUDIO_SERVER
        );
        assert_eq!(name(&[NOPE]), NOPE);
    }
}
