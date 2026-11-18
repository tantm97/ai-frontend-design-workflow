//! Ports `core/lib/compose.mjs`: component masters, instances, slots, conditions, `build_system`.
//!
//! Screens reuse components with `<dfw-use>` and icons with `<dfw-icon>`; this module expands
//! them into plain HTML. Studio, checks, screenshots and handoff all see the composed output, so
//! a change to a component master updates every screen.
//!
//! The Node module found tags with regular expressions; here every tag comes from
//! `design::html`. Well-formed files compose to the same bytes. A tag written inside a comment,
//! a quoted attribute, `<script>` or `<style>` is no longer taken for a real one.
// Over 800 lines with its tests: one Node module, kept whole so the two can be read side by side.

// compose_file, build_composed, list_icons and design_base_for are for the groups ported next
// (lint, check, system page, Studio server): this allow goes when they call them.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::html::{element_at, is_js_space, line_of, parse_attrs, Attr, Tag, Token, Tokenizer};
use super::tokens::{dir_names, js, relative_path};
use crate::core::error::DfwError;
use crate::core::paths::{project_paths, resolve, to_posix};

/// Attributes of `<dfw-use>` that are not props and are not copied to the root element.
const RESERVED: [&str; 6] = ["c", "component", "variant", "size", "id", "class"];

/// `escapeHtml`: `&`, `<`, `>` and `"`.
pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The object `parseAttrs` built: decoded values by name, in the order the names first appear;
/// a repeated name keeps its place and takes the later value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs(Vec<(String, String)>);

impl Attrs {
    /// The value of `name` (empty for an attribute without a value).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The value of `name` when it is not empty (`attrs.name || ...` in the Node code).
    pub fn nonempty(&self, name: &str) -> Option<&str> {
        self.get(name).filter(|v| !v.is_empty())
    }

    /// Set `name`, keeping its place when it exists.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        match self.0.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value.into(),
            None => self.0.push((name.to_string(), value.into())),
        }
    }

    /// Name and value pairs in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// `parseAttrs`: the attributes of a tag as decoded name and value pairs.
pub fn attrs_of(src: &str, attrs: &[Attr]) -> Attrs {
    let mut out = Attrs::default();
    for attr in attrs {
        out.set(&src[attr.name.clone()], super::html::attr_value(src, attr));
    }
    out
}

/// `attrsToString`: ` name="value"` for each attribute; an empty value prints the bare name
/// except for `data-*`, `aria-*`, `id`, `class`, `alt` and `value`.
pub fn attrs_to_string(attrs: &Attrs) -> String {
    let keeps_empty = |k: &str| {
        k.starts_with("data-")
            || k.starts_with("aria-")
            || matches!(k, "id" | "class" | "alt" | "value")
    };
    attrs
        .iter()
        .map(|(k, v)| {
            if v.is_empty() && !keeps_empty(k) {
                format!(" {k}")
            } else {
                format!(" {k}=\"{}\"", escape_html(v))
            }
        })
        .collect()
}

// ---------------------------------------------------------------- library

/// One component master (`<dfw-component>`). `name` is `None` when the attribute is missing
/// (the Node code kept such a master under the key `undefined`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComponentDef {
    pub name: Option<String>,
    pub file: String,
    pub line: usize,
    pub description: String,
    pub group: String,
    pub variants: Vec<String>,
    pub sizes: Vec<String>,
    pub default_variant: String,
    pub default_size: String,
    pub props: Vec<String>,
    pub prop_defaults: Vec<(String, String)>,
    pub replaces: Vec<String>,
    pub preview: String,
    pub accepts: Vec<String>,
    pub styles: String,
    pub template: String,
    pub examples: String,
}

impl ComponentDef {
    /// The name as the Node code printed it.
    pub fn display_name(&self) -> &str {
        self.name.as_deref().unwrap_or("undefined")
    }

    /// The definition as the JSON object the Node code built (same keys, same order).
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        if let Some(name) = &self.name {
            map.insert("name".into(), json!(name));
        }
        let mut defaults = Map::new();
        for (k, v) in &self.prop_defaults {
            defaults.insert(k.clone(), json!(v));
        }
        for (key, value) in [
            ("file", json!(self.file)),
            ("line", json!(self.line)),
            ("description", json!(self.description)),
            ("group", json!(self.group)),
            ("variants", json!(self.variants)),
            ("sizes", json!(self.sizes)),
            ("defaultVariant", json!(self.default_variant)),
            ("defaultSize", json!(self.default_size)),
            ("props", json!(self.props)),
            ("propDefaults", js::key_order(Value::Object(defaults))),
            ("replaces", json!(self.replaces)),
            ("preview", json!(self.preview)),
            ("accepts", json!(self.accepts)),
            ("styles", json!(self.styles)),
            ("template", json!(self.template)),
            ("examples", json!(self.examples)),
        ] {
            map.insert(key.into(), value);
        }
        Value::Object(map)
    }
}

/// The component masters of a project by name, in the order they were loaded.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library(Vec<ComponentDef>);

impl Library {
    /// The master called `name` (`None` finds a master without a name, as in the Node code).
    pub fn get(&self, name: Option<&str>) -> Option<&ComponentDef> {
        self.0.iter().find(|d| d.name.as_deref() == name)
    }

    /// Number of masters.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// True when the project has no master.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// The masters in order.
    pub fn iter(&self) -> impl Iterator<Item = &ComponentDef> {
        self.0.iter()
    }

    /// Add a master; one with the same name is replaced and keeps its place.
    pub fn insert(&mut self, def: ComponentDef) {
        match self.0.iter_mut().find(|d| d.name == def.name) {
            Some(slot) => *slot = def,
            None => self.0.push(def),
        }
    }
}

fn system_dir(root: &Path) -> PathBuf {
    project_paths(root).design.join("system")
}

/// `loadComponents`: every master of `design/system/components/*.html`, files in name order.
pub fn load_components(root: &Path) -> Result<Library, DfwError> {
    let dir = system_dir(root).join("components");
    let mut lib = Library::default();
    let mut files: Vec<String> = dir_names(&dir)?
        .into_iter()
        .filter(|f| f.ends_with(".html"))
        .collect();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    for f in files {
        let text = js::read_text(&dir.join(&f))?;
        for def in parse_component_file(&text, &format!("design/system/components/{f}")) {
            lib.insert(def);
        }
    }
    Ok(lib)
}

