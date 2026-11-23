//! The component library: masters read from `design/system/components/*.html`
//! (`loadComponents`, `parseComponentFile`), icons and `componentsCss`.

use super::*;

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

pub(super) fn system_dir(root: &Path) -> PathBuf {
    project_paths(root).system
}

/// `loadComponents`: every master of `design/system/components/*.html`, files in name order.
pub fn load_components(root: &Path) -> Result<Library, DfwError> {
    let dir = project_paths(root).components;
    let mut lib = Library::default();
    let mut files: Vec<String> = dir_names(&dir)?
        .into_iter()
        .filter(|f| f.ends_with(".html"))
        .collect();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    for f in files {
        let text = js::read_text(&dir.join(&f))?;
        for def in parse_component_file(&text, &rel::component(&f)) {
            lib.insert(def);
        }
    }
    Ok(lib)
}

/// True when `text` starts with `prefix` and the name ends there (`\b` after a letter).
pub(super) fn starts_tag(text: &str, prefix: &str) -> bool {
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
        t.split(js::is_space)
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
                t = t[4 + len + 3..].trim_start_matches(js::is_space);
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
    let mut abs = project_paths(root).icons;
    abs.extend(file.split('/').filter(|part| !part.is_empty()));
    if abs.is_file() {
        js::read_text(&abs).map(Some)
    } else {
        Ok(None)
    }
}

/// `listIcons`: names of the icons in `design/system/icons/`, sorted.
pub fn list_icons(root: &Path) -> Result<Vec<String>, DfwError> {
    let mut names: Vec<String> = dir_names(&project_paths(root).icons)?
        .iter()
        .filter_map(|f| f.strip_suffix(".svg").map(str::to_string))
        .collect();
    names.sort_by(|a, b| js::cmp_utf16(a, b));
    Ok(names)
}

/// `componentsCss`: the styles of every master (`design/system/components.css` is this text).
pub fn components_css(lib: &Library) -> String {
    let mut parts = vec![format!(
        "/* Generated by dfw from {}/*.html. Do not edit by hand. */",
        rel::COMPONENTS
    )];
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
