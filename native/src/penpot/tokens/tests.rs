//! tests/unit/penpot-tokens.test.mjs, and the token cases of penpot-hardening.test.mjs.
//! The CLI case is in native/tests/penpot.rs (it needs the binary).

use super::*;
use crate::core::fixture::DARK_THEME;
use crate::core::js;
use crate::core::util::{test_dir, PROTO};
use crate::design::tokens::tokens_to_declarations;
use std::path::PathBuf;

/// A colour of the fixtures, as `#rrggbb`.
const BLUE: &str = "#0066ff";
/// The dark theme: its name and its Penpot set.
const DARK: &str = "dark";
const DARK_SET: &str = "themes/dark";
/// The group of the colour tokens in the fixtures.
const COLOR_GROUP: &str = "color";
/// The field of a report entry that names the token.
const TOKEN_FIELD: &str = "token";
/// A token the edited Penpot file deletes (kept, or removed with `--prune`).
const VEIL: &str = "color.veil";

fn fix() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/penpot")
}

fn fixture(name: &str) -> Value {
    js::read_json(&fix().join(name), None).unwrap()
}

/// `assert.deepEqual` on JSON: key order and `1` against `1.0` do not matter.
#[track_caller]
fn assert_json(actual: &Value, expected: Value) {
    assert!(
        same(actual, &expected),
        "{}\n  is not\n{}",
        to_json_compact(actual),
        to_json_compact(&expected)
    );
}

fn tok(ty: &str, value: Value) -> Token {
    Token {
        ty: Some(ty.into()),
        value,
        description: None,
    }
}

fn to(name: &str, ty: &str, value: Value) -> Converted {
    token_to_penpot_quiet(name, &tok(ty, value))
}

fn from(name: &str, ty: &str, value: Value) -> Converted {
    token_from_penpot(name, &tok(ty, value), None, &mut |_| {}).unwrap()
}

fn value_of(c: &Converted) -> Value {
    c.value().cloned().unwrap()
}

fn project(tokens: bool) -> PathBuf {
    let root = test_dir("penpot-tokens");
    fs::create_dir_all(root.join(rel::DFW)).unwrap();
    fs::write(root.join(rel::STATE), "{}").unwrap();
    fs::create_dir_all(root.join(rel::THEMES)).unwrap();
    if tokens {
        fs::copy(fix().join("dfw-tokens.json"), root.join(rel::TOKENS_JSON)).unwrap();
        fs::copy(
            fix().join("dfw-theme-dark.json"),
            root.join(rel::join(rel::THEMES, DARK_THEME)),
        )
        .unwrap();
    }
    root
}

fn import(root: &Path, file: &Path, prune: bool) -> TokenImport {
    import_penpot_tokens(root, file, None, false, prune).unwrap()
}

fn names(list: &[Value]) -> Vec<String> {
    let mut names: Vec<String> = list
        .iter()
        .map(|c| js::to_string(c.get(TOKEN_FIELD)))
        .collect();
    names.sort();
    names
}

fn diff_of<'a>(r: &'a TokenImport, set: &str) -> &'a SetDiff {
    &r.diff.iter().find(|(s, _)| s == set).unwrap().1
}

/* ------------------------------------------------------------ per-type mapping */

/// penpot tokens: colour mapping both ways
#[test]
fn colour_mapping_both_ways() {
    let t = |value: Value| to("color.x", dtcg::COLOR, value);
    assert_json(
        &t(json!({ "colorSpace": color_space::SRGB, "components": [0, 0.4, 1], "hex": "#0066FF" }))
            .to_json(),
        json!({ "type": penpot_type::COLOR, "value": BLUE }),
    );
    assert_eq!(
        value_of(&t(
            json!({ "colorSpace": color_space::SRGB, "components": [0, 0, 0], "alpha": 0.5 })
        )),
        "rgba(0, 0, 0, 0.5)"
    );
    assert_eq!(value_of(&t(json!("{color.blue}"))), "{color.blue}");
    let mut notes = Vec::new();
    let oklch = token_to_penpot(
        "c",
        &tok(
            dtcg::COLOR,
            json!({ "colorSpace": color_space::OKLCH, "components": [0.6, 0.1, 250] }),
        ),
        &mut |n| notes.push(n),
    );
    let hex = value_of(&oklch);
    let hex = hex.as_str().unwrap();
    assert!(hex.len() == 7 && hex.starts_with('#') && HEX_COLOR.is_match(hex));
    assert!(notes[0].contains(color_space::OKLCH));
    assert!(t(json!({ "colorSpace": "lab", "components": [50, 0, 0] }))
        .unsupported()
        .is_some());

    assert_json(
        &parse_color(BLUE).unwrap(),
        json!({ "colorSpace": color_space::SRGB, "components": [0, 0.4, 1], "hex": BLUE }),
    );
    assert_json(
        &parse_color("#fff").unwrap(),
        json!({ "colorSpace": color_space::SRGB, "components": [1, 1, 1], "hex": "#ffffff" }),
    );
    assert_eq!(parse_color("#00000080").unwrap()["alpha"], json!(0.502));
    assert_json(
        &parse_color("rgba(255, 102, 0, 0.5)").unwrap(),
        json!({ "colorSpace": color_space::SRGB, "components": [1, 0.4, 0], "alpha": 0.5, "hex": "#ff6600" }),
    );
    assert_eq!(parse_color("hsl(210, 50%, 40%)").unwrap()["hex"], "#336699");
    assert_eq!(parse_color("darken({a}, 10)"), None);
}

