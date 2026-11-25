//! Ports the live comment loop of `core/lib/penpot.mjs`: thread links, `continueThread`,
//! `threadReplies`, `markPosted`, `exportStale` and `syncThreads`.
//!
//! Penpot comment threads are linked to dfw comments in `.dfw/penpot/threads.json`:
//! `{ "<file id>#<thread number>": { comment: "fb_012", seen: <designer messages read>,
//! resolved, posted: [reply keys] } }`. The plugin sends the file's threads every few seconds;
//! new threads become dfw comments, linked threads continue theirs, and the answer carries
//! dfw's replies for the plugin to post in the threads.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde_json::{json, Value};

use super::diff::list;
use super::export::{penpot_dir, CHANGES_SCHEMA};
use super::import::{import_changes, ImportOptions};
use super::threads::{thread_key, threads_file};
use super::wire;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, project_paths};
use crate::core::util::{list_files, read_json, write_json_inside};

use crate::review::comments::all_comments;
use crate::review::status::{self, Status};

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
            .find(|c| js::strict_eq(c.get(status::field::ID), link.get(wire::link::COMMENT)))
        else {
            continue;
        };
        let posted = list(js::truthy_value(link.get(wire::link::POSTED)));
        let seq = js::num(js::str_to_number(number));
        let id = js::to_string(c.get(status::field::ID));
        let mut events: Vec<(String, String)> = Vec::new();
        if status::of(c) == Some(Status::Acknowledged) {
            events.push((
                // One "seen" per round: keyed by how often the comment was reopened, not by
                // its last change (a reply would post it again).
                format!(
                    "ack:{}",
                    list(c.get(status::field::THREAD))
                        .iter()
                        .filter(|e| status::of(e) == Some(Status::Reopened))
                        .count()
                ),
                format!("[dfw] {id} seen, working on it."),
            ));
        }
        for e in list(js::truthy_value(c.get(status::field::THREAD))) {
            let message = js::truthy_value(e.get(wire::line::MESSAGE));
            let (Some(message), false) = (
                message,
                e.get(wire::line::BY).and_then(Value::as_str) == Some("user"),
            ) else {
                continue;
            };
            let message = js::to_string(Some(message));
            let text = match status::of(e) {
                Some(Status::Fixed) => format!("[dfw] {id} fixed: {message}\nUpdate the boards to see it. Resolve this thread to accept, or reply here to reopen."),
                Some(Status::Dismissed) => format!("[dfw] {id} dismissed: {message}\nReply here to reopen."),
                _ => format!("[dfw] {id}: {message}"),
            };
            let status = js::truthy_value(e.get(wire::line::STATUS))
                .map_or("reply".to_string(), |s| js::to_string(Some(s)));
            events.push((
                format!("{status}:{}", js::to_string(e.get(wire::line::AT))),
                text,
            ));
        }
        for (key, text) in events {
            if !posted.iter().any(|p| p.as_str() == Some(&key)) {
                out.push(json!({ (wire::SEQ): seq, (wire::KEY): key, (wire::TEXT): text }));
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
        let tk = thread_key(Some(&file_id.into()), p.get(wire::SEQ));
        let Some(link) = threads.get_mut(&tk).filter(|l| js::truthy(Some(l))) else {
            continue;
        };
        let (Some(key), Value::Object(link)) = (p.get(wire::KEY).and_then(Value::as_str), link)
        else {
            continue;
        };
        let slot = link.entry(wire::link::POSTED).or_insert(Value::Null);
        if !js::truthy(&*slot) {
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
        write_json_inside(root, &threads_file(root), &threads)?;
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
    let doc = read_json(
        &penpot_dir(root).join(file::PENPOT_EXPORT),
        Some(Value::Null),
    )?;
    let generated_at = js::truthy_value(doc.get(wire::GENERATED_AT)).and_then(Value::as_str);
    let Some(at) = generated_at.and_then(parse_iso_ms).filter(|at| *at != 0.0) else {
        return Ok(false);
    };
    let design = project_paths(root).design;
    for rel in list_files(&design, &[file::APPROVED, file::HANDOFF])? {
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
        .get(wire::FILE_ID)
        .and_then(Value::as_str)
        .map_or(String::new(), |id| js::slice16(id, 0, 80));
    mark_posted(root, &file_id, payload.get(wire::POSTED))?;
    let threads: Vec<Value> = list(payload.get(wire::THREADS))
        .iter()
        .take(500)
        .cloned()
        .collect();
    let mut changes = js::object(vec![
        (wire::SCHEMA, Some(CHANGES_SCHEMA.into())),
        (wire::EXPORT_ID, payload.get(wire::EXPORT_ID).cloned()),
        (wire::FILE_ID, Some(file_id.as_str().into())),
        (wire::COMMENTS, Some(Value::Array(threads))),
        (wire::BOARDS, Some(json!([]))),
        (wire::COMPONENTS, Some(json!([]))),
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
        .map(|c| c.get(status::field::ID).cloned().unwrap_or(Value::Null))
        .collect();
    Ok(json!({
        wire::COMMENTS: ids,
        wire::UPDATED: r.updated,
        wire::REPLIES: thread_replies(root, &file_id)?,
        (wire::report::STALE): export_stale(root)?,
        (wire::report::WARNINGS): r.warnings,
    }))
}

#[cfg(test)]
#[path = "live_tests.rs"]
mod tests;
