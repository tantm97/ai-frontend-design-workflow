//! The diff cases of tests/unit/penpot-screens.test.mjs and penpot-components.test.mjs.

use super::*;
use crate::core::fixture::BUTTON_MASTER;
use crate::core::js;
use crate::penpot::export::tests::{draft, fixture_export, project, read};
use crate::penpot::wire;

fn dims() -> Vec<Dim> {
    dimension_token_index(Some(&read("project/design/tokens/tokens.json")))
}

fn changes_for(doc: &Value, file: &str) -> Value {
    let mut changes = read(file);
    changes["exportId"] = doc["id"].clone();
    changes
}

fn sorted(mut list: Vec<String>) -> Vec<String> {
    list.sort();
    list
}

/// The keys of the two button masters of the fixture.
const PRIMARY_KEY: &str = "button|primary|";
const GHOST_KEY: &str = "button|ghost|";
const PRIMARY: &str = "primary";
const GHOST: &str = "ghost";
/// The board of the fixture's items screen.
const BOARD: &str = "items|a|desktop";
const INK: &str = "color.ink";
/// The tag of a text layer.
const SPAN: &str = "span";

fn compact(v: &Value) -> String {
    crate::core::util::to_json_compact(v)
}

/// penpot import: change set maps layers to data-dfw-id (text, move, fill, removed, added, comments)
#[test]
fn change_set_maps_layers_to_ids() {
    let doc = fixture_export();
    let diff = diff_changes(&doc, &mut changes_for(&doc, "changes.json"), &[]).unwrap();
    assert!(diff.warnings.is_empty(), "{:?}", diff.warnings);
    let changes = diff.changes;
    let brief = sorted(
        changes
            .iter()
            .map(|c| {
                let anchor = js::truthy_value(c.get("anchor"))
                    .map_or("-".into(), |a| js::to_string(Some(a)));
                format!("{}:{anchor}", js::to_string(c.get(field::TYPE)))
            })
            .collect(),
    );
    assert_eq!(
        brief,
        [
            "added:items.panel",
            "comment:items.summary",
            "comment:items.title",
            "fill:items.panel",
            "move:items.panel",
            "removed:items.note",
            "text:items.add"
        ]
    );
    let of = |ty: &str| changes.iter().find(|c| c[field::TYPE] == ty).unwrap();
    let text = of(change_type::TEXT);
    assert_eq!(
        compact(&json!([
            text[field::FROM],
            text[field::TO],
            text["screen"],
            text["variant"],
            text["breakpoint"],
            text["file"]
        ])),
        r#"["Add item","Add stock","items","a","desktop","design/screens/items/a.html"]"#
    );
    let fill = of(change_type::FILL);
    assert_eq!(fill[field::FROM][field::TOKEN], "color.surface");
    assert_eq!(fill[field::TO][field::TOKEN], "color.canvas");
    assert_eq!(
        compact(&of(change_type::MOVE)["delta"]),
        r#"{"x":0,"y":24,"w":0,"h":0}"#
    );
    assert_eq!(
        of(change_type::ADDED)["characters"],
        "Last audit: 2 days ago"
    );
    let open = changes
        .iter()
        .find(|c| c[field::TYPE] == change_type::COMMENT && c["resolved"] == false)
        .unwrap();
    let authors: Vec<&Value> = open["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| &m["author"])
        .collect();
    assert_eq!(authors, ["Designer", "Lead"]);
    let err = diff_changes(&doc, &mut json!({ "schema": "x" }), &[]).unwrap_err();
    assert!(err.message().contains("Not a dfw Penpot change file"));
}

/// penpot components: dimension tokens by role (radius, space), font sizes left out
#[test]
fn dimension_tokens_by_role() {
    let brief = sorted(
        dims()
            .iter()
            .map(|d| format!("{}:{}={}", d.role, d.name, d.px))
            .collect(),
    );
    assert_eq!(
        brief,
        ["radius:radius.md=6", "space:space.2=8", "space:space.4=16"]
    );
}

/// penpot components: main component edits map to the master, variant and element, with tokens
#[test]
fn main_component_edits_map_to_the_master() {
    let doc = fixture_export();
    let diff = diff_changes(
        &doc,
        &mut changes_for(&doc, "changes-components.json"),
        &dims(),
    )
    .unwrap();
    assert!(diff.warnings.is_empty(), "{:?}", diff.warnings);
    let changes = diff.changes;
    let brief = sorted(
        changes
            .iter()
            .map(|c| {
                let path =
                    js::present(c.get("path")).map_or("-".into(), |p| js::to_string(Some(p)));
                format!(
                    "{}:{}:{path}",
                    js::to_string(c.get(field::TYPE)),
                    js::to_string(c[field::COMPONENT].get(wire::KEY))
                )
            })
            .collect(),
    );
    assert_eq!(
        brief,
        sorted(
            [
                "added:button|primary|:0.1",
                "comment:button|primary|:-",
                "fill:button|primary|:0.1",
                "radius:button|primary|:0.1",
                "rename:button|ghost|:0.2",
                "rename:button|ghost|:0.2.0",
                "size:button|primary|:0.1",
                "spacing:button|ghost|:0.2",
                "spacing:button|primary|:0.1",
                "stroke:button|ghost|:0.2",
                "text:button|primary|:0.1.0",
            ]
            .map(str::to_string)
            .to_vec()
        )
    );
    let pick = |ty: &str, key: &str| {
        changes
            .iter()
            .find(|c| c[field::TYPE] == ty && c[field::COMPONENT][wire::KEY] == key)
            .unwrap()
    };
    let fill = pick(change_type::FILL, PRIMARY_KEY);
    assert_eq!(
        compact(&fill[field::COMPONENT]),
        r#"{"key":"button|primary|","name":"button","variant":"primary","size":null,"title":"button / primary"}"#
    );
    assert_eq!(fill[field::FROM][field::TOKEN], "color.accent");
    assert_eq!(fill[field::TO][field::TOKEN], INK);
    let stroke = pick(change_type::STROKE, GHOST_KEY);
    assert_eq!(stroke[field::FROM][field::TOKEN], "color.border");
    assert_eq!(stroke[field::TO][field::TOKEN], INK);
    // space.2 is 8px but not a radius
    assert_eq!(
        compact(&pick(change_type::RADIUS, PRIMARY_KEY)[field::TO]),
        r#"{"radius":[8,8,8,8],"token":null}"#
    );
    assert_eq!(
        compact(&pick(change_type::SIZE, PRIMARY_KEY)[field::TO]),
        "[97.14,48]"
    );
    assert_eq!(
        compact(&pick(change_type::SPACING, PRIMARY_KEY)[field::SIDES]),
        r#"{"top":{"from":11,"to":16,"token":"space.4"}}"#
    );
    // Ghost has a 1px border: a 9px inset is space.2 + the border.
    assert_eq!(
        compact(&pick(change_type::SPACING, GHOST_KEY)[field::SIDES]),
        r#"{"left":{"from":17,"to":9,"token":"space.2"}}"#
    );
    let text = pick(change_type::TEXT, PRIMARY_KEY);
    assert_eq!(
        (&text[field::FROM], &text[field::TO]),
        (&json!("Add item"), &json!("Add to stock"))
    );
    let renames: Vec<Value> = changes
        .iter()
        .filter(|c| c[field::TYPE] == change_type::RENAME)
        .map(|c| json!([c["isRoot"], c[field::FROM], c[field::TO]]))
        .collect();
    assert_eq!(
        compact(&json!(renames)),
        r#"[[true,"button / ghost","button / outline"],[false,"text","label"]]"#
    );
    assert_eq!(pick(change_type::ADDED, PRIMARY_KEY)["layer"], "icon");
    // No screen board was touched: no screen changes.
    assert!(changes
        .iter()
        .all(|c| c["board"].is_null() && c["screen"].is_null()));
}

/// penpot components: removed layers and deleted main components
#[test]
fn removed_layers_and_deleted_main_components() {
    let doc = fixture_export();
    let mut ch = read("changes-components.json");
    let mut ghost = ch[wire::COMPONENTS][1].clone();
    ghost[wire::LAYERS] = json!([ghost[wire::LAYERS][0]]);
    ch[wire::COMPONENTS] = json!([ghost]);
    ch[wire::MISSING_COMPONENTS] = json!([PRIMARY_KEY]);
    let changes = diff_changes(&doc, &mut ch.clone(), &dims())
        .unwrap()
        .changes;
    assert!(changes
        .iter()
        .any(|c| c[field::TYPE] == change_type::REMOVED
            && c["path"] == "0.2.0"
            && c["text"] == "Export"));
    assert!(changes
        .iter()
        .any(|c| c[field::TYPE] == change_type::COMPONENT_REMOVED
            && c[field::COMPONENT][wire::KEY] == PRIMARY_KEY));
    // Unknown keys are reported, not guessed.
    ch[wire::COMPONENTS] = json!([{ "key": "card||", "layers": [] }]);
    ch[wire::MISSING_COMPONENTS] = json!([]);
    let warnings = diff_changes(&doc, &mut ch, &[]).unwrap().warnings;
    assert!(warnings[0].contains("Component \"card||\" is not in export"));
}

/// penpot components: source line = CSS rule of the variant, base rule, or template element
#[test]
fn source_line_of_a_master_change() {
    let root = project("penpot-source", draft());
    let comp = |name: &str, variant: &str| json!({ "key": PRIMARY_KEY, "name": name, "variant": variant, "size": null, "title": "button / primary" });
    let src = |mut change: Value, variant: &str| {
        change[field::COMPONENT] = comp("button", variant);
        master_source(&root, &change, None).unwrap().source.unwrap()
    };
    let at = |line: u32| format!("{}:{line}", rel::component(BUTTON_MASTER));
    // .btn--primary background
    assert_eq!(
        src(
            json!({ "type": change_type::FILL, "kind": wire::kind::FRAME, "isRoot": true }),
            PRIMARY
        ),
        at(4)
    );
    // .btn border-radius
    assert_eq!(
        src(
            json!({ "type": change_type::RADIUS, "kind": wire::kind::FRAME, "isRoot": true }),
            PRIMARY
        ),
        at(3)
    );
    assert_eq!(
        src(
            json!({ "type": change_type::SPACING, "kind": wire::kind::FRAME, "isRoot": true }),
            PRIMARY
        ),
        at(3)
    );
    // not the :focus-visible outline
    assert_eq!(
        src(
            json!({ "type": change_type::STROKE, "kind": wire::kind::FRAME, "isRoot": true }),
            GHOST
        ),
        at(5)
    );
    // <dfw-slot>
    assert_eq!(
        src(
            json!({ "type": change_type::TEXT, "kind": wire::kind::TEXT, "tag": SPAN }),
            PRIMARY
        ),
        at(9)
    );
    assert_eq!(
        src(
            json!({ "type": change_type::RENAME, "kind": wire::kind::TEXT, "tag": SPAN, "name": "text \"Save\"" }),
            PRIMARY
        ),
        at(9)
    );
    let missing = master_source(
        &root,
        &json!({ "component": comp("nope", PRIMARY), "type": change_type::FILL }),
        None,
    );
    assert_eq!(missing.unwrap().source, None);
    assert_eq!(
        master_source(&root, &json!({ "type": change_type::FILL }), None),
        None
    );
}

/// A layer the export does not know moves to the board's `added` list of the change file.
#[test]
fn unknown_layers_are_added_to_the_change_file() {
    let doc = fixture_export();
    let mut changes = changes_for(&doc, "changes.json");
    let extra =
        json!({ "path": "9.9", "name": GHOST, "kind": "rect", "x": 20, "y": 140, "w": 4, "h": 4 });
    changes[wire::BOARDS][0][wire::LAYERS]
        .as_array_mut()
        .unwrap()
        .push(extra.clone());
    let diff = diff_changes(&doc, &mut changes, &[]).unwrap();
    assert!(diff.warnings[0].contains("layer path 9.9 unknown in the export; treated as added."));
    assert_eq!(
        changes[wire::BOARDS][0][wire::ADDED]
            .as_array()
            .unwrap()
            .last(),
        Some(&extra)
    );
    assert!(diff
        .changes
        .iter()
        .any(|c| c[field::TYPE] == change_type::ADDED
            && c["name"] == GHOST
            && c["anchor"] == "items.panel"));
}

/// Values of the wrong type in a change file are read as JavaScript read them, without a panic;
/// lists the Node code could not walk (it threw a TypeError) are refused.
#[test]
fn a_change_file_with_wrong_types_does_not_panic() {
    let doc = fixture_export();
    let hostile = json!({
        "schema": "dfw-penpot-changes/1",
        "boards": [5, { "key": BOARD, "layers": [{ "path": 7 }, { "path": "0", "x": "a", "fills": "red", "name": {} }], "added": "" }, { "key": {} }],
        "missingBoards": BOARD,
        "components": [{ "key": PRIMARY_KEY, "layers": [{ "root": true, "radius": ["a", null], "strokes": [null], "fills": 3 }], "added": [7] }],
        "missingComponents": [null, 1],
        "comments": [{ "boardKey": BOARD, "x": "1", "comments": [{ "content": 5 }] }],
    });
    let diff = diff_changes(&doc, &mut hostile.clone(), &dims()).unwrap();
    assert!(!diff.changes.is_empty());
    for c in &diff.changes {
        let line = if c.get(field::COMPONENT).is_some_and(|c| !c.is_null()) {
            describe_component_change(c)
        } else {
            describe_change(c)
        };
        assert!(c[field::TYPE] == change_type::COMMENT || !line.is_empty());
    }
    const BOARDS: &str = "/boards";
    const BOARD_ADDED: &str = "/boards/1/added";
    const MESSAGES: &str = "/comments/0/comments";
    let refused = |pointer: &str, value: Value| {
        let mut changes = hostile.clone();
        *changes.pointer_mut(pointer).unwrap() = value;
        diff_changes(&doc, &mut changes, &dims()).is_err()
    };
    assert!(refused(BOARDS, json!([null])));
    assert!(refused(BOARDS, json!({ "a": 1 })));
    assert!(refused("/boards/1/layers", json!([null])));
    assert!(refused(BOARD_ADDED, json!({ "a": 1 })));
    // An unknown layer is pushed onto `added`: a string has no push.
    assert!(refused(BOARD_ADDED, json!("x")));
    assert!(refused("/comments", json!([null])));
    assert!(refused(MESSAGES, json!("text")));
    assert!(refused(MESSAGES, json!([null])));
    assert!(refused("/components", json!(7)));
    assert!(refused("/components/0/layers", json!([null])));
    assert!(refused("/components/0/added", json!([null])));
    assert!(refused("/missingComponents", json!(true)));
    // A board the export does not know is skipped before its lists are read.
    assert!(!refused(
        "/boards/2",
        json!({ "key": "nope", "layers": [null] })
    ));
}

/// Coordinates sent as text are joined before they are compared, as `+` does in JavaScript.
#[test]
fn coordinates_sent_as_text() {
    // "20" + 2 is "202"; 20 + 2 is 22.
    assert_eq!(
        centre(&json!({ "x": "20", "y": 20, "w": 4, "h": 4 })),
        (202.0, 22.0)
    );
    assert!(centre(&json!({ "x": 1, "y": {}, "w": {} })).0.is_nan());
    assert!(contains(
        &json!({ "x": 0, "y": 0, "w": 10, "h": 10 }),
        5.0,
        5.0
    ));
    // x <= "0" + "10" = 10 holds, x <= "0" + 3 = 3 does not.
    assert!(contains(
        &json!({ "x": "0", "y": 0, "w": "10", "h": 10 }),
        5.0,
        5.0
    ));
    assert!(!contains(
        &json!({ "x": "0", "y": 0, "w": 3, "h": 10 }),
        5.0,
        5.0
    ));
}

/// A coordinate that is not a number: the sentence says NaN, as in Node.
#[test]
fn a_move_with_a_coordinate_that_is_not_a_number() {
    let doc = fixture_export();
    let mut changes = changes_for(&doc, "changes.json");
    changes[wire::BOARDS][0][wire::LAYERS] =
        json!([{ "path": "0.3", "x": "a", "y": 200, "w": 992, "h": 127 }]);
    let diff = diff_changes(&doc, &mut changes, &[]).unwrap();
    let moved = diff
        .changes
        .iter()
        .find(|c| c[field::TYPE] == change_type::MOVE)
        .unwrap();
    assert_eq!(compact(&moved["delta"]["x"]), "null");
    assert!(
        describe_change(moved).starts_with("Moved by NaN, 88 px (now x NaN, y 200)"),
        "{}",
        describe_change(moved)
    );
}
