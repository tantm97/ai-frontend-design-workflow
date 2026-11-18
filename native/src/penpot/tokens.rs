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
// Over 800 lines: one Node file ported whole, plus `jv`, the JavaScript value rules the Penpot
// modules share (the tests are in tokens_tests.rs).

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::{to_json_compact, write_json};
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

/* ------------------------------------------------------------------ dfw value -> Penpot string */

/// Collects the notes of a lossy conversion.
type Note<'a> = &'a mut dyn FnMut(String);

/// `x.toString(16).padStart(2, '0')` for a rounded channel.
fn hex2(x: f64) -> String {
    if x.is_finite() {
        format!("{:0>2}", format!("{:x}", x.max(0.0) as u64))
    } else {
        js::number(x)
    }
}

/// `parseInt(text, 16)`: the leading hexadecimal digits, `NaN` without any.
fn parse_int16(text: &str) -> f64 {
    let t = js::trim(text);
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

/// The three channels of `#rrggbb`, as `parseInt` of each pair.
fn hex_channels(hex: &str) -> Vec<f64> {
    [1, 3, 5]
        .iter()
        .map(|i| parse_int16(&js::slice16(hex, *i, i + 2)))
        .collect()
}

fn color_out(v: Option<&Value>, note: Note) -> Result<Value, String> {
    let v = match v {
        Some(Value::String(s)) => return Ok(Value::String(s.clone())),
        Some(v @ (Value::Object(_) | Value::Array(_))) => v,
        other => return Err(format!("not a colour: {}", js::stringify(other))),
    };
    let alpha = js::present(v.get("alpha"));
    let opaque = alpha.is_none_or(|a| a.as_f64() == Some(1.0));
    let space = v.get("colorSpace").and_then(Value::as_str);
    let components = v.get("components");
    let hex = js::truthy_value(v.get("hex"));
    let numbers = |list: &Vec<Value>| list.iter().all(Value::is_number);
    let rgb: Vec<f64> = if let Some(list) = components
        .and_then(Value::as_array)
        .filter(|list| space == Some("srgb") && numbers(list))
    {
        match hex {
            Some(Value::String(h)) if opaque => return Ok(Value::String(h.to_lowercase())),
            Some(_) if opaque => return Err("v.hex.toLowerCase is not a function".to_string()),
            _ => {}
        }
        list.iter()
            .map(|x| js::round(js::min(1.0, js::max(0.0, js::to_number(Some(x)))) * 255.0))
            .collect()
    } else if space == Some("oklch") && js::present(components).is_some() {
        let Some(list) = components.and_then(Value::as_array) else {
            return Err("v.components?.every is not a function".to_string());
        };
        if !numbers(list) {
            return hex_fallback(v, hex, opaque, note);
        }
        let hex = oklch_to_hex(Oklch {
            l: js::to_number(list.first()),
            c: js::to_number(list.get(1)),
            h: js::to_number(list.get(2)),
        });
        note(format!("oklch converted to sRGB {hex}"));
        if opaque {
            return Ok(Value::String(hex));
        }
        hex_channels(&hex)
    } else {
        return hex_fallback(v, hex, opaque, note);
    };
    Ok(Value::String(rgb_text(&rgb, opaque, alpha)))
}

/// The last two branches of `colorOut`: another colour space goes through its `hex`.
fn hex_fallback(v: &Value, hex: Option<&Value>, opaque: bool, note: Note) -> Result<Value, String> {
    let space = js::to_string(v.get("colorSpace"));
    let Some(hex) = hex else {
        return Err(format!(
            "colour space \"{space}\" has no sRGB form (add a \"hex\" fallback)"
        ));
    };
    note(format!("{space} colour exported through its hex fallback"));
    let Value::String(hex) = hex else {
        return Err(if opaque {
            "v.hex.toLowerCase is not a function".to_string()
        } else {
            "v.hex.slice is not a function".to_string()
        });
    };
    if opaque {
        return Ok(Value::String(hex.to_lowercase()));
    }
    Ok(Value::String(rgb_text(
        &hex_channels(hex),
        opaque,
        js::present(v.get("alpha")),
    )))
}

fn rgb_text(rgb: &[f64], opaque: bool, alpha: Option<&Value>) -> String {
    if opaque {
        return format!("#{}", rgb.iter().map(|x| hex2(*x)).collect::<String>());
    }
    let channels: Vec<String> = rgb.iter().map(|x| js::number(*x)).collect();
    format!(
        "rgba({}, {})",
        channels.join(", "),
        fmt(js::to_number(alpha))
    )
}

/// dfw wraps Penpot math in `calc()` so the CSS stays valid; Penpot gets the bare expression.
fn uncalc(text: &str) -> String {
    match CALC.captures(text) {
        Some(c) => c[1].to_string(),
        None => text.to_string(),
    }
}

fn is_expression(value: &Value) -> bool {
    let Value::String(s) = value else {
        return false;
    };
    let plain = ALIAS.replace_all(s, "x");
    MATH.is_match(plain.strip_prefix('-').unwrap_or(&plain))
}

fn dim_out(v: Option<&Value>, note: Note, bare: bool) -> Result<String, String> {
    match v {
        Some(Value::String(_) | Value::Number(_)) => Ok(uncalc(&js::to_string(v))),
        Some(Value::Object(map)) if map.get("value").is_some_and(Value::is_number) => {
            let value = fmt(js::to_number(map.get("value")));
            if map.get("unit").and_then(Value::as_str) == Some("px") {
                return Ok(if bare { value } else { format!("{value}px") });
            }
            let unit = js::to_string(map.get("unit"));
            note(format!(
                "unit \"{unit}\": Penpot works in px; check the value after import"
            ));
            Ok(format!("{value}{unit}"))
        }
        other => Err(format!("not a dimension: {}", js::stringify(other))),
    }
}

fn shadow_out(v: &Value, note: Note) -> Result<Value, String> {
    fn one(s: &Value, note: Note) -> Result<Value, String> {
        match s {
            Value::String(_) => return Ok(s.clone()),
            Value::Null => {
                return Err("Cannot read properties of null (reading 'color')".to_string())
            }
            _ => {}
        }
        let zero = json!({ "value": 0, "unit": "px" });
        let color = color_out(s.get("color"), note)?;
        let offset_x = dim_out(s.get("offsetX"), note, true)?;
        let offset_y = dim_out(s.get("offsetY"), note, true)?;
        let blur = dim_out(s.get("blur"), note, true)?;
        let spread = dim_out(js::present(s.get("spread")).or(Some(&zero)), note, true)?;
        Ok(json!({
            "color": color,
            "offsetX": offset_x,
            "offsetY": offset_y,
            "blur": blur,
            "spread": spread,
            "inset": js::truthy(s.get("inset")),
        }))
    }
    match v {
        Value::String(_) => Ok(v.clone()),
        Value::Array(items) => {
            let mut list = Vec::new();
            for s in items {
                list.push(one(s, note)?);
            }
            Ok(Value::Array(list))
        }
        other => one(other, note),
    }
}

const TYPO_OUT: [(&str, &str); 7] = [
    ("fontFamily", "fontFamilies"),
    ("fontSize", "fontSizes"),
    ("fontWeight", "fontWeights"),
    ("letterSpacing", "letterSpacing"),
    ("lineHeight", "lineHeights"),
    ("textCase", "textCase"),
    ("textDecoration", "textDecoration"),
];
const TEXT_CASES: [&str; 4] = ["none", "uppercase", "lowercase", "capitalize"];
const TEXT_DECORATIONS: [&str; 3] = ["underline", "strike-through", "line-through"];

/// `penpotDimensionType`: the Penpot type for a dfw dimension token, from its name (dfw has one
/// `dimension` type, Penpot several).
pub fn penpot_dimension_type(name: &str, value: &Value) -> &'static str {
    let n = name.to_lowercase();
    let px = match value {
        Value::Object(map) => map.get("unit").and_then(Value::as_str) == Some("px"),
        Value::Array(_) => false,
        _ => true,
    };
    if FONT_SIZE.is_match(&n) {
        return if px { "fontSizes" } else { "dimension" };
    }
    if TRACKING.is_match(&n) {
        return if px { "letterSpacing" } else { "dimension" };
    }
    if RADIUS.is_match(&n) {
        "borderRadius"
    } else if BORDER_WIDTH.is_match(&n) {
        "borderWidth"
    } else if SPACING.is_match(&n) {
        "spacing"
    } else if SIZING.is_match(&n) {
        "sizing"
    } else {
        "dimension"
    }
}

