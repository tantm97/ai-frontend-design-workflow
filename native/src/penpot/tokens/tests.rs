//! tests/unit/penpot-tokens.test.mjs, and the token cases of penpot-hardening.test.mjs.
//! The CLI case is in native/tests/penpot.rs (it needs the binary).

use super::*;
use crate::core::js;
use crate::core::util::test_dir;
use crate::design::tokens::tokens_to_declarations;
use std::path::PathBuf;

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
    fs::create_dir_all(root.join(".dfw")).unwrap();
    fs::write(root.join(".dfw/state.json"), "{}").unwrap();
    fs::create_dir_all(root.join("design/tokens/themes")).unwrap();
    if tokens {
        fs::copy(
            fix().join("dfw-tokens.json"),
            root.join("design/tokens/tokens.json"),
        )
        .unwrap();
        fs::copy(
            fix().join("dfw-theme-dark.json"),
            root.join("design/tokens/themes/dark.json"),
        )
        .unwrap();
    }
    root
}

fn import(root: &Path, file: &Path, prune: bool) -> TokenImport {
    import_penpot_tokens(root, file, None, false, prune).unwrap()
}

fn names(list: &[Value]) -> Vec<String> {
    let mut names: Vec<String> = list.iter().map(|c| js::to_string(c.get("token"))).collect();
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
    let t = |value: Value| to("color.x", "color", value);
    assert_json(
        &t(json!({ "colorSpace": "srgb", "components": [0, 0.4, 1], "hex": "#0066FF" })).to_json(),
        json!({ "type": "color", "value": "#0066ff" }),
    );
    assert_eq!(
        value_of(&t(
            json!({ "colorSpace": "srgb", "components": [0, 0, 0], "alpha": 0.5 })
        )),
        "rgba(0, 0, 0, 0.5)"
    );
    assert_eq!(value_of(&t(json!("{color.blue}"))), "{color.blue}");
    let mut notes = Vec::new();
    let oklch = token_to_penpot(
        "c",
        &tok(
            "color",
            json!({ "colorSpace": "oklch", "components": [0.6, 0.1, 250] }),
        ),
        &mut |n| notes.push(n),
    );
    let hex = value_of(&oklch);
    let hex = hex.as_str().unwrap();
    assert!(hex.len() == 7 && hex.starts_with('#') && HEX_COLOR.is_match(hex));
    assert!(notes[0].contains("oklch"));
    assert!(t(json!({ "colorSpace": "lab", "components": [50, 0, 0] }))
        .unsupported()
        .is_some());

    assert_json(
        &parse_color("#0066ff").unwrap(),
        json!({ "colorSpace": "srgb", "components": [0, 0.4, 1], "hex": "#0066ff" }),
    );
    assert_json(
        &parse_color("#fff").unwrap(),
        json!({ "colorSpace": "srgb", "components": [1, 1, 1], "hex": "#ffffff" }),
    );
    assert_eq!(parse_color("#00000080").unwrap()["alpha"], json!(0.502));
    assert_json(
        &parse_color("rgba(255, 102, 0, 0.5)").unwrap(),
        json!({ "colorSpace": "srgb", "components": [1, 0.4, 0], "alpha": 0.5, "hex": "#ff6600" }),
    );
    assert_eq!(parse_color("hsl(210, 50%, 40%)").unwrap()["hex"], "#336699");
    assert_eq!(parse_color("darken({a}, 10)"), None);
}

