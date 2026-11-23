//! Ports `diffChanges`, `diffComponents`, `dimensionTokenIndex`, `masterSource` and the
//! `describe*` helpers of `core/lib/penpot.mjs`.
//!
//! The plugin sends back what the designer changed (texts, boxes, fills, added layers, comment
//! threads), on the screen boards and on the main components of the "dfw components" page.
//! Here that change file is compared with the export the boards were built from. Pure: no file
//! is written, and the change file is untrusted (every read may be missing or of another type).
//!
//! This file holds the readers of untrusted layers every part shares; one file per job:
//! `boards`, `masters`, `source` (where a master change lands), `sentences`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};

use super::export::{resolve_alias, CHANGES_SCHEMA};
use super::tokens::flatten_tokens;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::rel;
use crate::design::compose::{load_components, Library};

/// Positions and sizes within this many px are the same (Penpot rounds, fonts differ).
const TOL: f64 = 1.5;

type Pairs = Vec<(&'static str, Option<Value>)>;

/// A list out of untrusted JSON: anything that is not an array is empty.
pub(super) fn list(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Refuse a list the Node code could not walk: there it threw a `TypeError` before anything
/// was written, so a malformed change file stops here too. `reads` is the property read of
/// every entry (a `null` entry has none); `method` is for a list used through an array method,
/// which a string does not have. A string walked with `for...of` is no list here.
pub(super) fn walkable(
    v: Option<&Value>,
    reads: Option<&str>,
    method: bool,
) -> Result<(), DfwError> {
    match js::truthy_value(v) {
        None => Ok(()),
        Some(Value::String(_)) if !method => Ok(()),
        Some(Value::Array(items)) => match reads.filter(|_| items.iter().any(Value::is_null)) {
            Some(property) => Err(DfwError::new(format!(
                "The change file has an empty entry (null) in a list of items with \"{property}\". Send the changes again from the dfw bridge plugin, or remove the null entry."
            ))),
            None => Ok(()),
        },
        Some(other) => Err(DfwError::new(format!(
            "The change file has {} where a list was expected. Send the changes again from the dfw bridge plugin, or fix the file.",
            js::slice16(&js::stringify(Some(other)), 0, 60)
        ))),
    }
}

fn kind(l: &Value) -> Option<&str> {
    l.get("kind").and_then(Value::as_str)
}

fn is_text(l: &Value) -> bool {
    kind(l) == Some("text")
}

/// `value ?? fallback` for an opacity or a width that defaults to 1.
fn or_one(v: Option<&Value>) -> Value {
    js::present(v).cloned().unwrap_or_else(|| json!(1))
}

/// `value?.[0]` of what the plugin sent, when it is truthy: the first item of a list, and, as
/// in JavaScript, the first character of a string.
fn first_of(v: Option<&Value>) -> Option<Value> {
    match v? {
        Value::Array(items) => items.first().filter(|x| js::truthy(Some(x))).cloned(),
        Value::String(s) => s.chars().next().map(|c| c.to_string().into()),
        _ => None,
    }
}

fn lower(v: Option<&Value>) -> String {
    js::to_string(v).to_lowercase()
}

/// `[l.x, l.y, l.w, l.h]`: a missing number is `null` in JSON.
fn bbox(l: &Value) -> Value {
    Value::Array(
        ["x", "y", "w", "h"]
            .iter()
            .map(|k| l.get(*k).cloned().unwrap_or(Value::Null))
            .collect(),
    )
}

/// The layers of a tree by path, parents before children (`flattenLayers`).
struct Layers<'a> {
    order: Vec<&'a Value>,
    index: HashMap<String, usize>,
}

impl<'a> Layers<'a> {
    fn of(layers: Option<&'a Value>) -> Self {
        fn walk<'a>(layers: Option<&'a Value>, out: &mut Layers<'a>) {
            for l in list(layers) {
                // The key keeps the type of the path: only the same string finds a layer.
                match out.index.get(&js::stringify(l.get("path"))) {
                    Some(i) => out.order[*i] = l,
                    None => {
                        out.index
                            .insert(js::stringify(l.get("path")), out.order.len());
                        out.order.push(l);
                    }
                }
                walk(l.get("children"), out);
            }
        }
        let mut out = Layers {
            order: Vec::new(),
            index: HashMap::new(),
        };
        walk(layers, &mut out);
        out
    }

    fn get(&self, path: Option<&Value>) -> Option<&'a Value> {
        self.index.get(&js::stringify(path)).map(|i| self.order[*i])
    }

    fn has(&self, path: Option<&Value>) -> bool {
        self.index.contains_key(&js::stringify(path))
    }
}