/// The result of converting one token: its form on the other side, or why it has none.
#[derive(Debug, Clone, PartialEq)]
pub enum Converted {
    Token {
        ty: Value,
        value: Value,
        description: Option<Value>,
    },
    Unsupported(String),
}

impl Converted {
    /// The reason, when the token has no form on the other side.
    pub fn unsupported(&self) -> Option<&str> {
        match self {
            Converted::Unsupported(reason) => Some(reason),
            Converted::Token { .. } => None,
        }
    }

    /// The converted value.
    pub fn value(&self) -> Option<&Value> {
        match self {
            Converted::Token { value, .. } => Some(value),
            Converted::Unsupported(_) => None,
        }
    }

    /// The description that travels with the token (only a truthy one does).
    pub fn description(&self) -> Option<&Value> {
        match self {
            Converted::Token { description, .. } => description.as_ref(),
            Converted::Unsupported(_) => None,
        }
    }

    /// The token as the object the Node code returned.
    #[cfg(test)]
    pub fn to_json(&self) -> Value {
        match self {
            Converted::Token {
                ty,
                value,
                description,
            } => js::object(vec![
                ("type", Some(ty.clone())),
                ("value", Some(value.clone())),
                ("description", description.clone()),
            ]),
            Converted::Unsupported(reason) => json!({ "unsupported": reason }),
        }
    }
}

fn res(ty: impl Into<Value>, value: impl Into<Value>, description: Option<&Value>) -> Converted {
    Converted::Token {
        ty: ty.into(),
        value: value.into(),
        description: js::truthy_value(description).cloned(),
    }
}

fn families_out(value: &Value) -> String {
    match value {
        Value::Array(items) => js::join(items, ", "),
        other => js::to_string(Some(other)),
    }
}

/// `tokenToPenpot`: one dfw token in its Penpot form. `note` collects lossy conversions.
pub fn token_to_penpot(name: &str, token: &Token, note: Note) -> Converted {
    to_penpot(name, token, note).unwrap_or_else(Converted::Unsupported)
}

/// [`token_to_penpot`] without collecting notes.
pub fn token_to_penpot_quiet(name: &str, token: &Token) -> Converted {
    token_to_penpot(name, token, &mut |_| {})
}

/// `typeof value !== 'object'`.
fn is_primitive(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Object(_) | Value::Array(_))
}

fn to_penpot(name: &str, token: &Token, note: Note) -> Result<Converted, String> {
    let value = &token.value;
    let description = token.description.as_ref();
    let done = |ty: &str, value: Value| Ok(res(ty, value, description));
    match token.type_name() {
        Some("color") => {
            if is_alias(Some(value)) {
                done("color", value.clone())
            } else {
                done("color", color_out(Some(value), note)?)
            }
        }
        Some("dimension") => {
            let ty = penpot_dimension_type(name, value);
            let text = match value {
                Value::String(s) => uncalc(s),
                other => dim_out(Some(other), note, false)?,
            };
            done(ty, text.into())
        }
        Some("fontFamily") => done("fontFamilies", families_out(value).into()),
        Some("fontWeight") => done("fontWeights", js::to_string(Some(value)).into()),
        Some("number") => done("number", uncalc(&js::to_string(Some(value))).into()),
        Some("duration") => {
            note(
                "duration exported as a Penpot \"number\" token in ms (Penpot has no duration type)"
                    .to_string(),
            );
            match value {
                Value::Null => Err("Cannot read properties of null (reading 'unit')".to_string()),
                Value::Object(_) | Value::Array(_) => {
                    let n = js::to_number(value.get("value"));
                    let seconds = value.get("unit").and_then(Value::as_str) == Some("s");
                    done("number", fmt(if seconds { n * 1000.0 } else { n }).into())
                }
                other => done("number", uncalc(&js::to_string(Some(other))).into()),
            }
        }
        Some("string") => match value.as_str() {
            Some(v) if TEXT_CASES.contains(&v) => done("textCase", v.into()),
            Some("line-through") => done("textDecoration", "strike-through".into()),
            Some(v) if TEXT_DECORATIONS.contains(&v) => done("textDecoration", v.into()),
            _ => Ok(Converted::Unsupported(
                "Penpot has no free-text token type (only textCase/textDecoration values map)"
                    .to_string(),
            )),
        },
        Some("shadow") => done("shadow", shadow_out(value, note)?),
        Some("typography") => {
            if is_alias(Some(value)) {
                return done("typography", value.clone());
            }
            if value.is_null() {
                return Err("Cannot read properties of null (reading 'fontFamily')".to_string());
            }
            let mut v = Map::new();
            for (key, penpot_key) in TYPO_OUT {
                let Some(sub) = value.get(key) else {
                    continue;
                };
                let out: Value = if is_alias(Some(sub)) {
                    sub.clone()
                } else if key == "fontFamily" {
                    families_out(sub).into()
                } else if key == "fontSize" || key == "letterSpacing" || !is_primitive(sub) {
                    dim_out(Some(sub), note, false)?.into()
                } else {
                    js::to_string(Some(sub)).into()
                };
                v.insert(penpot_key.to_string(), out);
            }
            done("typography", Value::Object(v))
        }
        _ => Ok(Converted::Unsupported(format!(
            "Penpot has no \"{}\" token type",
            js::to_string(token.ty.as_ref())
        ))),
    }
}

/// One token for [`nest`]: a field that is `None` is not written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NestEntry {
    pub name: String,
    pub value: Option<Value>,
    pub ty: Option<Value>,
    pub description: Option<Value>,
}

/// `nest`: a nested Penpot set from flat entries; names with a reserved segment are left out.
pub fn nest(entries: &[NestEntry]) -> Value {
    let mut root = json!({});
    for entry in entries {
        let Some(parts) = token_path(&entry.name) else {
            continue;
        };
        let Some((leaf, groups)) = parts.split_last() else {
            continue;
        };
        // Node bug kept: `node[name]` of a new group with an inherited name found the built-in
        // function, and the token went onto it instead of into the set.
        let mut probe = Some(&root);
        let lost = groups.iter().any(|part| {
            let here = probe.and_then(Value::as_object);
            let own = here.is_some_and(|map| map.contains_key(*part));
            probe = here.and_then(|map| map.get(*part));
            !own && INHERITED.contains(part)
        });
        if lost {
            continue;
        }
        let mut node = &mut root;
        for part in groups {
            let Value::Object(map) = node else {
                break;
            };
            let slot = map.entry((*part).to_string()).or_insert(Value::Null);
            if !slot.is_object() {
                *slot = json!({});
            }
            node = slot;
        }
        if let Value::Object(map) = node {
            map.insert(
                (*leaf).to_string(),
                js::object(vec![
                    ("$value", entry.value.clone()),
                    ("$type", entry.ty.clone()),
                    (
                        "$description",
                        js::truthy_value(entry.description.as_ref()).cloned(),
                    ),
                ]),
            );
        }
    }
    js::key_order(root)
}

