//! Ports `cmdHandoff` of `core/lib/cli.mjs`: the handoff document of an approved design. It is
//! a module of its own so `cli/tools.rs` stays a list of command handlers.

use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::{json, Value};

use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::paths::{plugin_root, project_paths};
use crate::core::util::{get_path, list_files, read_json};
use crate::review::comments::all_comments;
use crate::workflow::gates::relative;
use crate::workflow::state::{current_approval, js_str, load_state, truthy, type_error};

/// `text.replaceAll(needle, replacement)` with a string pattern: `$$`, `$&`, `` $` `` and `$'`
/// in the replacement mean what JavaScript says, so a product name or a comment with a dollar
/// sign lands in the handoff as it does from Node.
fn js_replace_all(text: &str, needle: &str, replacement: &str) -> String {
    if !replacement.contains('$') {
        return text.replace(needle, replacement);
    }
    let mut result = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in text.match_indices(needle) {
        result.push_str(&text[last..at]);
        last = at + needle.len();
        let mut chars = replacement.chars().peekable();
        while let Some(c) = chars.next() {
            let special = match (c, chars.peek()) {
                ('$', Some('$')) => Some("$"),
                ('$', Some('&')) => Some(needle),
                ('$', Some('`')) => Some(&text[..at]),
                ('$', Some('\'')) => Some(&text[last..]),
                _ => None,
            };
            match special {
                Some(piece) => {
                    result.push_str(piece);
                    chars.next();
                }
                None => result.push(c),
            }
        }
    }
    result.push_str(&text[last..]);
    result
}

/// `(value || []).map(...)`: the items of a list that may be missing.
fn items<'a>(value: Option<&'a Value>, what: &str) -> Result<&'a [Value], DfwError> {
    match truthy(value) {
        None => Ok(&[]),
        Some(Value::Array(list)) => Ok(list),
        Some(_) => Err(DfwError::internal(format!(
            "({what} || []).map is not a function"
        ))),
    }
}

/// `(list || []).map((s) => s.name || s).join(', ')` of the stack text.
fn names(value: Option<&Value>, what: &str) -> Result<String, DfwError> {
    let parts: Vec<String> = items(value, what)?
        .iter()
        .map(|item| match truthy(item.get("name")).unwrap_or(item) {
            Value::Null => String::new(),
            other => js_str(Some(other)),
        })
        .collect();
    Ok(parts.join(", "))
}

/// `value || fallback` in a template literal.
fn text_or(value: Option<&Value>, fallback: &str) -> String {
    truthy(value).map_or_else(|| fallback.to_string(), |v| js_str(Some(v)))
}

