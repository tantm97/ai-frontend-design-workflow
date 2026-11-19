//! Ports `core/lib/style.mjs`: curated style guides (list, search, show, apply, match).
//!
//! A guide (`kits/styles/*.json`) is applied as DTCG tokens on top of a kit preset: a named
//! palette with usage rules, semantic roles as aliases, fonts, display scale, shape, tinted
//! elevation, density and signature motifs. Guides are read as JSON values so a project's own
//! guide file may carry less than the kit's.
// Over 800 lines with its tests: one Node module, kept whole so the two read side by side.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Map, Value};

use super::icons::plugin_dir;
use super::tokens::dir_names;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::{write_atomic, write_json};

/// `STYLES_DIR`: the guides shipped with dfw.
pub fn styles_dir() -> Result<PathBuf, DfwError> {
    Ok(plugin_dir()?.join("kits").join("styles"))
}

fn presets_dir() -> Result<PathBuf, DfwError> {
    Ok(plugin_dir()?.join("kits").join("base").join("presets"))
}

fn text(v: Option<&Value>) -> String {
    js::to_string(v)
}

fn list(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    list(v).iter().map(|x| text(Some(x))).collect()
}

fn entries(v: Option<&Value>) -> Vec<(&String, &Value)> {
    v.and_then(Value::as_object)
        .map(js::entries)
        .unwrap_or_default()
}

/// `listStyles`: every guide of the library, in file-name order.
pub fn list_styles() -> Result<Vec<Value>, DfwError> {
    let dir = styles_dir()?;
    let mut guides = Vec::new();
    for f in dir_names(&dir)?.iter().filter(|f| f.ends_with(".json")) {
        guides.push(js::read_json(&dir.join(f), None)?);
    }
    Ok(guides)
}

/// `getStyle`: a guide by id, or a project's own guide by the path of its `.json` file.
pub fn get_style(id: &str) -> Result<Value, DfwError> {
    if id.ends_with(".json") {
        let own = js::read_json(&resolve(id), Some(Value::Null))?;
        if !js::truthy(Some(&own)) {
            return Err(DfwError::new(format!("Style guide file not found: {id}")));
        }
        let mut guide = Map::new();
        guide.insert(
            "status".into(),
            get_style("swiss-grid")?
                .get("status")
                .cloned()
                .unwrap_or(Value::Null),
        );
        guide.insert("onFill".into(), json!("#ffffff"));
        for key in [
            "motifs",
            "contrasts",
            "fits",
            "tags",
            "do",
            "dont",
            "components",
        ] {
            guide.insert(key.into(), json!([]));
        }
        for (k, v) in entries(Some(&own)) {
            guide.insert(k.clone(), v.clone());
        }
        return Ok(js::key_order(Value::Object(guide)));
    }
    let f = styles_dir()?.join(format!("{id}.json"));
    if !f.exists() {
        return Err(DfwError::new(format!(
            "Unknown style guide \"{id}\". List them: dfw style list"
        )));
    }
    js::read_json(&f, None)
}

/// `isStyle`: the name of a guide file, or the id of a guide of the library.
pub fn is_style(id: &str) -> bool {
    id.ends_with(".json") || styles_dir().is_ok_and(|dir| dir.join(format!("{id}.json")).exists())
}

/// `searchStyles`: guides ranked by tag and concept match, as `{ id, name, score, tags, concept }`.
pub fn search_styles(query: &str) -> Result<Vec<Value>, DfwError> {
    let words = js::words(&query.to_lowercase());
    let mut hits: Vec<(f64, String, Value)> = Vec::new();
    for g in list_styles()? {
        let tags = strings(g.get("tags"));
        let hay = format!(
            "{} {} {}",
            text(g.get("name")),
            text(g.get("concept")),
            strings(g.get("fits")).join(" ")
        )
        .to_lowercase();
        let mut score = 0.0;
        for w in &words {
            if tags.contains(w) {
                score += 5.0;
            } else if tags.iter().any(|t| t.contains(w.as_str())) {
                score += 2.0;
            }
            if hay.contains(w.as_str()) {
                score += 1.0;
            }
        }
        if score > 0.0 || words.is_empty() {
            let mut hit = Map::new();
            for (key, value) in [("id", g.get("id")), ("name", g.get("name"))] {
                if let Some(value) = value {
                    hit.insert(key.into(), value.clone());
                }
            }
            hit.insert("score".into(), json!(score));
            for (key, value) in [("tags", g.get("tags")), ("concept", g.get("concept"))] {
                if let Some(value) = value {
                    hit.insert(key.into(), value.clone());
                }
            }
            hits.push((score, text(g.get("id")), Value::Object(hit)));
        }
    }
    // `a.id.localeCompare(b.id)`: ids are lower-case ASCII words joined by "-", which collate as
    // their bytes do.
    hits.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    Ok(hits.into_iter().map(|h| h.2).collect())
}

