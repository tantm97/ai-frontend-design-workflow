//! Ports the live comment loop of `core/lib/penpot.mjs`: thread links, `continueThread`,
//! `threadReplies`, `markPosted`, `exportStale` and `syncThreads`.
//!
//! Penpot comment threads are linked to dfw comments in `.dfw/penpot/threads.json`:
//! `{ "<file id>#<thread number>": { comment: "fb_012", seen: <designer messages read>,
//! resolved, posted: [reply keys] } }`. The plugin sends the file's threads every few seconds;
//! new threads become dfw comments, linked threads continue theirs, and the answer carries
//! dfw's replies for the plugin to post in the threads.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};

use super::diff::list;
use super::export::{penpot_dir, CHANGES_SCHEMA};
use super::import::{import_changes, ImportOptions};
use super::tokens::jv;
use crate::core::error::DfwError;
use crate::core::paths::project_paths;
use crate::core::util::{list_files, read_json, write_json};
use crate::design::tokens::js;
use crate::review::comments::{all_comments, update_comment, Update};

/// `.dfw/penpot/threads.json`.
pub(super) fn threads_file(root: &Path) -> PathBuf {
    penpot_dir(root).join("threads.json")
}

/// The key of a thread: the Penpot file's id and the thread's number in it.
pub(super) fn thread_key(file_id: Option<&Value>, seq: Option<&Value>) -> String {
    let file_id = jv::truthy_value(file_id).map_or(String::new(), |f| jv::text(Some(f)));
    format!("{file_id}#{}", jv::text(seq))
}