/// penpot tokens: dimension types by name (radius, spacing, sizing, border width, font size)
#[test]
fn dimension_types_by_name() {
    const FONT_SIZE: &str = "font.size.md";
    const REM: &str = "rem";
    /// A Penpot math expression and the token it is the value of.
    const SPACE_8: &str = "space.8";
    const TWICE_SPACE_4: &str = "{space.4} * 2";
    let px = json!({ "value": 4, "unit": unit::PX });
    assert_eq!(
        penpot_dimension_type("radius.md", &px),
        penpot_type::BORDER_RADIUS
    );
    assert_eq!(
        penpot_dimension_type("space.1-5", &px),
        penpot_type::SPACING
    );
    assert_eq!(
        penpot_dimension_type("size.control", &px),
        penpot_type::SIZING
    );
    assert_eq!(
        penpot_dimension_type("border.width", &px),
        penpot_type::BORDER_WIDTH
    );
    assert_eq!(
        penpot_dimension_type("focus.width", &px),
        penpot_type::BORDER_WIDTH
    );
    assert_eq!(
        penpot_dimension_type(FONT_SIZE, &px),
        penpot_type::FONT_SIZES
    );
    assert_eq!(
        penpot_dimension_type(FONT_SIZE, &json!({ "value": 1, "unit": REM })),
        penpot_type::DIMENSION
    );
    assert_eq!(
        penpot_dimension_type(
            "font.tracking.tight",
            &json!({ "value": 0.5, "unit": "px" })
        ),
        penpot_type::LETTER_SPACING
    );
    assert_eq!(
        penpot_dimension_type("elevation.offset", &px),
        penpot_type::DIMENSION
    );
    assert_json(
        &to(
            "space.4",
            dtcg::DIMENSION,
            json!({ "value": 16, "unit": "px" }),
        )
        .to_json(),
        json!({ "type": penpot_type::SPACING, "value": "16px" }),
    );
    for pt in [
        penpot_type::DIMENSION,
        penpot_type::SPACING,
        penpot_type::SIZING,
        penpot_type::BORDER_RADIUS,
        penpot_type::BORDER_WIDTH,
        penpot_type::FONT_SIZES,
        penpot_type::LETTER_SPACING,
    ] {
        assert_json(
            &value_of(&from("x", pt, json!("12"))),
            json!({ "value": 12, "unit": "px" }),
        );
    }
    assert_json(
        &parse_dimension(Some(&json!("1.5rem"))).unwrap(),
        json!({ "value": 1.5, "unit": REM }),
    );
    let mut w = Vec::new();
    let expr = token_from_penpot(
        SPACE_8,
        &tok(penpot_type::SPACING, json!(TWICE_SPACE_4)),
        Some(&json!(dtcg::DIMENSION)),
        &mut |x| w.push(x),
    )
    .unwrap();
    assert_eq!(value_of(&expr), "calc({space.4} * 2)");
    assert!(w[0].contains("math expression"));
    let css = tokens_to_declarations(
        &json!({ "space": { "8": { "$type": dtcg::DIMENSION, "$value": value_of(&expr) } } }),
    )
    .unwrap();
    assert_eq!(
        css,
        [(
            "--space-8".to_string(),
            "calc(var(--space-4) * 2)".to_string()
        )]
    );
    assert_eq!(
        value_of(&to(SPACE_8, dtcg::DIMENSION, value_of(&expr))),
        TWICE_SPACE_4
    );
}

