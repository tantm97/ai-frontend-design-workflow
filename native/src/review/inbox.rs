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
use crate::core::js;
use crate::core::paths::{file, project_paths, rel};
use crate::core::util::{read_json, write_json_inside};
use crate::review::status::{self, by, field, Status};

use super::comments::{all_comments, locked, try_locked, write_reviews, Held};

/// How many delivery keys are kept.
const MAX_DELIVERED: usize = 2000;

/// What identifies one state of a comment: a change of status or a new thread line is news.
fn key(c: &Value) -> String {
    let at = js::truthy_value(c.get(field::UPDATED_AT)).or(c.get("createdAt"));
    format!(
        "{}:{}:{}",
        js::to_string(c.get(field::ID)),
        js::to_string(c.get(field::STATUS)),
        js::to_string(at)
    )
}

fn delivered_file(root: &Path) -> PathBuf {
    project_paths(root).reviews.join(file::REVIEWS_DELIVERED)
}

fn read_delivered(root: &Path) -> Result<Vec<Value>, DfwError> {
    match read_json(&delivered_file(root), Some(json!([])))? {
        Value::Array(seen) => Ok(seen),
        _ => Err(DfwError::internal(format!(
            "{} does not hold a list.",
            rel::join(rel::REVIEWS, file::REVIEWS_DELIVERED)
        ))),
    }
}

fn last_of_thread(c: &Value) -> Option<&Value> {
    c.get(field::THREAD).and_then(Value::as_array)?.last()
}

/// `undelivered(root)`: open and reopened comments, and acknowledged ones the user replied to,
/// whose current state the agent has not been given yet.
pub fn undelivered(root: &Path) -> Result<Vec<Value>, DfwError> {
    let seen = read_delivered(root)?;
    let seen: HashSet<&str> = seen.iter().filter_map(Value::as_str).collect();
    let status = |c: &Value| status::of(c);
    // A follow-up from the user on a comment being worked on (e.g. a reply in its Penpot thread) is news too.
    let follow_up = |c: &Value| {
        status(c) == Some(Status::Acknowledged)
            && last_of_thread(c).and_then(|l| l.get("by")) == Some(&json!(by::USER))
    };
    let active = |c: &Value| matches!(status(c), Some(Status::Open | Status::Reopened));
    // A Studio draft waits for the user's "Send to AI".
    let draft = |c: &Value| c.get("draft") == Some(&Value::Bool(true));
    Ok(all_comments(root)?
        .into_iter()
        .filter(|c| (active(c) || follow_up(c)) && !draft(c) && !seen.contains(key(c).as_str()))
        .collect())
}

/// `markDelivered(root, comments)`: remember that the agent was given these comment states.
/// The read-modify-write of `.delivered.json` holds the reviews lock. Callers deliver through
/// [`take_undelivered`]; the tests mark by hand.
#[cfg(test)]
pub fn mark_delivered(root: &Path, comments: &[Value]) -> Result<(), DfwError> {
    if comments.is_empty() {
        return Ok(());
    }
    locked(root, |held| mark_delivered_unlocked(held, root, comments))
}

fn mark_delivered_unlocked(held: &Held, root: &Path, comments: &[Value]) -> Result<(), DfwError> {
    let mut seen = read_delivered(root)?;
    for c in comments {
        let k = Value::String(key(c));
        if !seen.contains(&k) {
            seen.push(k);
        }
    }
    let skip = seen.len().saturating_sub(MAX_DELIVERED);
    write_reviews(
        held,
        root,
        &delivered_file(root),
        &Value::Array(seen.split_off(skip)),
    )
}

/// How long a hook waits for the reviews lock before it leaves the comments for the next call.
const HOOK_LOCK_WAIT: Duration = Duration::from_millis(50);