/// What an export left out or changed (`report` of `dfwToPenpot`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportReport {
    pub exported: usize,
    pub sets: Vec<String>,
    /// `{ set, token, type, reason }`
    pub unsupported: Vec<Value>,
    /// `{ set, token, note }`
    pub lossy: Vec<Value>,
    pub skipped_themes: Vec<String>,
}

impl ExportReport {
    /// The report as the object the Node code printed.
    pub fn to_json(&self) -> Value {
        json!({
            "exported": self.exported,
            "sets": self.sets,
            "unsupported": self.unsupported,
            "lossy": self.lossy,
            "skippedThemes": self.skipped_themes,
        })
    }
}

/// `dfwToPenpot`: dfw tokens (base tree and themes by name) as a Penpot token file.
pub fn dfw_to_penpot(base: Option<&Value>, themes: &Map<String, Value>) -> (Value, ExportReport) {
    let mut report = ExportReport::default();
    let mut doc = Map::new();
    let empty = json!({});
    let theme_names: Vec<&String> = js::entries(themes).into_iter().map(|(k, _)| k).collect();
    let mut sets: Vec<(String, &Value)> = vec![(
        BASE_SET.to_string(),
        js::truthy_value(base).unwrap_or(&empty),
    )];
    for name in &theme_names {
        let tree = js::truthy_value(themes.get(*name)).unwrap_or(&empty);
        sets.push((format!("{THEME_PREFIX}{name}"), tree));
    }
    for (set, tree) in &sets {
        let mut entries = Vec::new();
        let flat = flatten_tokens(tree);
        for (name, token) in flat.iter() {
            let mut notes = Vec::new();
            let converted = token_to_penpot(name, token, &mut |n| notes.push(n));
            for note in notes {
                report
                    .lossy
                    .push(json!({ "set": set, "token": name, "note": note }));
            }
            match converted {
                Converted::Unsupported(reason) => report.unsupported.push(js::object(vec![
                    ("set", Some(set.as_str().into())),
                    ("token", Some(name.into())),
                    ("type", token.ty.clone()),
                    ("reason", Some(reason.into())),
                ])),
                Converted::Token {
                    ty,
                    value,
                    description,
                } => {
                    entries.push(NestEntry {
                        name: name.to_string(),
                        value: Some(value),
                        ty: Some(ty),
                        description,
                    });
                    report.exported += 1;
                }
            }
        }
        doc.insert(set.clone(), nest(&entries));
    }
    report.sets = sets.iter().map(|(s, _)| s.clone()).collect();
    let mut penpot_themes = vec![json!({
        "name": "default",
        "group": THEME_GROUP,
        "description": "dfw base tokens (design/tokens/tokens.json)",
        "isSource": false,
        "selectedTokenSets": { BASE_SET: "enabled" },
    })];
    for name in &theme_names {
        penpot_themes.push(json!({
            "name": name,
            "group": THEME_GROUP,
            "description": format!("dfw theme design/tokens/themes/{name}.json over the base"),
            "isSource": false,
            "selectedTokenSets": { BASE_SET: "enabled", format!("{THEME_PREFIX}{name}"): "enabled" },
        }));
    }
    doc.insert("$themes".into(), Value::Array(penpot_themes));
    doc.insert(
        "$metadata".into(),
        json!({
            "tokenSetOrder": report.sets,
            "activeThemes": [format!("{THEME_GROUP}/default")],
            "activeSets": [BASE_SET],
        }),
    );
    (Value::Object(doc), report)
}

/* ------------------------------------------------------------------ Penpot string -> dfw value */

const NAMED_WEIGHTS: [(&str, u32); 17] = [
    ("thin", 100),
    ("hairline", 100),
    ("extralight", 200),
    ("ultralight", 200),
    ("light", 300),
    ("regular", 400),
    ("normal", 400),
    ("book", 400),
    ("medium", 500),
    ("semibold", 600),
    ("demibold", 600),
    ("bold", 700),
    ("extrabold", 800),
    ("ultrabold", 800),
    ("black", 900),
    ("heavy", 900),
    ("extrablack", 950),
];

fn srgb(r: f64, g: f64, b: f64, a: f64) -> Value {
    let components: Vec<Value> = [r, g, b]
        .iter()
        .map(|x| js::num(round4(x / 255.0)))
        .collect();
    let hex = format!(
        "#{}",
        [r, g, b]
            .iter()
            .map(|x| hex2(js::round(*x)))
            .collect::<String>()
    );
    if a == 1.0 {
        json!({ "colorSpace": "srgb", "components": components, "hex": hex })
    } else {
        json!({ "colorSpace": "srgb", "components": components, "alpha": js::num(round4(a)), "hex": hex })
    }
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let (s, l) = (s / 100.0, l / 100.0);
    let k = |n: f64| (n + h / 30.0) % 12.0;
    let f = |n: f64| {
        l - s * js::min(l, 1.0 - l) * js::max(-1.0, js::min(k(n) - 3.0, js::min(9.0 - k(n), 1.0)))
    };
    [f(0.0) * 255.0, f(8.0) * 255.0, f(4.0) * 255.0]
}

/// The alpha of `rgb()`/`hsl()`: a number or a percentage; 1 when it is not given.
fn alpha_of(text: Option<&str>) -> f64 {
    match text {
        None => 1.0,
        Some(t) if t.ends_with('%') => js::parse_float(t) / 100.0,
        Some(t) => js::parse_float(t),
    }
}

/// `parseColor`: a Penpot colour string as a DTCG colour object, or `None` when it is not a
/// plain colour.
pub fn parse_color(text: &str) -> Option<Value> {
    let t = js::trim(text);
    if let Some(m) = HEX_COLOR.captures(t) {
        let mut h = m[1].to_string();
        if h.len() <= 4 {
            h = h.chars().flat_map(|c| [c, c]).collect();
        }
        let n = |i: usize| parse_int16(&h[i..i + 2]);
        let alpha = if h.len() > 6 { n(6) / 255.0 } else { 1.0 };
        return Some(srgb(n(0), n(2), n(4), alpha));
    }
    let number = |m: &regex::Captures, i: usize| js::str_to_number(&m[i]);
    if let Some(m) = RGB_COLOR.captures(t) {
        let a = alpha_of(m.get(4).map(|g| g.as_str()));
        return Some(srgb(number(&m, 1), number(&m, 2), number(&m, 3), a));
    }
    if let Some(m) = HSL_COLOR.captures(t) {
        let a = alpha_of(m.get(4).map(|g| g.as_str()));
        let [r, g, b] = hsl_to_rgb(number(&m, 1), number(&m, 2), number(&m, 3));
        return Some(srgb(js::round(r), js::round(g), js::round(b), a));
    }
    None
}

/// `parseDimension`: `"16"`, `"16px"`, `"-2.5"`, `"1.5rem"` as `{ value, unit }` (no unit is
/// px); `None` when it is an expression.
pub fn parse_dimension(value: Option<&Value>) -> Option<Value> {
    if let Some(n @ Value::Number(_)) = value {
        return Some(json!({ "value": n, "unit": "px" }));
    }
    let text = js::to_string(value);
    let m = DIMENSION.captures(&text)?;
    let unit = m
        .get(2)
        .map_or("px".to_string(), |u| u.as_str().to_lowercase());
    Some(json!({ "value": js::num(js::str_to_number(&m[1])), "unit": unit }))
}

fn parse_number(value: Option<&Value>) -> Option<Value> {
    if let Some(n @ Value::Number(_)) = value {
        return Some(n.clone());
    }
    let text = js::to_string(value);
    let m = NUMBER.captures(&text)?;
    let n = js::str_to_number(&m[1]);
    Some(js::num(if m.get(2).is_some() {
        round4(n / 100.0)
    } else {
        n
    }))
}