/// True when `text` starts with `prefix` and the name ends there (`\b` after a letter).
fn starts_tag(text: &str, prefix: &str) -> bool {
    text.strip_prefix(prefix)
        .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_'))
}

/// `props="a b=default"`, or comma-separated when a default has spaces: a default then runs to
/// the next comma, so "asset name site=Asset Tracking, printed" gives asset, name, site
/// (default "Asset Tracking"), printed.
fn prop_list(text: Option<&str>) -> Vec<String> {
    let text = text.unwrap_or_default();
    if !text.contains(',') {
        return js::words(text);
    }
    let pieces = |t: &str| -> Vec<String> {
        if t.is_empty() {
            return vec![String::new()];
        }
        t.split(is_js_space)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect()
    };
    let mut out = Vec::new();
    for part in text.split(',') {
        let t = js::trim(part);
        if t.is_empty() {
            continue;
        }
        match t.find('=') {
            None => out.extend(pieces(t)),
            Some(eq) => {
                let mut before = pieces(js::trim(&t[..eq]));
                let last = before.pop().unwrap_or_default();
                out.extend(before);
                out.push(format!("{last}={}", js::trim(&t[eq + 1..])));
            }
        }
    }
    out
}

/// The parts of a master: the text of every `<style>`, of the first `<template>` and of the
/// first `<dfw-examples>` (each up to the first closing tag of that name).
fn master_parts(inner: &str) -> (String, Option<&str>, Option<&str>) {
    let tokens: Vec<Token> = Tokenizer::new(inner).collect();
    let mut styles: Vec<&str> = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Token::Open(tag) = token else { continue };
        if !tag.is(inner, "style") || tag.self_closing {
            continue;
        }
        let (content, close) = match tokens.get(i + 1) {
            Some(Token::RawText(r)) => (&inner[r.clone()], tokens.get(i + 2)),
            other => ("", other),
        };
        if matches!(close, Some(Token::Close(c)) if c.is(inner, "style")) {
            styles.push(js::trim(content));
        }
    }
    let first = |name: &str| -> Option<&str> {
        let (i, open) = tokens.iter().enumerate().find_map(|(i, t)| match t {
            Token::Open(tag) if tag.is(inner, name) => Some((i, tag)),
            _ => None,
        })?;
        let close = tokens[i + 1..].iter().find_map(|t| match t {
            Token::Close(tag) if tag.is(inner, name) => Some(tag),
            _ => None,
        })?;
        Some(&inner[open.range.end..close.range.start])
    };
    (styles.join("\n"), first("template"), first("dfw-examples"))
}

/// `parseComponentFile`: the masters written in one file. `rel` is the path shown in messages.
pub fn parse_component_file(text: &str, rel: &str) -> Vec<ComponentDef> {
    let mut defs = Vec::new();
    let mut tokens = Tokenizer::new(text);
    while let Some(token) = tokens.next() {
        let tag = match token {
            Token::Open(tag) if tag.name(text) == "dfw-component" => tag,
            // The Node code stopped at a <dfw-component whose tag it could not read.
            Token::Text(r) if text[r.clone()].starts_with("<dfw-component") => break,
            _ => continue,
        };
        let Some(el) = element_at(text, tag.range.start) else {
            break;
        };
        let a = attrs_of(text, &el.open.attrs);
        let (styles, template, examples) = master_parts(&text[el.inner.clone()]);
        let list = |name: &str| js::words(a.get(name).unwrap_or_default());
        let or = |name: &str, fallback: &str| a.nonempty(name).unwrap_or(fallback).to_string();
        let props = prop_list(a.get("props"));
        let variants = list("variants");
        let sizes = list("sizes");
        let mut prop_defaults: Vec<(String, String)> = Vec::new();
        for (name, default) in props.iter().filter_map(|p| p.split_once('=')) {
            match prop_defaults.iter_mut().find(|(k, _)| k == name) {
                Some(slot) => slot.1 = default.to_string(),
                None => prop_defaults.push((name.to_string(), default.to_string())),
            }
        }
        // Leading comments are dropped so the root element comes first (annotate_root stamps
        // id and class on it).
        let template = template.map(|t| {
            let mut t = js::trim(t);
            while let Some(len) = t.strip_prefix("<!--").and_then(|rest| rest.find("-->")) {
                t = t[4 + len + 3..].trim_start_matches(is_js_space);
            }
            t.to_string()
        });
        defs.push(ComponentDef {
            name: a.get("name").map(str::to_string),
            file: rel.to_string(),
            line: line_of(text, tag.range.start),
            description: or("description", ""),
            group: or("group", "Components"),
            default_variant: a
                .nonempty("default-variant")
                .or(variants.first().map(String::as_str))
                .unwrap_or_default()
                .to_string(),
            default_size: a
                .nonempty("default-size")
                .or(sizes.first().map(String::as_str))
                .unwrap_or_default()
                .to_string(),
            variants,
            sizes,
            props: props
                .iter()
                .map(|p| p.split('=').next().unwrap_or_default().to_string())
                .collect(),
            prop_defaults,
            replaces: list("replaces"),
            preview: or("preview", ""),
            accepts: list("accepts"),
            styles,
            template: template.unwrap_or_default(),
            examples: examples
                .map(|e| js::trim(e).to_string())
                .unwrap_or_default(),
        });
        tokens.seek(el.range.end);
    }
    defs
}

/// `loadIcon`: the SVG of `design/system/icons/<name>.svg`, or `None`.
pub fn load_icon(root: &Path, name: &str) -> Result<Option<String>, DfwError> {
    // The name comes from a design file: it may name a sub-folder, never leave the icons folder.
    let file = format!("{name}.svg");
    if file.split(['/', '\\']).any(|part| part == "..") {
        return Ok(None);
    }
    let mut abs = system_dir(root).join("icons");
    abs.extend(file.split('/').filter(|part| !part.is_empty()));
    if abs.is_file() {
        js::read_text(&abs).map(Some)
    } else {
        Ok(None)
    }
}

/// `listIcons`: names of the icons in `design/system/icons/`, sorted.
pub fn list_icons(root: &Path) -> Result<Vec<String>, DfwError> {
    let mut names: Vec<String> = dir_names(&system_dir(root).join("icons"))?
        .iter()
        .filter_map(|f| f.strip_suffix(".svg").map(str::to_string))
        .collect();
    names.sort_by(|a, b| js::cmp_utf16(a, b));
    Ok(names)
}

