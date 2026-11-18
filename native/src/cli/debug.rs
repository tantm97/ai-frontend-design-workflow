//! `dfw __debug <subcommand>`: hidden commands that print what a ported module computes, so
//! `tests/conformance/*.conformance.mjs` can compare it with the Node module. Not in the help
//! text, not for users; it goes when the Node core is deleted.
//!
//! To add a subcommand: write `pub(super) fn debug_<name>(ctx: &Ctx) -> Result<i32, DfwError>` in
//! your own `cli/<group>.rs` (arguments start at `ctx.args.pos(2)`, print with `out`), and add
//! one line to `SUBCOMMANDS` below: `("<name>", super::<group>::debug_<name>)`.

use std::path::Path;

use serde_json::Value;

use super::{Ctx, Handler};
use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::host::{format_hook_output, render_text};
use crate::core::util::{out, read_stdin, to_json_compact};

type Subcommand = fn(&Ctx) -> Result<i32, DfwError>;

/// Every debug subcommand: one line per subcommand, the function lives with its owner group.
const SUBCOMMANDS: &[(&str, Subcommand)] = &[
    ("config", config),
    ("host-format", host_format),
    ("host-render", host_render),
    ("hot", super::workflow::debug_hot),
    ("compose", super::design::debug_compose),
    ("color", super::design::debug_color),
    ("png-info", super::browser::debug_png_info),
    ("insp-path", super::tools::debug_insp_path),
    ("penpot-build-export", super::penpot::debug_build_export),
    ("penpot-sync", super::penpot::debug_sync),
];

/// The command of this file.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![("__debug", Handler::Native(run))]
}

fn run(ctx: &Ctx) -> Result<i32, DfwError> {
    let name = ctx.args.pos(1).unwrap_or_default();
    match SUBCOMMANDS.iter().find(|(n, _)| *n == name) {
        Some((_, sub)) => sub(ctx),
        None => {
            let known: Vec<&str> = SUBCOMMANDS.iter().map(|(n, _)| *n).collect();
            Err(DfwError::new(format!(
                "Unknown debug subcommand \"{name}\". Known: {}",
                known.join(", ")
            )))
        }
    }
}

/// `__debug config [root]`: `JSON.stringify(loadConfig(root))`.
fn config(ctx: &Ctx) -> Result<i32, DfwError> {
    out(&to_json_compact(&load_config(
        ctx.args.pos(2).map(Path::new),
    )?));
    Ok(0)
}

/// `__debug host-format <host> <event>`: a neutral hook result as JSON on stdin (nothing is
/// null), `JSON.stringify(formatHookOutput(host, event, result))` on stdout.
fn host_format(ctx: &Ctx) -> Result<i32, DfwError> {
    let stdin = read_stdin()?;
    let result: Value = if stdin.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&stdin)?
    };
    let formatted = format_hook_output(
        ctx.args.pos(2).unwrap_or_default(),
        ctx.args.pos(3).unwrap_or_default(),
        &result,
    )?;
    out(&to_json_compact(&formatted.unwrap_or(Value::Null)));
    Ok(0)
}

/// `__debug host-render <host>`: text on stdin, `renderText(text, host)` and a newline on stdout.
fn host_render(ctx: &Ctx) -> Result<i32, DfwError> {
    out(&render_text(
        &read_stdin()?,
        ctx.args.pos(2).unwrap_or_default(),
    )?);
    Ok(0)
}