fn parse_weight(value: Option<&Value>, warn: Note) -> Option<Value> {
    if let Some(n @ Value::Number(_)) = value {
        return Some(n.clone());
    }
    let text = js::to_string(value);
    let t = js::trim(&text);
    if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) {
        return Some(js::num(js::str_to_number(t)));
    }
    let key: String = t
        .to_lowercase()
        .chars()
        .filter(|c| !(crate::core::js::is_space(*c) || *c == '_' || *c == '-'))
        .collect();
    let italic = key.ends_with("italic");
    let base = key.strip_suffix("italic").unwrap_or(&key);
    let weight = NAMED_WEIGHTS
        .iter()
        .find(|(name, _)| *name == base)
        .map(|(_, w)| *w)
        .or((italic && key == "italic").then_some(400))?;
    if italic {
        warn(
            "italic is not part of a dfw fontWeight token: set font-style in the master"
                .to_string(),
        );
    }
    Some(json!(weight))
}

fn split_families(value: &Value) -> Value {
    let items: Vec<String> = match value {
        Value::Array(items) => items.iter().map(|x| js::to_string(Some(x))).collect(),
        other => js::to_string(Some(other))
            .split(',')
            .map(str::to_string)
            .collect(),
    };
    let quote = |c: char| c == '"' || c == '\'';
    Value::Array(
        items
            .iter()
            .map(|x| {
                let t = js::trim(x);
                let t = t.strip_prefix(quote).unwrap_or(t);
                t.strip_suffix(quote).unwrap_or(t)
            })
            .filter(|x| !x.is_empty())
            .map(Value::from)
            .collect(),
    )
}

const PENPOT_DIMENSION_TYPES: [&str; 9] = [
    "dimension",
    "spacing",
    "sizing",
    "borderRadius",
    "borderWidth",
    "strokeWidth",
    "fontSizes",
    "fontSize",
    "letterSpacing",
];

/// The dfw type of a plain alias, from the Penpot type.
fn alias_type(ty: Option<&str>) -> Option<&'static str> {
    Some(match ty? {
        "color" => "color",
        "fontFamilies" | "fontFamily" => "fontFamily",
        "fontWeights" | "fontWeight" => "fontWeight",
        "number" | "opacity" | "rotation" => "number",
        "shadow" | "boxShadow" => "shadow",
        "typography" => "typography",
        "textCase" | "textDecoration" => "string",
        other if PENPOT_DIMENSION_TYPES.contains(&other) => "dimension",
        _ => return None,
    })
}

/// `tokenFromPenpot`: one Penpot token as a dfw token. `prefer` is the current dfw type of the
/// name (keeps a duration a duration); `warn` collects notes. An error is a value the Node code
/// could not read at all (a `null` shadow).
pub fn token_from_penpot(
    name: &str,
    token: &Token,
    prefer: Option<&Value>,
    warn: Note,
) -> Result<Converted, DfwError> {
    let value = &token.value;
    let description = token.description.as_ref();
    let ty = token.type_name();
    // A number token named like a duration is a dfw duration (Penpot has no duration type).
    let duration = json!("duration");
    let mut prefer = js::truthy_value(prefer);
    if prefer.is_none() && ty == Some("number") && DURATION_NAME.is_match(name) {
        prefer = Some(&duration);
    }
    let unsupported = || {
        Converted::Unsupported(format!(
            "Penpot type \"{}\" has no dfw equivalent",
            js::to_string(token.ty.as_ref())
        ))
    };
    let raw = |t: &str, why: &str, warn: Note| {
        if matches!(t, "dimension" | "number" | "duration") && is_expression(value) {
            let text = js::to_string(Some(value));
            warn(format!(
                "math expression kept as calc({text}) (check that the CSS is what you expect)"
            ));
            return res(t, format!("calc({text})"), description);
        }
        let shown = match value {
            Value::String(s) => s.clone(),
            other => to_json_compact(other),
        };
        warn(format!("{why}; kept as the raw string \"{shown}\""));
        res(t, value.clone(), description)
    };
    if let Value::String(s) = value {
        let trimmed = js::trim(s);
        if PLAIN_ALIAS.is_match(trimmed) {
            // A plain alias: keep it; the dfw type follows the current token or the Penpot type.
            let t = match prefer {
                Some(p) => p.clone(),
                None => match alias_type(ty) {
                    Some(t) => t.into(),
                    None => return Ok(unsupported()),
                },
            };
            return Ok(res(t, trimmed, description));
        }
    }
    let wants_duration = prefer.and_then(Value::as_str) == Some("duration");
    Ok(match ty {
        Some("color") => match parse_color(&js::to_string(Some(value))) {
            Some(c) => res("color", c, description),
            None => raw(
                "color",
                "not a plain hex/rgb/hsl colour (expression?)",
                warn,
            ),
        },
        Some("fontFamilies" | "fontFamily") => {
            res("fontFamily", split_families(value), description)
        }
        Some("fontWeights" | "fontWeight") => match parse_weight(Some(value), warn) {
            Some(w) => res("fontWeight", w, description),
            None => raw("fontWeight", "not a numeric or named weight", warn),
        },
        Some(t @ ("number" | "opacity" | "rotation")) => match parse_number(Some(value)) {
            None => raw(
                if wants_duration { "duration" } else { "number" },
                "not a plain number",
                warn,
            ),
            Some(n) if wants_duration => {
                res("duration", json!({ "value": n, "unit": "ms" }), description)
            }
            Some(n) => {
                if t != "number" {
                    warn(format!("Penpot {t} token stored as a dfw number"));
                }
                res("number", n, description)
            }
        },
        Some("textCase" | "textDecoration") => {
            res("string", js::to_string(Some(value)), description)
        }
        Some("shadow" | "boxShadow") => match value {
            Value::String(_) => raw("shadow", "shadow given as a string", warn),
            Value::Array(items) => {
                let list: Result<Vec<Value>, DfwError> = items.iter().map(shadow_in).collect();
                res("shadow", Value::Array(list?), description)
            }
            other => res("shadow", shadow_in(other)?, description),
        },
        Some("typography") => match value {
            Value::Object(_) | Value::Array(_) => {
                res("typography", typography_in(value, warn), description)
            }
            _ => raw("typography", "typography given as a string", warn),
        },
        Some(t) if PENPOT_DIMENSION_TYPES.contains(&t) => match parse_dimension(Some(value)) {
            Some(d) => res("dimension", d, description),
            None => raw("dimension", "not a plain number with px/rem/em", warn),
        },
        _ => unsupported(),
    })
}

fn shadow_in(s: &Value) -> Result<Value, DfwError> {
    match s {
        Value::String(_) => return Ok(s.clone()),
        Value::Null => {
            return Err(DfwError::internal(
                "Cannot read properties of null (reading 'color')",
            ))
        }
        _ => {}
    }
    // `x ?? 0`, then the alias as it is, the dimension, or the value untouched.
    let zero = json!(0);
    let d = |x: Option<&Value>| -> Value {
        let x = js::present(x).unwrap_or(&zero);
        if is_alias(Some(x)) {
            x.clone()
        } else {
            parse_dimension(Some(x)).unwrap_or_else(|| x.clone())
        }
    };
    let color = s.get("color");
    let color = if is_alias(color) {
        color.cloned()
    } else {
        parse_color(&js::to_string(color)).or_else(|| color.cloned())
    };
    let either = |a: &str, b: &str| js::present(s.get(a)).or(s.get(b));
    let inset =
        js::truthy(s.get("inset")) || s.get("type").and_then(Value::as_str) == Some("innerShadow");
    Ok(js::object(vec![
        ("color", color),
        ("offsetX", Some(d(either("offsetX", "x")))),
        ("offsetY", Some(d(either("offsetY", "y")))),
        ("blur", Some(d(s.get("blur")))),
        ("spread", Some(d(s.get("spread")))),
        ("inset", inset.then_some(Value::Bool(true))),
    ]))
}

