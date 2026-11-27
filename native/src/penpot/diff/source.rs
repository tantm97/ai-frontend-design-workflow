//! Where a master change lands (`masterSource`): the CSS rule, template element or component
//! tag in the master's file.

use super::*;
use crate::core::js::regex::fixed_regex;
use crate::design::compose::{attr, ComponentDef};
use crate::design::html::{self, Tag};

/// The CSS properties a change type is made with.
fn css_property(change: &Value) -> Option<&'static str> {
    Some(match change.get(field::TYPE).and_then(Value::as_str)? {
        change_type::FILL if kind(change) == Some(wire::kind::TEXT) => "color",
        change_type::FILL => "background(-color)?",
        change_type::STROKE => "border(-color|-width|-style)?|outline",
        change_type::RADIUS => "border-radius|border-(top|bottom)-(left|right)-radius",
        change_type::SPACING => "padding(-top|-right|-bottom|-left|-inline|-block)?|gap",
        change_type::SIZE => "height|width|min-height|min-width|block-size|inline-size",
        _ => return None,
    })
}

static CSS_RULE: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"([^{}<>]+)\{([^{}]*)\}"));
static STATE_SELECTOR: LazyLock<Regex> = LazyLock::new(|| {
    fixed_regex(r":(hover|focus|focus-visible|focus-within|active|disabled|checked)|\[(aria|data)-")
});
static TEXT_SELECTOR: LazyLock<Regex> =
    LazyLock::new(|| fixed_regex(r"(?-u:\b)(span|p|label|strong)(?-u:\b)"));
static CLASS_NAME: LazyLock<Regex> =
    LazyLock::new(|| fixed_regex(r"(?i)^[a-z][0-9A-Za-z_-]*\.([0-9A-Za-z_-]+)$"));

/// Where a master change is made: the file, and `file:line` when the file exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterSource {
    pub file: String,
    pub source: Option<String>,
}

fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    text.get(from..)?.find(needle).map(|i| i + from)
}

/// True when `word` is in `text` with an ASCII word boundary on both sides, ignoring ASCII case
/// (`/\bword\b/i`).
fn has_word(text: &str, word: &str) -> bool {
    let is_word = |b: Option<&u8>| b.is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
    let text = text.to_ascii_lowercase();
    let word = word.to_ascii_lowercase();
    let bytes = text.as_bytes();
    let boundary =
        |at: usize| is_word(at.checked_sub(1).and_then(|i| bytes.get(i))) != is_word(bytes.get(at));
    !word.is_empty()
        && text
            .match_indices(&word)
            .any(|(at, _)| boundary(at) && boundary(at + word.len()))
}

/// `masterSource`: `design/system/components/<file>:<line>` for a master change: the CSS rule
/// that sets the property (variant/size rules first), else the element in the template (by
/// tag/class), else the component tag. `None` when the change is not on a component.
pub fn master_source(root: &Path, change: &Value, lib: Option<&Library>) -> Option<MasterSource> {
    let comp = js::truthy_value(change.get(field::COMPONENT))?;
    let comp_name = comp.get(wire::NAME).and_then(Value::as_str);
    let loaded;
    let lib = match lib {
        Some(lib) => Some(lib),
        None => {
            loaded = load_components(root).ok();
            loaded.as_ref()
        }
    };
    let def = lib.and_then(|lib| lib.get(comp_name));
    let rel = master_file(def, comp);
    let abs = root.join(&rel);
    if !abs.exists() {
        return Some(MasterSource {
            file: rel,
            source: None,
        });
    }
    let text = js::read_text(&abs).ok()?;
    let start = master_start(&text, def);
    let block = &text[..find_from(&text, "</dfw-component>", start).unwrap_or(text.len())];
    let name =
        js::truthy_value(change.get(wire::NAME)).map_or(String::new(), |n| js::to_string(Some(n)));
    let cls = CLASS_NAME.captures(&name).map(|m| m[1].to_string());
    let offset = match css_rule_offset(block, start, change, comp, cls.as_deref())? {
        Some(i) => i,
        None => template_offset(block, start, change, cls.as_deref()).unwrap_or(start),
    };
    let line = text[..offset.min(text.len())].matches('\n').count() + 1;
    Some(MasterSource {
        source: Some(format!("{rel}:{line}")),
        file: rel,
    })
}

/// The master's file, relative to the project: the library's, else named after the component.
fn master_file(def: Option<&ComponentDef>, comp: &Value) -> String {
    match def.map(|d| d.file.as_str()).filter(|f| !f.is_empty()) {
        Some(file) => file.to_string(),
        None => rel::master(&js::to_string(comp.get(wire::NAME))),
    }
}

/// Where the master block starts in its file.
fn master_start(text: &str, def: Option<&ComponentDef>) -> usize {
    match def.map(|d| d.line).filter(|line| *line > 0) {
        // The end of the line before the master's, as the Node code counted it.
        Some(1) => 0,
        Some(line) => text
            .match_indices('\n')
            .nth(line - 2)
            .map_or(text.len(), |(i, _)| i),
        None => text.find("<dfw-component").unwrap_or(0),
    }
}

