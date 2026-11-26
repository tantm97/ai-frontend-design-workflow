//! `typeScale` and `auditTypeScale`: a modular type scale and its rhythm checks.

use crate::core::js;
use crate::core::level::{finding, Level};
use serde_json::{json, Value};

/// A modular type scale (`typeScale`): sizes in px by step name, in the order Node listed them.
#[derive(Debug, Clone, PartialEq)]
pub struct TypeScale {
    pub ui: Vec<(&'static str, f64)>,
    pub display: Vec<(&'static str, f64)>,
    pub ratio: f64,
    pub display_ratio: f64,
    pub base: f64,
}

/// `typeScale`: UI steps use `ratio` around the base; display steps use `display_ratio` from
/// `display_from` (when it is given and not zero).
pub fn type_scale(
    base: f64,
    ratio: f64,
    display_ratio: f64,
    display_from: Option<f64>,
) -> TypeScale {
    let r = js::round;
    let two_xs = js::max(11.0, r(base / ratio.powf(2.5)));
    let mut xs = js::max(12.0, r(base / ratio.powf(2.0)));
    let mut sm = r(base / ratio);
    if xs <= two_xs {
        xs = two_xs + 1.0;
    }
    if sm <= xs {
        sm = xs + 1.0;
    }
    let two_xl = r(base * ratio.powf(3.0));
    let from = display_from
        .filter(|f| *f != 0.0 && !f.is_nan())
        .unwrap_or_else(|| r(base * ratio.powf(4.0)));
    let d0 = js::max(from, two_xl + 2.0);
    let display = vec![
        ("display-sm", d0),
        ("display-md", r(d0 * display_ratio)),
        ("display-lg", r(d0 * display_ratio.powf(2.0))),
        ("display-xl", r(d0 * display_ratio.powf(3.0))),
    ];
    let ui = vec![
        ("2xs", two_xs),
        ("xs", xs),
        ("sm", sm),
        ("md", base),
        ("lg", r(base * ratio)),
        ("xl", r(base * ratio.powf(2.0))),
        ("2xl", two_xl),
        ("3xl", display[0].1),
        ("4xl", display[1].1),
    ];
    TypeScale {
        ui,
        display,
        ratio,
        display_ratio,
        base,
    }
}

/// `[...new Set(list)].sort((a, b) => a - b)`.
fn unique_sorted(list: Option<&Value>) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    for v in list.and_then(Value::as_array).into_iter().flatten() {
        let n = js::to_number(Some(v));
        if !out.iter().any(|o| *o == n || (o.is_nan() && n.is_nan())) {
            out.push(n);
        }
    }
    out.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// The surface of an app's screens, the default when the manifest names none (the other is
/// `marketing`).
pub const PRODUCT_SURFACE: &str = "product";

/// The keys of the scale [`audit_type_scale`] reads (when it is not a flat list).
pub mod key {
    /// The UI sizes, in px.
    pub const UI: &str = "ui";
    /// The display sizes, in px.
    pub const DISPLAY: &str = "display";
}

/// `auditTypeScale`: check a scale for rhythm and hierarchy. `scale` is `{ ui: [px], display:
/// [px] }` or a flat list of UI sizes; returns `[{ level, rule, message }]`.
pub fn audit_type_scale(scale: &Value, body: f64, surface: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let ui = unique_sorted(if scale.is_array() {
        Some(scale)
    } else {
        scale.get(key::UI)
    });
    let display = unique_sorted(if scale.is_array() {
        None
    } else {
        scale.get(key::DISPLAY)
    });
    let list = |values: &[f64], sep: &str| {
        values
            .iter()
            .map(|v| js::number(*v))
            .collect::<Vec<_>>()
            .join(sep)
    };
    if display.len() >= 3 {
        let rr: Vec<f64> = display.windows(2).map(|w| w[1] / w[0]).collect();
        let mean = rr.iter().fold(0.0, |a, b| a + b) / rr.len() as f64;
        let dev = rr
            .iter()
            .map(|x| (x - mean).abs())
            .fold(f64::NEG_INFINITY, js::max);
        if dev > 0.08 {
            let ratios: Vec<String> = rr.iter().map(|x| js::to_fixed(*x, 2)).collect();
            out.push(json!({ (finding::LEVEL): Level::Warn.as_str(), (finding::RULE): "type-ratio", (finding::MESSAGE): format!("Display steps {} do not follow one ratio ({}). Use a modular scale (dfw type scale).", list(&display, "/"), ratios.join(", ")) }));
        }
    }
    let top = if display.is_empty() {
        ui.last()
    } else {
        display.last()
    }
    .copied();
    let need = if surface == "marketing" { 3.0 } else { 1.8 };
    if let Some(top) = top {
        if top / body < need {
            out.push(json!({ (finding::LEVEL): Level::Warn.as_str(), (finding::RULE): "type-contrast", (finding::MESSAGE): format!("Largest size {}px is only {}× body; {surface} UIs need ≥ {}× for clear hierarchy.", js::number(top), js::to_fixed(top / body, 1), js::number(need)) }));
        }
    }
    let mut all = ui.clone();
    for d in &display {
        if !all.contains(d) {
            all.push(*d);
        }
    }
    all.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    // Below ~20px the eye cannot separate sizes less than ~8% apart; above, 6%.
    let close = all
        .windows(2)
        .filter(|w| w[1] / w[0] < if w[0] < 20.0 { 1.08 } else { 1.06 })
        .count();
    if close >= 2 {
        out.push(json!({ (finding::LEVEL): Level::Warn.as_str(), (finding::RULE): "type-steps", (finding::MESSAGE): format!("{close} adjacent sizes are too close (e.g. 14/15): they look like mistakes, not hierarchy. Merge them or use a modular scale.") }));
    }
    out
}