/// penpot tokens: dimension types by name (radius, spacing, sizing, border width, font size)
#[test]
fn dimension_types_by_name() {
    let px = json!({ "value": 4, "unit": "px" });
    assert_eq!(penpot_dimension_type("radius.md", &px), "borderRadius");
    assert_eq!(penpot_dimension_type("space.1-5", &px), "spacing");
    assert_eq!(penpot_dimension_type("size.control", &px), "sizing");
    assert_eq!(penpot_dimension_type("border.width", &px), "borderWidth");
    assert_eq!(penpot_dimension_type("focus.width", &px), "borderWidth");
    assert_eq!(penpot_dimension_type("font.size.md", &px), "fontSizes");
    assert_eq!(
        penpot_dimension_type("font.size.md", &json!({ "value": 1, "unit": "rem" })),
        "dimension"
    );
    assert_eq!(
        penpot_dimension_type(
            "font.tracking.tight",
            &json!({ "value": 0.5, "unit": "px" })
        ),
        "letterSpacing"
    );
    assert_eq!(penpot_dimension_type("elevation.offset", &px), "dimension");
    assert_json(
        &to("space.4", "dimension", json!({ "value": 16, "unit": "px" })).to_json(),
        json!({ "type": "spacing", "value": "16px" }),
    );
    for pt in [
        "dimension",
        "spacing",
        "sizing",
        "borderRadius",
        "borderWidth",
        "fontSizes",
        "letterSpacing",
    ] {
        assert_json(
            &value_of(&from("x", pt, json!("12"))),
            json!({ "value": 12, "unit": "px" }),
        );
    }
    assert_json(
        &parse_dimension(Some(&json!("1.5rem"))).unwrap(),
        json!({ "value": 1.5, "unit": "rem" }),
    );
    let mut w = Vec::new();
    let expr = token_from_penpot(
        "space.8",
        &tok("spacing", json!("{space.4} * 2")),
        Some(&json!("dimension")),
        &mut |x| w.push(x),
    )
    .unwrap();
    assert_eq!(value_of(&expr), "calc({space.4} * 2)");
    assert!(w[0].contains("math expression"));
    let css = tokens_to_declarations(
        &json!({ "space": { "8": { "$type": "dimension", "$value": value_of(&expr) } } }),
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
        value_of(&to("space.8", "dimension", value_of(&expr))),
        "{space.4} * 2"
    );
}

/// penpot tokens: font family, weight, number, duration, string, unsupported
#[test]
fn font_family_weight_number_duration_string_unsupported() {
    assert_json(
        &to(
            "font.family.body",
            "fontFamily",
            json!(["Inter", "Segoe UI", "sans-serif"]),
        )
        .to_json(),
        json!({ "type": "fontFamilies", "value": "Inter, Segoe UI, sans-serif" }),
    );
    assert_json(
        &value_of(&from(
            "f",
            "fontFamilies",
            json!("\"Source Sans 3\", sans-serif"),
        )),
        json!(["Source Sans 3", "sans-serif"]),
    );
    assert_json(
        &to("w", "fontWeight", json!(600)).to_json(),
        json!({ "type": "fontWeights", "value": "600" }),
    );
    assert_json(
        &value_of(&from("w", "fontWeights", json!("Semi Bold"))),
        json!(600),
    );
    assert_json(
        &value_of(&from("w", "fontWeights", json!("300"))),
        json!(300),
    );
    assert_json(
        &to("n", "number", json!(1.5)).to_json(),
        json!({ "type": "number", "value": "1.5" }),
    );
    assert_json(&value_of(&from("o", "opacity", json!("45%"))), json!(0.45));
    let mut notes = Vec::new();
    let slow = token_to_penpot(
        "duration.slow",
        &tok("duration", json!({ "value": 0.3, "unit": "s" })),
        &mut |n| notes.push(n),
    );
    assert_json(&slow.to_json(), json!({ "type": "number", "value": "300" }));
    assert!(notes[0].contains("no duration type"));
    let duration = json!({ "type": "duration", "value": { "value": 300, "unit": "ms" } });
    assert_json(
        &from("duration.slow", "number", json!("300")).to_json(),
        duration.clone(),
    );
    let preferred = token_from_penpot(
        "motion.x",
        &tok("number", json!("300")),
        Some(&json!("duration")),
        &mut |_| {},
    )
    .unwrap();
    assert_json(&preferred.to_json(), duration);
    assert_json(
        &to("font.eyebrow.case", "string", json!("uppercase")).to_json(),
        json!({ "type": "textCase", "value": "uppercase" }),
    );
    assert_json(
        &from("c", "textCase", json!("uppercase")).to_json(),
        json!({ "type": "string", "value": "uppercase" }),
    );
    assert!(to("s", "string", json!("hello")).unsupported().is_some());
    assert!(to("e", "cubicBezier", json!([0.2, 0, 0, 1]))
        .unsupported()
        .is_some());
    assert!(to("b", "border", json!({})).unsupported().is_some());
    assert!(from("b", "boolean", json!("true")).unsupported().is_some());
    let described = token_to_penpot_quiet(
        "d",
        &Token {
            ty: Some("color".into()),
            value: json!({ "colorSpace": "srgb", "components": [0, 0, 0], "hex": "#000000" }),
            description: Some("Ink".into()),
        },
    );
    assert_eq!(described.description(), Some(&json!("Ink")));
}

