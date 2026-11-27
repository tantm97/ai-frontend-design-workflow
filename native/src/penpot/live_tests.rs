//! tests/unit/penpot-live.test.mjs. Its two cases that run the plugin's browser code against a
//! fake of Penpot's API stay JavaScript.

use super::*;
use crate::core::fixture::ITEMS_PAGE;
use crate::core::paths::rel;
use crate::core::util::{iso_from_millis, to_json_compact};
use crate::penpot::export::tests::{draft, fixture_export_at, project, with_export};
use crate::penpot::wire;
use crate::review::comments::{update_comment, Update};
use crate::review::inbox::{format_for_claude, undelivered};
use crate::review::status::field;
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

/// The Penpot file the threads are in.
const FILE: &str = "file-1";
/// What designers write in the threads.
const LOW_STOCK: &str = "Make low stock more visible";
const RED: &str = "Make it red";
const STILL_SMALL: &str = "Still too small";
const PLEASE_DO: &str = "Actually please do it";
const ACKNOWLEDGED: &str = Status::Acknowledged.as_str();
const FIXED: &str = Status::Fixed.as_str();
const RESOLVED: &str = Status::Resolved.as_str();
const REOPENED: &str = Status::Reopened.as_str();
const DISMISSED: &str = Status::Dismissed.as_str();

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
            thread(1, &[LOW_STOCK], false),
            thread(2, &["Old note"], true),
        ]
    };
    let first = sync(&root, FILE, &id, threads(), json!([]));
    assert_eq!(count(&first, wire::COMMENTS), 1); // the resolved thread was never open for dfw
    assert_eq!(first[wire::REPLIES], json!([]));
    let c = one(&root);
    assert_eq!(c["provider"], "penpot");
    assert!(c["comment"].as_str().unwrap().contains(LOW_STOCK));
    assert_eq!(c["screen"], "items");
    let again = sync(&root, FILE, &id, threads(), json!([]));
    assert_eq!(
        (count(&again, wire::COMMENTS), count(&again, wire::UPDATED)),
        (0, 0)
    );
    assert_eq!(all_comments(&root).unwrap().len(), 1);
    // "Send changes" from the same file does not import the thread a second time.
    let mut changes = json!({ "schema": CHANGES_SCHEMA, "exportId": id, "fileId": FILE, "boards": [], "comments": threads() });
    import_changes(&root, &mut changes, ImportOptions::default()).unwrap();
    assert_eq!(all_comments(&root).unwrap().len(), 1);
}

/// penpot live: what dfw does comes back as thread replies, once
#[test]
fn what_dfw_does_comes_back_as_thread_replies_once() {
    let (root, export) = live_project("penpot-live-replies");
    let t = || vec![thread(1, &[LOW_STOCK], false)];
    sync(&root, FILE, &export, t(), json!([]));
    let id = one(&root)["id"].as_str().unwrap().to_string();
    set(&root, &id, ACKNOWLEDGED, None);
    let ack = sync(&root, FILE, &export, t(), json!([]))[wire::REPLIES].clone();
    assert_eq!(ack.as_array().unwrap().len(), 1);
    let text = ack[0][wire::TEXT].as_str().unwrap();
    assert!(text.starts_with(&format!("[dfw] {id} seen")), "{text}");
    assert_eq!(to_json_compact(&ack[0]["seq"]), "1");
    // Not confirmed as posted yet: offered again. Confirmed: not offered again.
    assert_eq!(
        count(&sync(&root, FILE, &export, t(), json!([])), wire::REPLIES),
        1
    );
    let posted = json!([{ "seq": 1, "key": ack[0][wire::KEY] }]);
    assert_eq!(
        count(&sync(&root, FILE, &export, t(), posted), wire::REPLIES),
        0
    );
    set(
        &root,
        &id,
        FIXED,
        Some("Low stock rows now have a warning badge."),
    );
    let fixed = sync(&root, FILE, &export, t(), json!([]))[wire::REPLIES].clone();
    assert_eq!(fixed.as_array().unwrap().len(), 1);
    let text = fixed[0][wire::TEXT].as_str().unwrap();
    assert!(text.contains("fixed: Low stock rows now have a warning badge."));
    assert!(text.contains("Resolve this thread to accept"));
    // The reply dfw posted is in the thread now: it is not a designer message.
    let with_reply = vec![thread(1, &[LOW_STOCK, text], false)];
    let posted = json!([{ "seq": 1, "key": fixed[0][wire::KEY] }]);
    let r = sync(&root, FILE, &export, with_reply, posted);
    assert_eq!(
        (
            count(&r, wire::COMMENTS),
            count(&r, wire::UPDATED),
            count(&r, wire::REPLIES)
        ),
        (0, 0, 0)
    );
    assert_eq!(one(&root)[field::STATUS], FIXED);
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
    let go = |threads: Vec<Value>| sync(&root, FILE, &export, threads, json!([]));
    go(vec![thread(1, &["A"], false), thread(3, &["B"], false)]);
    let ids: Vec<String> = all_comments(&root)
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap().to_string())
        .collect();
    let (a, b) = (ids[0].as_str(), ids[1].as_str());
    set(&root, a, FIXED, Some("Done."));
    // Designer replies after the fix: reopened with the message, and the agent is told.
    let r = go(vec![
        thread(1, &["A", "[dfw] fixed: Done.", STILL_SMALL], false),
        thread(3, &["B"], false),
    ]);
    assert_eq!(r[wire::UPDATED], json!([{ "id": a, "status": REOPENED }]));
    let mine: Vec<Value> = undelivered(&root)
        .unwrap()
        .into_iter()
        .filter(|c| c["id"] == a)
        .collect();
    assert!(format_for_claude(&mine, None).contains("reopened by user: \"Still too small\""));
    set(&root, a, FIXED, Some("Bigger now."));
    // Resolving the thread accepts the fix; resolving an open thread dismisses the comment.
    let resolved = || {
        vec![
            thread(1, &["A", STILL_SMALL], true),
            thread(3, &["B"], true),
        ]
    };
    let r = go(resolved());
    assert_eq!(
        r[wire::UPDATED],
        json!([{ "id": a, "status": RESOLVED }, { "id": b, "status": DISMISSED }])
    );
    let list = all_comments(&root).unwrap();
    let first = list.iter().find(|c| c["id"] == a).unwrap();
    assert_eq!(
        first["thread"].as_array().unwrap().last().unwrap()["by"],
        "user"
    );
    // Nothing changes on the next check; un-resolving the thread reopens.
    assert_eq!(go(resolved())[wire::UPDATED], json!([]));
    let r = go(vec![
        thread(1, &["A", STILL_SMALL], false),
        thread(3, &["B"], true),
    ]);
    assert_eq!(r[wire::UPDATED], json!([{ "id": a, "status": REOPENED }]));
}