/// The selector of the best scored CSS rule in the master's `<style>` that sets the change's
/// property (variant, size, class and text selectors up, state selectors down). `Some(None)`
/// when no rule fits; `None` when the declaration pattern cannot be built (the lookup stops).
fn css_rule_offset(
    block: &str,
    start: usize,
    change: &Value,
    comp: &Value,
    cls: Option<&str>,
) -> Option<Option<usize>> {
    let Some(prop) = css_property(change) else {
        return Some(None);
    };
    let style_at = find_from(block, "<style", start);
    let style_end = style_at.and_then(|s| find_from(block, "</style>", s));
    let (Some(style_at), Some(style_end)) = (style_at, style_end.filter(|e| *e > 0)) else {
        return Some(None);
    };
    let is_text_change = kind(change) == Some(wire::kind::TEXT);
    let css = &block[style_at..style_end];
    let decl = Regex::new(&format!(r"(^|[;{{{s}])({prop})[{s}]*:", s = js::SPACE)).ok()?;
    let named = |value: Option<&Value>| {
        js::truthy_value(value).and_then(|v| {
            Regex::new(&format!(
                r#"(--|[-_.{s}="']){}(?-u:\b)"#,
                regex::escape(&js::to_string(Some(v))),
                s = js::SPACE
            ))
            .ok()
        })
    };
    let (variant, size) = (named(comp.get(wire::VARIANT)), named(comp.get(wire::SIZE)));
    let mut best: Option<(i32, usize)> = None;
    for m in CSS_RULE.captures_iter(css) {
        let (whole, selector) = (m.get(0)?, js::trim(m.get(1)?.as_str()));
        if !decl.is_match(&format!("{{{}", &m[2])) || selector.contains('@') {
            continue;
        }
        let mut score = 0;
        if STATE_SELECTOR.is_match(selector) {
            score -= 10;
        }
        if variant.as_ref().is_some_and(|re| re.is_match(selector)) {
            score += 4;
        }
        if size.as_ref().is_some_and(|re| re.is_match(selector)) {
            score += 2;
        }
        if cls.is_some_and(|cls| selector.contains(&format!(".{cls}"))) {
            score += 1;
        }
        if is_text_change && TEXT_SELECTOR.is_match(selector) {
            score += 1;
        }
        if best.is_none_or(|(s, _)| score > s) {
            let offset = whole.as_str().find(selector).unwrap_or(0);
            best = Some((score, style_at + whole.start() + offset));
        }
    }
    Some(best.filter(|(score, _)| *score > -10).map(|(_, i)| i))
}

/// The element of the master's `<template>` a change is on: the slot for a text change, the
/// element with the layer's class or tag, else the first element.
fn template_offset(block: &str, start: usize, change: &Value, cls: Option<&str>) -> Option<usize> {
    let tpl_at = find_from(block, "<template", start)?;
    let tpl_end = find_from(block, "</template>", tpl_at)
        .filter(|e| *e > 0)
        .unwrap_or(block.len());
    let first = |pick: &dyn Fn(&Tag) -> bool| {
        html::first_open_tag(block, tpl_at..tpl_end, pick).map(|tag| tag.range.start)
    };
    if change.get(field::TYPE).and_then(Value::as_str) == Some(change_type::TEXT) {
        if let Some(slot) = first(&|tag| tag.is(block, "dfw-slot")) {
            return Some(slot);
        }
    }
    if !js::truthy(change.get(wire::IS_ROOT)) {
        let tag = js::truthy_value(change.get(wire::TAG)).map(|t| js::to_string(Some(t)));
        let found = match (cls, tag) {
            (Some(cls), _) => first(&|tag| {
                tag.attrs.iter().any(|a| {
                    block[a.name.clone()].eq_ignore_ascii_case(attr::CLASS)
                        && has_word(&html::attr_value(block, a), cls)
                })
            }),
            (None, Some(name)) => first(&|tag| tag.is(block, &name)),
            (None, None) => None,
        };
        if found.is_some() {
            return found;
        }
    }
    first(&|tag| !tag.is(block, "template"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The offset `template_offset` gives in `block` for a change on a layer of class `cls` or
    /// of tag `tag` (neither: the first element).
    fn offset(block: &str, cls: Option<&str>, tag: Option<&str>) -> Option<usize> {
        let change = json!({ (field::TYPE): change_type::FILL, (wire::TAG): tag });
        template_offset(block, 0, &change, cls)
    }

    #[test]
    fn a_layer_inside_a_comment_is_not_found() {
        let block = "<template><!-- <i class=\"icon\"> <b> --><span><i class=\"icon\"></i><b></b></span></template>";
        assert_eq!(offset(block, Some("icon"), None), block.rfind("<i class"));
        assert_eq!(offset(block, None, Some("b")), block.find("<b></b>"));
        let block = "<template><!-- <i> --><span></span></template>";
        assert_eq!(offset(block, None, None), block.find("<span"));
    }

    #[test]
    fn single_quoted_and_mixed_case_classes_are_found() {
        let block = "<template>\r\n<p class='x'><i CLASS='a Icon-sm'></i></p></template>";
        assert_eq!(offset(block, Some("icon"), None), block.find("<i"));
        // A class that only starts with the name is no match.
        assert_eq!(offset(block, Some("ico"), None), block.find("<p"));
    }

    #[test]
    fn an_unclosed_tag_does_not_hide_the_layer() {
        let block = "<template><p title=\"open\n<b class=\"x\"></b></template>";
        assert_eq!(offset(block, Some("x"), None), block.find("<b"));
    }

    #[test]
    fn has_word_matches_on_word_boundaries() {
        assert!(has_word("btn btn--primary", "btn--primary"));
        /// A class name.
        const BTN: &str = "btn";
        assert!(has_word("btn-primary", BTN));
        assert!(!has_word("btnx", BTN));
        assert!(has_word("A BTN", BTN));
        assert!(!has_word("x", ""));
    }
}
