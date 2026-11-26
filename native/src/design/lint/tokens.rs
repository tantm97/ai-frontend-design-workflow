//! `lintTokens` of lint.mjs: a DTCG token tree.

use serde_json::{Map, Value};

use super::slop::ai_default_color;
use super::{finding, rule};
use crate::core::js;
use crate::core::js::regex::js_re;
use crate::core::level::Level;
use crate::design::color::{hex_to_oklch, Oklch};
use crate::design::tokens::key as dtcg_key;

/// The `$type` of a colour token.
pub(crate) use crate::design::tokens::dtcg::COLOR as COLOR_TYPE;

js_re!(ALIAS, r"\{([^{}]+)\}");
js_re!(HEX6, "^#[0-9a-f]{6}$", "i");

/// A token: an object with `$value`, under its dotted name.
struct Leaf<'a> {
    name: String,
    node: &'a Map<String, Value>,
    is_color: bool,
}

/// `Object.entries(node)` for an object or an array (whose keys are its indexes).
fn entries(node: &Value) -> Vec<(String, &Value)> {
    match node {
        Value::Object(map) => js::entries(map)
            .into_iter()
            .map(|(k, v)| (k.clone(), v))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    }
}

/// Every string inside a token value, in JavaScript's order.
fn strings<'a>(value: Option<&'a Value>, out: &mut Vec<&'a str>) {
    match value {
        Some(Value::String(s)) => out.push(s),
        Some(v @ (Value::Array(_) | Value::Object(_))) => {
            for (_, item) in entries(v) {
                strings(Some(item), out);
            }
        }
        _ => {}
    }
}

/// `Math.hypot(a, b, c)` as V8 computes it: scaled by the largest value, summed with compensation.
fn hypot3(values: [f64; 3]) -> f64 {
    let values = values.map(f64::abs);
    let max = values.iter().fold(0.0_f64, |m, v| m.max(*v));
    if max == 0.0 || max.is_nan() {
        return max;
    }
    let (mut sum, mut compensation) = (0.0, 0.0);
    for v in values {
        let n = v / max;
        let summand = n * n - compensation;
        let preliminary = sum + summand;
        compensation = (preliminary - sum) - summand;
        sum = preliminary;
    }
    sum.sqrt() * max
}

/// `AI_DEFAULT_COLORS[key]` on a plain JavaScript object: besides the colours, the two
/// lower-case names every object inherits answer too (a Node bug, kept: a token whose value is
/// the text "constructor" or "__proto__" is reported as an AI default colour).
fn ai_default_lookup(key: &str) -> Option<&'static str> {
    match key {
        "constructor" => Some("function Object() { [native code] }"),
        "__proto__" => Some("[object Object]"),
        _ => ai_default_color(key),
    }
}

/// Collect the tokens under `node`; report tokens without a type and names that cannot be
/// written in an alias.
fn walk<'a>(
    node: &'a Value,
    trail: &mut Vec<String>,
    inherited: Option<&'a Value>,
    rel: &str,
    leaves: &mut Vec<Leaf<'a>>,
    out: &mut Vec<Value>,
) {
    let own = node.get(dtcg_key::TYPE).filter(|t| js::truthy(Some(t)));
    let kind = own.or(inherited);
    if let Value::Object(map) = node {
        if map.contains_key(dtcg_key::VALUE) {
            let name = trail.join(".");
            let leaf = Leaf {
                name: name.clone(),
                node: map,
                is_color: kind.and_then(Value::as_str) == Some(COLOR_TYPE),
            };
            // A Map: a name seen again keeps its place and takes the new token.
            match leaves.iter_mut().find(|l| l.name == name) {
                Some(slot) => *slot = leaf,
                None => leaves.push(leaf),
            }
            if kind.is_none() {
                out.push(finding(
                    Level::Warn.as_str(),
                    "token-type",
                    rel,
                    1,
                    format!("Token \"{name}\" has no $type (own or inherited)."),
                ));
            }
            return;
        }
    }
    for (k, v) in entries(node) {
        if k.starts_with('$') {
            continue;
        }
        if k.contains(['{', '}', '.']) {
            out.push(finding(
                Level::Error.as_str(),
                "token-name",
                rel,
                1,
                format!("Token/group name \"{k}\" must not contain \"{{\", \"}}\" or \".\"."),
            ));
        }
        trail.push(k);
        walk(v, trail, kind, rel, leaves, out);
        trail.pop();
    }
}

