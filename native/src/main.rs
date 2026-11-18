//! dfw: design-first frontend workflow CLI (native core).
//!
//! Ports the dispatch of `core/bin/dfw.mjs` and `main()` in `core/lib/cli.mjs`. Every command is in
//! one of three states: native (ported), forward (still run by the Node CLI) or unknown.
//! The forward state goes when the Node core is deleted (rust-core, stage 4).

mod assets;
mod browser;
mod cli;
mod core;
mod design;
mod hooks;
mod penpot;
mod review;
mod studio;
mod tools;
mod workflow;

use crate::cli::{Ctx, Handler};
use crate::core::error::DfwError;
use crate::core::paths::env_nonempty;
use crate::core::util::{parse_args, Args};

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = match run(argv) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("{}", err.report());
            err.exit_code()
        }
    };
    std::process::exit(code);
}

fn run(argv: Vec<String>) -> Result<i32, DfwError> {
    // Escape hatch while porting: the Node CLI handles everything.
    if env_nonempty("DFW_NATIVE").as_deref() == Some("off") {
        return cli::forward(&argv);
    }
    let args = parse_args(&argv);
    let name = command_name(&argv, &args);
    // The internal server commands exist only as the first argument; anywhere else cli.mjs
    // reaches its switch, which does not know them.
    let misplaced = cli::INTERNAL.contains(&name.as_str()) && argv.first() != Some(&name);
    let table = cli::commands();
    let handler = table.iter().find(|(n, _)| *n == name && !misplaced).map(|(_, h)| h);
    match handler {
        Some(Handler::Native(run)) => run(&Ctx { argv, args }),
        Some(Handler::Forward) => cli::forward(&argv),
        None => Err(DfwError::new(format!("Unknown command \"{name}\". Run \"dfw help\"."))),
    }
}

/// Which command the arguments select, in the order `main()` of cli.mjs decides it: an internal
/// server command as first argument, `version`, the commands checked before `--help`, `help`,
/// then the first positional.
fn command_name(argv: &[String], args: &Args) -> String {
    let first = argv.first().map(String::as_str).unwrap_or_default();
    if cli::INTERNAL.contains(&first) {
        return first.into();
    }
    let cmd = args.pos(0).unwrap_or_default();
    if cmd == "version" || args.truthy("version") {
        "version".into()
    } else if cli::BEFORE_HELP.contains(&cmd) {
        cmd.into()
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
        assert_eq!(name(&["--root", "/p", "status"]), "status", "a flag's value is not the command");
        assert_eq!(name(&["status", "--version"]), "version");
        assert_eq!(name(&["--version"]), "version");
        assert_eq!(name(&["--version="]), "help", "an empty value is falsy");
        assert_eq!(name(&["mcp", "--help"]), "mcp", "mcp, install and uninstall are decided before --help");
        assert_eq!(name(&["install", "--help"]), "install");
        assert_eq!(name(&["hook", "stop", "--help"]), "help");
        assert_eq!(name(&["__studio-server", "--help"]), "__studio-server");
        assert_eq!(name(&["nope"]), "nope");
    }
}