/// The undelivered comments, marked delivered in the same hold of the reviews lock, so two
/// callers at once (two hooks, a hook and `dfw comments inbox`) never deliver one comment
/// state twice. For a `hook`, a lock busy for [`HOOK_LOCK_WAIT`] gives an empty list: the
/// comments come with the next call (hooks stay fast and fail open). Otherwise it waits as
/// every writer does.
pub fn take_undelivered(root: &Path, hook: bool) -> Result<Vec<Value>, DfwError> {
    let take = |held: &Held| {
        let list = undelivered(root)?;
        // Nothing new writes nothing (no empty `.delivered.json` in a project without comments).
        if !list.is_empty() {
            mark_delivered_unlocked(held, root, &list)?;
        }
        Ok(list)
    };
    if hook {
        Ok(try_locked(root, HOOK_LOCK_WAIT, take)?.unwrap_or_default())
    } else {
        locked(root, take)
    }
}

/// `formatForClaude(comments)`: the comments as the text the agent reads, with what to do next.
pub fn format_for_claude(comments: &[Value]) -> String {
    let lines: Vec<String> = comments
        .iter()
        .map(|c| {
            let place: Vec<String> = ["screen", "variant", "breakpoint", "state", "theme"]
                .iter()
                .filter_map(|k| js::truthy_value(c.get(k)))
                .map(|v| js::to_string(Some(v)))
                .collect();
            let place = place.join(" / ");
            let place = if place.is_empty() {
                "design system"
            } else {
                &place
            };
            let of_target = |k: &str| js::truthy_value(c.get("target").and_then(|t| t.get(k)));
            let target = of_target("dfwId")
                .or(of_target("selector"))
                .map_or("?".to_string(), |t| js::to_string(Some(t)));
            let last = last_of_thread(c);
            let said = |l: &Value| js::to_string(l.get("message"));
            let reopened = match last {
                Some(l) if c.get("status") == Some(&json!("reopened")) => {
                    format!(
                        "\n    reopened by {}: \"{}\"",
                        js::to_string(l.get("by")),
                        said(l)
                    )
                }
                Some(l) if l.get("by") == Some(&json!(by::USER)) => {
                    format!("\n    follow-up by user: \"{}\"", said(l))
                }
                _ => String::new(),
            };
            format!(
                "- {} [{}] {place} -> {target}{}{}\n    \"{}\"{reopened}",
                js::to_string(c.get("id")),
                js::to_string(c.get("status")),
                of_target("source")
                    .map_or(String::new(), |s| format!(" ({})", js::to_string(Some(s)))),
                of_target("component").map_or(String::new(), |s| format!(
                    " component {}",
                    js::to_string(Some(s))
                )),
                js::to_string(c.get("comment")),
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

fn listening_file(root: &Path) -> PathBuf {
    project_paths(root).reviews.join(file::REVIEWS_LISTENING)
}

/// The marker of a running `dfw comments wait`, removed when the wait ends (also on an error).
struct Listening(PathBuf);

impl Listening {
    fn start(root: &Path) -> Result<Self, DfwError> {
        let file = listening_file(root);
        let marker = json!({ "pid": std::process::id(), "since": crate::core::util::now_iso() });
        write_json_inside(root, &file, &marker)?;
        Ok(Listening(file))
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        // A marker left behind (killed process) is ignored by `is_listening`: its pid is gone.
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `isListening(root)`: whether a `dfw comments wait` is running for this project, so the Studio
/// can tell the user whether a sent comment wakes Claude now or with their next message.
pub fn is_listening(root: &Path) -> bool {
    read_json(&listening_file(root), Some(Value::Null))
        .ok()
        .is_some_and(|m| {
            crate::core::process::Pid::of(m.get("pid")).is_some_and(crate::core::process::is_alive)
        })
}

/// `waitForComments(root, { timeoutMs, pollMs, settleMs })`: block until undelivered comments
/// exist (or the timeout passes). Returns the comments, marked delivered.
pub fn wait_for_comments(root: &Path, options: WaitOptions) -> Result<Vec<Value>, DfwError> {
    let _listening = Listening::start(root)?;
    let start = Instant::now();
    let before_end = || start.elapsed().as_secs_f64() * 1000.0 < options.timeout_ms;
    while before_end() {
        if !undelivered(root)?.is_empty() {
            // Let the user finish a burst of comments before waking the agent.
            thread::sleep(Duration::from_millis(options.settle_ms));
            return take_undelivered(root, false);
        }
        thread::sleep(Duration::from_millis(options.poll_ms));
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::SCREEN_PAGE;
    use crate::core::util::test_dir;
    use crate::review::comments::{add_comment, update_comment, Update};
    use crate::workflow::state::{new_state, save_state};

    fn project(label: &str) -> PathBuf {
        let root = test_dir(label);
        save_state(&root, &mut new_state("new-product", "".into()).unwrap()).unwrap();
        root
    }

    fn update(status: Status, message: Option<&str>, by: Option<&str>) -> Update {
        Update {
            status: Some(status.as_str().to_string()),
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
        const CONFIRM: &str = "slip.confirm";
        let writer = {
            let root = root.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                add_comment(&root, &json!({ "comment": "Make the confirm button bigger", "screen": "borrow", "target": { "dfwId": CONFIRM } })).unwrap();
            })
        };
        let got = wait_for_comments(&root, quick(5000.0, 50)).unwrap();
        writer.join().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0][field::TARGET]["dfwId"], CONFIRM);
        assert_eq!(undelivered(&root).unwrap().len(), 0);
        let id = got[0][field::ID].as_str();
        update_comment(&root, id, &update(Status::Acknowledged, None, None)).unwrap();
        update_comment(&root, id, &update(Status::Fixed, Some("lg size"), None)).unwrap();
        update_comment(
            &root,
            id,
            &update(Status::Reopened, Some("Still too small"), Some(by::USER)),
        )
        .unwrap();
        assert_eq!(
            undelivered(&root).unwrap().len(),
            1,
            "a reopen is delivered again"
        );
        assert_eq!(wait_for_comments(&root, quick(1.0, 1)).unwrap().len(), 1);
    }

    /// Studio drafts reach the agent only once the user sends them, all in one wake-up.
    #[test]
    fn drafts_are_delivered_only_after_send() {
        use crate::review::comments::send_drafts;
        let root = project("inbox-drafts");
        const LIVE: &str = "Live one";
        add_comment(&root, &json!({ "comment": "Box the tiles", "draft": true })).unwrap();
        add_comment(&root, &json!({ "comment": "Bigger price", "draft": true })).unwrap();
        // Comments of other providers (dfw review, Penpot) are never drafts.
        add_comment(&root, &json!({ "comment": LIVE, "provider": "dfw-live" })).unwrap();
        let first = take_undelivered(&root, false).unwrap();
        assert_eq!(first.len(), 1, "drafts are held back");
        assert_eq!(first[0][field::COMMENT], LIVE);
        assert!(wait_for_comments(&root, quick(60.0, 10))
            .unwrap()
            .is_empty());
        let sent = send_drafts(&root).unwrap();
        assert_eq!(sent.len(), 2);
        assert!(sent
            .iter()
            .all(|c| c.get("draft").is_none() && c["sentAt"].is_string()));
        let got = wait_for_comments(&root, quick(1000.0, 10)).unwrap();
        assert_eq!(got.len(), 2, "both drafts arrive in one wake-up");
        assert!(
            send_drafts(&root).unwrap().is_empty(),
            "nothing left to send"
        );
    }

    /// The Studio sees a running wait, and not one that ended.
    #[test]
    fn a_running_wait_is_listening() {
        let root = project("inbox-listening");
        assert!(!is_listening(&root));
        // Long enough never to end by itself on a slow runner: a comment ends it.
        let waiter = {
            let root = root.clone();
            thread::spawn(move || wait_for_comments(&root, quick(60_000.0, 20)).unwrap())
        };
        // Wait for the marker with a deadline, not a fixed sleep (testing.md T12): on the
        // windows-latest runner the thread took longer than 150 ms to write it.
        let end = Instant::now() + Duration::from_secs(10);
        while !is_listening(&root) && Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(is_listening(&root));
        add_comment(&root, &json!({ "comment": "wake up" })).unwrap();
        assert_eq!(waiter.join().unwrap().len(), 1, "the comment ends the wait");
        assert!(!is_listening(&root), "the marker goes when the wait ends");
        // A marker of a process that is gone (killed wait) does not count.
        std::fs::write(
            listening_file(&root),
            r#"{"pid": 2147483646, "since": "2026-01-01T00:00:00Z"}"#,
        )
        .unwrap();
        assert!(!is_listening(&root));
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
        let id = c[field::ID].as_str();
        let list = undelivered(&root).unwrap();
        assert_eq!(
            format_for_claude(&list).lines().nth(1).unwrap(),
            "- fb_001 [open] items / a -> tr.low component table"
        );
        mark_delivered(&root, &list).unwrap();
        update_comment(&root, id, &update(Status::Acknowledged, None, None)).unwrap();
        assert_eq!(undelivered(&root).unwrap().len(), 0);
        update_comment(
            &root,
            id,
            &Update {
                message: Some("Also the header".into()),
                by: Some(by::USER.into()),
                ..Update::default()
            },
        )
        .unwrap();
        let text = format_for_claude(&undelivered(&root).unwrap());
        assert!(
            text.contains("\n    follow-up by user: \"Also the header\""),
            "{text}"
        );
        update_comment(&root, id, &update(Status::Fixed, Some("Done."), None)).unwrap();
        assert_eq!(
            undelivered(&root).unwrap().len(),
            0,
            "fixed waits for the user"
        );
        update_comment(
            &root,
            id,
            &update(Status::Reopened, Some("Still too small"), Some(by::USER)),
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
        let open = Status::Open.as_str();
        let comments = [
            json!({ "id": "fb_001", "status": open, "screen": "home", "breakpoint": "mobile", "theme": "dark", "target": { "dfwId": "home.cta", "source": format!("{}:3", rel::screen(SCREEN_PAGE)) }, "comment": "Bigger" }),
            json!({ "id": "fb_002", "status": open, "comment": "x", "provider": "dfw-live" }),
        ];
        assert_eq!(
            format_for_claude(&comments),
            format!("2 new Studio comment(s) from the user:\n- fb_001 [open] home / mobile / dark -> home.cta ({}:3)\n    \"Bigger\"\n- fb_002 [open] design system -> ?\n    \"x\"\nComments marked \"live\" are on the running app (dfw review): open target.source (file:line:col) and fix the implementation there; without a source, find the element by its selector and text (phase-verify).\nApply them now (references/phase-review.md > \"Applying comments\"): \"dfw comments ack <id>\", fix (prefer design-system fixes when the comment is about a component or token), check, then \"dfw comments fixed <id> --message \\\"<what changed>\\\"\". If a comment conflicts with an approved decision or is ambiguous, reply with \"dfw comments reply <id> --message ...\" and ask the user.", rel::screen(SCREEN_PAGE))
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

    /// Eight callers at once deliver each comment once (review CO-5: parallel
    /// `dfw comments inbox` calls gave the same comments three times).
    #[test]
    fn parallel_takers_deliver_each_comment_once() {
        let root = project("inbox-parallel");
        for i in 0..5 {
            add_comment(&root, &json!({ "comment": format!("c{i}") })).unwrap();
        }
        let delivered: Vec<usize> = std::thread::scope(|s| {
            let takers: Vec<_> = (0..8)
                .map(|_| s.spawn(|| take_undelivered(&root, false).unwrap().len()))
                .collect();
            takers.into_iter().map(|t| t.join().unwrap()).collect()
        });
        assert_eq!(delivered.iter().sum::<usize>(), 5, "{delivered:?}");
        assert!(undelivered(&root).unwrap().is_empty());
    }

    /// A hook gives up on a busy lock fast and leaves the comments for its next call.
    #[test]
    fn a_hook_leaves_the_comments_when_the_lock_is_busy() {
        let root = project("inbox-busy");
        // Nothing to deliver writes no delivery list.
        assert!(take_undelivered(&root, true).unwrap().is_empty());
        assert!(!delivered_file(&root).exists());
        add_comment(&root, &json!({ "comment": "later" })).unwrap();
        let lock = root.join(rel::join(rel::REVIEWS, file::REVIEWS_LOCK));
        let held = crate::core::lock::acquire(&lock, &crate::core::lock::STANDARD).unwrap();
        let start = Instant::now();
        assert!(take_undelivered(&root, true).unwrap().is_empty());
        assert!(start.elapsed() < Duration::from_secs(1));
        drop(held);
        assert_eq!(take_undelivered(&root, true).unwrap().len(), 1);
        assert!(take_undelivered(&root, false).unwrap().is_empty());
    }
}