/// `hexComp`: the channels of `#rrggbb` from 0 to 1, rounded to four decimals.
fn hex_comp(hex: &str) -> Value {
    let chars: Vec<char> = hex.chars().collect();
    let channel = |i: usize| {
        let pair: String = chars.iter().skip(i).take(2).collect();
        let n = u8::from_str_radix(&pair, 16)
            .ok()
            .filter(|_| pair.len() == 2 && !pair.starts_with(['+', '-']));
        n.map_or(Value::Null, |n| {
            js::num(js::round((f64::from(n) / 255.0) * 10000.0) / 10000.0)
        })
    };
    json!([channel(1), channel(3), channel(5)])
}

/// A colour token; `description` is left out when empty.
fn col(hex: &str, description: Option<&str>) -> Value {
    let mut token = Map::new();
    token.insert(
        "$value".into(),
        json!({ "colorSpace": "srgb", "components": hex_comp(hex), "hex": hex.to_lowercase() }),
    );
    if let Some(d) = description.filter(|d| !d.is_empty()) {
        token.insert("$description".into(), json!(d));
    }
    Value::Object(token)
}

/// `{ $value: { value, unit: "px" } }`; a missing value leaves only the unit, as in Node.
fn px(value: Option<&Value>) -> Value {
    let mut dimension = Map::new();
    if let Some(v) = value {
        dimension.insert("value".into(), v.clone());
    }
    dimension.insert("unit".into(), json!("px"));
    json!({ "$value": dimension })
}

fn role_description(role: &str) -> &'static str {
    match role {
        "canvas" => "App/page background.",
        "surface" => "Cards, tables, inputs.",
        "surface-sunken" => "Table headers, wells, quiet panels.",
        "surface-hover" => "Hover background.",
        "ink" => "Primary text.",
        "muted" => "Secondary text.",
        "subtle" => "Placeholders, disabled text.",
        "border" => "Control boundaries (3:1).",
        "border-strong" => "Hovered/emphasised boundaries.",
        "rule" => "Dividers only, never the sole boundary of a control.",
        "accent" => "Primary action, current item, links.",
        "accent-hover" => "Hover/pressed accent.",
        "accent-soft" => "Selected rows, active nav background.",
        "accent-text" => "Accent-coloured text on light backgrounds.",
        "on-accent" => "Text on accent fills.",
        "focus" => "Focus ring.",
        "focus-ring" => "Soft focus halo.",
        "brand" => "Large brand surfaces (headers, hero bands, sidebars).",
        "on-brand" => "Text on brand surfaces.",
        "signature" => "Signature motif colour (ornament, tape, rules) — not for body text.",
        _ => "",
    }
}

/// Control heights by density: `control`, `control-sm`, `control-lg`.
fn density(name: Option<&str>) -> [(&'static str, u32); 3] {
    let [control, sm, lg] = match name {
        Some("compact") => [32, 28, 40],
        Some("large") => [44, 36, 56],
        _ => [40, 34, 48],
    };
    [("control", control), ("control-sm", sm), ("control-lg", lg)]
}

fn shadows(tint: &str, hard: bool) -> Value {
    let c = hex_comp(tint);
    let layer = |x: i32, y: i32, b: i32, s: i32, a: f64| {
        json!({
            "color": { "colorSpace": "srgb", "components": c, "alpha": a },
            "offsetX": { "value": x, "unit": "px" },
            "offsetY": { "value": y, "unit": "px" },
            "blur": { "value": b, "unit": "px" },
            "spread": { "value": s, "unit": "px" },
        })
    };
    let mut out = json!({
        "$type": "shadow",
        "sm": { "$value": [layer(0, 1, 2, 0, 0.08)], "$description": format!("Tinted with {tint}, not black.") },
        "md": { "$value": [layer(0, 2, 6, -1, 0.08), layer(0, 12, 28, -8, 0.16)] },
        "lg": { "$value": [layer(0, 4, 10, -2, 0.1), layer(0, 24, 56, -12, 0.24)] },
    });
    if hard {
        out["hard"] = json!({ "$value": [layer(4, 4, 0, 0, 1.0)], "$description": "Hard offset shadow (signature of this style)." });
    }
    out
}

/// The font set of a guide: the English-only one when asked for and present.
fn fonts_for<'a>(g: &'a Value, which: &str) -> Option<&'a Value> {
    match g
        .get("fontsLatin")
        .filter(|f| which == "latin" && js::truthy(Some(f)))
    {
        Some(latin) => Some(latin),
        None => g.get("fonts"),
    }
}

