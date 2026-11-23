//! Ports `core/lib/color.mjs`: OKLCH ramps, contrast, palette building and the type scale.
//!
//! Results that reach a user are hex colours, steps and numbers printed with `toFixed`, so the
//! arithmetic follows the Node code operation by operation (`x ** 3` is `powf(3.0)`, not
//! `x * x * x`; `Math.hypot` is V8's scaled sum) to round the same way.
// Over 800 lines with its tests: one Node module, kept whole so the two read side by side.

use crate::core::js;
use serde_json::{json, Map, Value};

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

/// The signature seed of a palette.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Signature {
    pub hue: Option<f64>,
    pub harmony: Option<String>,
    pub chroma: Option<f64>,
    pub hue_shift: Option<f64>,
}

/// The decisions a palette is built from (`seeds` of `buildPalette`). `None` is `undefined`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Seeds {
    pub brand_hex: Option<String>,
    pub brand_hue: Option<f64>,
    pub brand_chroma: Option<f64>,
    pub brand_hue_shift: Option<f64>,
    pub neutral_hue: Option<f64>,
    pub neutral_tint: Option<f64>,
    pub signature: Option<Signature>,
    pub accent_from: Option<String>,
    pub dark: bool,
    pub warm_surface: bool,
    pub accent_step: Option<f64>,
    pub brand_step: Option<f64>,
    pub signature_step: Option<f64>,
    pub on_brand_signature: bool,
}

impl Seeds {
    /// Seeds from their JSON form (the `seeds` of a style guide): `null` counts as missing.
    pub fn from_value(seeds: &Value) -> Seeds {
        let get = |path: &[&str]| {
            let mut v = Some(seeds);
            for key in path {
                v = v.and_then(|v| v.get(key));
            }
            js::present(v)
        };
        let num = |path: &[&str]| get(path).map(|v| js::to_number(Some(v)));
        let text = |path: &[&str]| get(path).map(|v| js::to_string(Some(v)));
        Seeds {
            brand_hex: text(&["brand", "hex"]).filter(|h| !h.is_empty()),
            brand_hue: num(&["brand", "hue"]),
            brand_chroma: num(&["brand", "chroma"]),
            brand_hue_shift: num(&["brand", "hueShift"]),
            neutral_hue: num(&["neutral", "hue"]),
            neutral_tint: num(&["neutral", "tint"]),
            signature: get(&["signature"])
                .filter(|s| js::truthy(Some(s)))
                .map(|_| Signature {
                    hue: num(&["signature", "hue"]),
                    harmony: text(&["signature", "harmony"]),
                    chroma: num(&["signature", "chroma"]),
                    hue_shift: num(&["signature", "hueShift"]),
                }),
            accent_from: text(&["accentFrom"]),
            dark: js::truthy(get(&["dark"])),
            warm_surface: js::truthy(get(&["warmSurface"])),
            accent_step: num(&["accentStep"]),
            brand_step: num(&["brandStep"]),
            signature_step: num(&["signatureStep"]),
            on_brand_signature: js::truthy(get(&["onBrandSignature"])),
        }
    }
}

/// Palette names in the product's world (`names` of `buildPalette`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Names {
    pub brand: Option<String>,
    pub neutral: Option<String>,
    pub signature: Option<String>,
}

fn next_step(step: f64) -> f64 {
    // `STEPS[Math.min(10, STEPS.indexOf(step) + 1)]`: a number that is no step gives the first.
    f64::from(STEPS[step_index(step).map_or(0, |i| (i + 1).min(10))])
}

fn previous_step(step: f64) -> f64 {
    // `STEPS[Math.max(0, STEPS.indexOf(step) - 1)]`.
    f64::from(STEPS[step_index(step).map_or(0, |i| i.saturating_sub(1))])
}

