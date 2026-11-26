//! Ports `core/lib/extract.mjs` ("dfw tokens extract --url"): a DTCG token seed from a reference
//! site or the existing app, read from computed styles in headless Chrome. It is a starting
//! point for the design system brief, never the design system itself: it writes its own file
//! and changes nothing else.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::launch::{fonts_ready, open_renderer_with, LaunchOptions};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{rel, resolve};
use crate::core::util::{ensure_inside, now_iso, to_json_pretty, write_atomic};
use crate::design::color::{hex_to_oklch, hex_to_rgb, Oklch};
use crate::design::tokens::{relative_path, TYPE_KEY};

/// `collect` of extract.mjs, byte for byte: it runs in the page and counts style values over
/// visible elements (backgrounds weighted by area).
const COLLECT: &str = include_str!("extract_collect.js");

/// The keys of what `COLLECT` returns: a bag per kind of value, each value counted as
/// `{ n, roles: { <role>: true } }`.
mod counted {
    pub const COLORS: &str = "colors";
    pub const FAMILIES: &str = "families";
    pub const SIZES: &str = "sizes";
    pub const RADII: &str = "radii";
    pub const SPACES: &str = "spaces";
    /// How often a value was seen (backgrounds weighted by area).
    pub const N: &str = "n";
    pub const ROLES: &str = "roles";
}

/// The keys of the token seed (DTCG): its groups, the token words and the extension each token
/// carries. The DTCG words are named here until `design::tokens` names them.
mod key {
    pub const DESCRIPTION: &str = "$description";
    pub const VALUE: &str = "$value";
    pub const EXTENSIONS: &str = "$extensions";
    /// The extension of an extracted token: `{ uses, roles? }`.
    pub const EXTRACTED: &str = "dfw.extracted";
    pub const USES: &str = "uses";
    /// The roles a colour was seen in, as a list.
    pub const ROLES: &str = "roles";

    /* --------------------------------------------------------------------- the groups */

    pub const COLOR: &str = "color";
    pub const FONT: &str = "font";
    pub const FAMILY: &str = "family";
    pub const SIZE: &str = "size";
    pub const RADIUS: &str = "radius";
    pub const SPACE: &str = "space";

    /* --------------------------------------------------------------------- the values */

    /// A colour value: `{ colorSpace, components, hex }`.
    pub const COLOR_SPACE: &str = "colorSpace";
    pub const COMPONENTS: &str = "components";
    pub const HEX: &str = "hex";
    /// A dimension value: `{ value, unit }`.
    pub const DIMENSION_VALUE: &str = "value";
    pub const UNIT: &str = "unit";
}

/// One counted value: how often it was seen and in which roles.
#[derive(Debug, Clone, PartialEq)]
struct Counted {
    value: String,
    n: f64,
    roles: Vec<String>,
}

/// `top(bag, n)`: the `n` most used values of a bag, ties in the order the page saw them.
fn top(bag: Option<&Value>, n: usize) -> Vec<Counted> {
    let mut all: Vec<Counted> = bag
        .and_then(Value::as_object)
        .map(js::entries)
        .unwrap_or_default()
        .into_iter()
        .map(|(value, v)| Counted {
            value: value.clone(),
            n: v.get(counted::N)
                .and_then(Value::as_f64)
                .unwrap_or(f64::NAN),
            roles: v
                .get(counted::ROLES)
                .and_then(Value::as_object)
                .map(|r| js::entries(r).into_iter().map(|(k, _)| k.clone()).collect())
                .unwrap_or_default(),
        })
        .collect();
    all.sort_by(|a, b| b.n.partial_cmp(&a.n).unwrap_or(std::cmp::Ordering::Equal));
    all.truncate(n);
    all
}

/// `px(v)`: a CSS length as a number with two decimals.
fn px(v: &str) -> f64 {
    js::round(js::parse_float(v) * 100.0) / 100.0
}

/// A group of colours that look the same.
struct Cluster {
    hex: String,
    c: Oklch,
    uses: f64,
    roles: Vec<String>,
}

