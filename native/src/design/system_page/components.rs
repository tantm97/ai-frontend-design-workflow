//! The component sections of the page: `describeParts`, `specLine`, the examples and the
//! table of variants, sizes, props, rules and tokens.

use regex::Regex;

use super::text::{sentences, DOT};
use super::tokens::Vars;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::with_js_classes;
use crate::design::compose::ComponentDef;
use crate::design::html::escape_html as esc;

/// What [`describe_parts`] makes of a master's description.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Parts {
    /// The first sentence.
    pub summary: String,
    /// The other sentences.
    pub notes: Vec<String>,
    /// The meaning of a variant, size or prop (`primary = the main action`), in the order found.
    pub meaning: Vec<(String, String)>,
}

impl Parts {
    pub(super) fn meaning_of(&self, name: &str) -> Option<&str> {
        self.meaning
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// `describeParts`: a master's description is one paragraph ("Text button. primary = the main
/// action; ghost = low emphasis. Never two primaries."). Split it into a summary, one meaning
/// per variant, size or prop, and notes. Nothing is dropped: every clause ends up in exactly
/// one of the three.
pub fn describe_parts(def: &ComponentDef) -> Result<Parts, DfwError> {
    let names: Vec<&str> = def
        .variants
        .iter()
        .chain(&def.sizes)
        .chain(&def.props)
        .map(String::as_str)
        .collect();
    describe_text(&def.description, &names)
}

/// The first match of `([\s;.(])<name>\s*=\s*((?:\([^)]*\)|[^;.()]|\.(?=\S))+)[;.]?` in
/// `text` (`head` is the part up to the value): the whole match, the leading character and the
/// value. A dot inside a value (`1.5rem`) does not end it. The value is scanned by hand because
/// the regex crate has no look-ahead; the loop is greedy and nothing after it can make it give
/// characters back (`[;.]?` is optional), so this is what the JavaScript expression matches.
fn find_meaning(
    head: &regex::Regex,
    text: &str,
) -> Option<(std::ops::Range<usize>, String, std::ops::Range<usize>)> {
    let mut from = 0;
    while let Some(m) = head.captures_at(text, from) {
        let (whole, lead) = (m.get(0)?, m.get(1)?);
        let start = whole.end();
        let mut at = start;
        loop {
            let rest = &text[at..];
            let Some(c) = rest.chars().next() else { break };
            if c == '(' {
                match rest.find(')') {
                    Some(close) => at += close + 1,
                    None => break,
                }
            } else if !matches!(c, ';' | '.' | '(' | ')') {
                at += c.len_utf8();
            } else if c == '.' && rest[1..].chars().next().is_some_and(|n| !js::is_space(n)) {
                at += 1;
            } else {
                break;
            }
        }
        if at > start {
            let end = if text[at..].starts_with([';', '.']) {
                at + 1
            } else {
                at
            };
            return Some((whole.start()..end, lead.as_str().to_string(), start..at));
        }
        // The value is empty here: the expression is tried again one character further.
        from = whole.start() + text[whole.start()..].chars().next()?.len_utf8();
    }
    None
}

/// [`describe_parts`] on the description and the names (variants, then sizes, then props).
pub fn describe_text(description: &str, names: &[&str]) -> Result<Parts, DfwError> {
    let words: Vec<&str> = description
        .split(js::is_space)
        .filter(|w| !w.is_empty())
        .collect();
    let mut text = format!(" {}", words.join(" "));
    let mut parts = Parts::default();
    let mut names = names.to_vec();
    names.sort_by_key(|n| std::cmp::Reverse(js::len16(n)));
    for name in names {
        let head = with_js_classes(&format!(r"([\s;.(]){}\s*=\s*", regex::escape(name)))?;
        let Some((all, lead, value)) = find_meaning(&head, &text) else {
            continue;
        };
        if parts.meaning_of(name).is_some() {
            continue;
        }
        parts
            .meaning
            .push((name.to_string(), js::trim(&text[value]).to_string()));
        text = format!("{}{lead}{}", &text[..all.start], &text[all.end..]);
    }
    let mut found = sentences(&text).into_iter().filter_map(|s| {
        let s = js::trim(s);
        let s = match s.strip_prefix([';', ',']) {
            Some(rest) => rest.trim_start_matches(js::is_space),
            None => s,
        };
        let weight: String = s
            .chars()
            .filter(|c| !js::is_space(*c) && !matches!(c, ';' | '.' | ','))
            .collect();
        (js::len16(&weight) > 1).then(|| s.to_string())
    });
    parts.summary = found.next().unwrap_or_default();
    parts.notes = found.collect();
    Ok(parts)
}

/// `specLine`: the key declarations of a component's root class, with token names and
/// resolved values.
pub(super) fn spec_line(def: &ComponentDef, vars: &Vars) -> Result<String, DfwError> {
    const WANT: [&str; 9] = [
        "min-height",
        "height",
        "width",
        "padding",
        "gap",
        "border-radius",
        "font",
        "font-size",
        "border",
    ];
    let class = with_js_classes(r#"class="([\w-]+)"#)?;
    let Some(cls) = class.captures(&def.template) else {
        return Ok(String::new());
    };
    let rule = with_js_classes(&format!(
        r"(?:^|[}}\s,])\.{}\s*\{{([^}}]*)\}}",
        regex::escape(&cls[1])
    ))?;
    let Some(rule) = rule.captures(&def.styles) else {
        return Ok(String::new());
    };
    let font_tail = with_js_classes(&format!(r#""[^"]*",{DOT}*$"#))?;
    let space = with_js_classes(r"\s+")?;
    let var = with_js_classes(r"var\(--([\w-]+)\)")?;
    let mut parts = String::new();
    for d in rule[1].split(';') {
        let Some((prop, val)) = d.split_once(':') else {
            continue;
        };
        let (prop, val) = (js::trim(prop), js::trim(val));
        if !WANT.contains(&prop) {
            continue;
        }
        let resolved = vars.resolve(val, 0);
        let resolved = font_tail.replace(&resolved, "");
        let resolved = space.replace_all(&resolved, " ");
        let names: Vec<&str> = var
            .captures_iter(val)
            .filter_map(|m| m.get(1).map(|g| g.as_str()))
            .collect();
        let names = names.join(", ");
        parts.push_str(&format!(
            "<tr><td><code>{}</code></td><td>{}</td><td>{}</td></tr>",
            esc(prop),
            esc(&js::slice16(js::trim(&resolved), 0, 48)),
            if names.is_empty() {
                String::new()
            } else {
                format!("<code>{}</code>", esc(&names))
            }
        ));
    }
    if parts.is_empty() {
        return Ok(String::new());
    }
    Ok(format!(
        "<table class=\"ds-spec\"><thead><tr><th>Property</th><th>Value</th><th>Token</th></tr></thead><tbody>{parts}</tbody></table>"
    ))
}

pub(super) fn auto_examples(def: &ComponentDef) -> String {
    let name = def.display_name();
    let one = [String::new()];
    let vs = if def.variants.is_empty() {
        &one[..]
    } else {
        &def.variants[..]
    };
    let ss = if def.sizes.is_empty() {
        &one[..]
    } else {
        &def.sizes[..]
    };
    let label = esc(&name.replace(['-', '_'], " "));
    let attr = |key: &str, v: &str| {
        if v.is_empty() {
            String::new()
        } else {
            format!(" {key}=\"{v}\"")
        }
    };
    let mut out = Vec::new();
    for v in vs {
        for s in ss {
            out.push(format!(
                "<dfw-use c=\"{name}\"{}{}>{label}</dfw-use>",
                attr("variant", v),
                attr("size", s)
            ));
        }
    }
    out.join("\n")
}

/// `/^([^:.;]{2,40}):\s+(.+)$/s`: a label of 2 to 40 UTF-16 units before the first colon, and
/// what follows the white space after it. Written out because the regex crate counts
/// characters where JavaScript counts UTF-16 units.
fn split_lead(t: &str) -> Option<(&str, &str)> {
    let colon = t
        .find([':', '.', ';'])
        .filter(|i| t[*i..].starts_with(':'))?;
    let label = &t[..colon];
    if !(2..=40).contains(&js::len16(label)) {
        return None;
    }
    let after = &t[colon + 1..];
    let rest = after.trim_start_matches(js::is_space);
    if rest.len() == after.len() {
        return None;
    }
    if !rest.is_empty() {
        return Some((label, rest));
    }
    // Only white space follows: the expression gives its last character to the text.
    let last = after.char_indices().next_back().map(|(i, _)| i)?;
    (last > 0).then(|| (label, &after[last..]))
}

/// The inline Markdown the page understands, and the "Label: explanation" lead.
pub(super) struct Inline {
    bold: Regex,
    code: Regex,
    because: Regex,
}

impl Inline {
    pub(super) fn new() -> Result<Inline, DfwError> {
        Ok(Inline {
            bold: with_js_classes(r"\*\*([^*]+)\*\*")?,
            code: with_js_classes(r"`([^`]+)`")?,
            because: with_js_classes(r"(?s)^(.*?)\s+(?:[-–—]\s+)?(?i-u:because)\s+(.*)$")?,
        })
    }

    /// `mdInline`: `**bold**` and `` `code` `` of an escaped text.
    pub(super) fn md(&self, t: &str) -> String {
        let t = esc(t);
        let t = self.bold.replace_all(&t, "<b>${1}</b>");
        self.code.replace_all(&t, "<code>${1}</code>").into_owned()
    }

    /// "Label: explanation" reads faster with the label in bold (a short lead before the
    /// first colon).
    pub(super) fn lead(&self, t: &str) -> String {
        match split_lead(t) {
            Some((label, rest)) => format!("<b>{}:</b> {}", esc(label), esc(rest)),
            None => self.md(t),
        }
    }

    /// A row of the "Why" table: "decision because reason" in two cells.
    pub(super) fn why(&self, x: &str) -> String {
        match self.because.captures(x) {
            Some(m) => format!(
                "<tr><td>{}</td><td>{}</td></tr>",
                self.md(&m[1]),
                self.md(&m[2])
            ),
            None => format!("<tr><td colspan=\"2\">{}</td></tr>", self.md(x)),
        }
    }
}

/// Names with a meaning get one line each; names without one share a line.
pub(super) fn named(list: &[String], parts: &Parts) -> String {
    let mut with_meaning = String::new();
    let mut plain = String::new();
    for n in list {
        match parts.meaning_of(n) {
            Some(meaning) => with_meaning.push_str(&format!(
                "<li><code>{}</code><span>{}</span></li>",
                esc(n),
                esc(meaning)
            )),
            None => plain.push_str(&format!("<code>{}</code>", esc(n))),
        }
    }
    let mut out = String::new();
    if !with_meaning.is_empty() {
        out.push_str(&format!("<ul class=\"ds-named\">{with_meaning}</ul>"));
    }
    if !plain.is_empty() {
        out.push_str(&format!("<p class=\"ds-plain\">{plain}</p>"));
    }
    out
}

pub(super) fn comp_section(
    def: &ComponentDef,
    vars: &Vars,
    inline: &Inline,
) -> Result<String, DfwError> {
    let parts = describe_parts(def)?;
    let spec = spec_line(def, vars)?;
    let row = |label: &str, html: String| {
        if html.is_empty() {
            String::new()
        } else {
            format!("<tr><th scope=\"row\">{label}</th><td>{html}</td></tr>")
        }
    };
    let name = esc(def.display_name());
    let examples = if def.examples.is_empty() {
        auto_examples(def)
    } else {
        def.examples.clone()
    };
    let preview = if def.preview == "frame" {
        // `split(/\n(?=<)/)`: one frame per example that starts a line.
        let frames: String = examples
            .replace("\n<", "\n\u{0}<")
            .split("\n\u{0}")
            .filter(|x| !js::trim(x).is_empty())
            .map(|ex| {
                format!("<div class=\"ds-frame\"><div class=\"ds-frame__inner\">{ex}</div></div>")
            })
            .collect();
        format!("<div class=\"ds-frames\">{frames}</div>")
    } else {
        format!("<div class=\"ds-examples\">{examples}</div>")
    };
    let notes: String = parts
        .notes
        .iter()
        .map(|n| format!("<li>{}</li>", inline.lead(n)))
        .collect();
    let list = |names: &[String]| {
        if names.is_empty() {
            String::new()
        } else {
            named(names, &parts)
        }
    };
    Ok(format!(
        "<section class=\"ds-comp\" data-dfw-id=\"system.component.{name}\" id=\"c-{name}\">
  <header><h3>{name}</h3><p>{summary}</p></header>
  {preview}
  <table class=\"ds-kv\">
  {variants}
  {sizes}
  {props}
  {rules}
  {tokens}
  {master}
  </table>
</section>",
        summary = esc(&parts.summary),
        variants = row("Variants", list(&def.variants)),
        sizes = row("Sizes", list(&def.sizes)),
        props = row("Props", list(&def.props)),
        rules = row(
            "Rules",
            if notes.is_empty() {
                String::new()
            } else {
                format!("<ul class=\"ds-notes\">{notes}</ul>")
            }
        ),
        tokens = row("Tokens", spec),
        master = row("Master", format!("<code>{}</code>", esc(&def.file))),
    ))
}
