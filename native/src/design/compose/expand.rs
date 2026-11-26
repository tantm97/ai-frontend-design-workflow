//! Expanding a page (`composeHtml`): `<dfw-use>` instances (nested, with a depth limit),
//! `<dfw-icon>`, `<dfw-head>`, and the instances and issues found on the way.

use super::*;
use crate::core::level::{finding, Level as FindingLevel};
use crate::core::paths::file;

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

/// What every level of a page's expansion shares: the masters and [`ComposeOptions::prop_values`].
#[derive(Clone, Copy)]
struct Ctx<'a> {
    lib: &'a Library,
    prop_values: bool,
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
    let name = attrs.get(attr::NAME);
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
        (
            "aria-label".into(),
            attrs.get(attr::LABEL).map(str::to_string),
        ),
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
                .trim_start_matches(js::is_space)
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
    let p = project_paths(root);
    let d = &p.design;
    let head_file = &p.head;
    let mut parts = Vec::new();
    if head_file.exists() {
        let text = js::strip_comments(&js::read_text(head_file)?).replace("{{base}}", design_base);
        parts.push(js::trim(&text).to_string());
    }
    parts.push(format!(
        "<link rel=\"stylesheet\" href=\"{design_base}{}\">",
        in_design::TOKENS_CSS
    ));
    if d.join(file::STYLES).join(file::BASE_CSS).exists() {
        parts.push(format!(
            "<link rel=\"stylesheet\" href=\"{design_base}{}\">",
            in_design::BASE_CSS
        ));
    }
    parts.push(format!(
        "<link rel=\"stylesheet\" href=\"{design_base}{}\">",
        in_design::COMPONENTS_CSS
    ));
    let mut end = tag.range.end;
    let after = &html[end..];
    let unspaced = after.trim_start_matches(js::is_space);
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
    let inside = rel::inside(rel::DESIGN, &posix).unwrap_or(&posix);
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
                    map.insert(key::COMPONENT.into(), json!(component));
                }
                map.insert(key::VARIANT.into(), json!(i.variant));
                map.insert(key::SIZE.into(), json!(i.size));
                map.insert(key::ID.into(), json!(i.id));
                map.insert(key::LINE.into(), json!(i.line));
                Value::Object(map)
            })
            .collect();
        let issues: Vec<Value> = self
            .issues
            .iter()
            .map(|i| json!({ (finding::LEVEL): i.level, (finding::RULE): i.rule, (finding::MESSAGE): i.message, (finding::LINE): i.line }))
            .collect();
        json!({ (key::HTML): self.html, (key::INSTANCES): instances, (key::ISSUES): issues })
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
    /// Write each instance's own prop values (as written on its `<dfw-use>`, without the
    /// master's defaults) on its root as `data-dfw-props` JSON, for the Studio's inspector;
    /// nothing when it sets none.
    /// Off for every other output.
    pub prop_values: bool,
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
    let ctx = Ctx {
        lib,
        prop_values: opts.prop_values,
    };
    let level = compose_level(root, text, ctx, 0)?;
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
                level: FindingLevel::Error.as_str(),
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
    ctx: Ctx,
    depth: usize,
    level: &mut Level,
) -> Result<(), DfwError> {
    let start = tag.range.start;
    let attrs = attrs_of(html, &tag.attrs);
    let name = attrs.nonempty("c").or(attrs.nonempty("component"));
    let shown = name.unwrap_or("undefined");
    let Some(def) = ctx.lib.get(name) else {
        level.issues.push(RawIssue {
            rule: "component-unknown",
            index: start,
            message: format!("Unknown component \"{shown}\". Define it in {}/ first (design system before screens).", rel::COMPONENTS),
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
    let mut own = Map::new();
    for (k, v) in attrs.iter() {
        if RESERVED.contains(&k) {
            continue;
        }
        if def.props.iter().any(|p| p == k) {
            props.set(k, v);
            own.insert(k.to_string(), json!(v));
        } else {
            set_extra(&mut extra, k, Some(v.to_string()));
        }
    }
    let (mut slots, rest) = take_fills(inner);
    if slots.get(DEFAULT_SLOT).is_none() {
        slots.set(DEFAULT_SLOT, rest);
    }
    let body = fill_template(def, &props, &slots);
    let id = attrs.get(attr::ID);
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
    if ctx.prop_values && !own.is_empty() {
        // `{{` as JSON escapes, so the page's `{{base}}` pass leaves the values as written.
        let text = Value::Object(own)
            .to_string()
            .replace("{{", "\\u007b\\u007b");
        set_extra(&mut extra, "data-dfw-props", Some(text));
    }
    let body = annotate_root(&body, &extra, attrs.get(attr::CLASS));
    level.instances.push(RawInstance {
        component: name.map(str::to_string),
        variant,
        size,
        id: id.filter(|i| !i.is_empty()).map(str::to_string),
        index: start,
    });
    let nested = compose_level(root, &body, ctx, depth + 1)?;
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

fn compose_level(root: &Path, html: &str, ctx: Ctx, depth: usize) -> Result<Level, DfwError> {
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
                ctx,
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
