//! `buildPalette`: the seeds and names of a palette, its ramps, the roles picked by contrast
//! and the status colours.

use super::{
    contrast, harmony, hex_to_oklch, hue_distance, neutral_ramp, ramp, step_for, step_index, Ramp,
    STATUS_HUE, STEPS,
};
use crate::core::js;
use serde_json::{json, Map, Value};

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

/// The keys of a palette ([`build_palette`]).
pub mod key {
    /// The OKLCH ramps, by name.
    pub const RAMPS: &str = "ramps";
    /// The role colours, by role.
    pub const ROLES: &str = "roles";
    /// The status colours, by name.
    pub const STATUS: &str = "status";
    /// The ramp names: `{ brand, neutral, signature }`.
    pub const NAMES: &str = "names";
    pub const BRAND: &str = "brand";
    pub const NEUTRAL: &str = "neutral";
    pub const SIGNATURE: &str = "signature";
    /// What the build adjusted, one line each.
    pub const NOTES: &str = "notes";
    /// The hues and chromas the palette is built around.
    pub const SEEDS: &str = "seeds";
    pub const BRAND_HUE: &str = "brandHue";
    pub const BRAND_CHROMA: &str = "brandChroma";
    /// The hue of the neutral ramp.
    pub const NEUTRAL_HUE: &str = "nHue";
    /// How much of the brand hue tints the neutrals.
    pub const TINT: &str = "tint";
    /// The hue of the signature ramp (`null` without one).
    pub const SIGNATURE_HUE: &str = "sigHue";
}

/// `buildPalette`: ramps, roles picked by contrast and status colours from a few decisions.
/// Returns `{ ramps, roles, status, names, notes, seeds }` as the Node function did.
pub fn build_palette(seeds: &Seeds, names: &Names) -> Value {
    let basis = palette_basis(seeds);
    let names = ramp_names(names);
    let ramps = build_ramps(seeds, &basis, &names);
    let accent = pick_accent(seeds, &basis, &names, &ramps);
    let mut notes: Vec<Value> = Vec::new();
    let status_ramps = status_ramps(&basis, &accent, &mut notes);

    let neutral = ramp_named(&ramps, &names.neutral);
    let inputs = RoleInputs {
        seeds,
        names: &names,
        neutral: &neutral,
        accent: &accent,
    };
    let roles = if seeds.dark {
        dark_roles(&inputs)
    } else {
        light_roles(&inputs)
    };
    let status = status_colors(&status_ramps, seeds.dark);

    let mut ramps_json = Map::new();
    for (name, ramp) in &ramps {
        ramps_json.insert(name.clone(), ramp.to_json());
    }
    js::key_order(json!({
        (key::RAMPS): ramps_json,
        (key::ROLES): roles,
        (key::STATUS): status,
        (key::NAMES): { (key::BRAND): names.brand, (key::NEUTRAL): names.neutral, (key::SIGNATURE): names.signature },
        (key::NOTES): notes,
        (key::SEEDS): {
            (key::BRAND_HUE): js::num(basis.brand_hue),
            (key::BRAND_CHROMA): js::num(basis.brand_chroma),
            (key::NEUTRAL_HUE): js::num(basis.n_hue),
            (key::TINT): js::num(basis.tint),
            (key::SIGNATURE_HUE): basis.sig_hue.map_or(Value::Null, js::num),
        },
    }))
}

/// The hues and chromas a palette is built around (the `seeds` of its result).
struct Basis {
    brand_hue: f64,
    brand_chroma: f64,
    n_hue: f64,
    tint: f64,
    sig_hue: Option<f64>,
}

/// The brand hue and chroma (from the brand hex when there is one), the neutral hue and tint,
/// and the signature hue (given, or turned from the brand hue by its harmony).
fn palette_basis(seeds: &Seeds) -> Basis {
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
    let sig_hue = seeds.signature.as_ref().map(|s| {
        s.hue.unwrap_or_else(|| {
            let turn = s.harmony.as_deref().and_then(harmony).unwrap_or(180.0);
            (brand_hue + turn) % 360.0
        })
    });
    Basis {
        brand_hue,
        brand_chroma,
        n_hue: seeds.neutral_hue.unwrap_or(brand_hue),
        tint: seeds.neutral_tint.unwrap_or(0.01),
        sig_hue,
    }
}

