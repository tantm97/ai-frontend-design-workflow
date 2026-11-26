//! Hidden `__debug` helpers of the edit group (Studio edits, the Design System page, the layout
//! cache). The group has no command of its own: the Studio server calls these modules. The
//! helpers let `tests/golden/edit.golden.mjs` compare them with the recorded results.

use std::path::PathBuf;

use serde_json::{json, Value};

use super::Ctx;
use crate::core::error::DfwError;
use crate::core::paths::resolve;
use crate::core::util::{out, read_stdin, to_json_compact};
use crate::design::edit::{apply_edit, undo_edit};
use crate::design::layout_cache::{changed_screens, mark_checked};
use crate::design::system_page::render_system_page;

/// The keys of what `__debug changed-screens` prints.
mod changed_key {
    /// The screens whose layout must be checked again.
    pub const SCREENS: &str = "screens";
    /// The component masters changed since the last check.
    pub const CHANGED_COMPS: &str = "changedComps";
    pub const TOKENS_CHANGED: &str = "tokensChanged";
}

fn root_of(ctx: &Ctx, usage: &str) -> Result<PathBuf, DfwError> {
    ctx.args
        .pos(2)
        .map(resolve)
        .ok_or_else(|| DfwError::new(format!("Usage: dfw __debug {usage}")))
}

/// `__debug edit <root>`: an edit op as JSON on stdin, `JSON.stringify(applyEdit(root, op))`.
pub(super) fn debug_edit(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = root_of(ctx, "edit <root> (op JSON on stdin)")?;
    let op: Value = serde_json::from_str(&read_stdin()?)?;
    out(&to_json_compact(&apply_edit(&root, &op)?.to_json()));
    Ok(0)
}

/// `__debug undo <root>`: `JSON.stringify(undoEdit(root))`.
pub(super) fn debug_undo(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = root_of(ctx, "undo <root>")?;
    out(&to_json_compact(&undo_edit(&root)?.to_json()));
    Ok(0)
}

/// `__debug system-page <root> [--base <b>]`: `renderSystemPage(root, { designBase })` as it is
/// (the page ends in a newline; none is added).
pub(super) fn debug_system_page(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = root_of(ctx, "system-page <root> [--base <b>]")?;
    let page = render_system_page(&root, ctx.args.text("base").unwrap_or("../"))?;
    out(page.strip_suffix('\n').unwrap_or(&page));
    Ok(0)
}

/// `__debug changed-screens <root>`: `JSON.stringify(changedScreens(root))`.
pub(super) fn debug_changed_screens(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = root_of(ctx, "changed-screens <root>")?;
    let changed = changed_screens(&root)?;
    out(&to_json_compact(&json!({
        (changed_key::SCREENS): changed.screens,
        (changed_key::CHANGED_COMPS): changed.changed_comps,
        (changed_key::TOKENS_CHANGED): changed.tokens_changed,
    })));
    Ok(0)
}

/// `__debug mark-checked <root>`: `markChecked(root)`; prints nothing.
pub(super) fn debug_mark_checked(ctx: &Ctx) -> Result<i32, DfwError> {
    mark_checked(&root_of(ctx, "mark-checked <root>")?)?;
    Ok(0)
}
