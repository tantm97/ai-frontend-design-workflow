//! What JavaScript does where Rust does something else: the one place for those rules.
//!
//! The Node code relied on JavaScript's coercions (`String(x)`, `Number(x)`, truthiness, object
//! spread), its key order of parsed objects, its white space and its regular expressions; the
//! port prints and decides what Node did by calling these, one function per rule. [`regex`]
//! translates JavaScript regular expressions for the `regex` crate.
// Over 800 lines only with its tests.

use std::cmp::Ordering;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::core::error::DfwError;
use crate::core::util::read_json as read_json_as_written;

pub mod regex;

/// What the value functions take: a JSON value, or `None` for `undefined`. Implemented for the
/// shapes call sites hold (`&Value`, `Option<&Value>`, and those behind one more reference), so
/// one function per rule serves them all.
pub trait Arg<'a> {
    /// The value, `None` for `undefined`.
    fn arg(self) -> Option<&'a Value>;
}

impl<'a> Arg<'a> for &'a Value {
    fn arg(self) -> Option<&'a Value> {
        Some(self)
    }
}

impl<'a> Arg<'a> for &&'a Value {
    fn arg(self) -> Option<&'a Value> {
        Some(*self)
    }
}

impl<'a> Arg<'a> for &'a mut Value {
    fn arg(self) -> Option<&'a Value> {
        Some(self)
    }
}

impl<'a> Arg<'a> for Option<&'a Value> {
    fn arg(self) -> Option<&'a Value> {
        self
    }
}

impl<'a> Arg<'a> for Option<&&'a Value> {
    fn arg(self) -> Option<&'a Value> {
        self.copied()
    }
}

impl<'a> Arg<'a> for Option<&'a &mut Value> {
    fn arg(self) -> Option<&'a Value> {
        self.map(|v| &**v)
    }
}

impl<'a> Arg<'a> for Option<&'a mut Value> {
    fn arg(self) -> Option<&'a Value> {
        self.map(|v| &*v)
    }
}

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
            let order: Vec<String> = entries(&map).into_iter().map(|(k, _)| k.clone()).collect();
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
    let bytes =
        fs::read(file).map_err(|err| DfwError::internal(format!("{}: {err}", file.display())))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `String(number)`, including `NaN` and `Infinity`.
pub fn number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "Infinity" } else { "-Infinity" }.into()
    } else {
        finite_number(v)
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
            // from_str_radix would accept a sign after the prefix; JavaScript does not.
            let plain = !digits.is_empty() && digits.chars().all(|c| c.is_digit(radix));
            return match u64::from_str_radix(digits, radix) {
                Ok(n) if plain => n as f64,
                _ => f64::NAN,
            };
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
    let t = text.trim_start_matches(is_space);
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

/// `String(value)` (a template literal's `${value}`); `None` is `undefined`. Takes `&Value` or
/// `Option<&Value>`.
pub fn to_string<'a>(value: impl Arg<'a>) -> String {
    match value.arg() {
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

/// Truthiness; `None` is `undefined`. Takes `&Value` or `Option<&Value>`.
pub fn truthy<'a>(value: impl Arg<'a>) -> bool {
    match value.arg() {
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
    text.trim_matches(is_space)
}

/// `text.trimEnd()`.
pub fn trim_end(text: &str) -> &str {
    text.trim_end_matches(is_space)
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
    text.split(|c: char| is_space(c) || c == ',')
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// The replacement text of `string.replace(pattern, replacement)` for a pattern without
/// capture groups: `$$`, `$&`, `` $` `` and `$'` are expanded, anything else is literal.
pub fn expand_replacement(replacement: &str, src: &str, matched: std::ops::Range<usize>) -> String {
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
        rest = rest[start + 4 + len + 3..].trim_start_matches(is_space);
    }
    out.push_str(rest);
    out
}

/// `\s` of a JavaScript regular expression (also what `String.prototype.trim` removes): U+FEFF
/// is in, U+0085 is out, unlike Rust's `char::is_whitespace`.
pub fn is_space(c: char) -> bool {
    matches!(
        c,
        '\t' | '\n' | '\u{b}' | '\u{c}' | '\r' | ' ' | '\u{a0}' | '\u{1680}' | '\u{2000}'
            ..='\u{200a}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202f}'
                | '\u{205f}'
                | '\u{3000}'
                | '\u{feff}'
    )
}

/// [`is_space`] as the inside of a `regex` crate character class (`[{SPACE}]`).
pub const SPACE: &str = r"\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}";

/// [`is_space`] as a `regex` crate character class.
pub const SPACE_CLASS: &str = r"[\t\n\x0B\x0C\r \x{a0}\x{1680}\x{2000}-\x{200a}\x{2028}\x{2029}\x{202f}\x{205f}\x{3000}\x{feff}]";

/// `Number.prototype.toString()` of a finite number (ECMAScript Number::toString).
fn finite_number(v: f64) -> String {
    if v == 0.0 {
        return "0".into();
    }
    // `{:e}` gives the shortest digits that read back as the same double: "d.ddde<exp>".
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{sign}{}", e.abs())
        } else {
            format!("{first}.{rest}e{sign}{}", e.abs())
        }
    };
    if v < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// The value when JavaScript treats it as true (`value || fallback` keeps it).
pub fn truthy_value<'a>(value: impl Arg<'a>) -> Option<&'a Value> {
    value.arg().filter(|v| truthy(Some(*v)))
}

