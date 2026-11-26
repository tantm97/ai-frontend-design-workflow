//! The import cases of tests/unit/penpot-screens.test.mjs, penpot-components.test.mjs and
//! penpot-hardening.test.mjs.

use super::*;
use crate::core::fixture::{BUTTON_MASTER, ITEMS_PAGE, OUTSIDE_JSON};
use crate::core::js;
use crate::design::tokens::dir_names;
use crate::penpot::export::tests::{
    design_hash, draft, fixture_export, project, read, with_export,
};
use crate::penpot::tokens::{key, penpot_type, BASE_SET, ENABLED};
use crate::penpot::wire;
use crate::review::comments::all_comments;

/// The archive of a change file named `name` (`<name>.json`).
fn archive_file(name: &str) -> String {
    format!("{name}.{}", file::JSON_EXT)
}

fn frozen() -> Value {
    json!({ "version": 2, "status": "approved", "locked": true })
}

fn run(root: &Path, changes: &Value, state: Option<&Value>, dry_run: bool) -> ChangeImport {
    import_changes(
        root,
        &mut changes.clone(),
        ImportOptions {
            dry_run,
            state,
            live: false,
        },
    )
    .unwrap()
}

fn text(c: &Value) -> &str {
    c["comment"].as_str().unwrap()
}

fn by_id<'a>(list: &'a [Value], id: &str) -> &'a Value {
    list.iter()
        .find(|c| c[comment::TARGET]["dfwId"] == id)
        .unwrap()
}

/// penpot import: writes feedback comments (provider penpot) and a token diff, never touches design/
#[test]
fn writes_feedback_comments_and_a_token_diff() {
    let root = project("penpot-import", draft());
    let changes = with_export(&root, &fixture_export(), "changes.json");
    let before = design_hash(&root);
    let state = js::read_json(&root.join(rel::STATE), None).unwrap();
    let r = run(&root, &changes, Some(&state), false);
    assert_eq!(design_hash(&root), before);
    let list = all_comments(&root).unwrap();
    assert!(list.len() >= 5);
    assert!(list
        .iter()
        .all(|c| c["provider"] == "penpot" && c["status"] == "open"));
    let add = by_id(&list, "items.add");
    assert!(text(add).contains("Text changed: \"Add item\" -> \"Add stock\""));
    assert_eq!(add["file"], rel::screen(ITEMS_PAGE));
    assert_eq!(add["breakpoint"], "desktop");
    assert_eq!(
        add[comment::TARGET]["selector"],
        "[data-dfw-id=\"items.add\"]"
    );
    let source = add[comment::TARGET][comment::SOURCE].as_str().unwrap();
    let line = source
        .strip_prefix(&format!("{}:", rel::screen(ITEMS_PAGE)))
        .unwrap();
    assert!(line.parse::<u32>().is_ok(), "{source}");
    let panel = text(by_id(&list, "items.panel"));
    assert!(panel.contains("Moved by 0, 24 px"));
    assert!(panel.contains("Colour changed: #ffffff (color.surface) -> #f7f7f8 (color.canvas)"));
    assert!(panel.contains("Added in Penpot: text \"Last audit\""));
    assert!(text(by_id(&list, "items.note")).contains("Removed in Penpot"));
    let thread = list
        .iter()
        .find(|c| text(c).starts_with(comment::THREAD_HEADER))
        .unwrap();
    assert!(text(thread).contains("Designer: Can low stock be more visible?\nLead: Maybe a badge."));
    assert!(
        !list.iter().any(|c| text(c).contains("Old note")),
        "resolved Penpot threads are not imported"
    );
    let tok = list
        .iter()
        .find(|c| text(c).contains("Token values changed"))
        .unwrap();
    assert!(text(tok).contains("base color.blue.600"));
    assert_eq!(r.token_diff.unwrap().diff[0].1.changed.len(), 1);
    assert!(root
        .join(rel::join(rel::PENPOT, file::PENPOT_TOKENS_IMPORTED))
        .exists());
    // Importing the same file again adds nothing.
    let again = run(&root, &changes, None, false);
    assert_eq!(again.comments.len(), 0);
    assert_eq!(all_comments(&root).unwrap().len(), list.len());
}