/// penpot tokens: shadow and typography composites
#[test]
fn shadow_and_typography_composites() {
    let shadow = json!([{ "color": { "colorSpace": "srgb", "components": [0, 0, 0], "hex": "#000000", "alpha": 0.5 }, "offsetX": { "value": 0, "unit": "px" }, "offsetY": { "value": 2, "unit": "px" }, "blur": { "value": 4, "unit": "px" }, "spread": { "value": 0, "unit": "px" } }]);
    let ps = to("shadow.sm", "shadow", shadow);
    assert_json(
        &ps.to_json(),
        json!({ "type": "shadow", "value": [{ "color": "rgba(0, 0, 0, 0.5)", "offsetX": "0", "offsetY": "2", "blur": "4", "spread": "0", "inset": false }] }),
    );
    let back = value_of(&from("shadow.sm", "shadow", value_of(&ps)));
    assert_json(&back[0]["offsetY"], json!({ "value": 2, "unit": "px" }));
    assert_json(&back[0]["color"]["alpha"], json!(0.5));
    let inner = from(
        "s",
        "shadow",
        json!({ "color": "{color.ink}", "x": "0", "y": "1", "blur": "0", "spread": "0", "type": "innerShadow" }),
    );
    assert_eq!(value_of(&inner)["inset"], true);

    let typo = json!({ "fontFamily": "{font.family.body}", "fontSize": { "value": 16, "unit": "px" }, "fontWeight": 600, "letterSpacing": { "value": 0, "unit": "px" }, "lineHeight": 1.4 });
    let pt = to("type.body", "typography", typo.clone());
    assert_json(
        &value_of(&pt),
        json!({ "fontFamilies": "{font.family.body}", "fontSizes": "16px", "fontWeights": "600", "letterSpacing": "0px", "lineHeights": "1.4" }),
    );
    assert_json(
        &value_of(&from("type.body", "typography", value_of(&pt))),
        typo,
    );
    assert_json(
        &value_of(&from(
            "t",
            "typography",
            json!({ "fontFamily": "Inter", "fontSize": "14", "lineHeight": "150%" }),
        )),
        json!({ "fontFamily": ["Inter"], "fontSize": { "value": 14, "unit": "px" }, "lineHeight": 1.5 }),
    );
}

/* ------------------------------------------------------------ file structure */