/// `clusterColors(bag, max)`: merge colours that look the same (OKLCH distance), keeping the
/// most used hex of each group.
fn cluster_colors(bag: Option<&Value>, max: usize) -> Vec<Cluster> {
    let mut kept: Vec<Cluster> = Vec::new();
    for counted in top(bag, 60) {
        let c = hex_to_oklch(&counted.value);
        let near = |k: &Cluster| {
            let dh = (k.c.h - c.h).abs();
            let hue = (dh.min(360.0 - dh) / 360.0) * k.c.c.min(c.c);
            ((k.c.l - c.l).powi(2) + (k.c.c - c.c).powi(2) + hue.powi(2)).sqrt() < 0.02
        };
        match kept.iter_mut().find(|k| near(k)) {
            Some(twin) => {
                twin.uses += counted.n;
                for role in counted.roles {
                    if !twin.roles.contains(&role) {
                        twin.roles.push(role);
                    }
                }
            }
            None => kept.push(Cluster {
                hex: counted.value,
                c,
                uses: counted.n,
                roles: counted.roles,
            }),
        }
    }
    kept.sort_by(|a, b| {
        b.uses
            .partial_cmp(&a.uses)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    kept.truncate(max);
    kept
}

/// `{ $extensions: { 'dfw.extracted': { uses, ...extra } } }`, added to a token.
fn with_source(mut token: Map<String, Value>, uses: f64, roles: Option<&[String]>) -> Value {
    let mut extracted = Map::new();
    extracted.insert(key::USES.into(), js::num(js::round(uses)));
    if let Some(roles) = roles {
        extracted.insert(key::ROLES.into(), json!(roles));
    }
    token.insert(
        key::EXTENSIONS.into(),
        json!({ (key::EXTRACTED): extracted }),
    );
    Value::Object(token)
}

fn token(value: Value) -> Map<String, Value> {
    let mut token = Map::new();
    token.insert(key::VALUE.into(), value);
    token
}

fn group(kind: &str) -> Map<String, Value> {
    let mut group = Map::new();
    group.insert(TYPE_KEY.into(), json!(kind));
    group
}

/// `dim(bag, n, prefix)`: the `n` most used lengths of a bag as dimension tokens, smallest
/// first.
fn dim(bag: Option<&Value>, n: usize, prefix: &str) -> Value {
    let mut uses: Vec<(f64, f64)> = Vec::new();
    for counted in top(bag, 40) {
        let v = px(&counted.value);
        if v > 0.0 {
            match uses.iter_mut().find(|(known, _)| *known == v) {
                Some((_, sum)) => *sum += counted.n,
                None => uses.push((v, counted.n)),
            }
        }
    }
    uses.truncate(n);
    uses.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
    let mut g = group("dimension");
    for (i, (v, n)) in uses.iter().enumerate() {
        let value = json!({ (key::DIMENSION_VALUE): js::num(*v), (key::UNIT): "px" });
        g.insert(
            format!("{prefix}{}", i + 1),
            with_source(token(value), *n, None),
        );
    }
    Value::Object(g)
}

/// The tokens for what the page counted (the part of `extractTokens` after the render).
fn tokens_from(url: &str, data: &Value) -> Value {
    let mut color = group("color");
    for (i, k) in cluster_colors(data.get(counted::COLORS), 10)
        .iter()
        .enumerate()
    {
        let components: Vec<Value> = hex_to_rgb(&k.hex)
            .iter()
            .map(|x| js::num(js::round(x * 10000.0) / 10000.0))
            .collect();
        let value =
            json!({ (key::COLOR_SPACE): "srgb", (key::COMPONENTS): components, (key::HEX): k.hex });
        color.insert(
            format!("c{:02}", i + 1),
            with_source(token(value), k.uses, Some(&k.roles)),
        );
    }
    let mut family = group("fontFamily");
    for (f, name) in
        top(data.get(counted::FAMILIES), 3)
            .iter()
            .zip(["primary", "secondary", "tertiary"])
    {
        let unquote = |font: &str| {
            let font = js::trim(font);
            let font = font.strip_prefix(['"', '\'']).unwrap_or(font);
            font.strip_suffix(['"', '\'']).unwrap_or(font).to_string()
        };
        let stack: Vec<String> = f.value.split(',').map(unquote).collect();
        family.insert(name.into(), with_source(token(json!(stack)), f.n, None));
    }
    let description = format!("Extracted from {url} on {} by \"dfw tokens extract\". A seed for the design system brief, not the design system: name the roles, merge, check contrast (dfw system audit), then build tokens.json with dfw palette build / dfw type scale.", &now_iso()[..10]);
    json!({
        (key::DESCRIPTION): description,
        (key::COLOR): color,
        (key::FONT): { (key::FAMILY): family, (key::SIZE): dim(data.get(counted::SIZES), 10, "s") },
        (key::RADIUS): dim(data.get(counted::RADII), 5, "r"),
        (key::SPACE): dim(data.get(counted::SPACES), 10, "sp"),
    })
}

/// `extractTokens(url)`: render the page at 1440x900 and build the token seed.
pub fn extract_tokens(url: &str) -> Result<Value, DfwError> {
    extract_tokens_with(url, &LaunchOptions::default())
}

/// [`extract_tokens`] with launch options for the browser.
pub fn extract_tokens_with(url: &str, launch: &LaunchOptions) -> Result<Value, DfwError> {
    let renderer = open_renderer_with(launch)?;
    let data = (|| {
        let page = renderer.page(1440, 900, 1.0)?;
        page.goto(url, 45_000)?;
        fonts_ready(&page, 3000)?;
        page.evaluate(COLLECT.trim_end(), None)
    })();
    renderer.close();
    Ok(tokens_from(url, &data?))
}

/// `extractTokensToFile(root, url, out)`: write the seed (default
/// `design/system/extracted.tokens.json`) and return the line to print.
pub fn extract_tokens_to_file(
    root: &Path,
    url: &str,
    out: Option<&str>,
) -> Result<String, DfwError> {
    // As in Node, `--out` may name any file the user can write, also outside the project. The
    // default is a project file: a link must not send it out (checked before the browser runs).
    let file = resolve(root.join(out.unwrap_or(rel::EXTRACTED_TOKENS)));
    if out.is_none() {
        ensure_inside(root, &file)?;
    }
    let tokens = extract_tokens(url)?;
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    write_atomic(&file, to_json_pretty(&tokens) + "\n")?;
    let n = |path: &[&str]| {
        let group = path.iter().try_fold(&tokens, |v, key| v.get(key));
        group
            .and_then(Value::as_object)
            .map_or(0, |g| g.keys().filter(|k| !k.starts_with('$')).count())
    };
    Ok(format!(
        "Wrote {}: {} colours, {} font families, {} font sizes, {} radii, {} spacing values. It is a seed: review it with the user before using any of it.",
        relative_path(root, &file),
        n(&[key::COLOR]),
        n(&[key::FONT, key::FAMILY]),
        n(&[key::FONT, key::SIZE]),
        n(&[key::RADIUS]),
        n(&[key::SPACE])
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The JavaScript that runs in the page is one self-contained function.
    #[test]
    fn page_script_is_one_function() {
        assert!(COLLECT.starts_with("function collect() {\n"));
        assert!(COLLECT.trim_end().ends_with('}'));
    }

    #[test]
    fn tokens_are_built_from_the_counts_like_node() {
        // Counts as `collect` returns them; the expected values are worked out by hand from
        // extract.mjs (render.golden.mjs checks it on a real page).
        let data = json!({
            "colors": {
                "#ffffff": { "n": 52.5, "roles": { "background": 1 } },
                "#111111": { "n": 4, "roles": { "text": 1 } },
                "#fefefe": { "n": 3, "roles": { "border": 1, "background": 1 } },
                "#0d6b78": { "n": 6.4, "roles": { "background": 1, "text": 1 } },
            },
            "families": { "\"Inter Tight\", system-ui, 'Segoe UI'": { "n": 5, "roles": {} }, "monospace": { "n": 1, "roles": {} } },
            "sizes": { "16px": { "n": 4, "roles": {} }, "12.8px": { "n": 4, "roles": {} }, "24px": { "n": 1, "roles": {} } },
            "radii": { "6px": { "n": 2, "roles": {} }, "5.999px": { "n": 1, "roles": {} }, "50%": { "n": 1, "roles": {} } },
            "spaces": { "normal": { "n": 9, "roles": {} }, "8px": { "n": 3, "roles": {} } },
        });
        let tokens = tokens_from("http://localhost:1/x", &data);
        let text = to_json_pretty(&tokens);
        let color = tokens["color"].as_object().unwrap();
        // The colours, most used first.
        const FIRST: &str = "c01";
        const SECOND: &str = "c02";
        assert_eq!(
            color.keys().collect::<Vec<_>>(),
            [TYPE_KEY, FIRST, SECOND, "c03"]
        );
        assert_eq!(
            crate::core::util::to_json_compact(&color[FIRST]),
            r##"{"$value":{"colorSpace":"srgb","components":[1,1,1],"hex":"#ffffff"},"$extensions":{"dfw.extracted":{"uses":56,"roles":["background","border"]}}}"##
        );
        assert_eq!(
            crate::core::util::to_json_compact(&color[SECOND]["$value"]),
            r##"{"colorSpace":"srgb","components":[0.051,0.4196,0.4706],"hex":"#0d6b78"}"##
        );
        assert_eq!(
            crate::core::util::to_json_compact(&tokens["font"]),
            r#"{"family":{"$type":"fontFamily","primary":{"$value":["Inter Tight","system-ui","Segoe UI"],"$extensions":{"dfw.extracted":{"uses":5}}},"secondary":{"$value":["monospace"],"$extensions":{"dfw.extracted":{"uses":1}}}},"size":{"$type":"dimension","s1":{"$value":{"value":12.8,"unit":"px"},"$extensions":{"dfw.extracted":{"uses":4}}},"s2":{"$value":{"value":16,"unit":"px"},"$extensions":{"dfw.extracted":{"uses":4}}},"s3":{"$value":{"value":24,"unit":"px"},"$extensions":{"dfw.extracted":{"uses":1}}}}}"#
        );
        assert_eq!(
            crate::core::util::to_json_compact(&tokens["radius"]),
            r#"{"$type":"dimension","r1":{"$value":{"value":6,"unit":"px"},"$extensions":{"dfw.extracted":{"uses":3}}},"r2":{"$value":{"value":50,"unit":"px"},"$extensions":{"dfw.extracted":{"uses":1}}}}"#
        );
        assert_eq!(
            tokens["space"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            [TYPE_KEY, "sp1"]
        );
        assert!(
            text.starts_with("{\n  \"$description\": \"Extracted from http://localhost:1/x on ")
        );
        assert!(tokens["$description"]
            .as_str()
            .unwrap()
            .ends_with("then build tokens.json with dfw palette build / dfw type scale."));
    }

    /// The default seed file is the project's: `design/system` linked out of the project is
    /// refused before any browser starts (review CO-3).
    #[cfg(unix)]
    #[test]
    fn the_default_seed_file_is_not_written_through_a_link_out() {
        let base = crate::core::util::test_dir("extract-link-out");
        let root = base.join("app");
        fs::create_dir_all(root.join(rel::DESIGN)).unwrap();
        let victim = base.join("victim");
        fs::create_dir_all(&victim).unwrap();
        std::os::unix::fs::symlink("../../victim", root.join(rel::SYSTEM)).unwrap();
        let err = extract_tokens_to_file(&root, "http://127.0.0.1:9/", None).unwrap_err();
        assert!(
            err.message().contains("outside the project"),
            "{}",
            err.message()
        );
        assert!(fs::read_dir(&victim).unwrap().next().is_none());
        fs::remove_dir_all(&base).unwrap();
    }
}
