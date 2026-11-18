//! Ports `core/lib/tokens.mjs`: DTCG tokens to CSS declarations and `tokens.css`.
//!
//! Also holds [`js`]: the JavaScript semantics the design modules need to print what Node
//! printed (number formatting, key order of parsed JSON, string coercion).
// Over 800 lines: it also carries `js` (until that moves to core::util) and their tests.

use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR_STR};

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::to_json_compact;

/// What JavaScript does where Rust does something else. The design modules print values the
/// way the Node code did, and these are the rules it relied on.
// TODO(merge): move to core::util (the key order is the exception its header describes).
pub mod js {
    use std::cmp::Ordering;
    use std::fs;
    use std::path::Path;

    use serde_json::{Map, Value};

    use crate::core::error::DfwError;
    use crate::core::util::{js_number, read_json as read_json_as_written};
    use crate::design::html::is_js_space;

    /// A key JavaScript treats as an array index: `"0"`, `"42"`, up to 2^32 - 2, no leading zero.
    fn array_index(key: &str) -> Option<u32> {
        let canonical = key == "0" || (!key.starts_with('0') && !key.is_empty());
        if !canonical || key.len() > 10 || !key.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        key.parse::<u32>().ok().filter(|n| *n < u32::MAX)
    }

    /// The entries of an object in the order JavaScript lists them: integer-like keys first, in
    /// ascending order, then the others as written.
    pub fn entries(map: &Map<String, Value>) -> Vec<(&String, &Value)> {
        let mut indexed: Vec<(u32, (&String, &Value))> = Vec::new();
        let mut named = Vec::new();
        for entry in map {
            match array_index(entry.0) {
                Some(n) => indexed.push((n, entry)),
                None => named.push(entry),
            }
        }
        indexed.sort_by_key(|(n, _)| *n);
        indexed.into_iter().map(|(_, e)| e).chain(named).collect()
    }

    /// A parsed JSON value with every object's keys in JavaScript order (see [`entries`]), so
    /// writing it back gives the file `JSON.stringify(JSON.parse(text))` gives.
    pub fn key_order(value: Value) -> Value {
        match value {
            Value::Array(items) => Value::Array(items.into_iter().map(key_order).collect()),
            Value::Object(map) => {
                let order: Vec<String> =
                    entries(&map).into_iter().map(|(k, _)| k.clone()).collect();
                let mut map = map;
                let mut out = Map::new();
                for key in order {
                    if let Some(v) = map.remove(&key) {
                        out.insert(key, key_order(v));
                    }
                }
                Value::Object(out)
            }
            other => other,
        }
    }

    /// `readJson(file, fallback)` with the keys in JavaScript order.
    pub fn read_json(file: &Path, fallback: Option<Value>) -> Result<Value, DfwError> {
        read_json_as_written(file, fallback).map(key_order)
    }

