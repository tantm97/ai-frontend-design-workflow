//! The dfw-owned `<style data-dfw-layout>` block of a screen: `parseLayoutCss`, `renderLayout`
//! and `setLayoutRule` of `edit.mjs`.

use super::js_regex as regex;
use super::moves::{line_start, own_lines, starts_line};
use super::style::parse_style;
use crate::core::error::DfwError;
use crate::design::html::{Token, Tokenizer};
use crate::design::tokens::js;

/// `LAYOUT_SPECIFICITY`: every layout rule is `[data-dfw-id='x']:not(#dfw)`. The `:not(#id)`
/// adds one id of specificity (1,1,0) without changing what matches, so a resize beats class,
/// attribute and type rules of the screen wherever they sit, without `!important`.
pub const LAYOUT_SPECIFICITY: &str = ":not(#dfw)";

const HAND_EDITED: &str = "The <style data-dfw-layout> block was edited by hand in a form the Studio cannot rewrite safely; fix it in code (one [data-dfw-id='...'] { ... } rule per element, optionally inside @media (max-width: Npx)).";

type Decls = Vec<(String, String)>;
type Rules = Vec<(String, Decls)>;

/// The rules of a layout block by breakpoint scope (`0` is the base scope, any other number
/// the `max-width` of a media block), each scope with its rules by element id in source order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayoutModel(Vec<(f64, Rules)>);

impl LayoutModel {
    /// The rules of one scope.
    pub fn scope(&self, max_width: f64) -> Option<&Rules> {
        self.0
            .iter()
            .find(|(w, _)| *w == max_width)
            .map(|(_, rules)| rules)
    }

    /// True when no scope has a rule.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn scope_mut(&mut self, max_width: f64) -> &mut Rules {
        let at = match self.0.iter().position(|(w, _)| *w == max_width) {
            Some(at) => at,
            None => {
                self.0.push((max_width, Vec::new()));
                self.0.len() - 1
            }
        };
        &mut self.0[at].1
    }
}

fn set(decls: &mut Decls, prop: &str, value: &str) {
    match decls.iter_mut().find(|(k, _)| k == prop) {
        Some(slot) => slot.1 = value.to_string(),
        None => decls.push((prop.to_string(), value.to_string())),
    }
}

/// `parseLayoutCss`: the rules of a layout block. Anything that is not a
/// `[data-dfw-id='...'] { ... }` rule, alone or inside `@media (max-width: Npx)`, is refused.
pub fn parse_layout_css(css: &str) -> Result<LayoutModel, DfwError> {
    // Single quotes are what dfw writes: tools that look for data-dfw-id="<id>" must find the
    // element, not this rule. The `:not(#dfw)` suffix is optional so older blocks still parse.
    let rule =
        regex(r#"\[data-dfw-id=(?:'([^'"\\]+)'|"([^'"\\]+)")\](?::not\(#dfw\))?\s*\{([^{}]*)\}"#)?;
    let media = regex(r"@media\s*\(\s*max-width\s*:\s*(\d+)px\s*\)\s*\{((?:[^{}]|\{[^{}]*\})*)\}")?;
    let comment = regex(r"(?s)/\*.*?\*/")?;
    let mut model = LayoutModel::default();
    // The rules of `chunk` go into scope `key`; what is left of the chunk is returned.
    let mut take = |chunk: &str, key: f64| -> String {
        let mut rest = String::new();
        let mut last = 0;
        for m in rule.captures_iter(chunk) {
            let all = m.get(0).map_or(0..0, |g| g.range());
            rest.push_str(&chunk[last..all.start]);
            last = all.end;
            let id = m.get(1).or(m.get(2)).map_or("", |g| g.as_str());
            let rules = model.scope_mut(key);
            let at = match rules.iter().position(|(k, _)| k == id) {
                Some(at) => at,
                None => {
                    rules.push((id.to_string(), Vec::new()));
                    rules.len() - 1
                }
            };
            for (prop, value) in parse_style(&m[3]) {
                set(&mut rules[at].1, &prop, &value);
            }
        }
        rest.push_str(&chunk[last..]);
        rest
    };
    let css = comment.replace_all(css, "");
    let mut rest = String::new();
    let mut last = 0;
    for m in media.captures_iter(&css) {
        let all = m.get(0).map_or(0..0, |g| g.range());
        rest.push_str(&css[last..all.start]);
        last = all.end;
        if !js::trim(&take(&m[2], js::str_to_number(&m[1]))).is_empty() {
            return Err(DfwError::new(HAND_EDITED));
        }
    }
    rest.push_str(&css[last..]);
    if !js::trim(&take(&rest, 0.0)).is_empty() {
        return Err(DfwError::new(HAND_EDITED));
    }
    Ok(model)
}

/// `renderLayout`: the block as dfw writes it, indented by `ind`: base rules first, then the
/// media blocks widest first so narrower ones win.
pub fn render_layout(model: &LayoutModel, ind: &str) -> String {
    let pad = format!("{ind}  ");
    let rule = |p: &str, id: &str, decls: &Decls| {
        let body: Vec<String> = decls.iter().map(|(k, v)| format!("{k}: {v};")).collect();
        format!(
            "{p}[data-dfw-id='{id}']{LAYOUT_SPECIFICITY} {{ {} }}",
            body.join(" ")
        )
    };
    let mut out = vec![
        format!("{ind}<style data-dfw-layout>"),
        format!("{pad}/* dfw Studio layout edits (drag/resize): layout properties only, one rule per element; :not(#dfw) lifts specificity over screen rules. */"),
    ];
    for (id, decls) in model.scope(0.0).into_iter().flatten() {
        out.push(rule(&pad, id, decls));
    }
    let mut widths: Vec<f64> = model
        .0
        .iter()
        .map(|(w, _)| *w)
        .filter(|w| *w != 0.0)
        .collect();
    widths.sort_by(|x, y| y.partial_cmp(x).unwrap_or(std::cmp::Ordering::Equal));
    for w in widths {
        out.push(format!("{pad}@media (max-width: {}px) {{", js::number(w)));
        for (id, decls) in model.scope(w).into_iter().flatten() {
            out.push(rule(&format!("{pad}  "), id, decls));
        }
        out.push(format!("{pad}}}"));
    }
    out.push(format!("{ind}</style>"));
    out.join("\n")
}

/// A `<style data-dfw-layout>` element: its extent and its content.
struct Block {
    start: usize,
    end: usize,
    css: std::ops::Range<usize>,
}

fn layout_blocks(text: &str) -> Vec<Block> {
    let tokens: Vec<Token> = Tokenizer::new(text).collect();
    let mut blocks = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Token::Open(tag) = token else { continue };
        let marked = tag
            .attrs
            .iter()
            .any(|a| text[a.name.clone()].eq_ignore_ascii_case("data-dfw-layout"));
        if !tag.is(text, "style") || !marked || tag.self_closing {
            continue;
        }
        let (css, close) = match tokens.get(i + 1) {
            Some(Token::RawText(r)) => (r.clone(), tokens.get(i + 2)),
            other => (tag.range.end..tag.range.end, other),
        };
        if let Some(Token::Close(close)) = close {
            blocks.push(Block {
                start: tag.range.start,
                end: close.range.end,
                css,
            });
        }
    }
    blocks
}