/// `headFor`: the font links of a guide (`which` is `vi` or `latin`).
pub fn head_for(g: &Value, which: &str) -> String {
    let f = fonts_for(g, which);
    let font = |key: &str| f.and_then(|f| f.get(key));
    let mut families: Vec<(String, Vec<String>)> = Vec::new();
    let mut add = |name: Option<&Value>, weights: &str| {
        let name = text(name);
        let index = match families.iter().position(|(n, _)| *n == name) {
            Some(i) => i,
            None => {
                families.push((name, Vec::new()));
                families.len() - 1
            }
        };
        for w in weights.split(';') {
            if !families[index].1.iter().any(|x| x == w) {
                families[index].1.push(w.to_string());
            }
        }
    };
    add(font("body"), "400;500;600;700");
    let display_weights = font("displayWeights")
        .filter(|w| js::truthy(Some(w)))
        .map_or("400;600".to_string(), |w| text(Some(w)));
    add(font("display"), &display_weights);
    add(font("mono"), "400;500");
    let query: Vec<String> = families
        .iter()
        .map(|(name, weights)| {
            let mut numbers: Vec<f64> = weights.iter().map(|w| js::str_to_number(w)).collect();
            numbers.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let weights: Vec<String> = numbers.into_iter().map(js::number).collect();
            format!(
                "family={}:wght@{}",
                name.replace(' ', "+"),
                weights.join(";")
            )
        })
        .collect();
    let kind = if which == "latin" {
        "English-only"
    } else {
        "Vietnamese-capable"
    };
    format!(
        "<!-- Fonts for style \"{}\" ({kind}; SIL OFL). Self-host for production. -->\n<link rel=\"preconnect\" href=\"https://fonts.googleapis.com\">\n<link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\n<link rel=\"stylesheet\" href=\"https://fonts.googleapis.com/css2?{}&display=swap\">\n",
        text(g.get("id")),
        query.join("&")
    )
}

const SERIF_DISPLAY: [&str; 6] = [
    "Serif",
    "Garamond",
    "Playfair",
    "Bodoni",
    "Fraunces",
    "Newsreader",
];

/// The token group at `path` of a token tree (`t.font.size` in the Node code).
fn group<'a>(tree: &'a mut Value, path: &[&str]) -> Result<&'a mut Map<String, Value>, DfwError> {
    let missing = || {
        DfwError::internal(format!(
            "The preset tokens have no \"{}\" group.",
            path.join(".")
        ))
    };
    let mut node = tree;
    for key in path {
        node = node.get_mut(key).ok_or_else(missing)?;
    }
    node.as_object_mut().ok_or_else(missing)
}