    /// `fs.readFileSync(file, 'utf8')`: bytes that are not UTF-8 become U+FFFD instead of an error.
    pub fn read_text(file: &Path) -> Result<String, DfwError> {
        let bytes = fs::read(file)
            .map_err(|err| DfwError::internal(format!("{}: {err}", file.display())))?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// `String(number)`, including `NaN` and `Infinity`.
    pub fn number(v: f64) -> String {
        if v.is_nan() {
            "NaN".into()
        } else if v.is_infinite() {
            if v > 0.0 { "Infinity" } else { "-Infinity" }.into()
        } else {
            js_number(v)
        }
    }

    /// A JavaScript number as a JSON value (`NaN` and the infinities are `null`).
    pub fn num(v: f64) -> Value {
        serde_json::Number::from_f64(v).map_or(Value::Null, Value::Number)
    }

    /// `Number(text)`.
    pub fn str_to_number(text: &str) -> f64 {
        let t = trim(text);
        if t.is_empty() {
            return 0.0;
        }
        let (sign, body) = match t.as_bytes()[0] {
            b'-' => (-1.0, &t[1..]),
            b'+' => (1.0, &t[1..]),
            _ => (1.0, t),
        };
        if body == "Infinity" {
            return sign * f64::INFINITY;
        }
        for (prefix, radix) in [
            ("0x", 16),
            ("0X", 16),
            ("0o", 8),
            ("0O", 8),
            ("0b", 2),
            ("0B", 2),
        ] {
            if let Some(digits) = t.strip_prefix(prefix) {
                return u64::from_str_radix(digits, radix).map_or(f64::NAN, |n| n as f64);
            }
        }
        let decimal = body
            .bytes()
            .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'e' | b'E' | b'+' | b'-'))
            && body.bytes().any(|b| b.is_ascii_digit());
        if !decimal || body.starts_with(['+', '-']) {
            return f64::NAN;
        }
        body.parse::<f64>().map_or(f64::NAN, |n| sign * n)
    }

    /// `Number(value)`; `None` is `undefined`.
    pub fn to_number(value: Option<&Value>) -> f64 {
        match value {
            None => f64::NAN,
            Some(Value::Null) => 0.0,
            Some(Value::Bool(b)) => f64::from(u8::from(*b)),
            Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
            Some(Value::String(s)) => str_to_number(s),
            Some(Value::Array(items)) => match items.as_slice() {
                [] => 0.0,
                [one] => str_to_number(&join(std::slice::from_ref(one), ",")),
                _ => f64::NAN,
            },
            Some(Value::Object(_)) => f64::NAN,
        }
    }

    /// `parseFloat(text)`: the longest decimal prefix, `NaN` when there is none.
    pub fn parse_float(text: &str) -> f64 {
        let t = text.trim_start_matches(is_js_space);
        let bytes = t.as_bytes();
        let mut end = usize::from(matches!(bytes.first(), Some(b'+' | b'-')));
        if t[end..].starts_with("Infinity") {
            return if bytes[0] == b'-' {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
        let digits = |from: usize| {
            bytes[from..]
                .iter()
                .take_while(|b| b.is_ascii_digit())
                .count()
        };
        let int = digits(end);
        end += int;
        let mut frac = 0;
        if bytes.get(end) == Some(&b'.') {
            frac = digits(end + 1);
            if int + frac > 0 {
                end += 1 + frac;
            }
        }
        if int + frac == 0 {
            return f64::NAN;
        }
        if matches!(bytes.get(end), Some(b'e' | b'E')) {
            let sign = usize::from(matches!(bytes.get(end + 1), Some(b'+' | b'-')));
            let exp = digits(end + 1 + sign);
            if exp > 0 {
                end += 1 + sign + exp;
            }
        }
        t[..end].parse().unwrap_or(f64::NAN)
    }

    /// `String(value)` (a template literal's `${value}`); `None` is `undefined`.
    pub fn to_string(value: Option<&Value>) -> String {
        match value {
            None => "undefined".into(),
            Some(Value::Null) => "null".into(),
            Some(Value::Bool(b)) => b.to_string(),
            Some(Value::Number(n)) => number(n.as_f64().unwrap_or(f64::NAN)),
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(items)) => join(items, ","),
            Some(Value::Object(_)) => "[object Object]".into(),
        }
    }

    /// `array.join(separator)`: `null` elements are empty.
    pub fn join(items: &[Value], separator: &str) -> String {
        let parts: Vec<String> = items
            .iter()
            .map(|v| {
                if v.is_null() {
                    String::new()
                } else {
                    to_string(Some(v))
                }
            })
            .collect();
        parts.join(separator)
    }

    /// Truthiness; `None` is `undefined`.
    pub fn truthy(value: Option<&Value>) -> bool {
        match value {
            None | Some(Value::Null) => false,
            Some(Value::Bool(b)) => *b,
            Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
            Some(Value::String(s)) => !s.is_empty(),
            Some(_) => true,
        }
    }

    /// `value ?? fallback` on an optional JSON value: `null` counts as missing.
    pub fn present(value: Option<&Value>) -> Option<&Value> {
        value.filter(|v| !v.is_null())
    }

    /// `Math.round`: halves go up.
    pub fn round(x: f64) -> f64 {
        let floor = x.floor();
        if x - floor >= 0.5 {
            floor + 1.0
        } else {
            floor
        }
    }

    /// `Math.max`: `NaN` when either is `NaN` (`f64::max` would drop it).
    pub fn max(a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else {
            a.max(b)
        }
    }

    /// `Math.min`: `NaN` when either is `NaN`.
    pub fn min(a: f64, b: f64) -> f64 {
        if a.is_nan() || b.is_nan() {
            f64::NAN
        } else {
            a.min(b)
        }
    }

    /// `x.toFixed(digits)` for `digits` up to 20 and finite `x` below 1e21.
    pub fn to_fixed(x: f64, digits: usize) -> String {
        if !x.is_finite() {
            return number(x);
        }
        // Rust rounds an exact tie to even, JavaScript away from zero. A tie at `digits`
        // decimals is an odd multiple of 2^-(digits+1), which scaling by a power of two shows
        // without rounding.
        let scaled = x.abs() * f64::from(1u32 << (digits as u32 + 1).min(30));
        let tie = scaled.fract() == 0.0 && scaled % 2.0 == 1.0;
        if !tie {
            return format!("{x:.digits$}");
        }
        let up = (x.abs() * 10f64.powi(digits as i32)).floor() + 1.0;
        let all = format!("{up:.0}");
        let all = format!("{all:0>width$}", width = digits + 1);
        let (int, frac) = all.split_at(all.len() - digits);
        let sign = if x < 0.0 { "-" } else { "" };
        if digits == 0 {
            format!("{sign}{int}")
        } else {
            format!("{sign}{int}.{frac}")
        }
    }

    /// `text.trim()`.
    pub fn trim(text: &str) -> &str {
        text.trim_matches(is_js_space)
    }

    /// `text.trimEnd()`.
    pub fn trim_end(text: &str) -> &str {
        text.trim_end_matches(is_js_space)
    }

    /// `text.padEnd(width)`: the width counts UTF-16 units.
    pub fn pad_end(text: &str, width: usize) -> String {
        let len = text.encode_utf16().count();
        format!("{text}{}", " ".repeat(width.saturating_sub(len)))
    }

    /// `text.padStart(width)`.
    pub fn pad_start(text: &str, width: usize) -> String {
        let len = text.encode_utf16().count();
        format!("{}{text}", " ".repeat(width.saturating_sub(len)))
    }

    /// Order of `Array.prototype.sort()` without a comparator: by UTF-16 code units.
    pub fn cmp_utf16(a: &str, b: &str) -> Ordering {
        a.encode_utf16().cmp(b.encode_utf16())
    }

    /// `text.split(/[\s,]+/).filter(Boolean)`.
    pub fn words(text: &str) -> Vec<String> {
        text.split(|c: char| is_js_space(c) || c == ',')
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    }

    /// The replacement text of `string.replace(pattern, replacement)` for a pattern without
    /// capture groups: `$$`, `$&`, `` $` `` and `$'` are expanded, anything else is literal.
    pub fn expand_replacement(
        replacement: &str,
        src: &str,
        matched: std::ops::Range<usize>,
    ) -> String {
        let mut out = String::with_capacity(replacement.len());
        let mut chars = replacement.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                out.push(c);
                continue;
            }
            match chars.peek() {
                Some('$') => out.push('$'),
                Some('&') => out.push_str(&src[matched.clone()]),
                Some('`') => out.push_str(&src[..matched.start]),
                Some('\'') => out.push_str(&src[matched.end..]),
                _ => {
                    out.push('$');
                    continue;
                }
            }
            chars.next();
        }
        out
    }

    /// `src.replace(pattern, replacement)` where the pattern matched `matched`.
    pub fn replace_range(src: &str, matched: std::ops::Range<usize>, replacement: &str) -> String {
        format!(
            "{}{}{}",
            &src[..matched.start],
            expand_replacement(replacement, src, matched.clone()),
            &src[matched.end..]
        )
    }

    /// `text.replace(/<!--[\s\S]*?-->\s*/g, '')`: comments and the white space after them.
    pub fn strip_comments(text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(start) = rest.find("<!--") {
            let Some(len) = rest[start + 4..].find("-->") else {
                break;
            };
            out.push_str(&rest[..start]);
            rest = rest[start + 4 + len + 3..].trim_start_matches(is_js_space);
        }
        out.push_str(rest);
        out
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use serde_json::json;

        #[test]
        fn integer_like_keys_come_first_as_in_javascript() {
            let v: Value = serde_json::from_str(
                r#"{"half":1,"2":2,"1-5":3,"10":4,"1":5,"01":6,"4294967295":7}"#,
            )
            .unwrap();
            let keys: Vec<String> = key_order(v).as_object().unwrap().keys().cloned().collect();
            assert_eq!(keys, ["1", "2", "10", "half", "1-5", "01", "4294967295"]);
        }

        #[test]
        fn numbers_print_and_parse_like_javascript() {
            assert_eq!(number(f64::NAN), "NaN");
            assert_eq!(number(-0.0), "0");
            assert_eq!(number(0.1 + 0.2), "0.30000000000000004");
            assert_eq!(str_to_number(" 12.5 "), 12.5);
            assert_eq!(str_to_number(""), 0.0);
            assert_eq!(str_to_number("0x1f"), 31.0);
            assert_eq!(str_to_number("-Infinity"), f64::NEG_INFINITY);
            for bad in ["12px", "inf", "nan", "1e", "--1", "+", "."] {
                assert!(str_to_number(bad).is_nan(), "{bad}");
            }
            assert_eq!(parse_float("0.08em"), 0.08);
            assert_eq!(parse_float("-.5e1x"), -5.0);
            assert_eq!(parse_float("3.e"), 3.0);
            assert!(parse_float("em").is_nan());
            assert_eq!(to_number(Some(&json!(true))), 1.0);
            assert_eq!(to_number(Some(&json!(["7"]))), 7.0);
            assert!(to_number(None).is_nan());
        }

        #[test]
        fn rounding_follows_javascript() {
            assert_eq!(round(2.5), 3.0);
            assert_eq!(round(-2.5), -2.0);
            assert_eq!(round(0.49999999999999994), 0.0);
            assert_eq!(to_fixed(0.125, 2), "0.13");
            assert_eq!(to_fixed(2.5, 0), "3");
            assert_eq!(to_fixed(1.005, 2), "1.00");
            assert_eq!(to_fixed(4.5, 2), "4.50");
            assert_eq!(to_fixed(21.0, 2), "21.00");
            assert_eq!(to_fixed(0.5, 1), "0.5");
            assert_eq!(to_fixed(-1.5, 0), "-2");
            assert_eq!(to_fixed(0.05, 1), "0.1");
            assert!(max(0.04, f64::NAN).is_nan());
        }

        #[test]
        fn strings_follow_javascript() {
            assert_eq!(to_string(Some(&json!([1, null, "a", [2, 3]]))), "1,,a,2,3");
            assert_eq!(to_string(Some(&json!({"a": 1}))), "[object Object]");
            assert_eq!(to_string(None), "undefined");
            assert_eq!(pad_end("ab", 4), "ab  ");
            assert_eq!(pad_start("7", 3), "  7");
            assert_eq!(trim("\u{feff} a \u{a0}"), "a");
            assert_eq!(words(" a, b\tc ,"), ["a", "b", "c"]);
            assert_eq!(expand_replacement("a$$b$&c$1$", "xMy", 1..2), "a$bMc$1$");
            assert_eq!(
                strip_comments("<!-- a -->\n<b><!-- c -->  d<!-- e"),
                "<b>d<!-- e"
            );
        }
    }
}

