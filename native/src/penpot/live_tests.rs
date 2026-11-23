//! tests/unit/penpot-live.test.mjs. Its two cases that run the plugin's browser code against a
//! fake of Penpot's API stay JavaScript.

use super::*;
use crate::core::fixture::ITEMS_PAGE;
use crate::core::paths::rel;
use crate::core::util::{iso_from_millis, to_json_compact};
use crate::penpot::export::tests::{draft, fixture_export_at, project, with_export};
use crate::review::comments::{update_comment, Update};
use crate::review::inbox::{format_for_claude, undelivered};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

/// The fixture project with an export rendered after its files were copied; returns the root
/// and the export's id.
fn live_project(label: &str) -> (PathBuf, Value) {
    let root = project(label, draft());
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
    let doc = fixture_export_at(&iso_from_millis(now.as_millis() as i64 + 2000));
    with_export(&root, &doc, "changes.json");
    (root, doc["id"].clone())
}

fn thread(seq: u32, messages: &[&str], resolved: bool) -> Value {
    let comments: Vec<Value> = messages
        .iter()
        .map(|content| json!({ "content": content, "user": "Designer" }))
        .collect();
    json!({ "seq": seq, "resolved": resolved, "page": "Stock", "boardKey": "items|a|desktop", "x": 120, "y": 175, "comments": comments })
}

fn sync(
    root: &Path,
    file_id: &str,
    export_id: &Value,
    threads: Vec<Value>,
    posted: Value,
) -> Value {
    let payload =
        json!({ "fileId": file_id, "exportId": export_id, "threads": threads, "posted": posted });
    sync_threads(root, &payload, None).unwrap()
}

fn count(r: &Value, key: &str) -> usize {
    r[key].as_array().unwrap().len()
}

fn set(root: &Path, id: &str, status: &str, message: Option<&str>) {
    let update = Update {
        status: Some(status.into()),
        message: message.map(Value::from),
        ..Update::default()
    };
    update_comment(root, Some(id), &update).unwrap();
}

fn one(root: &Path) -> Value {
    all_comments(root).unwrap().remove(0)
}

/// penpot live: a new thread becomes one dfw comment, and stays one
#[test]
fn a_new_thread_becomes_one_dfw_comment_and_stays_one() {
    let (root, id) = live_project("penpot-live-new");
    let threads = || {
        vec![
            thread(1, &["Make low stock more visible"], false),
            thread(2, &["Old note"], true),
        ]
    };
    let first = sync(&root, "file-1", &id, threads(), json!([]));
    assert_eq!(count(&first, "comments"), 1); // the resolved thread was never open for dfw
    assert_eq!(first["replies"], json!([]));
    let c = one(&root);
    assert_eq!(c["provider"], "penpot");
    assert!(c["comment"]
        .as_str()
        .unwrap()
        .contains("Make low stock more visible"));
    assert_eq!(c["screen"], "items");
    let again = sync(&root, "file-1", &id, threads(), json!([]));
    assert_eq!(
        (count(&again, "comments"), count(&again, "updated")),
        (0, 0)
    );
    assert_eq!(all_comments(&root).unwrap().len(), 1);
    // "Send changes" from the same file does not import the thread a second time.
    let mut changes = json!({ "schema": "dfw-penpot-changes/1", "exportId": id, "fileId": "file-1", "boards": [], "comments": threads() });
    import_changes(&root, &mut changes, ImportOptions::default()).unwrap();
    assert_eq!(all_comments(&root).unwrap().len(), 1);
}

