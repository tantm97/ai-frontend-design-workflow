//! Where a master change lands (`masterSource`): the CSS rule, template element or component
//! tag in the master's file.

use super::*;
use crate::core::js::regex::fixed_regex;
use crate::design::compose::ComponentDef;

/// The CSS properties a change type is made with.
fn css_property(change: &Value) -> Option<&'static str> {
    Some(match change.get("type").and_then(Value::as_str)? {
        "fill" if kind(change) == Some("text") => "color",
        "fill" => "background(-color)?",
        "stroke" => "border(-color|-width|-style)?|outline",
        "radius" => "border-radius|border-(top|bottom)-(left|right)-radius",
        "spacing" => "padding(-top|-right|-bottom|-left|-inline|-block)?|gap",
        "size" => "height|width|min-height|min-width|block-size|inline-size",
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

/// `/<(?!template\b)[a-z]/i`: the first tag that is not a template tag.
fn first_element(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len()).find(|&i| {
        if bytes[i] != b'<' || !bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            return false;
        }
        let name = &bytes[i + 1..];
        let template = name.len() >= 8
            && name[..8].eq_ignore_ascii_case(b"template")
            && !name
                .get(8)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        !template
    })
}

/// `masterSource`: `design/system/components/<file>:<line>` for a master change: the CSS rule
/// that sets the property (variant/size rules first), else the element in the template (by
/// tag/class), else the component tag. `None` when the change is not on a component.
pub fn master_source(root: &Path, change: &Value, lib: Option<&Library>) -> Option<MasterSource> {
    let comp = js::truthy_value(change.get("component"))?;
    let comp_name = comp.get("name").and_then(Value::as_str);
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
        js::truthy_value(change.get("name")).map_or(String::new(), |n| js::to_string(Some(n)));
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
        None => format!(
            "design/system/components/{}.html",
            js::to_string(comp.get("name"))
        ),
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
    let is_text_change = kind(change) == Some("text");
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
    let (variant, size) = (named(comp.get("variant")), named(comp.get("size")));
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
    let tpl = &block[tpl_at..tpl_end];
    if change.get("type").and_then(Value::as_str) == Some("text") {
        if let Some(slot) = tpl.find("<dfw-slot") {
            return Some(tpl_at + slot);
        }
    }
    if !js::truthy(change.get("isRoot")) {
        let tag = js::truthy_value(change.get("tag")).map(|t| js::to_string(Some(t)));
        let re = match (cls, tag) {
            (Some(cls), _) => Regex::new(&format!(
                r#"(?i)<[a-z][0-9A-Za-z_-]*[^>]*class="[^"]*(?-u:\b){}(?-u:\b)"#,
                regex::escape(cls)
            ))
            .ok(),
            (None, Some(tag)) => Regex::new(&format!(r"(?i)<{tag}(?-u:\b)")).ok(),
            (None, None) => None,
        };
        if let Some(m) = re.and_then(|re| re.find(tpl)) {
            return Some(tpl_at + m.start());
        }
    }
    tpl.get(1..)
        .and_then(first_element)
        .map(|first| tpl_at + 1 + first)
}
