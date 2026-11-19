//! Ports `core/lib/gate.mjs`: the write gate decision (`decideWrite`) and the paths a tool call
//! writes (`filesFromPatch`, `filesFromToolInput`).

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::core::config::{gate_allow, gate_design_paths, gate_immutable};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::resolve;
use crate::core::util::matches_any;

use super::state::{current_approval, gate_passed};

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

/// Whether paths are compared without case: macOS (APFS, HFS+) and Windows (NTFS) volumes ignore
/// case by default, so `design/Approved` is `design/approved` there. Decided per OS, without
/// testing the volume: on the rare case-sensitive volume of those systems it only refuses more.
const FOLD_CASE: bool = cfg!(any(target_os = "macos", target_os = "windows"));

/// [`matches_any`], without case when `fold` is set.
fn matches(rel: &str, globs: &[String], fold: bool) -> bool {
    if !fold {
        return matches_any(rel, globs);
    }
    let globs: Vec<String> = globs.iter().map(|g| g.to_lowercase()).collect();
    matches_any(&rel.to_lowercase(), &globs)
}

/// The real path of an absolute, normalized path that may not exist yet: the nearest existing
/// ancestor with its symlinks resolved, then the rest as given. A symlink whose target is
/// missing is followed as well, since a write through it creates the target. `None` when the
/// path cannot be resolved (a loop, an unreadable folder).
pub fn real_path(path: &Path) -> Option<PathBuf> {
    let mut path = path.to_path_buf();
    for _ in 0..40 {
        let mut existing = path.as_path();
        let mut rest: Vec<&OsStr> = Vec::new();
        while fs::symlink_metadata(existing).is_err() {
            rest.push(existing.file_name()?);
            existing = existing.parent()?;
        }
        let append = |base: PathBuf| rest.iter().rev().fold(base, |p, part| p.join(part));
        match crate::core::paths::canonicalize(existing) {
            Ok(real) => return Some(append(real)),
            Err(_) => {
                // A dangling symlink: go on from its target.
                let target = fs::read_link(existing).ok()?;
                let next = append(resolve(existing.parent()?.join(target)));
                path = next;
            }
        }
    }
    None
}

/// The path of `abs_file` relative to the project, by real path: a symlink, a case variant or
/// an alias of the root (`/tmp` and `/private/tmp`) names the file it reaches. `None` when one
/// of the two cannot be resolved.
fn real_relative(root: &Path, abs_file: &Path) -> Option<String> {
    let real_root = real_path(&resolve(root))?;
    let real_file = real_path(&resolve(root.join(abs_file)))?;
    Some(relative(&real_root, &real_file))
}

/// `decideWrite({ root, state, config, absFile })`: decide whether a write to `abs_file` is
/// allowed. `state` is `None` when the project has no workflow.
///
/// The decision is made on the real path. A path that cannot be resolved is decided by its
/// written form, and is never "outside the project".
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
    let (rel, resolved) = match real_relative(root, abs_file) {
        Some(rel) => (rel, true),
        None => (
            relative(&resolve(root), &resolve(root.join(abs_file))),
            false,
        ),
    };
    decide_rel(state, config, &rel, resolved, FOLD_CASE)
}

