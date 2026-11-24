//! Ports `userPromptSubmit` and `inboxFor` of `core/lib/hooks.mjs`: new Studio comments ride
//! along with the user's next message.

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::host::{event, HookInput, HookResult, Host};
use crate::review::inbox::{format_for_claude, take_undelivered};
use crate::workflow::state::load_state;

use super::pre_tool_use::root_from;

/// `inboxFor(input)`: the undelivered comments of the project as text for the agent, marked
/// delivered in the same hold of the reviews lock (two hooks at once never deliver one comment
/// twice); `None` when there are none, the lock is busy, or the workflow is not active.
pub fn inbox_for(input: &HookInput) -> Result<Option<String>, DfwError> {
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    if state.get("active") == Some(&Value::Bool(false)) {
        return Ok(None);
    }
    let list = take_undelivered(&root, true)?;
    if list.is_empty() {
        return Ok(None);
    }
    Ok(Some(format_for_claude(&list)))
}

/// A host that cannot add context to the user's prompt gets new comments on PostToolUse and
/// Stop instead.
pub fn delivers_on_prompt(host: &Host) -> bool {
    host.context(event::USER_PROMPT_SUBMIT) != Some(false)
}

/// `userPromptSubmit(input, host)`: undelivered comments as context.
pub fn user_prompt_submit(input: &HookInput, host: &Host) -> Result<Option<HookResult>, DfwError> {
    if !delivers_on_prompt(host) {
        return Ok(None);
    }
    Ok(inbox_for(input)?.map(HookResult::Context))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::host::normalize_hook_input;
    use crate::core::util::test_dir;
    use crate::review::comments::add_comment;
    use crate::workflow::state::{new_state, save_state};
    use serde_json::json;

    /// Eight prompt hooks at once give each comment to the agent once (review CO-5: the hook
    /// read and marked in two steps, so two hooks could both deliver one comment).
    #[test]
    fn parallel_prompt_hooks_deliver_each_comment_once() {
        let root = test_dir("prompt-hook-parallel");
        save_state(&root, &mut new_state("new-product", "".into()).unwrap()).unwrap();
        let marker = |i: usize| format!("marker-{i}-end");
        for i in 0..5 {
            add_comment(&root, &json!({ "comment": marker(i) })).unwrap();
        }
        let input = normalize_hook_input(&json!({
            "hook_event_name": event::USER_PROMPT_SUBMIT,
            "cwd": root.to_string_lossy(),
            "prompt": "go",
        }))
        .unwrap();
        let mut told: Vec<String> = std::thread::scope(|s| {
            let hooks: Vec<_> = (0..8)
                .map(|_| s.spawn(|| inbox_for(&input).unwrap()))
                .collect();
            hooks
                .into_iter()
                .filter_map(|h| h.join().unwrap())
                .collect()
        });
        // A hook that found the lock busy left its comments for the next prompt.
        while let Some(text) = inbox_for(&input).unwrap() {
            told.push(text);
        }
        let all = told.join("\n");
        for i in 0..5 {
            assert_eq!(all.matches(&marker(i)).count(), 1, "{all}");
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}