/// `styleTokens`: the DTCG token tree of a guide (the kit preset it is based on, with the
/// guide's colour, type, shape and elevation on top).
// One function as in style.mjs: the token groups are written in the order the Node code
// assigned them, which is the order of the file users diff.
#[allow(clippy::too_many_lines)]
pub fn style_tokens(g: &Value, fonts: &str) -> Result<Value, DfwError> {
    let mut t = js::read_json(
        &presets_dir()?.join(format!("{}.tokens.json", text(g.get("base")))),
        None,
    )?;
    let hex_of = |v: &Value| text(Some(v));

    let mut palette = Map::new();
    for (k, v) in entries(g.get("palette")) {
        palette.insert(
            k.clone(),
            col(&text(v.get("hex")), v.get("usage").and_then(Value::as_str)),
        );
    }
    let mut status = Map::new();
    for (k, v) in entries(g.get("status")) {
        status.insert(k.clone(), col(&hex_of(v), None));
    }
    let mut color = Map::new();
    color.insert("$type".into(), json!("color"));
    color.insert("palette".into(), Value::Object(palette));
    for (k, v) in entries(g.get("roles")) {
        let v = hex_of(v);
        let value = if v.starts_with('#') {
            col(&v, None)["$value"].clone()
        } else {
            json!(format!("{{color.palette.{v}}}"))
        };
        color.insert(
            k.clone(),
            json!({ "$value": value, "$description": role_description(k) }),
        );
    }
    let on_fill = g
        .get("onFill")
        .filter(|f| js::truthy(Some(f)))
        .map_or("#ffffff".to_string(), hex_of);
    color.insert(
        "on-fill".into(),
        col(&on_fill, Some("Text on ok/err/info fills.")),
    );
    color.insert(
        "on-warn".into(),
        col("#111518", Some("Text on the warn fill.")),
    );
    color.insert("status".into(), Value::Object(status));
    group(&mut t, &[])?.insert("color".into(), Value::Object(color));

    let f = fonts_for(g, fonts);
    let font_name = |key: &str| f.and_then(|f| f.get(key)).cloned().unwrap_or(Value::Null);
    let display = text(f.and_then(|f| f.get("display")));
    let stack = |name: Value, fallback: &[&str]| {
        let mut families = vec![name];
        families.extend(fallback.iter().map(|x| json!(x)));
        Value::Array(families)
    };
    let sans = ["Segoe UI", "system-ui", "sans-serif"];
    let serif_stack = SERIF_DISPLAY
        .iter()
        .chain(&["Lora", "Merriweather"])
        .any(|s| display.contains(s));
    let note = g
        .get("fonts")
        .and_then(|f| f.get("note"))
        .filter(|n| js::truthy(Some(n)))
        .cloned()
        .unwrap_or(json!(""));
    group(&mut t, &["font"])?.insert(
        "family".into(),
        json!({
            "$type": "fontFamily",
            "body": { "$value": stack(font_name("body"), &sans) },
            "display": { "$value": stack(font_name("display"), if serif_stack { &["Georgia", "serif"] } else { &sans }), "$description": note },
            "mono": { "$value": stack(font_name("mono"), &["Consolas", "monospace"]) },
        }),
    );
    let ty = g.get("type");
    let type_of = |key: &str| ty.and_then(|t| t.get(key));
    let size = group(&mut t, &["font", "size"])?;
    if js::truthy(type_of("ui")) {
        for (k, v) in entries(type_of("ui")) {
            let mut token = px(Some(v));
            if k == "md" {
                token["$description"] = json!(format!(
                    "Body. Scale: base {}, UI ratio {}, display ratio {}.",
                    text(type_of("base")),
                    text(type_of("ratio")),
                    text(type_of("displayRatio"))
                ));
            }
            size.insert(k.clone(), token);
        }
    }
    let steps = list(type_of("display"));
    for (i, key) in ["display-xl", "display-lg", "display-md", "display-sm"]
        .iter()
        .enumerate()
    {
        size.insert((*key).into(), px(steps.get(i)));
    }
    let serif = SERIF_DISPLAY.iter().any(|s| display.contains(s));
    for (name, value) in [
        ("weight", json!(if serif { 400 } else { 600 })),
        ("leading", json!(if serif { 1.05 } else { 1.02 })),
        (
            "tracking",
            json!({ "value": if serif { -0.01 } else { -0.025 }, "unit": "em" }),
        ),
    ] {
        group(&mut t, &["font", name])?.insert("display".into(), json!({ "$value": value }));
    }
    let eyebrow = type_of("eyebrow");
    let eye = |key: &str| eyebrow.and_then(|e| e.get(key));
    let dimension = |value: Option<Value>, unit: &str| {
        let mut d = Map::new();
        if let Some(v) = value {
            d.insert("value".into(), v);
        }
        d.insert("unit".into(), json!(unit));
        Value::Object(d)
    };
    let mut weight = Map::new();
    weight.insert("$type".into(), json!("fontWeight"));
    if let Some(w) = eye("weight") {
        weight.insert("$value".into(), w.clone());
    }
    group(&mut t, &["font"])?.insert(
        "eyebrow".into(),
        json!({
            "size": { "$type": "dimension", "$value": dimension(eye("size").cloned(), "px") },
            "weight": weight,
            "tracking": { "$type": "dimension", "$value": dimension(Some(js::num(js::parse_float(&text(eye("tracking"))))), "em") },
            "case": { "$type": "string", "$value": if js::truthy(eye("upper")) { "uppercase" } else { "none" }, "$description": "text-transform for eyebrows (dfw string token)" },
        }),
    );
    let mut radius = Map::new();
    radius.insert("$type".into(), json!("dimension"));
    for (k, v) in entries(g.get("radius")) {
        radius.insert(k.clone(), px(Some(v)));
    }
    radius.insert("pill".into(), px(Some(&json!(999))));
    group(&mut t, &[])?.insert("radius".into(), Value::Object(radius));
    let sizes = group(&mut t, &["size"])?;
    for (k, v) in density(g.get("density").and_then(Value::as_str)) {
        sizes.insert(k.into(), px(Some(&json!(v))));
    }
    let root = group(&mut t, &[])?;
    root.insert(
        "shadow".into(),
        shadows(&text(g.get("shadowTint")), js::truthy(g.get("hardShadow"))),
    );
    root.insert(
        "$description".into(),
        json!(format!(
            "Style guide \"{}\" ({}): {}",
            text(g.get("name")),
            text(g.get("id")),
            text(g.get("concept"))
        )),
    );
    let mut dfw = Map::new();
    if let Some(id) = g.get("id") {
        dfw.insert("style".into(), id.clone());
    }
    dfw.insert("fonts".into(), json!(fonts));
    root.insert("$extensions".into(), json!({ "dfw": dfw }));
    Ok(js::key_order(t))
}

