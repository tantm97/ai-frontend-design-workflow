//! Ports `core/lib/color.mjs`: OKLCH ramps, contrast, palette building and the type scale.
//!
//! Results that reach a user are hex colours, steps and numbers printed with `toFixed`, so the
//! arithmetic follows the Node code operation by operation (`x ** 3` is `powf(3.0)`, not
//! `x * x * x`; `Math.hypot` is V8's scaled sum) to round the same way.
//!
//! One file per job: conversions and ramps here, the palette (`palette`), the colour audit
//! (`audit`) and the type scale (`type_scale`) next to it.

use crate::core::js;
use serde_json::{Map, Value};

mod audit;
mod palette;
#[cfg(test)]
mod tests;
mod type_scale;

pub use audit::{audit_colors, key as audit_key};
pub use palette::{build_palette, Names, Seeds, Signature};
pub use type_scale::{audit_type_scale, key as type_scale_key, type_scale, PRODUCT_SURFACE};

// ------------------------------------------------------------------ conversions

fn to_lin(v: f64) -> f64 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn to_gam(v: f64) -> f64 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `parseInt(text, 16)`: the longest hexadecimal prefix, `NaN` when there is none.
fn parse_int_hex(text: &str) -> f64 {
    let t = text.trim_start_matches(crate::core::js::is_space);
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let digits: Vec<u32> = t.chars().map_while(|c| c.to_digit(16)).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.iter().fold(0.0, |n, d| n * 16.0 + f64::from(*d))
}

/// `hexToRgb`: `#rrggbb` (or the `#rgb` shorthand) as three channels from 0 to 1.
pub fn hex_to_rgb(hex: &str) -> [f64; 3] {
    let mut h: Vec<char> = hex.replacen('#', "", 1).chars().collect();
    if h.len() < 6 {
        h = h.iter().flat_map(|c| [*c, *c]).collect();
    }
    [0, 2, 4].map(|i| {
        let pair: String = h.iter().skip(i).take(2).collect();
        parse_int_hex(&pair) / 255.0
    })
}

/// `rgbToHex`: channels are clamped to 0..1.
pub fn rgb_to_hex(rgb: [f64; 3]) -> String {
    let mut out = String::from("#");
    for v in rgb {
        let n = js::round(js::min(1.0, js::max(0.0, v)) * 255.0);
        if n.is_nan() {
            out.push_str("NaN");
        } else {
            out.push_str(&format!("{:02x}", n as u8));
        }
    }
    out
}

/// A colour in OKLCH: lightness 0..1, chroma, hue in degrees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Oklch {
    pub l: f64,
    pub c: f64,
    pub h: f64,
}

