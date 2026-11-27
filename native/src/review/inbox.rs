//! Ports `core/lib/inbox.mjs`: delivers Studio review comments to the agent without the user
//! having to relay them:
//! - `dfw comments wait` (run in the background by the agent) exits as soon as a new comment arrives;
//! - the UserPromptSubmit and Stop hooks inject undelivered comments into the conversation.
//!
//! Each comment state is delivered once (`.dfw/reviews/.delivered.json`).

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, project_paths, rel};
use crate::core::process::{is_alive, Pid};
use crate::core::util::{now_iso, read_json};
use crate::design::pages::page;
use crate::review::status::{self, by, field, listening, Status};

use super::comments::{all_comments, locked, try_locked, write_reviews, Held};

/// The command the agent keeps running in the background while the user reviews in the Studio.
pub const WAIT_COMMAND: &str = "dfw comments wait --timeout 3600";

/// How long `dfw comments wait` waits when `--timeout` names no number, in seconds.
pub const WAIT_SECONDS: f64 = 3600.0;

/// Where a comment on the Design System page is, in the text for the agent.
const DESIGN_SYSTEM: &str = "design system";

/// How many delivery keys are kept.
const MAX_DELIVERED: usize = 2000;

/// What identifies one state of a comment: a change of status or a new thread line is news.
fn key(c: &Value) -> String {
    let at = js::truthy_value(c.get(field::UPDATED_AT)).or(c.get(field::CREATED_AT));
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
            && last_of_thread(c).and_then(|l| l.get(field::BY)) == Some(&json!(by::USER))
    };
    let active = |c: &Value| matches!(status(c), Some(Status::Open | Status::Reopened));
    // A Studio draft waits for the user's "Send to AI".
    let draft = |c: &Value| c.get(field::DRAFT) == Some(&Value::Bool(true));
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

/// "in the background", with the host's words for a background command when they are known
/// (the `{{BACKGROUND}}` text of the host table).
pub fn in_background(background: Option<&str>) -> String {
    background.map_or("in the background".to_string(), |how| {
        format!("in the background ({how})")
    })
}

/// The last sentence of every batch of comments: re-arm the wait, so the next "Send to AI" in
/// the Studio wakes the agent again.
pub fn re_arm(background: Option<&str>) -> String {
    format!(
        "Then, before you end your turn, start \"{WAIT_COMMAND}\" again {} so the user's next Studio comments wake you.",
        in_background(background)
    )
}

/// `formatForClaude(comments)`: the comments as the text the agent reads, with what to do next.
/// `background` is the host's wording for a background command (`None`: not known).
pub fn format_for_claude(comments: &[Value], background: Option<&str>) -> String {
    let lines: Vec<String> = comments
        .iter()
        .map(|c| {
            let place: Vec<String> = [
                field::SCREEN,
                field::VARIANT,
                field::BREAKPOINT,
                field::STATE,
                field::THEME,
            ]
            .iter()
            .filter_map(|k| js::truthy_value(c.get(k)))
            .map(|v| js::to_string(Some(v)))
            // The Studio's Design System page is the screen `system`.
            .map(|v| {
                if v == page::SYSTEM {
                    DESIGN_SYSTEM.to_string()
                } else {
                    v
                }
            })
            .collect();
            let place = place.join(" / ");
            let place = if place.is_empty() {
                DESIGN_SYSTEM
            } else {
                &place
            };
            let of_target = |k: &str| js::truthy_value(c.get(field::TARGET).and_then(|t| t.get(k)));
            let target = of_target(field::DFW_ID)
                .or(of_target(field::SELECTOR))
                .map_or("?".to_string(), |t| js::to_string(Some(t)));
            let last = last_of_thread(c);
            let said = |l: &Value| js::to_string(l.get(field::MESSAGE));
            let reopened = match last {
                Some(l) if c.get(field::STATUS) == Some(&json!("reopened")) => {
                    format!(
                        "\n    reopened by {}: \"{}\"",
                        js::to_string(l.get(field::BY)),
                        said(l)
                    )
                }
                Some(l) if l.get(field::BY) == Some(&json!(by::USER)) => {
                    format!("\n    follow-up by user: \"{}\"", said(l))
                }
                _ => String::new(),
            };
            format!(
                "- {} [{}] {place} -> {target}{}{}\n    \"{}\"{reopened}",
                js::to_string(c.get(field::ID)),
                js::to_string(c.get(field::STATUS)),
                of_target(field::SOURCE)
                    .map_or(String::new(), |s| format!(" ({})", js::to_string(Some(s)))),
                of_target(field::COMPONENT).map_or(String::new(), |s| format!(
                    " component {}",
                    js::to_string(Some(s))
                )),
                js::to_string(c.get(field::COMMENT)),
            )
        })
        .collect();
    let live = if comments
        .iter()
        .any(|c| c.get(field::PROVIDER) == Some(&json!("dfw-live")))
    {
        "\nComments marked \"live\" are on the running app (dfw review): open target.source (file:line:col) and fix the implementation there; without a source, find the element by its selector and text (phase-verify)."
    } else {
        ""
    };
    format!(
        "{} new Studio comment(s) from the user:\n{}{live}\nApply them now (references/phase-review.md > \"Applying comments\"): \"dfw comments ack <id>\", fix (prefer design-system fixes when the comment is about a component or token), check, then \"dfw comments fixed <id> --message \\\"<what changed>\\\"\". If a comment conflicts with an approved decision or is ambiguous, reply with \"dfw comments reply <id> --message ...\" and ask the user (they answer in the Studio).\n{}",
        comments.len(),
        lines.join("\n"),
        re_arm(background)
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
            timeout_ms: WAIT_SECONDS * 1000.0,
            poll_ms: 1000,
            settle_ms: 1500,
        }
    }
}