/// penpot import: frozen design -> comments only, with the gate noted; dry run writes nothing
#[test]
fn frozen_design_comments_only_and_a_dry_run_writes_nothing() {
    let root = project("penpot-frozen", frozen());
    let changes = with_export(&root, &fixture_export(), "changes.json");
    let state = json!({ "design": { "version": 2, "locked": true } });
    let dry = run(&root, &changes, Some(&state), true);
    assert!(!dry.comments.is_empty());
    assert!(dry.comments.iter().all(|c| c["id"] == "(dry run)"));
    assert_eq!(all_comments(&root).unwrap().len(), 0);
    assert!(!root
        .join(rel::join(rel::PENPOT, file::PENPOT_TOKENS_IMPORTED))
        .exists());
    assert!(!root
        .join(rel::join(rel::PENPOT, file::PENPOT_IMPORTED))
        .exists());
    let leftovers: Vec<String> = dir_names(&root.join(rel::PENPOT))
        .unwrap()
        .into_iter()
        .filter(|f| f.starts_with(".tokens-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
    let r = run(&root, &changes, Some(&state), false);
    assert!(r.frozen);
    assert!(all_comments(&root)
        .unwrap()
        .iter()
        .all(|c| text(c)
            .contains("frozen: discuss with the user; applying this needs \"dfw reopen\"")));
    assert!(format_import_changes(&r, false).contains("The design is frozen (G2)"));
}

/// penpot import: a dry run whose token comparison fails leaves no temporary file behind
#[test]
fn a_failed_dry_run_token_diff_leaves_no_temporary_file() {
    let root = project("penpot-dry-fail", draft());
    let mut changes = with_export(&root, &fixture_export(), "changes.json");
    changes["tokens"] = json!({ "sets": [{ "name": "base", "tokens": [
        { "name": "shadow.card", "type": "boxShadow", "value": [null] }
    ] }] });
    let dry = run(&root, &changes, None, true);
    assert!(
        dry.warnings
            .iter()
            .any(|w| w.starts_with("Token diff skipped: A shadow token")),
        "{:?}",
        dry.warnings
    );
    let leftovers: Vec<String> = dir_names(&root.join(rel::PENPOT))
        .unwrap()
        .into_iter()
        .filter(|f| f.starts_with(".tokens-"))
        .collect();
    assert!(leftovers.is_empty(), "{leftovers:?}");
}

/// penpot import: plugin token sets become a Penpot token file
#[test]
fn plugin_token_sets_become_a_penpot_token_file() {
    let doc = tokens_from_plugin(read("changes.json").get("tokens"))
        .unwrap()
        .unwrap();
    assert_eq!(
        to_json_compact(&doc[BASE_SET]["color"]["accent"]),
        r#"{"$value":"{color.blue.600}","$type":"color"}"#
    );
    assert_eq!(doc[key::METADATA]["tokenSetOrder"], json!([BASE_SET]));
    assert_eq!(
        doc[key::THEMES][0]["selectedTokenSets"],
        json!({ BASE_SET: ENABLED })
    );
    assert_eq!(tokens_from_plugin(Some(&Value::Null)).unwrap(), None);
    assert_eq!(tokens_from_plugin(None).unwrap(), None);
    // Lists the Node code could not walk are refused, as there.
    /// Sets given as text, not as a list.
    const TEXT_SETS: &str = "abc";
    for hostile in [
        json!({ "sets": [null] }),
        json!({ "sets": TEXT_SETS }),
        json!({ "sets": [{ "name": BASE_SET, "tokens": 5 }] }),
        json!({ "sets": [{ "name": BASE_SET }], "themes": [null] }),
    ] {
        let err = tokens_from_plugin(Some(&hostile)).unwrap_err();
        // A message that says what is wrong and what to do, not a JavaScript TypeError.
        assert!(
            matches!(err, DfwError::User { .. }) && err.message().contains("dfw bridge plugin"),
            "{hostile}: {}",
            err.message()
        );
    }
    assert_eq!(
        tokens_from_plugin(Some(&json!({ "sets": TEXT_SETS }))).unwrap_err().message(),
        "The change file's \"tokens.sets\" is text, not a list of token sets. Send the changes again from the dfw bridge plugin (\"Send changes to dfw\"), or fix the file."
    );
}

/// penpot tokens: "__proto__" in a token or set name never reaches Object.prototype
/// (the plugin half; the token file half is in tokens/tests.rs)
#[test]
fn reserved_set_and_token_names_from_the_plugin_are_left_out() {
    let doc = tokens_from_plugin(Some(&json!({ "sets": [
        { "name": BASE_SET, "tokens": [{ "name": "__proto__.polluted", "type": penpot_type::COLOR, "value": "#fff" }, { "name": "color.ok", "type": penpot_type::COLOR, "value": "#000" }] },
        { "name": "__proto__", "tokens": [] },
        { "name": "constructor", "tokens": [] },
    ] })))
    .unwrap()
    .unwrap();
    assert_eq!(
        to_json_compact(&doc[BASE_SET]),
        r##"{"color":{"ok":{"$value":"#000","$type":"color"}}}"##
    );
    assert_eq!(doc[key::METADATA]["tokenSetOrder"], json!([BASE_SET]));
    let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
    assert_eq!(keys, [BASE_SET, key::THEMES, key::METADATA]);
}

/// penpot components: import writes master comments (target.component, source, note), never touches design/, idempotent
#[test]
fn import_writes_master_comments() {
    let root = project("penpot-masters", draft());
    let changes = with_export(&root, &fixture_export(), "changes-components.json");
    let before = design_hash(&root);
    let state = json!({ "design": { "version": 1, "locked": false } });
    let r = run(&root, &changes, Some(&state), false);
    assert_eq!(design_hash(&root), before);
    let list = all_comments(&root).unwrap();
    // primary root, primary text, ghost root, ghost label, Penpot comment
    assert_eq!(list.len(), 5);
    assert!(list.iter().all(|c| c["provider"] == "penpot"
        && c["file"] == rel::component(BUTTON_MASTER)
        && c["screen"].is_null()));
    for c in &list {
        let primary = format!("Change the master {} (component \"button\", variant \"primary\"): it affects every screen using it", rel::component(BUTTON_MASTER));
        assert!(
            text(c).contains(&primary) || text(c).contains(&primary.replace("primary", "ghost")),
            "{}",
            text(c)
        );
    }
    let layer = |path: &str| {
        list.iter()
            .find(|c| c[comment::TARGET]["layer"] == path)
            .unwrap()
    };
    let root1 = layer("0.1");
    assert_eq!(
        to_json_compact(&root1[comment::TARGET]["component"]),
        r#"{"name":"button","variant":"primary","size":null}"#
    );
    let button = rel::component(BUTTON_MASTER);
    assert_eq!(
        root1[comment::TARGET][comment::SOURCE],
        format!("{button}:4")
    );
    let body = text(root1);
    assert!(body.starts_with("[Penpot master] button / primary:"));
    assert!(body.contains("Fill changed: #0066ff (color.accent) -> #1c1b1a (color.ink)"));
    assert!(body.contains("Corner radius changed: 6px 6px 6px 6px (radius.md) -> 8px 8px 8px 8px (no radius token has this value"));
    assert!(body.contains("Size changed: 97x40 -> 97x48 px"));
    assert!(body.contains("Spacing (padding) changed: top 11px -> 16px (space.4)"));
    assert!(body.contains("Added in Penpot inside \"button / primary\": rect \"icon\""));
    // other rules than the target's
    assert!(body.contains(&format!("[{button}:3]")));
    let label = layer("0.1.0");
    assert!(text(label).contains("Example text changed: \"Add item\" -> \"Add to stock\""));
    assert_eq!(
        label[comment::TARGET][comment::SOURCE],
        format!("{button}:9")
    );
    assert_eq!(label[comment::TARGET]["text"], "Add item");
    let ghost = text(layer("0.2"));
    assert!(ghost.contains("Component renamed \"button / ghost\" -> \"button / outline\""));
    assert!(ghost.contains("Border changed: 1px #e4e4e7 (color.border) -> 1px #1c1b1a (color.ink)"));
    let spacing = ghost
        .lines()
        .find(|l| l.contains("left 17px -> 9px (space.2)"));
    assert!(spacing.is_some_and(|l| l.contains("1px border")));
    let thread = list
        .iter()
        .find(|c| text(c).starts_with("[Penpot comment on master button / primary]"))
        .unwrap();
    assert!(text(thread).contains("Designer: Primary should feel heavier."));
    let summary = format_import_changes(&r, false);
    assert!(summary.contains("1 component fill") && summary.contains("master -> button/primary"));
    // Re-sending the same changes adds nothing; a new edit on one element adds one comment.
    assert_eq!(run(&root, &changes, None, false).comments.len(), 0);
    assert_eq!(all_comments(&root).unwrap().len(), 5);
    let mut more = changes.clone();
    more[wire::COMPONENTS][0][wire::LAYERS][1]["characters"] = "Add".into();
    let again = run(&root, &more, None, false);
    assert_eq!(again.comments.len(), 1);
    assert!(text(&again.comments[0]).contains("\"Add item\" -> \"Add\""));
}

/// penpot components: frozen design -> master comments say dfw reopen is needed
#[test]
fn frozen_design_master_comments_say_reopen_is_needed() {
    let root = project("penpot-masters-frozen", frozen());
    let changes = with_export(&root, &fixture_export(), "changes-components.json");
    let state = json!({ "design": { "version": 2, "locked": true } });
    let r = run(&root, &changes, Some(&state), false);
    assert!(r.frozen);
    assert_eq!(r.comments.len(), 5);
    let list = all_comments(&root).unwrap();
    assert!(list
        .iter()
        .all(|c| text(c).contains("Change the master") || text(c).starts_with("[Penpot comment")));
    assert!(list
        .iter()
        .all(|c| text(c)
            .contains("frozen: discuss with the user; applying this needs \"dfw reopen\"")));
}

/// penpot import: a comment thread is imported once, even after the layer under it moved
#[test]
fn a_comment_thread_is_imported_once() {
    /// A reply the designer adds to the thread.
    const AGREED: &str = "Agreed.";
    let root = project("penpot-thread-once", draft());
    let changes = with_export(&root, &fixture_export(), "changes.json");
    run(&root, &changes, None, false);
    let threads = || {
        all_comments(&root)
            .unwrap()
            .iter()
            .filter(|c| text(c).starts_with(comment::THREAD_HEADER))
            .count()
    };
    assert_eq!(threads(), 1);
    let mut again = changes.clone();
    let nudge = |v: &mut Value| v["x"] = json!(v["x"].as_f64().unwrap() + 3.0);
    for b in again[wire::BOARDS].as_array_mut().unwrap() {
        b[wire::LAYERS]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .for_each(nudge);
    }
    again[wire::COMMENTS]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .for_each(nudge);
    run(&root, &again, None, false);
    assert_eq!(
        threads(),
        1,
        "same thread, same messages: not a new comment"
    );
    // A reply in the thread continues the same dfw comment.
    again[wire::COMMENTS][0][wire::COMMENTS]
        .as_array_mut()
        .unwrap()
        .push(json!({ "content": AGREED, "user": "Designer" }));
    run(&root, &again, None, false);
    assert_eq!(threads(), 1);
    let list = all_comments(&root).unwrap();
    let c = list
        .iter()
        .find(|c| text(c).starts_with(comment::THREAD_HEADER))
        .unwrap();
    let last = c["thread"].as_array().unwrap().last().unwrap();
    assert_eq!(
        (&last["by"], &last["message"]),
        (&json!("user"), &json!(AGREED))
    );
}

/// penpot import: exportId cannot name a file outside .dfw/penpot/exports
#[test]
fn export_id_cannot_name_a_file_outside_the_exports() {
    let root = project("penpot-export-id", draft());
    let doc = fixture_export();
    let mut changes = with_export(&root, &doc, "changes.json");
    let mut evil = doc.clone();
    evil["id"] = "evil".into();
    fs::write(root.join(OUTSIDE_JSON), to_json_compact(&evil)).unwrap();
    // From .dfw/penpot/exports up to the project.
    changes[wire::EXPORT_ID] =
        format!("../../../{}", OUTSIDE_JSON.trim_end_matches(".json")).into();
    let r = run(&root, &changes, None, true);
    assert_eq!(
        r.baseline,
        Some(doc["id"].clone()),
        "falls back to the latest export"
    );
    for id in ["px_", "px_a/b", "px_a.b", "PX_1", "px_1\n", " px_1"] {
        changes[wire::EXPORT_ID] = id.into();
        assert_eq!(
            run(&root, &changes, None, true).baseline,
            Some(doc["id"].clone()),
            "{id:?}"
        );
    }
}

/// penpot import: a non-string pushedAt does not fail after the comments were written
#[test]
fn a_non_string_pushed_at_does_not_fail() {
    let root = project("penpot-pushed-at", draft());
    let mut changes = with_export(&root, &fixture_export(), "changes.json");
    /// A push time given as a number (milliseconds), and its archive.
    const STAMP: &str = "1731750000000";
    changes[wire::PUSHED_AT] = json!(1731750000000u64);
    let r = run(&root, &changes, None, false);
    assert!(r.comments.len() >= 5);
    let archive = root.join(rel::join(rel::PENPOT, file::PENPOT_CHANGES));
    assert_eq!(dir_names(&archive).unwrap(), [archive_file(STAMP)]);
    changes[wire::PUSHED_AT] = "../../x".into();
    let r2 = run(&root, &changes, None, false);
    assert_eq!(r2.comments.len(), 0);
    // Archive names stay inside changes/.
    assert_eq!(
        dir_names(&archive).unwrap(),
        [archive_file(STAMP), archive_file(UNSTAMPED)]
    );
}

/// §0 row 16: two change files pushed at the same time (or without a time) are both kept; the
/// second archive never replaces the first (`contracts.md` C9).
#[test]
fn two_imports_with_one_push_time_keep_both_archives() {
    /// A push time, and its archive name.
    const PUSHED: &str = "2026-11-16T10:00:00Z";
    const STAMP: &str = "20261116T100000Z";
    /// A note that tells the two change files apart.
    const NOTE: &str = "note";
    let root = project("penpot-archive-twice", draft());
    let mut changes = with_export(&root, &fixture_export(), "changes.json");
    changes[wire::PUSHED_AT] = PUSHED.into();
    let archive = root.join(rel::join(rel::PENPOT, file::PENPOT_CHANGES));
    for n in 1..=3 {
        changes[NOTE] = n.into();
        run(&root, &changes, None, false);
    }
    let second = format!("{STAMP}-2");
    let third = format!("{STAMP}-3");
    let names = [
        archive_file(STAMP),
        archive_file(&second),
        archive_file(&third),
    ];
    let mut sorted = names.to_vec();
    sorted.sort();
    assert_eq!(dir_names(&archive).unwrap(), sorted);
    for (n, name) in names.iter().enumerate() {
        let kept = read_json(&archive.join(name), None).unwrap();
        assert_eq!(kept[NOTE], n + 1, "{name}");
    }
}
