//! `auditColors`: contrast, hue and chroma checks on resolved role colours.

use super::{contrast, hex_to_oklch, hue_distance, Oklch};
use crate::core::js;
use crate::core::level::Level;
use serde_json::{json, Map, Value};

/// The rule of an accent too close in hue to a status colour.
pub(crate) const ACCENT_VS_STATUS: &str = "accent-vs-status";

const ROLE_PAIRS: [(&str, &str, f64, bool, &str); 13] = [
    ("ink", "surface", 7.0, true, "Body text"),
    ("ink", "canvas", 7.0, true, "Body text on canvas"),
    ("muted", "surface", 4.5, true, "Secondary text"),
    ("muted", "canvas", 4.5, true, "Secondary text on canvas"),
    (
        "muted",
        "surface-sunken",
        4.5,
        true,
        "Secondary text in table headers",
    ),
    ("subtle", "surface", 4.5, false, "Placeholder text"),
    (
        "border",
        "surface",
        3.0,
        true,
        "Control boundary (WCAG 1.4.11)",
    ),
    ("accent", "surface", 4.5, true, "Links and accent text"),
    ("on-accent", "accent", 4.5, true, "Primary button label"),
    (
        "on-accent",
        "accent-hover",
        4.5,
        true,
        "Primary button label (hover)",
    ),
    (
        "accent-text",
        "accent-soft",
        4.5,
        true,
        "Selected item text",
    ),
    ("focus", "surface", 3.0, true, "Focus indicator"),
    ("on-brand", "brand", 4.5, true, "Text on brand surfaces"),
];

/// `auditColors`: review resolved role colours (hex). `input` is `{ roles, status, onFill }`;
/// returns `[{ level, rule, message }]`. The checks run in the order their findings are
/// printed (the golden tests compare that order).
pub fn audit_colors(input: &Value, exceptions: &[String]) -> Vec<Value> {
    let empty = Map::new();
    let colors = RoleColors {
        roles: input
            .get("roles")
            .and_then(Value::as_object)
            .unwrap_or(&empty),
        status: input
            .get("status")
            .and_then(Value::as_object)
            .unwrap_or(&empty),
        on_fill: input
            .get("onFill")
            .and_then(Value::as_str)
            .unwrap_or("#ffffff"),
    };
    let mut findings = Findings {
        out: Vec::new(),
        exceptions,
    };
    check_role_contrast(&colors, &mut findings);
    check_status_contrast(&colors, &mut findings);
    check_accent_vs_status(&colors, &mut findings);
    check_neutral_temperature(&colors, &mut findings);
    check_brand_vs_error(&colors, &mut findings);
    check_canvas_chroma(&colors, &mut findings);
    check_hue_budget(&colors, &mut findings);
    findings.out
}

/// The resolved colours an audit reads.
struct RoleColors<'a> {
    roles: &'a Map<String, Value>,
    status: &'a Map<String, Value>,
    on_fill: &'a str,
}

impl RoleColors<'_> {
    /// A role colour; `None` when it is missing or falsy.
    fn role(&self, key: &str) -> Option<String> {
        truthy_text(self.roles, key)
    }

    /// A status colour; `None` when it is missing or falsy.
    fn stat(&self, key: &str) -> Option<String> {
        truthy_text(self.status, key)
    }
}

fn truthy_text(map: &Map<String, Value>, key: &str) -> Option<String> {
    map.get(key)
        .filter(|v| js::truthy(Some(v)))
        .map(|v| js::to_string(Some(v)))
}

/// The findings of an audit; a rule the user recorded an exception for is lowered to info.
struct Findings<'a> {
    out: Vec<Value>,
    exceptions: &'a [String],
}

impl Findings<'_> {
    fn add(&mut self, level: Level, rule: &str, message: String) {
        let excepted = self.exceptions.iter().any(|e| e == rule);
        self.out.push(json!({
            "level": if excepted && level == Level::Error { Level::Info } else { level }.as_str(),
            "rule": rule,
            "message": if excepted { format!("{message} (recorded exception)") } else { message },
        }));
    }
}

/// Text and controls on their backgrounds ([`ROLE_PAIRS`]).
fn check_role_contrast(colors: &RoleColors, findings: &mut Findings) {
    for (fg, bg, min, must, what) in ROLE_PAIRS {
        let (Some(f), Some(b)) = (colors.role(fg), colors.role(bg)) else {
            continue;
        };
        let r = contrast(&f, &b);
        if r < min {
            findings.add(
                if must { Level::Error } else { Level::Warn },
                "contrast",
                format!(
                    "{what}: {fg} on {bg} is {}:1, needs {}:1.",
                    js::to_fixed(r, 2),
                    js::number(min)
                ),
            );
        }
    }
}

