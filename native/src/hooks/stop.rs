//! Ports `stop` of `core/lib/hooks.mjs`: before the agent stops, (1) deliver comments the user
//! posted meanwhile; (2) re-render every screen whose file, masters or tokens changed and block
//! once on layout MUST findings, so edits never leave the design broken. Fails open.
//!
//! Native here: (1), and the cheap part of the decision whether (2) applies (loop guard, phase,
//! `checks.layoutOnStop`). The layout check itself needs the browser and design groups, which
//! are not ported, so a stop that reaches it is handed to the Node CLI.

use serde_json::Value;

use crate::core::config::{checks_layout_on_stop, load_config};
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult};
use crate::workflow::state::load_state;

use super::post_tool_use::Outcome;
use super::pre_tool_use::root_from;
use super::user_prompt_submit::inbox_for;

/// `stop(input)`.
pub fn stop(input: &HookInput) -> Result<Outcome, DfwError> {
    if input.stop_hook_active {
        return Ok(Outcome::Done(None)); // never loop
    }
    if let Some(text) = inbox_for(input)? {
        return Ok(Outcome::Done(Some(HookResult::Block(text))));
    }
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(Outcome::Done(None));
    };
    let designing = matches!(
        state.get("phase").and_then(Value::as_str),
        Some("system" | "concepts")
    );
    if state.get("active") == Some(&Value::Bool(false)) || !designing {
        return Ok(Outcome::Done(None));
    }
    if !checks_layout_on_stop(&load_config(Some(&root))?) {
        return Ok(Outcome::Done(None));
    }
    // From here Node asks whether a browser is installed and which screens changed.
    Ok(Outcome::Forward)
}
