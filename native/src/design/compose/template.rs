//! Filling one master: `{{...}}` placeholders, `<dfw-slot>` fills, `data-dfw-if` conditions
//! and the attributes an instance adds to the root element.

use super::*;

/// A reader over template text for the `{{...}}` placeholders.
struct Cursor<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn space(&mut self) {
        let rest = &self.text[self.pos..];
        self.pos += rest.len() - rest.trim_start_matches(js::is_space).len();
    }

    /// `[\w-]+`
    fn word(&mut self) -> Option<&'a str> {
        let rest = &self.text[self.pos..];
        let len = rest
            .bytes()
            .take_while(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            .count();
        self.pos += len;
        (len > 0).then(|| &rest[..len])
    }

    fn eat(&mut self, literal: &str) -> Option<()> {
        self.text[self.pos..].starts_with(literal).then(|| {
            self.pos += literal.len();
        })
    }
}

/// Replace every placeholder `read` recognises at a `{{`; `read` returns the replacement and
/// leaves the cursor after the placeholder.
fn replace_placeholders(text: &str, mut read: impl FnMut(&mut Cursor) -> Option<String>) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    let mut from = 0;
    while let Some(i) = text[from..].find("{{") {
        let start = from + i;
        let mut cursor = Cursor {
            text,
            pos: start + 2,
        };
        match read(&mut cursor) {
            Some(replacement) => {
                out.push_str(&text[done..start]);
                out.push_str(&replacement);
                done = cursor.pos;
                from = cursor.pos;
            }
            None => from = start + 1,
        }
    }
    out.push_str(&text[done..]);
    out
}

/// `<dfw-slot>`, `<dfw-slot name="x">` or either self-closed, exactly as the Node pattern
/// accepted it: the slot name and whether the tag closes itself.
fn slot_shape(tag: &str) -> Option<(Option<&str>, bool)> {
    let mut rest = tag.strip_prefix("<dfw-slot")?;
    let mut name = None;
    let unspaced = rest.trim_start_matches(js::is_space);
    if unspaced.len() < rest.len() {
        if let Some(after) = unspaced.strip_prefix("name=\"") {
            let end = after.find('"')?;
            let word = &after[..end];
            let valid = !word.is_empty()
                && word
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'));
            if !valid {
                return None;
            }
            name = Some(word);
            rest = &after[end + 1..];
        }
    }
    match rest.trim_start_matches(js::is_space) {
        "/>" => Some((name, true)),
        ">" => Some((name, false)),
        _ => None,
    }
}

fn fill_slots(text: &str, slots: &Attrs) -> String {
    let mut out = String::with_capacity(text.len());
    let mut done = 0;
    let mut tokens = Tokenizer::new(text);
    while let Some(token) = tokens.next() {
        let Token::Open(tag) = token else { continue };
        if tag.name(text) != "dfw-slot" {
            continue;
        }
        let Some((name, closed)) = slot_shape(&text[tag.range.clone()]) else {
            continue;
        };
        let (fallback, end) = if closed {
            ("", tag.range.end)
        } else {
            let close = tokens.clone().find_map(|t| match t {
                Token::Close(c) if &text[c.range.clone()] == "</dfw-slot>" => Some(c.range),
                _ => None,
            });
            match close {
                Some(close) => (&text[tag.range.end..close.start], close.end),
                None => continue,
            }
        };
        let filled = slots
            .get(name.unwrap_or("default"))
            .filter(|v| !js::trim(v).is_empty());
        out.push_str(&text[done..tag.range.start]);
        out.push_str(filled.unwrap_or(fallback));
        done = end;
        tokens.seek(end);
    }
    out.push_str(&text[done..]);
    out
}

/// Whether an element with `data-dfw-if="<cond>"` stays.
fn condition_holds(cond: &str, props: &Attrs) -> bool {
    // data-dfw-if="variant==focus" / "variant!=focus"
    let compare = || -> Option<bool> {
        let mut c = Cursor { text: cond, pos: 0 };
        let name = c.word()?;
        c.space();
        let equal = if c.eat("==").is_some() {
            true
        } else {
            c.eat("!=")?;
            false
        };
        c.space();
        let value = c.word()?;
        (c.pos == cond.len()).then(|| (props.get(name).unwrap_or_default() == value) == equal)
    };
    if let Some(holds) = compare() {
        return holds;
    }
    let (negated, name) = match cond.strip_prefix('!') {
        Some(name) => (true, name),
        None => (false, cond),
    };
    let truthy = props
        .get(name)
        .is_some_and(|v| !v.is_empty() && v != "false");
    truthy != negated
}