/// The names of the three ramps: the product's names, or `brand`, `neutral`, `signature`.
struct RampNames {
    brand: String,
    neutral: String,
    signature: String,
}

fn ramp_names(names: &Names) -> RampNames {
    let name = |given: &Option<String>, fallback: &str| {
        given
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| fallback.to_string())
    };
    RampNames {
        brand: name(&names.brand, "brand"),
        neutral: name(&names.neutral, "neutral"),
        signature: name(&names.signature, "signature"),
    }
}

/// The brand, neutral and (when there is a signature) signature ramps, by name.
fn build_ramps(seeds: &Seeds, basis: &Basis, names: &RampNames) -> Vec<(String, Ramp)> {
    // An object in the Node code: a repeated name replaces the ramp and keeps its place.
    let mut ramps: Vec<(String, Ramp)> = Vec::new();
    let mut set_ramp = |name: &str, ramp: Ramp| match ramps.iter_mut().find(|(n, _)| n == name) {
        Some(slot) => slot.1 = ramp,
        None => ramps.push((name.to_string(), ramp)),
    };
    set_ramp(
        &names.brand,
        ramp(
            basis.brand_hue,
            basis.brand_chroma,
            seeds.brand_hue_shift.unwrap_or(0.0),
        ),
    );
    set_ramp(&names.neutral, neutral_ramp(basis.n_hue, basis.tint));
    if let (Some(s), Some(hue)) = (seeds.signature.as_ref(), basis.sig_hue) {
        set_ramp(
            &names.signature,
            ramp(
                hue,
                s.chroma.unwrap_or(basis.brand_chroma),
                s.hue_shift.unwrap_or(0.0),
            ),
        );
    }
    ramps
}

/// The ramp of a name, or a ramp of empty colours when there is none.
fn ramp_named(ramps: &[(String, Ramp)], name: &str) -> Ramp {
    ramps
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, r)| r.clone())
        .unwrap_or_else(|| Ramp(Default::default()))
}

/// The ramp the accent roles are taken from.
struct Accent {
    name: String,
    ramp: Ramp,
    hue: f64,
    chromatic: bool,
}

/// Accent source: brand ramp (default), signature ramp, or the neutral ramp (ink buttons).
fn pick_accent(
    seeds: &Seeds,
    basis: &Basis,
    names: &RampNames,
    ramps: &[(String, Ramp)],
) -> Accent {
    let name = match seeds.accent_from.as_deref() {
        Some("signature") if seeds.signature.is_some() => names.signature.clone(),
        Some("neutral") => names.neutral.clone(),
        _ => names.brand.clone(),
    };
    let hue = if name == names.signature {
        basis.sig_hue.unwrap_or(f64::NAN)
    } else if name == names.neutral {
        basis.n_hue
    } else {
        basis.brand_hue
    };
    Accent {
        ramp: ramp_named(ramps, &name),
        hue,
        chromatic: name != names.neutral,
        name,
    }
}

/// The status ramps. Semantic hues stay recognisable; only a small temperature pull toward
/// the neutrals. Info is the accent ramp when the accent is blue (noted in `notes`).
fn status_ramps(
    basis: &Basis,
    accent: &Accent,
    notes: &mut Vec<Value>,
) -> Vec<(&'static str, Ramp)> {
    let mut status_ramps: Vec<(&str, Ramp)> = Vec::new();
    for (k, base, chroma) in STATUS_HUE {
        let diff = ((basis.n_hue - base + 540.0) % 360.0) - 180.0;
        let pull = js::max(
            -6.0,
            js::min(6.0, diff * js::min(1.0, basis.tint / 0.02) * 0.1),
        );
        status_ramps.push((k, ramp((base + pull + 360.0) % 360.0, chroma, 0.0)));
    }
    // Info = accent when they are close (a second blue next to the accent is noise).
    if accent.chromatic && hue_distance(250.0, accent.hue) < 40.0 {
        status_ramps[1].1 = accent.ramp.clone();
        notes.push(json!(
            "info uses the accent ramp (the accent is blue; a second blue would compete)."
        ));
    }
    status_ramps
}

