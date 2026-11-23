//! Ports `core/lib/lint.mjs`: dfw's own design-workspace lint: manifest integrity, stable
//! element ids, token usage and a few accessibility basics. Fast and dependency-free so it can
//! run inside the PostToolUse hook.
//!
//! The Node module is a list of regular expressions over the raw text, and a finding of the
//! native lint must be the finding Node reports for the same bytes, hostile input included (a
//! tag inside a comment is still reported). So the rules keep their expressions (`jsre`
//! translates them to the `regex` crate) instead of moving to the tokenizer. `design::html` is
//! used where it reads the same thing: line numbers, and the extent of an instance whose master
//! accepts raw elements (`findElement`). What a screen composes comes from `design::compose`.
//!
//! Split by job: `jsre` (JavaScript regular expressions), `slop` (generic-AI tells and CSS
//! guideline rules), `tokens` (the token tree), `manifest` (design.json), this file (HTML,
//! screens, masters, CSS and the walk over the workspace).

mod manifest;
mod slop;
#[cfg(test)]
mod tests;
mod tokens;

use std::path::Path;

use serde_json::{json, Value};

pub use self::manifest::lint_manifest;
use self::slop::{slop_css, slop_html};
pub use self::tokens::lint_tokens;
use super::compose::{compose_html, design_base_for, load_components, ComposeOptions, Library};
use super::html::{element_at, line_of};
use super::tokens::relative_path;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::{blank_css_comments, js_re, js_regex};
use crate::core::level::Level;
use crate::core::paths::{self, project_paths, resolve, to_posix};
use crate::core::util::{list_files, read_json};
use crate::design::manifest::Manifest;

/// Reports a finding: level, rule, byte offset in the linted text (`None` is line 1), message.
type Add<'a> = dyn FnMut(&'static str, &'static str, Option<usize>, String) + 'a;

/// A finding of dfw's own lint, with the keys in the order the Node code wrote them.
fn finding(level: &str, rule: &str, file: &str, line: usize, message: String) -> Value {
    json!({ "level": level, "rule": rule, "file": file, "line": line, "message": message, "source": "dfw" })
}

