//! Ports `sessionStart` of `core/lib/hooks.mjs`: remember the session id, prune old language
//! sessions and tell the agent where the workflow stands.

use std::fs::OpenOptions;
use std::io::Write;

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult};
use crate::core::js;
use crate::core::paths::env_nonempty;
use crate::review::comments::all_comments;
use crate::workflow::lang::prune_sessions;
use crate::workflow::state::{load_state, summarize};

use super::pre_tool_use::root_from;

/// `sessionStart(input)`: the status lines as context; nothing when there is no active workflow.
pub fn session_start(input: &HookInput) -> Result<Option<HookResult>, DfwError> {
    let session_id =
        js::truthy_value(input.raw.get("session_id")).map(|id| js::to_string(Some(id)));
    // Claude Code only: persist the session id for later Bash calls (dfw lang).
    if let (Some(id), Some(env_file)) = (&session_id, env_nonempty("CLAUDE_ENV_FILE")) {
        let line = format!("export DFW_SESSION_ID='{}'\n", id.replace('\'', ""));
        // A file that cannot be written is not worth failing the hook for.
        let _ = OpenOptions::new()
            .create(true)
            .append(true)
            .open(env_file)
            .and_then(|mut f| f.write_all(line.as_bytes()));
    }
    prune_sessions()?;
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    if state.get("active") == Some(&Value::Bool(false)) {
        return Ok(None);
    }
    let comments = all_comments(&root)?;
    let count = |statuses: &[&str]| {
        comments
            .iter()
            .filter(|c| {
                c.get("status")
                    .and_then(Value::as_str)
                    .is_some_and(|s| statuses.contains(&s))
            })
            .count()
    };
    let lines = [
        summarize(Some(&state))?,
        format!(
            "review comments: {} open, {} awaiting user verification",
            count(&["open", "reopened", "acknowledged"]),
            count(&["fixed"])
        ),
        format!(
            "Session id for \"dfw lang\": {}.",
            session_id.as_deref().unwrap_or("unknown")
        ),
        "This project uses the dfw design-first workflow. Before working on its UI, invoke the dfw skill (it asks the communication language once per session and enforces approval gates).".to_string(),
    ];
    Ok(Some(HookResult::Context(lines.join("\n"))))
}