/// The path of the parent layer (`"0.1"` for `"0.1.3"`), `None` for a top layer.
fn parent_path(path: &str) -> Option<&str> {
    path.rfind('.').map(|i| &path[..i])
}

/// Whether `value + x` joins text in JavaScript instead of adding numbers.
fn joins(v: Option<&Value>) -> bool {
    matches!(
        v,
        Some(Value::String(_) | Value::Array(_) | Value::Object(_))
    )
}

/// `a + b` as a number: JavaScript joins the two as text when one of them is not a number
/// (a coordinate sent as `"20"`), and the comparison that follows reads the text as a number.
fn js_add(a: Option<&Value>, b: Option<&Value>) -> f64 {
    if joins(a) || joins(b) {
        js::str_to_number(&format!("{}{}", js::to_string(a), js::to_string(b)))
    } else {
        js::to_number(a) + js::to_number(b)
    }
}

fn contains(l: &Value, x: f64, y: f64) -> bool {
    let n = |k: &str| js::to_number(l.get(k));
    x >= n("x")
        && x <= js_add(l.get("x"), l.get("w"))
        && y >= n("y")
        && y <= js_add(l.get("y"), l.get("h"))
}

fn area(l: &Value) -> f64 {
    js::to_number(l.get("w")) * js::to_number(l.get("h"))
}

/// The smallest layer with an id (its own or its nearest ancestor's) under a point.
fn deepest_at<'a>(layers: impl Iterator<Item = &'a Value>, x: f64, y: f64) -> Option<&'a Value> {
    let mut best: Option<&Value> = None;
    for l in layers {
        if !(js::truthy(l.get("anchor")) || js::truthy(l.get("dfwId"))) {
            continue;
        }
        if contains(l, x, y) && best.is_none_or(|b| area(l) <= area(b)) {
            best = Some(l);
        }
    }
    best
}

/// The middle of a layer the designer added.
fn centre(a: &Value) -> (f64, f64) {
    let middle = |pos: &str, len: &str| {
        let half = js::truthy_value(a.get(len)).map_or(0.0, |v| js::to_number(Some(v))) / 2.0;
        if joins(a.get(pos)) {
            js::str_to_number(&format!(
                "{}{}",
                js::to_string(a.get(pos)),
                js::number(half)
            ))
        } else {
            js::to_number(a.get(pos)) + half
        }
    };
    (middle("x", "w"), middle("y", "h"))
}

/// `host?.anchor || host?.dfwId || null`.
fn anchor_of(host: Option<&Value>) -> Value {
    host.and_then(|h| js::truthy_value(h.get("anchor")).or(js::truthy_value(h.get("dfwId"))))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Replies the plugin posted for dfw (`[dfw] ...`) are not designer messages.
pub(super) fn is_dfw_reply(content: Option<&Value>) -> bool {
    let text = js::truthy_value(content).map_or(String::new(), |c| js::to_string(Some(c)));
    let start = text.trim_start_matches(js::is_space);
    start
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("[dfw]"))
}

/// The token of the export with exactly this colour.
fn color_token(baseline: &Value, c: &Value) -> Value {
    let hex = lower(c.get("color"));
    let opacity = js::to_number(Some(&or_one(c.get("opacity"))));
    list(baseline.get("colors"))
        .iter()
        .find(|t| {
            t.get("hex").and_then(Value::as_str) == Some(&hex)
                && (js::to_number(t.get("opacity")) - opacity).abs() < 0.01
        })
        .map_or(Value::Null, |t| js::or_null(t.get("name")))
}

/// Whether two paints differ: another colour, or an opacity more than 1% apart.
fn paint_differs(after: &Value, before_color: Option<&Value>, before: &Value) -> bool {
    let opacity = |v: &Value| js::to_number(Some(&or_one(v.get("opacity"))));
    lower(after.get("color")) != lower(before_color)
        || (opacity(after) - opacity(before)).abs() > 0.01
}

mod boards;
mod masters;
mod sentences;
mod source;

pub use boards::*;
pub use masters::*;
pub use sentences::*;
pub use source::*;

#[cfg(test)]
mod tests;