js_re!(STYLE_BLOCK, r"<style[^>]*>([\s\S]*?)<\/style>", "i");
js_re!(
    COLOR,
    r"#(?:[0-9a-fA-F]{3,4}){1,2}\b|\b(?:rgba?|hsla?|oklch|oklab|lab|lch|color)\("
);
js_re!(CUSTOM_PROPERTY_BEFORE, r"--[\w-]+\s*:\s*[^;]*$");
js_re!(HTML_OPEN, "<html", "i");
js_re!(HTML_LANG, r"<html[^>]*\blang=", "i");
js_re!(TOKENS_LINK, r"tokens\.css|<dfw-head\b");
js_re!(DFW_ID, r#"data-dfw-id\s*=\s*"([^"]*)""#);
js_re!(DFW_ID_FORMAT, "^[a-z0-9][a-z0-9._-]*$");
js_re!(BODY, "<body", "i");
js_re!(
    STRUCTURAL,
    r"<(button|a|input|select|textarea|nav|header|main|footer|section|form|table|dialog|aside|img|h1|h2|h3|ul|ol|label)\b([^>]*)>",
    "i"
);
js_re!(HAS_DFW_ID, r"data-dfw-id\s*=");
js_re!(HAS_ALT, r"\balt\s*=");
js_re!(HAS_STYLE, r"\bstyle\s*=");

// Visual properties belong to the design system (tokens + component masters); screens may
// only arrange components (layout).
js_re!(
    VISUAL_PROPS,
    r"^(color|background(-color|-image)?|border(-(top|right|bottom|left))?(-color)?|outline-color|font(-family|-size|-weight|-style)?|line-height|letter-spacing|text-transform|box-shadow|border-radius|text-decoration(-color)?|fill|stroke)$"
);
js_re!(DECLARATION, r"(^|[;{\s])([a-z-]+)\s*:\s*([^;{}]+)");
js_re!(INLINE_STYLE, r#"\sstyle\s*=\s*"([^"]*)""#);
js_re!(USE_WITH_C, r#"<dfw-use\b[^>]*\bc="([\w-]+)""#);

// Data surfaces and forms must design their non-happy states (the most common silent gap in
// AI-built UI).
js_re!(
    DATA_SURFACE,
    r#"<table\b|<dfw-use\b[^>]*\bc="[\w-]*(?:table|list|grid|feed)[\w-]*""#,
    "i"
);
js_re!(
    FORM,
    r#"<form\b|<dfw-use\b[^>]*\bc="[\w-]*form[\w-]*""#,
    "i"
);

js_re!(COMPONENT_TAG, r"<dfw-component\b");
js_re!(COMPONENT_OPEN, r"<dfw-component\b([^>]*)>");
js_re!(COMPONENT_NAME, r#"\bname\s*=\s*"[a-z][a-z0-9-]*""#);
js_re!(TEMPLATE_TAG, r"<template\b");
js_re!(TEMPLATE, r"<template\b[^>]*>([\s\S]*?)<\/template>");
js_re!(TEMPLATE_ROOT, r"^\s*<([a-z][\w-]*)\b([^>]*)>", "i");
js_re!(
    TEMPLATE_ROLE,
    r#"^\s*<[^>]*role="(?:button|tab|switch|menuitem)""#
);
js_re!(
    DIMENSION,
    r"(?:padding|margin|gap|font-size|border-radius)[\w-]*\s*:\s*([^;}]*)"
);
js_re!(VAR_CALL, r"var\([^)]*\)");
js_re!(PX, r"\b(\d+(?:\.\d+)?)px\b");
js_re!(FIELD_ROOT, "^(input|select|textarea)$");
js_re!(APPEARANCE_NONE, r"appearance\s*:\s*none");
js_re!(OUTLINE_NONE, r"outline\s*:\s*(none|0)\b");
js_re!(HOVER, ":hover");
js_re!(DISABLED, r":disabled|\[disabled\]|\[aria-disabled");
js_re!(FOCUS_VISIBLE_OR_WITHIN, ":focus-visible|:focus-within");

js_re!(TOKENS_CSS_FILE, r"tokens\/tokens\.css$");
js_re!(CUSTOM_PROPERTY_START, r"^\s*--[\w-]+\s*:");
js_re!(TRANSITION_ALL, r"transition\s*:\s*all\b");

/// The content of one `<style>` element.
struct StyleBlock<'a> {
    css: &'a str,
    /// Where the Node code takes `css` to start: the first place its text occurs in the whole
    /// element (`sm.index + sm[0].indexOf(sm[1])`), which for an empty or repeated text is not
    /// always after the opening tag. Only line numbers depend on it.
    offset: usize,
}

fn style_blocks(text: &str) -> Vec<StyleBlock<'_>> {
    STYLE_BLOCK
        .captures_iter(text)
        .filter_map(|c| {
            let (whole, css) = (c.get(0)?, c.get(1)?.as_str());
            Some(StyleBlock {
                css,
                offset: whole.start() + whole.as_str().find(css).unwrap_or(0),
            })
        })
        .collect()
}

/// Colours written in a `<style>` block outside a custom property definition.
fn hardcoded_colors<'a>(block: &StyleBlock<'a>) -> Vec<(usize, &'a str)> {
    COLOR
        .find_iter(block.css)
        // The 80 characters before the colour decide: `--name: ... <colour>` defines a token.
        .filter(|m| !CUSTOM_PROPERTY_BEFORE.is_match(js::suffix16(&block.css[..m.start()], 80)))
        .map(|m| (block.offset + m.start(), m.as_str()))
        .collect()
}

/// `lintHtml(rel, text)`: lint one HTML screen file.
pub fn lint_html(rel: &str, text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut add =
        |level: &'static str, rule: &'static str, index: Option<usize>, message: String| {
            let line = index.map_or(1, |i| line_of(text, i));
            out.push(finding(level, rule, rel, line, message));
        };

    if let Some(html) = HTML_OPEN.find(text).filter(|_| !HTML_LANG.is_match(text)) {
        add(
            Level::Warn.as_str(),
            "html-lang",
            Some(html.start()),
            "<html> has no lang attribute.".into(),
        );
    }
    if !TOKENS_LINK.is_match(text) {
        add(
            Level::Warn.as_str(),
            "tokens-link",
            None,
            format!(
                "Screen does not load {}/{}; values may not come from tokens.",
                paths::file::DESIGN,
                paths::in_design::TOKENS_CSS
            ),
        );
    }

    // Stable ids.
    let mut ids: Vec<(&str, usize)> = Vec::new();
    for c in DFW_ID.captures_iter(text) {
        let (Some(whole), Some(id)) = (c.get(0), c.get(1)) else {
            continue;
        };
        let (at, id) = (whole.start(), id.as_str());
        if js::trim(id).is_empty() {
            add(
                Level::Error.as_str(),
                "dfw-id-empty",
                Some(at),
                "Empty data-dfw-id.".into(),
            );
        } else if !DFW_ID_FORMAT.is_match(id) {
            add(Level::Warn.as_str(), "dfw-id-format", Some(at), format!("data-dfw-id \"{id}\" should be lowercase dot.separated (e.g. \"filter-bar.search\")."));
        }
        match ids.iter().find(|(seen, _)| *seen == id) {
            Some((_, first)) => add(
                Level::Error.as_str(),
                "dfw-id-duplicate",
                Some(at),
                format!(
                    "Duplicate data-dfw-id \"{id}\" (first on line {}).",
                    line_of(text, *first)
                ),
            ),
            None => ids.push((id, at)),
        }
    }
    let body = BODY.find(text).map(|m| m.start());
    let mut missing = 0;
    let mut first_missing = None;
    for c in STRUCTURAL.captures_iter(text) {
        let (Some(whole), Some(name), Some(attrs)) = (c.get(0), c.get(1), c.get(2)) else {
            continue;
        };
        if body.is_some_and(|b| whole.start() < b) {
            continue;
        }
        let (name, attrs) = (name.as_str(), attrs.as_str());
        if !HAS_DFW_ID.is_match(attrs) {
            missing += 1;
            first_missing.get_or_insert(whole.start());
        }
        if name.eq_ignore_ascii_case("img") && !HAS_ALT.is_match(attrs) {
            add(
                Level::Error.as_str(),
                "img-alt",
                Some(whole.start()),
                "<img> without alt (WCAG 1.1.1). Use alt=\"\" for decorative images.".into(),
            );
        }
        if name.eq_ignore_ascii_case("button") && HAS_STYLE.is_match(attrs) {
            add(
                Level::Warn.as_str(),
                "inline-style",
                Some(whole.start()),
                "Inline style on a button; use classes and tokens.".into(),
            );
        }
    }
    if missing > 0 {
        add(Level::Warn.as_str(), "dfw-id-missing", first_missing, format!("{missing} structural/interactive element(s) have no data-dfw-id; click-to-comment cannot target them reliably."));
    }

    // Inline <style> colors outside tokens.
    for block in style_blocks(text) {
        for (at, color) in hardcoded_colors(&block) {
            add(
                Level::Warn.as_str(),
                "hardcoded-color",
                Some(at),
                format!("Hard-coded color \"{color}\" in <style>; use a token (var(--...))."),
            );
        }
    }
    slop_html(text, &mut add);
    out
}

/// The message of `no-components`.
fn no_components() -> String {
    format!(
        "Screen uses no design-system components; build screens from {}.",
        paths::rel::COMPONENTS
    )
}

/// `lintScreen(root, rel, text, lib)`: screen-only rules: compose issues, visual styling
/// outside the design system, raw elements that have a component, state coverage.
// Over 120 lines, one function as in lint.mjs: the rules run in the order their findings are
// printed and share the composed result.
pub fn lint_screen(
    root: &Path,
    rel: &str,
    text: &str,
    lib: &Library,
) -> Result<Vec<Value>, DfwError> {
    let mut out = Vec::new();
    let (warn, error) = (Level::Warn.as_str(), Level::Error.as_str());
    let mut add = |level: &'static str, rule: &'static str, line: usize, message: String| {
        out.push(finding(level, rule, rel, line.max(1), message));
    };
    let base = design_base_for(rel);
    let composed = compose_html(
        root,
        text,
        ComposeOptions {
            lib: Some(lib),
            design_base: Some(&base),
        },
    )?;
    for issue in &composed.issues {
        add(
            issue.level,
            issue.rule,
            usize::try_from(issue.line).unwrap_or(0),
            issue.message.clone(),
        );
    }
    if !lib.is_empty() {
        for block in style_blocks(text) {
            let css = blank_css_comments(block.css);
            for c in DECLARATION.captures_iter(&css) {
                let (Some(whole), Some(prop)) = (c.get(0), c.get(2)) else {
                    continue;
                };
                let prop = prop.as_str();
                if prop.starts_with("--") || !VISUAL_PROPS.is_match(prop) {
                    continue;
                }
                add(error, "screen-visual-style", line_of(text, block.offset + whole.start()), format!("\"{prop}\" set in a screen. Visual styling lives in the design system: use or extend a component variant ({}/) or a token; screens only handle layout.", paths::rel::COMPONENTS));
            }
        }
        for c in INLINE_STYLE.captures_iter(text) {
            let (Some(whole), Some(style)) = (c.get(0), c.get(1)) else {
                continue;
            };
            for declaration in style.as_str().split(';') {
                let name = declaration.split(':').next().unwrap_or_default();
                let prop = js::trim(name).to_lowercase();
                if !prop.is_empty() && !prop.starts_with("--") && VISUAL_PROPS.is_match(&prop) {
                    add(error, "screen-visual-style", line_of(text, whole.start()), format!("Inline \"{prop}\" in a screen; visual styling belongs to the design system."));
                }
            }
        }
        // Raw tag -> the component that replaces it. A Map: a tag named again keeps its place.
        let mut replaced: Vec<(&str, &str)> = Vec::new();
        for def in lib.iter() {
            for tag in &def.replaces {
                match replaced.iter_mut().find(|(t, _)| *t == tag.as_str()) {
                    Some(slot) => slot.1 = def.display_name(),
                    None => replaced.push((tag, def.display_name())),
                }
            }
        }
        let body = BODY.find(text).map_or(0, |m| m.start());
        // Raw elements inside instances whose master accepts them (segmented buttons, tabs) are fine.
        let mut allowed = Vec::new();
        for c in USE_WITH_C.captures_iter(text) {
            let (Some(whole), Some(name)) = (c.get(0), c.get(1)) else {
                continue;
            };
            let Some(def) = lib
                .get(Some(name.as_str()))
                .filter(|d| !d.accepts.is_empty())
            else {
                continue;
            };
            if let Some(el) = element_at(text, whole.start()) {
                allowed.push((el.inner, &def.accepts));
            }
        }
        for (tag, component) in replaced {
            let re = js_regex(&format!("<{tag}\\b"), "i")?;
            let mut count = 0;
            let mut first = 0;
            for m in re.find_iter(text) {
                let at = m.start();
                let accepted = allowed.iter().any(|(inner, tags)| {
                    at > inner.start && at < inner.end && tags.iter().any(|t| t == tag)
                });
                if at < body || accepted {
                    continue;
                }
                if count == 0 {
                    first = line_of(text, at);
                }
                count += 1;
            }
            if count > 0 {
                add(warn, "use-component", first, format!("{count} raw <{tag}> element(s); the design system has component \"{component}\" for this. Use <dfw-use c=\"{component}\">."));
            }
        }
        let primaries = text.matches("variant=\"primary\"").count();
        if primaries > 2 {
            add(warn, "primary-overuse", 1, format!("{primaries} primary-variant instances on one screen: the accent loses its meaning. Keep one primary action per area (two at most per screen)."));
        }
        let instances = &composed.instances;
        if instances.is_empty() && BODY.is_match(text) {
            add(warn, "no-components", line_of(text, body), no_components());
        }
        let shared = |name: &str| {
            lib.get(Some(name)).is_some()
                && !instances.is_empty()
                && !instances
                    .iter()
                    .any(|i| i.component.as_deref() == Some(name))
                && !rel.contains("/_")
        };
        if shared("page") {
            add(warn, "page-layout", 1, "Screen does not use the page layout master (<dfw-use c=\"page\" variant=\"task|flow|split\">): paddings, widths and overflow behaviour will drift from other screens.".into());
        }
        if shared("app-shell") {
            add(warn, "app-shell", 1, "Screen does not use the shared app-shell: every screen should be framed by the same navigation and layout (<dfw-use c=\"app-shell\" current=\"...\">).".into());
        }
    }
    out.extend(state_coverage(root, rel, text)?);
    Ok(out)
}

/// A design.json value of the wrong type: the lint stops (a manifest of the wrong shape never
/// passes a gate).
fn malformed(what: &str) -> DfwError {
    super::malformed(paths::rel::MANIFEST, what)
}

js_re!(STATE_NAME, r"^[a-z][a-z0-9-]*$");

/// The states, out of `states`, that this screen actually designs: a `.when-<state>` or
/// `.unless-<state>` class, or a `data-state` rule naming it, in the screen's source or in a
/// master it uses. A declared state without any is the default screen again (same frame).
pub fn designed_states(
    root: &Path,
    text: &str,
    states: &[String],
) -> Result<Vec<String>, DfwError> {
    let mut sources: Vec<String> = vec![text.to_string()];
    let components = project_paths(root).components;
    let mut seen: Vec<String> = Vec::new();
    for cap in USE_WITH_C.captures_iter(text) {
        let name = cap[1].to_string();
        if seen.contains(&name) {
            continue;
        }
        // A master is read only inside the project (rule 5); a missing one is the compose check's job.
        if let Ok(file) = crate::core::paths::resolve_inside(&components, &format!("{name}.html")) {
            if file.is_file() {
                sources.push(js::read_text(&file)?);
            }
        }
        seen.push(name);
    }
    let mut out = Vec::new();
    for state in states {
        let s = state.to_lowercase();
        if !STATE_NAME.is_match(&s) {
            continue;
        }
        let re = js_regex(
            &format!(
                r#"\b(?:when|unless)-{s}(?:$|[^\w-])|data-state\s*[~|^$*]?=\s*["']?{s}(?:$|[^\w-])"#
            ),
            "i",
        )?;
        if sources.iter().any(|t| re.is_match(t)) {
            out.push(state.clone());
        }
    }
    Ok(out)
}

/// `stateCoverage(root, rel, text)`: warn when a screen with a table/list or a form declares no
/// loading/empty/error states (design.json states), and when a declared state is not designed
/// (its frame would be the default screen again).
pub fn state_coverage(root: &Path, rel: &str, text: &str) -> Result<Vec<Value>, DfwError> {
    // A design.json that is not JSON is reported by lint_manifest (manifest-json).
    let Ok(manifest) = read_json(&project_paths(root).manifest, Some(Value::Null)) else {
        return Ok(Vec::new());
    };
    let file = paths::rel::inside(paths::rel::DESIGN, rel).unwrap_or(rel);
    let m = Manifest(&manifest);
    let Some(screen) = m.screen_of_file_checked(file)?.filter(|s| !s.is_private()) else {
        return Ok(Vec::new());
    };
    let declared = screen.declared_states_checked(m)?;
    let screen = screen.value();
    let states: Vec<String> = declared
        .iter()
        .map(|s| js::to_string(Some(s)).to_lowercase())
        .collect();
    let undesigned_finding = {
        // Only state names that can be designed (`.when-<name>`): other values are the manifest
        // check's business.
        let names: Vec<String> = declared
            .iter()
            .filter_map(Value::as_str)
            .filter(|s| STATE_NAME.is_match(&s.to_lowercase()))
            .map(str::to_string)
            .collect();
        let designed = designed_states(root, text, &names)?;
        let undesigned: Vec<&String> = names.iter().filter(|s| !designed.contains(s)).collect();
        (!undesigned.is_empty()).then(|| {
            let list: Vec<&str> = undesigned.iter().map(|s| s.as_str()).collect();
            let message = format!("Screen \"{}\" shows state {} but nothing in it changes for {}: the Studio does not draw those frames (they would repeat the default screen). Design each one (a .when-<state> block, e.g. a skeleton for loading, a message with an action for empty, what went wrong and how to recover for error), or remove it from the screen's \"states\" (a screen without its own \"states\" inherits the ones at the top of design.json).", js::to_string(screen.get("id")), list.join("/"), if list.len() > 1 { "them" } else { "it" });
            json!({ "level": Level::Warn.as_str(), "rule": "state-undesigned", "file": rel, "line": 1, "source": "dfw", "message": message })
        })
    };
    let (data, form) = (DATA_SURFACE.is_match(text), FORM.is_match(text));
    let mut need: Vec<&str> = Vec::new();
    if data {
        need.extend(["loading", "empty", "error"]);
    } else if form {
        need.push("error");
    }
    let missing: Vec<&str> = need
        .into_iter()
        .filter(|s| !states.iter().any(|have| have == s))
        .collect();
    if missing.is_empty() {
        return Ok(undesigned_finding.into_iter().collect());
    }
    let what = if data { "a data surface" } else { "a form" };
    let message = format!("Screen \"{}\" has {what} but no {} state. Add them to its \"states\" in design.json and design each (.when-<state> blocks): empty needs a headline, explanation and primary action; error says what happened and how to recover, keeping user input. Also check edge content (very long text, missing optional fields, large counts).", js::to_string(screen.get("id")), missing.join("/"));
    let mut out = vec![
        json!({ "level": Level::Warn.as_str(), "rule": "state-coverage", "file": rel, "line": 1, "source": "dfw", "message": message }),
    ];
    out.extend(undesigned_finding);
    Ok(out)
}

/// `lintComponentFile(rel, text)`: lint a component master file.
pub fn lint_component_file(rel: &str, text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    let mut add =
        |level: &'static str, rule: &'static str, index: Option<usize>, message: String| {
            let line = index.map_or(1, |i| line_of(text, i));
            out.push(finding(level, rule, rel, line, message));
        };
    if !COMPONENT_TAG.is_match(text) {
        add(
            Level::Error.as_str(),
            "component-missing",
            None,
            "Component file has no <dfw-component name=\"...\"> element.".into(),
        );
    }
    for c in COMPONENT_OPEN.captures_iter(text) {
        if !COMPONENT_NAME.is_match(&c[1]) {
            add(
                Level::Error.as_str(),
                "component-name",
                c.get(0).map(|m| m.start()),
                "Component needs name=\"kebab-case\".".into(),
            );
        }
    }
    if !TEMPLATE_TAG.is_match(text) {
        add(
            Level::Error.as_str(),
            "component-template",
            None,
            "Component has no <template>.".into(),
        );
    }
    let blocks = style_blocks(text);
    for block in &blocks {
        for (at, color) in hardcoded_colors(block) {
            add(
                Level::Error.as_str(),
                "hardcoded-color",
                Some(at),
                format!("Hard-coded color \"{color}\" in a component; use a color token."),
            );
        }
        for c in DIMENSION.captures_iter(block.css) {
            let (Some(whole), Some(value)) = (c.get(0), c.get(1)) else {
                continue;
            };
            let without_vars = VAR_CALL.replace_all(value.as_str(), "");
            let raw = PX
                .captures_iter(&without_vars)
                .any(|px| px[1].parse::<f64>().is_ok_and(|n| n > 2.0));
            if raw {
                add(
                    Level::Warn.as_str(),
                    "raw-dimension",
                    Some(block.offset + whole.start()),
                    format!(
                        "Raw length \"{}\" in a component; use space/font/radius tokens.",
                        js::trim(whole.as_str())
                    ),
                );
            }
        }
    }
    slop_html(text, &mut add);
    // Interactive masters design their state matrix: hover, keyboard focus, disabled.
    let template = TEMPLATE
        .captures(text)
        .and_then(|c| c.get(1))
        .map_or("", |m| m.as_str());
    // Only the root element counts (a card containing a link is not a control); native controls
    // keep their own states unless restyled.
    let root = TEMPLATE_ROOT
        .captures(template)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_lowercase())
        .unwrap_or_default();
    let css = blocks.iter().map(|b| b.css).collect::<Vec<_>>().join("\n");
    let mut need: Vec<(&str, &regex::Regex)> = Vec::new();
    if root == "button" || root == "a" || TEMPLATE_ROLE.is_match(template) {
        need.push((":hover", &HOVER));
    }
    if root == "button" || (FIELD_ROOT.is_match(&root) && APPEARANCE_NONE.is_match(&css)) {
        need.push(("disabled", &DISABLED));
    }
    if OUTLINE_NONE.is_match(&css) {
        need.push((":focus-visible", &FOCUS_VISIBLE_OR_WITHIN));
    }
    let missing: Vec<&str> = need
        .into_iter()
        .filter(|(_, re)| !re.is_match(&css))
        .map(|(name, _)| name)
        .collect();
    if !missing.is_empty() {
        add(Level::Warn.as_str(), "component-states", None, format!("Interactive component has no {} style. Design its state matrix (default, hover, focus-visible, active, disabled, plus loading/selected where they apply).", missing.join(" / ")));
    }
    out
}

