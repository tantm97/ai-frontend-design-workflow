//! Ports `core/lib/gate.mjs`: the write gate decision (`decideWrite`) and the paths a tool call
//! writes (`filesFromPatch`, `filesFromToolInput`).

use std::path::{Component, Path};

use serde_json::Value;

use crate::core::config::{gate_allow, gate_design_paths, gate_immutable};
use crate::core::error::DfwError;
use crate::core::paths::resolve;
use crate::core::util::matches_any;

use super::state::{current_approval, gate_passed, js_str, js_trim, truthy};

/// Canonical (normalized by `core::host`) names of tools that write files.
const WRITE_TOOLS: [&str; 5] = ["Edit", "Write", "MultiEdit", "NotebookEdit", "ApplyPatch"];

/// Whether a write is allowed, and why not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub allow: bool,
    /// The message for the agent when the write is refused.
    pub reason: Option<String>,
}

impl Decision {
    fn allow() -> Self {
        Decision {
            allow: true,
            reason: None,
        }
    }

    fn deny(reason: String) -> Self {
        Decision {
            allow: false,
            reason: Some(reason),
        }
    }
}

/// `toPosix(path.relative(from, to))` for two absolute, normalized paths.
pub fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<Component> = from.components().collect();
    let to: Vec<Component> = to.components().collect();
    let common = from.iter().zip(&to).take_while(|(a, b)| a == b).count();
    let up = std::iter::repeat_n("..".to_string(), from.len() - common);
    let down = to[common..]
        .iter()
        .map(|c| c.as_os_str().to_string_lossy().into_owned());
    up.chain(down).collect::<Vec<_>>().join("/")
}

/// `decideWrite({ root, state, config, absFile })`: decide whether a write to `abs_file` is
/// allowed. `state` is `None` when the project has no workflow.
pub fn decide_write(
    root: &Path,
    state: Option<&Value>,
    config: &Value,
    abs_file: &Path,
) -> Result<Decision, DfwError> {
    let Some(state) = state else {
        return Ok(Decision::allow());
    };
    if state.get("active") == Some(&Value::Bool(false)) {
        return Ok(Decision::allow());
    }
    let rel = relative(&resolve(root), &resolve(root.join(abs_file)));
    // Outside the project; "..x.ts" is inside.
    if rel == ".." || rel.starts_with("../") {
        return Ok(Decision::allow());
    }
    if matches_any(&rel, &gate_immutable(config)) {
        return Ok(Decision::deny(format!("dfw gate: \"{rel}\" is part of a frozen, approved design snapshot and is immutable. To change the approved design, ask the user, then run \"dfw reopen --reason <why>\" and edit the working copy in design/.")));
    }
    if matches_any(&rel, &gate_allow(config)) {
        return Ok(Decision::allow());
    }
    if state.get("mode").and_then(Value::as_str) == Some("review-only") {
        return Ok(Decision::deny("dfw gate: this project is in review-only mode; no files may be modified except under .dfw/. Report findings instead.".into()));
    }
    if matches_any(&rel, &gate_design_paths(config)) {
        let design = state.get("design");
        if truthy(design.and_then(|d| d.get("locked"))).is_some() {
            return Ok(Decision::deny(format!("dfw gate: design v{} is approved and locked. Visual changes to an approved design need user approval: ask the user, then run \"dfw reopen --reason <why>\".", js_str(design.and_then(|d| d.get("version"))))));
        }
        return Ok(Decision::allow());
    }

    // Everything else is implementation code.
    let mut missing: Vec<String> = Vec::new();
    if current_approval(state)?.is_none() {
        missing.push("G2 (approved, frozen design)".into());
    }
    if !gate_passed(state, "G3") {
        missing.push("G3 (approved technology stack)".into());
    }
    let phase = state.get("phase");
    if !matches!(
        phase.and_then(Value::as_str),
        Some("implement" | "verify" | "done")
    ) {
        missing.push(format!(
            "phase \"implement\" (current: \"{}\")",
            js_str(phase)
        ));
    }
    if !missing.is_empty() {
        return Ok(Decision::deny(format!("dfw gate: implementation files cannot be modified yet (\"{rel}\"). Missing: {}. Continue the design workflow (/dfw status) or, if the user explicitly wants this edit outside the workflow, ask them to add the path to gate.allow (dfw config set gate.allow ...).", missing.join(", "))));
    }
    Ok(Decision::allow())
}