/// What the role tables are picked from.
struct RoleInputs<'a> {
    seeds: &'a Seeds,
    names: &'a RampNames,
    neutral: &'a Ramp,
    accent: &'a Accent,
}

/// A role value naming a ramp step: `neutral-500`.
fn step_ref(ramp_name: &str, step: f64) -> String {
    format!("{ramp_name}-{}", js::number(step))
}

fn set_role(roles: &mut Map<String, Value>, key: &str, value: String) {
    roles.insert(key.to_string(), Value::String(value));
}

/// The roles of a light palette: surfaces and text from the neutral ramp, then the accent and
/// brand roles.
fn light_roles(r: &RoleInputs) -> Map<String, Value> {
    let mut roles = Map::new();
    let surface = if r.seeds.warm_surface {
        step_ref(&r.names.neutral, 50.0)
    } else {
        "#ffffff".to_string()
    };
    // The surface colour the contrast checks run against.
    let surf = if surface.starts_with('#') {
        surface.clone()
    } else {
        r.neutral.at(50.0).to_string()
    };
    light_neutral_roles(&mut roles, r, surface, &surf);
    light_accent_roles(&mut roles, r, &surf);
    roles
}

/// Canvas, surfaces, rules, borders and text of a light palette.
fn light_neutral_roles(
    roles: &mut Map<String, Value>,
    r: &RoleInputs,
    surface: String,
    surf: &str,
) {
    let (nn, n) = (r.names.neutral.as_str(), r.neutral);
    let warm = r.seeds.warm_surface;
    set_role(
        roles,
        "canvas",
        step_ref(nn, if warm { 100.0 } else { 50.0 }),
    );
    set_role(roles, "surface", surface);
    set_role(roles, "surface-sunken", step_ref(nn, 100.0));
    set_role(roles, "surface-hover", step_ref(nn, 100.0));
    set_role(roles, "rule", step_ref(nn, 200.0));
    set_role(roles, "border", step_ref(nn, step_for(n, surf, 3.0, true)));
    set_role(
        roles,
        "border-strong",
        step_ref(nn, step_for(n, surf, 4.5, true)),
    );
    let subtle = step_for(n, surf, 4.5, true);
    set_role(roles, "subtle", step_ref(nn, subtle));
    let mut muted = step_for(n, n.at(100.0), 5.2, true);
    if muted == subtle {
        muted = next_step(subtle);
    }
    set_role(roles, "muted", step_ref(nn, muted));
    let ink = step_for(n, n.at(100.0), 13.0, true);
    set_role(
        roles,
        "ink",
        step_ref(
            nn,
            if ink == 950.0 {
                950.0
            } else {
                js::max(900.0, ink)
            },
        ),
    );
}

