//! The cases of tests/unit/compose.test.mjs (through `compose_html` and the build outputs).

use super::*;
use crate::core::fixture::{
    APP_SHELL_MASTER, COMPOSED_PAGE, DIVIDER_MASTER, HTML_FILE_B, INDEX_FILE, LOGO_MASTER,
    MISSING_HTML_FILE, NESTED_PAGE, PLAIN_PAGE,
};
use crate::core::paths::file;
use crate::core::util::test_dir;

fn project(files: &[(&str, &str)]) -> PathBuf {
    let root = test_dir("compose");
    for (rel, text) in files {
        let file = root.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }
    root
}

fn master(name: &str, tpl: &str, attrs: &str) -> String {
    format!("<dfw-component name=\"{name}\"{attrs}>\n<template>\n{tpl}\n</template>\n</dfw-component>\n")
}

fn compose(root: &Path, html: &str) -> Composed {
    compose_html(root, html, ComposeOptions::default()).unwrap()
}

fn rules(c: &Composed) -> Vec<&str> {
    c.issues.iter().map(|i| i.rule).collect()
}

/// compose: unknown component and icon are reported, recursion is bounded
#[test]
fn unknown_component_and_icon_and_bounded_recursion() {
    let root = project(&[]);
    let r = compose(
        &root,
        "<dfw-use c=\"ghost\">x</dfw-use><dfw-icon name=\"nope\"></dfw-icon>",
    );
    assert_eq!(rules(&r), ["component-unknown", "icon-unknown"]);
    assert_eq!(r.html, "<div data-dfw-missing-component=\"ghost\">x</div><span data-dfw-missing-icon=\"nope\"></span>");
    let root = project(&[(&rel::master("loop"), "<dfw-component name=\"loop\"><template><div><dfw-use c=\"loop\"></dfw-use></div></template></dfw-component>")]);
    let r2 = compose(&root, "<dfw-use c=\"loop\"></dfw-use>");
    assert!(rules(&r2).contains(&"compose-depth"));
    assert_eq!(r2.instances.len(), 13);
}

/// compose: a nested instance keeps its own slot fills
#[test]
fn nested_instance_keeps_its_own_slot_fills() {
    let root = project(&[
        (&rel::master("box"), "<dfw-component name=\"box\"><template><section><dfw-slot></dfw-slot><footer><dfw-slot name=\"legend\"></dfw-slot></footer></section></template></dfw-component>"),
        (&rel::master("bar"), "<dfw-component name=\"bar\"><template><div class=\"bar\"><dfw-slot></dfw-slot><ul><dfw-slot name=\"legend\"></dfw-slot></ul></div></template></dfw-component>"),
    ]);
    let r = compose(&root, "<dfw-use c=\"box\"><dfw-use c=\"bar\"><i>seg</i><dfw-fill slot=\"legend\"><li>inner</li></dfw-fill></dfw-use><dfw-fill slot=\"legend\">outer</dfw-fill></dfw-use>");
    assert!(r.html.contains("<ul><li>inner</li></ul>"), "{}", r.html);
    assert!(r.html.contains("<footer>outer</footer>"), "{}", r.html);
}

/// compose: {{prop==value?text}} marks the current item of a shared shell
#[test]
fn conditional_text_marks_the_current_item() {
    let root = project(&[(&rel::master("shell"), "<dfw-component name=\"shell\" props=\"current\"><template><nav><a aria-current=\"{{current==home?page}}\">H</a><a aria-current=\"{{current==list?page}}\">L</a></nav><main><dfw-slot></dfw-slot></main></template></dfw-component>")]);
    let r = compose(&root, "<dfw-use c=\"shell\" current=\"list\">x</dfw-use>");
    assert!(
        r.html
            .contains("<a aria-current=\"\">H</a><a aria-current=\"page\">L</a>"),
        "{}",
        r.html
    );
}

/// compose: prop lists with spaces, commas and spaced defaults
#[test]
fn prop_lists_with_spaces_commas_and_spaced_defaults() {
    let a = &parse_component_file("<dfw-component name=\"x\" props=\"asset name location site=Asset Tracking, printed\"><template><i></i></template></dfw-component>", "x.html")[0];
    /// The prop with a default of two words.
    const SITE: &str = "site";
    assert_eq!(a.props, ["asset", "name", "location", SITE, "printed"]);
    assert_eq!(
        a.prop_defaults,
        [(SITE.to_string(), "Asset Tracking".to_string())]
    );
    let b = &parse_component_file("<dfw-component name=\"y\" props=\"as=p tone=default\"><template><i></i></template></dfw-component>", "y.html")[0];
    assert_eq!(
        b.prop_defaults,
        [
            ("as".to_string(), "p".to_string()),
            ("tone".to_string(), "default".to_string())
        ]
    );
}

