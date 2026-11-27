//! `dfw screen add`: one more screen in an existing design, next to its siblings
//! (`design::screen_add`).

use super::word::{command, sub};
use super::{flag_key, Ctx, Handler};
use crate::core::error::DfwError;
use crate::core::paths::rel;
use crate::core::util::{out, to_json_pretty};
use crate::design::screen_add::{add_screen, Added, NewScreen};

/// The usage line of the command.
const USAGE: &str = "Usage: dfw screen add <id> --page <page> [--after <screen>] [--from <screen>[,<screen>...]] [--title <text>] [--route <path>] [--json]";

/// The flags of `dfw screen add`, by key.
mod key {
    pub const PAGE: &str = "page";
    pub const AFTER: &str = "after";
    pub const FROM: &str = "from";
    pub const TITLE: &str = "title";
    pub const ROUTE: &str = "route";
}

pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![(command::SCREEN, screen as Handler)]
}

/// A value flag's text, `None` when missing or empty.
fn text(ctx: &Ctx, key: &str) -> Option<String> {
    ctx.args
        .text(key)
        .map(str::trim)
        .filter(|t| !t.is_empty())
        .map(str::to_string)
}

fn screen(ctx: &Ctx) -> Result<i32, DfwError> {
    if ctx.args.pos(1) != Some(sub::ADD) {
        return Err(DfwError::new(USAGE));
    }
    let (Some(id), Some(page)) = (ctx.args.pos(2), text(ctx, key::PAGE)) else {
        return Err(DfwError::new(USAGE));
    };
    let from = text(ctx, key::FROM)
        .map(|f| {
            f.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default();
    let added = add_screen(
        &ctx.root(),
        &NewScreen {
            id: id.to_string(),
            page,
            after: text(ctx, key::AFTER),
            from,
            title: text(ctx, key::TITLE),
            route: text(ctx, key::ROUTE),
        },
    )?;
    if ctx.args.truthy(flag_key::JSON) {
        out(&to_json_pretty(&added.to_json()));
    } else {
        out(&report(&added));
    }
    Ok(0)
}

/// What the command did and what the agent does next, as text.
fn report(a: &Added) -> String {
    let place = match &a.after {
        Some(after) => format!("after \"{after}\""),
        None => "at the end".to_string(),
    };
    let mut lines = vec![
        format!(
            "Added screen \"{}\" to page \"{}\", {place}.",
            a.screen, a.page
        ),
        format!("  {}: screen entry and the page's screens", rel::MANIFEST),
    ];
    lines.push(match &a.shell_from {
        Some(sibling) => format!("  {}: skeleton, app-shell as in \"{sibling}\"", a.file),
        None => format!("  {}: skeleton", a.file),
    });
    for f in &a.link_from {
        lines.push(format!(
            "Link to it: add data-dfw-goto=\"{}\" to the control in {f} that opens it.",
            a.screen
        ));
    }
    lines.push(format!(
        "Next: fill its blueprint (dfw blueprint {}), build it from the design system, wire its primary, back and cancel actions, then dfw layout check --changed and dfw check.",
        a.screen
    ));
    lines.join("\n")
}