/// `componentsCss`: the styles of every master (`design/system/components.css` is this text).
pub fn components_css(lib: &Library) -> String {
    let mut parts = vec![
        "/* Generated by dfw from design/system/components/*.html. Do not edit by hand. */"
            .to_string(),
    ];
    for d in lib.iter().filter(|d| !d.styles.is_empty()) {
        parts.push(format!(
            "/* {} ({}) */\n{}",
            d.display_name(),
            d.file,
            d.styles
        ));
    }
    parts.join("\n\n") + "\n"
}

// ---------------------------------------------------------------- expansion

/// A reader over template text for the `{{...}}` placeholders.
struct Cursor<'a> {
    text: &'a str,
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn space(&mut self) {
        let rest = &self.text[self.pos..];
        self.pos += rest.len() - rest.trim_start_matches(is_js_space).len();
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
    let unspaced = rest.trim_start_matches(is_js_space);
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
    match rest.trim_start_matches(is_js_space) {
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

fn fill_template(def: &ComponentDef, props: &Attrs, slots: &Attrs) -> String {
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
        let cond = super::html::attr_value(&out, &attr);
        let cut = if condition_holds(&cond, props) {
            // Keep the element, drop the attribute and the white space before it. A bare value
            // runs to the next white space, as the Node pattern removed it (`=mode==x`).
            let lead = out[tag.name.end..attr.span.start].trim_end_matches(is_js_space);
            let mut end = attr.span.end;
            if attr.quote == super::html::Quote::Bare {
                let rest = &out[end..tag.range.end - 1];
                end += rest.find(is_js_space).unwrap_or(rest.len());
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
fn annotate_root(html: &str, extra: &[(String, Option<String>)], class: Option<&str>) -> String {
    let start = html.len() - html.trim_start_matches(is_js_space).len();
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

struct RawInstance {
    component: Option<String>,
    variant: String,
    size: String,
    id: Option<String>,
    index: usize,
}

struct RawIssue {
    rule: &'static str,
    message: String,
    index: usize,
}

struct Level {
    html: String,
    instances: Vec<RawInstance>,
    issues: Vec<RawIssue>,
}

fn expand_icon(
    root: &Path,
    attrs: &Attrs,
    issues: &mut Vec<RawIssue>,
    at: usize,
) -> Result<String, DfwError> {
    let name = attrs.get("name");
    let shown = name.unwrap_or("undefined");
    let Some(svg) = load_icon(root, shown)? else {
        issues.push(RawIssue {
            rule: "icon-unknown",
            index: at,
            message: format!("Unknown icon \"{shown}\" (add it: dfw icons add {shown})."),
        });
        return Ok(format!(
            "<span data-dfw-missing-icon=\"{}\"></span>",
            escape_html(name.unwrap_or_default())
        ));
    };
    let size = attrs.nonempty("size").unwrap_or("24").to_string();
    let label = attrs.nonempty("label").is_some();
    let some = |v: &str| Some(v.to_string());
    let mut extra: Vec<(String, Option<String>)> = vec![
        ("width".into(), Some(size.clone())),
        ("height".into(), Some(size)),
        (
            "aria-hidden".into(),
            if label { None } else { some("true") },
        ),
        ("role".into(), if label { some("img") } else { None }),
        ("aria-label".into(), attrs.get("label").map(str::to_string)),
        ("data-dfw-icon".into(), name.map(str::to_string)),
    ];
    for (from, to) in [("stroke-width", "stroke-width"), ("id", "data-dfw-id")] {
        if let Some(v) = attrs.nonempty(from) {
            set_extra(&mut extra, to, some(v));
        }
    }
    let class = match attrs.nonempty("class") {
        Some(own) => format!("icon {own}"),
        None => "icon".to_string(),
    };
    // Drop the XML declaration and comments so the <svg> tag comes first.
    let mut svg = svg;
    if let Some(start) = svg.find("<?xml") {
        if let Some(len) = svg[start..].find('>') {
            let after = svg[start + len + 1..]
                .trim_start_matches(is_js_space)
                .to_string();
            svg.truncate(start);
            svg.push_str(&after);
        }
    }
    let svg = js::strip_comments(&svg);
    Ok(annotate_root(js::trim(&svg), &extra, Some(&class)))
}

fn set_extra(extra: &mut Vec<(String, Option<String>)>, key: &str, value: Option<String>) {
    match extra.iter_mut().find(|(k, _)| k == key) {
        Some(slot) => slot.1 = value,
        None => extra.push((key.to_string(), value)),
    }
}

/// `expandHead`: replace `<dfw-head></dfw-head>` by `design/system/head.html` and the links to
/// tokens.css, base.css (if present) and components.css. `design_base` is the path from the
/// page to `design/`.
pub fn expand_head(root: &Path, html: &str, design_base: &str) -> Result<String, DfwError> {
    let head = Tokenizer::new(html).find_map(|token| match token {
        Token::Open(tag) if tag.name(html) == "dfw-head" => Some(tag),
        _ => None,
    });
    let Some(tag) = head else {
        return Ok(html.to_string());
    };
    let d = project_paths(root).design;
    let head_file = d.join("system").join("head.html");
    let mut parts = Vec::new();
    if head_file.exists() {
        let text = js::strip_comments(&js::read_text(&head_file)?).replace("{{base}}", design_base);
        parts.push(js::trim(&text).to_string());
    }
    parts.push(format!(
        "<link rel=\"stylesheet\" href=\"{design_base}tokens/tokens.css\">"
    ));
    if d.join("styles").join("base.css").exists() {
        parts.push(format!(
            "<link rel=\"stylesheet\" href=\"{design_base}styles/base.css\">"
        ));
    }
    parts.push(format!(
        "<link rel=\"stylesheet\" href=\"{design_base}system/components.css\">"
    ));
    let mut end = tag.range.end;
    let after = &html[end..];
    let unspaced = after.trim_start_matches(is_js_space);
    if unspaced.starts_with("</dfw-head>") {
        end += after.len() - unspaced.len() + "</dfw-head>".len();
    }
    Ok(js::replace_range(
        html,
        tag.range.start..end,
        &parts.join("\n"),
    ))
}

/// `designBaseFor`: relative path from a design file (relative to the project root) to
/// `design/`, with a trailing slash.
pub fn design_base_for(rel: &str) -> String {
    let posix = to_posix(rel);
    let inside = posix.strip_prefix("design/").unwrap_or(&posix);
    let depth = inside.split('/').count() - 1;
    if depth > 0 {
        "../".repeat(depth)
    } else {
        "./".to_string()
    }
}

/// One `<dfw-use>` found while composing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub component: Option<String>,
    pub variant: String,
    pub size: String,
    pub id: Option<String>,
    /// Line in the source file of the instance (of the outermost one for nested instances).
    pub line: i64,
}

/// A problem found while composing; `level` is always `error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issue {
    pub level: &'static str,
    pub rule: &'static str,
    pub message: String,
    pub line: i64,
}

/// The result of [`compose_html`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Composed {
    pub html: String,
    pub instances: Vec<Instance>,
    pub issues: Vec<Issue>,
}

impl Composed {
    /// `{ html, instances, issues }` as the Node function returned it.
    pub fn to_json(&self) -> Value {
        let instances: Vec<Value> = self
            .instances
            .iter()
            .map(|i| {
                let mut map = Map::new();
                if let Some(component) = &i.component {
                    map.insert("component".into(), json!(component));
                }
                map.insert("variant".into(), json!(i.variant));
                map.insert("size".into(), json!(i.size));
                map.insert("id".into(), json!(i.id));
                map.insert("line".into(), json!(i.line));
                Value::Object(map)
            })
            .collect();
        let issues: Vec<Value> = self
            .issues
            .iter()
            .map(|i| json!({ "level": i.level, "rule": i.rule, "message": i.message, "line": i.line }))
            .collect();
        json!({ "html": self.html, "instances": instances, "issues": issues })
    }
}

/// Options of [`compose_html`].
#[derive(Debug, Clone, Copy, Default)]
pub struct ComposeOptions<'a> {
    /// The masters to use; loaded from the project when `None`.
    pub lib: Option<&'a Library>,
    /// Path from the page to `design/` (`../../`, or `/design/` in the Studio). When set,
    /// `<dfw-head>` is expanded and `{{base}}` resolved.
    pub design_base: Option<&'a str>,
}

/// `composeHtml`: expand `<dfw-use>`, `<dfw-icon>` (and `<dfw-head>` with a design base) in an
/// HTML text.
pub fn compose_html(root: &Path, html: &str, opts: ComposeOptions) -> Result<Composed, DfwError> {
    let loaded;
    let lib = match opts.lib {
        Some(lib) => lib,
        None => {
            loaded = load_components(root)?;
            &loaded
        }
    };
    let lines = |text: &str| line_of(text, text.len()) as i64;
    let base = opts.design_base.filter(|b| !b.is_empty());
    let expanded;
    let mut text = html;
    let mut source_line = 0;
    if let Some(base) = base {
        expanded = expand_head(root, html, base)?;
        // Report lines of the source file, not of the expanded head.
        source_line -= lines(&expanded) - lines(html);
        text = &expanded;
    }
    let level = compose_level(root, text, lib, 0)?;
    let line = |index: usize| line_of(text, index) as i64 + source_line;
    Ok(Composed {
        // {{base}} in masters, examples and instance attributes: the path from this page to design/.
        html: match base {
            Some(base) => level.html.replace("{{base}}", base),
            None => level.html,
        },
        instances: level
            .instances
            .into_iter()
            .map(|i| Instance {
                line: line(i.index),
                component: i.component,
                variant: i.variant,
                size: i.size,
                id: i.id,
            })
            .collect(),
        issues: level
            .issues
            .into_iter()
            .map(|i| Issue {
                level: "error",
                rule: i.rule,
                line: line(i.index),
                message: i.message,
            })
            .collect(),
    })
}

/// The `<dfw-fill>` elements that belong to this instance (fills inside a nested `<dfw-use>`
/// belong to that one), and the content left without them.
fn take_fills(inner: &str) -> (Attrs, String) {
    let mut slots = Attrs::default();
    let mut rest = String::new();
    let mut pos = 0;
    let mut tokens = Tokenizer::new(inner);
    while let Some(token) = tokens.next() {
        let tag = match token {
            Token::Open(tag) => tag,
            Token::Text(r)
                if starts_tag(&inner[r.clone()], "<dfw-use")
                    || starts_tag(&inner[r.clone()], "<dfw-fill") =>
            {
                break
            }
            _ => continue,
        };
        let fill = match tag.name(inner) {
            "dfw-use" => false,
            "dfw-fill" => true,
            _ => continue,
        };
        let Some(el) = element_at(inner, tag.range.start) else {
            break;
        };
        if fill {
            let slot = attrs_of(inner, &el.open.attrs);
            slots.set(
                slot.nonempty("slot").unwrap_or("default"),
                &inner[el.inner.clone()],
            );
            rest.push_str(&inner[pos..tag.range.start]);
            pos = el.range.end;
        }
        tokens.seek(el.range.end);
    }
    rest.push_str(&inner[pos..]);
    (slots, rest)
}

fn expand_use(
    root: &Path,
    html: &str,
    tag: &Tag,
    inner: &str,
    lib: &Library,
    depth: usize,
    level: &mut Level,
) -> Result<(), DfwError> {
    let start = tag.range.start;
    let attrs = attrs_of(html, &tag.attrs);
    let name = attrs.nonempty("c").or(attrs.nonempty("component"));
    let shown = name.unwrap_or("undefined");
    let Some(def) = lib.get(name) else {
        level.issues.push(RawIssue {
            rule: "component-unknown",
            index: start,
            message: format!("Unknown component \"{shown}\". Define it in design/system/components/ first (design system before screens)."),
        });
        level.html.push_str(&format!(
            "<div data-dfw-missing-component=\"{}\">{inner}</div>",
            escape_html(name.unwrap_or_default())
        ));
        return Ok(());
    };
    let variant = attrs
        .nonempty("variant")
        .unwrap_or(&def.default_variant)
        .to_string();
    let size = attrs
        .nonempty("size")
        .unwrap_or(&def.default_size)
        .to_string();
    if !def.variants.is_empty() && !def.variants.contains(&variant) {
        level.issues.push(RawIssue {
            rule: "component-variant",
            index: start,
            message: format!(
                "Component \"{shown}\" has no variant \"{variant}\" (variants: {}).",
                def.variants.join(", ")
            ),
        });
    }
    if !def.sizes.is_empty() && !def.sizes.contains(&size) {
        level.issues.push(RawIssue {
            rule: "component-size",
            index: start,
            message: format!(
                "Component \"{shown}\" has no size \"{size}\" (sizes: {}).",
                def.sizes.join(", ")
            ),
        });
    }
    let mut props = Attrs::default();
    for (k, v) in &def.prop_defaults {
        props.set(k, v.clone());
    }
    props.set("variant", variant.clone());
    props.set("size", size.clone());
    let mut extra: Vec<(String, Option<String>)> = Vec::new();
    for (k, v) in attrs.iter() {
        if RESERVED.contains(&k) {
            continue;
        }
        if def.props.iter().any(|p| p == k) {
            props.set(k, v);
        } else {
            set_extra(&mut extra, k, Some(v.to_string()));
        }
    }
    let (mut slots, rest) = take_fills(inner);
    if slots.get("default").is_none() {
        slots.set("default", rest);
    }
    let body = fill_template(def, &props, &slots);
    let id = attrs.get("id");
    set_extra(&mut extra, "data-dfw-id", id.map(str::to_string));
    set_extra(&mut extra, "data-dfw-component", name.map(str::to_string));
    set_extra(
        &mut extra,
        "data-dfw-variant",
        (!def.variants.is_empty()).then(|| variant.clone()),
    );
    set_extra(
        &mut extra,
        "data-dfw-size",
        (!def.sizes.is_empty()).then(|| size.clone()),
    );
    let body = annotate_root(&body, &extra, attrs.get("class"));
    level.instances.push(RawInstance {
        component: name.map(str::to_string),
        variant,
        size,
        id: id.filter(|i| !i.is_empty()).map(str::to_string),
        index: start,
    });
    let nested = compose_level(root, &body, lib, depth + 1)?;
    level.html.push_str(&nested.html);
    level.instances.extend(
        nested
            .instances
            .into_iter()
            .map(|i| RawInstance { index: start, ..i }),
    );
    level.issues.extend(
        nested
            .issues
            .into_iter()
            .map(|i| RawIssue { index: start, ..i }),
    );
    Ok(())
}

fn compose_level(root: &Path, html: &str, lib: &Library, depth: usize) -> Result<Level, DfwError> {
    let mut level = Level {
        html: String::new(),
        instances: Vec::new(),
        issues: Vec::new(),
    };
    if depth > 12 {
        level.issues.push(RawIssue {
            rule: "compose-depth",
            index: 0,
            message: "Component nesting deeper than 12 levels (recursive component?).".to_string(),
        });
        level.html = html.to_string();
        return Ok(level);
    }
    let mut done = 0;
    let mut tokens = Tokenizer::new(html);
    while let Some(token) = tokens.next() {
        let tag = match token {
            Token::Open(tag) => tag,
            // A <dfw-use or <dfw-icon whose tag cannot be read (a quote that never closes): the
            // Node code stopped there and left the rest as written.
            Token::Text(r)
                if starts_tag(&html[r.clone()], "<dfw-use")
                    || starts_tag(&html[r.clone()], "<dfw-icon") =>
            {
                break
            }
            _ => continue,
        };
        let kind = match tag.name(html) {
            kind @ ("dfw-use" | "dfw-icon") => kind,
            _ => continue,
        };
        let start = tag.range.start;
        let Some(el) = element_at(html, start) else {
            break;
        };
        if el.unclosed {
            // Without its closing tag the instance would swallow the rest of the page: report
            // it and leave the rest as written.
            level.issues.push(RawIssue {
                rule: "component-unclosed",
                index: start,
                message: format!("<{kind}> has no closing </{kind}> tag."),
            });
            break;
        }
        level.html.push_str(&html[done..start]);
        if kind == "dfw-icon" {
            let attrs = attrs_of(html, &el.open.attrs);
            let svg = expand_icon(root, &attrs, &mut level.issues, start)?;
            level.html.push_str(&svg);
        } else {
            expand_use(
                root,
                html,
                &el.open,
                &html[el.inner.clone()],
                lib,
                depth,
                &mut level,
            )?;
        }
        done = el.range.end;
        tokens.seek(el.range.end);
    }
    level.html.push_str(&html[done..]);
    Ok(level)
}

/// The result of [`compose_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedFile {
    pub composed: Composed,
    /// The file uses components and does not link `system/components.css` itself.
    pub needs_components_css: bool,
}

/// `composeFile`: compose a design HTML file (path relative to the project root).
pub fn compose_file(root: &Path, rel: &str) -> Result<ComposedFile, DfwError> {
    let text = js::read_text(&root.join(rel))?;
    let base = design_base_for(rel);
    let composed = compose_html(
        root,
        &text,
        ComposeOptions {
            lib: None,
            design_base: Some(&base),
        },
    )?;
    let needs_components_css =
        !text.contains("system/components.css") && !composed.instances.is_empty();
    Ok(ComposedFile {
        composed,
        needs_components_css,
    })
}

/// What [`build_system`] found and wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltSystem {
    /// Number of masters.
    pub components: usize,
    /// Files written (relative to the project root): `components.css` when its text changed.
    pub written: Vec<String>,
}