/// `cssVarName`: `color.accent` is `--color-accent`.
pub fn css_var_name(name: &str) -> String {
    format!("--{}", name.replace('.', "-"))
}

/// `path.relative(from, to)` for two paths (made absolute first).
pub fn relative_path(from: &Path, to: &Path) -> String {
    let (from, to) = (resolve(from), resolve(to));
    let from: Vec<_> = from.components().collect();
    let to: Vec<_> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join(MAIN_SEPARATOR_STR)
}

/// An error where the Node code threw a `TypeError` on a malformed token value.
fn malformed(what: &str) -> DfwError {
    DfwError::internal(format!("Malformed token value: {what}."))
}

struct Token<'a> {
    name: String,
    value: &'a Value,
    ty: Option<String>,
}

/// Every token of the tree by dotted name, with the `$type` it has or inherits.
fn collect(tree: &Value) -> Vec<Token<'_>> {
    fn walk<'a>(
        node: &'a Value,
        trail: &mut Vec<String>,
        ty: Option<&str>,
        out: &mut Vec<Token<'a>>,
    ) {
        let children: Vec<(String, &Value)> = match node {
            Value::Object(map) => {
                // `node.$type || type`: a truthy value that is not a string matches no type.
                let own = map.get("$type").filter(|t| js::truthy(Some(t)));
                let own = own.map(|t| t.as_str().unwrap_or_default().to_string());
                let ty = own.as_deref().or(ty);
                if let Some(value) = map.get("$value") {
                    let name = trail.join(".");
                    let token = Token {
                        name,
                        value,
                        ty: ty.map(str::to_string),
                    };
                    // A Map in the Node code: a repeated name keeps its first place.
                    match out.iter_mut().find(|t| t.name == token.name) {
                        Some(slot) => *slot = token,
                        None => out.push(token),
                    }
                    return;
                }
                for (key, child) in js::entries(map) {
                    if !key.starts_with('$') {
                        trail.push(key.clone());
                        walk(child, trail, ty, out);
                        trail.pop();
                    }
                }
                return;
            }
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
            _ => return,
        };
        for (key, child) in children {
            trail.push(key);
            walk(child, trail, ty, out);
            trail.pop();
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), None, &mut out);
    out
}