/// penpot live: what dfw does comes back as thread replies, once
#[test]
fn what_dfw_does_comes_back_as_thread_replies_once() {
    let (root, export) = live_project("penpot-live-replies");
    let t = || vec![thread(1, &["Make low stock more visible"], false)];
    sync(&root, "file-1", &export, t(), json!([]));
    let id = one(&root)["id"].as_str().unwrap().to_string();
    set(&root, &id, "acknowledged", None);
    let ack = sync(&root, "file-1", &export, t(), json!([]))["replies"].clone();
    assert_eq!(ack.as_array().unwrap().len(), 1);
    let text = ack[0]["text"].as_str().unwrap();
    assert!(text.starts_with(&format!("[dfw] {id} seen")), "{text}");
    assert_eq!(to_json_compact(&ack[0]["seq"]), "1");
    // Not confirmed as posted yet: offered again. Confirmed: not offered again.
    assert_eq!(
        count(&sync(&root, "file-1", &export, t(), json!([])), "replies"),
        1
    );
    let posted = json!([{ "seq": 1, "key": ack[0]["key"] }]);
    assert_eq!(
        count(&sync(&root, "file-1", &export, t(), posted), "replies"),
        0
    );
    set(
        &root,
        &id,
        "fixed",
        Some("Low stock rows now have a warning badge."),
    );
    let fixed = sync(&root, "file-1", &export, t(), json!([]))["replies"].clone();
    assert_eq!(fixed.as_array().unwrap().len(), 1);
    let text = fixed[0]["text"].as_str().unwrap();
    assert!(text.contains("fixed: Low stock rows now have a warning badge."));
    assert!(text.contains("Resolve this thread to accept"));
    // The reply dfw posted is in the thread now: it is not a designer message.
    let with_reply = vec![thread(1, &["Make low stock more visible", text], false)];
    let posted = json!([{ "seq": 1, "key": fixed[0]["key"] }]);
    let r = sync(&root, "file-1", &export, with_reply, posted);
    assert_eq!(
        (
            count(&r, "comments"),
            count(&r, "updated"),
            count(&r, "replies")
        ),
        (0, 0, 0)
    );
    assert_eq!(one(&root)["status"], "fixed");
    // Another file's thread #1 is another thread.
    assert_eq!(
        count(
            &sync(&root, "file-2", &export, vec![], json!([])),
            "replies"
        ),
        0
    );
}

/// penpot live: replying reopens, resolving accepts a fix or dismisses an open comment
#[test]
fn replying_reopens_resolving_accepts_or_dismisses() {
    let (root, export) = live_project("penpot-live-resolve");
    let go = |threads: Vec<Value>| sync(&root, "file-1", &export, threads, json!([]));
    go(vec![thread(1, &["A"], false), thread(3, &["B"], false)]);
    let ids: Vec<String> = all_comments(&root)
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    let (a, b) = (ids[0].as_str(), ids[1].as_str());
    set(&root, a, "fixed", Some("Done."));
    // Designer replies after the fix: reopened with the message, and the agent is told.
    let r = go(vec![
        thread(1, &["A", "[dfw] fixed: Done.", "Still too small"], false),
        thread(3, &["B"], false),
    ]);
    assert_eq!(r["updated"], json!([{ "id": a, "status": "reopened" }]));
    let mine: Vec<Value> = undelivered(&root)
        .unwrap()
        .into_iter()
        .filter(|c| c["id"] == a)
        .collect();
    assert!(format_for_claude(&mine).contains("reopened by user: \"Still too small\""));
    set(&root, a, "fixed", Some("Bigger now."));
    // Resolving the thread accepts the fix; resolving an open thread dismisses the comment.
    let resolved = || {
        vec![
            thread(1, &["A", "Still too small"], true),
            thread(3, &["B"], true),
        ]
    };
    let r = go(resolved());
    assert_eq!(
        r["updated"],
        json!([{ "id": a, "status": "resolved" }, { "id": b, "status": "dismissed" }])
    );
    let list = all_comments(&root).unwrap();
    let first = list.iter().find(|c| c["id"] == a).unwrap();
    assert_eq!(
        first["thread"].as_array().unwrap().last().unwrap()["by"],
        "user"
    );
    // Nothing changes on the next check; un-resolving the thread reopens.
    assert_eq!(go(resolved())["updated"], json!([]));
    let r = go(vec![
        thread(1, &["A", "Still too small"], false),
        thread(3, &["B"], true),
    ]);
    assert_eq!(r["updated"], json!([{ "id": a, "status": "reopened" }]));
}

/// penpot live: a follow-up on a comment being worked on reaches the agent
#[test]
fn a_follow_up_on_a_comment_being_worked_on_reaches_the_agent() {
    let (root, export) = live_project("penpot-live-follow-up");
    sync(
        &root,
        "file-1",
        &export,
        vec![thread(1, &["A"], false)],
        json!([]),
    );
    let id = one(&root)["id"].as_str().unwrap().to_string();
    set(&root, &id, "acknowledged", None);
    assert_eq!(undelivered(&root).unwrap().len(), 0);
    let threads = vec![thread(1, &["A", "Also the header"], false)];
    sync(&root, "file-1", &export, threads, json!([]));
    assert_eq!(one(&root)["status"], "acknowledged");
    let told = format_for_claude(&undelivered(&root).unwrap());
    assert!(
        told.contains("follow-up by user: \"Also the header\""),
        "{told}"
    );
}