/// `buildSystem`: write `design/system/components.css` from the masters when it changed.
pub fn build_system(root: &Path) -> Result<BuiltSystem, DfwError> {
    let lib = load_components(root)?;
    let sys = system_dir(root);
    let mut written = Vec::new();
    if !lib.is_empty() {
        fs::create_dir_all(&sys)?;
        let file = sys.join("components.css");
        let css = components_css(&lib);
        if !file.exists() || js::read_text(&file)? != css {
            fs::write(&file, css)?;
            written.push(to_posix(&relative_path(root, &file)));
        }
    }
    Ok(BuiltSystem {
        components: lib.len(),
        written,
    })
}

/// One entry of [`build_composed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFile {
    /// The design file, relative to the project root.
    pub rel: String,
    /// Plain HTML to hand to a tool: the copy under `.dfw/build/`, or the file itself when it
    /// uses no dfw element.
    pub out: PathBuf,
    pub composed: bool,
}

/// The local stylesheet a `<link rel="stylesheet" href="...">` tag names, as written.
fn stylesheet_href<'a>(html: &'a str, tag: &Tag) -> Option<&'a str> {
    let quoted = |attr: &&Attr| attr.quote != super::html::Quote::Bare;
    let raw = |attr: &Attr| attr.value.clone().map(|r| &html[r]).unwrap_or_default();
    tag.attrs.iter().filter(quoted).find(|a| {
        html[a.name.clone()].eq_ignore_ascii_case("rel")
            && raw(a).eq_ignore_ascii_case("stylesheet")
    })?;
    let href = tag
        .attrs
        .iter()
        .filter(quoted)
        .find(|a| &html[a.name.clone()] == "href")
        .map(raw)
        .filter(|h| !h.is_empty())?;
    let remote = ["//", "http://", "https://"]
        .iter()
        .any(|p| href.starts_with(p));
    (!remote).then_some(href)
}