/// `lintCss(rel, text)`: lint a CSS file that is not the generated tokens file.
pub fn lint_css(rel: &str, text: &str) -> Vec<Value> {
    let mut out = Vec::new();
    if TOKENS_CSS_FILE.is_match(rel) {
        return out;
    }
    let clean = blank_css_comments(text);
    let mut add =
        |level: &'static str, rule: &'static str, index: Option<usize>, message: String| {
            let line = index.map_or(1, |i| line_of(&clean, i));
            out.push(finding(level, rule, rel, line, message));
        };
    for m in COLOR.find_iter(&clean) {
        let before = &clean[..m.start()];
        let declaration = before.rfind(['{', ';']).map_or(0, |i| i + 1);
        // Defining a custom property is fine.
        if CUSTOM_PROPERTY_START.is_match(&before[declaration..]) {
            continue;
        }
        add(
            Level::Warn.as_str(),
            "hardcoded-color",
            Some(m.start()),
            format!(
                "Hard-coded color \"{}\"; use a token (var(--...)).",
                m.as_str()
            ),
        );
    }
    if let Some(m) = TRANSITION_ALL.find(&clean) {
        add(
            Level::Warn.as_str(),
            "transition-all",
            Some(m.start()),
            "\"transition: all\" animates unintended properties; list the properties.".into(),
        );
    }
    slop_css(&clean, 0, &mut add);
    if let Some(m) = OUTLINE_NONE
        .find(&clean)
        .filter(|_| !clean.contains(":focus-visible"))
    {
        add(
            Level::Error.as_str(),
            "focus-removed",
            Some(m.start()),
            "Focus outline removed without a :focus-visible replacement (WCAG 2.4.7).".into(),
        );
    }
    out
}