/// The decision for a path relative to the project. `outside_allowed` is false for a path that
/// could not be resolved; `fold` compares the frozen and locked paths without case.
fn decide_rel(
    state: &Value,
    config: &Value,
    rel: &str,
    outside_allowed: bool,
    fold: bool,
) -> Result<Decision, DfwError> {
    // Outside the project; "..x.ts" is inside.
    if outside_allowed && (rel == ".." || rel.starts_with("../")) {
        return Ok(Decision::allow());
    }
    if matches(rel, &gate_immutable(config), fold) {
        if rel.to_lowercase().starts_with(".dfw/") {
            return Ok(Decision::deny(format!("dfw gate: \"{rel}\" holds dfw's gates, approvals, decisions, settings or review comments; only dfw commands change it. Run the dfw command for the change (dfw approve, dfw reopen, dfw decide, dfw config set, dfw stack set, dfw comments ...); approvals, re-opening and gate settings need the user's word first.")));
        }
        return Ok(Decision::deny(format!("dfw gate: \"{rel}\" is part of a frozen, approved design snapshot and is immutable. To change the approved design, ask the user, then run \"dfw reopen --reason <why>\" and edit the working copy in design/.")));
    }
    if matches_any(rel, &gate_allow(config)) {
        return Ok(Decision::allow());
    }
    if state.get("mode").and_then(Value::as_str) == Some("review-only") {
        return Ok(Decision::deny("dfw gate: this project is in review-only mode; no files may be modified except under .dfw/. Report findings instead.".into()));
    }
    let design_paths = gate_design_paths(config);
    let design = state.get("design");
    // A locked design is refused under any case of its name; an open one is allowed as written.
    if js::truthy_value(design.and_then(|d| d.get("locked"))).is_some()
        && matches(rel, &design_paths, fold)
    {
        return Ok(Decision::deny(format!("dfw gate: design v{} is approved and locked. Visual changes to an approved design need user approval: ask the user, then run \"dfw reopen --reason <why>\".", js::to_string(design.and_then(|d| d.get("version"))))));
    }
    if matches_any(rel, &design_paths) {
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
            js::to_string(phase)
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
        .map(|path| js::trim(path).to_string())
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
        return Err(DfwError::new(format!("the host sent no tool input for {tool}; the write gate could not check this call. Update the host CLI or report it to the dfw maintainers.")));
    }
    let first = |keys: &[&str]| keys.iter().find_map(|k| js::truthy_value(input.get(k)));
    if tool == "ApplyPatch" {
        let body = first(&["input", "patch", "content"]);
        return Ok(body.map_or(Vec::new(), |b| files_from_patch(&js::to_string(Some(b)))));
    }
    const KEYS: [&str; 4] = ["file_path", "notebook_path", "path", "filePath"];
    match KEYS.iter().find_map(|k| js::truthy_value(input.get(k)).map(|v| (k, v))) {
        None => Ok(Vec::new()),
        Some((_, Value::String(f))) => Ok(vec![f.clone()]),
        Some((key, other)) => Err(DfwError::new(format!(
            "the {tool} call has a \"{key}\" that is not a path ({}); the write gate could not check it. Update the host CLI or report it to the dfw maintainers.",
            js::to_string(Some(other))
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::defaults;
    use crate::core::host::normalize_hook_input;
    use crate::workflow::state::new_state;
    use serde_json::json;

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

    /// A frozen state: v1 approved and locked, G3 approved, phase implement.
    fn implementing() -> Value {
        let mut s = new_state("new-product", "".into()).unwrap();
        s["design"] = json!({ "version": 1, "status": "approved", "locked": true, "approved": [{ "version": "v1" }] });
        s["gates"]["G3"] = json!({ "approvedAt": "x" });
        s["phase"] = "implement".into();
        s
    }

    /// CO-4: a case variant of a frozen or locked path is refused where the volume ignores case.
    #[test]
    fn case_variants_of_frozen_and_locked_paths() {
        let config = defaults();
        let draft = new_state("new-product", "".into()).unwrap();
        let allow = |s: &Value, rel: &str, fold: bool| {
            decide_rel(s, &config, rel, true, fold).unwrap().allow
        };
        assert!(!allow(&draft, "design/Approved/v1/a.html", true));
        assert!(!allow(&draft, "DESIGN/APPROVED/v1/a.html", true));
        assert!(
            allow(&draft, "design/Approved/v1/a.html", false),
            "case-sensitive: another folder"
        );
        let s = implementing();
        assert!(
            !allow(&s, "Design/screens/a.html", true),
            "locked under any case"
        );
        assert!(!allow(&s, "design.MD", true));
        assert!(
            allow(&s, "Design/screens/a.html", false),
            "case-sensitive: app code"
        );
        assert!(allow(&s, "design/handoff/v1.md", true));
        // On the file system of this machine: a reopened project, the frozen file by another case.
        let root = crate::core::util::test_dir("gate-case");
        fs::create_dir_all(root.join("design/approved/v1")).unwrap();
        fs::write(root.join("design/approved/v1/a.html"), "x").unwrap();
        let mut reopened = draft.clone();
        reopened["design"]["version"] = 2.into();
        let written = decide_write(
            &root,
            Some(&reopened),
            &config,
            &root.join("design/Approved/v1/a.html"),
        )
        .unwrap();
        assert_eq!(written.allow, !FOLD_CASE);
    }

    /// CO-4: a symlink is decided by the file it reaches.
    #[cfg(unix)]
    #[test]
    fn symlinks_are_decided_by_their_target() {
        use std::os::unix::fs::symlink;
        let config = defaults();
        let root = crate::core::util::test_dir("gate-link");
        fs::create_dir_all(root.join("design/approved/v1")).unwrap();
        fs::create_dir_all(root.join("src")).unwrap();
        symlink(root.join("design/approved"), root.join("design/snap")).unwrap();
        symlink("../src/new.ts", root.join("design/dangling.html")).unwrap();
        symlink(root.join("src"), root.join("design/code")).unwrap();
        let s = new_state("new-product", "".into()).unwrap();
        let decide = |f: &str| decide_write(&root, Some(&s), &config, &root.join(f)).unwrap();
        let snap = decide("design/snap/v1/a.html");
        assert!(!snap.allow);
        assert!(snap
            .reason
            .unwrap()
            .contains("\"design/approved/v1/a.html\" is part of a frozen"));
        assert!(
            !decide("design/dangling.html").allow,
            "a write through a dangling link creates src/new.ts"
        );
        assert!(!decide("design/code/a.ts").allow);
        assert!(decide("design/screens/a.html").allow);
    }

    /// CO-4: an alias of the project folder (`/tmp` and `/private/tmp` on macOS, a symlinked
    /// root, `CLAUDE_PROJECT_DIR` set to the alias) is the project.
    #[cfg(unix)]
    #[test]
    fn an_alias_of_the_root_is_the_project() {
        use std::os::unix::fs::symlink;
        let config = defaults();
        let base = crate::core::util::test_dir("gate-alias");
        let root = base.join("project");
        fs::create_dir_all(root.join("src")).unwrap();
        symlink(&root, base.join("alias")).unwrap();
        let alias = base.join("alias");
        let s = new_state("new-product", "".into()).unwrap();
        let reason = |r: &Path, f: PathBuf| decide_write(r, Some(&s), &config, &f).unwrap();
        for (r, f) in [
            (&root, alias.join("src/a.ts")),
            (&alias, root.join("src/a.ts")),
            (&alias, alias.join("src/new/b.ts")),
        ] {
            let d = reason(r, f.clone());
            assert!(!d.allow, "{} in {}", f.display(), r.display());
            assert!(d.reason.unwrap().contains("cannot be modified yet (\"src/"));
        }
        assert!(
            reason(&alias, base.join("elsewhere/x.ts")).allow,
            "outside the project"
        );
        // A path that cannot be resolved (a symlink loop) is never "outside".
        symlink("loop", base.join("loop")).unwrap();
        assert_eq!(real_path(&base.join("loop/x.ts")), None);
        assert!(!reason(&root, base.join("loop/x.ts")).allow);
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
        // E2: the field and what happened, not a JavaScript TypeError.
        assert_eq!(
            files_from_tool_input(Some("Write"), &json!({ "file_path": 5 })).unwrap_err().message(),
            "the Write call has a \"file_path\" that is not a path (5); the write gate could not check it. Update the host CLI or report it to the dfw maintainers."
        );
        assert!(files_from_tool_input(Some("Write"), &Value::Null)
            .unwrap_err()
            .message()
            .starts_with("the host sent no tool input for Write;"));
        assert_eq!(
            files(
                "ApplyPatch",
                json!({ "patch": "*** Delete File:  old.css \r\n*** Add File: \n*** Add File:x" })
            ),
            ["old.css"]
        );
    }
}
