//! The link between a Penpot comment thread and a dfw review comment (`.dfw/penpot/threads.json`):
//! its key, its file, and what a change in the thread does to the comment. Used by the import of a
//! Penpot export and by the live sync, which also imports, so it lives apart from both.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::diff::list;
use super::export::penpot_dir;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::file;
use crate::review::comments::{all_comments, update_comment, Update};
use crate::review::status::{self, Status};

/// `.dfw/penpot/threads.json`.
pub(crate) fn threads_file(root: &Path) -> PathBuf {
    penpot_dir(root).join(file::PENPOT_THREADS)
}

/// The key of a thread: the Penpot file's id and the thread's number in it.
pub(crate) fn thread_key(file_id: Option<&Value>, seq: Option<&Value>) -> String {
    let file_id = js::truthy_value(file_id).map_or(String::new(), |f| js::to_string(Some(f)));
    format!("{file_id}#{}", js::to_string(seq))
}

/// `continueThread`: a linked thread changed in Penpot. New designer messages are added to the
/// dfw comment (and reopen it when it was fixed, resolved or dismissed); resolving the thread
/// accepts a fix, or dismisses a comment that was not fixed yet. These are the user's own
/// actions, like the Studio buttons.
pub(crate) fn continue_thread(
    root: &Path,
    threads: &mut Value,
    tk: &str,
    c0: &Value,
    updated: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) -> Result<(), DfwError> {
    let link = threads.get(tk).cloned().unwrap_or(Value::Null);
    let comments = all_comments(root)?;
    let Some(cur) = comments
        .iter()
        .find(|c| js::strict_eq(c.get("id"), link.get("comment")))
    else {
        // The dfw comment is gone: the thread is imported as new next time.
        if let Value::Object(links) = threads {
            links.shift_remove(tk);
        }
        return Ok(());
    };
    let messages = list(c0.get("messages"));
    let seen = js::truthy_value(link.get("seen")).map_or(0.0, |s| js::to_number(Some(s)));
    let from = if seen.is_nan() {
        0
    } else if seen < 0.0 {
        messages.len().saturating_sub((-seen) as usize)
    } else {
        (seen as usize).min(messages.len())
    };
    let fresh = &messages[from..];
    let contents: Vec<Value> = fresh
        .iter()
        .map(|m| m.get("content").cloned().unwrap_or(Value::Null))
        .collect();
    let text = js::join(&contents, "\n");
    let status = status::of(cur);
    let done = matches!(
        status,
        Some(Status::Fixed | Status::Resolved | Status::Dismissed)
    );
    let (thread_resolved, link_resolved) = (
        js::truthy(c0.get("resolved")),
        js::truthy(link.get("resolved")),
    );
    let or = |fallback: &str| {
        if text.is_empty() {
            fallback.to_string()
        } else {
            text.clone()
        }
    };
    let (to, message) = if thread_resolved && !link_resolved {
        if status == Some(Status::Fixed) {
            (Some(Status::Resolved.as_str()), or("Resolved in Penpot."))
        } else if !done {
            (
                Some(Status::Dismissed.as_str()),
                or("Thread resolved in Penpot before a fix."),
            )
        } else {
            (None, text.clone())
        }
    } else if ((!thread_resolved && (!fresh.is_empty() || link_resolved))
        || (!fresh.is_empty() && link_resolved))
        && done
    {
        // A reply in a thread that stays resolved reopens the comment too: otherwise the agent
        // never sees it.
        (
            Some(Status::Reopened.as_str()),
            or("Thread reopened in Penpot."),
        )
    } else {
        (None, text.clone())
    };
    if to.is_some() || !message.is_empty() {
        let id = cur.get("id").and_then(Value::as_str);
        let update = Update {
            status: to.map(str::to_string),
            message: Some(message.into()),
            by: Some("user".into()),
            evidence: None,
        };
        match update_comment(root, id, &update) {
            Ok(c) => updated.push(js::object(vec![
                ("id", c.get("id").cloned()),
                ("status", c.get("status").cloned()),
            ])),
            Err(err) => warnings.push(format!(
                "Thread #{} ({}): {}",
                js::to_string(c0.get("thread")),
                js::to_string(cur.get("id")),
                err.message()
            )),
        }
    }
    if let Some(Value::Object(link)) = threads.get_mut(tk) {
        link.insert("seen".into(), messages.len().into());
        link.insert("resolved".into(), thread_resolved.into());
    }
    Ok(())
}