/// `/tokens\/.*\.json$/.test(rel) || rel.endsWith('.tokens.json')`.
fn is_token_file(rel: &str) -> bool {
    let in_tokens = rel.strip_suffix(".json").is_some_and(|stem| {
        stem.rfind("tokens/")
            .is_some_and(|i| !stem[i..].contains(['\n', '\r', '\u{2028}', '\u{2029}']))
    });
    in_tokens || rel.ends_with(".tokens.json")
}

/// The project-relative targets of [`lint_design`]: every file under a directory target, the
/// path itself otherwise.
fn lint_targets(root: &Path, files: Option<&[String]>) -> Result<Vec<String>, DfwError> {
    let Some(files) = files else {
        let all = list_files(&project_paths(root).design, &[paths::file::APPROVED])?;
        return Ok(all.into_iter().map(|r| paths::rel::design(&r)).collect());
    };
    let mut targets = Vec::new();
    for f in files {
        let abs = resolve(root.join(f));
        if abs.is_dir() {
            // A directory target lints every file under it.
            for r in list_files(&abs, &[])? {
                targets.push(to_posix(&relative_path(root, &abs.join(r))));
            }
        } else {
            targets.push(to_posix(&relative_path(root, &abs)));
        }
    }
    Ok(targets)
}

/// `lintDesign(root, files)`: lint the whole design workspace, or only the given files
/// (absolute or project-relative).
pub fn lint_design(root: &Path, files: Option<&[String]>) -> Result<Vec<Value>, DfwError> {
    let p = project_paths(root);
    let mut out = Vec::new();
    let targets = lint_targets(root, files)?;
    if files.is_none() || targets.iter().any(|t| t == paths::rel::MANIFEST) {
        out.extend(lint_manifest(root)?);
    }
    let lib = load_components(root)?;
    for rel in &targets {
        if paths::rel::under(paths::rel::APPROVED, rel) {
            continue;
        }
        let abs = root.join(rel);
        if !abs.exists() {
            continue;
        }
        let text = js::read_text(&abs)?;
        // head.html is a font links snippet, not a page; components.css is generated.
        if rel == paths::rel::HEAD {
            continue;
        }
        if paths::rel::under(paths::rel::COMPONENTS, rel) && rel.ends_with(".html") {
            out.extend(lint_component_file(rel, &text));
        } else if rel.ends_with(".html") {
            out.extend(lint_html(rel, &text));
            if paths::rel::under(paths::rel::SCREENS, rel) {
                out.extend(lint_screen(root, rel, &text, &lib)?);
            }
        } else if rel == paths::rel::COMPONENTS_CSS {
            continue;
        } else if rel.ends_with(".css") {
            out.extend(lint_css(rel, &text));
        } else if is_token_file(rel) {
            match serde_json::from_str::<Value>(&text) {
                Ok(json) => {
                    // A theme overrides part of tokens.json, so its aliases may point at base
                    // tokens. A broken tokens.json gets its own token-json finding.
                    let base = if paths::rel::under(paths::rel::THEMES, rel) {
                        read_json(&p.tokens_json, Some(Value::Null)).ok()
                    } else {
                        None
                    };
                    out.extend(lint_tokens(rel, &json, base.as_ref()));
                }
                Err(err) => out.push(finding(
                    Level::Error.as_str(),
                    "token-json",
                    rel,
                    1,
                    err.to_string(),
                )),
            }
        }
    }
    Ok(out)
}

