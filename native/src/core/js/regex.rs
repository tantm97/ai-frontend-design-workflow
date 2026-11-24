//! JavaScript regular expressions for the `regex` crate.
//!
//! lint.mjs is a list of regular expressions, and its findings are compared with Node's on
//! hostile input, so each expression is kept as written and translated here instead of being
//! reworded by hand. The translation covers what JavaScript (without the `u` flag) reads
//! differently:
//! - `\d`, `\w` and `\b` are ASCII only;
//! - `\s` is JavaScript's white space (U+FEFF is in, U+0085 is out);
//! - `.` stops at `\n`, `\r`, U+2028 and U+2029;
//! - the `i` flag folds ASCII letters only (`s` does not match U+017F, `k` not U+212A);
//! - `[` inside a class is a literal.
//!
//! Not translated (no such construct in the `regex` crate): look-around and back-references;
//! the three expressions that use them are written by hand in `lint.rs` and `slop.rs`. The `m`
//! flag is refused: JavaScript's line starts differ and no expression here needs it.
//! Lengths counted in UTF-16 units (`.{20,}`, `\S{1,4}`, `slice`) are handled by the callers
//! with the helpers at the end.

use regex::Regex;

use crate::core::error::DfwError;

const WS: &str = super::SPACE;
const DOT: &str = r"[^\n\r\x{2028}\x{2029}]";
const ANY: &str = r"[\s\S]";

fn other_case(c: char) -> char {
    if c.is_ascii_lowercase() {
        c.to_ascii_uppercase()
    } else {
        c.to_ascii_lowercase()
    }
}

/// The source of a `regex` crate expression that matches what `new RegExp(source, flags)` does.
pub fn translate(source: &str, flags: &str) -> Result<String, DfwError> {
    let unsupported = |what: &str| {
        DfwError::internal(format!(
            "regular expression /{source}/{flags}: {what} is not supported"
        ))
    };
    if flags.contains('m') || flags.contains('s') || flags.contains('y') {
        return Err(unsupported("this flag"));
    }
    let fold = flags.contains('i');
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len() + 16);
    let mut in_class = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let rest: String = chars[i..].iter().take(ANY.len()).collect();
        if !in_class && rest == ANY {
            out.push_str("(?s:.)");
            i += ANY.len();
            continue;
        }
        match c {
            '\\' => {
                let Some(&e) = chars.get(i + 1) else {
                    return Err(unsupported("a trailing backslash"));
                };
                i += 2;
                match (e, in_class) {
                    ('d', false) => out.push_str("[0-9]"),
                    ('d', true) => out.push_str("0-9"),
                    ('w', false) => out.push_str("[0-9A-Za-z_]"),
                    ('w', true) => out.push_str("0-9A-Za-z_"),
                    ('s', false) => out.push_str(&format!("[{WS}]")),
                    ('s', true) => out.push_str(WS),
                    ('S', false) => out.push_str(&format!("[^{WS}]")),
                    ('b', false) => out.push_str(r"(?-u:\b)"),
                    ('/', _) => out.push('/'),
                    ('p', _) => {
                        // \p{Name}: copied whole, so the name is not case folded.
                        let end = chars[i..]
                            .iter()
                            .position(|&x| x == '}')
                            .ok_or_else(|| unsupported("\\p without a name"))?;
                        out.push_str("\\p");
                        out.extend(&chars[i..=i + end]);
                        i += end + 1;
                    }
                    (e, _) if e.is_ascii_alphanumeric() && !matches!(e, 'n' | 'r' | 't') => {
                        return Err(unsupported(&format!("\\{e}")));
                    }
                    (e, _) => {
                        out.push('\\');
                        out.push(e);
                    }
                }
                continue;
            }
            '[' if !in_class => {
                in_class = true;
                out.push('[');
                if chars.get(i + 1) == Some(&'^') {
                    out.push('^');
                    i += 1;
                }
            }
            '[' => out.push_str(r"\["),
            ']' if in_class => {
                in_class = false;
                out.push(']');
            }
            '.' if !in_class => out.push_str(DOT),
            '(' if !in_class && chars.get(i + 1) == Some(&'?') => {
                if chars.get(i + 2) != Some(&':') {
                    return Err(unsupported("look-around or a named group"));
                }
                out.push_str("(?:");
                i += 3;
                continue;
            }
            c if fold && c.is_ascii_alphabetic() => {
                if !in_class {
                    out.push('[');
                    out.push(c);
                    out.push(other_case(c));
                    out.push(']');
                } else if let (Some('-'), Some(&end)) = (chars.get(i + 1), chars.get(i + 2)) {
                    if end == ']' {
                        out.push(c);
                        out.push(other_case(c));
                    } else {
                        // A range of letters, also in the other case.
                        if !end.is_ascii_alphabetic()
                            || end.is_ascii_lowercase() != c.is_ascii_lowercase()
                        {
                            return Err(unsupported(
                                "a class range that is not letters of one case",
                            ));
                        }
                        out.extend([c, '-', end, other_case(c), '-', other_case(end)]);
                        i += 3;
                        continue;
                    }
                } else {
                    out.push(c);
                    out.push(other_case(c));
                }
            }
            c => out.push(c),
        }
        i += 1;
    }
    Ok(out)
}

