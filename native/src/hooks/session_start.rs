//! Ports `sessionStart` of `core/lib/hooks.mjs`: remember the session id, prune old language
//! sessions and tell the agent where the workflow stands.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult, Host};
use crate::core::js;
use crate::core::paths::env_nonempty;
use crate::design::pages::system_link;
use crate::review::comments::all_comments;
use crate::review::inbox::{in_background, is_listening, WAIT_COMMAND};
use crate::review::status::{self, Status};
use crate::workflow::lang::prune_sessions;
use crate::workflow::server::running_studio;
use crate::workflow::state::{field, load_state, summarize};

use super::pre_tool_use::root_from;
use crate::core::host::input as payload;

/// `sessionStart(input)`: the status lines as context; nothing when there is no active workflow.
pub fn session_start(input: &HookInput, host: &Host) -> Result<Option<HookResult>, DfwError> {
    let session_id =
        js::truthy_value(input.raw.get(payload::SESSION_ID)).map(|id| js::to_string(Some(id)));
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
    if state.get(field::ACTIVE) == Some(&Value::Bool(false)) {
        return Ok(None);
    }
    let comments = all_comments(&root)?;
    let count = |wanted: fn(Status) -> bool| {
        comments
            .iter()
            .filter(|c| status::of(c).is_some_and(wanted))
            .count()
    };
    let mut lines = vec![
        summarize(Some(&state))?,
        format!(
            "review comments: {} open, {} awaiting user verification",
            count(Status::is_active),
            count(|s| s == Status::Fixed)
        ),
    ];
    lines.extend(studio_lines(&root, host)?);
    lines.extend([
        format!(
            "Session id for \"dfw lang\": {}.",
            session_id.as_deref().unwrap_or("unknown")
        ),
        "This project uses the dfw design-first workflow. Before working on its UI, invoke the dfw skill (it asks the communication language once per session and enforces approval gates).".to_string(),
    ]);
    Ok(Some(HookResult::Context(lines.join("\n"))))
}

/// With a running Studio: its links and whether a wait listens, and, when none does, how to
/// keep listening during review. Nothing without a Studio.
fn studio_lines(root: &Path, host: &Host) -> Result<Vec<String>, DfwError> {
    let Some(url) = running_studio(root)? else {
        return Ok(Vec::new());
    };
    let listening = is_listening(root);
    let mut lines = vec![format!(
        "studio: {url} (design system: {}), listening: {}",
        system_link(&url),
        if listening { "yes" } else { "no" }
    )];
    if !listening {
        lines.push(format!(
            "While the user reviews in the Studio, keep \"{WAIT_COMMAND}\" running {} so \"Send to AI\" wakes you, and write the Studio link in your reply whenever you ask the user to look at something there.",
            in_background(host.background())
        ));
    }
    Ok(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::host::{event, get_host, normalize_hook_input};
    use crate::core::paths::project_paths;
    use crate::core::util::test_dir;
    use crate::workflow::server::field as record;
    use crate::workflow::state::{new_state, save_state};
    use serde_json::json;

    fn context(root: &Path) -> String {
        let input = normalize_hook_input(&json!({
            (payload::HOOK_EVENT_NAME): event::SESSION_START,
            (payload::CWD): root.to_string_lossy(),
        }))
        .unwrap();
        match session_start(&input, get_host(None).unwrap()).unwrap() {
            Some(HookResult::Context(text)) => text,
            other => panic!("expected context, got {other:?}"),
        }
    }

    /// A running Studio is named with its Design System link and the listener; none, no line.
    #[test]
    fn session_start_names_the_studio_and_the_listener() {
        let root = test_dir("session-studio");
        save_state(&root, &mut new_state("new-product", "".into()).unwrap()).unwrap();
        assert!(!context(&root).contains("studio:"), "no Studio, no line");
        let url = "http://127.0.0.1:4601/";
        let studio = json!({ (record::PID): std::process::id(), (record::URL): url });
        std::fs::write(project_paths(&root).studio, studio.to_string()).unwrap();
        let text = context(&root);
        assert!(
            text.contains("studio: http://127.0.0.1:4601/ (design system: http://127.0.0.1:4601/?page=system), listening: no"),
            "{text}"
        );
        assert!(text.contains(WAIT_COMMAND), "{text}");
    }
}
