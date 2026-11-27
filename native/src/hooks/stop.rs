//! Ports `stop` of `core/lib/hooks.mjs`: before the agent stops, (1) deliver comments the user
//! posted meanwhile; (2) re-render every screen whose file, masters or tokens changed and block
//! once on layout MUST findings, so edits never leave the design broken; (3) while the Studio is
//! open in a design phase and nothing listens for its comments, block once so the agent starts
//! `dfw comments wait` in the background. Fails open.

use std::path::Path;

use serde_json::Value;

use crate::browser::launch::browser_status;
use crate::browser::layout_check::{layout_check, Finding, LayoutOptions, LayoutResult};
use crate::core::config::{checks_layout_on_stop, load_config};
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult, Host};
use crate::core::level::Level;
use crate::design::layout_cache::{changed_screens, mark_checked};
use crate::review::inbox::{in_background, is_listening, WAIT_COMMAND};
use crate::workflow::server::running_studio;
use crate::workflow::state::{field, load_state};

use super::pre_tool_use::root_from;
use super::user_prompt_submit::inbox_for;

/// `stop(input)`.
pub fn stop(input: &HookInput, host: &Host) -> Result<Option<HookResult>, DfwError> {
    if input.stop_hook_active {
        return Ok(None); // never loop
    }
    if let Some(text) = inbox_for(input, host)? {
        return Ok(Some(HookResult::Block(text)));
    }
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    let designing = matches!(
        state.get(field::PHASE).and_then(Value::as_str),
        Some("system" | "concepts")
    );
    if state.get(field::ACTIVE) == Some(&Value::Bool(false)) || !designing {
        return Ok(None);
    }
    let text: Vec<String> = [layout_findings(&root)?, unheard(&root, host)?]
        .into_iter()
        .flatten()
        .collect();
    Ok((!text.is_empty()).then(|| HookResult::Block(text.join("\n\n"))))
}

/// The instruction to start listening when the Studio of the project runs and no
/// `dfw comments wait` listens: the user's "Send to AI" would reach nobody.
fn unheard(root: &Path, host: &Host) -> Result<Option<String>, DfwError> {
    let Some(url) = running_studio(root)? else {
        return Ok(None);
    };
    if is_listening(root) {
        return Ok(None);
    }
    Ok(Some(format!(
        "The Studio is open ({url}) and nothing listens for its comments, so the user's \"Send to AI\" would not reach you. Start \"{WAIT_COMMAND}\" {} now, then end your turn (references/phase-review.md > \"Live comments\").",
        in_background(host.background())
    )))
}

/// The layout check of the screens changed since the last one, as the text of a block; `None`
/// when it is off, cannot run, or finds nothing.
fn layout_findings(root: &Path) -> Result<Option<String>, DfwError> {
    if !checks_layout_on_stop(&load_config(Some(root))?, Some(root))?
        || !browser_status().installed()
    {
        return Ok(None);
    }
    let ch = changed_screens(root)?;
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
    let LayoutResult::Ran { findings, .. } = layout_check(root, &opts)? else {
        return Ok(None);
    };
    let must = findings.iter().filter(|f| f.level == Level::Error.as_str());
    let should = findings.iter().filter(|f| {
        f.level == Level::Warn.as_str() && ["void", "consistency", "clipped"].contains(&&*f.rule)
    });
    let report: Vec<&Finding> = must.chain(should).collect();
    mark_checked(root)?; // report each state of the design once
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
    Ok(Some(format!(
        "dfw layout check after your edits ({} screen(s){masters}):\n{}\nFix these in the design system (masters, page layout) where possible, re-run \"dfw layout check --changed\", then finish. See references/layout-patterns.md.",
        ch.screens.len(),
        lines.join("\n")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::key;
    use crate::core::host::{event, get_host, input as payload, normalize_hook_input};
    use crate::core::paths::project_paths;
    use crate::core::util::test_dir;
    use crate::workflow::server::field as record;
    use crate::workflow::state::{new_state, save_state};
    use serde_json::json;

    /// A project in `phase` whose layout check on stop is off (no browser in a unit test),
    /// with a Studio record of this process (alive) at `STUDIO`.
    fn project(label: &str, phase: &str) -> std::path::PathBuf {
        let root = test_dir(label);
        let mut state = new_state("new-product", "".into()).unwrap();
        state[field::PHASE] = json!(phase);
        save_state(&root, &mut state).unwrap();
        let p = project_paths(&root);
        let off = json!({ (key::CHECKS): { (key::LAYOUT_ON_STOP): false } });
        std::fs::write(&p.config, off.to_string()).unwrap();
        let studio = json!({ (record::PID): std::process::id(), (record::URL): STUDIO });
        std::fs::write(&p.studio, studio.to_string()).unwrap();
        root
    }

    const CONCEPTS: &str = "concepts";
    const STUDIO: &str = "http://127.0.0.1:4601/";

    fn stop_in(root: &Path, active: bool) -> Option<HookResult> {
        let input = normalize_hook_input(&json!({
            (payload::HOOK_EVENT_NAME): event::STOP,
            (payload::CWD): root.to_string_lossy(),
            (payload::STOP_HOOK_ACTIVE): active,
        }))
        .unwrap();
        stop(&input, get_host(None).unwrap()).unwrap()
    }

    /// The Studio is open and nobody listens: block once with the wait command, never loop.
    #[test]
    fn stop_blocks_once_when_the_studio_runs_and_nobody_listens() {
        let root = project("stop-unheard", "system");
        let Some(HookResult::Block(text)) = stop_in(&root, false) else {
            panic!("expected a block");
        };
        assert!(
            text.contains(WAIT_COMMAND) && text.contains(STUDIO),
            "{text}"
        );
        assert!(
            text.contains("run_in_background"),
            "the host's words: {text}"
        );
        assert!(
            stop_in(&root, true).is_none(),
            "stop_hook_active: never loops"
        );
    }

    /// A wait listens, no Studio, another phase: the agent stops.
    #[test]
    fn stop_does_not_block_with_a_listener_without_studio_or_outside_design() {
        let root = project("stop-listening", CONCEPTS);
        // A marker of a live process (this one), in the form an older dfw wrote.
        let marker = json!({ (crate::review::status::listening::PID): std::process::id() });
        let reviews = project_paths(&root).reviews;
        std::fs::create_dir_all(&reviews).unwrap();
        std::fs::write(
            reviews.join(crate::core::paths::file::REVIEWS_LISTENING),
            marker.to_string(),
        )
        .unwrap();
        assert!(stop_in(&root, false).is_none(), "a wait listens");
        let root = project("stop-no-studio", CONCEPTS);
        std::fs::remove_file(project_paths(&root).studio).unwrap();
        assert!(stop_in(&root, false).is_none(), "no Studio");
        let root = project("stop-other-phase", "stack");
        assert!(stop_in(&root, false).is_none(), "not a design phase");
    }
}