/// `fmtNumber`: integers as they are, anything else rounded to four decimals.
fn fmt_number(value: Option<&Value>) -> String {
    fmt_f64(js::to_number(value))
}

fn fmt_f64(n: f64) -> String {
    if n.is_finite() && n.fract() == 0.0 {
        js::number(n)
    } else {
        js::number(js::round(n * 10000.0) / 10000.0)
    }
}

fn alpha_suffix(alpha: Option<&Value>) -> String {
    // `a !== 1`: only the number one is left out.
    match alpha {
        Some(Value::Number(n)) if n.as_f64() == Some(1.0) => String::new(),
        None => String::new(),
        other => format!(" / {}", js::to_string(other)),
    }
}

fn color_to_css(value: Option<&Value>) -> Result<String, DfwError> {
    let fields = match value {
        Some(Value::String(s)) => return Ok(s.clone()),
        Some(Value::Object(map)) => Some(map),
        Some(Value::Array(_)) => None,
        other => {
            let shown = other.map_or("undefined".to_string(), to_json_compact);
            return Err(DfwError::internal(format!(
                "Unsupported color value {shown}"
            )));
        }
    };
    let get = |key: &str| fields.and_then(|m| m.get(key));
    let alpha = js::present(get("alpha"));
    let opaque = alpha_suffix(alpha).is_empty();
    let components: &[Value] = match get("components").filter(|c| js::truthy(Some(c))) {
        None => &[],
        Some(Value::Array(items)) => items,
        Some(_) => return Err(malformed("color components must be a list")),
    };
    let none = |v: &Value| v.as_str() == Some("none");
    let numbers = |scale: f64| -> String {
        let parts: Vec<String> = components
            .iter()
            .map(|c| {
                if none(c) {
                    "none".into()
                } else {
                    fmt_f64(js::to_number(Some(c)) * scale)
                }
            })
            .collect();
        parts.join(" ")
    };
    let plain = || -> String {
        let parts: Vec<String> = components.iter().map(|c| fmt_number(Some(c))).collect();
        parts.join(" ")
    };
    let hex = get("hex").filter(|h| js::truthy(Some(h)));
    let space = get("colorSpace");
    let a = alpha_suffix(alpha);
    Ok(match space.and_then(Value::as_str) {
        Some("srgb") => match hex {
            Some(hex) if opaque => js::to_string(Some(hex)),
            _ => format!("rgb({}{a})", numbers(255.0)),
        },
        Some("oklch") => {
            let third = match components.get(2) {
                Some(c) if none(c) => "none".to_string(),
                c => fmt_number(c),
            };
            format!(
                "oklch({} {} {third}{a})",
                fmt_number(components.first()),
                fmt_number(components.get(1))
            )
        }
        Some(name @ ("oklab" | "lab" | "lch" | "hsl" | "hwb")) => format!("{name}({}{a})", plain()),
        _ => match hex {
            Some(hex) => js::to_string(Some(hex)),
            None => format!("color({} {}{a})", js::to_string(space), plain()),
        },
    })
}