/// penpot tokens: font family, weight, number, duration, string, unsupported
#[test]
fn font_family_weight_number_duration_string_unsupported() {
    const GENERIC: &str = "sans-serif";
    /// 0.3 s in milliseconds, as Penpot writes it.
    const SLOW_MS: &str = "300";
    const SLOW: &str = "duration.slow";
    const CASE: &str = "uppercase";
    assert_json(
        &to(
            "font.family.body",
            dtcg::FONT_FAMILY,
            json!(["Inter", "Segoe UI", GENERIC]),
        )
        .to_json(),
        json!({ "type": penpot_type::FONT_FAMILIES, "value": "Inter, Segoe UI, sans-serif" }),
    );
    assert_json(
        &value_of(&from(
            "f",
            penpot_type::FONT_FAMILIES,
            json!("\"Source Sans 3\", sans-serif"),
        )),
        json!(["Source Sans 3", GENERIC]),
    );
    assert_json(
        &to("w", dtcg::FONT_WEIGHT, json!(600)).to_json(),
        json!({ "type": penpot_type::FONT_WEIGHTS, "value": "600" }),
    );
    assert_json(
        &value_of(&from("w", penpot_type::FONT_WEIGHTS, json!("Semi Bold"))),
        json!(600),
    );
    assert_json(
        &value_of(&from("w", penpot_type::FONT_WEIGHTS, json!(SLOW_MS))),
        json!(300),
    );
    assert_json(
        &to("n", dtcg::NUMBER, json!(1.5)).to_json(),
        json!({ "type": penpot_type::NUMBER, "value": "1.5" }),
    );
    assert_json(&value_of(&from("o", "opacity", json!("45%"))), json!(0.45));
    let mut notes = Vec::new();
    let slow = token_to_penpot(
        SLOW,
        &tok(dtcg::DURATION, json!({ "value": 0.3, "unit": "s" })),
        &mut |n| notes.push(n),
    );
    assert_json(
        &slow.to_json(),
        json!({ "type": penpot_type::NUMBER, "value": SLOW_MS }),
    );
    assert!(notes[0].contains("no duration type"));
    let duration = json!({ "type": dtcg::DURATION, "value": { "value": 300, "unit": "ms" } });
    assert_json(
        &from(SLOW, penpot_type::NUMBER, json!(SLOW_MS)).to_json(),
        duration.clone(),
    );
    let preferred = token_from_penpot(
        "motion.x",
        &tok(penpot_type::NUMBER, json!(SLOW_MS)),
        Some(&json!(dtcg::DURATION)),
        &mut |_| {},
    )
    .unwrap();
    assert_json(&preferred.to_json(), duration);
    assert_json(
        &to("font.eyebrow.case", dtcg::STRING, json!(CASE)).to_json(),
        json!({ "type": penpot_type::TEXT_CASE, "value": CASE }),
    );
    assert_json(
        &from("c", penpot_type::TEXT_CASE, json!(CASE)).to_json(),
        json!({ "type": dtcg::STRING, "value": CASE }),
    );
    assert!(to("s", dtcg::STRING, json!("hello"))
        .unsupported()
        .is_some());
    assert!(to("e", "cubicBezier", json!([0.2, 0, 0, 1]))
        .unsupported()
        .is_some());
    assert!(to("b", "border", json!({})).unsupported().is_some());
    assert!(from("b", "boolean", json!("true")).unsupported().is_some());
    const INK: &str = "Ink";
    let described = token_to_penpot_quiet(
        "d",
        &Token {
            ty: Some(dtcg::COLOR.into()),
            value: json!({ "colorSpace": color_space::SRGB, "components": [0, 0, 0], "hex": "#000000" }),
            description: Some(INK.into()),
        },
    );
    assert_eq!(described.description(), Some(&json!(INK)));
}

