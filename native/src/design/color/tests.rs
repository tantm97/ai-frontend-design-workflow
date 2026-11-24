use super::audit::ACCENT_VS_STATUS;
use super::*;
use crate::core::level::Level;
use serde_json::json;

/// The keys of a palette: its role colours and its status colours.
const ROLES: &str = "roles";
const STATUS: &str = "status";

/// `resolve` of tests/unit/color.test.mjs: role references to hex.
fn resolve(pal: &Value) -> Value {
    let mut out = Map::new();
    for (k, v) in pal[ROLES].as_object().unwrap() {
        let v = v.as_str().unwrap();
        let hex = if v.starts_with('#') {
            v.to_string()
        } else {
            let cut = v.rfind('-').unwrap();
            pal["ramps"][&v[..cut]][&v[cut + 1..]]
                .as_str()
                .unwrap()
                .to_string()
        };
        out.insert(k.clone(), json!(hex));
    }
    Value::Object(out)
}

fn seeds(v: Value) -> Seeds {
    Seeds::from_value(&v)
}

fn has(findings: &[Value], level: Option<&str>, rule: &str) -> bool {
    findings
        .iter()
        .any(|f| f["rule"] == rule && level.is_none_or(|l| f["level"] == l))
}

/// oklch round trip and contrast
#[test]
fn oklch_round_trip_and_contrast() {
    for hex in ["#0b5cad", "#f5c400", "#9a2f22", "#fbf7ef"] {
        let back = oklch_to_hex(hex_to_oklch(hex));
        assert!(contrast(&back, hex) < 1.02, "{hex} -> {back}");
    }
    assert_eq!(js::round(contrast("#000000", "#ffffff")), 21.0);
}