/// Every constant expression of the lint, for the test that compiles them.
#[cfg(test)]
fn patterns() -> Vec<&'static regex::Regex> {
    let mut all: Vec<&'static regex::Regex> = vec![
        &STYLE_BLOCK,
        &COLOR,
        &CUSTOM_PROPERTY_BEFORE,
        &HTML_OPEN,
        &HTML_LANG,
        &TOKENS_LINK,
        &DFW_ID,
        &DFW_ID_FORMAT,
        &BODY,
        &STRUCTURAL,
        &HAS_DFW_ID,
        &HAS_ALT,
        &HAS_STYLE,
        &VISUAL_PROPS,
        &DECLARATION,
        &INLINE_STYLE,
        &USE_WITH_C,
        &DATA_SURFACE,
        &FORM,
        &COMPONENT_TAG,
        &COMPONENT_OPEN,
        &COMPONENT_NAME,
        &TEMPLATE_TAG,
        &TEMPLATE,
        &TEMPLATE_ROOT,
        &TEMPLATE_ROLE,
        &DIMENSION,
        &VAR_CALL,
        &PX,
        &FIELD_ROOT,
        &APPEARANCE_NONE,
        &OUTLINE_NONE,
        &HOVER,
        &DISABLED,
        &FOCUS_VISIBLE_OR_WITHIN,
        &TOKENS_CSS_FILE,
        &CUSTOM_PROPERTY_START,
        &TRANSITION_ALL,
    ];
    all.extend(slop::patterns());
    all.extend(tokens::patterns());
    all
}