fn listening_file(root: &Path) -> PathBuf {
    project_paths(root).reviews.join(file::REVIEWS_LISTENING)
}

/// The entries of `.listening.json`, one per running wait. A marker of an older dfw (one
/// object) is one entry; a missing or unreadable file is none (the file is dfw's own, and a
/// wait that cannot read it starts a new list).
fn listeners(root: &Path) -> Vec<Value> {
    match read_json(&listening_file(root), Some(Value::Null)) {
        Ok(Value::Array(list)) => list,
        Ok(one @ Value::Object(_)) => vec![one],
        _ => Vec::new(),
    }
}

/// Whether the process of a listening entry still runs.
fn entry_alive(entry: &Value) -> bool {
    Pid::of(entry.get(listening::PID)).is_some_and(is_alive)
}

/// Waits started by this process, so two waits of one process (threads of a test) keep two
/// entries.
static WAITS: AtomicU64 = AtomicU64::new(0);

/// The entry of a running `dfw comments wait` in `.listening.json`, removed when the wait ends
/// (also on an error). Each wait adds and removes only its own entry, under the reviews lock, so
/// a second wait never ends the first one's; entries of processes that are gone are dropped by
/// every write.
struct Listening {
    root: PathBuf,
    token: String,
}

impl Listening {
    fn start(root: &Path) -> Result<Self, DfwError> {
        let pid = std::process::id();
        let token = format!("{pid}.{}", WAITS.fetch_add(1, Ordering::Relaxed));
        let entry = json!({ (listening::PID): pid, (listening::SINCE): now_iso(), (listening::TOKEN): token });
        locked(root, |held| {
            let mut list: Vec<Value> = listeners(root).into_iter().filter(entry_alive).collect();
            list.push(entry);
            write_reviews(held, root, &listening_file(root), &Value::Array(list))
        })?;
        Ok(Listening {
            root: root.to_path_buf(),
            token,
        })
    }

    fn stop(&self) -> Result<(), DfwError> {
        locked(&self.root, |held| {
            let mine = Some(&Value::String(self.token.clone()));
            let list: Vec<Value> = listeners(&self.root)
                .into_iter()
                .filter(|e| e.get(listening::TOKEN) != mine && entry_alive(e))
                .collect();
            let file = listening_file(&self.root);
            if list.is_empty() {
                // The last wait leaves no file behind.
                return match std::fs::remove_file(&file) {
                    Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.into()),
                    _ => Ok(()),
                };
            }
            write_reviews(held, &self.root, &file, &Value::Array(list))
        })
    }
}

impl Drop for Listening {
    fn drop(&mut self) {
        // A drop cannot fail: an entry left behind is ignored by `is_listening` once its process
        // is gone, and dropped by the next wait.
        let _ = self.stop();
    }
}

/// `isListening(root)`: whether a `dfw comments wait` is running for this project, so the Studio
/// can tell the user whether a sent comment wakes the agent now or with their next message.
/// Reads only: entries of processes that are gone are ignored, never removed here (the Studio
/// asks on a GET, which writes nothing).
pub fn is_listening(root: &Path) -> bool {
    listeners(root).iter().any(entry_alive)
}

/// What the Studio tells the user about the agent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listener {
    /// A `dfw comments wait` runs: a sent comment wakes the agent.
    Listening,
    /// Nobody listens, but the agent was given comments it has not finished and was active
    /// within [`WORKING_WINDOW`]: it is between waking and re-arming.
    Working,
    /// Sent comments wait for the agent's next turn.
    Off,
}

/// How recent the agent's last delivery or design edit must be to count as working.
pub const WORKING_WINDOW: Duration = Duration::from_secs(10 * 60);