/// `{color.accent}` references in a string value become `var(--color-accent)`.
fn aliases_to_css(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let body = &rest[open + 1..];
        let stop = body.find(['{', '}']);
        match stop {
            Some(len) if len > 0 && body[len..].starts_with('}') => {
                out.push_str(&rest[..open]);
                out.push_str(&format!("var({})", css_var_name(&body[..len])));
                rest = &body[len + 1..];
            }
            _ => {
                out.push_str(&rest[..=open]);
                rest = body;
            }
        }
    }
    out.push_str(rest);
    out
}

fn field<'a>(
    value: Option<&'a Value>,
    key: &str,
    what: &str,
) -> Result<Option<&'a Value>, DfwError> {
    match value {
        Some(Value::Object(map)) => Ok(map.get(key)),
        Some(Value::Null) | None => Err(malformed(what)),
        Some(_) => Ok(None),
    }
}

fn list<'a>(value: Option<&'a Value>, what: &str) -> Result<&'a [Value], DfwError> {
    match value {
        Some(Value::Array(items)) => Ok(items),
        _ => Err(malformed(what)),
    }
}

// One match over the DTCG types, as the switch of tokens.mjs (over 120 lines once formatted).
fn value_to_css(value: Option<&Value>, ty: &str) -> Result<String, DfwError> {
    if let Some(Value::String(text)) = value {
        return Ok(aliases_to_css(text));
    }
    let zero = |unit: &str| serde_json::json!({ "value": 0, "unit": unit });
    Ok(match ty {
        "color" => color_to_css(value)?,
        "dimension" | "duration" => match value {
            Some(Value::Object(map)) if map.contains_key("value") => {
                format!(
                    "{}{}",
                    fmt_number(map.get("value")),
                    js::to_string(map.get("unit"))
                )
            }
            Some(Value::Null) => return Err(malformed("a dimension is null")),
            other => js::to_string(other),
        },
        "number" | "fontWeight" => js::to_string(value),
        "fontFamily" => {
            let single = [value.cloned().unwrap_or(Value::Null)];
            let families = match value {
                Some(Value::Array(items)) => items.as_slice(),
                _ => &single,
            };
            let parts: Vec<String> = families
                .iter()
                .map(|f| {
                    let name = js::to_string(Some(f));
                    if name.contains(|c: char| crate::design::html::is_js_space(c) || c == ',') {
                        format!("\"{name}\"")
                    } else if f.is_null() {
                        String::new()
                    } else {
                        name
                    }
                })
                .collect();
            parts.join(", ")
        }
        "cubicBezier" => {
            let parts: Vec<String> = list(value, "a cubicBezier must be a list")?
                .iter()
                .map(|n| fmt_number(Some(n)))
                .collect();
            format!("cubic-bezier({})", parts.join(", "))
        }
        "shadow" => {
            // Blur and spread are optional in CSS; a missing one must not print "undefined".
            let len = |v: Option<&Value>| {
                value_to_css(Some(js::present(v).unwrap_or(&zero("px"))), "dimension")
            };
            let one = |shadow: &Value| -> Result<String, DfwError> {
                let get = |key: &str| field(Some(shadow), key, "a shadow layer is null");
                let parts = [
                    if js::truthy(get("inset")?) {
                        "inset".to_string()
                    } else {
                        String::new()
                    },
                    len(get("offsetX")?)?,
                    len(get("offsetY")?)?,
                    len(get("blur")?)?,
                    match js::present(get("spread")?) {
                        Some(spread) => len(Some(spread))?,
                        None => String::new(),
                    },
                    value_to_css(get("color")?, "color")?,
                ];
                Ok(parts
                    .into_iter()
                    .filter(|p| !p.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "))
            };
            match value {
                Some(Value::Array(layers)) => layers
                    .iter()
                    .map(one)
                    .collect::<Result<Vec<_>, _>>()?
                    .join(", "),
                Some(single) => one(single)?,
                None => return Err(malformed("a shadow has no value")),
            }
        }
        "border" => {
            let get = |key: &str| field(value, key, "a border is null");
            format!(
                "{} {} {}",
                value_to_css(get("width")?, "dimension")?,
                js::to_string(get("style")?),
                value_to_css(get("color")?, "color")?
            )
        }
        "transition" => {
            let get = |key: &str| field(value, key, "a transition is null");
            let delay = zero("ms");
            format!(
                "{} {} {}",
                value_to_css(get("duration")?, "duration")?,
                value_to_css(get("timingFunction")?, "cubicBezier")?,
                value_to_css(
                    Some(js::present(get("delay")?).unwrap_or(&delay)),
                    "duration"
                )?
            )
        }
        // A string was returned above; every other shape is "dashed".
        "strokeStyle" => "dashed".to_string(),
        "gradient" => {
            let stops = list(value, "a gradient must be a list of stops")?;
            let parts = stops
                .iter()
                .map(|stop| {
                    let get = |key: &str| field(Some(stop), key, "a gradient stop is null");
                    let position = js::to_number(get("position")?) * 100.0;
                    Ok(format!(
                        "{} {}%",
                        value_to_css(get("color")?, "color")?,
                        fmt_f64(position)
                    ))
                })
                .collect::<Result<Vec<_>, DfwError>>()?;
            parts.join(", ")
        }
        _ => match value {
            Some(Value::Number(n)) => js::number(n.as_f64().unwrap_or(f64::NAN)),
            Some(other) => to_json_compact(other),
            None => "undefined".to_string(),
        },
    })
}

/// `tokensToDeclarations`: `(css variable, css value)` pairs in the order JavaScript walks the
/// tree; a typography token expands to one variable per sub-property.
pub fn tokens_to_declarations(tree: &Value) -> Result<Vec<(String, String)>, DfwError> {
    const TYPOGRAPHY: [(&str, &str, &str); 5] = [
        ("fontFamily", "font-family", "fontFamily"),
        ("fontSize", "font-size", "dimension"),
        ("fontWeight", "font-weight", "fontWeight"),
        ("letterSpacing", "letter-spacing", "dimension"),
        ("lineHeight", "line-height", "number"),
    ];
    let mut decls = Vec::new();
    for token in collect(tree) {
        if token.value.is_null() {
            return Err(DfwError::new(format!(
                "Token \"{}\" has no value ($value is null).",
                token.name
            )));
        }
        let ty = token.ty.as_deref().unwrap_or_default();
        let var = css_var_name(&token.name);
        if let (true, Value::Object(parts)) = (ty == "typography", token.value) {
            for (key, suffix, part_type) in TYPOGRAPHY {
                if let Some(part) = parts.get(key) {
                    decls.push((
                        format!("{var}-{suffix}"),
                        value_to_css(Some(part), part_type)?,
                    ));
                }
            }
            continue;
        }
        decls.push((var, value_to_css(Some(token.value), ty)?));
    }
    Ok(decls)
}

/// What [`build_tokens`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltTokens {
    /// Path of `design/tokens/tokens.css`.
    pub out: PathBuf,
    /// Names of the themes found in `design/tokens/themes/`.
    pub themes: Vec<String>,
}

/// `.json` file names of a directory in the order Node lists them (sorted by bytes), without
/// the extension.
pub fn json_names(dir: &Path) -> Result<Vec<String>, DfwError> {
    Ok(dir_names(dir)?
        .into_iter()
        .filter_map(|f| f.strip_suffix(".json").map(str::to_string))
        .collect())
}

/// `fs.readdirSync(dir)`: the entry names, sorted by bytes as libuv returns them. A missing
/// directory is an empty list here; callers check `exists()` where Node did.
pub fn dir_names(dir: &Path) -> Result<Vec<String>, DfwError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(DfwError::internal(format!("{}: {err}", dir.display()))),
    };
    let mut names = Vec::new();
    for entry in entries {
        names.push(entry?.file_name().to_string_lossy().into_owned());
    }
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    Ok(names)
}

