//! Tests of the lint: the cases of tests/unit that cover lint.mjs, and the rules one by one.
// Over 800 lines: every rule has its inputs and the expected findings written out, next to
// the ported JavaScript cases.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::*;
use crate::core::paths::{file, rel};
use crate::core::util::test_dir;
use crate::design::compose::build_system;
use crate::design::icons::add_icons;
use crate::design::inventory::kit_components;
use crate::design::system::{system_init, InitOptions};

fn write(root: &Path, rel: &str, text: &str) {
    let file = root.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

fn rules(findings: &[Value]) -> Vec<String> {
    findings
        .iter()
        .map(|f| f["rule"].as_str().unwrap().to_string())
        .collect()
}

fn leveled(findings: &[Value]) -> Vec<String> {
    findings
        .iter()
        .map(|f| {
            format!(
                "{}:{}",
                f["level"].as_str().unwrap(),
                f["rule"].as_str().unwrap()
            )
        })
        .collect()
}

fn messages(findings: &[Value]) -> Vec<&str> {
    findings
        .iter()
        .map(|f| f["message"].as_str().unwrap())
        .collect()
}

fn has(findings: &[Value], rule: &str) -> bool {
    findings.iter().any(|f| f["rule"] == rule)
}

fn lines_of(findings: &[Value], rule: &str) -> Vec<u64> {
    findings
        .iter()
        .filter(|f| f["rule"] == rule)
        .map(|f| f["line"].as_u64().unwrap())
        .collect()
}

/// `project()` of tests/unit/system.test.mjs.
fn project() -> PathBuf {
    let root = test_dir("lint");
    write(&root, rel::STATE, "{}");
    fs::create_dir_all(root.join(rel::TOKENS)).unwrap();
    write(
        &root,
        rel::MANIFEST,
        r#"{"schema":1,"product":"T","screens":[]}"#,
    );
    root
}

/// `useKit()` of tests/unit/system.test.mjs: put starter-kit masters into a project (the CLI
/// never copies them).
fn use_kit(root: &Path, names: Option<&[&str]>) {
    let dir = root.join(rel::COMPONENTS);
    fs::create_dir_all(&dir).unwrap();
    let kit = kit_components().unwrap();
    for entry in fs::read_dir(&kit).unwrap() {
        let name = entry.unwrap().file_name().to_string_lossy().into_owned();
        let Some(stem) = name.strip_suffix(".html") else {
            continue;
        };
        if names.is_none_or(|n| n.contains(&stem)) {
            fs::copy(kit.join(&name), dir.join(&name)).unwrap();
        }
    }
    let icons = [
        "chevron-down",
        "triangle-alert",
        "search",
        "x",
        "check",
        "package",
        "house",
        "file-clock",
        "settings",
        "layout-dashboard",
        "circle-check",
        "circle-x",
        "circle-alert",
        "octagon-alert",
        "package-open",
        "ellipsis",
        "wrench",
        "log-in",
        "log-out",
        "plus",
    ]
    .map(String::from);
    add_icons(root, &icons).unwrap();
    build_system(root).unwrap();
}

/// Every constant expression translates and compiles (the `js_re!` statics rely on it).
#[test]
fn patterns_compile() {
    let all = patterns();
    assert!(all.len() > 55);
    for re in all {
        assert!(!re.as_str().is_empty());
    }
}

/// lint: ids, alt, colors, focus (tests/unit/dfw.test.mjs)
#[test]
fn ids_alt_colors_focus() {
    let html = r##"<html><head><link href="../../tokens/tokens.css"></head><body><button data-dfw-id="a.b">x</button><button data-dfw-id="a.b">y</button><img src="x.png"><a href="#">z</a></body></html>"##;
    let found = rules(&lint_html("x.html", html));
    for rule in ["dfw-id-duplicate", "img-alt", "dfw-id-missing", "html-lang"] {
        assert!(found.iter().any(|r| r == rule), "{rule} in {found:?}");
    }
    let mut css = rules(&lint_css(
        &rel::join(rel::STYLES, "x.css"),
        ":root{--x:#fff}\n.a{color:#123456}\n.b:focus{outline:none}",
    ));
    css.sort();
    assert_eq!(css, ["focus-removed", "hardcoded-color"]);
}

/// lint: generic-AI tells (default colours, emoji icons, filler, metrics, placeholder hosts,
/// gradient, left-border card) (tests/unit/dfw.test.mjs)
#[test]
fn generic_ai_tells() {
    let html = r##"<!doctype html><html lang="en"><head><link rel="stylesheet" href="tokens.css"><style>
  .hero{background:linear-gradient(135deg,#7c3aed,#2563eb)} .tile{border-radius:12px;border-left:4px solid var(--color-accent)}
  </style></head><body><h2 data-dfw-id="a">🚀 Fast</h2><button data-dfw-id="b">Go</button><p>Lorem ipsum dolor</p>
  <p>Teams ship 10x faster</p><img data-dfw-id="c" alt="" src="https://images.unsplash.com/x.jpg"><span style="color:#6366f1">x</span></body></html>"##;
    let found = leveled(&lint_html(&rel::screen("a.html"), html));
    for rule in [
        "error:emoji-icon",
        "error:filler-copy",
        "warn:invented-metric",
        "warn:placeholder-image-host",
        "warn:ai-gradient",
        "warn:left-border-card",
        "warn:ai-default-color",
    ] {
        assert!(found.iter().any(|r| r == rule), "{rule} in {found:?}");
    }
    // Clean markup raises none of them.
    let clean = lint_html(
        &rel::screen("b.html"),
        r#"<html lang="en"><body><h2 data-dfw-id="a">Assets due for audit</h2><p>12 of 340 assets are overdue.</p></body></html>"#,
    );
    let tells = [
        "ai-",
        "emoji",
        "filler",
        "metric",
        "placeholder-image",
        "left-border",
    ];
    assert!(!rules(&clean)
        .iter()
        .any(|r| tells.iter().any(|t| r.contains(t))));
    assert!(has(
        &lint_css(
            &rel::design("x.css"),
            ".card{border-radius:8px;border-left:3px solid red}"
        ),
        "left-border-card"
    ));
    let tokens = json!({ "color": { "$type": "color", "accent": { "$value": "#4F46E5" } } });
    assert!(has(
        &lint_tokens("t.json", &tokens, None),
        "ai-default-color"
    ));
}

/// lint: state coverage for data surfaces and forms (tests/unit/dfw.test.mjs)
#[test]
fn state_coverage_for_data_surfaces_and_forms() {
    let root = test_dir("lint-states");
    let manifest = |states: Value| {
        let text = json!({ "states": states, "screens": [{ "id": "list", "variants": [{ "id": "a", "file": "screens/list/a.html" }] }] });
        write(&root, rel::MANIFEST, &text.to_string());
    };
    manifest(json!([]));
    let rel = &rel::screen("list/a.html");
    let found = state_coverage(&root, rel, "<body><table></table><form></form></body>").unwrap();
    assert_eq!(found.len(), 1);
    assert!(found[0]["message"]
        .as_str()
        .unwrap()
        .contains("no loading/empty/error state"));
    manifest(json!(["loading", "empty", "error"]));
    let designed = r#"<body><table class="unless-empty"></table><p class="when-loading">Loading</p><p class="when-empty">No orders</p><p class="when-error">Could not load</p></body>"#;
    assert!(state_coverage(&root, rel, designed).unwrap().is_empty());
    assert!(state_coverage(
        &root,
        rel,
        r#"<body><p class="when-empty">x</p><p class="when-loading">x</p><p class="when-error">x</p></body>"#
    )
    .unwrap()
    .is_empty());
}

/// lint: a declared state that nothing in the screen (or its masters) changes is reported, and
/// the Studio is told which states a screen designs (its frames would repeat the default).
#[test]
fn declared_but_undesigned_states() {
    let root = test_dir("lint-states-undesigned");
    let text = json!({ "states": ["loading", "empty", "error"], "screens": [{ "id": "home", "variants": [{ "id": "a", "file": "screens/home/a.html" }] }] });
    write(&root, rel::MANIFEST, &text.to_string());
    let rel = &rel::screen("home/a.html");
    // Inherited from the top of design.json and designed nowhere: one finding naming all three.
    let found = state_coverage(&root, rel, "<body><h1>Home</h1></body>").unwrap();
    assert_eq!(rules(&found), ["state-undesigned"]);
    let message = found[0]["message"].as_str().unwrap();
    assert!(message.contains("loading/empty/error"), "{message}");
    // A screen block, a data-state rule and a master the screen uses each count as designed.
    write(
        &root,
        &rel::component("order-list.html"),
        r#"<dfw-component name="order-list"><style>html[data-state="loading"] .ol { opacity: .5 }</style><template><ul class="ol"></ul></template></dfw-component>"#,
    );
    let html = r#"<body><p class="when-empty">No orders yet</p><style>[data-state='error'] main{display:none}</style><dfw-use c="order-list"></dfw-use></body>"#;
    let names: Vec<String> = ["loading", "empty", "error"].map(String::from).to_vec();
    assert_eq!(designed_states(&root, html, &names).unwrap(), names);
    assert!(state_coverage(&root, rel, html).unwrap().is_empty());
    // Only one of them: the other two are reported.
    let one = state_coverage(
        &root,
        rel,
        r#"<body><p class="when-empty-hint">x</p><p class="when-error">x</p></body>"#,
    )
    .unwrap();
    assert!(
        one[0]["message"]
            .as_str()
            .unwrap()
            .contains("loading/empty"),
        "{one:?}"
    );
}

/// lint: interface-guideline and copy rules, near-duplicate colours, component state matrix
/// (tests/unit/dfw.test.mjs)
#[test]
fn guideline_and_copy_rules_near_duplicates_component_states() {
    let html = r##"<html lang="en"><head><style>input{font-size:13px} .menu{z-index:999}</style></head><body>
  <p data-dfw-id="a">Seamlessly manage everything.</p><p data-dfw-id="b">Oops! Something went wrong.</p><p data-dfw-id="c">Loading...</p>
  <a data-dfw-id="d" href="#x">click here</a></body></html>"##;
    let found = rules(&lint_html(&rel::screen("x.html"), html));
    for rule in [
        "copy-tell",
        "vague-error",
        "ellipsis-char",
        "link-text",
        "input-font-size",
        "z-index-raw",
    ] {
        assert!(found.iter().any(|r| r == rule), "{rule} in {found:?}");
    }
    let clean = lint_html(
        &rel::screen("y.html"),
        r##"<html lang="en"><head><style>input{font-size:16px} .menu{z-index:var(--z-menu)}</style></head><body><p data-dfw-id="a">Loading…</p><a data-dfw-id="d" href="#x">View invoice 1042</a></body></html>"##,
    );
    let guideline = [
        "copy-tell",
        "vague-error",
        "ellipsis",
        "link-text",
        "input-font",
        "z-index",
    ];
    assert!(!rules(&clean)
        .iter()
        .any(|r| guideline.iter().any(|g| r.contains(g))));
    let tokens = json!({ "color": { "$type": "color", "a": { "$value": "#f4f4f5" }, "b": { "$value": "#f4f4f6" }, "c": { "$value": "#2453c7" } } });
    let near = lint_tokens("t.json", &tokens, None);
    assert_eq!(lines_of(&near, "near-duplicate-color").len(), 1);
    let button = |css: &str| {
        lint_component_file(
            &rel::component("b.html"),
            &format!(
                r#"<dfw-component name="b"><style>{css}</style><template><button class="b">x</button></template></dfw-component>"#
            ),
        )
    };
    assert!(has(&button(".b{color:var(--c)}"), "component-states"));
    assert!(!has(
        &button(".b:hover{} .b:disabled{}"),
        "component-states"
    ));
    // A card that merely contains a link is not a control.
    let card = lint_component_file(
        "c.html",
        r##"<dfw-component name="c"><style>.c{}</style><template><div class="c"><a href="#">x</a></div></template></dfw-component>"##,
    );
    assert!(!has(&card, "component-states"));
}

/// kit: every reference master composes its examples without issues and lints clean
/// (tests/unit/system.test.mjs; the composing half is in design/compose, the Design System page
/// half belongs to design/system_page)
#[test]
fn kit_masters_lint_clean() {
    let root = project();
    system_init(&root, &InitOptions::default()).unwrap();
    use_kit(&root, None);
    let errors: Vec<Value> = lint_design(&root, None)
        .unwrap()
        .into_iter()
        .filter(|f| rel::under(rel::SYSTEM, f["file"].as_str().unwrap()) && f["level"] == "error")
        .collect();
    assert_eq!(errors, Vec::<Value>::new());
}

/// lint: screens must compose the design system and keep visual styling out
/// (tests/unit/system.test.mjs)
#[test]
fn screens_compose_the_design_system_and_keep_visual_styling_out() {
    let root = project();
    system_init(&root, &InitOptions::default()).unwrap();
    use_kit(&root, Some(&["button"]));
    let lib = load_components(&root).unwrap();
    let bad = r##"<!doctype html><html lang="en"><head><dfw-head></dfw-head><style>.x { display: grid; color: red; gap: 8px } .y{font-size:14px}</style></head>
<body><main data-dfw-id="m"><button data-dfw-id="b">Raw</button><span style="background: #fff">x</span></main></body></html>"##;
    let found = rules(&lint_screen(&root, &rel::screen("s/a.html"), bad, &lib).unwrap());
    assert_eq!(
        found.iter().filter(|r| *r == "screen-visual-style").count(),
        3
    );
    assert!(found.iter().any(|r| r == "use-component"));
    assert!(found.iter().any(|r| r == "no-components"));
    let good = r#"<!doctype html><html lang="en"><head><dfw-head></dfw-head><style>.x { display: grid; gap: var(--space-2) }</style></head><body><main data-dfw-id="m"><dfw-use c="button" id="m.ok">OK</dfw-use></main></body></html>"#;
    assert_eq!(
        lint_screen(&root, &rel::screen("s/b.html"), good, &lib).unwrap(),
        Vec::<Value>::new()
    );
}

/// 1: an unclosed <dfw-use> is reported and does not swallow the page; comments are not expanded
/// (tests/unit/audit-fixes.test.mjs, the lint half)
#[test]
fn an_unclosed_instance_is_a_lint_finding() {
    let root = test_dir("lint-unclosed");
    write(
        &root,
        &rel::component("divider.html"),
        "<dfw-component name=\"divider\">\n<template>\n<hr class=\"divider\">\n</template>\n</dfw-component>\n",
    );
    let lib = load_components(&root).unwrap();
    let unclosed = "<body>\n<dfw-use c=\"divider\">\n<main data-dfw-id=\"main\"><h1>Title</h1></main>\n</body>";
    let found = lint_screen(&root, &rel::screen("a/index.html"), unclosed, &lib).unwrap();
    assert_eq!(lines_of(&found, "component-unclosed"), [2]);
}

/// 6: a theme may alias tokens of the base file; unknown aliases are still errors
/// (tests/unit/audit-fixes.test.mjs)
#[test]
fn a_theme_may_alias_tokens_of_the_base_file() {
    let root = test_dir("lint-theme");
    write(&root, rel::MANIFEST, r#"{"screens":[]}"#);
    let tokens = json!({ "color": { "$type": "color", "palette": { "n900": { "$value": "#111111" }, "n50": { "$value": "#fafafa" } }, "ink": { "$value": "{color.palette.n900}" } } });
    write(&root, rel::TOKENS_JSON, &tokens.to_string());
    let theme = |alias: &str| {
        let text = json!({ "color": { "$type": "color", "ink": { "$value": alias } } });
        write(
            &root,
            &rel::join(rel::THEMES, "dark.json"),
            &text.to_string(),
        );
    };
    let alias_errors = || -> Vec<String> {
        lint_design(&root, None)
            .unwrap()
            .iter()
            .filter(|f| f["rule"] == "token-alias")
            .map(|f| f["file"].as_str().unwrap().to_string())
            .collect()
    };
    theme("{color.palette.n50}");
    assert_eq!(alias_errors(), Vec::<String>::new());
    theme("{color.palette.nope}");
    assert_eq!(alias_errors(), [rel::join(rel::THEMES, "dark.json")]);
}

/// lint: a malformed design.json is a finding, not a crash (tests/unit/audit-fixes.test.mjs,
/// the lint half)
#[test]
fn a_malformed_manifest_is_a_finding() {
    let root = test_dir("lint-manifest-json");
    write(&root, rel::MANIFEST, "{\"screens\": [");
    write(
        &root,
        rel::TOKENS_JSON,
        r##"{"color":{"$type":"color","ink":{"$value":"#111111"}}}"##,
    );
    write(
        &root,
        &rel::screen("a/index.html"),
        r#"<html lang="en"><head><dfw-head></dfw-head></head><body><main data-dfw-id="m">x</main></body></html>"#,
    );
    let found = lint_design(&root, None).unwrap();
    let broken: Vec<&Value> = found
        .iter()
        .filter(|f| f["rule"] == "manifest-json")
        .collect();
    assert_eq!(broken.len(), 1);
    assert!(broken[0]["message"]
        .as_str()
        .unwrap()
        .starts_with("Invalid JSON in "));
}

#[test]
fn findings_have_the_keys_of_the_node_objects_in_order() {
    let found = lint_html("a.html", "<html><body></body></html>");
    assert_eq!(
        found[0].to_string(),
        r#"{"level":"warn","rule":"html-lang","file":"a.html","line":1,"message":"<html> has no lang attribute.","source":"dfw"}"#
    );
    let root = test_dir("lint-keys");
    write(
        &root,
        rel::MANIFEST,
        r#"{"screens":[{"id":"s","variants":[{"id":"a","file":"s.html"}]}]}"#,
    );
    let coverage = state_coverage(&root, &rel::design("s.html"), "<form>").unwrap();
    let keys: Vec<&String> = coverage[0].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["level", "rule", "file", "line", "source", "message"]);
    assert!(coverage[0]["message"]
        .as_str()
        .unwrap()
        .starts_with("Screen \"s\" has a form but no error state."));
}

#[test]
fn html_ids_lines_and_structure() {
    let html = "<html lang=\"en\">\n<head><dfw-head></dfw-head>\n<nav>before body is not counted</nav></head>\n<body>\n<p data-dfw-id=\"\">a</p>\n<p data-dfw-id=\"Bad Id\">b</p>\n<p data-dfw-id=\"ok.one\">c</p>\n<p data-dfw-id = \"ok.one\">d</p>\n<IMG src=x>\n<button style=\"x\" data-dfw-id=\"b\">e</button>\n<aside>f</aside>\n<!-- <img src=y> -->\n</body></html>";
    let found = lint_html("a.html", html);
    assert_eq!(lines_of(&found, "dfw-id-empty"), [5]);
    assert_eq!(lines_of(&found, "dfw-id-format"), [6]);
    assert_eq!(lines_of(&found, "dfw-id-duplicate"), [8]);
    assert!(messages(&found).contains(&"Duplicate data-dfw-id \"ok.one\" (first on line 7)."));
    // A tag inside a comment is still a tag for the Node expressions, and so here.
    assert_eq!(lines_of(&found, "img-alt"), [9, 12]);
    assert_eq!(lines_of(&found, "inline-style"), [10]);
    assert_eq!(lines_of(&found, "dfw-id-missing"), [9]);
    assert!(messages(&found)
        .iter()
        .any(|m| m.starts_with("3 structural/interactive element(s)")));
    assert!(!has(&found, "html-lang"));
    assert!(!has(&found, "tokens-link"));
    // Without <body> nothing is "before the body".
    let no_body = lint_html("a.html", "<nav></nav>");
    assert_eq!(lines_of(&no_body, "dfw-id-missing"), [1]);
    assert!(has(&no_body, "tokens-link"));
}

#[test]
fn style_colors_and_the_eighty_character_window() {
    let html = "<style>\n:root{--a:#fff; --b: rgb(0 0 0)}\n.x{color:#abc;\n background:oklch(50% 0 0)}\n.y{--local:1px solid #123456}\n</style>";
    let found = lint_html("a.html", html);
    // "#abc" is excused: the definition before it ("--b: ...}") has no semicolon in between,
    // so the Node expression takes the colour for part of it (a Node bug, kept).
    assert_eq!(lines_of(&found, "hardcoded-color"), [4]);
    assert!(messages(&found)
        .contains(&"Hard-coded color \"oklch(\" in <style>; use a token (var(--...))."));
    // The definition is more than 80 UTF-16 units back: the colour is no longer excused.
    let far = format!("<style>.x{{--v: {} #fff}}</style>", "a".repeat(80));
    assert_eq!(lines_of(&lint_html("a.html", &far), "hardcoded-color"), [1]);
    let near = format!("<style>.x{{--v: {} #fff}}</style>", "a".repeat(70));
    assert!(!has(&lint_html("a.html", &near), "hardcoded-color"));
    // 40 emoji are 80 units: the window starts after "--v:".
    let emoji = format!("<style>.x{{--v:{}#fff}}</style>", "🚀".repeat(40));
    assert_eq!(
        lines_of(&lint_html("a.html", &emoji), "hardcoded-color"),
        [1]
    );
    let fewer = format!("<style>.x{{--v:{}#fff}}</style>", "🚀".repeat(38));
    assert!(!has(&lint_html("a.html", &fewer), "hardcoded-color"));
}

#[test]
fn slop_rules_one_by_one() {
    let found = |html: &str| lint_html("a.html", html);
    // Emoji: text blocks close on their own name, any case; an unclosed one is skipped.
    let html = "<body>\n<h1>Plain</h1>\n<LI>🚀 x</li>\n<button>never closed 🚀\n<label><b>🎉</b></label>\n<span class=\"x icon\">\n ⭐</span>\n<i class=\"icon\">ab</i></body>";
    let f = found(html);
    assert_eq!(lines_of(&f, "emoji-icon"), [3, 5, 6]);
    assert!(messages(&f)
        .iter()
        .any(|m| m.starts_with("Emoji in <LI> used")));
    // Filler, copy tells, vague errors, ellipses, link text, metrics, placeholder hosts.
    let f = found("<body><p>Lorem IPSUM</p><p title=\"seamless\">Unlock the power</p><p>Oops</p><p>a... b.... c…</p><a href=x> Read More </a><p>3.5× faster, 99.99% uptime</p><img alt src=//PLACEHOLD.co/1></body>");
    for (rule, count) in [
        ("filler-copy", 1),
        ("copy-tell", 1),
        ("vague-error", 1),
        ("ellipsis-char", 1),
        ("link-text", 1),
        ("invented-metric", 2),
        ("placeholder-image-host", 1),
    ] {
        assert_eq!(lines_of(&f, rule).len(), count, "{rule}: {:?}", rules(&f));
    }
    // Text inside <style> and <script> is not copy; an attribute is not text.
    let f = found("<body><style>.a::after{content:'lorem ipsum'}</style><script>var s = 'click here... seamless';</script><p title=\"oops\">fine</p></body>");
    for rule in ["filler-copy", "copy-tell", "vague-error", "ellipsis-char"] {
        assert!(!has(&f, rule), "{rule}");
    }
    // Before <body> (or <template>) nothing is copy, but colours and hosts count everywhere.
    let f = found("<title>Lorem ipsum #6366F1 picsum.photos</title><body></body>");
    assert!(!has(&f, "filler-copy"));
    assert!(has(&f, "placeholder-image-host"));
    assert!(messages(&f)
        .iter()
        .any(|m| m.starts_with("#6366F1 is Tailwind indigo,")));
    // ASCII-only case folding: U+017F is not "s".
    assert!(!has(&found("<body>\u{17f}eamless</body>"), "copy-tell"));
}

#[test]
fn css_rules_one_by_one() {
    let css = "/* #fff in a comment\n */\n.a{color:#fff;transition : all .2s}\n.b{--c:#000;border-left:2px solid var(--c);border-radius:4px}\nselect.x,\n.y{font-size:15.5px;z-index:-2}\n.z{z-index:1;font-size:12px}\n.g{background:LINEAR-GRADIENT(#3b82f6,#8b5cf6)}\n.h{background:linear-gradient(#ff0000,#00ff00)}\n.o{outline:0}";
    let f = lint_css(&rel::design("a.css"), css);
    assert_eq!(lines_of(&f, "hardcoded-color"), [3, 8, 8, 9, 9]);
    assert_eq!(lines_of(&f, "transition-all"), [3]);
    assert_eq!(lines_of(&f, "left-border-card"), [4]);
    // A rule is reported on the line its selector text starts (the end of the rule before it).
    assert_eq!(lines_of(&f, "input-font-size"), [4]);
    assert_eq!(lines_of(&f, "z-index-raw"), [4]);
    assert_eq!(lines_of(&f, "ai-gradient"), [8]);
    assert_eq!(lines_of(&f, "focus-removed"), [10]);
    assert!(messages(&f)
        .iter()
        .any(|m| m.starts_with("Form field text is 15.5px")));
    assert!(messages(&f)
        .iter()
        .any(|m| m.starts_with("Raw z-index -2:")));
    assert_eq!(rules(&f).last().unwrap(), "focus-removed");
    assert!(lint_css(&rel::join(rel::TOKENS, file::TOKENS_CSS), ".a{color:#fff}").is_empty());
    let replaced = lint_css(
        "a.css",
        ".o{outline:none}.o:focus-visible{outline:1px solid}",
    );
    assert!(!has(&replaced, "focus-removed"));
}

#[test]
fn component_rules_one_by_one() {
    let lint = |text: &str| lint_component_file(&rel::component("x.html"), text);
    assert_eq!(
        rules(&lint("<p>nothing</p>")),
        ["component-missing", "component-template"]
    );
    let f = lint("<dfw-component name=\"Bad\">\n<style>\n.x{color:red;padding:var(--space-2) 12px;margin:2px;gap:1.5px;\n font-size:14px;border-radius:calc(4px + 1px)}\n.y{color:#fff;outline:none}\n</style>\n<template>\n <input class=\"x\">\n</template></dfw-component>");
    assert_eq!(lines_of(&f, "component-name"), [1]);
    assert_eq!(lines_of(&f, "hardcoded-color"), [5]);
    assert_eq!(lines_of(&f, "raw-dimension"), [3, 4, 4]);
    assert!(messages(&f).contains(
        &"Raw length \"padding:var(--space-2) 12px\" in a component; use space/font/radius tokens."
    ));
    assert!(messages(&f)
        .iter()
        .any(|m| m.starts_with("Interactive component has no :focus-visible style.")));
    let states = |text: &str| {
        lint(text)
            .iter()
            .find(|x| x["rule"] == "component-states")
            .map(|x| {
                let message = x["message"].as_str().unwrap();
                message.split(" style.").next().unwrap().to_string()
            })
    };
    let master = |css: &str, tpl: &str| {
        format!("<dfw-component name=\"x\"><style>{css}</style><template>{tpl}</template></dfw-component>")
    };
    let missing = |css: &str, tpl: &str| states(&master(css, tpl));
    let no = |what: &str| Some(format!("Interactive component has no {what}"));
    assert_eq!(missing("", "<A href=x>y</A>"), no(":hover"));
    assert_eq!(missing("", "<div role=\"tab\">y</div>"), no(":hover"));
    assert_eq!(
        missing("select{appearance: none}", "<select></select>"),
        no("disabled")
    );
    assert_eq!(missing("", "<select></select>"), None);
    assert_eq!(
        missing(".b{outline:0}", "<button>y</button>"),
        no(":hover / disabled / :focus-visible")
    );
    assert_eq!(
        missing(
            ".b:hover{}[aria-disabled]{} .b:focus-within{outline:0}",
            "<button>y</button>"
        ),
        None
    );
}

#[test]
fn token_rules_one_by_one() {
    let tree = json!({
        "$description": "skipped",
        "color": {
            "$type": "color",
            "a.b": { "$value": "#111111" },
            "list": [{ "$value": "{color.nope} and {color.list.0}" }],
            "deep": { "$type": "", "x": { "$value": { "hex": "#111112", "more": ["{0}", { "y": "CONSTRUCTOR " }] } } }
        },
        "untyped": { "$value": 4 },
        "0": { "$value": "{untyped}", "$type": "number" }
    });
    let f = lint_tokens("t.json", &tree, None);
    assert_eq!(
        messages(&f),
        [
            "Token/group name \"a.b\" must not contain \"{\", \"}\" or \".\".",
            "Token \"untyped\" has no $type (own or inherited).",
            "Token \"color.list.0\" references unknown token \"{color.nope}\".",
            "Tokens \"color.a.b\" (#111111) and \"color.deep.x\" (#111112) are visually the same colour. Merge them or alias one to the other.",
            "Token \"color.deep.x\" is CONSTRUCTOR  (function Object() { [native code] }), a colour AI-generated UIs default to. Derive the palette from the brand (dfw palette build); keep it only if the brand requires it (dfw decide).",
        ]
    );
    assert_eq!(
        rules(&lint_tokens("t.json", &json!({}), None)),
        ["token-empty"]
    );
    assert_eq!(
        rules(&lint_tokens("t.json", &json!("text"), None)),
        ["token-empty"]
    );
    // A theme resolves aliases against its base.
    let theme = json!({ "c": { "$type": "color", "$value": "{base.ink}" } });
    assert_eq!(rules(&lint_tokens("t.json", &theme, None)), ["token-alias"]);
    let base = json!({ "base": { "ink": { "$value": "#000000" } } });
    assert!(lint_tokens("t.json", &theme, Some(&base)).is_empty());
}

#[test]
fn manifest_rules_one_by_one() {
    let root = test_dir("lint-manifest");
    assert_eq!(rules(&lint_manifest(&root).unwrap()), ["manifest-missing"]);
    write(&root, rel::MANIFEST, "null");
    assert_eq!(rules(&lint_manifest(&root).unwrap()), ["manifest-missing"]);
    write(&root, rel::MANIFEST, "{}");
    assert_eq!(rules(&lint_manifest(&root).unwrap()), ["manifest-screens"]);
    write(&root, &rel::screen("a.html"), "x");
    let manifest = json!({
        "screens": [
            { "id": "a", "blueprint": { "goal": "g", "layout": "l", "aboveFold": [], "data": "d" }, "variants": [{ "id": "v", "file": "screens/a.html" }, { "id": "w" }, { "id": "x", "file": "screens/none.html" }] },
            { "id": "a", "variants": [] },
            { "id": "_draft" },
            { "blueprint": "text", "variants": [{ "id": "v", "file": "/screens/a.html" }] }
        ],
        "pages": [{ "id": "p", "screens": ["a", "ghost", 7] }]
    });
    write(&root, rel::MANIFEST, &manifest.to_string());
    let f = lint_manifest(&root).unwrap();
    assert_eq!(
        messages(&f),
        [
            "Screen \"a\" blueprint is missing \"aboveFold\".",
            "Screen \"a\" blueprint is missing \"feedback\".",
            "Screen \"a\" has no blueprint (goal, layout, aboveFold, data, feedback). Agree it with the user before building: it is cheaper to fix a blueprint than a screen.",
            "Screen \"undefined\" blueprint is missing \"goal\".",
            "Screen \"undefined\" blueprint is missing \"layout\".",
            "Screen \"undefined\" blueprint is missing \"aboveFold\".",
            "Screen \"undefined\" blueprint is missing \"data\".",
            "Screen \"undefined\" blueprint is missing \"feedback\".",
            "Page \"p\" lists unknown screen \"ghost\".",
            "Page \"p\" lists unknown screen \"7\".",
            "Variant \"a/w\" has no file.",
            &format!(
                "Variant \"a/x\" points to missing file {}.",
                rel::screen("none.html")
            ),
            "Duplicate screen id \"a\".",
            "Screen \"a\" has no variants.",
            "Screen \"_draft\" has no variants.",
            "A screen has no id.",
        ]
    );
    assert!(f
        .iter()
        .all(|x| x["file"] == rel::MANIFEST && x["line"] == 1));
}

#[test]
fn screen_rules_one_by_one() {
    let root = test_dir("lint-screen");
    let master = |name: &str, attrs: &str, tpl: &str| {
        write(
            &root,
            &rel::component(&format!("{name}.html")),
            &format!("<dfw-component name=\"{name}\"{attrs}>\n<template>\n{tpl}\n</template>\n</dfw-component>\n"),
        );
    };
    master(
        "button",
        " variants=\"primary ghost\" replaces=\"button\"",
        "<button class=\"b\"><dfw-slot></dfw-slot></button>",
    );
    master(
        "tabs",
        " accepts=\"button\"",
        "<div class=\"tabs\"><dfw-slot></dfw-slot></div>",
    );
    master(
        "page",
        "",
        "<div class=\"page\"><dfw-slot></dfw-slot></div>",
    );
    master(
        "app-shell",
        "",
        "<div class=\"shell\"><dfw-slot></dfw-slot></div>",
    );
    let lib = load_components(&root).unwrap();
    let lint = |rel: &str, text: &str| lint_screen(&root, rel, text, &lib).unwrap();
    let text = "<html><head><button>in head</button><style>\n/* color: red */\n.x{display:grid;\n  BORDER-color: red; border-top-color:red}\n</style></head>\n<body>\n<button>raw</button>\n<dfw-use c=\"tabs\"> <button>accepted</button></dfw-use><dfw-use c=\"tabs\"><button>first child</button></dfw-use>\n<BUTTON>raw too</BUTTON>\n<p STYLE=\"color:red\" style=\"Color: red; --color: x; margin:0;FILL:x\">x</p>\n<dfw-use c=\"button\" variant=\"primary\">1</dfw-use><dfw-use c=\"button\" variant=\"primary\">2</dfw-use><dfw-use c=\"button\" variant=\"primary\">3</dfw-use>\n</body></html>";
    let f = lint(&rel::screen("s/a.html"), text);
    // "BORDER-color" is not a property name for the Node expression; the next one is reported
    // from the white space before it.
    assert_eq!(lines_of(&f, "screen-visual-style"), [4, 10, 10]);
    assert!(messages(&f)
        .contains(&"Inline \"fill\" in a screen; visual styling belongs to the design system."));
    assert_eq!(lines_of(&f, "use-component"), [7]);
    // Two raw buttons, and the one that directly follows the opening tag of an instance that
    // accepts buttons: "inside" is compared strictly (a Node bug, kept).
    assert!(messages(&f)
        .iter()
        .any(|m| m
            .starts_with("3 raw <button> element(s); the design system has component \"button\"")));
    assert!(messages(&f)
        .iter()
        .any(|m| m.starts_with("3 primary-variant instances")));
    assert!(has(&f, "page-layout"));
    assert!(has(&f, "app-shell"));
    assert!(!has(&f, "no-components"));
    // Shared screens (a path segment that starts with "_") need no page or shell.
    let shared = lint(
        &rel::screen("_shared/a.html"),
        "<body><dfw-use c=\"button\">x</dfw-use></body>",
    );
    assert_eq!(rules(&shared), Vec::<String>::new());
    let empty = lint(&rel::screen("s/b.html"), "<html>\n<body></body></html>");
    assert_eq!(lines_of(&empty, "no-components"), [2]);
    // Compose issues come first, with their line.
    let unknown = lint(
        &rel::screen("s/c.html"),
        "<body>\n<dfw-use c=\"nope\">x</dfw-use></body>",
    );
    assert_eq!(unknown[0]["line"], 2);
    assert_eq!(unknown[0]["level"], "error");
    // Without masters only the compose issues and the state coverage remain.
    let bare = test_dir("lint-screen-bare");
    let none = load_components(&bare).unwrap();
    let found = lint_screen(&bare, &rel::screen("s/a.html"), text, &none).unwrap();
    assert_eq!(rules(&found), ["component-unknown"; 5]);
}

#[test]
fn design_targets_and_file_kinds() {
    let root = test_dir("lint-design");
    write(&root, rel::MANIFEST, r#"{"screens":[]}"#);
    write(&root, &rel::approved("v1/a.html"), "<img>");
    write(&root, rel::HEAD, "<img>");
    write(&root, rel::COMPONENTS_CSS, ".a{color:#fff}");
    write(
        &root,
        &rel::join(rel::SYSTEM, file::BASE_CSS),
        ".a{color:#fff}",
    );
    write(&root, rel::TOKENS_JSON, "{ nope");
    write(
        &root,
        &rel::join(rel::TOKENS, file::TOKENS_CSS),
        ".a{color:#fff}",
    );
    write(&root, &rel::design("brand.tokens.json"), "{}");
    write(&root, &rel::design("notes.json"), "{ not linted");
    write(&root, &rel::screen("a/index.html"), "<img>");
    write(&root, &rel::design("pages/b.html"), "<img>");
    let files = |found: &[Value]| -> Vec<String> {
        found
            .iter()
            .map(|f| {
                format!(
                    "{} {}",
                    f["file"].as_str().unwrap(),
                    f["rule"].as_str().unwrap()
                )
            })
            .collect()
    };
    let at = |file: &str, rule: &str| format!("{file} {rule}");
    let screen = rel::screen("a/index.html");
    let page = rel::design("pages/b.html");
    let base_css = rel::join(rel::SYSTEM, file::BASE_CSS);
    let all = files(&lint_design(&root, None).unwrap());
    assert_eq!(
        all,
        [
            at(rel::MANIFEST, "manifest-screens"),
            at(&rel::design("brand.tokens.json"), "token-empty"),
            at(&page, "tokens-link"),
            at(&page, "img-alt"),
            at(&page, "dfw-id-missing"),
            at(&screen, "tokens-link"),
            at(&screen, "img-alt"),
            at(&screen, "dfw-id-missing"),
            at(&base_css, "hardcoded-color"),
            at(rel::TOKENS_JSON, "token-json"),
        ]
    );
    // A directory target lints every file under it; a missing file is skipped; the manifest is
    // linted only when it is a target.
    let targets = |list: &[&str]| -> Vec<String> {
        let list: Vec<String> = list.iter().map(|s| s.to_string()).collect();
        files(&lint_design(&root, Some(&list)).unwrap())
    };
    assert_eq!(
        targets(&[rel::SCREENS, &rel::design("nope.html")]),
        [
            at(&screen, "tokens-link"),
            at(&screen, "img-alt"),
            at(&screen, "dfw-id-missing"),
        ]
    );
    assert_eq!(
        targets(&[&rel::approved("v1/a.html")]),
        Vec::<String>::new()
    );
    assert_eq!(
        targets(&[&format!("./{}", rel::MANIFEST)]),
        [at(rel::MANIFEST, "manifest-screens")]
    );
    let abs = root.join(&base_css);
    assert_eq!(
        targets(&[abs.to_str().unwrap()]),
        [at(&base_css, "hardcoded-color")]
    );
    assert!(is_token_file(&rel::join(rel::THEMES, "dark.json")));
    assert!(is_token_file("x.tokens.json"));
    assert!(!is_token_file(&rel::design(file::TOKENS_JSON)));
    assert!(!is_token_file(&rel::join(rel::TOKENS, "a.css")));
}

/// Where the Node code died on a TypeError the lint ends with an error too (exit code 1): a
/// manifest of the wrong shape never passes a gate.
#[test]
fn a_manifest_of_the_wrong_shape_is_an_error_where_node_dies() {
    let root = test_dir("lint-manifest-shapes");
    write(&root, &rel::screen("a.html"), "<table></table>");
    let variant = json!({ "id": "a", "file": "screens/a.html" });
    let check = |manifest: Value| {
        write(&root, rel::MANIFEST, &manifest.to_string());
        let coverage = state_coverage(&root, &rel::screen("a.html"), "<table></table>");
        (lint_manifest(&root).is_ok(), coverage.is_ok())
    };
    // Wrong types the Node code read without trouble.
    for fine in [
        json!(5),
        json!("text"),
        json!([1]),
        json!({ "screens": 0, "pages": "ab", "states": 0 }),
        json!({ "screens": [5, "x", { "id": "a", "variants": 0 }] }),
        json!({ "screens": [{ "id": "a", "variants": [{ "file": "" }, { "file": 0 }] }] }),
        json!({ "screens": [{ "id": "a" }], "pages": [{ "screens": "ab" }, "x", 5, []] }),
        json!({ "screens": [{ "id": "a", "states": { "length": 0 }, "variants": [variant] }] }),
    ] {
        assert_eq!(check(fine.clone()), (true, true), "{fine}");
    }
    // (lint_manifest, state_coverage): which of the two the Node code died in.
    for (broken, ok) in [
        (json!({ "screens": {} }), (false, false)),
        (json!({ "screens": "ab" }), (false, false)),
        (json!({ "screens": [null] }), (false, false)),
        (json!({ "screens": [{ "variants": "x" }] }), (true, false)),
        (
            json!({ "screens": [{ "variants": [null] }] }),
            (false, false),
        ),
        (
            json!({ "screens": [{ "variants": [{ "file": 5 }] }] }),
            (false, true),
        ),
        // `find` and `some` stop at the first match: what follows it is not read.
        (
            json!({ "screens": [{ "id": "a", "states": ["loading", "empty", "error"], "variants": [variant, null] }, null] }),
            (false, true),
        ),
        (json!({ "screens": [], "pages": {} }), (false, true)),
        (json!({ "screens": [], "pages": [null] }), (false, true)),
        (
            json!({ "screens": [], "pages": [{ "screens": {} }] }),
            (false, true),
        ),
        (
            json!({ "screens": [{ "id": "a", "states": "loading", "variants": [variant] }] }),
            (true, false),
        ),
        (
            json!({ "screens": [{ "id": "a", "states": { "length": 2 }, "variants": [variant] }] }),
            (true, false),
        ),
        (
            json!({ "screens": [{ "id": "a", "variants": [variant] }], "states": { "a": 1 } }),
            (true, false),
        ),
    ] {
        assert_eq!(check(broken.clone()), ok, "{broken}");
    }
    write(&root, rel::MANIFEST, r#"{"screens":[],"pages":7}"#);
    let err = lint_manifest(&root).unwrap_err();
    assert_eq!(err.exit_code(), 1);
    assert_eq!(
        err.message(),
        format!(
            "Malformed {0}: \"pages\" must be a list. Correct it in {0} and run the command again.",
            rel::MANIFEST
        )
    );
    // Of a text, each character is a screen id.
    write(
        &root,
        rel::MANIFEST,
        r#"{"screens":[{"id":"a","variants":[]}],"pages":[{"id":"p","screens":"ab"}]}"#,
    );
    let found = lint_manifest(&root).unwrap();
    assert!(messages(&found).contains(&"Page \"p\" lists unknown screen \"b\"."));
    assert!(!messages(&found).contains(&"Page \"p\" lists unknown screen \"a\"."));
}