/// The names of the tokens under `node`.
fn names(node: &Value, trail: &mut Vec<String>, known: &mut Vec<String>) {
    if node
        .as_object()
        .is_some_and(|m| m.contains_key(dtcg_key::VALUE))
    {
        known.push(trail.join("."));
        return;
    }
    for (k, v) in entries(node) {
        if !k.starts_with('$') {
            trail.push(k);
            names(v, trail, known);
            trail.pop();
        }
    }
}

/// `lintTokens(rel, json, base)`: every leaf has `$value` and a type; aliases resolve (for a
/// theme, also against the `base` tree it overrides); no near-duplicate or AI-default colours.
pub fn lint_tokens(rel: &str, json: &Value, base: Option<&Value>) -> Vec<Value> {
    let mut out = Vec::new();
    let mut leaves: Vec<Leaf> = Vec::new();
    walk(json, &mut Vec::new(), None, rel, &mut leaves, &mut out);

    let mut known: Vec<String> = leaves.iter().map(|l| l.name.clone()).collect();
    if let Some(base) = base {
        names(base, &mut Vec::new(), &mut known);
    }

    for leaf in &leaves {
        let mut texts = Vec::new();
        strings(leaf.node.get(dtcg_key::VALUE), &mut texts);
        for text in texts {
            for m in ALIAS.captures_iter(text) {
                if !known.iter().any(|k| k == &m[1]) {
                    out.push(finding(
                        Level::Error.as_str(),
                        "token-alias",
                        rel,
                        1,
                        format!(
                            "Token \"{}\" references unknown token \"{{{}}}\".",
                            leaf.name, &m[1]
                        ),
                    ));
                }
            }
        }
    }

    // Near-duplicate colours: two tokens that look the same are one decision made twice.
    let colors: Vec<(&str, String, Oklch)> = leaves
        .iter()
        .filter(|l| l.is_color)
        .filter_map(|l| {
            let hex = match l.node.get(dtcg_key::VALUE) {
                Some(Value::String(s)) => Some(s.as_str()),
                Some(Value::Object(o)) => o.get(dtcg_key::HEX).and_then(Value::as_str),
                _ => None,
            }?;
            HEX6.is_match(hex)
                .then(|| (l.name.as_str(), hex.to_ascii_lowercase(), hex_to_oklch(hex)))
        })
        .collect();
    let mut reported: Vec<&str> = Vec::new();
    for (i, a) in colors.iter().enumerate() {
        for b in &colors[i + 1..] {
            // Identical values are usually deliberate aliases.
            if a.1 == b.1 || reported.contains(&b.0) {
                continue;
            }
            let hue = (a.2.h - b.2.h).abs();
            let dh = hue.min(360.0 - hue) * a.2.c.min(b.2.c) * 0.0175;
            if hypot3([a.2.l - b.2.l, a.2.c - b.2.c, dh]) < 0.005 {
                reported.push(b.0);
                out.push(finding(Level::Warn.as_str(), "near-duplicate-color", rel, 1, format!("Tokens \"{}\" ({}) and \"{}\" ({}) are visually the same colour. Merge them or alias one to the other.", a.0, a.1, b.0, b.1)));
            }
        }
    }
    for leaf in &leaves {
        let mut texts = Vec::new();
        strings(leaf.node.get(dtcg_key::VALUE), &mut texts);
        for s in texts {
            if let Some(what) = ai_default_lookup(&js::trim(s).to_lowercase()) {
                out.push(finding(Level::Warn.as_str(), "ai-default-color", rel, 1, format!("Token \"{}\" is {s} ({what}), a colour AI-generated UIs default to. Derive the palette from the brand (dfw palette build); keep it only if the brand requires it (dfw decide).", leaf.name)));
            }
        }
    }
    if leaves.is_empty() {
        out.push(finding(
            Level::Error.as_str(),
            rule::TOKEN_EMPTY,
            rel,
            1,
            "Token file has no tokens.".into(),
        ));
    }
    out
}

/// Every constant expression of this file, for the test that compiles them.
#[cfg(test)]
pub(super) fn patterns() -> Vec<&'static regex::Regex> {
    vec![&ALIAS, &HEX6]
}
