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
use crate::core::paths::{file, project_paths, rel, resolve};
use crate::core::util::{to_json_compact, write_json, write_json_inside};
use crate::design::color::{oklch_to_hex, Oklch};
use crate::design::tokens::{dir_names, option, relative_path};

/// The Penpot set that holds `design/tokens/tokens.json`.
pub(crate) const BASE_SET: &str = "base";
/// The prefix of the Penpot set of a dfw theme (`themes/<name>`).
pub(crate) const THEME_PREFIX: &str = "themes/";
/// The Penpot theme group of the dfw themes.
pub(crate) const THEME_GROUP: &str = "dfw";
/// The state of a set a Penpot theme turns on (`selectedTokenSets`).
pub(crate) const ENABLED: &str = "enabled";

/// The `$` keys of a DTCG tree and of a Penpot token file.
pub(crate) mod key {
    pub(crate) const VALUE: &str = "$value";
    pub(crate) const TYPE: &str = crate::design::tokens::TYPE_KEY;
    pub(crate) const DESCRIPTION: &str = "$description";
    pub(crate) const THEMES: &str = "$themes";
    pub(crate) const METADATA: &str = "$metadata";
    /// The sRGB fallback of a DTCG colour object.
    pub(crate) const HEX: &str = "hex";

    /* ------------------------------------- a Tokens Studio token of the legacy form (no `$`) */

    pub(crate) const LEGACY_VALUE: &str = "value";
    pub(crate) const LEGACY_TYPE: &str = "type";
    pub(crate) const LEGACY_DESCRIPTION: &str = "description";

    /* ------------------------------------------------------------ a DTCG colour object */

    /// The colour space of a DTCG colour object (`srgb`, `oklch`, ...).
    pub(crate) const COLOR_SPACE: &str = "colorSpace";
    /// The channels of a DTCG colour object, in its colour space.
    pub(crate) const COMPONENTS: &str = "components";
    pub(crate) const ALPHA: &str = "alpha";

    /* ---------------------------------------------- a DTCG dimension or duration `{ value, unit }` */

    /// The number of a DTCG dimension or duration (not the `$value` of a token).
    pub(crate) const AMOUNT: &str = "value";
    pub(crate) const UNIT: &str = "unit";

    /* --------------------------------------------------- a shadow layer (DTCG and Penpot) */

    pub(crate) const COLOR: &str = "color";
    pub(crate) const OFFSET_X: &str = "offsetX";
    pub(crate) const OFFSET_Y: &str = "offsetY";
    pub(crate) const BLUR: &str = "blur";
    pub(crate) const SPREAD: &str = "spread";
    pub(crate) const INSET: &str = "inset";
    /// The Penpot spelling of `offsetX`.
    pub(crate) const X: &str = "x";
    /// The Penpot spelling of `offsetY`.
    pub(crate) const Y: &str = "y";
    /// The kind of a Penpot shadow layer (`innerShadow` is an inset one).
    pub(crate) const SHADOW_TYPE: &str = "type";

    /* ------------------------------------------------------- a typography value (DTCG) */

    pub(crate) const FONT_FAMILY: &str = "fontFamily";
    pub(crate) const FONT_SIZE: &str = "fontSize";
    pub(crate) const FONT_WEIGHT: &str = "fontWeight";
    pub(crate) const LETTER_SPACING: &str = "letterSpacing";
    pub(crate) const LINE_HEIGHT: &str = "lineHeight";
    pub(crate) const TEXT_CASE: &str = "textCase";
    pub(crate) const TEXT_DECORATION: &str = "textDecoration";

    /* ---------------------------------------- the other spellings of a Penpot typography value */

    pub(crate) const FONT_FAMILIES: &str = "fontFamilies";
    pub(crate) const FONT_SIZES: &str = "fontSizes";
    pub(crate) const FONT_WEIGHTS: &str = "fontWeights";
    pub(crate) const LINE_HEIGHTS: &str = "lineHeights";
    pub(crate) const CSS_FONT_FAMILY: &str = "font-family";
    pub(crate) const CSS_FONT_SIZE: &str = "font-size";
    pub(crate) const CSS_FONT_WEIGHT: &str = "font-weight";
    pub(crate) const CSS_LETTER_SPACING: &str = "letter-spacing";
    pub(crate) const CSS_LINE_HEIGHT: &str = "line-height";
    pub(crate) const CSS_TEXT_CASE: &str = "text-case";
    pub(crate) const CSS_TEXT_DECORATION: &str = "text-decoration";

    /* --------------------------------------------------------- `$metadata` of a Penpot file */

    /// The order of the sets in a Penpot file.
    pub(crate) const TOKEN_SET_ORDER: &str = "tokenSetOrder";
    pub(crate) const ACTIVE_THEMES: &str = "activeThemes";
    pub(crate) const ACTIVE_SETS: &str = "activeSets";

    /* ------------------------------------------------------ one theme of `$themes` (Penpot) */