/// compose: {{base}} in masters, examples and instance props resolves to the page path to
/// design/ (the Design System page assertions belong to design/system_page)
#[test]
fn base_placeholder_resolves_to_the_page_path() {
    let root = project(&[
        (&rel::component(LOGO_MASTER), "<dfw-component name=\"logo\" props=\"img={{base}}assets/logo.png\"><template><a href=\"{{base}}screens/home/a.html\"><img src=\"{{img}}\" alt=\"\"></a></template><dfw-examples><dfw-use c=\"logo\"></dfw-use></dfw-examples></dfw-component>"),
        (&rel::master("tile"), "<dfw-component name=\"tile\" props=\"base\"><template><i data-b=\"{{base}}\"></i></template></dfw-component>"),
    ]);
    let with_base = |html: &str, base: &str| {
        compose_html(
            &root,
            html,
            ComposeOptions {
                lib: None,
                design_base: Some(base),
                prop_values: false,
            },
        )
        .unwrap()
        .html
    };
    let screen = with_base("<dfw-use c=\"logo\"></dfw-use><dfw-use c=\"logo\" img=\"{{base}}assets/alt.png\"></dfw-use>", "../../");
    assert!(
        screen.contains("href=\"../../screens/home/a.html\""),
        "{screen}"
    );
    assert!(screen.contains("src=\"../../assets/logo.png\""));
    assert!(screen.contains("src=\"../../assets/alt.png\""));
    assert!(with_base("<dfw-use c=\"logo\"></dfw-use>", "/design/")
        .contains("src=\"/design/assets/logo.png\""));
    assert!(
        compose(&root, "<dfw-use c=\"tile\" base=\"x\"></dfw-use>")
            .html
            .contains("data-b=\"x\""),
        "a master that declares a \"base\" prop keeps it"
    );
}

/// compose: with `prop_values` (the Studio), an instance's root carries the prop values written
/// on its `<dfw-use>` as JSON, escaped, without the master's defaults and with `{{base}}` kept
/// as written; every other output stays without it
#[test]
fn prop_values_reach_the_instance_root_only_when_asked() {
    /// The master of the instance.
    const CHIP: &str = "chip";
    /// What comes right before the props JSON.
    const PROPS_ATTR: &str = "data-dfw-props=\"";
    let root = project(&[(
        &rel::master(CHIP),
        &master(
            CHIP,
            "<b>{{label}}</b>",
            " props=\"label, note, img, level=h2\"",
        ),
    )]);
    let page = "<dfw-use c=\"chip\" id=\"x.chip\" label=\"Quế &amp; &quot;hoa&quot;\" img=\"{{base}}a.png\" title=\"t\"></dfw-use>";
    let studio = compose_html(
        &root,
        page,
        ComposeOptions {
            lib: None,
            design_base: Some("/design/"),
            prop_values: true,
        },
    )
    .unwrap()
    .html;
    let start = studio.find(PROPS_ATTR).unwrap() + PROPS_ATTR.len();
    let attr = &studio[start..start + studio[start..].find('"').unwrap()];
    let decoded = attr.replace("&quot;", "\"").replace("&amp;", "&");
    let props: Value = serde_json::from_str(&decoded).unwrap();
    assert_eq!(
        props,
        json!({ "label": "Quế & \"hoa\"", "img": "{{base}}a.png" }),
        "{studio}"
    );
    assert!(studio.contains("data-dfw-id=\"x.chip\""), "{studio}");
    assert!(!compose(&root, page).html.contains(PROPS_ATTR));
}

/// 1: an unclosed <dfw-use> is reported and does not swallow the page; comments are not
/// expanded (tests/unit/audit-fixes.test.mjs; the lintScreen assertion belongs to design/lint)
#[test]
fn unclosed_instance_is_reported_and_comments_are_not_expanded() {
    let root = project(&[(
        &rel::component(DIVIDER_MASTER),
        &master("divider", "<hr class=\"divider\">", ""),
    )]);
    let unclosed = "<body>\n<dfw-use c=\"divider\">\n<main data-dfw-id=\"main\"><h1>Title</h1></main>\n</body>";
    let r = compose(&root, unclosed);
    assert!(r.html.contains("<main data-dfw-id=\"main\">"));
    assert_eq!(r.html, unclosed);
    let got: Vec<(&str, &str, i64)> = r.issues.iter().map(|i| (i.rule, i.level, i.line)).collect();
    assert_eq!(got, [("component-unclosed", "error", 2)]);
    assert_eq!(
        r.issues[0].message,
        "<dfw-use> has no closing </dfw-use> tag."
    );

    let commented =
        "<body>\n<!-- TODO replace with <dfw-use c=\"divider\"> later -->\n<main>x</main>\n</body>";
    let c = compose(&root, commented);
    assert_eq!(c.html, commented);
    assert!(c.issues.is_empty());
}