fn typography_in(value: &Value, warn: Note) -> Value {
    let pick = |keys: &[&str]| keys.iter().find_map(|k| value.get(*k));
    let or_self = |parsed: Option<Value>, raw: &Value| parsed.unwrap_or_else(|| raw.clone());
    let mut v = Map::new();
    if let Some(fam) = pick(&["fontFamilies", "fontFamily", "font-family"]) {
        let out = if is_alias(Some(fam)) {
            fam.clone()
        } else {
            split_families(fam)
        };
        v.insert("fontFamily".into(), out);
    }
    if let Some(size) = pick(&["fontSizes", "fontSize", "font-size"]) {
        let out = if is_alias(Some(size)) {
            size.clone()
        } else {
            or_self(parse_dimension(Some(size)), size)
        };
        v.insert("fontSize".into(), out);
    }
    if let Some(w) = pick(&["fontWeights", "fontWeight", "font-weight"]) {
        let out = if is_alias(Some(w)) {
            w.clone()
        } else {
            or_self(parse_weight(Some(w), warn), w)
        };
        v.insert("fontWeight".into(), out);
    }
    if let Some(ls) = pick(&["letterSpacing", "letter-spacing"]) {
        let out = if is_alias(Some(ls)) {
            ls.clone()
        } else {
            or_self(parse_dimension(Some(ls)), ls)
        };
        v.insert("letterSpacing".into(), out);
    }
    if let Some(lh) = pick(&["lineHeights", "lineHeight", "line-height"]) {
        let out = if is_alias(Some(lh)) {
            lh.clone()
        } else {
            or_self(parse_number(Some(lh)), lh)
        };
        v.insert("lineHeight".into(), out);
    }
    if let Some(tc) = pick(&["textCase", "text-case"]) {
        v.insert("textCase".into(), tc.clone());
    }
    if let Some(td) = pick(&["textDecoration", "text-decoration"]) {
        v.insert("textDecoration".into(), td.clone());
    }
    Value::Object(v)
}

/* ------------------------------------------------------------------ reading a Penpot file */

/// The tokens of one Penpot set (`$value`/`$type`, or the older `value`/`type`). Names with a
/// reserved segment are not walked into: they are collected in `reserved`.
fn walk_set(tree: &Value, reserved: &mut Vec<String>) -> Flat {
    fn walk(
        node: &Value,
        trail: &mut Vec<String>,
        ty: Option<&Value>,
        out: &mut Flat,
        reserved: &mut Vec<String>,
    ) {
        let name = trail.join(".");
        if token_path(&name).is_none() {
            reserved.push(name);
            return;
        }
        let Value::Object(map) = node else {
            return;
        };
        let ty = js::truthy_value(map.get("$type"))
            .or_else(|| js::truthy_value(map.get("type")))
            .or(ty);
        let legacy = map.contains_key("value") && map.contains_key("type") && !trail.is_empty();
        if map.contains_key("$value") || legacy {
            out.set(
                name,
                Token {
                    ty: ty.cloned(),
                    value: map
                        .get("$value")
                        .or_else(|| map.get("value"))
                        .cloned()
                        .unwrap_or(Value::Null),
                    description: js::present(map.get("$description"))
                        .or_else(|| map.get("description"))
                        .cloned(),
                },
            );
            return;
        }
        for (key, child) in js::entries(map) {
            if key.starts_with('$') {
                continue;
            }
            let depth = trail.len();
            trail.extend(key.split('.').map(str::to_string));
            walk(child, trail, ty, out, reserved);
            trail.truncate(depth);
        }
    }
    let mut out = Flat::default();
    walk(tree, &mut Vec::new(), None, &mut out, reserved);
    out
}

/// A Penpot token export as read: its sets by name, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct PenpotFile {
    pub sets: Vec<(String, Value)>,
    pub themes: Value,
    pub metadata: Value,
    /// A document without `$themes`/`$metadata`: the whole file is one set.
    pub single: bool,
}

impl PenpotFile {
    fn set(&self, name: &str) -> Option<&Value> {
        self.sets.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// ASCII punctuation in the order the root collation sorts it.
const PUNCTUATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// Order of `String.prototype.localeCompare` for file names, without the collation tables:
/// punctuation (ASCII in collation order), then digits, then letters; letters without regard
/// to case first. Accented letters sort after the plain ones here.
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let class = |c: char| -> (u8, usize, String) {
        let rank = if c.is_alphabetic() {
            2
        } else if c.is_numeric() {
            1
        } else {
            0
        };
        let place = PUNCTUATION.find(c).unwrap_or(PUNCTUATION.len());
        (rank, place, c.to_lowercase().collect())
    };
    let key = |s: &str| s.chars().map(class).collect::<Vec<_>>();
    key(a)
        .cmp(&key(b))
        // Lowercase before uppercase when nothing else differs.
        .then_with(|| b.cmp(a))
}

fn parse_json_file(f: &Path) -> Result<Value, DfwError> {
    let text = js::read_text(f)
        .map_err(|err| DfwError::new(format!("{}: {}", f.display(), err.message())))?;
    serde_json::from_str::<Value>(&text)
        .map(js::key_order)
        .map_err(|err| DfwError::new(format!("{}: {err}", f.display())))
}

fn walk_set_files(
    dir: &Path,
    rel: &mut Vec<String>,
    sets: &mut Vec<(String, Value)>,
) -> Result<(), DfwError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push((name, entry.file_type()?.is_dir()));
    }
    entries.sort_by(|a, b| locale_cmp(&a.0, &b.0));
    for (name, is_dir) in entries {
        if is_dir {
            rel.push(name.clone());
            walk_set_files(&dir.join(&name), rel, sets)?;
            rel.pop();
        } else if name.ends_with(".json") && !name.starts_with('$') {
            let stem = name.strip_suffix(".json").unwrap_or(&name);
            let key = rel
                .iter()
                .map(String::as_str)
                .chain([stem])
                .collect::<Vec<_>>()
                .join("/");
            let doc = parse_json_file(&dir.join(&name))?;
            match sets.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = doc,
                None => sets.push((key, doc)),
            }
        }
    }
    Ok(())
}

/// `readPenpotFile`: a single JSON file (multi-set or one set) or a folder of set files with
/// `$themes.json` / `$metadata.json`.
pub fn read_penpot_file(file: &Path) -> Result<PenpotFile, DfwError> {
    if !file.exists() {
        return Err(DfwError::new(format!("{} not found.", file.display())));
    }
    if !file.is_dir() {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fallback = name.strip_suffix(".json").unwrap_or(&name);
        return parse_penpot_json(parse_json_file(file)?, fallback);
    }
    let mut sets = Vec::new();
    walk_set_files(file, &mut Vec::new(), &mut sets)?;
    let optional = |name: &str, fallback: Value| -> Result<Value, DfwError> {
        let f = file.join(name);
        if f.exists() {
            parse_json_file(&f)
        } else {
            Ok(fallback)
        }
    };
    Ok(PenpotFile {
        sets,
        themes: optional("$themes.json", json!([]))?,
        metadata: optional("$metadata.json", json!({}))?,
        single: false,
    })
}

