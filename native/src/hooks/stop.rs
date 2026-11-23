//! Ports `stop` of `core/lib/hooks.mjs`: before the agent stops, (1) deliver comments the user
//! posted meanwhile; (2) re-render every screen whose file, masters or tokens changed and block
//! once on layout MUST findings, so edits never leave the design broken. Fails open.

use serde_json::Value;

use crate::browser::launch::browser_status;
use crate::browser::layout_check::{layout_check, Finding, LayoutOptions, LayoutResult};
use crate::core::config::{checks_layout_on_stop, load_config};
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult};
use crate::core::level::Level;
use crate::design::layout_cache::{changed_screens, mark_checked};
use crate::workflow::state::load_state;

use super::pre_tool_use::root_from;
use super::user_prompt_submit::inbox_for;

/// `stop(input)`.
pub fn stop(input: &HookInput) -> Result<Option<HookResult>, DfwError> {
    if input.stop_hook_active {
        return Ok(None); // never loop
    }
    if let Some(text) = inbox_for(input)? {
        return Ok(Some(HookResult::Block(text)));
    }
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    let designing = matches!(
        state.get("phase").and_then(Value::as_str),
        Some("system" | "concepts")
    );
    if state.get("active") == Some(&Value::Bool(false)) || !designing {
        return Ok(None);
    }
    if !checks_layout_on_stop(&load_config(Some(&root))?) || !browser_status().installed() {
        return Ok(None);
    }
    let ch = changed_screens(&root)?;
    if ch.is_empty() {
        return Ok(None);
    }
    let opts = LayoutOptions {
        screens: Some(ch.screens.clone()),
        bp: None,
        states: true,
        stress: true,
        system: !ch.changed_comps.is_empty() || ch.tokens_changed,
    };
    let LayoutResult::Ran { findings, .. } = layout_check(&root, &opts)? else {
        return Ok(None);
    };
    let must = findings.iter().filter(|f| f.level == Level::Error.as_str());
    let should = findings.iter().filter(|f| {
        f.level == Level::Warn.as_str() && ["void", "consistency", "clipped"].contains(&&*f.rule)
    });
    let report: Vec<&Finding> = must.chain(should).collect();
    mark_checked(&root)?; // report each state of the design once
    if report.is_empty() {
        return Ok(None);
    }
    let lines: Vec<String> = report
        .iter()
        .take(15)
        .map(|f| {
            let label = if f.level == Level::Error.as_str() {
                "MUST"
            } else {
                "SHOULD"
            };
            format!("- [{label}] ({}) {}", f.rule, f.message)
        })
        .collect();
    let masters = if ch.changed_comps.is_empty() {
        String::new()
    } else {
        format!(", changed masters: {}", ch.changed_comps.join(", "))
    };
    Ok(Some(HookResult::Block(format!(
        "dfw layout check after your edits ({} screen(s){masters}):\n{}\nFix these in the design system (masters, page layout) where possible, re-run \"dfw layout check --changed\", then finish. See references/layout-patterns.md.",
        ch.screens.len(),
        lines.join("\n")
    ))))
}