/// `buildPalette`: ramps, roles picked by contrast and status colours from a few decisions.
/// Returns `{ ramps, roles, status, names, notes, seeds }` as the Node function did.
// Over 120 lines: the light and dark role tables are built side by side, so the two values
// of a role read together.
#[allow(clippy::too_many_lines, reason = "A10 debt")]
pub fn build_palette(seeds: &Seeds, names: &Names) -> Value {
    let brand_seed = match &seeds.brand_hex {
        Some(hex) => {
            let o = hex_to_oklch(hex);
            (Some(o.h), Some(o.c))
        }
        None => (seeds.brand_hue, seeds.brand_chroma),
    };
    let brand_hue = brand_seed.0.unwrap_or(f64::NAN);
    let brand_chroma = js::min(
        0.24,
        js::max(
            0.04,
            seeds.brand_chroma.or(brand_seed.1).unwrap_or(f64::NAN),
        ),
    );
    let n_hue = seeds.neutral_hue.unwrap_or(brand_hue);
    let tint = seeds.neutral_tint.unwrap_or(0.01);
    let sig = seeds.signature.as_ref();
    let sig_hue = sig.map(|s| {
        s.hue.unwrap_or_else(|| {
            let turn = s.harmony.as_deref().and_then(harmony).unwrap_or(180.0);
            (brand_hue + turn) % 360.0
        })
    });

    let name = |given: &Option<String>, fallback: &str| {
        given
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback.to_string())
    };
    let n_brand = name(&names.brand, "brand");
    let n_neutral = name(&names.neutral, "neutral");
    let n_signature = name(&names.signature, "signature");

    // An object in the Node code: a repeated name replaces the ramp and keeps its place.
    let mut ramps: Vec<(String, Ramp)> = Vec::new();
    let mut set_ramp = |name: &str, ramp: Ramp| match ramps.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 = ramp,
        None => ramps.push((name.to_string(), ramp)),
    };
    set_ramp(
        &n_brand,
        ramp(
            brand_hue,
            brand_chroma,
            seeds.brand_hue_shift.unwrap_or(0.0),
        ),
    );
    set_ramp(&n_neutral, neutral_ramp(n_hue, tint));
    if let (Some(s), Some(hue)) = (sig, sig_hue) {
        set_ramp(
            &n_signature,
            ramp(
                hue,
                s.chroma.unwrap_or(brand_chroma),
                s.hue_shift.unwrap_or(0.0),
            ),
        );
    }
    let ramp_of = |name: &str| {
        ramps
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, r)| r.clone())
            .unwrap_or_else(|| Ramp(Default::default()))
    };

    // Accent source: brand ramp (default), signature ramp, or the neutral ramp (ink buttons).
    let accent_name = match seeds.accent_from.as_deref() {
        Some("signature") if sig.is_some() => n_signature.clone(),
        Some("neutral") => n_neutral.clone(),
        _ => n_brand.clone(),
    };
    let accent_ramp = ramp_of(&accent_name);
    let accent_hue = if accent_name == n_signature {
        sig_hue.unwrap_or(f64::NAN)
    } else if accent_name == n_neutral {
        n_hue
    } else {
        brand_hue
    };
    let accent_is_chromatic = accent_name != n_neutral;
    // Status: semantic hues stay recognisable; only a small temperature pull toward the neutrals.
    let mut status_ramps: Vec<(&str, Ramp)> = Vec::new();
    let mut notes: Vec<Value> = Vec::new();
    for (k, base, chroma) in STATUS_HUE {
        let diff = ((n_hue - base + 540.0) % 360.0) - 180.0;
        let pull = js::max(-6.0, js::min(6.0, diff * js::min(1.0, tint / 0.02) * 0.1));
        status_ramps.push((k, ramp((base + pull + 360.0) % 360.0, chroma, 0.0)));
    }
    // Info = accent when they are close (a second blue next to the accent is noise).
    if accent_is_chromatic && hue_distance(250.0, accent_hue) < 40.0 {
        status_ramps[1].1 = accent_ramp.clone();
        notes.push(json!(
            "info uses the accent ramp (the accent is blue; a second blue would compete)."
        ));
    }

    let n = ramp_of(&n_neutral);
    let b = &accent_ramp;
    let mut roles = Map::new();
    let reference = |ramp_name: &str, step: f64| format!("{ramp_name}-{}", js::number(step));
    let mut role = |key: &str, value: String| {
        roles.insert(key.to_string(), Value::String(value));
    };
    let with_signature = sig.is_some() && seeds.on_brand_signature;
    if !seeds.dark {
        let surface = if seeds.warm_surface {
            reference(&n_neutral, 50.0)
        } else {
            "#ffffff".to_string()
        };
        role(
            "canvas",
            reference(&n_neutral, if seeds.warm_surface { 100.0 } else { 50.0 }),
        );
        let surf = if surface.starts_with('#') {
            surface.clone()
        } else {
            n.at(50.0).to_string()
        };
        role("surface", surface);
        role("surface-sunken", reference(&n_neutral, 100.0));
        role("surface-hover", reference(&n_neutral, 100.0));
        role("rule", reference(&n_neutral, 200.0));
        role(
            "border",
            reference(&n_neutral, step_for(&n, &surf, 3.0, true)),
        );
        role(
            "border-strong",
            reference(&n_neutral, step_for(&n, &surf, 4.5, true)),
        );
        let subtle = step_for(&n, &surf, 4.5, true);
        role("subtle", reference(&n_neutral, subtle));
        let mut muted = step_for(&n, n.at(100.0), 5.2, true);
        if muted == subtle {
            muted = next_step(subtle);
        }
        role("muted", reference(&n_neutral, muted));
        let ink = step_for(&n, n.at(100.0), 13.0, true);
        role(
            "ink",
            reference(
                &n_neutral,
                if ink == 950.0 {
                    950.0
                } else {
                    js::max(900.0, ink)
                },
            ),
        );
        let acc = seeds.accent_step.unwrap_or_else(|| {
            if accent_is_chromatic {
                step_for(b, &surf, 4.5, true)
            } else {
                900.0
            }
        });
        role("accent", reference(&accent_name, acc));
        role("accent-hover", reference(&accent_name, next_step(acc)));
        if accent_is_chromatic {
            role("accent-soft", reference(&accent_name, 50.0));
            let text = step_for(b, b.at(50.0), 4.5, true);
            role(
                "accent-text",
                reference(&accent_name, if text < 700.0 { 700.0 } else { text }),
            );
            role("focus-ring", reference(&accent_name, 200.0));
        } else {
            // Ink accent: selection must still be visible on the surface.
            role(
                "accent-soft",
                reference(&n_neutral, if seeds.warm_surface { 200.0 } else { 100.0 }),
            );
            role("accent-text", reference(&n_neutral, 900.0));
            role("focus-ring", reference(&n_neutral, 300.0));
        }
        role(
            "on-accent",
            if contrast("#ffffff", b.at(acc)) >= 4.5 {
                "#ffffff".to_string()
            } else {
                reference(&n_neutral, 950.0)
            },
        );
        role("focus", reference(&accent_name, acc));
        role(
            "brand",
            reference(&n_brand, seeds.brand_step.unwrap_or(800.0)),
        );
        role(
            "on-brand",
            reference(
                if with_signature {
                    &n_signature
                } else {
                    &n_neutral
                },
                if with_signature { 100.0 } else { 50.0 },
            ),
        );
        role(
            "signature",
            if sig.is_some() {
                reference(&n_signature, seeds.signature_step.unwrap_or(400.0))
            } else {
                reference(&n_brand, 400.0)
            },
        );
    } else {
        let on = n.at(900.0);
        role("canvas", reference(&n_neutral, 950.0));
        role("surface", reference(&n_neutral, 900.0));
        role("surface-sunken", reference(&n_neutral, 800.0));
        role("surface-hover", reference(&n_neutral, 800.0));
        role("rule", reference(&n_neutral, 700.0));
        role(
            "border",
            reference(&n_neutral, step_for(&n, on, 3.0, false)),
        );
        role("border-strong", reference(&n_neutral, 400.0));
        role(
            "subtle",
            reference(&n_neutral, step_for(&n, on, 4.5, false)),
        );
        role("muted", reference(&n_neutral, step_for(&n, on, 7.0, false)));
        role("ink", reference(&n_neutral, 50.0));
        let acc = seeds
            .accent_step
            .unwrap_or_else(|| step_for(b, on, 6.0, false));
        role("accent", reference(&accent_name, acc));
        role("accent-hover", reference(&accent_name, previous_step(acc)));
        role("accent-soft", reference(&accent_name, 950.0));
        role("accent-text", reference(&accent_name, 300.0));
        role("on-accent", reference(&n_neutral, 950.0));
        role("focus", reference(&accent_name, acc));
        role("focus-ring", reference(&accent_name, 800.0));
        role("brand", reference(&n_neutral, 800.0));
        role("on-brand", reference(&n_neutral, 50.0));
        role(
            "signature",
            if sig.is_some() {
                reference(&n_signature, 400.0)
            } else {
                reference(&n_brand, 400.0)
            },
        );
    }

    let mut status = Map::new();
    for (k, r) in &status_ramps {
        let (fill, tint_step, text) = if seeds.dark {
            (400.0, 950.0, 300.0)
        } else {
            let fill = if *k == "warn" {
                300.0
            } else {
                step_for(r, "#ffffff", 4.6, true)
            };
            let text = step_for(r, r.at(50.0), 6.0, true);
            (fill, 50.0, if text < 700.0 { 700.0 } else { text })
        };
        status.insert(k.to_string(), json!(r.at(fill)));
        status.insert(format!("{k}-tint"), json!(r.at(tint_step)));
        status.insert(format!("{k}-text"), json!(r.at(text)));
    }

    let mut ramps_json = Map::new();
    for (name, ramp) in &ramps {
        ramps_json.insert(name.clone(), ramp.to_json());
    }
    js::key_order(json!({
        "ramps": ramps_json,
        "roles": roles,
        "status": status,
        "names": { "brand": n_brand, "neutral": n_neutral, "signature": n_signature },
        "notes": notes,
        "seeds": {
            "brandHue": js::num(brand_hue),
            "brandChroma": js::num(brand_chroma),
            "nHue": js::num(n_hue),
            "tint": js::num(tint),
            "sigHue": sig_hue.map_or(Value::Null, js::num),
        },
    }))
}

