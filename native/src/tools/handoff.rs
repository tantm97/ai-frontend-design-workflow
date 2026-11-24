//! Ports `cmdHandoff` of `core/lib/cli.mjs`: the handoff document of an approved design. It is
//! a module of its own so `cli/tools.rs` stays a list of command handlers.

use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::{json, Value};

use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, in_design, project_paths, rel};
use crate::core::util::{get_path, list_files, read_json, write_atomic_inside};
use crate::design::manifest::key;
use crate::review::comments::all_comments;
use crate::review::status::{self, Status};
use crate::workflow::freeze::check_snapshot_name;
use crate::workflow::gates::relative;
use crate::workflow::state::{current_approval, require_state};

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
    match js::truthy_value(value) {
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
        .map(
            |item| match js::truthy_value(item.get("name")).unwrap_or(item) {
                Value::Null => String::new(),
                other => js::to_string(Some(other)),
            },
        )
        .collect();
    Ok(parts.join(", "))
}

/// `value || fallback` in a template literal.
fn text_or(value: Option<&Value>, fallback: &str) -> String {
    js::truthy_value(value).map_or_else(|| fallback.to_string(), |v| js::to_string(Some(v)))
}

/// `lines.join('\n') || fallback`.
fn or_text(lines: Vec<String>, fallback: &str) -> String {
    if lines.is_empty() {
        fallback.to_string()
    } else {
        lines.join("\n")
    }
}

/// The table rows of the screens: each with its approved, else recommended, else first variant.
fn screen_rows(m: &Value, version: &str) -> Result<Vec<String>, DfwError> {
    let mut screens = Vec::new();
    for s in items(m.get(key::SCREENS), "m.screens")? {
        let variants = s
            .get(key::VARIANTS)
            .and_then(Value::as_array)
            .ok_or_else(|| {
                DfwError::new(format!(
                    "{}: screen \"{}\" has no \"variants\" list. The snapshot is frozen: run \"dfw verify-snapshot {version}\", and if it was edited restore it from version control.",
                    rel::join(&rel::approved(version), file::MANIFEST),
                    js::to_string(s.get("id"))
                ))
            })?;
        // `v.id === s.approvedVariant` also holds when both are missing.
        let chosen = variants
            .iter()
            .find(|v| v.get("id") == s.get("approvedVariant"))
            .or_else(|| {
                variants
                    .iter()
                    .find(|v| js::truthy_value(v.get("recommended")).is_some())
            })
            .or_else(|| variants.first());
        screens.push(format!(
            "| `{}` | {} | `{}` (`{}/{}`) | {} |",
            js::to_string(s.get("id")),
            text_or(s.get("title"), ""),
            js::to_string(chosen.and_then(|c| c.get("id"))),
            rel::approved(version),
            js::to_string(chosen.and_then(|c| c.get("file"))),
            text_or(s.get("route"), "TBD"),
        ));
    }
    Ok(screens)
}

/// The stack in one line, and whether G3 approved it.
fn stack_line(stack: &Value, state: &Value) -> Result<String, DfwError> {
    if js::truthy_value(Some(stack)).is_none() {
        return Ok("NOT SET - gate G3 required".to_string());
    }
    let or = |text: String, fallback: &str| {
        if text.is_empty() {
            fallback.to_string()
        } else {
            text
        }
    };
    let approved = js::truthy_value(state.get("gates").and_then(|g| g.get("G3"))).is_some();
    Ok(format!(
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
    ))
}