/// penpot tokens: shadow and typography composites
#[test]
fn shadow_and_typography_composites() {
    const SHADOW: &str = "shadow.sm";
    const BODY: &str = "type.body";
    const FAMILY: &str = "Inter";
    let shadow = json!([{ "color": { "colorSpace": color_space::SRGB, "components": [0, 0, 0], "hex": "#000000", "alpha": 0.5 }, "offsetX": { "value": 0, "unit": "px" }, "offsetY": { "value": 2, "unit": "px" }, "blur": { "value": 4, "unit": "px" }, "spread": { "value": 0, "unit": "px" } }]);
    let ps = to(SHADOW, dtcg::SHADOW, shadow);
    assert_json(
        &ps.to_json(),
        json!({ "type": penpot_type::SHADOW, "value": [{ "color": "rgba(0, 0, 0, 0.5)", "offsetX": "0", "offsetY": "2", "blur": "4", "spread": "0", "inset": false }] }),
    );
    let back = value_of(&from(SHADOW, penpot_type::SHADOW, value_of(&ps)));
    assert_json(&back[0]["offsetY"], json!({ "value": 2, "unit": "px" }));
    assert_json(&back[0]["color"]["alpha"], json!(0.5));
    let inner = from(
        "s",
        penpot_type::SHADOW,
        json!({ "color": "{color.ink}", "x": "0", "y": "1", "blur": "0", "spread": "0", "type": "innerShadow" }),
    );
    assert_eq!(value_of(&inner)["inset"], true);

    let typo = json!({ "fontFamily": "{font.family.body}", "fontSize": { "value": 16, "unit": "px" }, "fontWeight": 600, "letterSpacing": { "value": 0, "unit": "px" }, "lineHeight": 1.4 });
    let pt = to(BODY, dtcg::TYPOGRAPHY, typo.clone());
    assert_json(
        &value_of(&pt),
        json!({ "fontFamilies": "{font.family.body}", "fontSizes": "16px", "fontWeights": "600", "letterSpacing": "0px", "lineHeights": "1.4" }),
    );
    assert_json(
        &value_of(&from(BODY, penpot_type::TYPOGRAPHY, value_of(&pt))),
        typo,
    );
    assert_json(
        &value_of(&from(
            "t",
            penpot_type::TYPOGRAPHY,
            json!({ "fontFamily": FAMILY, "fontSize": "14", "lineHeight": "150%" }),
        )),
        json!({ "fontFamily": [FAMILY], "fontSize": { "value": 14, "unit": "px" }, "lineHeight": 1.5 }),
    );
}

/* ------------------------------------------------------------ file structure */

/// penpot tokens: export structure (sets, themes, metadata, aliases, unsupported reported)
#[test]
fn export_structure_sets_themes_metadata() {
    let mut themes = Map::new();
    themes.insert(DARK.into(), fixture("dfw-theme-dark.json"));
    let (doc, report) = dfw_to_penpot(Some(&fixture("dfw-tokens.json")), &themes);
    let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
    assert_eq!(keys, [BASE_SET, DARK_SET, key::THEMES, key::METADATA]);
    assert_json(
        &doc[key::METADATA],
        json!({ "tokenSetOrder": [BASE_SET, DARK_SET], "activeThemes": ["dfw/default"], "activeSets": [BASE_SET] }),
    );
    let themes: Vec<Value> = doc[key::THEMES]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| json!([t["group"], t["name"], t["selectedTokenSets"]]))
        .collect();
    assert_json(
        &json!(themes),
        json!([[THEME_GROUP, "default", { BASE_SET: ENABLED }], [THEME_GROUP, DARK, { BASE_SET: ENABLED, DARK_SET: ENABLED }]]),
    );
    assert_json(
        &doc[BASE_SET][COLOR_GROUP]["accent"],
        json!({ "$value": "{color.blue.600}", "$type": penpot_type::COLOR }),
    );
    const INK: &str = "ink";
    assert_eq!(
        doc[BASE_SET][COLOR_GROUP][INK][key::DESCRIPTION],
        "Body text"
    );
    assert_eq!(doc[DARK_SET][COLOR_GROUP][INK][key::VALUE], "#f0f0f2");
    assert_eq!(
        names(&report.unsupported),
        ["easing.out", "font.eyebrow.note"]
    );
    assert!(report.lossy.iter().any(|l| l["token"] == "duration.fast"));
    // Every exported token has a Penpot type name.
    fn walk(n: &Value, types: &mut Vec<String>) {
        let Value::Object(map) = n else { return };
        if map.contains_key(key::VALUE) {
            types.push(js::to_string(map.get(key::TYPE)));
        } else {
            map.values().for_each(|v| walk(v, types));
        }
    }
    let mut types = Vec::new();
    walk(&doc[BASE_SET], &mut types);
    types.sort();
    types.dedup();
    assert_eq!(
        types,
        [
            penpot_type::BORDER_RADIUS,
            penpot_type::BORDER_WIDTH,
            penpot_type::COLOR,
            penpot_type::DIMENSION,
            penpot_type::FONT_FAMILIES,
            penpot_type::FONT_SIZES,
            penpot_type::FONT_WEIGHTS,
            penpot_type::NUMBER,
            penpot_type::SHADOW,
            penpot_type::SIZING,
            penpot_type::SPACING,
            penpot_type::TEXT_CASE,
            penpot_type::TYPOGRAPHY
        ]
    );
}