/// `buildComposed`: compose each design file into `.dfw/build/<same path>` for tools that need
/// plain HTML. Local stylesheets are inlined so static checkers see the real styles, and a
/// `<base>` keeps other relative links (images, fonts) working from the copy.
pub fn build_composed(root: &Path, rels: &[String]) -> Result<Vec<BuiltFile>, DfwError> {
    let out_dir = project_paths(root).dfw.join("build");
    let lib = load_components(root)?;
    let mut res = Vec::new();
    for rel in rels {
        let abs = root.join(rel);
        if !abs.exists() {
            continue;
        }
        let text = js::read_text(&abs)?;
        let uses_dfw = ["<dfw-use", "<dfw-icon", "<dfw-head"].iter().any(|p| {
            text.match_indices(p)
                .any(|(i, _)| starts_tag(&text[i..], p))
        });
        if !uses_dfw {
            res.push(BuiltFile {
                rel: rel.clone(),
                out: abs,
                composed: false,
            });
            continue;
        }
        let base = design_base_for(rel);
        let composed = compose_html(
            root,
            &text,
            ComposeOptions {
                lib: Some(&lib),
                design_base: Some(&base),
            },
        )?;
        let out = out_dir.join(rel);
        let (from_dir, out_parent) = (
            abs.parent().unwrap_or(root),
            out.parent().unwrap_or(&out_dir),
        );
        fs::create_dir_all(out_parent)?;
        let html = &composed.html;
        let mut inlined = String::with_capacity(html.len());
        let mut done = 0;
        for token in Tokenizer::new(html) {
            let Token::Open(tag) = token else { continue };
            if !tag.is(html, "link") {
                continue;
            }
            let Some(href) = stylesheet_href(html, &tag) else {
                continue;
            };
            let file = resolve(from_dir.join(href.split('?').next().unwrap_or_default()));
            if file.exists() {
                inlined.push_str(&html[done..tag.range.start]);
                inlined.push_str(&format!(
                    "<style data-dfw-inlined=\"{}\">\n{}\n</style>",
                    escape_html(href),
                    js::read_text(&file)?
                ));
                done = tag.range.end;
            }
        }
        inlined.push_str(&html[done..]);
        let base_href = match to_posix(&relative_path(out_parent, from_dir)) {
            rel if rel.is_empty() => ".".to_string(),
            rel => rel,
        };
        // The Node pattern was `<head[^>]*>`: the first tag whose name starts with "head".
        let head = Tokenizer::new(&inlined).find_map(|token| match token {
            Token::Open(tag)
                if tag
                    .name(&inlined)
                    .get(..4)
                    .is_some_and(|n| n.eq_ignore_ascii_case("head")) =>
            {
                Some(tag.range.end)
            }
            _ => None,
        });
        if let Some(at) = head {
            inlined.insert_str(at, &format!("\n<base href=\"{base_href}/\">"));
        }
        fs::write(&out, inlined)?;
        res.push(BuiltFile {
            rel: rel.clone(),
            out,
            composed: true,
        });
    }
    Ok(res)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn project(files: &[(&str, &str)]) -> PathBuf {
        let root = test_dir("compose");
        for (rel, text) in files {
            let file = root.join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        }
        root
    }

    fn master(name: &str, tpl: &str, attrs: &str) -> String {
        format!("<dfw-component name=\"{name}\"{attrs}>\n<template>\n{tpl}\n</template>\n</dfw-component>\n")
    }

    fn compose(root: &Path, html: &str) -> Composed {
        compose_html(root, html, ComposeOptions::default()).unwrap()
    }

    fn rules(c: &Composed) -> Vec<&str> {
        c.issues.iter().map(|i| i.rule).collect()
    }

    /// compose: unknown component and icon are reported, recursion is bounded
    #[test]
    fn unknown_component_and_icon_and_bounded_recursion() {
        let root = project(&[]);
        let r = compose(
            &root,
            "<dfw-use c=\"ghost\">x</dfw-use><dfw-icon name=\"nope\"></dfw-icon>",
        );
        assert_eq!(rules(&r), ["component-unknown", "icon-unknown"]);
        assert_eq!(r.html, "<div data-dfw-missing-component=\"ghost\">x</div><span data-dfw-missing-icon=\"nope\"></span>");
        let root = project(&[("design/system/components/loop.html", "<dfw-component name=\"loop\"><template><div><dfw-use c=\"loop\"></dfw-use></div></template></dfw-component>")]);
        let r2 = compose(&root, "<dfw-use c=\"loop\"></dfw-use>");
        assert!(rules(&r2).contains(&"compose-depth"));
        assert_eq!(r2.instances.len(), 13);
    }

    /// compose: a nested instance keeps its own slot fills
    #[test]
    fn nested_instance_keeps_its_own_slot_fills() {
        let root = project(&[
            ("design/system/components/box.html", "<dfw-component name=\"box\"><template><section><dfw-slot></dfw-slot><footer><dfw-slot name=\"legend\"></dfw-slot></footer></section></template></dfw-component>"),
            ("design/system/components/bar.html", "<dfw-component name=\"bar\"><template><div class=\"bar\"><dfw-slot></dfw-slot><ul><dfw-slot name=\"legend\"></dfw-slot></ul></div></template></dfw-component>"),
        ]);
        let r = compose(&root, "<dfw-use c=\"box\"><dfw-use c=\"bar\"><i>seg</i><dfw-fill slot=\"legend\"><li>inner</li></dfw-fill></dfw-use><dfw-fill slot=\"legend\">outer</dfw-fill></dfw-use>");
        assert!(r.html.contains("<ul><li>inner</li></ul>"), "{}", r.html);
        assert!(r.html.contains("<footer>outer</footer>"), "{}", r.html);
    }

    /// compose: {{prop==value?text}} marks the current item of a shared shell
    #[test]
    fn conditional_text_marks_the_current_item() {
        let root = project(&[("design/system/components/shell.html", "<dfw-component name=\"shell\" props=\"current\"><template><nav><a aria-current=\"{{current==home?page}}\">H</a><a aria-current=\"{{current==list?page}}\">L</a></nav><main><dfw-slot></dfw-slot></main></template></dfw-component>")]);
        let r = compose(&root, "<dfw-use c=\"shell\" current=\"list\">x</dfw-use>");
        assert!(
            r.html
                .contains("<a aria-current=\"\">H</a><a aria-current=\"page\">L</a>"),
            "{}",
            r.html
        );
    }

    /// compose: prop lists with spaces, commas and spaced defaults
    #[test]
    fn prop_lists_with_spaces_commas_and_spaced_defaults() {
        let a = &parse_component_file("<dfw-component name=\"x\" props=\"asset name location site=Asset Tracking, printed\"><template><i></i></template></dfw-component>", "x.html")[0];
        assert_eq!(a.props, ["asset", "name", "location", "site", "printed"]);
        assert_eq!(
            a.prop_defaults,
            [("site".to_string(), "Asset Tracking".to_string())]
        );
        let b = &parse_component_file("<dfw-component name=\"y\" props=\"as=p tone=default\"><template><i></i></template></dfw-component>", "y.html")[0];
        assert_eq!(
            b.prop_defaults,
            [
                ("as".to_string(), "p".to_string()),
                ("tone".to_string(), "default".to_string())
            ]
        );
    }

    /// compose: {{base}} in masters, examples and instance props resolves to the page path to
    /// design/ (the Design System page assertions belong to design/system_page)
    #[test]
    fn base_placeholder_resolves_to_the_page_path() {
        let root = project(&[
            ("design/system/components/logo.html", "<dfw-component name=\"logo\" props=\"img={{base}}assets/logo.png\"><template><a href=\"{{base}}screens/home/a.html\"><img src=\"{{img}}\" alt=\"\"></a></template><dfw-examples><dfw-use c=\"logo\"></dfw-use></dfw-examples></dfw-component>"),
            ("design/system/components/tile.html", "<dfw-component name=\"tile\" props=\"base\"><template><i data-b=\"{{base}}\"></i></template></dfw-component>"),
        ]);
        let with_base = |html: &str, base: &str| {
            compose_html(
                &root,
                html,
                ComposeOptions {
                    lib: None,
                    design_base: Some(base),
                },
            )
            .unwrap()
            .html
        };
        let screen = with_base("<dfw-use c=\"logo\"></dfw-use><dfw-use c=\"logo\" img=\"{{base}}assets/alt.png\"></dfw-use>", "../../");
        assert!(
            screen.contains("href=\"../../screens/home/a.html\""),
            "{screen}"
        );
        assert!(screen.contains("src=\"../../assets/logo.png\""));
        assert!(screen.contains("src=\"../../assets/alt.png\""));
        assert!(with_base("<dfw-use c=\"logo\"></dfw-use>", "/design/")
            .contains("src=\"/design/assets/logo.png\""));
        assert!(
            compose(&root, "<dfw-use c=\"tile\" base=\"x\"></dfw-use>")
                .html
                .contains("data-b=\"x\""),
            "a master that declares a \"base\" prop keeps it"
        );
    }

    /// 1: an unclosed <dfw-use> is reported and does not swallow the page; comments are not
    /// expanded (tests/unit/audit-fixes.test.mjs; the lintScreen assertion belongs to design/lint)
    #[test]
    fn unclosed_instance_is_reported_and_comments_are_not_expanded() {
        let root = project(&[(
            "design/system/components/divider.html",
            &master("divider", "<hr class=\"divider\">", ""),
        )]);
        let unclosed = "<body>\n<dfw-use c=\"divider\">\n<main data-dfw-id=\"main\"><h1>Title</h1></main>\n</body>";
        let r = compose(&root, unclosed);
        assert!(r.html.contains("<main data-dfw-id=\"main\">"));
        assert_eq!(r.html, unclosed);
        let got: Vec<(&str, &str, i64)> =
            r.issues.iter().map(|i| (i.rule, i.level, i.line)).collect();
        assert_eq!(got, [("component-unclosed", "error", 2)]);
        assert_eq!(
            r.issues[0].message,
            "<dfw-use> has no closing </dfw-use> tag."
        );

        let commented = "<body>\n<!-- TODO replace with <dfw-use c=\"divider\"> later -->\n<main>x</main>\n</body>";
        let c = compose(&root, commented);
        assert_eq!(c.html, commented);
        assert!(c.issues.is_empty());
    }

    /// 8: data-dfw-if with single quotes is evaluated; a template may start with a comment
    #[test]
    fn single_quoted_condition_and_leading_comment() {
        let root = project(&[
            ("design/system/components/c1.html", &master("c1", "<div class=\"c1\"><span data-dfw-if='a'>A</span><b data-dfw-if=\"b\">B</b><i data-dfw-if=a>I</i></div>", " props=\"a b\"")),
            ("design/system/components/c2.html", &master("c2", "<!-- root -->\n<div class=\"c2\"><dfw-slot></dfw-slot></div>", "")),
        ]);
        assert_eq!(
            compose(&root, "<dfw-use c=\"c1\" a=\"1\"></dfw-use>").html,
            "<div class=\"c1\" data-dfw-component=\"c1\"><span>A</span><i>I</i></div>"
        );
        assert_eq!(
            compose(
                &root,
                "<dfw-use c=\"c2\" id=\"x.y\" class=\"wide\">hi</dfw-use>"
            )
            .html,
            "<div class=\"c2 wide\" data-dfw-id=\"x.y\" data-dfw-component=\"c2\">hi</div>"
        );
    }

    /// slot content: slot children render inside the master-built container (the composeHtml
    /// assertion of tests/unit/edit.test.mjs)
    #[test]
    fn slot_children_render_inside_the_master_container() {
        let root = project(&[("design/system/components/app-shell.html", "<dfw-component name=\"app-shell\">\n<template>\n<div class=\"shell\"><nav data-dfw-id=\"shell.nav\">Nav</nav><main class=\"shell-main\" data-dfw-id=\"shell.main\"><dfw-slot name=\"main\"></dfw-slot></main></div>\n</template>\n</dfw-component>")]);
        let r = compose(&root, "<dfw-use c=\"app-shell\">\n  <dfw-fill slot=\"main\">\n      <section data-dfw-id=\"orders.summary\">S</section>\n  </dfw-fill>\n</dfw-use>");
        assert!(r.html.contains("<main class=\"shell-main\" data-dfw-id=\"shell.main\">\n      <section data-dfw-id=\"orders.summary\">"), "{}", r.html);
    }

    #[test]
    fn conditions_slots_and_root_attributes_follow_the_node_rules() {
        let root = project(&[
            ("design/system/components/f.html", &master("f", "<p class=\"f f--{{ variant }}\" data-x='1'\n   data-dfw-if=\"variant!=hidden\"><em data-dfw-if=\"!note\">none</em><em data-dfw-if=\"note\">{{note}}</em><dfw-slot>fallback</dfw-slot><dfw-slot name=\"end\"/><dfw-slot name='odd'></dfw-slot>{{missing}}{{ tone==warn ? a < b }}</p>", " variants=\"plain hidden\" sizes=\"md\" props=\"note, tone=warn\"")),
        ]);
        let r = compose(&root, "<dfw-use c=\"f\" note=\"a &amp; &lt;b&gt;\" hidden data-k=\"\" title=\"\" aria-label=\"\" data-dfw-id=\"lost\">  <dfw-fill slot=\"end\">E</dfw-fill></dfw-use>");
        assert_eq!(
            r.html,
            "<p class=\"f f--plain\" data-x=\"1\" hidden data-k=\"\" title aria-label=\"\" data-dfw-component=\"f\" data-dfw-variant=\"plain\" data-dfw-size=\"md\"><em>a &amp; &lt;b&gt;</em>fallbackE<dfw-slot name='odd'></dfw-slot>a &lt; b </p>"
        );
        assert_eq!(
            compose(&root, "<dfw-use c=\"f\" variant=\"hidden\">x</dfw-use>").html,
            ""
        );
        let bad = compose(
            &root,
            "a\n<dfw-use c=\"f\" variant=\"nope\" size=\"xl\" id=\"\">x</dfw-use>",
        );
        assert_eq!(rules(&bad), ["component-variant", "component-size"]);
        assert_eq!(
            bad.issues[0].message,
            "Component \"f\" has no variant \"nope\" (variants: plain, hidden)."
        );
        assert_eq!(bad.issues[1].line, 2);
        assert_eq!(bad.instances[0].id, None);
        assert!(bad.html.contains("data-dfw-id=\"\""));
    }

    #[test]
    fn icons_head_and_build_outputs() {
        let root = project(&[
            ("design/system/icons/x.svg", "<?xml version=\"1.0\"?>\n<!-- lucide -->\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"24\" height=\"24\" class=\"lucide\"><path d=\"M1 1\"/></svg>\n"),
            ("design/system/head.html", "<!-- fonts -->\n<link rel=\"stylesheet\" href=\"{{base}}fonts.css\">\n"),
            ("design/system/components/b.html", "<dfw-component name=\"b\"><style>\n.b { color: red; }\n</style><template><b class=\"b\"><dfw-slot></dfw-slot></b></template></dfw-component>"),
            ("design/styles/base.css", "body{margin:0}"),
            ("design/tokens/tokens.css", ":root{}"),
            ("design/screens/s/a.html", "<html>\n<head>\n<dfw-head>\n</dfw-head>\n</head>\n<body>\n<dfw-use c=\"b\" id=\"go\">Go</dfw-use><dfw-icon name=\"x\" size=\"16\" label=\"Close\" id=\"i\" class=\"big\"></dfw-icon><dfw-icon name=\"../x\"></dfw-icon></body></html>"),
            ("design/screens/s/plain.html", "<html><body>plain</body></html>"),
        ]);
        let file = compose_file(&root, "design/screens/s/a.html").unwrap();
        assert!(file.needs_components_css);
        let c = &file.composed;
        assert!(c.html.starts_with("<html>\n<head>\n<link rel=\"stylesheet\" href=\"../../fonts.css\">\n<link rel=\"stylesheet\" href=\"../../tokens/tokens.css\">\n<link rel=\"stylesheet\" href=\"../../styles/base.css\">\n<link rel=\"stylesheet\" href=\"../../system/components.css\">\n</head>"), "{}", c.html);
        assert!(c
            .html
            .contains("<b class=\"b\" data-dfw-id=\"go\" data-dfw-component=\"b\">Go</b>"));
        assert!(c.html.contains("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"16\" height=\"16\" class=\"lucide icon big\" role=\"img\" aria-label=\"Close\" data-dfw-icon=\"x\" data-dfw-id=\"i\"><path d=\"M1 1\"/></svg>"), "{}", c.html);
        assert_eq!(c.instances[0].line, 7, "lines are those of the source file");
        assert_eq!(rules(c), ["icon-unknown"]);
        assert_eq!(c.issues[0].line, 7);

        let built = build_system(&root).unwrap();
        assert_eq!(
            (built.components, built.written.as_slice()),
            (1, &["design/system/components.css".to_string()][..])
        );
        assert_eq!(fs::read_to_string(root.join("design/system/components.css")).unwrap(), "/* Generated by dfw from design/system/components/*.html. Do not edit by hand. */\n\n/* b (design/system/components/b.html) */\n.b { color: red; }\n");
        assert!(
            build_system(&root).unwrap().written.is_empty(),
            "unchanged text is not rewritten"
        );

        let rels = [
            "design/screens/s/a.html".to_string(),
            "design/screens/s/plain.html".to_string(),
            "design/nope.html".to_string(),
        ];
        let out = build_composed(&root, &rels).unwrap();
        assert_eq!(out.len(), 2);
        assert!(out[0].composed && !out[1].composed);
        assert_eq!(out[1].out, root.join("design/screens/s/plain.html"));
        let html = fs::read_to_string(&out[0].out).unwrap();
        assert!(html.starts_with("<html>\n<head>\n<base href=\"../../../../../design/screens/s/\">\n<link rel=\"stylesheet\" href=\"../../fonts.css\">\n<style data-dfw-inlined=\"../../tokens/tokens.css\">\n:root{}\n</style>\n<style data-dfw-inlined=\"../../styles/base.css\">\nbody{margin:0}\n</style>"), "{html}");
        assert_eq!(list_icons(&root).unwrap(), ["x"]);
        assert_eq!(design_base_for("design/screens/a/b.html"), "../../");
        assert_eq!(design_base_for("design/index.html"), "./");
    }
}
