//! Ports `postToolUse` of `core/lib/hooks.mjs`: after an edit, checks on the files it touched
//! and, for hosts without prompt context, new Studio comments.
//!
//! Native here: the decision whether the edit needs checks, and the comment delivery when it
//! does not. The checks themselves (token and component rebuild, lint: the design group) are
//! not ported, so an edit that needs them is handed to the Node CLI as a whole.

use serde_json::Value;

use crate::core::config::{checks_on_edit, load_config, OnEdit};
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult, Host};
use crate::core::paths::resolve;
use crate::workflow::gates::relative;
use crate::workflow::state::load_state;

use super::pre_tool_use::{root_from, tool_files};
use super::user_prompt_submit::{delivers_on_prompt, inbox_for};

/// What a handler that is only partly ported decided.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome<T = HookResult> {
    /// Handled here: the result to print (or nothing).
    Done(Option<T>),
    /// Needs code of a group that is not ported: the Node CLI runs this hook call.
    Forward,
}

/// `UI_EXT` of check.mjs: the files the checks look at.
// TODO(merge): use tools::check::UI_EXT
const UI_EXT: [&str; 11] = [
    ".html", ".htm", ".css", ".scss", ".sass", ".less", ".jsx", ".tsx", ".vue", ".svelte", ".astro",
];

/// `path.extname`: from the last dot of the last segment, unless the name starts with it.
fn extname(rel: &str) -> &str {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.rfind('.') {
        Some(i) if i > 0 => &name[i..],
        _ => "",
    }
}

/// The first part of `postToolUseChecks`: the project-relative files of this edit that the
/// checks would run on. Empty when nothing needs checking.
pub fn check_targets(input: &HookInput) -> Result<Vec<String>, DfwError> {
    let files = tool_files(input)?;
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(Vec::new());
    };
    if state.get("active") == Some(&Value::Bool(false)) {
        return Ok(Vec::new());
    }
    let config = load_config(Some(&root))?;
    if checks_on_edit(&config) == OnEdit::Off {
        return Ok(Vec::new());
    }
    let root = resolve(&root);
    let design_json = |rel: &str| {
        rel.starts_with("design/")
            && rel.ends_with(".json")
            && rel.len() >= "design/.json".len()
            && !rel.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
    };
    Ok(files
        .iter()
        .map(|f| relative(&root, f))
        .filter(|rel| {
            !rel.starts_with("..")
                && !rel.starts_with("design/approved/")
                && (UI_EXT.contains(&extname(rel)) || design_json(rel))
        })
        .collect())
}

/// `postToolUse(input, host)`.
pub fn post_tool_use(input: &HookInput, host: &Host) -> Result<Outcome, DfwError> {
    let targets = check_targets(input);
    if matches!(&targets, Ok(t) if !t.is_empty()) {
        // Node delivers the comments and runs the checks in one call; nothing was delivered yet.
        return Ok(Outcome::Forward);
    }
    // Hosts that cannot add context to the user's prompt get new Studio comments here instead.
    // Node reads them before it looks at the edit, so they are delivered even when that fails.
    let inbox = if delivers_on_prompt(host) {
        None
    } else {
        inbox_for(input)?
    };
    targets?;
    Ok(Outcome::Done(inbox.map(HookResult::Context)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extname_is_the_node_extname() {
        assert_eq!(extname("design/screens/a/index.html"), ".html");
        assert_eq!(extname("src/App.test.tsx"), ".tsx");
        assert_eq!(extname("src/.env"), "");
        assert_eq!(extname("a.b/readme"), "");
        assert_eq!(extname("notes."), ".");
        assert_eq!(extname(""), "");
    }
}