/// `cmdHandoff(root)`: write the `design/handoff/<version>.md` skeleton of the approved design
/// and return the line to print.
pub fn write_handoff(root: &Path) -> Result<String, DfwError> {
    let p = project_paths(root);
    let state = load_state(root)?.ok_or_else(|| {
        DfwError::new(format!(
            "No dfw workflow in {}. Run \"dfw init\" first.",
            root.display()
        ))
    })?;
    let Some(appr) = current_approval(&state)? else {
        return Err(DfwError::new(
            "Handoff is generated from an approved design (G2).",
        ));
    };
    let version = js_str(appr.get("version"));
    let dir = p.approved.join(&version);
    let m = read_json(&dir.join("design.json"), Some(json!({})))?;
    let stack = read_json(&p.stack, Some(Value::Null))?;
    let tokens_css = dir.join("tokens").join("tokens.css");
    let tokens = if tokens_css.exists() {
        // `--[\w-]+(?=:)`: the colon cannot start another match, so counting it in is the same.
        let property =
            Regex::new(r"--[A-Za-z0-9_-]+:").map_err(|err| DfwError::internal(err.to_string()))?;
        property
            .find_iter(&fs::read_to_string(&tokens_css)?)
            .count()
    } else {
        0
    };
    let comps: Vec<String> = list_files(&dir.join("components"), &[])?
        .into_iter()
        .filter(|f| f.ends_with(".html"))
        .collect();
    let resolved: Vec<Value> = all_comments(root)?
        .into_iter()
        .filter(|c| {
            c.get("status").and_then(Value::as_str) == Some("resolved")
                && c.get("designVersion") == appr.get("version")
        })
        .collect();
    let file = p.handoff.join(format!("{version}.md"));
    let rel = relative(root, &file);
    if file.exists() {
        return Err(DfwError::new(format!("{rel} already exists.")));
    }
    // TODO(merge): use the embedded templates of `assets` when they exist.
    let templates = plugin_root().map(|r| r.join("templates")).ok_or_else(|| {
        DfwError::new(
            "The dfw templates were not found. Set DFW_PLUGIN_ROOT to the dfw plugin folder.",
        )
    })?;
    let tpl = fs::read_to_string(templates.join("handoff.md"))?;

    let mut screens = Vec::new();
    for s in items(m.get("screens"), "m.screens")? {
        let variants = s
            .get("variants")
            .and_then(Value::as_array)
            .ok_or_else(|| type_error("find"))?;
        // `v.id === s.approvedVariant` also holds when both are missing.
        let chosen = variants
            .iter()
            .find(|v| v.get("id") == s.get("approvedVariant"))
            .or_else(|| {
                variants
                    .iter()
                    .find(|v| truthy(v.get("recommended")).is_some())
            })
            .or_else(|| variants.first());
        screens.push(format!(
            "| `{}` | {} | `{}` (`design/approved/{version}/{}`) | {} |",
            js_str(s.get("id")),
            text_or(s.get("title"), ""),
            js_str(chosen.and_then(|c| c.get("id"))),
            js_str(chosen.and_then(|c| c.get("file"))),
            text_or(s.get("route"), "TBD"),
        ));
    }
    let stack_text = if truthy(Some(&stack)).is_some() {
        let or = |text: String, fallback: &str| {
            if text.is_empty() {
                fallback.to_string()
            } else {
                text
            }
        };
        let approved = truthy(state.get("gates").and_then(|g| g.get("G3"))).is_some();
        format!(
            "{}; styling: {}; UI libraries: {} ({})",
            text_or(stack.get("framework").and_then(|f| f.get("name")), "TBD"),
            or(names(stack.get("styling"), "stack.styling")?, "TBD"),
            or(
                names(stack.get("uiLibraries"), "stack.uiLibraries")?,
                "none"
            ),
            if approved {
                "approved"
            } else {
                "NOT YET APPROVED"
            }
        )
    } else {
        "NOT SET - gate G3 required".to_string()
    };
    let config;
    let breakpoints = match truthy(m.get("breakpoints")) {
        Some(own) => Some(own),
        None => {
            config = load_config(Some(root))?;
            get_path(&config, "studio.breakpoints")
        }
    };
    let breakpoints: Vec<String> = items(breakpoints, "m.breakpoints")?
        .iter()
        .map(|b| format!("{} {}px", js_str(b.get("id")), js_str(b.get("width"))))
        .collect();
    let or_text = |lines: Vec<String>, fallback: &str| {
        if lines.is_empty() {
            fallback.to_string()
        } else {
            lines.join("\n")
        }
    };
    let components = comps
        .iter()
        .map(|c| format!("- `design/approved/{version}/components/{c}`"))
        .collect();
    let tokens_text = if tokens > 0 {
        format!("{tokens} CSS custom properties in `design/approved/{version}/tokens/tokens.css` (source: `tokens/tokens.json`, W3C DTCG).")
    } else {
        "No tokens found.".to_string()
    };
    let resolved_lines = resolved
        .iter()
        .map(|c| {
            format!(
                "- {}: {} -> {}",
                js_str(c.get("id")),
                js_str(c.get("comment")),
                text_or(c.get("resolution").and_then(|r| r.get("note")), "")
            )
        })
        .collect();
    let product = truthy(m.get("product")).or_else(|| truthy(state.get("product")));
    let mut body = tpl;
    for (mark, value) in [
        ("{{version}}", version.clone()),
        ("{{hash}}", js_str(appr.get("hash"))),
        ("{{product}}", text_or(product, "")),
        ("{{approvedAt}}", js_str(appr.get("at"))),
        ("{{stack}}", stack_text),
        ("{{screens}}", or_text(screens, "| | | | |")),
        ("{{components}}", or_text(components, "- (none extracted)")),
        ("{{tokens}}", tokens_text),
        ("{{breakpoints}}", breakpoints.join(", ")),
        ("{{resolved}}", or_text(resolved_lines, "- none")),
    ] {
        body = js_replace_all(&body, mark, &value);
    }
    fs::create_dir_all(&p.handoff)?;
    fs::write(&file, body)?;
    Ok(format!("Wrote {rel}. Fill in the sections marked TODO with the user's requirements before implementing."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_patterns_follow_javascript() {
        assert_eq!(js_replace_all("a {{x}} b {{x}}", "{{x}}", "1"), "a 1 b 1");
        assert_eq!(
            js_replace_all("a {{x}} b", "{{x}}", "$$5 $& $1"),
            "a $5 {{x}} $1 b"
        );
        assert_eq!(
            js_replace_all("a {{x}} b", "{{x}}", "[$`|$']"),
            "a [a | b] b"
        );
        assert_eq!(js_replace_all("{{x}}", "{{x}}", "cost: 5$"), "cost: 5$");
    }
}
