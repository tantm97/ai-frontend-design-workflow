//! The JavaScript text rules the page depends on: UTF-16 slices, multi-line expressions the
//! regex crate cannot express (look-behind, `^` after any line terminator), `encodeURI`.

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::with_js_classes;
use crate::design::tokens::key as dtcg_key;

/// `.` of a JavaScript regular expression: anything but a line terminator.
pub(super) const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";

pub(super) fn is_line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// Where a line starts for `^` with the `m` flag: byte 0 and after every line terminator.
pub(super) fn line_starts(text: &str) -> impl Iterator<Item = usize> + '_ {
    std::iter::once(0).chain(
        text.char_indices()
            .filter(|(_, c)| is_line_end(*c))
            .map(|(i, c)| i + c.len_utf8()),
    )
}

/// `text.split(/^(?=## )/m)`: the parts of a Markdown text, each from one `## ` heading.
pub(super) fn md_parts(text: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut last = 0;
    for at in line_starts(text) {
        if at > 0 && text[at..].starts_with("## ") {
            parts.push(&text[last..at]);
            last = at;
        }
    }
    parts.push(&text[last..]);
    parts
}

/// `new RegExp('^## <name>\\s*$', 'm').test(part.split('\n')[0])` for any of `names`.
pub(super) fn has_heading(part: &str, names: &[&str]) -> bool {
    let first = part.split('\n').next().unwrap_or_default();
    line_starts(first).any(|at| {
        names.iter().any(|name| {
            let Some(rest) = first[at..]
                .strip_prefix("## ")
                .and_then(|r| r.strip_prefix(name))
            else {
                return false;
            };
            // `\s*$`: the white space runs to the end, or holds a line terminator to stop at.
            let after = rest.trim_start_matches(js::is_space);
            after.is_empty() || rest[..rest.len() - after.len()].contains(is_line_end)
        })
    })
}

/// The body of a part: everything after its first line.
pub(super) fn part_body(part: &str) -> &str {
    part.split_once('\n').map_or("", |(_, body)| body)
}

/// `text.match(/^\s*[-*] .{min,}/gm)`: the bullets of a Markdown text, each without its marker
/// and up to the end of its line; `min` is the least number of UTF-16 units a bullet has.
pub(super) fn bullets(text: &str, min: usize) -> Vec<&str> {
    let mut out = Vec::new();
    let mut done = 0;
    for at in line_starts(text) {
        if at < done {
            continue;
        }
        let rest = text[at..].trim_start_matches(js::is_space);
        let Some(item) = rest
            .strip_prefix(['-', '*'])
            .and_then(|r| r.strip_prefix(' '))
        else {
            continue;
        };
        let item = &item[..item.find(is_line_end).unwrap_or(item.len())];
        if js::len16(item) >= min.max(1) {
            done = text.len() - rest.len() + 2 + item.len();
            out.push(item);
        }
    }
    out
}

/// `text.split(/(?<=[.!?])\s+(?=[^a-z(])/)`: sentences. White space after `.`, `!` or `?`
/// splits when what follows is not a lower-case letter or `(`; before one of those, a run of
/// two or more still splits and leaves its last character (the expression backs off by one).
pub(super) fn sentences(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut last = 0;
    let mut prev: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if !js::is_space(c) || !matches!(prev, Some('.' | '!' | '?')) {
            prev = Some(c);
            continue;
        }
        let mut end = i + c.len_utf8();
        let mut last_space = i;
        while let Some((j, d)) = chars.peek().copied().filter(|(_, d)| js::is_space(*d)) {
            last_space = j;
            end = j + d.len_utf8();
            chars.next();
        }
        let next = text[end..].chars().next();
        let stop = match next {
            Some(n) if !n.is_ascii_lowercase() && n != '(' => Some(end),
            _ if last_space > i => Some(last_space),
            _ => None,
        };
        if let Some(stop) = stop {
            out.push(&text[last..i]);
            last = stop;
        }
        prev = Some(' ');
    }
    out.push(&text[last..]);
    out
}

/// `encodeURI`.
pub(super) fn encode_uri(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b";/?:@&=+$,-_.!~*'()#".contains(&b) {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// A token alias `{name}`: the name.
pub(super) fn alias(value: &Value) -> Option<&str> {
    let inner = value.as_str()?.strip_prefix('{')?.strip_suffix('}')?;
    (!inner.is_empty() && !inner.contains('}')).then_some(inner)
}

/// `dim`: a dimension `{value, unit}` as `16px`, anything else as text.
pub(super) fn dim(v: &Value) -> String {
    match v {
        Value::Object(map) if map.contains_key(dtcg_key::AMOUNT) => format!(
            "{}{}",
            js::to_string(map.get(dtcg_key::AMOUNT)),
            js::to_string(map.get(dtcg_key::UNIT))
        ),
        other => js::to_string(Some(other)),
    }
}

/// `value?.hex || null`.
pub(super) fn hex_field(value: &Value) -> Option<String> {
    let hex = value.as_object().and_then(|o| o.get(dtcg_key::HEX));
    js::truthy(hex).then(|| js::to_string(hex))
}

/// `v.slice(1, -1)`.
pub(super) fn slice_inner(v: &str) -> &str {
    let mut chars = v.chars();
    chars.next();
    chars.next_back();
    chars.as_str()
}

/// `text.replace(/\s*<marker>.*$/, '')`: cut a note that runs to the end of the text.
pub(super) fn cut_note(text: &str, marker: &str) -> Result<String, DfwError> {
    let re = with_js_classes(&format!(r"\s*{}{DOT}*$", regex::escape(marker)))?;
    Ok(re.replace(text, "").into_owned())
}