/// `cmdHandoff(root)`: write the `design/handoff/<version>.md` skeleton of the approved design
/// and return the line to print.
pub fn write_handoff(root: &Path) -> Result<String, DfwError> {
    let p = project_paths(root);
    let state = require_state(root)?;
    let Some(appr) = current_approval(&state)? else {
        return Err(DfwError::new(
            "Handoff is generated from an approved design (G2).",
        ));
    };
    let version = js::to_string(appr.get("version"));
    // The state is the user's file: its version names folders and files only in the form
    // freeze writes (`v<number>`), never `../x`.
    check_snapshot_name(&version).map_err(|_| {
        DfwError::new(format!(
            "{}: the last design.approved entry has version \"{version}\", not a snapshot name (v1, v2, ...). Restore the file from version control, or set that version to the folder under {}/, and run the command again.",
            rel::STATE,
            rel::APPROVED
        ))
    })?;
    let dir = p.approved.join(&version);
    let m = read_json(&dir.join(file::MANIFEST), Some(json!({})))?;
    let stack = read_json(&p.stack, Some(Value::Null))?;
    let tokens_css = dir.join(file::TOKENS).join(file::TOKENS_CSS);
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
    let comps: Vec<String> = list_files(&dir.join(file::COMPONENTS), &[])?
        .into_iter()
        .filter(|f| f.ends_with(".html"))
        .collect();
    let resolved: Vec<Value> = all_comments(root)?
        .into_iter()
        .filter(|c| {
            status::of(c) == Some(Status::Resolved) && c.get("designVersion") == appr.get("version")
        })
        .collect();
    let file = p.handoff.join(format!("{version}.md"));
    let rel = relative(root, &file);
    if file.exists() {
        return Err(DfwError::new(format!("{rel} already exists.")));
    }
    let tpl = crate::assets::template_text("handoff.md")?;

    let screens = screen_rows(&m, &version)?;
    let stack_text = stack_line(&stack, &state)?;
    let config;
    let breakpoints = match js::truthy_value(m.get(key::BREAKPOINTS)) {
        Some(own) => Some(own),
        None => {
            config = load_config(Some(root))?;
            get_path(&config, "studio.breakpoints")
        }
    };
    let breakpoints: Vec<String> = items(breakpoints, "m.breakpoints")?
        .iter()
        .map(|b| {
            format!(
                "{} {}px",
                js::to_string(b.get("id")),
                js::to_string(b.get("width"))
            )
        })
        .collect();
    let components = comps
        .iter()
        .map(|c| format!("- `{}/{}/{c}`", rel::approved(&version), file::COMPONENTS))
        .collect();
    let tokens_text = if tokens > 0 {
        format!(
            "{tokens} CSS custom properties in `{}/{}` (source: `{}`, W3C DTCG).",
            rel::approved(&version),
            in_design::TOKENS_CSS,
            in_design::TOKENS_JSON
        )
    } else {
        "No tokens found.".to_string()
    };
    let resolved_lines = resolved
        .iter()
        .map(|c| {
            format!(
                "- {}: {} -> {}",
                js::to_string(c.get("id")),
                js::to_string(c.get("comment")),
                text_or(c.get("resolution").and_then(|r| r.get("note")), "")
            )
        })
        .collect();
    let product =
        js::truthy_value(m.get("product")).or_else(|| js::truthy_value(state.get("product")));
    let mut body = tpl;
    for (mark, value) in [
        ("{{version}}", version.clone()),
        ("{{hash}}", js::to_string(appr.get("hash"))),
        ("{{product}}", text_or(product, "")),
        ("{{approvedAt}}", js::to_string(appr.get("at"))),
        ("{{stack}}", stack_text),
        ("{{screens}}", or_text(screens, "| | | | |")),
        ("{{components}}", or_text(components, "- (none extracted)")),
        ("{{tokens}}", tokens_text),
        ("{{breakpoints}}", breakpoints.join(", ")),
        ("{{resolved}}", or_text(resolved_lines, "- none")),
    ] {
        body = js_replace_all(&body, mark, &value);
    }
    write_atomic_inside(root, &file, body)?;
    Ok(format!("Wrote {rel}. Fill in the sections marked TODO with the user's requirements before implementing."))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_patterns_follow_javascript() {
        /// The placeholder replaced, and a text with one in the middle.
        const MARK: &str = "{{x}}";
        const TEXT: &str = "a {{x}} b";
        assert_eq!(js_replace_all("a {{x}} b {{x}}", MARK, "1"), "a 1 b 1");
        assert_eq!(js_replace_all(TEXT, MARK, "$$5 $& $1"), "a $5 {{x}} $1 b");
        assert_eq!(js_replace_all(TEXT, MARK, "[$`|$']"), "a [a | b] b");
        let price = "cost: 5$";
        assert_eq!(js_replace_all(MARK, MARK, price), price);
    }

    /// A version in the state that is not `v<number>` never names a folder or the handoff file
    /// (review RV-1: `../../x` wrote the handoff outside `design/handoff`).
    #[test]
    fn an_approved_version_that_is_not_a_snapshot_name_is_refused() {
        let root = crate::core::util::test_dir("handoff-version");
        crate::core::util::write_json(
            &root.join(rel::STATE),
            &json!({ "design": { "status": "approved", "approved": [{ "version": "../../x", "hash": "h" }] } }),
        )
        .unwrap();
        let err = write_handoff(&root).unwrap_err();
        assert!(
            err.message().contains("not a snapshot name"),
            "{}",
            err.message()
        );
        let escaped = "x.md";
        assert!(!root.join(escaped).exists() && !root.parent().unwrap().join(escaped).exists());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