/// 8: data-dfw-if with single quotes is evaluated; a template may start with a comment
#[test]
fn single_quoted_condition_and_leading_comment() {
    let root = project(&[
        (&rel::master("c1"), &master("c1", "<div class=\"c1\"><span data-dfw-if='a'>A</span><b data-dfw-if=\"b\">B</b><i data-dfw-if=a>I</i></div>", " props=\"a b\"")),
        (&rel::master("c2"), &master("c2", "<!-- root -->\n<div class=\"c2\"><dfw-slot></dfw-slot></div>", "")),
    ]);
    assert_eq!(
        compose(&root, "<dfw-use c=\"c1\" a=\"1\"></dfw-use>").html,
        "<div class=\"c1\" data-dfw-component=\"c1\"><span>A</span><i>I</i></div>"
    );
    assert_eq!(
        compose(
            &root,
            "<dfw-use c=\"c2\" id=\"x.y\" class=\"wide\">hi</dfw-use>"
        )
        .html,
        "<div class=\"c2 wide\" data-dfw-id=\"x.y\" data-dfw-component=\"c2\">hi</div>"
    );
}

/// slot content: slot children render inside the master-built container (the composeHtml
/// assertion of tests/unit/edit.test.mjs)
#[test]
fn slot_children_render_inside_the_master_container() {
    let root = project(&[(&rel::component(APP_SHELL_MASTER), "<dfw-component name=\"app-shell\">\n<template>\n<div class=\"shell\"><nav data-dfw-id=\"shell.nav\">Nav</nav><main class=\"shell-main\" data-dfw-id=\"shell.main\"><dfw-slot name=\"main\"></dfw-slot></main></div>\n</template>\n</dfw-component>")]);
    let r = compose(&root, "<dfw-use c=\"app-shell\">\n  <dfw-fill slot=\"main\">\n      <section data-dfw-id=\"orders.summary\">S</section>\n  </dfw-fill>\n</dfw-use>");
    assert!(r.html.contains("<main class=\"shell-main\" data-dfw-id=\"shell.main\">\n      <section data-dfw-id=\"orders.summary\">"), "{}", r.html);
}

#[test]
fn conditions_slots_and_root_attributes_follow_the_node_rules() {
    let root = project(&[
        (&rel::master("f"), &master("f", "<p class=\"f f--{{ variant }}\" data-x='1'\n   data-dfw-if=\"variant!=hidden\"><em data-dfw-if=\"!note\">none</em><em data-dfw-if=\"note\">{{note}}</em><dfw-slot>fallback</dfw-slot><dfw-slot name=\"end\"/><dfw-slot name='odd'></dfw-slot>{{missing}}{{ tone==warn ? a < b }}</p>", " variants=\"plain hidden\" sizes=\"md\" props=\"note, tone=warn\"")),
    ]);
    let r = compose(&root, "<dfw-use c=\"f\" note=\"a &amp; &lt;b&gt;\" hidden data-k=\"\" title=\"\" aria-label=\"\" data-dfw-id=\"lost\">  <dfw-fill slot=\"end\">E</dfw-fill></dfw-use>");
    assert_eq!(
        r.html,
        "<p class=\"f f--plain\" data-x=\"1\" hidden data-k=\"\" title aria-label=\"\" data-dfw-component=\"f\" data-dfw-variant=\"plain\" data-dfw-size=\"md\"><em>a &amp; &lt;b&gt;</em>fallbackE<dfw-slot name='odd'></dfw-slot>a &lt; b </p>"
    );
    assert_eq!(
        compose(&root, "<dfw-use c=\"f\" variant=\"hidden\">x</dfw-use>").html,
        ""
    );
    let bad = compose(
        &root,
        "a\n<dfw-use c=\"f\" variant=\"nope\" size=\"xl\" id=\"\">x</dfw-use>",
    );
    assert_eq!(rules(&bad), ["component-variant", "component-size"]);
    assert_eq!(
        bad.issues[0].message,
        "Component \"f\" has no variant \"nope\" (variants: plain, hidden)."
    );
    assert_eq!(bad.issues[1].line, 2);
    assert_eq!(bad.instances[0].id, None);
    assert!(bad.html.contains("data-dfw-id=\"\""));
}