/// penpot tokens: single-set files and set mapping
#[test]
fn single_set_files_and_set_mapping() {
    /// The name a single-set file is read under.
    const FALLBACK: &str = "core";
    /// The first set of a multi-set file, and a set that is neither base nor theme.
    const BRAND: &str = "brand";
    const OTHER: &str = "Light/Colors";
    let single = parse_penpot_json(
        json!({ "color": { "a": { "$value": "#000000", "$type": "color" } } }),
        FALLBACK,
    )
    .unwrap();
    assert_eq!(
        map_sets(&single).unwrap().base_name.as_deref(),
        Some(FALLBACK)
    );
    let multi = parse_penpot_json(
        json!({ BRAND: {}, DARK_SET: {}, OTHER: {}, "$themes": [], "$metadata": { "tokenSetOrder": [BRAND, DARK_SET, OTHER] } }),
        BASE_SET,
    )
    .unwrap();
    let m = map_sets(&multi).unwrap();
    assert_eq!(m.base_name.as_deref(), Some(BRAND));
    assert_eq!(m.get(DARK_SET), Some(&SetKind::Theme(DARK.into())));
    assert_eq!(m.skipped, [OTHER]);
    // Theme names made of or starting with dots would be hidden files: skipped.
    let dots = parse_penpot_json(
        json!({ BASE_SET: {}, "themes/..": {}, "themes/.": {}, "themes/.env": {}, "themes/a.b": {}, "$themes": [], "$metadata": {} }),
        BASE_SET,
    )
    .unwrap();
    let m = map_sets(&dots).unwrap();
    assert_eq!(m.skipped, ["themes/..", "themes/.", "themes/.env"]);
    assert_eq!(m.get("themes/a.b"), Some(&SetKind::Theme("a.b".into())));
    // Multi-file export (folder of set files + $themes.json/$metadata.json).
    let dir = test_dir("penpot-multi");
    fs::create_dir_all(dir.join("themes")).unwrap();
    fs::write(
        dir.join("base.json"),
        r##"{"color":{"a":{"$value":"#000000","$type":"color"}}}"##,
    )
    .unwrap();
    fs::write(
        dir.join("themes/dark.json"),
        r##"{"color":{"a":{"$value":"#ffffff","$type":"color"}}}"##,
    )
    .unwrap();
    fs::write(
        dir.join("$metadata.json"),
        r#"{"tokenSetOrder":["base","themes/dark"]}"#,
    )
    .unwrap();
    fs::write(dir.join("$themes.json"), "[]").unwrap();
    let r = read_penpot_file(&dir).unwrap();
    let mut sets: Vec<&str> = r.sets.iter().map(|(n, _)| n.as_str()).collect();
    sets.sort();
    assert_eq!(sets, [BASE_SET, DARK_SET]);
}

/* ------------------------------------------------------------ round trips */

/// penpot tokens: dfw -> penpot -> dfw is exact in the exporting project
#[test]
fn round_trip_is_exact_in_the_exporting_project() {
    let root = project(true);
    let before = fs::read_to_string(root.join(rel::TOKENS_JSON)).unwrap();
    let before_dark = fs::read_to_string(root.join(rel::join(rel::THEMES, DARK_THEME))).unwrap();
    let export = export_penpot_tokens(&root, None, false).unwrap();
    assert!(export.file.ends_with(rel::PENPOT_TOKENS));
    let r = import(&root, &export.file, false);
    for (_, d) in &r.diff {
        assert!(d.added.is_empty() && d.changed.is_empty() && d.removed.is_empty());
    }
    assert!(r.written.is_empty());
    assert_eq!(
        fs::read_to_string(root.join(rel::TOKENS_JSON)).unwrap(),
        before
    );
    assert_eq!(
        fs::read_to_string(root.join(rel::join(rel::THEMES, DARK_THEME))).unwrap(),
        before_dark
    );
}

