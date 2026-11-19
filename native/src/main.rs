//! dfw: design-first frontend workflow CLI (native core).
//!
//! The command line: the arguments select one command of the table (`cli`), its handler runs
//! and its exit code is the process's. The dispatch order is the one of the Node CLI of 0.5.2,
//! which the golden tests (tests/golden) recorded.
//! The end of every command is a record point of `telemetry` (off unless a collector address exists).

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
    if !internal && ctx.args.pos(0) != Some("hook") {
        require_flag_values(&ctx.args)?;
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
/// `version`, `help` (also `--help` on any command), then the first positional. The Node CLI ran
/// `mcp`, `install` and `uninstall` before it looked at `--help`, so `dfw install --help`
/// installed; here it prints the help.
fn command_name(argv: &[String], args: &Args) -> String {
    let first = argv.first().map(String::as_str).unwrap_or_default();
    if cli::INTERNAL.contains(&first) {
        return first.into();
    }
    let cmd = args.pos(0).unwrap_or_default();
    if cmd == "version" || args.truthy("version") {
        "version".into()
    } else if cmd.is_empty() || cmd == "help" || args.truthy("help") {
        "help".into()
    } else {
        cmd.into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(argv: &[&str]) -> String {
        let argv: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        command_name(&argv, &parse_args(&argv))
    }

    #[test]
    fn command_selection_follows_cli_mjs() {
        assert_eq!(name(&[]), "help");
        assert_eq!(name(&[""]), "help");
        assert_eq!(name(&["help"]), "help");
        assert_eq!(name(&["status", "--help"]), "help");
        assert_eq!(name(&["status", "--json"]), "status");
        assert_eq!(
            name(&["--root", "/p", "status"]),
            "status",
            "a flag's value is not the command"
        );
        assert_eq!(name(&["status", "--version"]), "version");
        assert_eq!(name(&["--version"]), "version");
        assert_eq!(name(&["--version="]), "help", "an empty value is falsy");
        // `dfw install --help` must never install (the Node CLI did).
        for cmd in ["mcp", "install", "uninstall"] {
            assert_eq!(name(&[cmd, "--help"]), "help", "{cmd}");
        }
        assert_eq!(name(&["install", "--host", "claude"]), "install");
        assert_eq!(name(&["hook", "stop", "--help"]), "help");
        assert_eq!(name(&["__studio-server", "--help"]), "__studio-server");
        assert_eq!(name(&["nope"]), "nope");
    }
}