/// penpot live: a follow-up on a comment being worked on reaches the agent
#[test]
fn a_follow_up_on_a_comment_being_worked_on_reaches_the_agent() {
    let (root, export) = live_project("penpot-live-follow-up");
    sync(
        &root,
        FILE,
        &export,
        vec![thread(1, &["A"], false)],
        json!([]),
    );
    let id = one(&root)["id"].as_str().unwrap().to_string();
    set(&root, &id, ACKNOWLEDGED, None);
    assert_eq!(undelivered(&root).unwrap().len(), 0);
    let threads = vec![thread(1, &["A", "Also the header"], false)];
    sync(&root, FILE, &export, threads, json!([]));
    assert_eq!(one(&root)[field::STATUS], ACKNOWLEDGED);
    let told = format_for_claude(&undelivered(&root).unwrap(), None);
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
    sync(&root, FILE, &export, threads(&[RED], false), json!([]));
    sync(&root, FILE, &export, threads(&[RED], true), json!([]));
    assert_eq!(one(&root)[field::STATUS], DISMISSED);
    assert_eq!(undelivered(&root).unwrap().len(), 0);
    let reply = [RED, PLEASE_DO];
    sync(&root, FILE, &export, threads(&reply, true), json!([]));
    assert_eq!(one(&root)[field::STATUS], REOPENED);
    let told = format_for_claude(&undelivered(&root).unwrap(), None);
    assert!(told.contains(PLEASE_DO), "{told}");
    let again = sync(&root, FILE, &export, threads(&reply, true), json!([]));
    assert_eq!(
        again[wire::UPDATED],
        json!([]),
        "the same messages change nothing"
    );
}

/// penpot live: "seen, working on it" is posted once, also after a later reply
#[test]
fn seen_working_on_it_is_posted_once_also_after_a_later_reply() {
    let (root, export) = live_project("penpot-live-ack-once");
    let threads = || vec![thread(1, &[RED], false)];
    sync(&root, FILE, &export, threads(), json!([]));
    let id = one(&root)["id"].as_str().unwrap().to_string();
    set(&root, &id, ACKNOWLEDGED, None);
    let first = sync(&root, FILE, &export, threads(), json!([]))[wire::REPLIES].clone();
    let texts = |r: &Value| -> Vec<String> {
        r.as_array()
            .unwrap()
            .iter()
            .map(|x| x[wire::TEXT].as_str().unwrap().to_string())
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
        .map(|r| json!({ "seq": r["seq"], "key": r[wire::KEY] }))
        .collect();
    let next = sync(&root, FILE, &export, threads(), json!(posted))[wire::REPLIES].clone();
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

/// §0 row 38: the plugin's sync runs on Studio connection threads at the same time; every
/// reply marked posted is kept (the ledger is read, changed and written under its lock).
#[test]
fn concurrent_syncs_keep_every_posted_reply() {
    /// How many syncs mark a reply posted at the same time.
    const WRITERS: usize = 24;
    let (root, _) = live_project("penpot-live-posted");
    let tk = thread_key(Some(&FILE.into()), Some(&1.into()));
    let link = json!({ (wire::link::COMMENT): "fb_001", (wire::link::SEEN): 0, (wire::link::RESOLVED): false, (wire::link::POSTED): [] });
    crate::core::util::write_json(&threads_file(&root), &json!({ tk.clone(): link })).unwrap();
    let start = std::sync::Barrier::new(WRITERS);
    std::thread::scope(|s| {
        for i in 0..WRITERS {
            let (root, start) = (&root, &start);
            s.spawn(move || {
                start.wait();
                let posted = json!([{ (wire::SEQ): 1, (wire::KEY): format!("reply:{i}") }]);
                mark_posted(root, FILE, Some(&posted)).unwrap();
            });
        }
    });
    let threads = read_json(&threads_file(&root), None).unwrap();
    assert_eq!(count(&threads[&tk], wire::link::POSTED), WRITERS);
}