/// `filesFromPatch(text)`: paths touched by an apply_patch body ("*** Update File: a/b.ts",
/// "*** Move to: ...").
pub fn files_from_patch(text: &str) -> Vec<String> {
    const HEADERS: [&str; 4] = [
        "*** Add File: ",
        "*** Update File: ",
        "*** Delete File: ",
        "*** Move to: ",
    ];
    // The line ends of a JavaScript expression with the `m` flag.
    text.split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .filter_map(|line| HEADERS.iter().find_map(|h| line.strip_prefix(h)))
        .filter(|path| !path.is_empty())
        .map(|path| js_trim(path).to_string())
        .collect()
}

/// `filesFromToolInput(toolName, input)`: the files a tool call writes; empty for a tool that
/// does not write. A path that is not text is an error (Node throws when it resolves it).
pub fn files_from_tool_input(
    tool_name: Option<&str>,
    input: &Value,
) -> Result<Vec<String>, DfwError> {
    let Some(tool) = tool_name.filter(|t| WRITE_TOOLS.contains(t)) else {
        return Ok(Vec::new());
    };
    if input.is_null() {
        return Err(DfwError::internal(
            "Cannot read properties of null (reading 'input')",
        ));
    }
    let first = |keys: &[&str]| keys.iter().find_map(|k| truthy(input.get(k)));
    if tool == "ApplyPatch" {
        let body = first(&["input", "patch", "content"]);
        return Ok(body.map_or(Vec::new(), |b| files_from_patch(&js_str(Some(b)))));
    }
    match first(&["file_path", "notebook_path", "path", "filePath"]) {
        None => Ok(Vec::new()),
        Some(Value::String(f)) => Ok(vec![f.clone()]),
        Some(_) => Err(DfwError::internal(
            "The \"paths[1]\" argument must be of type string.",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::defaults;
    use crate::core::host::normalize_hook_input;
    use crate::workflow::state::new_state;
    use serde_json::json;
    use std::path::PathBuf;

    /// write gate decisions (tests/unit/dfw.test.mjs)
    #[test]
    fn write_gate_decisions() {
        let root = PathBuf::from("/proj");
        let config = defaults();
        let mut s = new_state("new-product", "".into()).unwrap();
        let w = |s: &Value, f: &str| {
            decide_write(&root, Some(s), &config, &root.join(f))
                .unwrap()
                .allow
        };
        assert!(w(&s, "design/screens/a/a.html"));
        assert!(w(&s, "PRODUCT.md"));
        assert!(w(&s, ".dfw/notes.md"));
        assert!(!w(&s, "src/App.tsx"));
        assert!(
            decide_write(&root, Some(&s), &config, Path::new("/elsewhere/x.js"))
                .unwrap()
                .allow,
            "outside project"
        );
        assert!(
            !w(&s, "design/approved/v1/screens/a.html"),
            "snapshots immutable"
        );
        // After freeze: design locked, handoff allowed, code still blocked until G3 + implement phase.
        s["design"] = json!({ "version": 1, "status": "approved", "locked": true, "approved": [{ "version": "v1" }] });
        assert!(!w(&s, "design/screens/a/a.html"));
        assert!(!w(&s, "DESIGN.md"));
        assert!(w(&s, "design/handoff/v1.md"));
        assert!(!w(&s, "src/App.tsx"));
        s["gates"]["G3"] = json!({ "approvedAt": "x" });
        s["phase"] = "implement".into();
        assert!(w(&s, "src/App.tsx"));
        // Paused workflow lets everything through.
        s["active"] = false.into();
        s["phase"] = "discovery".into();
        assert!(w(&s, "src/App.tsx"));
        // Review-only blocks everything except .dfw.
        let r = new_state("review-only", "".into()).unwrap();
        assert!(!w(&r, "design/x.html"));
        assert!(w(&r, ".dfw/report.md"));
    }

    #[test]
    fn refusals_say_what_is_missing() {
        let root = PathBuf::from("/proj");
        let config = defaults();
        let mut s = new_state("new-product", "".into()).unwrap();
        let reason = |s: &Value, f: &str| {
            decide_write(&root, Some(s), &config, &root.join(f))
                .unwrap()
                .reason
                .unwrap()
        };
        assert_eq!(reason(&s, "src/a.ts"), "dfw gate: implementation files cannot be modified yet (\"src/a.ts\"). Missing: G2 (approved, frozen design), G3 (approved technology stack), phase \"implement\" (current: \"discovery\"). Continue the design workflow (/dfw status) or, if the user explicitly wants this edit outside the workflow, ask them to add the path to gate.allow (dfw config set gate.allow ...).");
        assert!(reason(&s, "design/approved/v1/a.html").contains("is immutable"));
        s["design"]["locked"] = true.into();
        assert!(
            reason(&s, "design/a.html").starts_with("dfw gate: design v1 is approved and locked.")
        );
        assert!(
            decide_write(&root, None, &config, &root.join("src/a.ts"))
                .unwrap()
                .allow
        );
        // A relative path is taken from the project root; `..` is resolved before the check.
        assert!(
            !decide_write(&root, Some(&s), &config, Path::new("design/../src/a.ts"))
                .unwrap()
                .allow
        );
        assert!(
            decide_write(&root, Some(&s), &config, Path::new("../other/a.ts"))
                .unwrap()
                .allow
        );
    }

    /// d: the write gate applies to a file whose name starts with two dots
    /// (tests/unit/node-stable.test.mjs)
    #[test]
    fn the_write_gate_applies_to_a_file_whose_name_starts_with_two_dots() {
        let root = crate::core::util::test_dir("gate-dots");
        let s = new_state("new-product", Value::Null).unwrap();
        let config = defaults();
        let allow = |f: &Path| decide_write(&root, Some(&s), &config, f).unwrap().allow;
        assert!(!allow(Path::new("..x.ts")));
        assert!(!allow(&root.join("..x").join("y.ts")));
        assert!(!allow(Path::new("...")));
        assert!(allow(Path::new("../x.ts")), "outside the project");
        assert!(allow(Path::new("..")), "outside the project");
    }

    #[test]
    fn relative_matches_node_path_relative() {
        let rel = |a: &str, b: &str| relative(Path::new(a), Path::new(b));
        assert_eq!(rel("/proj", "/proj/src/a.ts"), "src/a.ts");
        assert_eq!(rel("/proj", "/proj"), "");
        assert_eq!(rel("/proj/design", "/proj/src/a.ts"), "../src/a.ts");
        assert_eq!(rel("/proj", "/elsewhere/x.js"), "../elsewhere/x.js");
        assert_eq!(rel("/a/b/c", "/a"), "../..");
    }

    /// host: normalizes Claude, Copilot PascalCase and Copilot camelCase payloads
    /// (tests/unit/host.test.mjs: the two assertions on filesFromToolInput and filesFromPatch)
    #[test]
    fn host_normalizes_payloads_files_of_a_patch() {
        let patch = normalize_hook_input(&json!({ "toolName": "apply_patch", "toolArgs": "*** Begin Patch\n*** Add File: src/x.ts\n*** Update File: design/a.html\n*** End Patch" })).unwrap();
        assert_eq!(
            files_from_tool_input(patch.tool_name.as_deref(), &patch.tool_input).unwrap(),
            ["src/x.ts", "design/a.html"]
        );
        assert_eq!(
            files_from_patch("*** Update File: a\n*** Move to: b"),
            ["a", "b"]
        );
    }

    #[test]
    fn files_of_other_tool_inputs() {
        let files = |tool: &str, input: Value| files_from_tool_input(Some(tool), &input).unwrap();
        assert_eq!(files("Write", json!({ "file_path": "a.ts" })), ["a.ts"]);
        assert_eq!(
            files("NotebookEdit", json!({ "notebook_path": "n.ipynb" })),
            ["n.ipynb"]
        );
        assert_eq!(
            files("Edit", json!({ "file_path": "", "path": "b.ts" })),
            ["b.ts"]
        );
        assert_eq!(files("MultiEdit", json!({ "filePath": "c.ts" })), ["c.ts"]);
        assert!(files("Read", json!({ "file_path": "a.ts" })).is_empty());
        assert!(files("Write", json!({})).is_empty());
        assert!(files_from_tool_input(None, &json!({ "file_path": "a.ts" }))
            .unwrap()
            .is_empty());
        assert!(files_from_tool_input(Some("Write"), &json!({ "file_path": 5 })).is_err());
        assert!(files_from_tool_input(Some("Write"), &Value::Null).is_err());
        assert_eq!(
            files(
                "ApplyPatch",
                json!({ "patch": "*** Delete File:  old.css \r\n*** Add File: \n*** Add File:x" })
            ),
            ["old.css"]
        );
    }
}