/// `setLayoutRule`: set (`css_value` is `Some`) or remove one layout declaration of element
/// `id`; `prop` `None` removes the element's whole rule. The block is created before `</head>`
/// and removed when it is empty. `max_width` above 0 scopes the rule to
/// `@media (max-width: <max_width>px)`.
pub fn set_layout_rule(
    text: &str,
    id: &str,
    prop: Option<&str>,
    css_value: Option<&str>,
    max_width: f64,
) -> Result<String, DfwError> {
    let mut blocks = layout_blocks(text);
    if blocks.len() > 1 {
        return Err(DfwError::new(
            "The screen has more than one <style data-dfw-layout> block; merge them in code.",
        ));
    }
    let hit = blocks.pop();
    let mut model = match &hit {
        Some(block) => parse_layout_css(&text[block.css.clone()])?,
        None => LayoutModel::default(),
    };
    let rules = model.scope_mut(max_width);
    let at = match rules.iter().position(|(k, _)| k == id) {
        Some(at) => at,
        None => {
            rules.push((id.to_string(), Vec::new()));
            rules.len() - 1
        }
    };
    match (prop, css_value) {
        (None, _) => rules[at].1.clear(),
        (Some(prop), None) => rules[at].1.retain(|(k, _)| k != prop),
        (Some(prop), Some(value)) => set(&mut rules[at].1, prop, value),
    }
    if rules[at].1.is_empty() {
        rules.remove(at);
    }
    model
        .0
        .retain(|(w, rules)| *w != max_width || !rules.is_empty());
    let empty = model.is_empty();
    if let Some(block) = hit {
        let own = own_lines(text, block.start, block.end);
        if empty {
            let (from, to) = own.map_or((block.start, block.end), |(ls, next, _)| (ls, next));
            return Ok(format!("{}{}", &text[..from], &text[to..]));
        }
        let ind = own.map_or("", |(_, _, indent)| indent);
        let rendered = render_layout(&model, ind);
        return Ok(format!(
            "{}{}{}",
            &text[..block.start],
            &rendered[ind.len()..],
            &text[block.end..]
        ));
    }
    if empty {
        return Ok(text.to_string());
    }
    let head = Tokenizer::new(text).find_map(|token| match token {
        Token::Close(tag) if tag.is(text, "head") => Some(tag.range.start),
        _ => None,
    });
    let Some(head) = head else {
        return Err(DfwError::new(
            "The screen has no </head>; layout rules go into a <style data-dfw-layout> block in the head.",
        ));
    };
    Ok(match starts_line(text, head) {
        Some(indent) => {
            let ls = line_start(text, head);
            let block = render_layout(&model, &format!("{indent}  "));
            format!("{}{block}\n{}", &text[..ls], &text[ls..])
        }
        None => format!(
            "{}\n{}\n{}",
            &text[..head],
            render_layout(&model, ""),
            &text[head..]
        ),
    })
}
