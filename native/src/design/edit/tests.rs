//! The cases of `tests/unit/edit.test.mjs` and the edit cases of `audit-fixes.test.mjs`; each
//! test's doc comment is the title of the JS test it ports.

// Over 800 lines: one file, as the JS test file it ports, so coverage can be compared.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};

use super::layout::{parse_layout_css, render_layout};
use super::style::parse_style;
use super::*;
use crate::core::fixture::{
    index_page, variant_page, APP_PAGE, APP_SHELL_MASTER, BUTTON_MASTER, COMPOSED_PAGE, INDEX_PAGE,
    INDEX_PAGE_C, MANY_EDITS_PAGE, OUTSIDE_DIR_PAGE, SCREEN, SCREEN_PAGE,
};
use crate::core::paths::{file, rel};
use crate::core::util::test_dir;
use crate::design::compose::{compose_html, ComposeOptions};
use crate::design::html::open_tag_at;

const SCREEN_HTML: &str = r#"<!doctype html><html><head><style>.x[data-dfw-id="home.title"] { color: red }</style></head><body>
<!-- <h1 data-dfw-id="home.title">commented out</h1> -->
<main data-dfw-id="home.main" class="m" style="color: var(--color-ink); padding: 4px">
  <h1 data-dfw-id="home.title">
    Welcome back
  </h1>
  <p data-dfw-id="home.lede">Orders &amp; stock <b>today</b></p>
  <dfw-use c="button" variant="primary" id="home.cta" kbd="Enter">New order</dfw-use>
  <dfw-use c="button" id="home.more"><dfw-fill slot="default"><b>More</b></dfw-fill></dfw-use>
  <input data-dfw-id="home.search">
  <div data-dfw-id="dup">a</div><div data-dfw-id="dup">b</div>
</main>
<script>const s = '<p data-dfw-id="home.lede">x</p>';</script>
</body></html>"#;

const BUTTON: &str = r#"<dfw-component name="button" variants="primary secondary ghost" sizes="md lg" props="kbd">
<style>.btn{}</style>
<template><button class="btn btn--{{variant}}"><dfw-slot></dfw-slot><kbd data-dfw-if="kbd">{{kbd}}</kbd></button></template>
</dfw-component>"#;

const TOKENS: &str = r##"{"space":{"$type":"dimension","1":{"$value":{"value":4,"unit":"px"}},"4":{"$value":{"value":16,"unit":"px"}}},"color":{"$type":"color","ink":{"$value":"#111111"}}}"##;

const GRID: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <style>.cards { display: grid; grid-template-columns: repeat(4, 1fr); gap: var(--space-4) }</style>
</head>
<body>
  <main data-dfw-id="dash">
    <section class="cards" data-dfw-id="dash.cards">
      <div data-dfw-id="dash.a">A</div>
      <article data-dfw-id="dash.b">
        <h2 data-dfw-id="dash.b.title">B</h2>
      </article>
      <dfw-use c="button" id="dash.c">C</dfw-use>
    </section>
    <div class="row" data-dfw-id="dash.row"><span data-dfw-id="dash.x">x</span><span data-dfw-id="dash.y">y</span></div>
  </main>
</body>
</html>"#;

const HOME: &str = "screens/home/a.html";
const G: &str = "screens/home/grid.html";

