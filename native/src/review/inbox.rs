//! Ports `core/lib/inbox.mjs`: delivers Studio review comments to the agent without the user
//! having to relay them:
//! - `dfw comments wait` (run in the background by the agent) exits as soon as a new comment arrives;
//! - the UserPromptSubmit and Stop hooks inject undelivered comments into the conversation.
//!
//! Each comment state is delivered once (`.dfw/reviews/.delivered.json`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::paths::project_paths;
use crate::core::util::{read_json, write_json};
use crate::workflow::state::{js_str, truthy};

use super::comments::all_comments;

/// How many delivery keys are kept.
const MAX_DELIVERED: usize = 2000;

/// What identifies one state of a comment: a change of status or a new thread line is news.
fn key(c: &Value) -> String {
    let at = truthy(c.get("updatedAt")).or(c.get("createdAt"));
    format!(
        "{}:{}:{}",
        js_str(c.get("id")),
        js_str(c.get("status")),
        js_str(at)
    )
}

fn delivered_file(root: &Path) -> PathBuf {
    project_paths(root)
        .dfw
        .join("reviews")
        .join(".delivered.json")
}

fn read_delivered(root: &Path) -> Result<Vec<Value>, DfwError> {
    match read_json(&delivered_file(root), Some(json!([])))? {
        Value::Array(seen) => Ok(seen),
        _ => Err(DfwError::internal(
            ".dfw/reviews/.delivered.json does not hold a list.",
        )),
    }
}

fn last_of_thread(c: &Value) -> Option<&Value> {
    c.get("thread").and_then(Value::as_array)?.last()
}

/// `undelivered(root)`: open and reopened comments, and acknowledged ones the user replied to,
/// whose current state the agent has not been given yet.
pub fn undelivered(root: &Path) -> Result<Vec<Value>, DfwError> {
    let seen = read_delivered(root)?;
    let seen: HashSet<&str> = seen.iter().filter_map(Value::as_str).collect();
    let status = |c: &Value| c.get("status").and_then(Value::as_str).map(str::to_string);
    // A follow-up from the user on a comment being worked on (e.g. a reply in its Penpot thread) is news too.
    let follow_up = |c: &Value| {
        status(c).as_deref() == Some("acknowledged")
            && last_of_thread(c).and_then(|l| l.get("by")) == Some(&json!("user"))
    };
    let active = |c: &Value| matches!(status(c).as_deref(), Some("open" | "reopened"));
    Ok(all_comments(root)?
        .into_iter()
        .filter(|c| (active(c) || follow_up(c)) && !seen.contains(key(c).as_str()))
        .collect())
}

/// `markDelivered(root, comments)`: remember that the agent was given these comment states.
pub fn mark_delivered(root: &Path, comments: &[Value]) -> Result<(), DfwError> {
    if comments.is_empty() {
        return Ok(());
    }
    let mut seen = read_delivered(root)?;
    for c in comments {
        let k = Value::String(key(c));
        if !seen.contains(&k) {
            seen.push(k);
        }
    }
    let skip = seen.len().saturating_sub(MAX_DELIVERED);
    write_json(&delivered_file(root), &Value::Array(seen.split_off(skip)))
}

/// `formatForClaude(comments)`: the comments as the text the agent reads, with what to do next.
pub fn format_for_claude(comments: &[Value]) -> String {
    let lines: Vec<String> = comments
        .iter()
        .map(|c| {
            let place: Vec<String> = ["screen", "variant", "breakpoint", "state", "theme"]
                .iter()
                .filter_map(|k| truthy(c.get(k)))
                .map(|v| js_str(Some(v)))
                .collect();
            let place = place.join(" / ");
            let place = if place.is_empty() {
                "design system"
            } else {
                &place
            };
            let of_target = |k: &str| truthy(c.get("target").and_then(|t| t.get(k)));
            let target = of_target("dfwId")
                .or(of_target("selector"))
                .map_or("?".to_string(), |t| js_str(Some(t)));
            let last = last_of_thread(c);
            let said = |l: &Value| js_str(l.get("message"));
            let reopened = match last {
                Some(l) if c.get("status") == Some(&json!("reopened")) => {
                    format!("\n    reopened by {}: \"{}\"", js_str(l.get("by")), said(l))
                }
                Some(l) if l.get("by") == Some(&json!("user")) => {
                    format!("\n    follow-up by user: \"{}\"", said(l))
                }
                _ => String::new(),
            };
            format!(
                "- {} [{}] {place} -> {target}{}{}\n    \"{}\"{reopened}",
                js_str(c.get("id")),
                js_str(c.get("status")),
                of_target("source").map_or(String::new(), |s| format!(" ({})", js_str(Some(s)))),
                of_target("component")
                    .map_or(String::new(), |s| format!(" component {}", js_str(Some(s)))),
                js_str(c.get("comment")),
            )
        })
        .collect();
    let live = if comments
        .iter()
        .any(|c| c.get("provider") == Some(&json!("dfw-live")))
    {
        "\nComments marked \"live\" are on the running app (dfw review): open target.source (file:line:col) and fix the implementation there; without a source, find the element by its selector and text (phase-verify)."
    } else {
        ""
    };
    format!(
        "{} new Studio comment(s) from the user:\n{}{live}\nApply them now (references/phase-review.md > \"Applying comments\"): \"dfw comments ack <id>\", fix (prefer design-system fixes when the comment is about a component or token), check, then \"dfw comments fixed <id> --message \\\"<what changed>\\\"\". If a comment conflicts with an approved decision or is ambiguous, reply with \"dfw comments reply <id> --message ...\" and ask the user.",
        comments.len(),
        lines.join("\n")
    )
}

