//! The link between a Penpot comment thread and a dfw review comment (`.dfw/penpot/threads.json`):
//! its key, its file, and what a change in the thread does to the comment. Used by the import of a
//! Penpot export and by the live sync, which also imports, so it lives apart from both.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::diff::list;
use super::export::penpot_dir;
use super::wire;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::lock;
use crate::core::paths::file;
use crate::core::util::write_json_inside;
use crate::review::comments::{all_comments, update_comment, Update};
use crate::review::status::{self, Status};

/// `.dfw/penpot/threads.json`.
pub(crate) fn threads_file(root: &Path) -> PathBuf {
    penpot_dir(root).join(file::PENPOT_THREADS)
}

/// Proof that the Penpot ledger lock (`.dfw/penpot/.lock`, a `core::lock`) is held: made only
/// by [`locked`] and asked for by [`write_ledger`], so a write of `threads.json` or
/// `imported.json` outside the lock does not compile. The Studio serves one thread per
/// connection, and the plugin's sync and "Send changes" read, change and write these files at
/// the same time (`process-and-io.md` P7).
pub(crate) struct Held {
    /// Released when the guard is dropped.
    _lock: lock::Lock,
}

/// Run `f` holding the Penpot ledger lock.
pub(crate) fn locked<T>(
    root: &Path,
    f: impl FnOnce(&Held) -> Result<T, DfwError>,
) -> Result<T, DfwError> {
    let held = Held {
        _lock: lock::acquire(&penpot_dir(root).join(file::PENPOT_LOCK), &lock::STANDARD)?,
    };
    f(&held)
}

/// The one write of a Penpot ledger (`threads.json`, `imported.json`): under the lock.
pub(crate) fn write_ledger(
    _held: &Held,
    root: &Path,
    file: &Path,
    value: &Value,
) -> Result<(), DfwError> {
    write_json_inside(root, file, value)
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
        .find(|c| js::strict_eq(c.get(status::field::ID), link.get(wire::link::COMMENT)))
    else {
        // The dfw comment is gone: the thread is imported as new next time.
        if let Value::Object(links) = threads {
            links.shift_remove(tk);
        }
        return Ok(());
    };
    let messages = list(c0.get(wire::MESSAGES));
    let seen = js::truthy_value(link.get(wire::link::SEEN)).map_or(0.0, |s| js::to_number(Some(s)));
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
        .map(|m| m.get(wire::CONTENT).cloned().unwrap_or(Value::Null))
        .collect();
    let text = js::join(&contents, "\n");
    let status = status::of(cur);
    let done = matches!(
        status,
        Some(Status::Fixed | Status::Resolved | Status::Dismissed)
    );
    let (thread_resolved, link_resolved) = (
        js::truthy(c0.get(wire::RESOLVED)),
        js::truthy(link.get(wire::link::RESOLVED)),
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
        let id = cur.get(status::field::ID).and_then(Value::as_str);
        let update = Update {
            status: to.map(str::to_string),
            message: Some(message.into()),
            by: Some("user".into()),
            evidence: None,
        };
        match update_comment(root, id, &update) {
            Ok(c) => updated.push(js::object(vec![
                (status::field::ID, c.get(status::field::ID).cloned()),
                (status::field::STATUS, c.get(status::field::STATUS).cloned()),
            ])),
            Err(err) => warnings.push(format!(
                "Thread #{} ({}): {}",
                js::to_string(c0.get(wire::THREAD)),
                js::to_string(cur.get(status::field::ID)),
                err.message()
            )),
        }
    }
    if let Some(Value::Object(link)) = threads.get_mut(tk) {
        link.insert(wire::link::SEEN.into(), messages.len().into());
        link.insert(wire::link::RESOLVED.into(), thread_resolved.into());
    }
    Ok(())
}