// ------------------------------------------------------------------ audit

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
/// returns `[{ level, rule, message }]`.
// Over 120 lines: the checks run in the order their findings are printed (the golden tests
// compare that order).
#[allow(clippy::too_many_lines, reason = "A10 debt")]
pub fn audit_colors(input: &Value, exceptions: &[String]) -> Vec<Value> {
    let empty = Map::new();
    let roles = input
        .get("roles")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let status = input
        .get("status")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    let on_fill = input
        .get("onFill")
        .and_then(Value::as_str)
        .unwrap_or("#ffffff");
    let hex = |map: &Map<String, Value>, key: &str| -> Option<String> {
        map.get(key)
            .filter(|v| js::truthy(Some(v)))
            .map(|v| js::to_string(Some(v)))
    };
    let role = |key: &str| hex(roles, key);
    let stat = |key: &str| hex(status, key);
    let mut out = Vec::new();
    let mut add = |level: &str, rule: &str, message: String| {
        let excepted = exceptions.iter().any(|e| e == rule);
        out.push(json!({
            "level": if excepted && level == "error" { "info" } else { level },
            "rule": rule,
            "message": if excepted { format!("{message} (recorded exception)") } else { message },
        }));
    };
    for (fg, bg, min, must, what) in ROLE_PAIRS {
        let (Some(f), Some(b)) = (role(fg), role(bg)) else {
            continue;
        };
        let r = contrast(&f, &b);
        if r < min {
            add(
                if must { "error" } else { "warn" },
                "contrast",
                format!(
                    "{what}: {fg} on {bg} is {}:1, needs {}:1.",
                    js::to_fixed(r, 2),
                    js::number(min)
                ),
            );
        }
    }
    for s in ["ok", "err"] {
        if let Some(fill) = stat(s) {
            let r = contrast(on_fill, &fill);
            if r < 4.5 {
                add(
                    "error",
                    "contrast",
                    format!("Text on status.{s} fill is {}:1.", js::to_fixed(r, 2)),
                );
            }
        }
    }
    for s in ["ok", "info", "warn", "err"] {
        if let Some(text) = stat(&format!("{s}-text")) {
            let tint = js::to_string(status.get(&format!("{s}-tint")));
            let r = contrast(&text, &tint);
            if r < 4.5 {
                add(
                    "error",
                    "contrast",
                    format!("status.{s}-text on its tint is {}:1.", js::to_fixed(r, 2)),
                );
            }
        }
    }

    let accent = role("accent").unwrap_or_default();
    let acc = hex_to_oklch(&accent);
    for s in ["err", "ok"] {
        let Some(fill) = stat(s) else {
            continue;
        };
        let d = hue_distance(acc.h, hex_to_oklch(&fill).h);
        if acc.c > 0.05 && d < 25.0 {
            let reads_as = if s == "err" {
                "an error/destructive"
            } else {
                "a success"
            };
            add("error", "accent-vs-status", format!("Accent ({accent}) and status.{s} ({fill}) are only {}° apart: users will read the primary action as {reads_as} signal. Move the accent, or make the brand hue a surface/ornament colour and use a different accent.", js::to_fixed(d, 0)));
        } else if acc.c > 0.05 && d < 40.0 {
            add(
                "warn",
                "accent-vs-status",
                format!(
                    "Accent and status.{s} are {}° apart; keep icons + words on every {s} message.",
                    js::to_fixed(d, 0)
                ),
            );
        }
    }
    // Neutrals: one temperature.
    let tinted: Vec<(&str, Oklch)> = ["canvas", "surface-sunken", "rule", "border", "muted", "ink"]
        .into_iter()
        .filter_map(|r| role(r).map(|hex| (r, hex_to_oklch(&hex))))
        .filter(|(_, o)| o.c > 0.006)
        .collect();
    if let Some((first, rest)) = tinted.split_first() {
        if let Some(other) = rest
            .iter()
            .find(|(_, o)| hue_distance(first.1.h, o.h) > 40.0)
        {
            add("warn", "neutral-temperature", format!("Neutrals mix temperatures: {} hue {}° vs {} {}°. Tint all neutrals toward one hue.", first.0, js::to_fixed(first.1.h, 0), other.0, js::to_fixed(other.1.h, 0)));
        }
    }
    if let (Some(brand), Some(err)) = (role("brand"), stat("err")) {
        let br = hex_to_oklch(&brand);
        if br.c > 0.05 && hue_distance(br.h, hex_to_oklch(&err).h) < 20.0 {
            add("info", "brand-vs-error", "The brand hue shares the error hue: never place error messages on brand surfaces, and keep errors iconified with words.".to_string());
        }
    }
    let canvas = hex_to_oklch(&role("canvas").unwrap_or_default());
    if canvas.c > 0.035 {
        add("warn", "canvas-chroma", format!("Canvas chroma {} is high for a large surface (tires the eye, shifts every colour on it). Keep ≤ 0.03.", js::to_fixed(canvas.c, 3)));
    }
    // Hue budget.
    let mut hues: Vec<f64> = Vec::new();
    for (key, value) in js::entries(roles) {
        if key == "signature" {
            continue;
        }
        let c = hex_to_oklch(&js::to_string(Some(value)));
        if c.c > 0.05 && !hues.iter().any(|h| hue_distance(*h, c.h) < 25.0) {
            hues.push(c.h);
        }
    }
    if hues.len() > 2 {
        add("warn", "hue-budget", format!("{} distinct chromatic hues in UI roles (besides status). One accent hue (+ one brand/signature) reads as designed; more reads as decoration.", hues.len()));
    }
    out
}