/// `Math.hypot(a, b)` as V8 computes it: scaled by the larger value, summed with compensation.
fn hypot(a: f64, b: f64) -> f64 {
    let (a, b) = (a.abs(), b.abs());
    if a.is_infinite() || b.is_infinite() {
        return f64::INFINITY;
    }
    if a.is_nan() || b.is_nan() {
        return f64::NAN;
    }
    let max = a.max(b);
    if max == 0.0 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut compensation = 0.0;
    for v in [a, b] {
        let n = v / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `hexToOklch`.
pub fn hex_to_oklch(hex: &str) -> Oklch {
    let [r, g, b] = hex_to_rgb(hex).map(to_lin);
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    let big_l = 0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s;
    let a = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
    let b = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
    Oklch {
        l: big_l,
        c: hypot(a, b),
        h: ((b.atan2(a) * 180.0) / std::f64::consts::PI + 360.0) % 360.0,
    }
}

fn oklch_to_linear(Oklch { l, c, h }: Oklch) -> [f64; 3] {
    let a = c * ((h * std::f64::consts::PI) / 180.0).cos();
    let b = c * ((h * std::f64::consts::PI) / 180.0).sin();
    let l_ = (l + 0.3963377774 * a + 0.2158037573 * b).powf(3.0);
    let m_ = (l - 0.1055613458 * a - 0.0638541728 * b).powf(3.0);
    let s_ = (l - 0.0894841775 * a - 1.291485548 * b).powf(3.0);
    [
        4.0767416621 * l_ - 3.3077115913 * m_ + 0.2309699292 * s_,
        -1.2684380046 * l_ + 2.6097574011 * m_ - 0.3413193965 * s_,
        -0.0041960863 * l_ - 0.7034186147 * m_ + 1.707614701 * s_,
    ]
}

/// `oklchToHex`: reduces chroma until the colour fits sRGB (keeps lightness and hue).
pub fn oklch_to_hex(color: Oklch) -> String {
    let Oklch { l, c, h } = color;
    let in_gamut = |cc: f64| {
        oklch_to_linear(Oklch { l, c: cc, h })
            .iter()
            .all(|v| *v >= -0.0005 && *v <= 1.0005)
    };
    if in_gamut(c) {
        return rgb_to_hex(oklch_to_linear(color).map(to_gam));
    }
    let (mut lo, mut hi) = (0.0, c);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if in_gamut(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    rgb_to_hex(oklch_to_linear(Oklch { l, c: lo, h }).map(to_gam))
}

/// `luminance`: WCAG relative luminance.
pub fn luminance(hex: &str) -> f64 {
    let [r, g, b] = hex_to_rgb(hex).map(to_lin);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// `contrast`: WCAG contrast ratio of two hex colours.
pub fn contrast(a: &str, b: &str) -> f64 {
    let (x, y) = (luminance(a), luminance(b));
    (js::max(x, y) + 0.05) / (js::min(x, y) + 0.05)
}

/// `hueDistance`: degrees between two hues, 0..180.
pub fn hue_distance(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    js::min(d, 360.0 - d)
}

// ------------------------------------------------------------------ ramps

/// The steps of a ramp.
pub const STEPS: [u32; 11] = [50, 100, 200, 300, 400, 500, 600, 700, 800, 900, 950];
// Perceptually even lightness targets (OKLCH L). Light steps are close together because
// the eye separates light tints less; dark steps hold text contrast.
const L_TARGET: [f64; 11] = [
    0.977, 0.951, 0.903, 0.836, 0.748, 0.655, 0.566, 0.484, 0.405, 0.33, 0.255,
];
// Chroma envelope: tints and shades carry less chroma than the middle.
const C_SHAPE: [f64; 11] = [
    0.12, 0.24, 0.42, 0.62, 0.82, 0.96, 1.0, 0.94, 0.82, 0.66, 0.5,
];

/// Eleven hex colours, one per entry of [`STEPS`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ramp(pub [String; 11]);

impl Ramp {
    /// The colour of a step; `None` for a number that is not a step.
    pub fn get(&self, step: f64) -> Option<&str> {
        step_index(step).map(|i| self.0[i].as_str())
    }

    /// The colour of a step, or an empty text (which no contrast check passes).
    fn at(&self, step: f64) -> &str {
        self.get(step).unwrap_or_default()
    }

    /// `{ "50": "#...", ... }`.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        for (step, hex) in STEPS.iter().zip(&self.0) {
            map.insert(step.to_string(), Value::String(hex.clone()));
        }
        Value::Object(map)
    }
}

/// `STEPS.indexOf(step)`.
fn step_index(step: f64) -> Option<usize> {
    STEPS.iter().position(|s| f64::from(*s) == step)
}

/// `ramp`: eleven steps around a hue. `chroma` is the peak chroma; `hue_shift` rotates the
/// darker steps.
pub fn ramp(hue: f64, chroma: f64, hue_shift: f64) -> Ramp {
    let mut i = 0usize;
    Ramp([(); 11].map(|()| {
        let t = (i as f64 - 5.0) / 5.0;
        let hex = oklch_to_hex(Oklch {
            l: L_TARGET[i],
            c: chroma * C_SHAPE[i],
            h: (hue + hue_shift * t + 360.0) % 360.0,
        });
        i += 1;
        hex
    }))
}

/// `neutralRamp`: a neutral ramp tinted toward a hue.
pub fn neutral_ramp(hue: f64, tint: f64) -> Ramp {
    let mut i = 0usize;
    Ramp([(); 11].map(|()| {
        let hex = oklch_to_hex(Oklch {
            l: L_TARGET[i],
            c: tint * (0.6 + 0.4 * C_SHAPE[i]),
            h: hue,
        });
        i += 1;
        hex
    }))
}

/// `HARMONY`: degrees from the brand hue to the signature hue.
pub fn harmony(name: &str) -> Option<f64> {
    Some(match name {
        "analogous" => 30.0,
        "analogous-wide" => 60.0,
        "triadic" => 120.0,
        "split-complementary" => 150.0,
        "complementary" => 180.0,
        _ => return None,
    })
}

// Semantic status hues (OKLCH): green, blue, amber, red.
const STATUS_HUE: [(&str, f64, f64); 4] = [
    ("ok", 150.0, 0.17),
    ("info", 250.0, 0.15),
    ("warn", 75.0, 0.16),
    ("err", 27.0, 0.2),
];

/// `stepFor`: the first step of a ramp (from the light or the dark end) that reaches `min`
/// contrast on `bg`.
pub fn step_for(ramp: &Ramp, bg: &str, min: f64, from_light: bool) -> f64 {
    let hit = |i: &usize| contrast(&ramp.0[*i], bg) >= min;
    let found = if from_light {
        (0..11).find(hit)
    } else {
        (0..11).rev().find(hit)
    };
    match found {
        Some(i) => f64::from(STEPS[i]),
        None if from_light => 950.0,
        None => 50.0,
    }
}