/// palette: roles picked by contrast, info merges into a blue accent, audit clean
#[test]
fn palette_roles_by_contrast_and_clean_audit() {
    let pal = build_palette(
        &seeds(
            json!({ "brand": { "hue": 250, "chroma": 0.12 }, "neutral": { "hue": 240, "tint": 0.008 }, "signature": { "hue": 95, "chroma": 0.17 } }),
        ),
        &Names {
            brand: Some("xanh".into()),
            neutral: Some("thep".into()),
            signature: Some("vang".into()),
        },
    );
    let roles = resolve(&pal);
    let c = |a: &str, b: &str| contrast(roles[a].as_str().unwrap(), roles[b].as_str().unwrap());
    /// The background text and borders are read on.
    const SURFACE: &str = "surface";
    assert!(c("ink", SURFACE) >= 13.0);
    assert!(c("border", SURFACE) >= 3.0);
    assert!(c("on-accent", "accent") >= 4.5);
    assert!(pal["notes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|n| n.as_str().unwrap().contains("info uses the accent")));
    let findings = audit_colors(&json!({ "roles": roles, "status": pal[STATUS] }), &[]);
    let error = Level::Error.as_str();
    assert!(
        !has(&findings, Some(error), "contrast") && !findings.iter().any(|f| f["level"] == error),
        "{findings:?}"
    );
}

/// audit: a red accent next to error red is a MUST; ink accent fixes it
#[test]
fn red_accent_next_to_error_red() {
    let red = build_palette(
        &seeds(
            json!({ "brand": { "hue": 28, "chroma": 0.16 }, "neutral": { "hue": 80, "tint": 0.012 } }),
        ),
        &Names::default(),
    );
    let f = audit_colors(
        &json!({ "roles": resolve(&red), "status": red[STATUS] }),
        &[],
    );
    assert!(has(&f, Some(Level::Error.as_str()), ACCENT_VS_STATUS));
    let ink = build_palette(
        &seeds(
            json!({ "brand": { "hue": 28, "chroma": 0.16 }, "neutral": { "hue": 80, "tint": 0.012 }, "accentFrom": "neutral" }),
        ),
        &Names::default(),
    );
    let g = audit_colors(
        &json!({ "roles": resolve(&ink), "status": ink[STATUS] }),
        &[],
    );
    assert!(!has(&g, None, ACCENT_VS_STATUS));
    // status red stays red (not bent to orange to dodge the accent)
    assert!(hue_distance(hex_to_oklch(ink[STATUS]["err"].as_str().unwrap()).h, 27.0) <= 8.0);
    // A recorded exception turns the MUST into a note.
    let excepted = audit_colors(
        &json!({ "roles": resolve(&red), "status": red[STATUS] }),
        &[ACCENT_VS_STATUS.to_string()],
    );
    let finding = excepted
        .iter()
        .find(|x| x["rule"] == ACCENT_VS_STATUS)
        .unwrap();
    assert_eq!(finding["level"], "info");
    assert!(finding["message"]
        .as_str()
        .unwrap()
        .ends_with("(recorded exception)"));
}

/// audit: mixed neutral temperatures and hand-picked type sizes are flagged
#[test]
fn mixed_neutrals_and_hand_picked_type_sizes() {
    /// A blue accent (and focus ring) on white (the surface and the text on the accent).
    const BLUE: &str = "#1d4ed8";
    const WHITE: &str = "#ffffff";
    let roles = json!({ "canvas": "#f3f5f8", "surface": WHITE, "surface-sunken": "#eef1f5", "rule": "#dde2e8", "border": "#7c8590", "muted": "#5a5048", "ink": "#2a2118", "accent": BLUE, "on-accent": WHITE, "accent-soft": "#e8effd", "accent-text": "#1e40af", "focus": BLUE });
    assert!(has(
        &audit_colors(&json!({ "roles": roles, "status": {} }), &[]),
        None,
        "neutral-temperature"
    ));
    let t = audit_type_scale(
        &json!({ "ui": [12, 13, 14, 15, 16], "display": [22, 26, 34, 40] }),
        16.0,
        PRODUCT_SURFACE,
    );
    assert!(has(&t, None, "type-ratio"));
    assert!(has(&t, None, "type-steps"));
    let ts = type_scale(16.0, 1.2, 1.414, Some(30.0));
    let display: Vec<f64> = ts.display.iter().map(|d| d.1).collect();
    assert_eq!(display, [30.0, 42.0, 60.0, 85.0]);
    let ui: Vec<f64> = ts.ui.iter().take(7).map(|u| u.1).collect();
    assert_eq!(
        audit_type_scale(
            &json!({ "ui": ui, "display": display }),
            16.0,
            PRODUCT_SURFACE
        ),
        Vec::<Value>::new()
    );
}

/// 5: 3-digit hex tokens are audited (the contrast assertion of tests/unit/audit-fixes.test.mjs)
#[test]
fn three_digit_hex_is_read_like_six() {
    assert_eq!(contrast("#ccc", "#fff"), contrast("#cccccc", "#ffffff"));
    assert_eq!(hex_to_rgb("#abcf"), hex_to_rgb("#aabbcc"));
    assert_eq!(rgb_to_hex([1.2, -1.0, 0.5]), "#ff0080");
    assert_eq!(rgb_to_hex(hex_to_rgb("#zz0000")), "#NaN0000");
    assert_eq!(
        oklch_to_hex(Oklch {
            l: 0.7,
            c: 0.4,
            h: 30.0
        })
        .len(),
        7
    );
}

#[test]
fn dark_palette_and_default_names() {
    let pal = build_palette(
        &seeds(json!({ "brand": { "hex": "#0b5cad" }, "dark": true })),
        &Names::default(),
    );
    /// The default name of the brand ramp.
    const BRAND: &str = "brand";
    assert_eq!(pal[ROLES]["canvas"], "neutral-950");
    assert_eq!(pal[ROLES]["ink"], "neutral-50");
    assert_eq!(
        pal["names"],
        json!({ "brand": BRAND, "neutral": "neutral", "signature": "signature" })
    );
    assert_eq!(pal["seeds"]["sigHue"], Value::Null);
    let keys: Vec<&String> = pal["ramps"][BRAND].as_object().unwrap().keys().collect();
    assert_eq!(keys.len(), 11);
    assert_eq!(keys[0], "50");
    assert_eq!(pal[STATUS].as_object().unwrap().len(), 12);
}