/// penpot live: a reply in a thread that stays resolved reopens the comment and reaches the agent
#[test]
fn a_reply_in_a_thread_that_stays_resolved_reopens_the_comment() {
    let (root, export) = live_project("penpot-live-resolved-reply");
    let threads = |messages: &[&str], resolved| vec![thread(1, messages, resolved)];
    sync(
        &root,
        "file-1",
        &export,
        threads(&["Make it red"], false),
        json!([]),
    );
    sync(
        &root,
        "file-1",
        &export,
        threads(&["Make it red"], true),
        json!([]),
    );
    assert_eq!(one(&root)["status"], "dismissed");
    assert_eq!(undelivered(&root).unwrap().len(), 0);
    let reply = ["Make it red", "Actually please do it"];
    sync(&root, "file-1", &export, threads(&reply, true), json!([]));
    assert_eq!(one(&root)["status"], "reopened");
    let told = format_for_claude(&undelivered(&root).unwrap());
    assert!(told.contains("Actually please do it"), "{told}");
    let again = sync(&root, "file-1", &export, threads(&reply, true), json!([]));
    assert_eq!(
        again["updated"],
        json!([]),
        "the same messages change nothing"
    );
}

/// penpot live: "seen, working on it" is posted once, also after a later reply
#[test]
fn seen_working_on_it_is_posted_once_also_after_a_later_reply() {
    let (root, export) = live_project("penpot-live-ack-once");
    let threads = || vec![thread(1, &["Make it red"], false)];
    sync(&root, "file-1", &export, threads(), json!([]));
    let id = one(&root)["id"].as_str().unwrap().to_string();
    set(&root, &id, "acknowledged", None);
    let first = sync(&root, "file-1", &export, threads(), json!([]))["replies"].clone();
    let texts = |r: &Value| -> Vec<String> {
        r.as_array()
            .unwrap()
            .iter()
            .map(|x| x["text"].as_str().unwrap().to_string())
            .collect()
    };
    assert_eq!(texts(&first), [format!("[dfw] {id} seen, working on it.")]);
    std::thread::sleep(Duration::from_millis(5));
    let update = Update {
        message: Some("Which red: brand or status?".into()),
        ..Update::default()
    };
    update_comment(&root, Some(&id), &update).unwrap();
    let posted: Vec<Value> = first
        .as_array()
        .unwrap()
        .iter()
        .map(|r| json!({ "seq": r["seq"], "key": r["key"] }))
        .collect();
    let next = sync(&root, "file-1", &export, threads(), json!(posted))["replies"].clone();
    assert_eq!(
        texts(&next),
        [format!("[dfw] {id}: Which red: brand or status?")]
    );
}

/// penpot live: boards are behind once a design file changes after the export
#[test]
fn boards_are_behind_once_a_design_file_changes_after_the_export() {
    let (root, export) = live_project("penpot-live-stale");
    assert!(!export_stale(&root).unwrap());
    let screen = root.join(rel::screen(ITEMS_PAGE));
    let future = SystemTime::now() + Duration::from_secs(60);
    fs::File::options()
        .write(true)
        .open(&screen)
        .unwrap()
        .set_modified(future)
        .unwrap();
    assert!(export_stale(&root).unwrap());
    assert_eq!(sync(&root, "f", &export, vec![], json!([]))["stale"], true);
}

/// The timestamps `Date.parse` reads for the staleness check.
#[test]
fn timestamps_of_an_export() {
    assert_eq!(parse_iso_ms("1970-01-01T00:00:00.000Z"), Some(0.0));
    assert_eq!(
        parse_iso_ms("2026-11-16T10:00:00.678Z"),
        Some(1_794_823_200_678.0)
    );
    assert_eq!(parse_iso_ms("2026-11-16"), Some(1_794_787_200_000.0));
    assert_eq!(
        parse_iso_ms("2026-11-16T12:00:00+02:00"),
        parse_iso_ms("2026-11-16T10:00:00Z")
    );
    assert_eq!(parse_iso_ms("yesterday"), None);
    assert_eq!(parse_iso_ms(""), None);
}
