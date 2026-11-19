//! Ports `core/lib/penpot-tokens.mjs`: token export to and import from the Penpot token format.
//!
//! Penpot's token file (single-file form, as written by "Export" in the Tokens panel) is a
//! Tokens Studio-style multi-set document: one key per set, `$themes` and `$metadata`. Values are
//! strings (`"16px"`, `"#0066ff"`, `"400"`) and the type names are Penpot's.
//! dfw mapping: `design/tokens/tokens.json` is set `base`, `design/tokens/themes/<name>.json` is
//! set `themes/<name>`, one Penpot theme per dfw theme in group `dfw`.
//!
//! Everything read from a Penpot file is untrusted: token and set names with a reserved path
//! segment are refused, and a theme name must match `[\w.-]+` before it becomes a file name.
//!
//! This file holds what every part shares (reserved names, patterns, the flattened token map);
//! one file per job: `to_penpot`, `from_penpot`, `read`, `merge`, `project` (the tests of the
//! whole module are in `tests.rs`).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::{to_json_compact, write_json, write_json_inside};
use crate::design::color::{oklch_to_hex, Oklch};
use crate::design::tokens::{dir_names, relative_path};

const BASE_SET: &str = "base";
const THEME_PREFIX: &str = "themes/";
const THEME_GROUP: &str = "dfw";

/// Token and set names come from a Penpot file (untrusted): in JavaScript a path segment like
/// `__proto__` would write into `Object.prototype` instead of the tree. Refused here too, so
/// both implementations accept the same names.
pub(super) const RESERVED: [&str; 3] = ["__proto__", "constructor", "prototype"];

/// The functions every JavaScript object inherits. The Node code read groups and themes with
/// `object[name]`, so one of these names found the built-in function where nothing was stored.
const INHERITED: [&str; 11] = [
    "constructor",
    "toString",
    "toLocaleString",
    "valueOf",
    "hasOwnProperty",
    "isPrototypeOf",
    "propertyIsEnumerable",
    "__defineGetter__",
    "__defineSetter__",
    "__lookupGetter__",
    "__lookupSetter__",
];

/// A pattern that is a constant of this file: it is known to compile (the tests of this module
/// run every one of them).
macro_rules! pattern {
    ($name:ident, $source:expr) => {
        #[allow(clippy::expect_used)]
        static $name: LazyLock<Regex> =
            LazyLock::new(|| Regex::new(&$source).expect("constant pattern"));
    };
}

pattern!(ALIAS, r"\{[^{}]+\}");
pattern!(PLAIN_ALIAS, r"^\{[^{}]+\}$");
pattern!(CALC, r"(?s)^calc\((.*)\)$");
pattern!(
    MATH,
    r"[-+*/%^]|(?-u:\b)(round|ceil|floor|min|max|abs|sqrt|pow|log|exp)\("
);
pattern!(DURATION_NAME, r"(^|\.)(duration|delay)(\.|$)");
pattern!(HEX_COLOR, r"(?i)^#([0-9a-f]{3,4}|[0-9a-f]{6}|[0-9a-f]{8})$");
pattern!(
    RGB_COLOR,
    format!(
        r"(?i)^rgba?\([{s}]*([0-9.]+)[{s},]+([0-9.]+)[{s},]+([0-9.]+)(?:[{s}]*[,/][{s}]*([0-9.]+%?))?[{s}]*\)$",
        s = js::SPACE
    )
);
pattern!(
    HSL_COLOR,
    format!(
        r"(?i)^hsla?\([{s}]*([0-9.]+)(?:deg)?[{s},]+([0-9.]+)%[{s},]+([0-9.]+)%(?:[{s}]*[,/][{s}]*([0-9.]+%?))?[{s}]*\)$",
        s = js::SPACE
    )
);
pattern!(
    DIMENSION,
    format!(
        r"(?i)^[{s}]*(-?[0-9]*\.?[0-9]+)[{s}]*(px|rem|em)?[{s}]*$",
        s = js::SPACE
    )
);
pattern!(
    NUMBER,
    format!(
        r"^[{s}]*(-?[0-9]*\.?[0-9]+)[{s}]*(%)?[{s}]*$",
        s = js::SPACE
    )
);
pattern!(THEME_SET, r"^themes?/([^\n\r\x{2028}\x{2029}]+)$");
pattern!(THEME_SET_PREFIX, r"^themes?/");
pattern!(THEME_NAME, r"^[0-9A-Za-z_.-]+$");
pattern!(FONT_SIZE, r"(^|\.)font\.size(\.|$)|(^|\.)font-?size(\.|$)");
pattern!(TRACKING, r"(^|\.)(tracking|letter-?spacing)(\.|$)");
pattern!(RADIUS, r"(^|\.)(radius|radii|rounded|corner)(\.|$)");
pattern!(
    BORDER_WIDTH,
    r"(^|\.)(border|stroke|outline|focus)\.(width|width-[0-9A-Za-z_-]+)$|(^|\.)(border-?width|stroke-?width)(\.|$)"
);
pattern!(
    SPACING,
    r"(^|\.)(space|spacing|gap|gutter|inset|padding|margin)(\.|$)"
);
pattern!(
    SIZING,
    r"(^|\.)(size|sizing|width|height|min-?width|max-?width)(\.|$)"
);