/// `parsePenpotJson`: like Penpot, a document without `$themes`/`$metadata` is one set;
/// otherwise the top-level keys are sets.
pub fn parse_penpot_json(doc: Value, fallback_name: &str) -> Result<PenpotFile, DfwError> {
    let single = |doc: Value| PenpotFile {
        sets: vec![(fallback_name.to_string(), doc)],
        themes: json!([]),
        metadata: json!({}),
        single: true,
    };
    let map = match doc {
        Value::Object(map) => map,
        Value::Array(_) => return Ok(single(doc)),
        _ => {
            return Err(DfwError::new(
                "Not a Penpot token file (expected a JSON object).",
            ))
        }
    };
    if !map.contains_key("$themes") && !map.contains_key("$metadata") {
        return Ok(single(Value::Object(map)));
    }
    let or =
        |key: &str, fallback: Value| js::truthy_value(map.get(key)).cloned().unwrap_or(fallback);
    let (themes, metadata) = (or("$themes", json!([])), or("$metadata", json!({})));
    let sets = js::entries(&map)
        .into_iter()
        .filter(|(k, _)| !k.starts_with('$'))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(PenpotFile {
        sets,
        themes,
        metadata,
        single: false,
    })
}

/// Which dfw file a Penpot set feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetKind {
    /// `design/tokens/tokens.json`
    Base,
    /// `design/tokens/themes/<name>.json`
    Theme(String),
}

/// The result of [`map_sets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetMap {
    pub map: Vec<(String, SetKind)>,
    pub skipped: Vec<String>,
    pub base_name: Option<String>,
}

impl SetMap {
    /// What the set with this name feeds.
    #[cfg(test)]
    pub fn get(&self, name: &str) -> Option<&SetKind> {
        self.map.iter().find(|(n, _)| n == name).map(|(_, k)| k)
    }
}

/// `mapSets`: `base` feeds tokens.json, `themes/<n>` (or `theme/<n>`) feeds a theme, the rest
/// is skipped.
pub fn map_sets(parsed: &PenpotFile) -> Result<SetMap, DfwError> {
    let listed: Vec<String> = match js::truthy_value(parsed.metadata.get("tokenSetOrder")) {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s.chars().map(String::from).collect(),
        Some(_) => {
            return Err(DfwError::internal(
                "parsed.metadata.tokenSetOrder is not iterable",
            ))
        }
    };
    let mut order: Vec<String> = Vec::new();
    for name in listed
        .into_iter()
        .chain(parsed.sets.iter().map(|(n, _)| n.clone()))
    {
        if parsed.set(&name).is_some() && !order.contains(&name) {
            order.push(name);
        }
    }
    // A lone "themes/<n>" set in a multi-set file is still a theme, never the base (it would
    // overwrite tokens.json).
    let base_name = if parsed.single {
        order.first().cloned()
    } else if parsed.set(BASE_SET).is_some() {
        Some(BASE_SET.to_string())
    } else {
        order
            .iter()
            .find(|s| !THEME_SET_PREFIX.is_match(s))
            .cloned()
    };
    let mut map = Vec::new();
    let mut skipped = Vec::new();
    for s in order {
        let theme = THEME_SET
            .captures(&s)
            .map(|m| m[1].to_string())
            .filter(|name| THEME_NAME.is_match(name));
        if Some(&s) == base_name.as_ref() {
            map.push((s, SetKind::Base));
        } else if let Some(name) = theme {
            map.push((s, SetKind::Theme(name)));
        } else {
            skipped.push(s);
        }
    }
    Ok(SetMap {
        map,
        skipped,
        base_name,
    })
}

/* ------------------------------------------------------------------ merging into dfw trees */

fn inherited_type_at<'a>(tree: &'a Value, parts: &[&str]) -> Option<&'a Value> {
    let mut ty = None;
    let mut node = Some(tree);
    for part in parts {
        let Some(n) = node.filter(|n| n.is_object() || n.is_array()) else {
            return ty;
        };
        if let Some(t) = js::truthy_value(n.get("$type")) {
            ty = Some(t);
        }
        node = n.get(*part);
    }
    ty
}

fn is_token(node: &Value) -> bool {
    node.as_object().is_some_and(|m| m.contains_key("$value"))
}

/// Write one token into a dfw tree: the `$type` only when it differs from the group's, the
/// other keys of an existing token kept (`$extensions`, ...). A description is set when given
/// and removed when not.
fn set_token(
    tree: &mut Value,
    name: &str,
    ty: Option<&Value>,
    value: &Value,
    description: Option<&Value>,
) {
    let Some(parts) = token_path(name) else {
        return;
    };
    let Some((leaf, groups)) = parts.split_last() else {
        return;
    };
    if !tree.is_object() {
        *tree = json!({});
    }
    // First make the groups, then read the inherited type, then write the leaf.
    let mut node = &mut *tree;
    for part in groups {
        let Value::Object(map) = node else {
            return;
        };
        let slot = map.entry((*part).to_string()).or_insert(Value::Null);
        if !slot.is_object() || is_token(slot) {
            *slot = json!({});
        }
        node = slot;
    }
    let inherited = inherited_type_at(tree, &parts).cloned();
    let mut node = &mut *tree;
    for part in groups {
        let Some(next) = node.get_mut(*part) else {
            return;
        };
        node = next;
    }
    let Value::Object(group) = node else {
        return;
    };
    let prev = group.get(*leaf).filter(|v| is_token(v)).cloned();
    let mut next = prev
        .as_ref()
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    match ty {
        Some(ty) if !js::strict_eq(Some(ty), inherited.as_ref()) => {
            next.insert("$type".into(), ty.clone());
        }
        _ => {
            next.shift_remove("$type");
        }
    }
    next.insert("$value".into(), value.clone());
    match js::truthy_value(description) {
        Some(d) => {
            next.insert("$description".into(), d.clone());
        }
        None => {
            next.shift_remove("$description");
        }
    }
    // Key order: $type, $value, $description, then anything else ($extensions...).
    let mut ordered = Map::new();
    for key in ["$type", "$value", "$description"] {
        if let Some(v) = next.get(key) {
            ordered.insert(key.to_string(), v.clone());
        }
    }
    for (key, v) in js::entries(&next) {
        if !ordered.contains_key(key) {
            ordered.insert(key.clone(), v.clone());
        }
    }
    group.insert((*leaf).to_string(), Value::Object(ordered));
}

/// Remove a token and the groups it leaves empty (the root stays).
fn delete_token(tree: &mut Value, name: &str) {
    /// True when the leaf was removed and every group below `node` on the way went with it.
    fn delete_in(node: &mut Map<String, Value>, parts: &[&str]) -> bool {
        let Some((first, rest)) = parts.split_first() else {
            return false;
        };
        if rest.is_empty() {
            node.shift_remove(*first);
            return true;
        }
        let Some(Value::Object(child)) = node.get_mut(*first) else {
            return false;
        };
        if !delete_in(child, rest) || child.keys().any(|k| !k.starts_with('$')) {
            return false;
        }
        node.shift_remove(*first);
        true
    }
    let parts: Vec<&str> = name.split('.').collect();
    if let Value::Object(root) = tree {
        delete_in(root, &parts);
    }
}

/// What an import changes in one set.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetDiff {
    /// `{ token, type, value }`
    pub added: Vec<Value>,
    /// `{ token, type, from, to, descriptionOnly? }`
    pub changed: Vec<Value>,
    /// `{ token, type, value }`
    pub removed: Vec<Value>,
    /// `{ token, type }`: in dfw, missing in Penpot, kept
    pub kept: Vec<Value>,
}

impl SetDiff {
    fn to_json(&self) -> Value {
        json!({ "added": self.added, "changed": self.changed, "removed": self.removed, "kept": self.kept })
    }

    fn writes(&self) -> bool {
        !(self.added.is_empty() && self.changed.is_empty() && self.removed.is_empty())
    }
}