/// Timings of [`wait_for_comments`], in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WaitOptions {
    /// Give up after this long. May be negative or infinite, as `Number(--timeout)` can be.
    pub timeout_ms: f64,
    /// Pause between two looks at the store.
    pub poll_ms: u64,
    /// Pause after the first new comment, so a burst arrives in one piece.
    pub settle_ms: u64,
}

impl Default for WaitOptions {
    fn default() -> Self {
        WaitOptions {
            timeout_ms: 3_600_000.0,
            poll_ms: 1000,
            settle_ms: 1500,
        }
    }
}

/// `waitForComments(root, { timeoutMs, pollMs, settleMs })`: block until undelivered comments
/// exist (or the timeout passes). Returns the comments, marked delivered.
pub fn wait_for_comments(root: &Path, options: WaitOptions) -> Result<Vec<Value>, DfwError> {
    let start = Instant::now();
    let before_end = || start.elapsed().as_secs_f64() * 1000.0 < options.timeout_ms;
    while before_end() {
        if !undelivered(root)?.is_empty() {
            // Let the user finish a burst of comments before waking the agent.
            thread::sleep(Duration::from_millis(options.settle_ms));
            let list = undelivered(root)?;
            mark_delivered(root, &list)?;
            return Ok(list);
        }
        thread::sleep(Duration::from_millis(options.poll_ms));
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use crate::review::comments::{add_comment, update_comment, Update};
    use crate::workflow::state::{new_state, save_state};

    fn project(label: &str) -> PathBuf {
        let root = test_dir(label);
        save_state(&root, &mut new_state("new-product", "".into()).unwrap()).unwrap();
        root
    }

    fn update(status: &str, message: Option<&str>, by: Option<&str>) -> Update {
        Update {
            status: Some(status.to_string()),
            message: message.map(Value::from),
            by: by.map(Value::from),
            evidence: None,
        }
    }

    fn quick(timeout_ms: f64, ms: u64) -> WaitOptions {
        WaitOptions {
            timeout_ms,
            poll_ms: ms,
            settle_ms: ms,
        }
    }

    /// inbox: wait wakes on a new comment and delivers each state once (tests/unit/inbox.test.mjs)
    #[test]
    fn inbox_wait_wakes_on_a_new_comment_and_delivers_each_state_once() {
        let root = project("inbox-wait");
        let writer = {
            let root = root.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                add_comment(&root, &json!({ "comment": "Make the confirm button bigger", "screen": "borrow", "target": { "dfwId": "slip.confirm" } })).unwrap();
            })
        };
        let got = wait_for_comments(&root, quick(5000.0, 50)).unwrap();
        writer.join().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0]["target"]["dfwId"], "slip.confirm");
        assert_eq!(undelivered(&root).unwrap().len(), 0);
        let id = got[0]["id"].as_str();
        update_comment(&root, id, &update("acknowledged", None, None)).unwrap();
        update_comment(&root, id, &update("fixed", Some("lg size"), None)).unwrap();
        update_comment(
            &root,
            id,
            &update("reopened", Some("Still too small"), Some("user")),
        )
        .unwrap();
        assert_eq!(
            undelivered(&root).unwrap().len(),
            1,
            "a reopen is delivered again"
        );
        assert_eq!(wait_for_comments(&root, quick(1.0, 1)).unwrap().len(), 1);
    }

    #[test]
    fn wait_gives_up_at_the_timeout() {
        let root = project("inbox-timeout");
        let start = Instant::now();
        assert!(wait_for_comments(&root, quick(120.0, 20))
            .unwrap()
            .is_empty());
        assert!(start.elapsed() >= Duration::from_millis(120));
        // A timeout in the past never looks at the store.
        add_comment(&root, &json!({ "comment": "x" })).unwrap();
        assert!(wait_for_comments(&root, quick(-1.0, 1)).unwrap().is_empty());
        assert_eq!(undelivered(&root).unwrap().len(), 1);
    }

    /// A user's reply on an acknowledged comment is delivered, a reopen names who reopened
    /// (tests/unit/penpot-live.test.mjs: the assertions on undelivered and formatForClaude)
    #[test]
    fn follow_ups_and_reopens_are_news() {
        let root = project("inbox-follow-up");
        let c = add_comment(&root, &json!({ "comment": "Low stock rows", "screen": "items", "variant": "a", "target": { "selector": "tr.low", "component": "table" } })).unwrap();
        let id = c["id"].as_str();
        let list = undelivered(&root).unwrap();
        assert_eq!(
            format_for_claude(&list).lines().nth(1).unwrap(),
            "- fb_001 [open] items / a -> tr.low component table"
        );
        mark_delivered(&root, &list).unwrap();
        update_comment(&root, id, &update("acknowledged", None, None)).unwrap();
        assert_eq!(undelivered(&root).unwrap().len(), 0);
        update_comment(
            &root,
            id,
            &Update {
                message: Some("Also the header".into()),
                by: Some("user".into()),
                ..Update::default()
            },
        )
        .unwrap();
        let text = format_for_claude(&undelivered(&root).unwrap());
        assert!(
            text.contains("\n    follow-up by user: \"Also the header\""),
            "{text}"
        );
        update_comment(&root, id, &update("fixed", Some("Done."), None)).unwrap();
        assert_eq!(
            undelivered(&root).unwrap().len(),
            0,
            "fixed waits for the user"
        );
        update_comment(
            &root,
            id,
            &update("reopened", Some("Still too small"), Some("user")),
        )
        .unwrap();
        let text = format_for_claude(&undelivered(&root).unwrap());
        assert!(
            text.contains("reopened by user: \"Still too small\""),
            "{text}"
        );
    }

    #[test]
    fn the_text_for_the_agent_is_the_node_text() {
        let comments = [
            json!({ "id": "fb_001", "status": "open", "screen": "home", "breakpoint": "mobile", "theme": "dark", "target": { "dfwId": "home.cta", "source": "design/screens/home/a.html:3" }, "comment": "Bigger" }),
            json!({ "id": "fb_002", "status": "open", "comment": "x", "provider": "dfw-live" }),
        ];
        assert_eq!(
            format_for_claude(&comments),
            "2 new Studio comment(s) from the user:\n- fb_001 [open] home / mobile / dark -> home.cta (design/screens/home/a.html:3)\n    \"Bigger\"\n- fb_002 [open] design system -> ?\n    \"x\"\nComments marked \"live\" are on the running app (dfw review): open target.source (file:line:col) and fix the implementation there; without a source, find the element by its selector and text (phase-verify).\nApply them now (references/phase-review.md > \"Applying comments\"): \"dfw comments ack <id>\", fix (prefer design-system fixes when the comment is about a component or token), check, then \"dfw comments fixed <id> --message \\\"<what changed>\\\"\". If a comment conflicts with an approved decision or is ambiguous, reply with \"dfw comments reply <id> --message ...\" and ask the user."
        );
    }

    #[test]
    fn the_delivered_list_keeps_the_newest_keys() {
        let root = project("inbox-cap");
        let many: Vec<Value> = (0..MAX_DELIVERED + 5)
            .map(|i| json!({ "id": format!("fb_{i}"), "status": "open", "createdAt": "t" }))
            .collect();
        mark_delivered(&root, &many).unwrap();
        mark_delivered(&root, &many[..3]).unwrap();
        let kept = read_delivered(&root).unwrap();
        assert_eq!(kept.len(), MAX_DELIVERED);
        assert_eq!(kept[0], "fb_8:open:t");
        assert_eq!(kept[MAX_DELIVERED - 1], "fb_2:open:t");
        mark_delivered(&root, &[]).unwrap();
    }
}