/// `continueThread`: a linked thread changed in Penpot. New designer messages are added to the
/// dfw comment (and reopen it when it was fixed, resolved or dismissed); resolving the thread
/// accepts a fix, or dismisses a comment that was not fixed yet. These are the user's own
/// actions, like the Studio buttons.
pub(super) fn continue_thread(
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
        .find(|c| jv::strict_eq(c.get("id"), link.get("comment")))
    else {
        // The dfw comment is gone: the thread is imported as new next time.
        if let Value::Object(links) = threads {
            links.shift_remove(tk);
        }
        return Ok(());
    };
    let messages = list(c0.get("messages"));
    let seen = jv::truthy_value(link.get("seen")).map_or(0.0, |s| jv::num(Some(s)));
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
    let status = cur.get("status").and_then(Value::as_str);
    let done = matches!(status, Some("fixed" | "resolved" | "dismissed"));
    let (thread_resolved, link_resolved) = (
        jv::truthy(c0.get("resolved")),
        jv::truthy(link.get("resolved")),
    );
    let or = |fallback: &str| {
        if text.is_empty() {
            fallback.to_string()
        } else {
            text.clone()
        }
    };
    let (to, message) = if thread_resolved && !link_resolved {
        if status == Some("fixed") {
            (Some("resolved"), or("Resolved in Penpot."))
        } else if !done {
            (
                Some("dismissed"),
                or("Thread resolved in Penpot before a fix."),
            )
        } else {
            (None, text.clone())
        }
    } else if !thread_resolved && (!fresh.is_empty() || link_resolved) && done {
        (Some("reopened"), or("Thread reopened in Penpot."))
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
            Ok(c) => updated.push(jv::obj(vec![
                ("id", c.get("id").cloned()),
                ("status", c.get("status").cloned()),
            ])),
            Err(err) => warnings.push(format!(
                "Thread #{} ({}): {}",
                jv::text(c0.get("thread")),
                jv::text(cur.get("id")),
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

/// `threadReplies`: what dfw did with a linked comment, as replies for its Penpot thread:
/// `[{ seq, key, text }]` not posted yet.
pub fn thread_replies(root: &Path, file_id: &str) -> Result<Vec<Value>, DfwError> {
    let threads = read_json(&threads_file(root), Some(json!({})))?;
    let comments = all_comments(root)?;
    let prefix = format!("{file_id}#");
    let mut out = Vec::new();
    let links = threads.as_object().map(js::entries).unwrap_or_default();
    for (tk, link) in links {
        let Some(number) = tk.strip_prefix(&prefix) else {
            continue;
        };
        // The last comment with an id wins, as in a JavaScript Map.
        let Some(c) = comments
            .iter()
            .rev()
            .find(|c| jv::strict_eq(c.get("id"), link.get("comment")))
        else {
            continue;
        };
        let posted = list(jv::truthy_value(link.get("posted")));
        let seq = js::num(js::str_to_number(number));
        let id = jv::text(c.get("id"));
        let mut events: Vec<(String, String)> = Vec::new();
        if c.get("status").and_then(Value::as_str) == Some("acknowledged") {
            events.push((
                format!("ack:{}", jv::text(c.get("updatedAt"))),
                format!("[dfw] {id} seen, working on it."),
            ));
        }
        for e in list(jv::truthy_value(c.get("thread"))) {
            let message = jv::truthy_value(e.get("message"));
            let (Some(message), false) =
                (message, e.get("by").and_then(Value::as_str) == Some("user"))
            else {
                continue;
            };
            let message = jv::text(Some(message));
            let text = match e.get("status").and_then(Value::as_str) {
                Some("fixed") => format!("[dfw] {id} fixed: {message}\nUpdate the boards to see it. Resolve this thread to accept, or reply here to reopen."),
                Some("dismissed") => format!("[dfw] {id} dismissed: {message}\nReply here to reopen."),
                _ => format!("[dfw] {id}: {message}"),
            };
            let status = jv::truthy_value(e.get("status"))
                .map_or("reply".to_string(), |s| jv::text(Some(s)));
            events.push((format!("{status}:{}", jv::text(e.get("at"))), text));
        }
        for (key, text) in events {
            if !posted.iter().any(|p| p.as_str() == Some(&key)) {
                out.push(json!({ "seq": seq, "key": key, "text": text }));
            }
        }
    }
    Ok(out)
}

/// `markPosted`: the plugin posted these replies (`[{ seq, key }]`): do not send them again.
pub fn mark_posted(root: &Path, file_id: &str, posted: Option<&Value>) -> Result<(), DfwError> {
    let posted = list(posted);
    if posted.is_empty() {
        return Ok(());
    }
    let mut threads = read_json(&threads_file(root), Some(json!({})))?;
    let mut changed = false;
    for p in posted {
        let tk = thread_key(Some(&file_id.into()), p.get("seq"));
        let Some(link) = threads.get_mut(&tk).filter(|l| jv::truthy(Some(l))) else {
            continue;
        };
        let (Some(key), Value::Object(link)) = (p.get("key").and_then(Value::as_str), link) else {
            continue;
        };
        let slot = link.entry("posted").or_insert(Value::Null);
        if !jv::truthy(Some(slot)) {
            *slot = json!([]);
        }
        let Value::Array(list) = slot else {
            continue;
        };
        if list.iter().any(|k| k.as_str() == Some(key)) {
            continue;
        }
        list.push(key.into());
        let extra = list.len().saturating_sub(200);
        list.drain(..extra);
        changed = true;
    }
    if changed {
        write_json(&threads_file(root), &threads)?;
    }
    Ok(())
}

/// `Date.parse` for the timestamps dfw writes (`2026-11-16T10:00:00.000Z`, also without the
/// time or its fraction, or with a `+hh:mm` offset): milliseconds since 1970, `None` otherwise.
fn parse_iso_ms(text: &str) -> Option<f64> {
    let digits = |s: &str, len: usize| -> Option<i64> {
        (s.len() == len && s.bytes().all(|b| b.is_ascii_digit()))
            .then(|| s.parse().ok())
            .flatten()
    };
    let (date, time) = match text.split_once('T') {
        Some((d, t)) => (d, Some(t)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let (y, m, d) = (
        digits(parts.next()?, 4)?,
        digits(parts.next()?, 2)?,
        digits(parts.next()?, 2)?,
    );
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // Days since 1970-01-01 from a civil date (Howard Hinnant's algorithm).
    let y_adj = if m <= 2 { y - 1 } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let mut ms = days as f64 * 86_400_000.0;
    let Some(time) = time else {
        return Some(ms);
    };
    let (clock, offset) = match time.find(['Z', '+', '-']) {
        Some(i) => (&time[..i], &time[i..]),
        None => (time, "Z"),
    };
    let (hms, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut fields = hms.split(':');
    let (h, min) = (digits(fields.next()?, 2)?, digits(fields.next()?, 2)?);
    let s = match fields.next() {
        Some(s) => digits(s, 2)?,
        None => 0,
    };
    if fields.next().is_some() || h > 24 || min > 59 || s > 59 {
        return None;
    }
    ms += (h * 3_600_000 + min * 60_000 + s * 1000) as f64;
    if !fraction.is_empty() {
        if !fraction.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        // Milliseconds: the first three digits, as JavaScript keeps them.
        let millis: String = fraction.chars().chain("000".chars()).take(3).collect();
        ms += millis.parse::<f64>().ok()?;
    }
    if offset != "Z" {
        let sign = if offset.starts_with('-') { 1.0 } else { -1.0 };
        let (oh, om) = offset[1..].split_once(':')?;
        ms += sign * (digits(oh, 2)? * 3_600_000 + digits(om, 2)? * 60_000) as f64;
    }
    Some(ms)
}

/// `exportStale`: true when a design file changed after the latest export was rendered (the
/// Penpot boards are behind).
pub fn export_stale(root: &Path) -> Result<bool, DfwError> {
    let doc = read_json(&penpot_dir(root).join("export.json"), Some(Value::Null))?;
    let generated_at = jv::truthy_value(doc.get("generatedAt")).and_then(Value::as_str);
    let Some(at) = generated_at.and_then(parse_iso_ms).filter(|at| *at != 0.0) else {
        return Ok(false);
    };
    let design = project_paths(root).design;
    for rel in list_files(&design, &["approved", "handoff"])? {
        let modified = fs::metadata(design.join(&rel)).and_then(|m| m.modified());
        let Ok(modified) = modified else {
            continue;
        };
        let ms = match modified.duration_since(UNIX_EPOCH) {
            Ok(after) => after.as_secs_f64() * 1000.0,
            Err(before) => -before.duration().as_secs_f64() * 1000.0,
        };
        if ms > at {
            return Ok(true);
        }
    }
    Ok(false)
}

/// `syncThreads`: the live comment loop with the Penpot plugin (the same loop as the Studio,
/// without pressing "Send changes"). `payload`: `{ fileId, exportId, threads: [...], posted:
/// [{ seq, key }] }`. Returns `{ comments: [ids], updated, replies, stale, warnings }`.
pub fn sync_threads(
    root: &Path,
    payload: &Value,
    state: Option<&Value>,
) -> Result<Value, DfwError> {
    let file_id = payload
        .get("fileId")
        .and_then(Value::as_str)
        .map_or(String::new(), |id| jv::slice16(id, 0, 80));
    mark_posted(root, &file_id, payload.get("posted"))?;
    let threads: Vec<Value> = list(payload.get("threads"))
        .iter()
        .take(500)
        .cloned()
        .collect();
    let mut changes = jv::obj(vec![
        ("schema", Some(CHANGES_SCHEMA.into())),
        ("exportId", payload.get("exportId").cloned()),
        ("fileId", Some(file_id.as_str().into())),
        ("comments", Some(Value::Array(threads))),
        ("boards", Some(json!([]))),
        ("components", Some(json!([]))),
    ]);
    let r = import_changes(
        root,
        &mut changes,
        ImportOptions {
            dry_run: false,
            state,
            live: true,
        },
    )?;
    let ids: Vec<Value> = r
        .comments
        .iter()
        .map(|c| c.get("id").cloned().unwrap_or(Value::Null))
        .collect();
    Ok(json!({
        "comments": ids,
        "updated": r.updated,
        "replies": thread_replies(root, &file_id)?,
        "stale": export_stale(root)?,
        "warnings": r.warnings,
    }))
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
