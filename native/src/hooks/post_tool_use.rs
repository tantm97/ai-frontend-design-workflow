//! Ports `postToolUse` of `core/lib/hooks.mjs`: after an edit, checks on the files it touched
//! and, for hosts without prompt context, new Studio comments.
//!
//! The checks: generated files are rebuilt when a token file or a component master changed (so
//! the Studio live-reloads without an extra command), then dfw lint runs on the edited files
//! (milliseconds). Every tool runs only with `checks.onEdit: "full"`; full checks otherwise run
//! at gates and on "dfw check".

use std::path::PathBuf;

use serde_json::Value;

use crate::core::config::load_config;
use crate::core::config::{checks_max_findings, checks_on_edit, checks_timeout_ms, OnEdit};
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult, Host};
use crate::core::paths::resolve;
use crate::design::compose::build_system;
use crate::design::tokens::build_tokens;
use crate::tools::check::{extname, format_report, run_checks, CheckOptions, UI_EXT};
use crate::workflow::gates::relative;
use crate::workflow::state::load_state;

use super::pre_tool_use::{root_from, tool_files};
use super::user_prompt_submit::{delivers_on_prompt, inbox_for};

/// `/^design\/<dir>.*\.json$/.test(rel)`.
fn design_json(rel: &str, dir: &str) -> bool {
    let prefix = format!("design/{dir}");
    rel.starts_with(&prefix)
        && rel.ends_with(".json")
        && rel.len() >= prefix.len() + ".json".len()
        && !rel.contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

/// Whether the checks look at a project-relative file: a UI file or design JSON, inside the
/// project (only ".." and "../x" are outside: "..x.html" is a file in it), not in a snapshot.
fn is_checked(rel: &str) -> bool {
    rel != ".."
        && !rel.starts_with("../")
        && !rel.starts_with("design/approved/")
        && (UI_EXT.contains(&extname(rel)) || design_json(rel, ""))
}

/// What an edit asks the checks to look at.
struct Edited {
    root: PathBuf,
    config: Value,
    /// Project-relative files of the edit that the checks run on.
    targets: Vec<String>,
}

/// The first part of `postToolUseChecks`: nothing when the project has no active workflow, the
/// checks are off, or no edited file is one they look at.
fn edited(input: &HookInput) -> Result<Option<Edited>, DfwError> {
    let files = tool_files(input)?;
    if files.is_empty() {
        return Ok(None);
    }
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    if state.get("active") == Some(&Value::Bool(false)) {
        return Ok(None);
    }
    let config = load_config(Some(&root))?;
    if checks_on_edit(&config) == OnEdit::Off {
        return Ok(None);
    }
    let root = resolve(&root);
    let targets: Vec<String> = files
        .iter()
        .map(|f| relative(&root, f))
        .filter(|rel| is_checked(rel))
        .collect();
    Ok((!targets.is_empty()).then_some(Edited {
        root,
        config,
        targets,
    }))
}

/// `postToolUseChecks(input)`: the report on the edited files, when it has MUST or SHOULD items.
fn post_tool_use_checks(input: &HookInput) -> Result<Option<String>, DfwError> {
    let Some(Edited {
        root,
        config,
        targets,
    }) = edited(input)?
    else {
        return Ok(None);
    };
    // Keep generated files in sync. A failure here is reported by lint.
    let rebuild = || -> Result<(), DfwError> {
        if targets.iter().any(|t| design_json(t, "tokens/")) {
            build_tokens(&root)?;
        }
        if targets
            .iter()
            .any(|t| t.starts_with("design/system/components/"))
        {
            build_system(&root)?;
        }
        Ok(())
    };
    let _ = rebuild();
    // Fast by default: dfw lint only.
    let full = checks_on_edit(&config) == OnEdit::Full;
    let opts = CheckOptions {
        timeout_ms: Some(checks_timeout_ms(&config)),
        tools: if full { None } else { Some(&["dfw"]) },
    };
    let results = run_checks(&root, Some(&targets), &config, opts)?;
    let report = format_report(&results, checks_max_findings(&config));
    if report.errors == 0 && report.warnings == 0 {
        return Ok(None);
    }
    Ok(Some(format!(
        "dfw checks on {}:\n{}\nFix MUST items before moving on; SHOULD items unless the brief or a recorded decision says otherwise.",
        targets.join(", "),
        report.text
    )))
}

/// `postToolUse(input, host)`: the checks of the files just written and, on hosts that cannot
/// add context to the prompt, the new Studio comments; `None` when there is nothing to say.
pub fn post_tool_use(input: &HookInput, host: &Host) -> Result<Option<HookResult>, DfwError> {
    // Hosts that cannot add context to the user's prompt get new Studio comments here instead.
    // Checks first: reading the inbox marks the comments delivered, so nothing may fail after it.
    let checks = post_tool_use_checks(input)?;
    let inbox = if delivers_on_prompt(host) {
        None
    } else {
        inbox_for(input)?
    };
    let text: Vec<String> = [checks, inbox].into_iter().flatten().collect();
    Ok((!text.is_empty()).then(|| HookResult::Context(text.join("\n\n"))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn design_json_is_the_node_expression() {
        assert!(design_json("design/design.json", ""));
        assert!(design_json("design/.json", ""));
        assert!(!design_json("design/json", ""));
        assert!(!design_json("src/design/a.json", ""));
        assert!(design_json("design/tokens/themes/dark.json", "tokens/"));
        assert!(design_json("design/tokens/.json", "tokens/"));
        assert!(!design_json("design/tokens.json", "tokens/"));
        assert!(!design_json("design/tokens/a\nb.json", "tokens/"));
    }

    /// d: a file whose name starts with two dots is in the project (tests/unit/node-stable)
    #[test]
    fn only_dot_dot_and_dot_dot_slash_are_outside() {
        assert!(is_checked("..x.html"));
        assert!(is_checked("..x/y.css"));
        assert!(is_checked("src/a.tsx"));
        assert!(!is_checked("../x.html"));
        assert!(!is_checked(".."));
        assert!(!is_checked("design/approved/v1/a.html"));
        assert!(!is_checked("README.md"));
    }
}