/// `motifComponent`: the motif master of a guide (one component with a variant per motif), or
/// `None` when the guide has no motif.
pub fn motif_component(g: &Value) -> Option<String> {
    let motifs = list(g.get("motifs"));
    if motifs.is_empty() {
        return None;
    }
    let field = |m: &Value, key: &str| text(m.get(key));
    let names: Vec<String> = motifs.iter().map(|m| field(m, "name")).collect();
    let desc: Vec<String> = motifs
        .iter()
        .map(|m| format!("{}: {}", field(m, "name"), field(m, "use")))
        .collect();
    let css: Vec<String> = motifs.iter().map(|m| field(m, "css")).collect();
    let examples: Vec<String> = names.iter().map(|n| format!("<div style=\"min-width:240px\"><dfw-use c=\"motif\" variant=\"{n}\">{n}</dfw-use></div>")).collect();
    let (id, name) = (text(g.get("id")), text(g.get("name")));
    Some(format!(
        "<dfw-component name=\"motif\" group=\"Signature\" variants=\"{variants}\" props=\"as=div\"
  description=\"Signature motifs of the {name} style. {desc}\">
<style>
/* Motifs belong to the \"{id}\" style. While comparing other option themes, decorative motifs
   are hidden and motifs with content fall back to plain text (content never disappears). */
[data-theme^=\"opt-\"]:not([data-theme=\"opt-{id}\"]) .motif:empty {{ display: none; }}
[data-theme^=\"opt-\"]:not([data-theme=\"opt-{id}\"]) .motif:not(:empty) {{ all: unset; font-family: var(--font-family-mono); font-size: var(--font-size-sm); color: var(--color-muted); }}
{css}
</style>
<template>
<{{{{as}}}} class=\"motif motif--{{{{variant}}}}\"><dfw-slot></dfw-slot></{{{{as}}}}>
</template>
<dfw-examples>
{examples}
</dfw-examples>
</dfw-component>
",
        variants = names.join(" "),
        desc = desc.join(" | ").replace('"', "'"),
        css = css.join("\n"),
        examples = examples.join("\n"),
    ))
}

/// White space and "any character" as JavaScript regular expressions define them.
const WS: &str = crate::core::js::SPACE_CLASS;
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

fn pattern(source: &str) -> Result<Regex, DfwError> {
    Regex::new(source).map_err(|err| DfwError::internal(err.to_string()))
}

/// Seed DESIGN.md with the guide's reasoning so the DS gate starts from reasons, not from a
/// blank page. A rationale the user has written is never replaced.
fn seed_rationale(design_md: &Path, id: &str, rationale: &[String]) -> Result<(), DfwError> {
    let text = js::read_text(design_md)?;
    let lines: Vec<String> = rationale.iter().map(|r| format!("- {r}")).collect();
    let block = format!("## Rationale\n<!-- Seeded from style guide \"{id}\". Rewrite each line for THIS product: decision - because <context fact or principle>. -->\n{}\n", lines.join("\n"));
    let seeded = pattern(&format!(
        r#"(?m)^## Rationale{WS}*\n<!-- Seeded from style guide "[^"]*"[\s\S]*?-->\n(?:- {DOT}*\n?)*"#
    ))?;
    let heading = pattern(&format!(r"(?m)^## Rationale{WS}*$"))?;
    let written = pattern(&format!(
        r"(?m)^## Rationale{WS}*$\n(?:<!--[\s\S]*?-->\n)?- "
    ))?;
    let empty = pattern(&format!(r"(?m)^## Rationale{WS}*$\n(?:<!--[\s\S]*?-->\n)?"))?;
    let out = if let Some(m) = seeded.find(&text) {
        // Still the untouched seed of another guide: replace it.
        js::replace_range(&text, m.range(), &block)
    } else if heading.is_match(&text) {
        match empty.find(&text) {
            Some(m) if !written.is_match(&text) => js::replace_range(&text, m.range(), &block),
            _ => return Ok(()),
        }
    } else {
        format!("{}\n\n{block}", js::trim_end(&text))
    };
    write_atomic(design_md, out)?;
    Ok(())
}

/// What [`apply_style`] applied.
#[derive(Debug, Clone, PartialEq)]
pub struct AppliedStyle {
    /// The guide's id (`undefined` when its file has none, as Node printed it).
    pub id: String,
    /// The font set written: `{ display, body, mono, ... }`.
    pub fonts: Value,
}

/// `applyStyle`: write a guide to the project: `tokens.json`, `head.html`, the motif master and
/// `design/system/style.json`.
pub fn apply_style(root: &Path, id: &str, fonts: &str) -> Result<AppliedStyle, DfwError> {
    let g = get_style(id)?;
    let p = project_paths(root);
    fs::create_dir_all(&p.tokens)?;
    write_json(&p.tokens.join("tokens.json"), &style_tokens(&g, fonts)?)?;
    let sys = p.design.join("system");
    fs::create_dir_all(sys.join("components"))?;
    write_atomic(&sys.join("head.html"), head_for(&g, fonts))?;
    if let Some(motif) = motif_component(&g) {
        write_atomic(&sys.join("components").join("motif.html"), motif)?;
    }
    let font_set = fonts_for(&g, fonts).cloned();
    let mut style = Map::new();
    let mut put = |key: &str, value: Option<Value>| {
        if let Some(value) = value {
            style.insert(key.to_string(), value);
        }
    };
    let or_empty = |key: &str| Some(g.get(key).cloned().unwrap_or(json!([])));
    let ty = g.get("type");
    let mut type_summary = Map::new();
    for key in ["base", "ratio", "displayRatio"] {
        if let Some(v) = ty.and_then(|t| t.get(key)) {
            type_summary.insert(key.into(), v.clone());
        }
    }
    let motifs: Vec<Value> = list(g.get("motifs"))
        .iter()
        .map(|m| {
            let mut motif = Map::new();
            for key in ["name", "use"] {
                if let Some(v) = m.get(key) {
                    motif.insert(key.into(), v.clone());
                }
            }
            Value::Object(motif)
        })
        .collect();
    for key in ["id", "name", "tags", "axes", "concept"] {
        put(key, g.get(key).cloned());
    }
    put("rationale", or_empty("rationale"));
    put("seeds", g.get("seeds").cloned());
    put("type", Some(Value::Object(type_summary)));
    for key in ["fits", "avoid", "layout", "components", "do", "dont"] {
        put(key, g.get(key).cloned());
    }
    put("motifs", Some(Value::Array(motifs)));
    put("roles", g.get("roles").cloned());
    put("contrasts", g.get("contrasts").cloned());
    put("audit", or_empty("audit"));
    put("paletteNotes", or_empty("paletteNotes"));
    put("fonts", font_set.clone());
    put("fontsVariant", Some(json!(fonts)));
    write_json(
        &sys.join("style.json"),
        &js::key_order(Value::Object(style)),
    )?;
    let id = text(g.get("id"));
    let design_md = root.join("DESIGN.md");
    let rationale = strings(g.get("rationale"));
    if design_md.exists() && !rationale.is_empty() {
        seed_rationale(&design_md, &id, &rationale)?;
    }
    Ok(AppliedStyle {
        id,
        fonts: font_set.unwrap_or(Value::Null),
    })
}

/// The personality axes of a guide and a brief, each from 1 (first word) to 5 (second word).
pub const AXES: [(&str, [&str; 2]); 7] = [
    ("formality", ["casual", "formal"]),
    ("energy", ["calm", "energetic"]),
    ("era", ["traditional", "modern"]),
    ("warmth", ["cool", "warm"]),
    ("density", ["airy", "dense"]),
    ("richness", ["minimal", "ornamented"]),
    ("seriousness", ["playful", "serious"]),
];

/// `matchStyles`: guides ranked against a brief's personality (1-5 per axis) and constraints, as
/// `{ id, name, fit, matches, mismatches, notes, concept }`, best fit first.
pub fn match_styles(brief: &Value) -> Result<Vec<Value>, DfwError> {
    let want = brief.get("personality");
    let weights = brief.get("weights");
    let mut ranked: Vec<(f64, Value)> = Vec::new();
    for g in list_styles()? {
        let axes = g.get("axes");
        let tags = strings(g.get("tags"));
        let mut dist = 0.0;
        let mut total = 0.0;
        let mut diffs = Vec::new();
        for (axis, words) in AXES {
            let (Some(wanted), Some(has)) = (
                js::present(want.and_then(|w| w.get(axis))),
                js::present(axes.and_then(|a| a.get(axis))),
            ) else {
                continue;
            };
            let weight = js::present(weights.and_then(|w| w.get(axis)))
                .map_or(1.0, |w| js::to_number(Some(w)));
            let (w, h) = (js::to_number(Some(wanted)), js::to_number(Some(has)));
            let d = (w - h).abs();
            dist += d * d * weight;
            total += 16.0 * weight;
            if d >= 2.0 {
                diffs.push(json!(format!(
                    "{axis} {} vs wanted {} (more {})",
                    text(Some(has)),
                    text(Some(wanted)),
                    words[usize::from(h > w)]
                )));
            }
        }
        let mut fit = if total != 0.0 && !total.is_nan() {
            js::round(100.0 * (1.0 - (dist / total).sqrt()))
        } else {
            0.0
        };
        let mut notes = Vec::new();
        let dark = js::present(brief.get("constraints").and_then(|c| c.get("dark")));
        if let Some(dark) = dark {
            if js::truthy(g.get("dark")) != js::truthy(Some(dark)) {
                fit -= 25.0;
                notes.push(json!(if js::truthy(g.get("dark")) {
                    "dark-first style"
                } else {
                    "light-first style"
                }));
            }
        }
        let keywords: Vec<Value> = list(brief.get("keywords"))
            .iter()
            .filter(|k| tags.contains(&text(Some(k)).to_lowercase()))
            .cloned()
            .collect();
        fit = js::min(100.0, fit + keywords.len() as f64 * 3.0);
        let mut hit = Map::new();
        for (key, value) in [
            ("id", g.get("id").cloned()),
            ("name", g.get("name").cloned()),
            ("fit", Some(js::num(fit))),
            ("matches", Some(Value::Array(keywords))),
            ("mismatches", Some(Value::Array(diffs))),
            ("notes", Some(Value::Array(notes))),
            ("concept", g.get("concept").cloned()),
        ] {
            if let Some(value) = value {
                hit.insert(key.into(), value);
            }
        }
        ranked.push((fit, Value::Object(hit)));
    }
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    Ok(ranked.into_iter().map(|r| r.1).collect())
}

/// `formatStyle`: a guide as the text `dfw style show` prints.
pub fn format_style(g: &Value) -> String {
    let get = |path: &[&str]| {
        let mut v = Some(g);
        for key in path {
            v = v.and_then(|v| v.get(key));
        }
        v
    };
    let t = |path: &[&str]| text(get(path));
    let joined = |path: &[&str], sep: &str| js::join(list(get(path)), sep);
    let avoid = list(get(&["avoid"]));
    let latin = get(&["fontsLatin"]).filter(|f| js::truthy(Some(f)));
    let radius: Vec<String> = entries(get(&["radius"]))
        .iter()
        .map(|(_, v)| text(Some(v)))
        .collect();
    let axes: Vec<String> = entries(get(&["axes"]))
        .iter()
        .map(|(k, v)| format!("{k} {}/5", text(Some(v))))
        .collect();
    let mut lines = vec![
        format!("{} ({})", t(&["name"]), t(&["id"])),
        format!("Concept: {}", t(&["concept"])),
        format!("Fits: {}{}", joined(&["fits"], "; "), if avoid.is_empty() { String::new() } else { format!(" · Avoid: {}", js::join(avoid, "; ")) }),
        format!("Tags: {}", joined(&["tags"], ", ")),
        format!(
            "Fonts (VI): display {}, body {}, mono {}{}",
            t(&["fonts", "display"]),
            t(&["fonts", "body"]),
            t(&["fonts", "mono"]),
            latin.map_or(String::new(), |l| format!(" · English-only alt: {} / {}", text(l.get("display")), text(l.get("body"))))
        ),
        format!(
            "Type: base {}, UI ratio {}, display ratio {} -> display {} px · radius {} · density {}{}",
            t(&["type", "base"]),
            t(&["type", "ratio"]),
            t(&["type", "displayRatio"]),
            joined(&["type", "display"], " / "),
            radius.join("/"),
            t(&["density"]),
            if js::truthy(get(&["dark"])) { " · dark" } else { "" }
        ),
        format!("Personality: {}", axes.join(", ")),
        "Why (senior rationale):".to_string(),
    ];
    lines.extend(
        list(get(&["rationale"]))
            .iter()
            .map(|r| format!("  - {}", text(Some(r)))),
    );
    lines.push("Palette (OKLCH ramps; role steps):".to_string());
    for (k, v) in entries(get(&["roles"])) {
        let v = text(Some(v));
        let shown = if v.starts_with('#') {
            v
        } else {
            let hex = get(&["palette", &v, "hex"])
                .filter(|h| js::truthy(Some(h)))
                .map_or(String::new(), |h| text(Some(h)));
            format!("{v} {hex}")
        };
        lines.push(format!("  {} {shown}", js::pad_end(k, 14)));
    }
    lines.push(format!(
        "Roles: accent={}, ink={}, canvas={}, brand={}, signature={}",
        t(&["roles", "accent"]),
        t(&["roles", "ink"]),
        t(&["roles", "canvas"]),
        t(&["roles", "brand"]),
        t(&["roles", "signature"])
    ));
    lines.push("Motifs:".to_string());
    lines.extend(
        list(get(&["motifs"]))
            .iter()
            .map(|m| format!("  {}: {}", text(m.get("name")), text(m.get("use")))),
    );
    lines.push(format!("Layout: {}", t(&["layout"])));
    lines.push("Components:".to_string());
    lines.extend(
        list(get(&["components"]))
            .iter()
            .map(|c| format!("  - {}", text(Some(c)))),
    );
    lines.push(format!("Do: {}", joined(&["do"], " · ")));
    lines.push(format!("Don't: {}", joined(&["dont"], " · ")));
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use crate::design::color::{hex_to_oklch, hue_distance};

    /// style library: every guide has rationale, axes, a generated palette and no MUST audit
    /// (tests/unit/color.test.mjs)
    #[test]
    fn every_guide_has_rationale_axes_palette_and_no_must_audit() {
        let guides = list_styles().unwrap();
        assert_eq!(guides.len(), 12);
        for g in guides {
            let id = g["id"].as_str().unwrap();
            assert!(g["rationale"].as_array().unwrap().len() >= 3, "{id}");
            assert_eq!(g["axes"].as_object().unwrap().len(), 7, "{id}");
            assert!(g["palette"].as_object().unwrap().len() >= 22, "{id}");
            assert!(
                !g["audit"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a["level"] == "error"),
                "{id}"
            );
            let accent = g["roles"]["accent"].as_str().unwrap();
            let acc = if accent.starts_with('#') {
                accent
            } else {
                g["palette"][accent]["hex"].as_str().unwrap()
            };
            let oc = hex_to_oklch(acc);
            if oc.c > 0.05 {
                for s in ["err", "ok"] {
                    assert!(
                        hue_distance(oc.h, hex_to_oklch(g["status"][s].as_str().unwrap()).h)
                            >= 25.0,
                        "{id} accent vs {s}"
                    );
                }
            }
        }
    }

    /// style match ranks by personality with explanations (tests/unit/color.test.mjs)
    #[test]
    fn match_ranks_by_personality_with_explanations() {
        let r = match_styles(&json!({ "personality": { "formality": 1, "energy": 5, "era": 5, "warmth": 4, "density": 2, "richness": 4, "seriousness": 1 } })).unwrap();
        assert_eq!(r[0]["id"], "playful-bold");
        let ops = match_styles(&json!({ "personality": { "formality": 3, "energy": 2, "era": 4, "warmth": 2, "density": 5, "richness": 1, "seriousness": 4 }, "constraints": { "dark": false } })).unwrap();
        assert!(["ops-dense", "swiss-grid", "industrial-utility"]
            .contains(&ops[0]["id"].as_str().unwrap()));
        assert!(!ops.last().unwrap()["mismatches"]
            .as_array()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn search_show_and_latin_fonts() {
        assert_eq!(
            search_styles("factory counter internal").unwrap()[0]["id"],
            "industrial-utility"
        );
        let latin = style_tokens(&get_style("editorial-luxe").unwrap(), "latin").unwrap();
        assert_eq!(
            latin["font"]["family"]["display"]["$value"][0],
            "Bodoni Moda"
        );
        let shown = format_style(&get_style("swiss-grid").unwrap());
        assert!(
            shown.starts_with("Swiss grid (swiss-grid)\nConcept: International typographic style"),
            "{shown}"
        );
        assert!(get_style("nope")
            .unwrap_err()
            .message()
            .contains("Unknown style guide \"nope\""));
        assert!(is_style("swiss-grid") && is_style("x.json") && !is_style("crisp"));
    }

    #[test]
    fn rationale_is_seeded_once_and_never_over_the_users_text() {
        let dir = test_dir("style-md");
        let md = dir.join("DESIGN.md");
        let seed = |text: &str, id: &str| {
            fs::write(&md, text).unwrap();
            seed_rationale(&md, id, &["one".to_string(), "two".to_string()]).unwrap();
            fs::read_to_string(&md).unwrap()
        };
        let block = |id: &str| {
            format!("## Rationale\n<!-- Seeded from style guide \"{id}\". Rewrite each line for THIS product: decision - because <context fact or principle>. -->\n- one\n- two\n")
        };
        // No heading: appended.
        assert_eq!(
            seed("# Design\n\ntext\n\n", "a"),
            format!("# Design\n\ntext\n\n{}", block("a"))
        );
        // An empty heading (with the template's comment): filled.
        assert_eq!(
            seed("# D\n## Rationale\n<!-- why -->\n\n## Next\n", "a"),
            format!("# D\n{}\n## Next\n", block("a"))
        );
        // The untouched seed of another guide: replaced.
        let other = "# D\n## Rationale\n<!-- Seeded from style guide \"b\". Rewrite -->\n- old\n- older\n\n## Next\n";
        assert_eq!(seed(other, "a"), format!("# D\n{}\n## Next\n", block("a")));
        // The user's own lines: kept.
        let own = "## Rationale\n- mine\n";
        assert_eq!(seed(own, "a"), own);
    }
}
