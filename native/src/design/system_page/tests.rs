//! The system page cases of `tests/unit/system.test.mjs`, `studio-presence.test.mjs` and
//! `audit-fixes.test.mjs`; each test's doc comment is the title of the JS test it ports.

use crate::core::fixture::LOGO_MASTER;
use crate::core::js;
use std::fs;
use std::path::PathBuf;

use regex::Regex;

use super::components::{describe_text, Parts};
use super::text::{alias, bullets, encode_uri, has_heading, md_parts, part_body, sentences};
use super::*;
use crate::core::paths::rel;
use crate::core::util::{copy_dir, test_dir};
use crate::design::compose::{build_system, compose_html, ComposeOptions};
use crate::design::icons::add_icons;
use crate::design::inventory::kit_components;
use crate::design::system::{system_init, InitOptions};
use crate::design::tokens::build_tokens;

fn write(root: &Path, rel: &str, text: &str) {
    let file = root.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// `project()` of system.test.mjs.
fn project() -> PathBuf {
    let root = test_dir("system-page");
    write(&root, rel::STATE, "{}");
    fs::create_dir_all(root.join(rel::TOKENS)).unwrap();
    write(
        &root,
        rel::MANIFEST,
        r#"{"schema":1,"product":"T","screens":[]}"#,
    );
    root
}

/// Test fixture only: put starter-kit masters into a project (the CLI never copies them).
fn use_kit(root: &Path, names: Option<&[&str]>) {
    let dir = root.join(rel::COMPONENTS);
    fs::create_dir_all(&dir).unwrap();
    let kit = kit_components().unwrap();
    for f in dir_names(&kit).unwrap() {
        let wanted = names.is_none_or(|n| n.contains(&f.trim_end_matches(".html")));
        if f.ends_with(".html") && wanted {
            fs::copy(kit.join(&f), dir.join(&f)).unwrap();
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
    ];
    add_icons(root, &icons.map(str::to_string)).unwrap();
    build_system(root).unwrap();
}

fn composed(
    root: &Path,
    base: &str,
    compose_base: Option<&str>,
) -> crate::design::compose::Composed {
    let page = render_system_page(root, base).unwrap();
    compose_html(
        root,
        &page,
        ComposeOptions {
            lib: None,
            design_base: compose_base,
            prop_values: false,
        },
    )
    .unwrap()
}

/// design system page: a description becomes a summary, one meaning per name, and rules; nothing is dropped
#[test]
fn description_parts() {
    let description = "Text button (renders a link when href is set). primary = the one main action per view (Save, Send); ghost = low emphasis; sm = dense tables. href turns it into a link. Never two primaries in one view.";
    /// Two variants the description gives a meaning.
    const PRIMARY: &str = "primary";
    const GHOST: &str = "ghost";
    let names = [PRIMARY, "secondary", GHOST, "md", "sm", "icon", "href"];
    let p = describe_text(description, &names).unwrap();
    assert_eq!(p.summary, "Text button (renders a link when href is set).");
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        p.meaning,
        [
            pair(PRIMARY, "the one main action per view (Save, Send)"),
            pair(GHOST, "low emphasis"),
            pair("sm", "dense tables")
        ]
    );
    assert_eq!(
        p.notes,
        [
            "href turns it into a link.",
            "Never two primaries in one view."
        ]
    );
    let mut all = vec![p.summary.clone()];
    all.extend(p.notes.iter().cloned());
    all.extend(p.meaning.iter().map(|(k, v)| format!("{k} = {v}")));
    let all = all.join(" ");
    let words = description.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'));
    for word in words.filter(|w| w.len() > 3) {
        assert!(all.contains(word), "kept: {word}");
    }
    assert_eq!(describe_text("", &[]).unwrap(), Parts::default());
}

/// design system page: a dot inside a meaning does not end it
#[test]
fn a_dot_inside_a_meaning_does_not_end_it() {
    let p = describe_text(
        "Text button. sm = 1.5rem tall; lg = 2.5rem tall. Never two primaries.",
        &["sm", "lg"],
    )
    .unwrap();
    let pair = |k: &str, v: &str| (k.to_string(), v.to_string());
    assert_eq!(
        p.meaning,
        [pair("sm", "1.5rem tall"), pair("lg", "2.5rem tall")]
    );
    // A dot before white space or the end still ends the value; one before ";" stays in it.
    let p = describe_text("x. md = wide. Rest.", &["md"]).unwrap();
    assert_eq!(p.meaning, [pair("md", "wide")]);
    let p = describe_text("x. md = 2.;", &["md"]).unwrap();
    assert_eq!(p.meaning, [pair("md", "2.")]);
    let p = describe_text("a. md = f(x. y) z..w end", &["md"]).unwrap();
    assert_eq!(p.meaning, [pair("md", "f(x. y) z..w end")]);
}

/// design system page: components are documented in rows, in three text sizes
#[test]
fn components_in_rows() {
    let root = test_dir("system-page");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/penpot/project");
    copy_dir(&fixture, &root, &[]).unwrap();
    build_tokens(&root).unwrap();
    let html = composed(&root, "../", None).html;
    assert!(html.contains("<table class=\"ds-kv\">"));
    let master = Regex::new(
        r#"<th scope="row">Master</th><td><code>design/system/components/[\w-]+\.html</code>"#,
    )
    .unwrap();
    assert!(master.is_match(&html));
    let css = Regex::new(r"(?s)<style>(.*?)</style>")
        .unwrap()
        .captures(&html)
        .unwrap()[1]
        .to_string();
    let sizes: Vec<u32> = Regex::new(r"font(?:-size)?:[^;{}]*?(\d+)px")
        .unwrap()
        .captures_iter(&css)
        .map(|m| m[1].parse().unwrap())
        .collect();
    assert!(!sizes.is_empty());
    assert!(
        sizes.iter().all(|n| *n >= 12),
        "no documentation text under 12px (found {sizes:?})"
    );
}

/// 5: 3-digit hex tokens are audited and do not crash the Design System page
// The audit counts of the JS test belong to design::dsaudit; the page assertion is here.
#[test]
fn short_hex_tokens() {
    /// The test folder of both projects, and the page's path to `design/`.
    const DIR: &str = "system-page";
    const BASE: &str = "../";
    let root = test_dir(DIR);
    write(
        &root,
        rel::TOKENS_JSON,
        r##"{"color":{"$type":"color","ink":{"$value":"#ccc"},"surface":{"$value":"#fff"},"canvas":{"$value":"#fff"},"accent":{"$value":"#ddd"},"muted":{"$value":"#eee"}}}"##,
    );
    let page = render_system_page(&root, BASE).unwrap();
    assert!(page.contains("1.6:1"));
    // Short and long hex give the same findings in the "Senior review" section.
    let count = |page: &str| page.matches("<li class=\"lv-").count();
    let root_long = test_dir(DIR);
    write(
        &root_long,
        rel::TOKENS_JSON,
        r##"{"color":{"$type":"color","ink":{"$value":"#cccccc"},"surface":{"$value":"#ffffff"},"canvas":{"$value":"#ffffff"},"accent":{"$value":"#dddddd"},"muted":{"$value":"#eeeeee"}}}"##,
    );
    let long = render_system_page(&root_long, BASE).unwrap();
    assert!(count(&long) > 0);
    assert_eq!(count(&page), count(&long));
}

/// kit: every reference master composes its examples without issues and lints clean
// The page part of the JS test: the page of a project with every kit master composes clean.
#[test]
fn kit_page_composes() {
    let root = project();
    system_init(&root, &InitOptions::default()).unwrap();
    use_kit(&root, None);
    let page = composed(&root, "../", None);
    assert_eq!(page.issues, []);
    assert!(page
        .html
        .contains("data-dfw-id=\"system.component.button\""));
}

/// style guides: library, search, apply as aliased tokens with usage rules, latin fonts, motifs, options
// The page part of the JS test: concept, role table, ramps and spec tables of a style guide.
#[test]
fn style_guide_page() {
    let root = project();
    let opts = InitOptions {
        style: Some("heritage-signboard".into()),
        ..InitOptions::default()
    };
    system_init(&root, &opts).unwrap();
    use_kit(&root, Some(&["button", "text"]));
    let page = composed(&root, "../", None);
    assert_eq!(page.issues, []);
    for part in [
        "<h2>Concept</h2>",
        "class=\"ds-table\"",
        "class=\"ds-ramp\"",
        "class=\"ds-spec\"",
    ] {
        assert!(page.html.contains(part), "{part}");
    }
}

/// compose: {{base}} in masters, examples and instance props resolves to the page path to design/
// The page part of the JS test.
#[test]
fn base_in_examples() {
    let root = project();
    write(
        &root,
        &rel::component(LOGO_MASTER),
        r#"<dfw-component name="logo" props="img={{base}}assets/logo.png"><template><a href="{{base}}screens/home/a.html"><img src="{{img}}" alt=""></a></template><dfw-examples><dfw-use c="logo"></dfw-use></dfw-examples></dfw-component>"#,
    );
    /// The page's absolute path to `design/`, for the page and for the examples.
    const BASE: &str = "/design/";
    let page = composed(&root, BASE, Some(BASE)).html;
    assert!(
        page.contains("src=\"/design/assets/logo.png\""),
        "Design System page examples resolve too"
    );
    assert!(!page.contains("{{base}}"));
}

/// The text rules the page takes from JavaScript: sentence split, bullets, headings, slices.
#[test]
fn javascript_text_rules() {
    assert_eq!(sentences("A. B! c? D"), ["A.", "B! c?", "D"]);
    assert_eq!(
        sentences("a.  b"),
        ["a.", " b"],
        "two spaces back off by one"
    );
    assert_eq!(sentences("a. (b). C"), ["a. (b).", "C"]);
    assert_eq!(
        bullets("\n\n- one\r\n  * two words\n-three\n- \n", 1),
        ["one", "two words"]
    );
    assert_eq!(bullets("- short\n- twenty characters long\n", 20).len(), 1);
    let md = "intro\n## Direction  \nbody\n## Rationale and more\n- x\n## Rationale\n- y";
    let parts = md_parts(md);
    assert_eq!(parts.len(), 4);
    assert!(has_heading(parts[1], &["Direction"]));
    /// The heading a part is looked up by.
    const RATIONALE: &str = "Rationale";
    assert!(!has_heading(parts[2], &[RATIONALE]));
    assert!(has_heading(parts[3], &[RATIONALE]));
    assert_eq!(part_body(parts[3]), "- y");
    /// A text with a character of two UTF-16 units.
    const WIDE: &str = "ab\u{1F600}c";
    assert_eq!(js::slice16(WIDE, 0, 3), "ab\u{fffd}");
    assert_eq!(js::slice16(WIDE, 0, 4), "ab\u{1F600}");
    assert_eq!(encode_uri("a b/ảnh(1)#.png"), "a%20b/%E1%BA%A3nh(1)#.png");
    assert_eq!(alias(&json!("{color.ink}")), Some("color.ink"));
    assert_eq!(alias(&json!("{a}b}")), None);
}

/// The type table shows the project's own text styles: a master with `h1 lead body` lists body
/// (a style dfw knows, with its usage) then h1 and lead with their meaning, and none of the kit
/// styles it lacks (they used to be listed and rendered as the default style).
#[test]
fn type_table_follows_the_text_master() {
    /// The master's own variants, and the meaning its description gives each.
    const MASTER: &str = r#"<dfw-component name="text" variants="h1 lead body" description="Text styles. h1 = page titles; lead = the intro paragraph; body = default text."><template><p class="t t--{variant}"><dfw-slot>Text</dfw-slot></p></template></dfw-component>"#;
    let root = project();
    system_init(&root, &InitOptions::default()).unwrap();
    write(&root, &rel::master("text"), MASTER);
    let page = render_system_page(&root, "../").unwrap();
    let shown: Vec<&str> = Regex::new(r#"<code>text ([a-z0-9-]+)"#)
        .unwrap()
        .captures_iter(&page)
        .map(|c| c.get(1).unwrap().as_str())
        .collect();
    assert_eq!(shown, ["body", "h1", "lead"]);
    assert!(page.contains("<b>Body</b><span>Default reading text</span>"));
    assert!(page.contains("<b>h1</b><span>page titles</span>"));
    assert!(page.contains("<b>lead</b><span>the intro paragraph</span>"));
}

/// The page's rules on bare elements never reach a component: each one skips `.ds-live`, and
/// every place where masters render (examples, app-shell frames, the type samples) is `.ds-live`.
/// `.ds code` used to set 13px on a master's own `<code>`, `.ds p` a 75ch width on its `<p>`.
#[test]
fn page_styles_leave_the_components_alone() {
    let css = include_str!("../system_page.css");
    let bare = Regex::new(r"\.ds\s+(?:[a-z][a-z0-9]*|:where\([^)]*\))[^{]*\{").unwrap();
    for rule in bare.find_iter(css) {
        assert!(
            rule.as_str().contains(":where(:not(.ds-live *))"),
            "a page rule on bare elements reaches the components: {}",
            rule.as_str()
        );
    }
    let root = project();
    system_init(&root, &InitOptions::default()).unwrap();
    use_kit(&root, Some(&["button", "text", "app-shell"]));
    let page = render_system_page(&root, "../").unwrap();
    for live in [
        "class=\"ds-examples ds-live\"",
        "class=\"ds-frame__inner ds-live\"",
        "<td class=\"ds-live\"><dfw-use c=\"text\"",
    ] {
        assert!(page.contains(live), "{live}");
    }
}