/// The result of [`merge_set`].
#[derive(Debug, Clone, PartialEq)]
pub struct Merged {
    pub tree: Value,
    pub diff: SetDiff,
    /// `{ set, token, type, reason }`
    pub unsupported: Vec<Value>,
    /// `{ set, token, note }`
    pub warnings: Vec<Value>,
}

/// `(a || undefined) === (b || undefined)` for two descriptions.
fn same_description(a: Option<&Value>, b: Option<&Value>) -> bool {
    js::strict_eq(js::truthy_value(a), js::truthy_value(b))
}

/// `mergeSet`: merge one Penpot set into a dfw tree. A token whose Penpot value equals what dfw
/// would export keeps dfw's exact value (lossless round trip).
pub fn merge_set(
    current: Option<&Value>,
    penpot_set: &Value,
    set: &str,
    prune: bool,
) -> Result<Merged, DfwError> {
    let empty = json!({});
    let current = js::truthy_value(current).unwrap_or(&empty);
    let mut tree = current.clone();
    let cur = flatten_tokens(current);
    let mut reserved = Vec::new();
    let incoming = walk_set(penpot_set, &mut reserved);
    let mut diff = SetDiff::default();
    let mut unsupported: Vec<Value> = reserved
        .into_iter()
        .map(|token| {
            json!({ "set": set, "token": token, "reason": "reserved name (__proto__, constructor, prototype)" })
        })
        .collect();
    let mut warnings = Vec::new();
    for (name, pt) in incoming.iter() {
        let c = cur.get(name);
        if let Some(c) = c {
            let exported = token_to_penpot_quiet(name, c);
            if exported.value().is_some_and(|v| same(v, &pt.value)) {
                if same_description(exported.description(), pt.description.as_ref()) {
                    continue; // unchanged
                }
                // Only the description was edited in Penpot: keep dfw's exact value (oklch, seconds, ...).
                set_token(
                    &mut tree,
                    name,
                    c.ty.as_ref(),
                    &c.value,
                    pt.description.as_ref(),
                );
                diff.changed.push(js::object(vec![
                    ("token", Some(name.into())),
                    ("type", c.ty.clone()),
                    ("from", Some(c.value.clone())),
                    ("to", Some(c.value.clone())),
                    ("descriptionOnly", Some(Value::Bool(true))),
                ]));
                continue;
            }
        }
        let mut notes = Vec::new();
        let converted = token_from_penpot(name, pt, c.and_then(|c| c.ty.as_ref()), &mut |w| {
            notes.push(w)
        })?;
        for note in notes {
            warnings.push(json!({ "set": set, "token": name, "note": note }));
        }
        let (ty, value, description) = match converted {
            Converted::Token {
                ty,
                value,
                description,
            } => (ty, value, description),
            Converted::Unsupported(reason) => {
                unsupported.push(js::object(vec![
                    ("set", Some(set.into())),
                    ("token", Some(name.into())),
                    ("type", pt.ty.clone()),
                    ("reason", Some(reason.into())),
                ]));
                continue;
            }
        };
        if let Some(c) = c {
            if same(&c.value, &value)
                && js::strict_eq(c.ty.as_ref(), Some(&ty))
                && same_description(c.description.as_ref(), description.as_ref())
            {
                continue;
            }
        }
        set_token(&mut tree, name, Some(&ty), &value, description.as_ref());
        match c {
            Some(c) => diff
                .changed
                .push(json!({ "token": name, "type": ty, "from": c.value, "to": value })),
            None => diff
                .added
                .push(json!({ "token": name, "type": ty, "value": value })),
        }
    }
    for (name, c) in cur.iter() {
        if incoming.has(name) {
            continue;
        }
        // Never exported (reported at export): kept without a word.
        if token_to_penpot_quiet(name, c).unsupported().is_some() {
            continue;
        }
        let entry = |with_value: bool| {
            js::object(vec![
                ("token", Some(name.into())),
                ("type", c.ty.clone()),
                ("value", with_value.then(|| c.value.clone())),
            ])
        };
        if prune {
            delete_token(&mut tree, name);
            diff.removed.push(entry(true));
        } else {
            diff.kept.push(entry(false));
        }
    }
    Ok(Merged {
        tree: js::key_order(tree),
        diff,
        unsupported,
        warnings,
    })
}

/* ------------------------------------------------------------------ project IO */

/// The tokens of a project: `tokens.json`, the themes by name, and the option themes left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DfwTokens {
    pub base: Value,
    pub themes: Map<String, Value>,
    pub skipped: Vec<String>,
}

/// `readDfwTokens`: `design/tokens/tokens.json` and the themes. `opt-*` themes (temporary
/// preset comparisons) are left out unless asked.
pub fn read_dfw_tokens(root: &Path, opt_themes: bool) -> Result<DfwTokens, DfwError> {
    let dir = project_paths(root).tokens;
    let base_file = dir.join("tokens.json");
    if !base_file.exists() {
        return Err(DfwError::new("design/tokens/tokens.json not found."));
    }
    let mut tokens = DfwTokens {
        base: js::read_json(&base_file, None)?,
        ..DfwTokens::default()
    };
    let themes_dir = dir.join("themes");
    let mut files: Vec<String> = dir_names(&themes_dir)?
        .into_iter()
        .filter(|f| f.ends_with(".json"))
        .collect();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    for file in files {
        let name = file.strip_suffix(".json").unwrap_or(&file).to_string();
        if name.starts_with("opt-") && !opt_themes {
            tokens.skipped.push(name);
        } else {
            let tree = js::read_json(&themes_dir.join(&file), None)?;
            tokens.themes.insert(name, tree);
        }
    }
    Ok(tokens)
}

/// What [`export_penpot_tokens`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenExport {
    pub file: PathBuf,
    pub json: Value,
    pub report: ExportReport,
}

/// `exportPenpotTokens`: write the project's tokens as a Penpot token file (`out` is relative
/// to the project; default `.dfw/penpot/tokens.penpot.json`).
pub fn export_penpot_tokens(
    root: &Path,
    out: Option<&str>,
    opt_themes: bool,
) -> Result<TokenExport, DfwError> {
    let tokens = read_dfw_tokens(root, opt_themes)?;
    let (json, mut report) = dfw_to_penpot(Some(&tokens.base), &tokens.themes);
    report.skipped_themes = tokens.skipped;
    let file = match out.filter(|o| !o.is_empty()) {
        Some(out) => resolve(root.join(out)),
        None => resolve(root.join(".dfw").join("penpot").join("tokens.penpot.json")),
    };
    write_json(&file, &json)?;
    Ok(TokenExport { file, json, report })
}

/// The result of [`import_penpot_tokens`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenImport {
    /// Files written, relative to the project.
    pub written: Vec<String>,
    /// The diff of every imported set, by set name.
    pub diff: Vec<(String, SetDiff)>,
    pub unsupported: Vec<Value>,
    pub warnings: Vec<Value>,
    pub skipped_sets: Vec<String>,
}

impl TokenImport {
    /// The result as the object the Node code printed.
    pub fn to_json(&self) -> Value {
        let diff: Map<String, Value> = self
            .diff
            .iter()
            .map(|(set, d)| (set.clone(), d.to_json()))
            .collect();
        json!({
            "written": self.written,
            "diff": diff,
            "unsupported": self.unsupported,
            "warnings": self.warnings,
            "skippedSets": self.skipped_sets,
        })
    }
}