/// `"color.brand.500"` as its segments, or `None` when a segment is reserved.
fn token_path(name: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = name.split('.').collect();
    if parts.iter().any(|p| RESERVED.contains(p)) {
        None
    } else {
        Some(parts)
    }
}

fn is_alias(value: Option<&Value>) -> bool {
    value
        .and_then(Value::as_str)
        .is_some_and(|s| ALIAS.is_match(s))
}

/// `round(n)` of the Node file: four decimals.
fn round4(n: f64) -> f64 {
    js::round(n * 10000.0) / 10000.0
}

fn fmt(n: f64) -> String {
    js::number(round4(n))
}

fn canon(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canon).collect()),
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| js::cmp_utf16(a, b));
            Value::Object(
                keys.into_iter()
                    .map(|k| (k.clone(), canon(&map[k])))
                    .collect(),
            )
        }
        other => other.clone(),
    }
}

/// Equal when written as JSON with sorted keys (`1` and `1.0` are the same number).
fn same(a: &Value, b: &Value) -> bool {
    to_json_compact(&canon(a)) == to_json_compact(&canon(b))
}

/// One token of a flattened tree. `ty` and `description` are `None` where JavaScript had
/// `undefined`.
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub ty: Option<Value>,
    pub value: Value,
    pub description: Option<Value>,
}

impl Token {
    /// The type when it is a string (anything else matches no known type).
    pub fn type_name(&self) -> Option<&str> {
        self.ty.as_ref().and_then(Value::as_str)
    }
}

/// Tokens by dotted name, in document order (a JavaScript `Map`: setting a name again keeps
/// its place and replaces the token).
#[derive(Debug, Clone, Default)]
pub struct Flat {
    items: Vec<(String, Token)>,
    index: HashMap<String, usize>,
}

impl Flat {
    fn set(&mut self, name: String, token: Token) {
        match self.index.get(&name) {
            Some(i) => self.items[*i].1 = token,
            None => {
                self.index.insert(name.clone(), self.items.len());
                self.items.push((name, token));
            }
        }
    }

    /// The token with this name.
    pub fn get(&self, name: &str) -> Option<&Token> {
        self.index.get(name).map(|i| &self.items[*i].1)
    }

    /// Whether a token has this name.
    pub fn has(&self, name: &str) -> bool {
        self.index.contains_key(name)
    }

    /// Names and tokens in document order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Token)> {
        self.items
            .iter()
            .map(|(name, token)| (name.as_str(), token))
    }
}

/// `flattenTokens`: a DTCG tree as name, type, value and description, in document order.
pub fn flatten_tokens(tree: &Value) -> Flat {
    fn walk(node: &Value, trail: &mut Vec<String>, ty: Option<&Value>, out: &mut Flat) {
        let Value::Object(map) = node else {
            return;
        };
        let ty = js::truthy_value(map.get("$type")).or(ty);
        if let Some(value) = map.get("$value") {
            out.set(
                trail.join("."),
                Token {
                    ty: ty.cloned(),
                    value: value.clone(),
                    description: map.get("$description").cloned(),
                },
            );
            return;
        }
        for (key, child) in js::entries(map) {
            if !key.starts_with('$') {
                trail.push(key.clone());
                walk(child, trail, ty, out);
                trail.pop();
            }
        }
    }
    let mut out = Flat::default();
    walk(tree, &mut Vec::new(), None, &mut out);
    out
}

mod from_penpot;
mod merge;
mod project;
mod read;
mod to_penpot;

pub use from_penpot::*;
pub use merge::*;
pub use project::*;
pub use read::*;
pub use to_penpot::*;

#[cfg(test)]
mod tests;