/// penpot tokens: dfw -> penpot -> fresh project keeps every supported token (8-bit sRGB colours)
#[test]
fn fresh_project_keeps_every_supported_token() {
    let src = project(true);
    let export = export_penpot_tokens(&src, None, false).unwrap();
    let dst = project(false);
    import(&dst, &export.file, false);
    let a = flatten_tokens(&fixture("dfw-tokens.json"));
    let b = flatten_tokens(&js::read_json(&dst.join(rel::TOKENS_JSON), None).unwrap());
    let unsupported = ["easing.out", "font.eyebrow.note"];
    let mut got: Vec<&str> = b.iter().map(|(n, _)| n).collect();
    let mut want: Vec<&str> = a
        .iter()
        .map(|(n, _)| n)
        .filter(|n| !unsupported.contains(n))
        .collect();
    got.sort();
    want.sort();
    assert_eq!(got, want);
    for (name, t) in b.iter() {
        let o = a.get(name).unwrap();
        assert_eq!(t.ty, o.ty, "{name}");
        if o.type_name() == Some(dtcg::COLOR) && o.value.is_object() {
            if let Some(hex) = o.value.get(key::HEX) {
                assert_eq!(t.value.get(key::HEX), Some(hex), "{name}");
            }
            continue;
        }
        if o.type_name() == Some(dtcg::SHADOW) {
            continue; // colour inside quantised; geometry checked below
        }
        assert!(same(&t.value, &o.value), "{name}");
    }
    assert_json(
        &b.get("shadow.sm").unwrap().value[0]["offsetY"],
        json!({ "value": 1, "unit": "px" }),
    );
    assert!(dst.join(rel::join(rel::THEMES, DARK_THEME)).exists());
}

/// penpot tokens: import of a Penpot-edited file -> diff, notes, skipped sets
#[test]
fn import_of_a_penpot_edited_file() {
    let root = project(true);
    let edited = fix().join("penpot-edited.tokens.json");
    let r = import(&root, &edited, false);
    let d = diff_of(&r, BASE_SET);
    assert_eq!(
        names(&d.changed),
        [
            "color.blue.600",
            "duration.fast",
            "font.family.body",
            "font.size.md",
            "radius.md",
            "space.4"
        ]
    );
    assert_eq!(
        names(&d.added),
        ["color.brand", "color.mix", "opacity.muted"]
    );
    assert!(d.kept.iter().any(|k| k[TOKEN_FIELD] == VEIL));
    assert_eq!(names(&diff_of(&r, DARK_SET).changed), ["color.ink"]);
    assert_eq!(r.skipped_sets, ["Brands/A"]);
    assert_eq!(names(&r.unsupported), ["flag"]);
    let t = js::read_json(&root.join(rel::TOKENS_JSON), None).unwrap();
    // The group's $type is kept, the value is changed in place.
    assert_eq!(
        to_json_compact(&t["duration"]["fast"]),
        r#"{"$value":{"value":150,"unit":"ms"}}"#
    );
    assert_json(
        &t[COLOR_GROUP]["blue"]["600"][key::VALUE],
        json!({ "colorSpace": color_space::SRGB, "components": [0, 0.3333, 0.8667], "hex": "#0055dd" }),
    );
    assert_json(&t["easing"]["out"][key::VALUE], json!([0.2, 0, 0, 1])); // unsupported tokens stay
    assert_json(&t["font"]["weight"]["bold"][key::VALUE], json!(700)); // "Bold" == 700: unchanged
                                                                       // --prune removes tokens deleted in Penpot.
    let p = import(&project(true), &edited, true);
    assert!(diff_of(&p, BASE_SET)
        .removed
        .iter()
        .any(|x| x[TOKEN_FIELD] == VEIL));
}

/// penpot tokens: exact round trip for every starter preset
#[test]
fn exact_round_trip_for_every_starter_preset() {
    let presets = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/kits/base/presets");
    let mut seen = 0;
    for f in dir_names(&presets).unwrap() {
        if !f.ends_with(".tokens.json") {
            continue;
        }
        seen += 1;
        let root = project(false);
        fs::copy(presets.join(&f), root.join(rel::TOKENS_JSON)).unwrap();
        let before = fs::read_to_string(root.join(rel::TOKENS_JSON)).unwrap();
        let export = export_penpot_tokens(&root, None, false).unwrap();
        let types: Vec<String> = export
            .report
            .unsupported
            .iter()
            .map(|u| js::to_string(u.get("type")))
            .collect();
        assert_eq!(types, ["cubicBezier"], "{f}");
        let r = import(&root, &export.file, false);
        assert!(r.written.is_empty(), "{f}");
        assert_eq!(
            fs::read_to_string(root.join(rel::TOKENS_JSON)).unwrap(),
            before,
            "{f}"
        );
    }
    assert!(seen >= 3);
}