/// A constant pattern written in the `regex` crate's own syntax (inline flags such as `(?i)`),
/// compiled once in a `LazyLock`. The one home of `expect` on a constant pattern: callers need no
/// `#[allow]`, and the unit tests that reach each pattern prove it compiles.
pub fn fixed_regex(source: &str) -> Regex {
    #[allow(
        clippy::expect_used,
        reason = "a constant pattern of this crate; the tests that reach it prove it compiles"
    )]
    Regex::new(source).expect("constant pattern")
}

/// `new RegExp(source, flags)`.
pub fn js_regex(source: &str, flags: &str) -> Result<Regex, DfwError> {
    Regex::new(&translate(source, flags)?)
        .map_err(|err| DfwError::internal(format!("regular expression /{source}/{flags}: {err}")))
}

/// A `regex` crate expression whose `\s`, `\d` and `\w` mean what they mean in JavaScript (the
/// crate reads them as Unicode properties). Unlike [`js_regex`], the rest is the crate's own
/// syntax (inline flags such as `(?i)` or `(?s)`): for expressions the port wrote by hand.
pub fn with_js_classes(pattern: &str) -> Result<Regex, DfwError> {
    let pattern = pattern
        .replace(r"\s", super::SPACE_CLASS)
        .replace(r"\d", "[0-9]")
        .replace(r"\w", "[A-Za-z0-9_]");
    Regex::new(&pattern).map_err(|err| DfwError::internal(err.to_string()))
}

/// A constant expression, compiled on first use.
macro_rules! js_re {
    ($name:ident, $source:expr) => {
        js_re!($name, $source, "");
    };
    ($name:ident, $source:expr, $flags:expr) => {
        static $name: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
            // The source is a literal of this crate and the unit tests of its module compile
            // every one of them (`patterns_compile`), so this cannot fail at run time.
            #[allow(
                clippy::expect_used,
                reason = "a constant pattern of this crate; the module tests compile every one (`patterns_compile`)"
            )]
            $crate::core::js::regex::js_regex($source, $flags).expect("constant regular expression")
        });
    };
}
pub(crate) use js_re;

/// Every character but `\n` replaced by spaces of the same byte length, so offsets and line
/// numbers of the text around it stay valid (`s.replace(/[^\n]/g, ' ')`).
pub fn blank_keep_lines(s: &str, out: &mut String) {
    for c in s.chars() {
        if c == '\n' {
            out.push('\n');
        } else {
            out.extend(std::iter::repeat_n(' ', c.len_utf8()));
        }
    }
}

/// `text` with every match of `re` replaced by spaces of the same byte length
/// (`text.replace(re, (s) => ' '.repeat(s.length))`: the offsets after it do not move).
pub fn blank_matches(text: &str, re: &Regex) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    for m in re.find_iter(text) {
        out.push_str(&text[done..m.start()]);
        out.extend(std::iter::repeat_n(' ', m.len()));
        done = m.end();
    }
    out.push_str(&text[done..]);
    out
}

/// CSS comments blanked, line breaks kept (`css.replace(/\/\*[\s\S]*?\*\//g, ...)`).
pub fn blank_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        let Some(len) = rest[start + 2..].find("*/") else {
            break;
        };
        let end = start + 2 + len + 2;
        out.push_str(&rest[..start]);
        blank_keep_lines(&rest[start..end], &mut out);
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_match(source: &str, flags: &str, text: &str) -> bool {
        js_regex(source, flags).unwrap().is_match(text)
    }

    #[test]
    fn classes_and_boundaries_are_ascii() {
        assert!(is_match(r"^\w+$", "", "a_1"));
        assert!(!is_match(r"^\w+$", "", "é"));
        assert!(!is_match(r"^\d$", "", "٣"));
        // "é" is not a word character in JavaScript, so there is a boundary after "a".
        assert!(is_match(r"a\b", "", "aé"));
        assert!(is_match(r"^\s$", "", "\u{feff}"));
        /// NEXT LINE: white space in Rust, not in JavaScript.
        const NEL: &str = "\u{85}";
        /// A carriage return, which `.` does not match.
        const CR: &str = "a\rb";
        assert!(!is_match(r"^\s$", "", NEL));
        assert!(is_match(r"^\S$", "", NEL));
        assert!(!is_match(r"^a.b$", "", CR));
        assert!(is_match(r"^a[\s\S]b$", "", CR));
    }

    #[test]
    fn case_folding_is_ascii() {
        assert!(is_match("<style", "i", "<STYLE"));
        assert!(!is_match("oops", "i", "oop\u{17f}"));
        assert!(!is_match("k", "i", "\u{212a}"));
        assert!(is_match("^[0-9a-f]+$", "i", "AbC09"));
        const X_OR_TIMES: &str = "^[x×]$";
        assert!(is_match(X_OR_TIMES, "i", "X"));
        assert!(is_match(X_OR_TIMES, "i", "×"));
        assert!(!is_match("^[^a]$", "i", "A"));
        assert!(is_match(r"\p{Extended_Pictographic}", "iu", "🚀"));
    }

    #[test]
    fn a_bracket_in_a_class_is_a_literal() {
        const CLASS: &str = "[[{]";
        assert!(is_match(CLASS, "", "x[y"));
        assert!(is_match(CLASS, "", "x{y"));
        assert!(!is_match(CLASS, "", "xy"));
    }

    #[test]
    fn what_cannot_be_translated_is_refused() {
        for (source, flags) in [
            (r"(?<![.])x", ""),
            (r"(a)\1", ""),
            ("^a", "m"),
            ("(?=a)", ""),
            ("[a-Z]", "i"),
        ] {
            assert!(translate(source, flags).is_err(), "{source}");
        }
    }
}