/// Every comment of the project with [`field::DELIVERED`]: whether its current state was given
/// to the agent (a draft never is; a comment that is no news, such as a fixed one, counts as
/// given). Computed, never stored.
pub fn with_delivery(root: &Path) -> Result<Vec<Value>, DfwError> {
    let news: HashSet<String> = undelivered(root)?.iter().map(key).collect();
    Ok(all_comments(root)?
        .into_iter()
        .map(|mut c| {
            let given = c.get(field::DRAFT) != Some(&Value::Bool(true)) && !news.contains(&key(&c));
            if let Value::Object(o) = &mut c {
                o.insert(field::DELIVERED.into(), Value::Bool(given));
            }
            c
        })
        .collect())
}

/// Whether a file changed within [`WORKING_WINDOW`] of `now`.
fn recent(path: &Path, now: SystemTime) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| now.duration_since(at).ok())
        .is_some_and(|age| age < WORKING_WINDOW)
}

/// [`Listener`] of the project. Reads only.
pub fn listener(root: &Path) -> Result<Listener, DfwError> {
    if is_listening(root) {
        return Ok(Listener::Listening);
    }
    let busy = with_delivery(root)?.iter().any(|c| {
        c.get(field::DELIVERED) == Some(&Value::Bool(true))
            && status::of(c).is_some_and(Status::is_active)
    });
    let now = SystemTime::now();
    let active = recent(&delivered_file(root), now) || recent(&project_paths(root).presence, now);
    Ok(if busy && active {
        Listener::Working
    } else {
        Listener::Off
    })
}