// ------------------------------------------------------------------ type

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

/// `auditTypeScale`: check a scale for rhythm and hierarchy. `scale` is `{ ui: [px], display:
/// [px] }` or a flat list of UI sizes; returns `[{ level, rule, message }]`.
pub fn audit_type_scale(scale: &Value, body: f64, surface: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let ui = unique_sorted(if scale.is_array() {
        Some(scale)
    } else {
        scale.get("ui")
    });
    let display = unique_sorted(if scale.is_array() {
        None
    } else {
        scale.get("display")
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
            out.push(json!({ "level": "warn", "rule": "type-ratio", "message": format!("Display steps {} do not follow one ratio ({}). Use a modular scale (dfw type scale).", list(&display, "/"), ratios.join(", ")) }));
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
            out.push(json!({ "level": "warn", "rule": "type-contrast", "message": format!("Largest size {}px is only {}× body; {surface} UIs need ≥ {}× for clear hierarchy.", js::number(top), js::to_fixed(top / body, 1), js::number(need)) }));
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
        out.push(json!({ "level": "warn", "rule": "type-steps", "message": format!("{close} adjacent sizes are too close (e.g. 14/15): they look like mistakes, not hierarchy. Merge them or use a modular scale.") }));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `resolve` of tests/unit/color.test.mjs: role references to hex.
    fn resolve(pal: &Value) -> Value {
        let mut out = Map::new();
        for (k, v) in pal["roles"].as_object().unwrap() {
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
        assert!(c("ink", "surface") >= 13.0);
        assert!(c("border", "surface") >= 3.0);
        assert!(c("on-accent", "accent") >= 4.5);
        assert!(pal["notes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n.as_str().unwrap().contains("info uses the accent")));
        let findings = audit_colors(&json!({ "roles": roles, "status": pal["status"] }), &[]);
        assert!(
            !has(&findings, Some("error"), "contrast")
                && !findings.iter().any(|f| f["level"] == "error"),
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
            &json!({ "roles": resolve(&red), "status": red["status"] }),
            &[],
        );
        assert!(has(&f, Some("error"), "accent-vs-status"));
        let ink = build_palette(
            &seeds(
                json!({ "brand": { "hue": 28, "chroma": 0.16 }, "neutral": { "hue": 80, "tint": 0.012 }, "accentFrom": "neutral" }),
            ),
            &Names::default(),
        );
        let g = audit_colors(
            &json!({ "roles": resolve(&ink), "status": ink["status"] }),
            &[],
        );
        assert!(!has(&g, None, "accent-vs-status"));
        // status red stays red (not bent to orange to dodge the accent)
        assert!(hue_distance(hex_to_oklch(ink["status"]["err"].as_str().unwrap()).h, 27.0) <= 8.0);
        // A recorded exception turns the MUST into a note.
        let excepted = audit_colors(
            &json!({ "roles": resolve(&red), "status": red["status"] }),
            &["accent-vs-status".to_string()],
        );
        let finding = excepted
            .iter()
            .find(|x| x["rule"] == "accent-vs-status")
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
        let roles = json!({ "canvas": "#f3f5f8", "surface": "#ffffff", "surface-sunken": "#eef1f5", "rule": "#dde2e8", "border": "#7c8590", "muted": "#5a5048", "ink": "#2a2118", "accent": "#1d4ed8", "on-accent": "#ffffff", "accent-soft": "#e8effd", "accent-text": "#1e40af", "focus": "#1d4ed8" });
        assert!(has(
            &audit_colors(&json!({ "roles": roles, "status": {} }), &[]),
            None,
            "neutral-temperature"
        ));
        let t = audit_type_scale(
            &json!({ "ui": [12, 13, 14, 15, 16], "display": [22, 26, 34, 40] }),
            16.0,
            "product",
        );
        assert!(has(&t, None, "type-ratio"));
        assert!(has(&t, None, "type-steps"));
        let ts = type_scale(16.0, 1.2, 1.414, Some(30.0));
        let display: Vec<f64> = ts.display.iter().map(|d| d.1).collect();
        assert_eq!(display, [30.0, 42.0, 60.0, 85.0]);
        let ui: Vec<f64> = ts.ui.iter().take(7).map(|u| u.1).collect();
        assert_eq!(
            audit_type_scale(&json!({ "ui": ui, "display": display }), 16.0, "product"),
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
        assert_eq!(pal["roles"]["canvas"], "neutral-950");
        assert_eq!(pal["roles"]["ink"], "neutral-50");
        assert_eq!(
            pal["names"],
            json!({ "brand": "brand", "neutral": "neutral", "signature": "signature" })
        );
        assert_eq!(pal["seeds"]["sigHue"], Value::Null);
        let keys: Vec<&String> = pal["ramps"]["brand"].as_object().unwrap().keys().collect();
        assert_eq!(keys.len(), 11);
        assert_eq!(keys[0], "50");
        assert_eq!(pal["status"].as_object().unwrap().len(), 12);
    }
}