    pub(crate) const NAME: &str = "name";
    pub(crate) const GROUP: &str = "group";
    pub(crate) const THEME_DESCRIPTION: &str = "description";
    /// Whether the theme's sets are a source (Penpot); dfw writes `false`.
    pub(crate) const IS_SOURCE: &str = "isSource";
    /// The sets a theme turns on: `{ <set>: "enabled" }`.
    pub(crate) const SELECTED_TOKEN_SETS: &str = "selectedTokenSets";

    /// The keys of the export and import reports dfw prints.
    pub(crate) mod report {
        /// The export report: how many tokens went out.
        pub(crate) const EXPORTED: &str = "exported";
        /// The export report: the Penpot sets written.
        pub(crate) const SETS: &str = "sets";
        /// The tokens left out: `{ set, token, type, reason }`.
        pub(crate) const UNSUPPORTED: &str = "unsupported";
        /// The tokens converted with a loss: `{ set, token, note }`.
        pub(crate) const LOSSY: &str = "lossy";
        pub(crate) const SKIPPED_THEMES: &str = "skippedThemes";
        /// The token files an import wrote.
        pub(crate) const WRITTEN: &str = "written";
        /// The import report: one set diff per set.
        pub(crate) const DIFF: &str = "diff";
        /// The import notes: `{ set, token, note }`.
        pub(crate) const WARNINGS: &str = "warnings";
        pub(crate) const SKIPPED_SETS: &str = "skippedSets";
        /* one set diff */
        pub(crate) const ADDED: &str = "added";
        pub(crate) const CHANGED: &str = "changed";
        pub(crate) const REMOVED: &str = "removed";
        /// In dfw, missing in Penpot, kept.
        pub(crate) const KEPT: &str = "kept";
        /* one line of a report */
        pub(crate) const SET: &str = "set";
        /// The dotted token name.
        pub(crate) const TOKEN: &str = "token";
        /// The DTCG type of the token.
        pub(crate) const TYPE: &str = "type";
        pub(crate) const VALUE: &str = "value";
        pub(crate) const REASON: &str = "reason";
        pub(crate) const NOTE: &str = "note";
        /// A changed token: its value before and after.
        pub(crate) const FROM: &str = "from";
        pub(crate) const TO: &str = "to";
        /// A changed token whose description alone changed.
        pub(crate) const DESCRIPTION_ONLY: &str = "descriptionOnly";
    }
}

/// The colour spaces a DTCG colour object names (`colorSpace`).
pub(crate) mod color_space {
    pub(crate) const SRGB: &str = "srgb";
    pub(crate) const OKLCH: &str = "oklch";
}

/// The units of a DTCG dimension or duration (`unit`).
pub(crate) mod unit {
    pub(crate) const PX: &str = "px";
    pub(crate) const MS: &str = "ms";
    pub(crate) const S: &str = "s";
}

/// The DTCG `$type` names dfw reads and writes (their home is `design::tokens`).
pub(crate) use crate::design::tokens::dtcg;

/// Penpot's token type names (`$type` in a Penpot token file).
pub(crate) mod penpot_type {
    pub(crate) const COLOR: &str = "color";
    pub(crate) const DIMENSION: &str = "dimension";
    pub(crate) const SPACING: &str = "spacing";
    pub(crate) const SIZING: &str = "sizing";
    pub(crate) const BORDER_RADIUS: &str = "borderRadius";
    pub(crate) const BORDER_WIDTH: &str = "borderWidth";
    pub(crate) const FONT_SIZES: &str = "fontSizes";
    pub(crate) const LETTER_SPACING: &str = "letterSpacing";
    pub(crate) const FONT_FAMILIES: &str = "fontFamilies";
    pub(crate) const FONT_WEIGHTS: &str = "fontWeights";
    pub(crate) const NUMBER: &str = "number";
    pub(crate) const TEXT_CASE: &str = "textCase";
    pub(crate) const TEXT_DECORATION: &str = "textDecoration";
    pub(crate) const SHADOW: &str = "shadow";
    pub(crate) const TYPOGRAPHY: &str = "typography";
}

/// Token and set names come from a Penpot file (untrusted): in JavaScript a path segment like
/// `__proto__` would write into `Object.prototype` instead of the tree. Refused here too, so
/// both implementations accept the same names.
pub(super) use crate::core::util::RESERVED_KEYS as RESERVED;
/// The best known of the [`INHERITED`] names.
pub(super) const TO_STRING: &str = "toString";

/// The functions every JavaScript object inherits. The Node code read groups and themes with
/// `object[name]`, so one of these names found the built-in function where nothing was stored.
const INHERITED: [&str; 11] = [
    "constructor",
    TO_STRING,
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
        static $name: LazyLock<Regex> =
            LazyLock::new(|| $crate::core::js::regex::fixed_regex(&$source));
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
        let ty = js::truthy_value(map.get(key::TYPE)).or(ty);
        if let Some(value) = map.get(key::VALUE) {
            out.set(
                trail.join("."),
                Token {
                    ty: ty.cloned(),
                    value: value.clone(),
                    description: map.get(key::DESCRIPTION).cloned(),
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