/// `waitForComments(root, { timeoutMs, pollMs, settleMs })`: block until undelivered comments
/// exist (or the timeout passes). Returns the comments, marked delivered. Comments another
/// reader (a hook) took during the settle time are not this wait's: it goes on waiting.
pub fn wait_for_comments(root: &Path, options: WaitOptions) -> Result<Vec<Value>, DfwError> {
    let _listening = Listening::start(root)?;
    let start = Instant::now();
    let before_end = || start.elapsed().as_secs_f64() * 1000.0 < options.timeout_ms;
    while before_end() {
        if !undelivered(root)?.is_empty() {
            // Let the user finish a burst of comments before waking the agent.
            thread::sleep(Duration::from_millis(options.settle_ms));
            let got = take_undelivered(root, false)?;
            if !got.is_empty() {
                return Ok(got);
            }
            continue;
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
            format_for_claude(&list, None).lines().nth(1).unwrap(),
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
        let text = format_for_claude(&undelivered(&root).unwrap(), None);
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
        let text = format_for_claude(&undelivered(&root).unwrap(), None);
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
            format_for_claude(&comments, None),
            format!("2 new Studio comment(s) from the user:\n- fb_001 [open] home / mobile / dark -> home.cta ({}:3)\n    \"Bigger\"\n- fb_002 [open] design system -> ?\n    \"x\"\nComments marked \"live\" are on the running app (dfw review): open target.source (file:line:col) and fix the implementation there; without a source, find the element by its selector and text (phase-verify).\nApply them now (references/phase-review.md > \"Applying comments\"): \"dfw comments ack <id>\", fix (prefer design-system fixes when the comment is about a component or token), check, then \"dfw comments fixed <id> --message \\\"<what changed>\\\"\". If a comment conflicts with an approved decision or is ambiguous, reply with \"dfw comments reply <id> --message ...\" and ask the user (they answer in the Studio).\nThen, before you end your turn, start \"dfw comments wait --timeout 3600\" again in the background so the user's next Studio comments wake you.", rel::screen(SCREEN_PAGE))
        );
    }

    /// A comment on the Design System page is placed as "design system", and the host's words
    /// for a background command end the text.
    #[test]
    fn design_system_comments_say_design_system() {
        let comments = [
            json!({ "id": "fb_003", "status": Status::Open.as_str(), "screen": page::SYSTEM, "theme": "dark", "target": { "dfwId": "system.button" }, "comment": "Rounder" }),
        ];
        let text = format_for_claude(&comments, Some("Bash `run_in_background`"));
        assert_eq!(
            text.lines().nth(1),
            Some("- fb_003 [open] design system / dark -> system.button")
        );
        assert!(
            text.ends_with("again in the background (Bash `run_in_background`) so the user's next Studio comments wake you."),
            "{text}"
        );
    }

    /// Polls `done` until it holds or 10 s pass (testing.md T12).
    fn eventually(done: impl Fn() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while !done() && Instant::now() < end {
            thread::sleep(Duration::from_millis(10));
        }
        done()
    }

    fn entries(root: &Path) -> usize {
        listeners(root).len()
    }

    /// Two waits keep one entry each: the one that ends removes only its own (the first wait
    /// was reported not listening while it still ran).
    #[test]
    fn two_waits_share_the_marker() {
        let root = project("inbox-two-waits");
        let long = {
            let root = root.clone();
            thread::spawn(move || wait_for_comments(&root, quick(60_000.0, 20)).unwrap())
        };
        assert!(eventually(|| entries(&root) == 1));
        let short = {
            let root = root.clone();
            thread::spawn(move || wait_for_comments(&root, quick(300.0, 20)).unwrap())
        };
        assert!(eventually(|| entries(&root) == 2));
        assert!(short.join().unwrap().is_empty(), "the short wait times out");
        assert_eq!(entries(&root), 1, "only its own entry went");
        assert!(is_listening(&root), "the long wait still listens");
        add_comment(&root, &json!({ "comment": "wake up" })).unwrap();
        assert_eq!(long.join().unwrap().len(), 1);
        assert!(
            !listening_file(&root).exists(),
            "the last wait leaves no file"
        );
    }

    /// An entry of a process that is gone does not listen, and the next wait drops it; an
    /// older dfw's single object is read.
    #[test]
    fn a_dead_entry_is_not_listening_and_is_dropped_by_the_next_wait() {
        let root = project("inbox-dead-entry");
        std::fs::create_dir_all(project_paths(&root).reviews).unwrap();
        let dead = json!([{ (listening::PID): 2_147_483_646u32 }]);
        std::fs::write(listening_file(&root), dead.to_string()).unwrap();
        assert!(!is_listening(&root));
        assert_eq!(entries(&root), 1, "a read removes nothing");
        let old = json!({ (listening::PID): std::process::id() });
        std::fs::write(listening_file(&root), old.to_string()).unwrap();
        assert!(is_listening(&root), "the marker of an older dfw");
        std::fs::write(listening_file(&root), dead.to_string()).unwrap();
        let waiter = {
            let root = root.clone();
            thread::spawn(move || wait_for_comments(&root, quick(60_000.0, 20)).unwrap())
        };
        assert!(eventually(|| is_listening(&root)));
        let list = listeners(&root);
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0][listening::PID], std::process::id());
        add_comment(&root, &json!({ "comment": "end" })).unwrap();
        waiter.join().unwrap();
    }

    /// Comments a hook took during the settle time are not the wait's: it goes on until the
    /// next comment (it printed "No new comments before the timeout" at once).
    #[test]
    fn a_wait_keeps_waiting_when_a_hook_took_its_comments() {
        let root = project("inbox-hook-took");
        let options = WaitOptions {
            timeout_ms: 60_000.0,
            poll_ms: 10,
            // Long, so the hook below takes the comment inside it even on a slow runner.
            settle_ms: 2000,
        };
        let waiter = {
            let root = root.clone();
            thread::spawn(move || wait_for_comments(&root, options).unwrap())
        };
        assert!(eventually(|| is_listening(&root)));
        add_comment(&root, &json!({ "comment": "taken by the hook" })).unwrap();
        // Inside the settle time: the wait has seen the comment (it polls every 10 ms). If a
        // slow runner had not yet, the wait still must not end empty.
        thread::sleep(Duration::from_millis(150));
        let taken = take_undelivered(&root, true).unwrap().len();
        let later = "sent later";
        add_comment(&root, &json!({ "comment": later })).unwrap();
        let got = waiter.join().unwrap();
        assert_eq!(taken, 1, "the hook took the first comment");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0][field::COMMENT], later);
    }

    /// Delivered and still open: the agent is working; fixed, or long ago: it is not.
    #[test]
    fn working_after_a_delivery_until_fixed() {
        let root = project("inbox-working");
        assert_eq!(listener(&root).unwrap(), Listener::Off);
        let c = add_comment(&root, &json!({ "comment": "Bigger" })).unwrap();
        let id = c[field::ID].as_str();
        assert_eq!(listener(&root).unwrap(), Listener::Off, "not given yet");
        assert_eq!(with_delivery(&root).unwrap()[0][field::DELIVERED], false);
        take_undelivered(&root, false).unwrap();
        assert_eq!(with_delivery(&root).unwrap()[0][field::DELIVERED], true);
        assert_eq!(listener(&root).unwrap(), Listener::Working);
        update_comment(&root, id, &update(Status::Acknowledged, None, None)).unwrap();
        assert_eq!(listener(&root).unwrap(), Listener::Working, "acknowledged");
        update_comment(&root, id, &update(Status::Fixed, Some("done"), None)).unwrap();
        assert_eq!(
            listener(&root).unwrap(),
            Listener::Off,
            "nothing left to do"
        );
        let draft = add_comment(&root, &json!({ "comment": "later", "draft": true })).unwrap();
        let shown = with_delivery(&root).unwrap();
        let draft = shown
            .iter()
            .find(|c| c[field::ID] == draft[field::ID])
            .unwrap();
        assert_eq!(draft[field::DELIVERED], false, "a draft is never delivered");
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