/// `buildTokens`: write `design/tokens/tokens.css` from `tokens.json` and one
/// `[data-theme="<name>"]` block per `design/tokens/themes/<name>.json`.
pub fn build_tokens(root: &Path) -> Result<BuiltTokens, DfwError> {
    let dir = project_paths(root).tokens;
    let base = dir.join("tokens.json");
    if !base.exists() {
        return Err(DfwError::new("design/tokens/tokens.json not found."));
    }
    let parse = |file: &Path| -> Result<Value, DfwError> {
        let text = js::read_text(file)?;
        serde_json::from_str(&text)
            .map_err(|err| DfwError::new(format!("{}: {err}", relative_path(root, file))))
    };
    let block = |selector: &str, tree: &Value| -> Result<String, DfwError> {
        let lines: Vec<String> = tokens_to_declarations(tree)?
            .iter()
            .map(|(k, v)| format!("  {k}: {v};"))
            .collect();
        Ok(format!("{selector} {{\n{}\n}}\n", lines.join("\n")))
    };
    let mut css = String::from("/* Generated by \"dfw tokens build\" from design/tokens/*.json (W3C DTCG). Do not edit by hand. */\n");
    css.push_str(&block(":root", &parse(&base)?)?);
    let themes_dir = dir.join("themes");
    let themes = json_names(&themes_dir)?;
    for name in &themes {
        css.push('\n');
        css.push_str(&block(
            &format!("[data-theme=\"{name}\"]"),
            &parse(&themes_dir.join(format!("{name}.json")))?,
        )?);
    }
    let out = dir.join("tokens.css");
    fs::write(&out, css)?;
    Ok(BuiltTokens { out, themes })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use serde_json::json;

    fn decls(tree: Value) -> Vec<(String, String)> {
        tokens_to_declarations(&tree).unwrap()
    }

    fn value_of(tree: Value, var: &str) -> String {
        decls(tree).into_iter().find(|(k, _)| k == var).unwrap().1
    }

    /// tokens: DTCG 2025.10 values and aliases (tests/unit/dfw.test.mjs; the lintTokens
    /// assertions belong to design/lint)
    #[test]
    fn dtcg_values_and_aliases() {
        let tree = json!({
            "color": {
                "$type": "color",
                "blue": { "$value": { "colorSpace": "srgb", "components": [0, 0.4, 1], "hex": "#0066ff" } },
                "accent": { "$value": "{color.blue}" },
                "soft": { "$value": { "colorSpace": "oklch", "components": [0.9, 0.05, 250], "alpha": 0.5 } },
            },
            "space": { "$type": "dimension", "4": { "$value": { "value": 16, "unit": "px" } } },
            "font": { "body": { "$type": "fontFamily", "$value": ["Source Sans 3", "sans-serif"] } },
            "shadow": { "card": { "$type": "shadow", "$value": { "color": "{color.blue}", "offsetX": { "value": 0, "unit": "px" }, "offsetY": { "value": 1, "unit": "px" }, "blur": { "value": 2, "unit": "px" }, "spread": { "value": 0, "unit": "px" } } } },
        });
        let d: std::collections::HashMap<String, String> = decls(tree).into_iter().collect();
        assert_eq!(d["--color-blue"], "#0066ff");
        assert_eq!(d["--color-accent"], "var(--color-blue)");
        assert_eq!(d["--color-soft"], "oklch(0.9 0.05 250 / 0.5)");
        assert_eq!(d["--space-4"], "16px");
        assert_eq!(d["--font-body"], "\"Source Sans 3\", sans-serif");
        assert_eq!(d["--shadow-card"], "0px 1px 2px 0px var(--color-blue)");
    }

    /// tokens: a shadow without blur/spread prints valid CSS; a null $value is a clear error
    /// (tests/unit/audit-fixes.test.mjs)
    #[test]
    fn shadow_without_blur_or_spread_and_null_value() {
        let px = |value: i32| json!({ "value": value, "unit": "px" });
        let decl = |shadow: Value| {
            value_of(
                json!({ "shadow": { "$type": "shadow", "sm": { "$value": shadow } } }),
                "--shadow-sm",
            )
        };
        assert_eq!(
            decl(json!({ "offsetX": px(0), "offsetY": px(1), "blur": px(2), "color": "#000000" })),
            "0px 1px 2px #000000"
        );
        assert_eq!(
            decl(
                json!({ "offsetX": px(0), "offsetY": px(1), "spread": px(3), "color": "#000000" })
            ),
            "0px 1px 0px 3px #000000"
        );
        let err = tokens_to_declarations(
            &json!({ "space": { "$type": "dimension", "a": { "$value": null } } }),
        )
        .unwrap_err();
        assert!(
            err.message().contains("Token \"space.a\" has no value"),
            "{err}"
        );
        assert_eq!(err.exit_code(), 1);
    }

    /// penpot tokens: a math expression keeps its aliases (the tokensToDeclarations assertion
    /// of tests/unit/penpot-tokens.test.mjs)
    #[test]
    fn aliases_inside_an_expression() {
        let tree =
            json!({ "space": { "8": { "$type": "dimension", "$value": "calc({space.4} * 2)" } } });
        assert_eq!(value_of(tree, "--space-8"), "calc(var(--space-4) * 2)");
        assert_eq!(
            aliases_to_css("{a.b}{c}{}{d{e.f}} {"),
            "var(--a-b)var(--c){}{dvar(--e-f)} {"
        );
    }

    #[test]
    fn declarations_follow_javascript_key_order_and_number_format() {
        let tree: Value = serde_json::from_str(
            r##"{"space":{"$type":"dimension","half":{"$value":{"value":2,"unit":"px"}},"2":{"$value":{"value":8.0,"unit":"px"}},"1":{"$value":{"value":4.123456,"unit":"px"}}},
                "n":{"$type":"number","$value":1.50},"w":{"$type":"fontWeight","$value":600},
                "ease":{"$type":"cubicBezier","$value":[0.2,0,0,1]},
                "c":{"$type":"color","a":{"$value":{"colorSpace":"srgb","components":[1,0.5,"none"],"alpha":0.5}},"b":{"$value":{"colorSpace":"display-p3","components":[1,0,0]}},"h":{"$value":{"colorSpace":"hsl","components":[210,50,40]}}},
                "t":{"$type":"typography","$value":{"fontFamily":["Inter","sans-serif"],"fontSize":{"value":16,"unit":"px"},"fontWeight":600,"lineHeight":1.4}},
                "b":{"$type":"border","$value":{"width":{"value":1,"unit":"px"},"style":"solid","color":"{c.a}"}},
                "tr":{"$type":"transition","$value":{"duration":{"value":120,"unit":"ms"},"timingFunction":[0,0,1,1]}},
                "g":{"$type":"gradient","$value":[{"color":"#000","position":0},{"color":"#fff","position":0.5}]},
                "s":{"$type":"string","$value":true},"list":[{"$type":"number","$value":3}]}"##,
        )
        .unwrap();
        let got: Vec<String> = decls(tree)
            .into_iter()
            .map(|(k, v)| format!("{k}: {v}"))
            .collect();
        assert_eq!(
            got,
            [
                "--space-1: 4.1235px",
                "--space-2: 8px",
                "--space-half: 2px",
                "--n: 1.5",
                "--w: 600",
                "--ease: cubic-bezier(0.2, 0, 0, 1)",
                "--c-a: rgb(255 127.5 none / 0.5)",
                "--c-b: color(display-p3 1 0 0)",
                "--c-h: hsl(210 50 40)",
                "--t-font-family: Inter, sans-serif",
                "--t-font-size: 16px",
                "--t-font-weight: 600",
                "--t-line-height: 1.4",
                "--b: 1px solid var(--c-a)",
                "--tr: 120ms cubic-bezier(0, 0, 1, 1) 0ms",
                "--g: #000 0%, #fff 50%",
                "--s: true",
                "--list-0: 3",
            ]
        );
    }

    #[test]
    fn build_tokens_writes_root_and_theme_blocks() {
        let root = test_dir("tokens");
        let dir = root.join("design/tokens");
        assert!(build_tokens(&root)
            .unwrap_err()
            .message()
            .contains("tokens.json not found"));
        fs::create_dir_all(dir.join("themes")).unwrap();
        fs::write(
            dir.join("tokens.json"),
            r##"{"color":{"$type":"color","ink":{"$value":"#111111"}}}"##,
        )
        .unwrap();
        fs::write(
            dir.join("themes/dark.json"),
            r##"{"color":{"$type":"color","ink":{"$value":"#fafafa"}}}"##,
        )
        .unwrap();
        fs::write(dir.join("themes/notes.txt"), "x").unwrap();
        let built = build_tokens(&root).unwrap();
        assert_eq!(built.themes, ["dark"]);
        assert_eq!(
            fs::read_to_string(&built.out).unwrap(),
            "/* Generated by \"dfw tokens build\" from design/tokens/*.json (W3C DTCG). Do not edit by hand. */\n:root {\n  --color-ink: #111111;\n}\n\n[data-theme=\"dark\"] {\n  --color-ink: #fafafa;\n}\n"
        );
        fs::write(dir.join("themes/dark.json"), "{").unwrap();
        let err = build_tokens(&root).unwrap_err();
        assert!(
            err.message()
                .starts_with(&relative_path(&root, &dir.join("themes/dark.json"))),
            "{err}"
        );
        assert_eq!(
            relative_path(Path::new("/a/b/c"), Path::new("/a/d/e.txt")),
            ["..", "..", "d", "e.txt"].join(MAIN_SEPARATOR_STR)
        );
    }
}