/// penpot tokens: export structure (sets, themes, metadata, aliases, unsupported reported)
#[test]
fn export_structure_sets_themes_metadata() {
    let mut themes = Map::new();
    themes.insert("dark".into(), fixture("dfw-theme-dark.json"));
    let (doc, report) = dfw_to_penpot(Some(&fixture("dfw-tokens.json")), &themes);
    let keys: Vec<&String> = doc.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["base", "themes/dark", "$themes", "$metadata"]);
    assert_json(
        &doc["$metadata"],
        json!({ "tokenSetOrder": ["base", "themes/dark"], "activeThemes": ["dfw/default"], "activeSets": ["base"] }),
    );
    let themes: Vec<Value> = doc["$themes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| json!([t["group"], t["name"], t["selectedTokenSets"]]))
        .collect();
    assert_json(
        &json!(themes),
        json!([["dfw", "default", { "base": "enabled" }], ["dfw", "dark", { "base": "enabled", "themes/dark": "enabled" }]]),
    );
    assert_json(
        &doc["base"]["color"]["accent"],
        json!({ "$value": "{color.blue.600}", "$type": "color" }),
    );
    assert_eq!(doc["base"]["color"]["ink"]["$description"], "Body text");
    assert_eq!(doc["themes/dark"]["color"]["ink"]["$value"], "#f0f0f2");
    assert_eq!(
        names(&report.unsupported),
        ["easing.out", "font.eyebrow.note"]
    );
    assert!(report.lossy.iter().any(|l| l["token"] == "duration.fast"));
    // Every exported token has a Penpot type name.
    fn walk(n: &Value, types: &mut Vec<String>) {
        let Value::Object(map) = n else { return };
        if map.contains_key("$value") {
            types.push(js::to_string(map.get("$type")));
        } else {
            map.values().for_each(|v| walk(v, types));
        }
    }
    let mut types = Vec::new();
    walk(&doc["base"], &mut types);
    types.sort();
    types.dedup();
    assert_eq!(
        types,
        [
            "borderRadius",
            "borderWidth",
            "color",
            "dimension",
            "fontFamilies",
            "fontSizes",
            "fontWeights",
            "number",
            "shadow",
            "sizing",
            "spacing",
            "textCase",
            "typography"
        ]
    );
}