/// `importPenpotTokens`: import a Penpot token file into `design/tokens`. Refused while the
/// design is frozen (G2) or the project is review-only, unless `dry_run`.
pub fn import_penpot_tokens(
    root: &Path,
    file: &Path,
    state: Option<&Value>,
    dry_run: bool,
    prune: bool,
) -> Result<TokenImport, DfwError> {
    let design = state.and_then(|s| s.get("design"));
    if !dry_run && js::truthy(design.and_then(|d| d.get("locked"))) {
        return Err(DfwError::new(format!(
            "Design v{} is approved and frozen (G2): token changes from Penpot are not applied. Show them with --dry-run, and if the user wants them, run \"dfw reopen --reason <why>\" first.",
            js::to_string(design.and_then(|d| d.get("version")))
        )));
    }
    if !dry_run && state.and_then(|s| s.get("mode")).and_then(Value::as_str) == Some("review-only")
    {
        return Err(DfwError::new(
            "This project is in review-only mode: use --dry-run to report token differences.",
        ));
    }
    let parsed = read_penpot_file(&resolve(file))?;
    let SetMap { map, skipped, .. } = map_sets(&parsed)?;
    if !map.iter().any(|(_, kind)| *kind == SetKind::Base) {
        return Err(DfwError::new(
            "No base token set found in the Penpot file (expected a set named \"base\").",
        ));
    }
    let dir = project_paths(root).tokens;
    let cur = if dir.join("tokens.json").exists() {
        read_dfw_tokens(root, true)?
    } else {
        DfwTokens {
            base: json!({}),
            ..DfwTokens::default()
        }
    };
    let mut result = TokenImport {
        skipped_sets: skipped,
        ..TokenImport::default()
    };
    let mut writes = Vec::new();
    let null = Value::Null;
    for (set, kind) in &map {
        let (current, target) = match kind {
            SetKind::Base => (Some(&cur.base), dir.join("tokens.json")),
            SetKind::Theme(name) => (
                cur.themes.get(name),
                dir.join("themes").join(format!("{name}.json")),
            ),
        };
        if let (SetKind::Theme(name), None) = (kind, current) {
            // Node bug kept: a new theme with an inherited name read the built-in function as
            // the current tokens and failed on it, before anything was written.
            if INHERITED.contains(&name.as_str()) {
                return Err(DfwError::internal("\"undefined\" is not valid JSON"));
            }
        }
        let merged = merge_set(current, parsed.set(set).unwrap_or(&null), set, prune)?;
        if merged.diff.writes() || !target.exists() {
            writes.push((target, merged.tree));
        }
        result.diff.push((set.clone(), merged.diff));
        result.unsupported.extend(merged.unsupported);
        result.warnings.extend(merged.warnings);
    }
    result.diff = js::key_order_pairs(std::mem::take(&mut result.diff));
    if !dry_run {
        for (file, tree) in &writes {
            write_json(file, tree)?;
            result.written.push(relative_path(root, file));
        }
    }
    Ok(result)
}

/// `shortValue`: a token value on one line of at most 60 characters.
pub fn short_value(v: Option<&Value>) -> String {
    let object = v.and_then(Value::as_object);
    let hex = object.and_then(|m| js::truthy_value(m.get("hex")).filter(|_| m.len() <= 4));
    let s = match (v, object, hex) {
        (Some(Value::String(s)), _, _) => s.clone(),
        (_, Some(m), Some(hex)) => match m.get("alpha") {
            Some(alpha) => format!(
                "{}/{}",
                js::to_string(Some(hex)),
                js::to_string(Some(alpha))
            ),
            None => js::to_string(Some(hex)),
        },
        (_, Some(m), None) if m.contains_key("unit") => {
            format!(
                "{}{}",
                js::to_string(m.get("value")),
                js::to_string(m.get("unit"))
            )
        }
        _ => js::stringify(v),
    };
    if js::len16(&s) > 60 {
        format!("{}...", js::slice16(&s, 0, 57))
    } else {
        s
    }
}

fn field(v: &Value, key: &str) -> String {
    js::to_string(v.get(key))
}

/// `formatImport`: the summary the CLI prints.
pub fn format_import(r: &TokenImport, dry_run: bool) -> String {
    let mut lines = Vec::new();
    for (set, d) in &r.diff {
        let n = d.added.len() + d.changed.len() + d.removed.len();
        let counts = if n > 0 {
            format!(
                "{} changed, {} added, {} removed",
                d.changed.len(),
                d.added.len(),
                d.removed.len()
            )
        } else {
            "no changes".to_string()
        };
        let kept = if d.kept.is_empty() {
            String::new()
        } else {
            format!(
                " ({} missing in Penpot, kept; --prune removes them)",
                d.kept.len()
            )
        };
        lines.push(format!("{set}: {counts}{kept}"));
        for c in &d.changed {
            lines.push(if js::truthy(c.get("descriptionOnly")) {
                format!(
                    "  ~ {}: description changed (value kept)",
                    field(c, "token")
                )
            } else {
                format!(
                    "  ~ {}: {} -> {}",
                    field(c, "token"),
                    short_value(c.get("from")),
                    short_value(c.get("to"))
                )
            });
        }
        for c in &d.added {
            lines.push(format!(
                "  + {} ({}): {}",
                field(c, "token"),
                field(c, "type"),
                short_value(c.get("value"))
            ));
        }
        for c in &d.removed {
            lines.push(format!("  - {}", field(c, "token")));
        }
    }
    if !r.skipped_sets.is_empty() {
        lines.push(format!(
            "Skipped Penpot sets (name them \"base\" or \"themes/<name>\" to import): {}",
            r.skipped_sets.join(", ")
        ));
    }
    for u in &r.unsupported {
        lines.push(format!(
            "Not imported: {} {} ({}): {}",
            field(u, "set"),
            field(u, "token"),
            field(u, "type"),
            field(u, "reason")
        ));
    }
    for w in &r.warnings {
        lines.push(format!(
            "Note: {} {}: {}",
            field(w, "set"),
            field(w, "token"),
            field(w, "note")
        ));
    }
    lines.push(if dry_run {
        "Dry run: nothing written.".to_string()
    } else if r.written.is_empty() {
        "Nothing to write.".to_string()
    } else {
        format!("Wrote {}; tokens.css rebuilt.", r.written.join(", "))
    });
    lines.join("\n")
}

/// `formatExport`: the summary the CLI prints.
pub fn format_export(export: &TokenExport, root: &Path) -> String {
    let report = &export.report;
    let rel = relative_path(root, &export.file);
    let shown = if rel.is_empty() {
        export.file.display().to_string()
    } else {
        rel
    };
    let mut lines = vec![format!(
        "Wrote {shown}: {} token(s) in Penpot format (sets: {}).",
        report.exported,
        report.sets.join(", ")
    )];
    if !report.skipped_themes.is_empty() {
        lines.push(format!(
            "Left out option themes: {} (temporary preset comparisons; --opt-themes includes them).",
            report.skipped_themes.join(", ")
        ));
    }
    for u in &report.unsupported {
        lines.push(format!(
            "Not exported: {} {} ({}): {}",
            field(u, "set"),
            field(u, "token"),
            field(u, "type"),
            field(u, "reason")
        ));
    }
    let mut lossy: Vec<(String, Vec<String>)> = Vec::new();
    for l in &report.lossy {
        let (note, token) = (field(l, "note"), field(l, "token"));
        match lossy.iter_mut().find(|(n, _)| *n == note) {
            Some(slot) => slot.1.push(token),
            None => lossy.push((note, vec![token])),
        }
    }
    for (note, tokens) in lossy {
        let more = if tokens.len() > 6 {
            format!(", +{}", tokens.len() - 6)
        } else {
            String::new()
        };
        let first: Vec<&str> = tokens.iter().take(6).map(String::as_str).collect();
        lines.push(format!("Lossy: {note}: {}{more}", first.join(", ")));
    }
    lines.push("Import it in Penpot: Tokens panel > Tools > Import (replaces the file's token sets). Re-import changes with \"dfw tokens import <file>\".".to_string());
    lines.join("\n")
}

#[cfg(test)]
#[path = "tokens_tests.rs"]
mod tests;
