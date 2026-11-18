//! Ports `userPromptSubmit` and `inboxFor` of `core/lib/hooks.mjs`: new Studio comments ride
//! along with the user's next message.

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult, Host};
use crate::review::inbox::{format_for_claude, mark_delivered, undelivered};
use crate::workflow::state::load_state;

use super::pre_tool_use::root_from;

/// `inboxFor(input)`: the undelivered comments of the project as text for the agent, marked
/// delivered; `None` when there are none or the workflow is not active.
pub fn inbox_for(input: &HookInput) -> Result<Option<String>, DfwError> {
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    if state.get("active") == Some(&Value::Bool(false)) {
        return Ok(None);
    }
    let list = undelivered(&root)?;
    if list.is_empty() {
        return Ok(None);
    }
    mark_delivered(&root, &list)?;
    Ok(Some(format_for_claude(&list)))
}

/// A host that cannot add context to the user's prompt gets new comments on PostToolUse and
/// Stop instead.
pub fn delivers_on_prompt(host: &Host) -> bool {
    host.context("UserPromptSubmit") != Some(false)
}

/// `userPromptSubmit(input, host)`: undelivered comments as context.
pub fn user_prompt_submit(input: &HookInput, host: &Host) -> Result<Option<HookResult>, DfwError> {
    if !delivers_on_prompt(host) {
        return Ok(None);
    }
    Ok(inbox_for(input)?.map(HookResult::Context))
}