/// Accent, focus, brand and signature roles of a light palette.
fn light_accent_roles(roles: &mut Map<String, Value>, r: &RoleInputs, surf: &str) {
    let (seeds, names, b) = (r.seeds, r.names, &r.accent.ramp);
    let (an, nn) = (r.accent.name.as_str(), names.neutral.as_str());
    let chromatic = r.accent.chromatic;
    let acc = seeds.accent_step.unwrap_or_else(|| {
        if chromatic {
            step_for(b, surf, 4.5, true)
        } else {
            900.0
        }
    });
    set_role(roles, "accent", step_ref(an, acc));
    set_role(roles, "accent-hover", step_ref(an, next_step(acc)));
    if chromatic {
        set_role(roles, "accent-soft", step_ref(an, 50.0));
        let text = step_for(b, b.at(50.0), 4.5, true);
        set_role(
            roles,
            "accent-text",
            step_ref(an, if text < 700.0 { 700.0 } else { text }),
        );
        set_role(roles, "focus-ring", step_ref(an, 200.0));
    } else {
        // Ink accent: selection must still be visible on the surface.
        set_role(
            roles,
            "accent-soft",
            step_ref(nn, if seeds.warm_surface { 200.0 } else { 100.0 }),
        );
        set_role(roles, "accent-text", step_ref(nn, 900.0));
        set_role(roles, "focus-ring", step_ref(nn, 300.0));
    }
    set_role(
        roles,
        "on-accent",
        if contrast("#ffffff", b.at(acc)) >= 4.5 {
            "#ffffff".to_string()
        } else {
            step_ref(nn, 950.0)
        },
    );
    set_role(roles, "focus", step_ref(an, acc));
    set_role(
        roles,
        "brand",
        step_ref(&names.brand, seeds.brand_step.unwrap_or(800.0)),
    );
    let with_signature = seeds.signature.is_some() && seeds.on_brand_signature;
    set_role(
        roles,
        "on-brand",
        if with_signature {
            step_ref(&names.signature, 100.0)
        } else {
            step_ref(nn, 50.0)
        },
    );
    set_role(
        roles,
        "signature",
        if seeds.signature.is_some() {
            step_ref(&names.signature, seeds.signature_step.unwrap_or(400.0))
        } else {
            step_ref(&names.brand, 400.0)
        },
    );
}

/// The roles of a dark palette, picked against the 900 neutral.
fn dark_roles(r: &RoleInputs) -> Map<String, Value> {
    let mut roles = Map::new();
    let (nn, n, b) = (r.names.neutral.as_str(), r.neutral, &r.accent.ramp);
    let an = r.accent.name.as_str();
    let on = n.at(900.0);
    set_role(&mut roles, "canvas", step_ref(nn, 950.0));
    set_role(&mut roles, "surface", step_ref(nn, 900.0));
    set_role(&mut roles, "surface-sunken", step_ref(nn, 800.0));
    set_role(&mut roles, "surface-hover", step_ref(nn, 800.0));
    set_role(&mut roles, "rule", step_ref(nn, 700.0));
    set_role(
        &mut roles,
        "border",
        step_ref(nn, step_for(n, on, 3.0, false)),
    );
    set_role(&mut roles, "border-strong", step_ref(nn, 400.0));
    set_role(
        &mut roles,
        "subtle",
        step_ref(nn, step_for(n, on, 4.5, false)),
    );
    set_role(
        &mut roles,
        "muted",
        step_ref(nn, step_for(n, on, 7.0, false)),
    );
    set_role(&mut roles, "ink", step_ref(nn, 50.0));
    let acc = r
        .seeds
        .accent_step
        .unwrap_or_else(|| step_for(b, on, 6.0, false));
    set_role(&mut roles, "accent", step_ref(an, acc));
    set_role(&mut roles, "accent-hover", step_ref(an, previous_step(acc)));
    set_role(&mut roles, "accent-soft", step_ref(an, 950.0));
    set_role(&mut roles, "accent-text", step_ref(an, 300.0));
    set_role(&mut roles, "on-accent", step_ref(nn, 950.0));
    set_role(&mut roles, "focus", step_ref(an, acc));
    set_role(&mut roles, "focus-ring", step_ref(an, 800.0));
    set_role(&mut roles, "brand", step_ref(nn, 800.0));
    set_role(&mut roles, "on-brand", step_ref(nn, 50.0));
    set_role(
        &mut roles,
        "signature",
        if r.seeds.signature.is_some() {
            step_ref(&r.names.signature, 400.0)
        } else {
            step_ref(&r.names.brand, 400.0)
        },
    );
    roles
}

/// The fill, tint and text colour of each status (`ok`, `ok-tint`, `ok-text`, ...).
fn status_colors(status_ramps: &[(&str, Ramp)], dark: bool) -> Map<String, Value> {
    let mut status = Map::new();
    for (k, r) in status_ramps {
        let (fill, tint_step, text) = if dark {
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
    status
}