/* ------------------------------------------------------------ untrusted input (penpot-hardening.test.mjs) */

/// penpot tokens: "__proto__" in a token or set name never reaches Object.prototype
#[test]
fn reserved_names_in_a_token_or_set_name_are_refused() {
    let root = test_dir("penpot-proto");
    fs::create_dir_all(root.join(rel::TOKENS)).unwrap();
    fs::write(
        root.join(rel::TOKENS_JSON),
        r##"{"color":{"$type":"color","a":{"$value":"#ffffff"}}}"##,
    )
    .unwrap();
    let file = root.join("in.json");
    fs::write(&file, r##"{"__proto__":{"zzz":{"$value":"#000000","$type":"color"}},"color":{"a":{"$value":"#ffffff","$type":"color"},"constructor":{"prototype":{"yyy":{"$value":"#111111","$type":"color"}}}}}"##).unwrap();
    let r = import(&root, &file, false);
    assert!(diff_of(&r, "in").added.is_empty());
    assert_eq!(names(&r.unsupported), [PROTO, "color.constructor"]);
    assert!(js::to_string(r.unsupported[0].get("reason")).contains("reserved name"));
    // Nothing of the reserved subtrees reached the tokens.
    let text = fs::read_to_string(root.join(rel::TOKENS_JSON)).unwrap();
    assert!(!text.contains("zzz") && !text.contains("yyy") && !text.contains(PROTO));
    // The same names in `nest` (the plugin's flat token list).
    let nested = nest(&[
        NestEntry {
            name: "__proto__.polluted".into(),
            value: Some("#fff".into()),
            ty: Some(penpot_type::COLOR.into()),
            description: None,
        },
        NestEntry {
            name: "color.ok".into(),
            value: Some("#000".into()),
            ty: Some(penpot_type::COLOR.into()),
            description: None,
        },
    ]);
    assert_eq!(
        to_json_compact(&nested),
        r##"{"color":{"ok":{"$value":"#000","$type":"color"}}}"##
    );
}

/// penpot tokens: a file holding only "themes/dark" is a theme, never the base
#[test]
fn a_file_holding_only_a_theme_is_never_the_base() {
    let only = json!({ "themes/dark": { "color": { "a": { "$value": "#000000", "$type": "color" } } }, "$themes": [], "$metadata": {} });
    let m = map_sets(&parse_penpot_json(only.clone(), BASE_SET).unwrap()).unwrap();
    assert_eq!(m.base_name, None);
    assert_eq!(m.get(DARK_SET), Some(&SetKind::Theme(DARK.into())));
    let root = test_dir("penpot-theme");
    fs::create_dir_all(root.join(rel::TOKENS)).unwrap();
    let base = r##"{"color":{"$type":"color","a":{"$value":"#ffffff"}}}"##;
    fs::write(root.join(rel::TOKENS_JSON), base).unwrap();
    fs::write(root.join(DARK_THEME), to_json_compact(&only)).unwrap();
    let err = import_penpot_tokens(&root, &root.join(DARK_THEME), None, false, false).unwrap_err();
    assert!(err.message().contains("No base token set"));
    assert_eq!(
        fs::read_to_string(root.join(rel::TOKENS_JSON)).unwrap(),
        base,
        "tokens.json untouched"
    );
    assert!(!root.join(rel::THEMES).exists());
    // One set with another name in a multi-set file is still the base (as before).
    let global = parse_penpot_json(json!({ "global": {}, "$themes": [] }), BASE_SET).unwrap();
    assert_eq!(
        map_sets(&global).unwrap().base_name.as_deref(),
        Some("global")
    );
}

/// penpot tokens: a description edited in Penpot keeps the exact dfw value (oklch, seconds)
#[test]
fn a_description_edited_in_penpot_keeps_the_exact_dfw_value() {
    const BRAND: &str = "brand";
    const MOTION: &str = "motion";
    const FAST: &str = "fast";
    let cur = json!({
        COLOR_GROUP: { "$type": dtcg::COLOR, BRAND: { "$value": { "colorSpace": color_space::OKLCH, "components": [0.6, 0.15, 250] }, "$description": "Brand" } },
        MOTION: { "$type": dtcg::DURATION, FAST: { "$value": { "value": 0.2, "unit": "s" } } },
    });
    let mut pen = dfw_to_penpot(Some(&cur), &Map::new()).0[BASE_SET].clone();
    pen[COLOR_GROUP][BRAND][key::DESCRIPTION] = "Brand colour".into();
    pen[MOTION][FAST][key::DESCRIPTION] = "Quick".into();
    let m = merge_set(Some(&cur), &pen, BASE_SET, false).unwrap();
    assert_eq!(
        to_json_compact(&m.tree[COLOR_GROUP][BRAND]),
        r#"{"$value":{"colorSpace":"oklch","components":[0.6,0.15,250]},"$description":"Brand colour"}"#
    );
    assert_eq!(
        to_json_compact(&m.tree[MOTION][FAST]),
        r#"{"$value":{"value":0.2,"unit":"s"},"$description":"Quick"}"#
    );
    let changed: Vec<Value> = m
        .diff
        .changed
        .iter()
        .map(|c| json!([c[TOKEN_FIELD], c["descriptionOnly"]]))
        .collect();
    assert_json(
        &json!(changed),
        json!([["color.brand", true], ["motion.fast", true]]),
    );
    let summary = format_import(
        &TokenImport {
            diff: vec![(BASE_SET.into(), m.diff.clone())],
            ..TokenImport::default()
        },
        false,
    );
    assert!(summary.contains("~ color.brand: description changed (value kept)"));
    // Description cleared in Penpot: removed, value still exact.
    pen[COLOR_GROUP][BRAND]
        .as_object_mut()
        .unwrap()
        .shift_remove(key::DESCRIPTION);
    let cleared = merge_set(Some(&cur), &pen, BASE_SET, false).unwrap();
    assert_eq!(
        to_json_compact(&cleared.tree[COLOR_GROUP][BRAND]),
        r#"{"$value":{"colorSpace":"oklch","components":[0.6,0.15,250]}}"#
    );
}

/// Values the Node code read with JavaScript's rules.
#[test]
fn javascript_value_rules() {
    assert_eq!(js::slice16("héllo wörld", 1, 4), "éll");
    assert_eq!(js::norm(Some(&json!("  a \n\t b\u{a0}c "))), "a b c");
    assert_eq!(js::norm(None), "");
    let ordered = js::key_order_pairs(vec![
        ("b".to_string(), 1),
        ("10".to_string(), 2),
        ("2".to_string(), 3),
    ]);
    let keys: Vec<&str> = ordered.iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["2", "10", "b"]);
    assert_eq!(parse_int16("ff"), 255.0);
    assert!(parse_int16("zz").is_nan());
    assert_eq!(
        short_value(Some(&json!({ "hex": BLUE, "alpha": 0.5 }))),
        "#0066ff/0.5"
    );
    assert_eq!(
        short_value(Some(&json!({ "value": 8, "unit": "px" }))),
        "8px"
    );
    assert_eq!(short_value(Some(&json!("x".repeat(70)))).len(), 60);
    let mut names = vec![
        "Zeta", "alpha", "Beta", "_x", "10", "9", "a-b", "a_b", "ab", "aB", "Ab",
    ];
    names.sort_by(|a, b| locale_cmp(a, b));
    assert_eq!(names.join(" "), "_x 10 9 a_b a-b ab aB Ab alpha Beta Zeta");
}