pub(super) fn fill_template(def: &ComponentDef, props: &Attrs, slots: &Attrs) -> String {
    let mut out = def.template.clone();
    // Conditionals first (on the raw template, before substitution). The Node loop gave up
    // after 200 rounds.
    for _ in 0..200 {
        let found = Tokenizer::new(&out).find_map(|token| match token {
            Token::Open(tag) => {
                let attr = tag
                    .attr(&out, "data-dfw-if")
                    .filter(|a| a.value.is_some())?
                    .clone();
                Some((tag, attr))
            }
            _ => None,
        });
        let Some((tag, attr)) = found else { break };
        let Some(el) = element_at(&out, tag.range.start) else {
            break;
        };
        let cond = crate::design::html::attr_value(&out, &attr);
        let cut = if condition_holds(&cond, props) {
            // Keep the element, drop the attribute and the white space before it. A bare value
            // runs to the next white space, as the Node pattern removed it (`=mode==x`).
            let lead = out[tag.name.end..attr.span.start].trim_end_matches(js::is_space);
            let mut end = attr.span.end;
            if attr.quote == crate::design::html::Quote::Bare {
                let rest = &out[end..tag.range.end - 1];
                end += rest.find(js::is_space).unwrap_or(rest.len());
            }
            tag.name.end + lead.len()..end
        } else {
            el.range
        };
        out.replace_range(cut, "");
    }
    // {{prop==value?text}}: text when the prop equals value (aria-current in a shared shell).
    let out = replace_placeholders(&out, |c| {
        c.space();
        let name = c.word()?;
        c.space();
        c.eat("==")?;
        c.space();
        let value = c.word()?;
        c.space();
        c.eat("?")?;
        c.space();
        let rest = &c.text[c.pos..];
        let text = &rest[..rest.find('}').unwrap_or(rest.len())];
        c.pos += text.len();
        c.eat("}}")?;
        Some(if props.get(name).unwrap_or_default() == value {
            escape_html(text)
        } else {
            String::new()
        })
    });
    // {{base}} is not a prop: it stays for compose_html to resolve to the page's path to design/.
    let keeps_base = !def.props.iter().any(|p| p == "base");
    let out = replace_placeholders(&out, |c| {
        c.space();
        let name = c.word()?;
        c.space();
        c.eat("}}")?;
        Some(if name == "base" && keeps_base {
            "{{base}}".to_string()
        } else {
            escape_html(props.get(name).unwrap_or_default())
        })
    });
    fill_slots(&out, slots)
}

/// Rewrite the first tag of `html` with `class` appended and the `extra` attributes set.
pub(super) fn annotate_root(
    html: &str,
    extra: &[(String, Option<String>)],
    class: Option<&str>,
) -> String {
    let start = html.len() - html.trim_start_matches(js::is_space).len();
    let Some(Token::Open(tag)) = Tokenizer::at(html, start).next() else {
        return html.to_string();
    };
    // The Node pattern kept a `/` right before `>` out of the attributes.
    let gt = tag.range.end - 1;
    let slash = if html[..gt].ends_with('/') { "/" } else { "" };
    let mut attrs = attrs_of(html, &parse_attrs(html, tag.name.end..gt - slash.len()));
    if let Some(class) = class.filter(|c| !c.is_empty()) {
        let joined = match attrs.nonempty("class") {
            Some(own) => format!("{own} {class}"),
            None => class.to_string(),
        };
        attrs.set("class", joined);
    }
    for (k, v) in extra {
        if let Some(v) = v {
            attrs.set(k, v.clone());
        }
    }
    format!(
        "{}<{}{}{slash}>{}",
        &html[..start],
        tag.name(html),
        attrs_to_string(&attrs),
        &html[tag.range.end..]
    )
}