fn write(root: &Path, rel: &str, text: &str) {
    let file = root.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

fn read(root: &Path, rel: &str) -> String {
    fs::read_to_string(root.join(rel)).unwrap()
}

fn project() -> PathBuf {
    let root = test_dir("edit");
    write(&root, &rel::screen(SCREEN_PAGE), SCREEN_HTML);
    write(&root, &rel::component(BUTTON_MASTER), BUTTON);
    write(&root, rel::TOKENS_JSON, TOKENS);
    write(&root, &rel::approved("v1/screens/home/a.html"), SCREEN_HTML);
    write(&root, APP_PAGE, SCREEN_HTML);
    root
}

fn grid_project() -> PathBuf {
    let root = project();
    write(&root, &rel::screen(&variant_page(SCREEN, "grid")), GRID);
    let mut tokens: Value = serde_json::from_str(TOKENS).unwrap();
    tokens["size"] =
        json!({ "$type": "dimension", "sidebar": { "$value": { "value": 240, "unit": "px" } } });
    write(&root, rel::TOKENS_JSON, &tokens.to_string());
    root
}

fn read_g(root: &Path) -> String {
    read(root, &rel::design(G))
}

/// `applyEdit(root, op)` that must succeed.
fn apply(root: &Path, op: Value) -> EditResult {
    apply_edit(root, &op).unwrap_or_else(|err| panic!("{op}: {err}"))
}

/// `assert.throws(() => applyEdit(root, op), pattern)`.
fn refused(root: &Path, op: Value, pattern: &str) {
    let err = apply_edit(root, &op).expect_err(&op.to_string());
    assert!(
        Regex::new(pattern).unwrap().is_match(err.message()),
        "{op}: {err}"
    );
}

fn assert_match(text: &str, pattern: &str) {
    assert!(
        Regex::new(pattern).unwrap().is_match(text),
        "/{pattern}/ in:\n{text}"
    );
}

/// edit text: replaces only the element text, keeps whitespace, escapes HTML
#[test]
fn text_replaces_only_the_element_text() {
    let root = project();
    let r = apply(
        &root,
        json!({ "file": HOME, "op": "text", "id": "home.title", "value": "Hi <you> & co" }),
    );
    assert!(r.changed);
    assert_eq!(r.file, rel::screen(SCREEN_PAGE));
    let after = read(&root, &rel::screen(SCREEN_PAGE));
    assert!(after.contains("<h1 data-dfw-id=\"home.title\">\n    Hi &lt;you&gt; &amp; co\n  </h1>"));
    assert!(after.contains("commented out"), "comment untouched");
    assert!(after.contains("color: red"), "style block untouched");
    assert_eq!(
        after.replace("Hi &lt;you&gt; &amp; co", "Welcome back"),
        SCREEN_HTML,
        "nothing else changed"
    );
}

/// edit text: component instance slot text via <dfw-use id>
#[test]
fn text_of_a_component_instance() {
    let root = project();
    apply(
        &root,
        json!({ "file": &rel::screen(SCREEN_PAGE), "op": "text", "id": "home.cta", "value": "Create order" }),
    );
    assert!(read(&root, &rel::screen(SCREEN_PAGE)).contains(
        r#"<dfw-use c="button" variant="primary" id="home.cta" kbd="Enter">Create order</dfw-use>"#
    ));
}

/// edit text: refusals (child elements, void, unknown, duplicate ids)
#[test]
fn text_refusals() {
    let root = project();
    let op =
        |id: &str, value: &str| json!({ "file": HOME, "op": "text", "id": id, "value": value });
    refused(&root, op("home.lede", "x"), "child elements");
    refused(&root, op("home.more", "x"), "child elements");
    refused(&root, op("home.search", "x"), "no text");
    refused(&root, op("nope", "x"), "not found");
    refused(&root, op("dup", "x"), "2 times");
    refused(&root, op("home.title", "   "), "empty");
    assert_eq!(
        read(&root, &rel::screen(SCREEN_PAGE)),
        SCREEN_HTML,
        "file unchanged after refusals"
    );
}

/// edit boundary: refuses design/approved, files outside design/, traversal and non-html
#[test]
fn boundary() {
    let root = project();
    let op = |file: &str, id: &str| json!({ "file": file, "op": "text", "id": id, "value": "x" });
    refused(
        &root,
        op("approved/v1/screens/home/a.html", "home.title"),
        "approved",
    );
    refused(
        &root,
        op(&rel::approved("v1/screens/home/a.html"), "home.title"),
        "approved",
    );
    refused(
        &root,
        op(&format!("../{APP_PAGE}"), "home.title"),
        "under design",
    );
    refused(&root, op("tokens/tokens.json", "x"), "HTML");
    assert_eq!(read(&root, APP_PAGE), SCREEN_HTML);
}

/// edit variant / size / prop on a component instance
#[test]
fn variant_size_prop() {
    let root = project();
    apply(
        &root,
        json!({ "file": HOME, "op": "variant", "id": "home.cta", "value": "secondary" }),
    );
    apply(
        &root,
        json!({ "file": HOME, "op": "size", "id": "home.cta", "value": "lg" }),
    );
    apply(
        &root,
        json!({ "file": HOME, "op": "prop", "id": "home.cta", "prop": "kbd", "value": "F2" }),
    );
    assert!(read(&root, &rel::screen(SCREEN_PAGE)).contains(
        r#"<dfw-use c="button" variant="secondary" id="home.cta" kbd="F2" size="lg">New order</dfw-use>"#
    ));
    apply(
        &root,
        json!({ "file": HOME, "op": "prop", "id": "home.cta", "prop": "kbd", "value": "" }),
    );
    assert!(read(&root, &rel::screen(SCREEN_PAGE)).contains(
        r#"<dfw-use c="button" variant="secondary" id="home.cta" size="lg">New order</dfw-use>"#
    ));
    refused(
        &root,
        json!({ "file": HOME, "op": "variant", "id": "home.cta", "value": "huge" }),
        "no variant \"huge\"",
    );
    refused(
        &root,
        json!({ "file": HOME, "op": "prop", "id": "home.cta", "prop": "onclick", "value": "x" }),
        "no prop",
    );
    refused(
        &root,
        json!({ "file": HOME, "op": "variant", "id": "home.title", "value": "primary" }),
        "not a component instance",
    );
}

/// edit spacing: sets/replaces inline style with space tokens only
#[test]
fn spacing() {
    let root = project();
    let op = |id: &str, prop: &str, value: &str| json!({ "file": HOME, "op": "spacing", "id": id, "prop": prop, "value": value });
    apply(&root, op("home.main", "padding", "space.4"));
    assert!(read(&root, &rel::screen(SCREEN_PAGE))
        .contains(r#"style="color: var(--color-ink); padding: var(--space-4)""#));
    apply(&root, op("home.main", "gap", "--space-1"));
    assert!(read(&root, &rel::screen(SCREEN_PAGE)).contains(
        r#"style="color: var(--color-ink); padding: var(--space-4); gap: var(--space-1)""#
    ));
    apply(&root, op("home.title", "margin-top", "space.1"));
    assert!(read(&root, &rel::screen(SCREEN_PAGE))
        .contains(r#"<h1 data-dfw-id="home.title" style="margin-top: var(--space-1)">"#));
    refused(
        &root,
        op("home.main", "padding", "color.ink"),
        r"space\.\* token",
    );
    refused(&root, op("home.main", "color", "space.1"), "not editable");
    refused(
        &root,
        op("home.cta", "padding", "space.1"),
        "component instance",
    );
}

/// undo restores edits in reverse order and refuses when the file changed since
#[test]
fn undo_restores_in_reverse_order() {
    let root = project();
    let text =
        |value: &str| json!({ "file": HOME, "op": "text", "id": "home.title", "value": value });
    apply(&root, text("One"));
    apply(&root, text("Two"));
    let mut recs: Vec<String> = fs::read_dir(root.join(rel::EDITS))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(recs.len(), 2);
    recs.sort();
    let rec: Value = serde_json::from_str(&read(&root, &rel::join(rel::EDITS, &recs[1]))).unwrap();
    let keys: Vec<&String> = rec.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["at", "file", "op", "before", "after"]);
    assert_eq!(undo_edit(&root).unwrap().remaining, 1);
    assert!(read(&root, &rel::screen(SCREEN_PAGE)).contains(">\n    One\n  <"));
    undo_edit(&root).unwrap();
    assert_eq!(read(&root, &rel::screen(SCREEN_PAGE)), SCREEN_HTML);
    assert_eq!(undo_edit(&root).unwrap_err().message(), "Nothing to undo.");

    apply(&root, text("Three"));
    let changed = read(&root, &rel::screen(SCREEN_PAGE)) + "\n<!-- edited by Claude -->";
    write(&root, &rel::screen(SCREEN_PAGE), &changed);
    assert!(undo_edit(&root)
        .unwrap_err()
        .message()
        .contains("changed after that edit"));
}

/// helpers: setAttr, parseStyle, setStyleProp
#[test]
fn helpers() {
    let set = |tag: &str, name: &str, value: &str| {
        let open = open_tag_at(tag, 0).unwrap();
        set_attr(tag, &open, name, Some(value)).unwrap().apply(tag)
    };
    assert_eq!(
        set("<a href=\"x\" data-q='1 > 2'>", "data-q", "y"),
        r#"<a href="x" data-q="y">"#
    );
    assert_eq!(
        set(r#"<img src="a.png"/>"#, "alt", r#"A "b""#),
        r#"<img src="a.png" alt="A &quot;b&quot;"/>"#
    );
    assert_eq!(
        set(r#"<p title="variant=x" class="c">"#, "variant", "v"),
        r#"<p title="variant=x" class="c" variant="v">"#,
        "does not match inside values"
    );
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        parse_style(r#"background: url("a;b"); color: red"#),
        [pair("background", r#"url("a;b")"#), pair("color", "red")]
    );
    assert_eq!(
        set_style_prop(
            "padding-top: 1px; padding: 2px; color: red",
            "padding",
            Some("var(--space-1)")
        ),
        "color: red; padding: var(--space-1)"
    );
    assert_eq!(set_style_prop("gap: 1px", "gap", None), "");
    let same = transform(
        &project(),
        SCREEN_HTML,
        &json!({ "op": "text", "id": "home.title", "value": "Welcome back" }),
    );
    assert_eq!(same.unwrap(), SCREEN_HTML);
}

/// move: reorders whole-line siblings by id and re-indents multi-line elements
#[test]
fn move_whole_lines() {
    let root = grid_project();
    let mv = |id: &str, target: &str, position: &str| json!({ "file": G, "op": "move", "id": id, "ref": target, "position": position });
    apply(&root, mv("dash.c", "dash.a", "before"));
    assert!(read_g(&root).contains("<section class=\"cards\" data-dfw-id=\"dash.cards\">\n      <dfw-use c=\"button\" id=\"dash.c\">C</dfw-use>\n      <div data-dfw-id=\"dash.a\">A</div>\n      <article"));
    apply(&root, mv("dash.b", "dash.c", "before"));
    assert!(read_g(&root).contains("cards\">\n      <article data-dfw-id=\"dash.b\">\n        <h2 data-dfw-id=\"dash.b.title\">B</h2>\n      </article>\n      <dfw-use c=\"button\" id=\"dash.c\">C</dfw-use>\n      <div data-dfw-id=\"dash.a\">A</div>\n    </section>"));
    // Back to the original order: the file is byte-identical.
    apply(&root, mv("dash.a", "dash.b", "before"));
    apply(&root, mv("dash.c", "dash.b", "after"));
    assert_eq!(read_g(&root), GRID);
    // Moving into another container re-indents to the new place.
    apply(&root, mv("dash.b", "dash.row", "after"));
    assert!(read_g(&root).contains("</div>\n    <article data-dfw-id=\"dash.b\">\n      <h2 data-dfw-id=\"dash.b.title\">B</h2>\n    </article>\n  </main>"));
}

/// move: inline siblings, no-op, and refusals
#[test]
fn move_inline_and_refusals() {
    let root = grid_project();
    let mv = |id: &str, target: &str, position: &str| json!({ "file": G, "op": "move", "id": id, "ref": target, "position": position });
    apply(&root, mv("dash.y", "dash.x", "before"));
    assert!(read_g(&root)
        .contains(r#"<span data-dfw-id="dash.y">y</span><span data-dfw-id="dash.x">x</span>"#));
    let same = apply(&root, mv("dash.y", "dash.x", "before"));
    assert!(!same.changed, "already there: idempotent");
    let before = read_g(&root);
    refused(&root, mv("dash.b", "dash.b.title", "after"), "into itself");
    refused(&root, mv("dash.b.title", "dash.b", "after"), "parent");
    refused(&root, mv("dash.a", "nope", "after"), "not found");
    refused(
        &root,
        mv("dash.a", "dash.b", "over"),
        "before.*after.*inside",
    );
    assert_eq!(read_g(&root), before);
}

fn layout(id: &str, prop: &str, value: &str) -> Value {
    json!({ "file": G, "op": "layout", "id": id, "prop": prop, "value": value })
}

fn layout_at(id: &str, prop: &str, value: Value, max_width: u32) -> Value {
    json!({ "file": G, "op": "layout", "id": id, "prop": prop, "value": value, "maxWidth": max_width })
}

/// layout: grid-column span rule in a dfw layout block; update, media placement, removal
#[test]
fn layout_block() {
    let root = grid_project();
    apply(&root, layout("dash.b", "grid-column", "span 2"));
    let t = read_g(&root);
    assert_match(
        &t,
        r" {2}<style data-dfw-layout>\n {4}/\* dfw Studio layout edits[^\n]*\*/\n {4}\[data-dfw-id='dash.b'\]:not\(#dfw\) \{ grid-column: span 2; \}\n {2}</style>\n</head>",
    );
    let block = Regex::new(r" {2}<style data-dfw-layout>[\s\S]*?</style>\n").unwrap();
    assert_eq!(block.replace(&t, ""), GRID, "nothing else changed");
    // Same value again: no change (idempotent).
    assert!(!apply(&root, layout("dash.b", "grid-column", "span 2")).changed);
    apply(&root, layout("dash.b", "grid-column", "3"));
    apply(
        &root,
        layout_at("dash.c", "grid-column", json!("span 1"), 834),
    );
    apply(
        &root,
        layout_at("dash.b", "grid-column", json!("span 1"), 390),
    );
    apply(
        &root,
        layout_at("dash.b", "grid-column", json!("span 2"), 834),
    );
    let t = read_g(&root);
    let block = Regex::new(r"<style data-dfw-layout>([\s\S]*?)</style>")
        .unwrap()
        .captures(&t)
        .unwrap()[1]
        .to_string();
    let block = Regex::new(r"/\*[^*]*\*/").unwrap().replace(&block, "/**/");
    assert_eq!(
        block,
        "
    /**/
    [data-dfw-id='dash.b']:not(#dfw) { grid-column: span 3; }
    @media (max-width: 834px) {
      [data-dfw-id='dash.c']:not(#dfw) { grid-column: span 1; }
      [data-dfw-id='dash.b']:not(#dfw) { grid-column: span 2; }
    }
    @media (max-width: 390px) {
      [data-dfw-id='dash.b']:not(#dfw) { grid-column: span 1; }
    }
  ",
        "base first, then media blocks widest first so narrower ones win"
    );
    // Removing every rule removes the block and restores the file.
    for (id, mw) in [
        ("dash.b", 0),
        ("dash.c", 834),
        ("dash.b", 390),
        ("dash.b", 834),
    ] {
        apply(&root, layout_at(id, "grid-column", Value::Null, mw));
    }
    assert_eq!(read_g(&root), GRID);
}

/// layout: width / min-height snap to tokens or percentages; refusals; lint stays clean
// The lint and comment-source assertions of the JS test belong to the lint and review groups.
#[test]
fn layout_values_and_refusals() {
    let root = grid_project();
    apply(&root, layout("dash.a", "width", "50%"));
    apply(&root, layout("dash.a", "min-height", "space.4"));
    apply(&root, layout("dash.c", "width", "--size-sidebar"));
    let t = read_g(&root);
    assert!(
        t.contains("[data-dfw-id='dash.a']:not(#dfw) { width: 50%; min-height: var(--space-4); }")
    );
    assert!(t.contains("[data-dfw-id='dash.c']:not(#dfw) { width: var(--size-sidebar); }"));
    refused(&root, layout("dash.a", "width", "37%"), "snap");
    refused(&root, layout("dash.a", "width", "300px"), "token");
    refused(&root, layout("dash.a", "min-height", "color.ink"), "token");
    refused(&root, layout("dash.a", "grid-column", "span 99"), "span N");
    refused(&root, layout("dash.a", "color", "x"), "not resizable");
    refused(
        &root,
        json!({ "file": G, "op": "layout", "id": "dash.a", "prop": "width", "value": "50%", "maxWidth": "wide" }),
        "breakpoint",
    );
    refused(&root, layout("nope", "width", "50%"), "not found");
    // The rule is written with single quotes, so the element is the only data-dfw-id="dash.a".
    assert_eq!(t.matches("data-dfw-id=\"dash.a\"").count(), 1);
}

/// layout: refuses to rewrite a hand-edited layout block; undo restores layout edits
#[test]
fn layout_hand_edited_and_undo() {
    let root = grid_project();
    apply(&root, layout("dash.b", "grid-column", "span 2"));
    apply(
        &root,
        json!({ "file": G, "op": "move", "id": "dash.c", "ref": "dash.a", "position": "before" }),
    );
    undo_edit(&root).unwrap();
    undo_edit(&root).unwrap();
    assert_eq!(read_g(&root), GRID);
    write(
        &root,
        &rel::design(G),
        &GRID.replace(
            "</head>",
            "<style data-dfw-layout>.cards > * { width: 1px }</style></head>",
        ),
    );
    refused(
        &root,
        layout("dash.b", "grid-column", "span 2"),
        "edited by hand",
    );
}

/// edits are refused once the design is approved and locked (G2) or in review-only mode
#[test]
fn refused_when_locked_or_review_only() {
    let root = grid_project();
    let state = |s: Value| write(&root, rel::STATE, &s.to_string());
    state(
        json!({ "schema": 1, "mode": "new-product", "phase": "stack", "design": { "version": 1, "status": "approved", "locked": true, "approved": [] } }),
    );
    for op in [
        json!({ "file": G, "op": "move", "id": "dash.c", "ref": "dash.a", "position": "before" }),
        layout("dash.b", "grid-column", "span 2"),
        json!({ "file": G, "op": "text", "id": "dash.a", "value": "Z" }),
    ] {
        refused(&root, op, r"approved and locked \(G2\)");
    }
    assert!(undo_edit(&root).unwrap_err().message().contains("locked"));
    state(
        json!({ "schema": 1, "mode": "review-only", "design": { "version": 1, "locked": false } }),
    );
    refused(
        &root,
        json!({ "file": G, "op": "text", "id": "dash.a", "value": "Z" }),
        "review-only",
    );
    state(
        json!({ "schema": 1, "mode": "new-product", "design": { "version": 2, "locked": false } }),
    );
    let again = apply(
        &root,
        json!({ "file": G, "op": "text", "id": "dash.a", "value": "Z" }),
    );
    assert!(again.changed, "editable again after reopen");
    assert_eq!(read_g(&root), GRID.replace(">A<", ">Z<"));
}

/// layout: rules outrank class-based screen rules via :not(#dfw); older unsuffixed blocks still parse
#[test]
fn layout_specificity() {
    let root = grid_project();
    apply(&root, layout("dash.a", "width", "50%"));
    let t = read_g(&root);
    let rule = Regex::new(r"(\[data-dfw-id='dash.a'\][^{]*)\{")
        .unwrap()
        .captures(&t)
        .unwrap()[1]
        .trim()
        .to_string();
    assert_eq!(
        rule, "[data-dfw-id='dash.a']:not(#dfw)",
        "one id of specificity: (1,1,0) beats e.g. \".dash .card\" (0,2,0) without !important"
    );
    // A block written before the suffix existed is read and rewritten in the current form.
    write(
        &root,
        &rel::design(G),
        &GRID.replace(
            "</head>",
            "  <style data-dfw-layout>\n    [data-dfw-id='dash.b'] { grid-column: span 2; }\n  </style>\n</head>",
        ),
    );
    apply(&root, layout("dash.c", "grid-column", "span 1"));
    assert!(read_g(&root).contains("[data-dfw-id='dash.b']:not(#dfw) { grid-column: span 2; }\n    [data-dfw-id='dash.c']:not(#dfw) { grid-column: span 1; }"));
}

/// layout reset: value "auto" removes one property; no prop removes the rule at that scope only
#[test]
fn layout_reset() {
    let root = grid_project();
    apply(&root, layout("dash.a", "width", "50%"));
    apply(&root, layout("dash.a", "min-height", "space.4"));
    apply(&root, layout_at("dash.a", "width", json!("25%"), 390));
    apply(&root, layout("dash.a", "width", "auto"));
    assert!(read_g(&root)
        .contains("[data-dfw-id='dash.a']:not(#dfw) { min-height: var(--space-4); }\n    @media"));
    let reset = json!({ "file": G, "op": "layout", "id": "dash.a", "maxWidth": 390 });
    apply(&root, reset.clone());
    let t = read_g(&root);
    assert!(!t.contains("@media"), "empty media block removed");
    assert!(t.contains("min-height: var(--space-4)"), "base rule kept");
    assert!(!apply(&root, reset).changed, "nothing to reset");
    apply(
        &root,
        json!({ "file": G, "op": "layout", "id": "dash.a", "prop": null, "value": null }),
    );
    assert_eq!(
        read_g(&root),
        GRID,
        "last rule removed: block removed, file restored"
    );
}

/// move inside: drops into an empty container (own-line close tag, one-line empty, inline); refusals
#[test]
fn move_inside_a_container() {
    let root = grid_project();
    let with_empty = GRID.replace(
        "    <div class=\"row\"",
        "    <section class=\"cards\" data-dfw-id=\"dash.empty\">\n    </section>\n    <div class=\"cols\" data-dfw-id=\"dash.cols\"></div>\n    <div class=\"row\"",
    );
    write(&root, &rel::design(G), &with_empty);
    let inside = |id: &str, target: &str| json!({ "file": G, "op": "move", "id": id, "ref": target, "position": "inside" });
    apply(&root, inside("dash.b", "dash.empty"));
    assert!(read_g(&root).contains("<section class=\"cards\" data-dfw-id=\"dash.empty\">\n      <article data-dfw-id=\"dash.b\">\n        <h2 data-dfw-id=\"dash.b.title\">B</h2>\n      </article>\n    </section>"));
    apply(&root, inside("dash.a", "dash.cols"));
    assert!(read_g(&root).contains("<div class=\"cols\" data-dfw-id=\"dash.cols\">\n      <div data-dfw-id=\"dash.a\">A</div>\n    </div>"));
    undo_edit(&root).unwrap();
    undo_edit(&root).unwrap();
    assert_eq!(
        read_g(&root),
        with_empty,
        "undo restores the source byte for byte"
    );
    apply(&root, inside("dash.x", "dash.cols"));
    assert!(
        read_g(&root).contains(
            r#"<div class="cols" data-dfw-id="dash.cols"><span data-dfw-id="dash.x">x</span></div>"#
        ),
        "inline element into an inline container"
    );
    refused(&root, inside("dash.cards", "dash.b.title"), "into itself");
    refused(&root, inside("dash.a", "dash.c"), "component instance");
}

const SHELL: &str = r#"<dfw-component name="app-shell" description="App frame with a nav and a main slot">
<template><div class="shell" data-dfw-id="shell.frame"><nav data-dfw-id="shell.nav">Nav</nav><main class="shell-main" data-dfw-id="shell.main"><dfw-slot name="main"></dfw-slot></main></div></template>
</dfw-component>"#;

const SLOTTED: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
</head>
<body>
  <dfw-use c="app-shell" id="orders.shell">
    <dfw-fill slot="main">
      <section data-dfw-id="orders.summary">Summary</section>
      <section data-dfw-id="orders.table">Table</section>
      <aside data-dfw-id="orders.help">Help</aside>
    </dfw-fill>
  </dfw-use>
</body>
</html>"#;

/// slot content: screen elements reorder among slot siblings; master-only ids are refused with a hint to edit the master
#[test]
fn slot_content_and_master_ids() {
    let root = grid_project();
    write(&root, &rel::component(APP_SHELL_MASTER), SHELL);
    let s = "screens/home/slotted.html";
    let file = rel::design(s);
    write(&root, &file, SLOTTED);
    let composed = compose_html(
        &root,
        SLOTTED,
        ComposeOptions {
            lib: None,
            design_base: None,
        },
    )
    .unwrap();
    assert_match(
        &composed.html,
        r#"<main class="shell-main" data-dfw-id="shell.main">\s*<section data-dfw-id="orders.summary">"#,
    );
    apply(
        &root,
        json!({ "file": s, "op": "move", "id": "orders.help", "ref": "orders.summary", "position": "before" }),
    );
    assert!(read(&root, &file).contains("<dfw-fill slot=\"main\">\n      <aside data-dfw-id=\"orders.help\">Help</aside>\n      <section data-dfw-id=\"orders.summary\">"));
    apply(
        &root,
        json!({ "file": s, "op": "layout", "id": "orders.table", "prop": "min-height", "value": "space.4" }),
    );
    assert!(read(&root, &file)
        .contains("[data-dfw-id='orders.table']:not(#dfw) { min-height: var(--space-4); }"));
    let before = read(&root, &file);
    for op in [
        json!({ "file": s, "op": "move", "id": "shell.nav", "ref": "orders.summary", "position": "after" }),
        json!({ "file": s, "op": "move", "id": "orders.table", "ref": "shell.nav", "position": "before" }),
        json!({ "file": s, "op": "move", "id": "orders.table", "ref": "shell.main", "position": "inside" }),
        json!({ "file": s, "op": "layout", "id": "shell.nav", "prop": "width", "value": "25%" }),
    ] {
        refused(
            &root,
            op,
            &format!(
                r#"part of the component master "app-shell" \({}\).*Edit the master"#,
                regex::escape(&rel::component(APP_SHELL_MASTER))
            ),
        );
    }
    refused(
        &root,
        json!({ "file": s, "op": "move", "id": "orders.table", "ref": "nope", "position": "before" }),
        "was not found",
    );
    assert_eq!(read(&root, &file), before, "refusals change nothing");
}

// ---------------------------------------------------------------- audit-fixes.test.mjs

/// 1: a tag inside an HTML comment does not end the element (move keeps the source intact)
#[test]
fn audit_1_tag_in_a_comment() {
    let a = "<div data-dfw-id=\"a\">\n  <!-- </div> old wrapper -->\n  <p data-dfw-id=\"p\">x</p>\n</div>";
    let b = "<div data-dfw-id=\"b\">B</div>";
    let page = |x: &str, y: &str| format!("<html><head></head><body>\n{x}\n{y}\n</body></html>\n");
    let root = test_dir("edit-audit");
    write(&root, &rel::screen(INDEX_PAGE), &page(a, b));
    apply(
        &root,
        json!({ "file": "screens/a/index.html", "op": "move", "id": "a", "ref": "b", "position": "after" }),
    );
    assert_eq!(read(&root, &rel::screen(INDEX_PAGE)), page(b, a));
}

/// 7: undo skips the edit of a deleted file, and the journal is capped
#[test]
fn audit_7_deleted_file_and_cap() {
    /// The screen whose folder is deleted.
    const GONE: &str = "d";
    let screen =
        |t: &str| format!("<html><head></head><body><p data-dfw-id=\"t\">{t}</p></body></html>");
    let root = test_dir("edit-audit");
    write(&root, &rel::screen(INDEX_PAGE_C), &screen("c"));
    write(&root, &rel::screen(&index_page(GONE)), &screen(GONE));
    let text =
        |file: &str, value: &str| json!({ "file": file, "op": "text", "id": "t", "value": value });
    apply(&root, text("screens/c/index.html", "c2"));
    apply(&root, text("screens/d/index.html", "d2"));
    fs::remove_dir_all(root.join(rel::screen(GONE))).unwrap();
    assert_eq!(undo_edit(&root).unwrap().file, rel::screen(INDEX_PAGE_C));
    assert_eq!(read(&root, &rel::screen(INDEX_PAGE_C)), screen("c"));
    assert_eq!(undo_edit(&root).unwrap_err().message(), "Nothing to undo.");

    for i in 0..MAX_JOURNAL + 5 {
        apply(&root, text("screens/c/index.html", &format!("v{i}")));
    }
    assert_eq!(
        fs::read_dir(root.join(rel::EDITS)).unwrap().count(),
        MAX_JOURNAL
    );
    undo_edit(&root).unwrap(); // the newest records are the ones kept
    assert_eq!(
        read(&root, &rel::screen(INDEX_PAGE_C)),
        screen(&format!("v{}", MAX_JOURNAL + 3))
    );
}

/// 9: a spacing edit keeps the case of custom properties in the inline style
#[test]
fn audit_9_custom_property_case() {
    let root = test_dir("edit-audit");
    write(
        &root,
        rel::TOKENS_JSON,
        r#"{"space":{"$type":"dimension","4":{"$value":{"value":16,"unit":"px"}}}}"#,
    );
    write(
        &root,
        &rel::screen(INDEX_PAGE_C),
        r#"<html><head></head><body><div data-dfw-id="g" style="--colW: 3; GAP: 4px">x</div></body></html>"#,
    );
    apply(
        &root,
        json!({ "file": "screens/c/index.html", "op": "spacing", "id": "g", "prop": "gap", "value": "space.4" }),
    );
    assert!(read(&root, &rel::screen(INDEX_PAGE_C))
        .contains(r#"style="--colW: 3; gap: var(--space-4)""#));
}

/// 10: the design/approved boundary holds for other letter case and for symlinks; missing files still get a clear error
#[test]
fn audit_10_boundary_on_real_paths() {
    let frozen = r#"<html><body><p data-dfw-id="t">frozen</p></body></html>"#;
    let root = test_dir("edit-audit");
    write(&root, &rel::approved("v1/index.html"), frozen);
    write(&root, &rel::screen(INDEX_PAGE), frozen);
    write(&root, OUTSIDE_DIR_PAGE, frozen);
    let resolve_file = |file: &str| resolve_design_file(&root, Some(&json!(file)));
    assert!(resolve_file("screens/nope/index.html")
        .unwrap_err()
        .message()
        .contains(&format!(
            "{} does not exist",
            rel::screen(&index_page("nope"))
        )));
    assert_eq!(
        resolve_file(&rel::screen(INDEX_PAGE)).unwrap().rel,
        rel::screen(INDEX_PAGE)
    );
    let op = |file: &str| json!({ "file": file, "op": "text", "id": "t", "value": "X" });
    #[cfg(unix)]
    {
        use crate::core::fixture::{OTHER_HTML_FILE, OUTSIDE_DIR};
        use std::os::unix::fs::symlink;
        /// A link in `design/` to the folder outside.
        const OUT_LINK: &str = "link";
        /// A link in `design/` to the snapshots.
        const SNAPSHOT_LINK: &str = "alias";
        symlink(root.join(OUTSIDE_DIR), root.join(rel::design(OUT_LINK))).unwrap();
        refused(
            &root,
            op(&rel::join(OUT_LINK, OTHER_HTML_FILE)),
            "limited to files under design",
        );
        symlink(
            root.join(rel::APPROVED),
            root.join(rel::design(SNAPSHOT_LINK)),
        )
        .unwrap();
        refused(
            &root,
            op(&rel::join(SNAPSHOT_LINK, &index_page("v1"))),
            "frozen",
        );
    }
    if root
        .join(rel::design(&file::APPROVED.to_uppercase()))
        .exists()
    {
        // Case-insensitive file system (macOS, Windows): "Approved/..." is the frozen file.
        refused(&root, op("Approved/v1/index.html"), "frozen");
    }
    assert_eq!(read(&root, &rel::approved("v1/index.html")), frozen);
    assert_eq!(read(&root, OUTSIDE_DIR_PAGE), frozen);
}

// ---------------------------------------------------------------- beyond the JS tests

/// Every byte outside the edited range stays as it was, whatever the file looks like.
#[test]
fn bytes_outside_the_edit_are_kept() {
    let root = test_dir("edit-bytes");
    write(&root, rel::TOKENS_JSON, TOKENS);
    let body = "<main data-dfw-id='m' class='a > b'>\r\n\t<!-- </main> <p data-dfw-id=\"t\"> -->\r\n\t<p data-dfw-id=\"t\" title='it\"s'>Text \u{1F600}</p>\r\n</main>";
    let source = format!("\u{feff}<html><head></head><body>\r\n{body}\r\n</body></html>");
    write(&root, &rel::design(COMPOSED_PAGE), &source);
    apply(
        &root,
        json!({ "file": "s/a.html", "op": "text", "id": "t", "value": "New" }),
    );
    assert_eq!(
        read(&root, &rel::design(COMPOSED_PAGE)),
        source.replace("Text \u{1F600}", "New")
    );
    apply(
        &root,
        json!({ "file": "s/a.html", "op": "spacing", "id": "t", "prop": "gap", "value": "space.4" }),
    );
    assert_eq!(
        read(&root, &rel::design(COMPOSED_PAGE)),
        source.replace("Text \u{1F600}", "New").replace(
            " title='it\"s'",
            " title='it\"s' style=\"gap: var(--space-4)\""
        )
    );
    undo_edit(&root).unwrap();
    undo_edit(&root).unwrap();
    assert_eq!(
        fs::read(root.join(rel::design(COMPOSED_PAGE))).unwrap(),
        source.as_bytes()
    );
}

/// The sequence of a record continues from the newest record, so two edits in one
/// millisecond keep their order.
#[test]
fn journal_names_stay_in_order() {
    assert!(next_stamp(&[], None).ends_with("-0000"));
    let next = next_stamp(&["00001700000000000-0041.json".to_string()], None);
    assert!(next.ends_with("-0042"), "{next}");
    assert!(next_stamp(&["00001700000000000-9999.json".to_string()], None).ends_with("-0000"));
    assert_eq!(next.len(), "00001700000000-0042".len());
}

/// `parseLayoutCss` and `renderLayout` agree: a block dfw wrote parses back to the same block.
#[test]
fn layout_block_round_trip() {
    let css = "\n  /* note */\n  [data-dfw-id='a']:not(#dfw) { width: 50%; min-height: var(--space-4); }\n  @media (max-width: 390px) {\n    [data-dfw-id=\"a\"] { width: 100%; }\n  }\n  @media (max-width: 834px) {\n    [data-dfw-id='b']:not(#dfw) { grid-column: span 2; }\n  }\n";
    let model = parse_layout_css(css).unwrap();
    let rendered = render_layout(&model, "");
    assert!(rendered.find("834px").unwrap() < rendered.find("390px").unwrap());
    let inner = rendered
        .trim_start_matches("<style data-dfw-layout>")
        .trim_end_matches("</style>");
    assert_eq!(
        render_layout(&parse_layout_css(inner).unwrap(), ""),
        rendered
    );
    assert!(parse_layout_css("a { }").is_err());
    assert!(parse_layout_css("@media (max-width: 1px) { a { } }").is_err());
}

/// `loadTokenNames`: names in file order with their css variable; integer-like keys first.
#[test]
fn token_names() {
    let root = project();
    let names: Vec<(String, String)> = load_token_names(&root)
        .unwrap()
        .iter()
        .map(|t| (t.name.clone(), t.var()))
        .collect();
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        names,
        [
            pair("space.1", "--space-1"),
            pair("space.4", "--space-4"),
            pair("color.ink", "--color-ink")
        ]
    );
    assert!(load_token_names(&test_dir("edit-none")).unwrap().is_empty());
}

/// Forty Studio threads editing one screen at once: every edit is kept, every edit has its own
/// journal record, and undoing all of them gives the original bytes back (review CO-1: 16 of
/// 40 edits were kept and undo records were lost).
#[test]
fn concurrent_edits_are_all_kept_and_journaled() {
    let root = test_dir("edit-threads");
    let paras: String = (0..40)
        .map(|i| format!("<p data-dfw-id=\"p.{i}\">old {i}</p>\n"))
        .collect();
    let source = format!("<!doctype html><html><body>\n{paras}</body></html>\n");
    write(&root, &rel::screen(MANY_EDITS_PAGE), &source);
    std::thread::scope(|s| {
        for i in 0..40 {
            let root = &root;
            s.spawn(move || {
                let op = json!({ "file": "screens/many/a.html", "op": "text", "id": format!("p.{i}"), "value": format!("new {i}") });
                assert!(apply_edit(root, &op).unwrap().changed);
            });
        }
    });
    let text = read(&root, &rel::screen(MANY_EDITS_PAGE));
    for i in 0..40 {
        assert!(text.contains(&format!(">new {i}<")), "edit {i} lost");
    }
    assert_eq!(journal(&root).unwrap().len(), 40);
    for left in (0..40).rev() {
        assert_eq!(undo_edit(&root).unwrap().remaining, left);
    }
    assert_eq!(read(&root, &rel::screen(MANY_EDITS_PAGE)), source);
    let names: Vec<String> = dir_names(&root.join(rel::EDITS)).unwrap();
    assert!(names.is_empty(), "no lock or record is left: {names:?}");
}

/// The journal never replaces a record whose name is taken.
#[test]
fn a_journal_name_in_use_is_never_overwritten() {
    let root = test_dir("edit-journal-name");
    let taken = next_stamp(&[], None);
    write(&root, &rel::join(rel::EDITS, &record_file(&taken)), "{}");
    write_record(&root, &json!({ "a": 1 })).unwrap();
    assert_eq!(
        read(&root, &rel::join(rel::EDITS, &record_file(&taken))),
        "{}"
    );
    assert_eq!(journal(&root).unwrap().len(), 2);
}