/// Node bugs kept: a group or a new theme named like a function every JavaScript object
/// inherits (`toString`, `valueOf`, ...).
#[test]
fn names_javascript_objects_inherit() {
    let entry = |name: &str| NestEntry {
        name: name.into(),
        value: Some("#000".into()),
        ty: Some(penpot_type::COLOR.into()),
        description: None,
    };
    let nested = nest(&[
        entry("toString.x"),
        entry("a.valueOf.y"),
        entry("a.toString"),
        entry("ok"),
    ]);
    assert_eq!(
        to_json_compact(&nested),
        r##"{"a":{"toString":{"$value":"#000","$type":"color"}},"ok":{"$value":"#000","$type":"color"}}"##
    );
    let root = project(true);
    let file = root.join("in.json");
    let doc = |theme: &str| {
        format!(
            r##"{{"base":{{}},"themes/{theme}":{{"c":{{"$value":"#000","$type":"color"}}}},"$themes":[]}}"##
        )
    };
    fs::write(&file, doc(TO_STRING)).unwrap();
    assert!(import_penpot_tokens(&root, &file, None, false, false).is_err());
    assert!(!root
        .join(rel::join(rel::THEMES, &file::theme(TO_STRING)))
        .exists());
    fs::write(&file, doc(PROTO)).unwrap();
    assert_eq!(
        import(&root, &file, false).written,
        [rel::join(rel::THEMES, &file::theme(PROTO))]
    );
}