#[test]
fn icons_head_and_build_outputs() {
    let root = project(&[
        (&rel::join(rel::ICONS, &file::icon("x")), "<?xml version=\"1.0\"?>\n<!-- lucide -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" class=\"lucide\"><path d=\"M1 1\"/></svg>\n"),
        (rel::HEAD, "<!-- fonts -->\n<link rel=\"stylesheet\" href=\"{{base}}fonts.css\">\n"),
        (&rel::component(HTML_FILE_B), "<dfw-component name=\"b\"><style>\n.b { color: red; }\n</style><template><b class=\"b\"><dfw-slot></dfw-slot></b></template></dfw-component>"),
        (rel::BASE_CSS, "body{margin:0}"),
        (&rel::join(rel::TOKENS, file::TOKENS_CSS), ":root{}"),
        (&rel::screen(COMPOSED_PAGE), "<html>\n<head>\n<dfw-head>\n</dfw-head>\n</head>\n<body>\n<dfw-use c=\"b\" id=\"go\">Go</dfw-use><dfw-icon name=\"x\" size=\"16\" label=\"Close\" id=\"i\" class=\"big\"></dfw-icon><dfw-icon name=\"../x\"></dfw-icon></body></html>"),
        (&rel::screen(PLAIN_PAGE), "<html><body>plain</body></html>"),
    ]);
    let file = compose_file(&root, &rel::screen(COMPOSED_PAGE)).unwrap();
    assert!(file.needs_components_css);
    let c = &file.composed;
    assert!(c.html.starts_with("<html>\n<head>\n<link rel=\"stylesheet\" href=\"../../fonts.css\">\n<link rel=\"stylesheet\" href=\"../../tokens/tokens.css\">\n<link rel=\"stylesheet\" href=\"../../styles/base.css\">\n<link rel=\"stylesheet\" href=\"../../system/components.css\">\n</head>"), "{}", c.html);
    assert!(c
        .html
        .contains("<b class=\"b\" data-dfw-id=\"go\" data-dfw-component=\"b\">Go</b>"));
    assert!(c.html.contains("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" class=\"lucide icon big\" role=\"img\" aria-label=\"Close\" data-dfw-icon=\"x\" data-dfw-id=\"i\"><path d=\"M1 1\"/></svg>"), "{}", c.html);
    assert_eq!(c.instances[0].line, 7, "lines are those of the source file");
    assert_eq!(rules(c), ["icon-unknown"]);
    assert_eq!(c.issues[0].line, 7);

    let built = build_system(&root).unwrap();
    assert_eq!(
        (built.components, built.written.as_slice()),
        (1, &[rel::COMPONENTS_CSS.to_string()][..])
    );
    assert_eq!(fs::read_to_string(root.join(rel::COMPONENTS_CSS)).unwrap(), format!("/* Generated by dfw from {}/*.html. Do not edit by hand. */\n\n/* b ({}) */\n.b {{ color: red; }}\n", rel::COMPONENTS, rel::component(HTML_FILE_B)));
    assert!(
        build_system(&root).unwrap().written.is_empty(),
        "unchanged text is not rewritten"
    );

    let rels = [
        rel::screen(COMPOSED_PAGE),
        rel::screen(PLAIN_PAGE),
        rel::design(MISSING_HTML_FILE),
    ];
    let out = build_composed(&root, &rels).unwrap();
    assert_eq!(out.len(), 2);
    assert!(out[0].composed && !out[1].composed);
    assert_eq!(out[1].out, root.join(rel::screen(PLAIN_PAGE)));
    let html = fs::read_to_string(&out[0].out).unwrap();
    assert!(html.starts_with("<html>\n<head>\n<base href=\"../../../../../design/screens/s/\">\n<link rel=\"stylesheet\" href=\"../../fonts.css\">\n<style data-dfw-inlined=\"../../tokens/tokens.css\">\n:root{}\n</style>\n<style data-dfw-inlined=\"../../styles/base.css\">\nbody{margin:0}\n</style>"), "{html}");
    assert_eq!(list_icons(&root).unwrap(), ["x"]);
    assert_eq!(design_base_for(&rel::screen(NESTED_PAGE)), "../../");
    assert_eq!(design_base_for(&rel::design(INDEX_FILE)), "./");
}