/// Text on the ok and err fills, and each status text on its tint.
fn check_status_contrast(colors: &RoleColors, findings: &mut Findings) {
    for s in ["ok", "err"] {
        if let Some(fill) = colors.stat(s) {
            let r = contrast(colors.on_fill, &fill);
            if r < 4.5 {
                findings.add(
                    Level::Error,
                    "contrast",
                    format!("Text on status.{s} fill is {}:1.", js::to_fixed(r, 2)),
                );
            }
        }
    }
    for s in ["ok", "info", "warn", "err"] {
        if let Some(text) = colors.stat(&format!("{s}-text")) {
            let tint = js::to_string(colors.status.get(&format!("{s}-tint")));
            let r = contrast(&text, &tint);
            if r < 4.5 {
                findings.add(
                    Level::Error,
                    "contrast",
                    format!("status.{s}-text on its tint is {}:1.", js::to_fixed(r, 2)),
                );
            }
        }
    }
}

/// A chromatic accent close in hue to the error or success colour reads as that signal.
fn check_accent_vs_status(colors: &RoleColors, findings: &mut Findings) {
    let accent = colors.role("accent").unwrap_or_default();
    let acc = hex_to_oklch(&accent);
    for s in ["err", "ok"] {
        let Some(fill) = colors.stat(s) else {
            continue;
        };
        let d = hue_distance(acc.h, hex_to_oklch(&fill).h);
        if acc.c > 0.05 && d < 25.0 {
            let reads_as = if s == "err" {
                "an error/destructive"
            } else {
                "a success"
            };
            findings.add(Level::Error, ACCENT_VS_STATUS, format!("Accent ({accent}) and status.{s} ({fill}) are only {}° apart: users will read the primary action as {reads_as} signal. Move the accent, or make the brand hue a surface/ornament colour and use a different accent.", js::to_fixed(d, 0)));
        } else if acc.c > 0.05 && d < 40.0 {
            findings.add(
                Level::Warn,
                ACCENT_VS_STATUS,
                format!(
                    "Accent and status.{s} are {}° apart; keep icons + words on every {s} message.",
                    js::to_fixed(d, 0)
                ),
            );
        }
    }
}

/// Neutrals: one temperature.
fn check_neutral_temperature(colors: &RoleColors, findings: &mut Findings) {
    let tinted: Vec<(&str, Oklch)> = ["canvas", "surface-sunken", "rule", "border", "muted", "ink"]
        .into_iter()
        .filter_map(|r| colors.role(r).map(|hex| (r, hex_to_oklch(&hex))))
        .filter(|(_, o)| o.c > 0.006)
        .collect();
    if let Some((first, rest)) = tinted.split_first() {
        if let Some(other) = rest
            .iter()
            .find(|(_, o)| hue_distance(first.1.h, o.h) > 40.0)
        {
            findings.add(Level::Warn, "neutral-temperature", format!("Neutrals mix temperatures: {} hue {}° vs {} {}°. Tint all neutrals toward one hue.", first.0, js::to_fixed(first.1.h, 0), other.0, js::to_fixed(other.1.h, 0)));
        }
    }
}

/// A chromatic brand on the error hue.
fn check_brand_vs_error(colors: &RoleColors, findings: &mut Findings) {
    if let (Some(brand), Some(err)) = (colors.role("brand"), colors.stat("err")) {
        let br = hex_to_oklch(&brand);
        if br.c > 0.05 && hue_distance(br.h, hex_to_oklch(&err).h) < 20.0 {
            findings.add(Level::Info, "brand-vs-error", "The brand hue shares the error hue: never place error messages on brand surfaces, and keep errors iconified with words.".to_string());
        }
    }
}

/// A canvas too saturated for a large surface.
fn check_canvas_chroma(colors: &RoleColors, findings: &mut Findings) {
    let canvas = hex_to_oklch(&colors.role("canvas").unwrap_or_default());
    if canvas.c > 0.035 {
        findings.add(Level::Warn, "canvas-chroma", format!("Canvas chroma {} is high for a large surface (tires the eye, shifts every colour on it). Keep ≤ 0.03.", js::to_fixed(canvas.c, 3)));
    }
}

/// Hue budget: more than two distinct chromatic hues in the roles (the signature aside).
fn check_hue_budget(colors: &RoleColors, findings: &mut Findings) {
    let mut hues: Vec<f64> = Vec::new();
    for (key, value) in js::entries(colors.roles) {
        if key == "signature" {
            continue;
        }
        let c = hex_to_oklch(&js::to_string(Some(value)));
        if c.c > 0.05 && !hues.iter().any(|h| hue_distance(*h, c.h) < 25.0) {
            hues.push(c.h);
        }
    }
    if hues.len() > 2 {
        findings.add(Level::Warn, "hue-budget", format!("{} distinct chromatic hues in UI roles (besides status). One accent hue (+ one brand/signature) reads as designed; more reads as decoration.", hues.len()));
    }
}