/// penpot tokens: single-set files and set mapping
#[test]
fn single_set_files_and_set_mapping() {
    let single = parse_penpot_json(
        json!({ "color": { "a": { "$value": "#000000", "$type": "color" } } }),
        "core",
    )
    .unwrap();
    assert_eq!(
        map_sets(&single).unwrap().base_name.as_deref(),
        Some("core")
    );
    let multi = parse_penpot_json(
        json!({ "brand": {}, "themes/dark": {}, "Light/Colors": {}, "$themes": [], "$metadata": { "tokenSetOrder": ["brand", "themes/dark", "Light/Colors"] } }),
        "base",
    )
    .unwrap();
    let m = map_sets(&multi).unwrap();
    assert_eq!(m.base_name.as_deref(), Some("brand"));
    assert_eq!(m.get("themes/dark"), Some(&SetKind::Theme("dark".into())));
    assert_eq!(m.skipped, ["Light/Colors"]);
    // Theme names made of or starting with dots would be hidden files: skipped.
    let dots = parse_penpot_json(
        json!({ "base": {}, "themes/..": {}, "themes/.": {}, "themes/.env": {}, "themes/a.b": {}, "$themes": [], "$metadata": {} }),
        "base",
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
    assert_eq!(sets, ["base", "themes/dark"]);
}

/* ------------------------------------------------------------ round trips */

/// penpot tokens: dfw -> penpot -> dfw is exact in the exporting project
#[test]
fn round_trip_is_exact_in_the_exporting_project() {
    let root = project(true);
    let before = fs::read_to_string(root.join("design/tokens/tokens.json")).unwrap();
    let before_dark = fs::read_to_string(root.join("design/tokens/themes/dark.json")).unwrap();
    let export = export_penpot_tokens(&root, None, false).unwrap();
    assert!(export.file.ends_with(".dfw/penpot/tokens.penpot.json"));
    let r = import(&root, &export.file, false);
    for (_, d) in &r.diff {
        assert!(d.added.is_empty() && d.changed.is_empty() && d.removed.is_empty());
    }
    assert!(r.written.is_empty());
    assert_eq!(
        fs::read_to_string(root.join("design/tokens/tokens.json")).unwrap(),
        before
    );
    assert_eq!(
        fs::read_to_string(root.join("design/tokens/themes/dark.json")).unwrap(),
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
    let b = flatten_tokens(&js::read_json(&dst.join("design/tokens/tokens.json"), None).unwrap());
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
        if o.type_name() == Some("color") && o.value.is_object() {
            if let Some(hex) = o.value.get("hex") {
                assert_eq!(t.value.get("hex"), Some(hex), "{name}");
            }
            continue;
        }
        if o.type_name() == Some("shadow") {
            continue; // colour inside quantised; geometry checked below
        }
        assert!(same(&t.value, &o.value), "{name}");
    }
    assert_json(
        &b.get("shadow.sm").unwrap().value[0]["offsetY"],
        json!({ "value": 1, "unit": "px" }),
    );
    assert!(dst.join("design/tokens/themes/dark.json").exists());
}

/// penpot tokens: import of a Penpot-edited file -> diff, notes, skipped sets
#[test]
fn import_of_a_penpot_edited_file() {
    let root = project(true);
    let edited = fix().join("penpot-edited.tokens.json");
    let r = import(&root, &edited, false);
    let d = diff_of(&r, "base");
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
    assert!(d.kept.iter().any(|k| k["token"] == "color.veil"));
    assert_eq!(names(&diff_of(&r, "themes/dark").changed), ["color.ink"]);
    assert_eq!(r.skipped_sets, ["Brands/A"]);
    assert_eq!(names(&r.unsupported), ["flag"]);
    let t = js::read_json(&root.join("design/tokens/tokens.json"), None).unwrap();
    // The group's $type is kept, the value is changed in place.
    assert_eq!(
        to_json_compact(&t["duration"]["fast"]),
        r#"{"$value":{"value":150,"unit":"ms"}}"#
    );
    assert_json(
        &t["color"]["blue"]["600"]["$value"],
        json!({ "colorSpace": "srgb", "components": [0, 0.3333, 0.8667], "hex": "#0055dd" }),
    );
    assert_json(&t["easing"]["out"]["$value"], json!([0.2, 0, 0, 1])); // unsupported tokens stay
    assert_json(&t["font"]["weight"]["bold"]["$value"], json!(700)); // "Bold" == 700: unchanged
                                                                     // --prune removes tokens deleted in Penpot.
    let p = import(&project(true), &edited, true);
    assert!(diff_of(&p, "base")
        .removed
        .iter()
        .any(|x| x["token"] == "color.veil"));
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
        fs::copy(presets.join(&f), root.join("design/tokens/tokens.json")).unwrap();
        let before = fs::read_to_string(root.join("design/tokens/tokens.json")).unwrap();
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
            fs::read_to_string(root.join("design/tokens/tokens.json")).unwrap(),
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
    fs::create_dir_all(root.join("design/tokens")).unwrap();
    fs::write(
        root.join("design/tokens/tokens.json"),
        r##"{"color":{"$type":"color","a":{"$value":"#ffffff"}}}"##,
    )
    .unwrap();
    let file = root.join("in.json");
    fs::write(&file, r##"{"__proto__":{"zzz":{"$value":"#000000","$type":"color"}},"color":{"a":{"$value":"#ffffff","$type":"color"},"constructor":{"prototype":{"yyy":{"$value":"#111111","$type":"color"}}}}}"##).unwrap();
    let r = import(&root, &file, false);
    assert!(diff_of(&r, "in").added.is_empty());
    assert_eq!(names(&r.unsupported), ["__proto__", "color.constructor"]);
    assert!(js::to_string(r.unsupported[0].get("reason")).contains("reserved name"));
    // Nothing of the reserved subtrees reached the tokens.
    let text = fs::read_to_string(root.join("design/tokens/tokens.json")).unwrap();
    assert!(!text.contains("zzz") && !text.contains("yyy") && !text.contains("__proto__"));
    // The same names in `nest` (the plugin's flat token list).
    let nested = nest(&[
        NestEntry {
            name: "__proto__.polluted".into(),
            value: Some("#fff".into()),
            ty: Some("color".into()),
            description: None,
        },
        NestEntry {
            name: "color.ok".into(),
            value: Some("#000".into()),
            ty: Some("color".into()),
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
    let m = map_sets(&parse_penpot_json(only.clone(), "base").unwrap()).unwrap();
    assert_eq!(m.base_name, None);
    assert_eq!(m.get("themes/dark"), Some(&SetKind::Theme("dark".into())));
    let root = test_dir("penpot-theme");
    fs::create_dir_all(root.join("design/tokens")).unwrap();
    let base = r##"{"color":{"$type":"color","a":{"$value":"#ffffff"}}}"##;
    fs::write(root.join("design/tokens/tokens.json"), base).unwrap();
    fs::write(root.join("dark.json"), to_json_compact(&only)).unwrap();
    let err = import_penpot_tokens(&root, &root.join("dark.json"), None, false, false).unwrap_err();
    assert!(err.message().contains("No base token set"));
    assert_eq!(
        fs::read_to_string(root.join("design/tokens/tokens.json")).unwrap(),
        base,
        "tokens.json untouched"
    );
    assert!(!root.join("design/tokens/themes").exists());
    // One set with another name in a multi-set file is still the base (as before).
    let global = parse_penpot_json(json!({ "global": {}, "$themes": [] }), "base").unwrap();
    assert_eq!(
        map_sets(&global).unwrap().base_name.as_deref(),
        Some("global")
    );
}

/// penpot tokens: a description edited in Penpot keeps the exact dfw value (oklch, seconds)
#[test]
fn a_description_edited_in_penpot_keeps_the_exact_dfw_value() {
    let cur = json!({
        "color": { "$type": "color", "brand": { "$value": { "colorSpace": "oklch", "components": [0.6, 0.15, 250] }, "$description": "Brand" } },
        "motion": { "$type": "duration", "fast": { "$value": { "value": 0.2, "unit": "s" } } },
    });
    let mut pen = dfw_to_penpot(Some(&cur), &Map::new()).0["base"].clone();
    pen["color"]["brand"]["$description"] = "Brand colour".into();
    pen["motion"]["fast"]["$description"] = "Quick".into();
    let m = merge_set(Some(&cur), &pen, "base", false).unwrap();
    assert_eq!(
        to_json_compact(&m.tree["color"]["brand"]),
        r#"{"$value":{"colorSpace":"oklch","components":[0.6,0.15,250]},"$description":"Brand colour"}"#
    );
    assert_eq!(
        to_json_compact(&m.tree["motion"]["fast"]),
        r#"{"$value":{"value":0.2,"unit":"s"},"$description":"Quick"}"#
    );
    let changed: Vec<Value> = m
        .diff
        .changed
        .iter()
        .map(|c| json!([c["token"], c["descriptionOnly"]]))
        .collect();
    assert_json(
        &json!(changed),
        json!([["color.brand", true], ["motion.fast", true]]),
    );
    let summary = format_import(
        &TokenImport {
            diff: vec![("base".into(), m.diff.clone())],
            ..TokenImport::default()
        },
        false,
    );
    assert!(summary.contains("~ color.brand: description changed (value kept)"));
    // Description cleared in Penpot: removed, value still exact.
    pen["color"]["brand"]
        .as_object_mut()
        .unwrap()
        .shift_remove("$description");
    let cleared = merge_set(Some(&cur), &pen, "base", false).unwrap();
    assert_eq!(
        to_json_compact(&cleared.tree["color"]["brand"]),
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
        short_value(Some(&json!({ "hex": "#0066ff", "alpha": 0.5 }))),
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
        ty: Some("color".into()),
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
    fs::write(&file, doc("toString")).unwrap();
    assert!(import_penpot_tokens(&root, &file, None, false, false).is_err());
    assert!(!root.join("design/tokens/themes/toString.json").exists());
    fs::write(&file, doc("__proto__")).unwrap();
    assert_eq!(
        import(&root, &file, false).written,
        ["design/tokens/themes/__proto__.json"]
    );
}