/// `String(value || '')`.
pub fn or_empty<'a>(value: impl Arg<'a>) -> String {
    truthy_value(value).map_or_else(String::new, to_string)
}

/// `value || null`.
pub fn or_null<'a>(value: impl Arg<'a>) -> Value {
    truthy_value(value).cloned().unwrap_or(Value::Null)
}

/// `{ ...value }`: an object's own keys, an array's or a string's indexes, nothing for the rest.
pub fn spread<'a>(value: impl Arg<'a>) -> Map<String, Value> {
    match value.arg() {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v.clone()))
            .collect(),
        // JavaScript indexes a string by UTF-16 unit; half of a surrogate pair has no JSON text here.
        Some(Value::String(s)) => s
            .encode_utf16()
            .enumerate()
            .map(|(i, unit)| {
                (
                    i.to_string(),
                    Value::String(String::from_utf16_lossy(&[unit])),
                )
            })
            .collect(),
        _ => Map::new(),
    }
}

/// An object literal built from the pairs whose value exists (`undefined` fields are not
/// written). A later pair with the name of an earlier one replaces its value and keeps its
/// place, like a spread followed by the same key; a later `undefined` removes it.
pub fn object(pairs: Vec<(&str, Option<Value>)>) -> Value {
    let mut map = Map::new();
    for (key, value) in pairs {
        match value {
            Some(value) => {
                map.insert(key.to_string(), value);
            }
            None => {
                map.shift_remove(key);
            }
        }
    }
    Value::Object(map)
}

/// `JSON.stringify(value)`; `undefined` prints as the word.
pub fn stringify(value: Option<&Value>) -> String {
    value.map_or_else(
        || "undefined".to_string(),
        crate::core::util::to_json_compact,
    )
}

/// `a === b` for values out of JSON: primitives by value, objects and arrays never (two
/// parsed objects are two objects).
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) | (Some(Value::Null), Some(Value::Null)) => true,
        (Some(Value::String(a)), Some(Value::String(b))) => a == b,
        (Some(Value::Bool(a)), Some(Value::Bool(b))) => a == b,
        (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
        _ => false,
    }
}

/// `text.length`: the number of UTF-16 units.
pub fn len16(text: &str) -> usize {
    text.chars().map(char::len_utf16).sum()
}

/// `text.slice(start, end)` for indexes that are not negative.
pub fn slice16(text: &str, start: usize, end: usize) -> String {
    if text.is_ascii() {
        let end = end.min(text.len());
        return text[start.min(end)..end].to_string();
    }
    let units: Vec<u16> = text.encode_utf16().collect();
    let end = end.min(units.len());
    String::from_utf16_lossy(&units[start.min(end)..end])
}

/// `text.slice(-n)` for `n > 0`, with the same rule for a character cut in half.
pub fn suffix16(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (at, c) in s.char_indices().rev() {
        units += c.len_utf16();
        if units > n {
            return &s[at + c.len_utf8()..];
        }
    }
    s
}

/// `String(value ?? '').replace(/\s+/g, ' ').trim()`.
pub fn norm(value: Option<&Value>) -> String {
    let text = match value {
        None | Some(Value::Null) => String::new(),
        other => to_string(other),
    };
    text.split(is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// [`entries`] for a list of pairs that stands for an object.
pub fn key_order_pairs<T>(pairs: Vec<(String, T)>) -> Vec<(String, T)> {
    let (mut indexed, named): (Vec<_>, Vec<_>) = pairs
        .into_iter()
        .partition(|(k, _)| array_index(k).is_some());
    indexed.sort_by_key(|(k, _)| array_index(k));
    indexed.into_iter().chain(named).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn integer_like_keys_come_first_as_in_javascript() {
        let v: Value =
            serde_json::from_str(r#"{"half":1,"2":2,"1-5":3,"10":4,"1":5,"01":6,"4294967295":7}"#)
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
        assert_eq!(to_string(None::<&Value>), "undefined");
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

    #[test]
    fn utf16_slices() {
        assert_eq!(len16("a🚀"), 3);
        assert_eq!(slice16("a🚀b", 0, 2), "a\u{fffd}");
        assert_eq!(slice16("a🚀b", 0, 3), "a🚀");
        assert_eq!(slice16("ab", 0, 9), "ab");
        assert_eq!(suffix16("a🚀b", 2), "b");
        assert_eq!(suffix16("a🚀b", 3), "🚀b");
    }

    #[test]
    fn hex_numbers_take_no_sign_after_the_prefix() {
        assert!(str_to_number("0x+1").is_nan());
        assert!(str_to_number("0x").is_nan());
        assert!(str_to_number("-0x10").is_nan());
    }

    #[test]
    fn truthy_and_spread_take_a_value_or_an_option() {
        assert!(truthy(&json!(1)));
        assert!(!truthy(Some(&json!(""))));
        assert!(!truthy(None::<&Value>));
        assert_eq!(spread(&json!("ab")).len(), 2);
        assert_eq!(object(vec![("a", Some(json!(1))), ("a", None)]), json!({}));
    }
}
