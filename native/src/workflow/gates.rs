//! Ports `core/lib/gate.mjs`: the write gate decision (`decideWrite`) and the paths a tool call
//! writes (`filesFromPatch`, `filesFromToolInput`).

use std::ffi::OsStr;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::core::config::{gate_allow, gate_design_paths, gate_immutable};
use crate::core::error::DfwError;
use crate::core::host::{patch, tool};
use crate::core::js;
use crate::core::paths::{self, resolve};
use crate::core::util::matches_any;

use super::state::{current_approval, design_lock, field, gate, gate_passed};

/// The text of the gate's refusals and errors, named once (the golden tests record it).
pub mod message {
    use crate::core::paths::rel;

    /// A write to the gate's own state in `.dfw/`.
    pub fn gate_state(file: &str) -> String {
        format!("dfw gate: \"{file}\" holds dfw's gates, approvals, decisions, settings or review comments; only dfw commands change it. Run the dfw command for the change (dfw approve, dfw reopen, dfw decide, dfw config set, dfw stack set, dfw comments ...); approvals, re-opening and gate settings need the user's word first.")
    }

    /// A write to a frozen snapshot (or another immutable path).
    pub fn frozen(file: &str) -> String {
        format!("dfw gate: \"{file}\" is part of a frozen, approved design snapshot and is immutable. To change the approved design, ask the user, then run \"dfw reopen --reason <why>\" and edit the working copy in {}/.", rel::DESIGN)
    }

    /// Any write outside `.dfw/` in a review-only project.
    pub fn review_only() -> String {
        format!("dfw gate: this project is in review-only mode; no files may be modified except under {}/. Report findings instead.", rel::DFW)
    }

    /// A write to the design while design `v{version}` is locked.
    pub fn locked(version: &str) -> String {
        format!("dfw gate: design v{version} is approved and locked. Visual changes to an approved design need user approval: ask the user, then run \"dfw reopen --reason <why>\".")
    }

    /// A write to implementation code before the gates; `missing` lists them, comma separated.
    pub fn not_yet(file: &str, missing: &str) -> String {
        format!("dfw gate: implementation files cannot be modified yet (\"{file}\"). Missing: {missing}. Continue the design workflow (/dfw status) or, if the user explicitly wants this edit outside the workflow, ask them to add the path to gate.allow (dfw config set gate.allow ...).")
    }

    /// The parts of [`not_yet`]'s list.
    pub const MISSING_G2: &str = "G2 (approved, frozen design)";
    pub const MISSING_G3: &str = "G3 (approved technology stack)";
    pub fn missing_phase(current: &str) -> String {
        format!("phase \"implement\" (current: \"{current}\")")
    }

    /// A write tool call without its input.
    pub fn no_tool_input(tool: &str) -> String {
        format!("the host sent no tool input for {tool}; the write gate could not check this call. Update the host CLI or report it to the dfw maintainers.")
    }

    /// A write tool call whose path field holds something else.
    pub fn not_a_path(tool: &str, key: &str, value: &str) -> String {
        format!("the {tool} call has a \"{key}\" that is not a path ({value}); the write gate could not check it. Update the host CLI or report it to the dfw maintainers.")
    }
}

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
    if state.get(field::ACTIVE) == Some(&Value::Bool(false)) {
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
        if paths::rel::under(paths::rel::DFW, &rel.to_lowercase()) {
            return Ok(Decision::deny(message::gate_state(rel)));
        }
        return Ok(Decision::deny(message::frozen(rel)));
    }
    if matches_any(rel, &gate_allow(config)) {
        return Ok(Decision::allow());
    }
    if state.get(field::MODE).and_then(Value::as_str) == Some("review-only") {
        return Ok(Decision::deny(message::review_only()));
    }
    let design_paths = gate_design_paths(config);
    // A locked design is refused under any case of its name; an open one is allowed as written.
    if let Some(version) = design_lock(state).filter(|_| matches(rel, &design_paths, fold)) {
        return Ok(Decision::deny(message::locked(&version)));
    }
    if matches_any(rel, &design_paths) {
        return Ok(Decision::allow());
    }

    // Everything else is implementation code.
    let mut missing: Vec<String> = Vec::new();
    if current_approval(state)?.is_none() {
        missing.push(message::MISSING_G2.into());
    }
    if !gate_passed(state, gate::G3) {
        missing.push(message::MISSING_G3.into());
    }
    let phase = state.get(field::PHASE);
    if !matches!(
        phase.and_then(Value::as_str),
        Some("implement" | "verify" | "done")
    ) {
        missing.push(message::missing_phase(&js::to_string(phase)));
    }
    if !missing.is_empty() {
        return Ok(Decision::deny(message::not_yet(rel, &missing.join(", "))));
    }
    Ok(Decision::allow())
}

/// `filesFromPatch(text)`: paths touched by an apply_patch body ("*** Update File: a/b.ts",
/// "*** Move to: ...").
pub fn files_from_patch(text: &str) -> Vec<String> {
    // The line ends of a JavaScript expression with the `m` flag.
    text.split(['\n', '\r', '\u{2028}', '\u{2029}'])
        .filter_map(|line| patch::HEADERS.iter().find_map(|h| line.strip_prefix(h)))
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
    let Some(tool) = tool_name.filter(|t| tool::WRITES.contains(t)) else {
        return Ok(Vec::new());
    };
    if input.is_null() {
        return Err(DfwError::new(message::no_tool_input(tool)));
    }
    let first = |keys: &[&str]| keys.iter().find_map(|k| js::truthy_value(input.get(k)));
    if tool == tool::APPLY_PATCH {
        let body = first(&["input", "patch", "content"]);
        return Ok(body.map_or(Vec::new(), |b| files_from_patch(&js::to_string(Some(b)))));
    }
    const KEYS: [&str; 4] = ["file_path", "notebook_path", "path", "filePath"];
    match KEYS
        .iter()
        .find_map(|k| js::truthy_value(input.get(k)).map(|v| (k, v)))
    {
        None => Ok(Vec::new()),
        Some((_, Value::String(f))) => Ok(vec![f.clone()]),
        Some((key, other)) => Err(DfwError::new(message::not_a_path(
            tool,
            key,
            &js::to_string(Some(other)),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::config::defaults;
    use crate::core::fixture::{
        ADDED_APP_FILE, APP_COMPONENT, APP_FILE, APP_MODULE, ELSEWHERE_FILE, HANDOFF_NOTE,
        HTML_FILE, NOTES_FILE, OTHER_HTML_FILE, SIBLING_FILE,
    };
    use crate::core::host::normalize_hook_input;
    use crate::core::paths::rel;
    use crate::workflow::state::new_state;
    use serde_json::json;

    /// The project root of the tests that need no folder on disk.
    const PROJ: &str = "/proj";
    const NEW_PRODUCT: &str = "new-product";
    const OUTSIDE: &str = "outside the project";

    /// write gate decisions (tests/unit/dfw.test.mjs)
    #[test]
    fn write_gate_decisions() {
        const SCREEN: &str = "a/a.html";
        const PHASE: &str = "phase";
        let root = PathBuf::from(PROJ);
        let config = defaults();
        let mut s = new_state(NEW_PRODUCT, "".into()).unwrap();
        let w = |s: &Value, f: &str| {
            decide_write(&root, Some(s), &config, &root.join(f))
                .unwrap()
                .allow
        };
        assert!(w(&s, &rel::screen(SCREEN)));
        assert!(w(&s, "PRODUCT.md"));
        assert!(w(&s, &rel::join(rel::DFW, NOTES_FILE)));
        assert!(!w(&s, APP_COMPONENT));
        assert!(
            decide_write(
                &root,
                Some(&s),
                &config,
                &Path::new("/").join(ELSEWHERE_FILE)
            )
            .unwrap()
            .allow,
            "{OUTSIDE}"
        );
        assert!(
            !w(&s, &rel::approved("v1/screens/a.html")),
            "snapshots immutable"
        );
        // After freeze: design locked, handoff allowed, code still blocked until G3 + implement phase.
        s["design"] = json!({ "version": 1, "status": "approved", "locked": true, "approved": [{ "version": "v1" }] });
        assert!(!w(&s, &rel::screen(SCREEN)));
        assert!(!w(&s, "DESIGN.md"));
        assert!(w(&s, &rel::join(rel::HANDOFF, HANDOFF_NOTE)));
        assert!(!w(&s, APP_COMPONENT));
        s["gates"]["G3"] = json!({ "approvedAt": "x" });
        s[PHASE] = "implement".into();
        assert!(w(&s, APP_COMPONENT));
        // Paused workflow lets everything through.
        s["active"] = false.into();
        s[PHASE] = "discovery".into();
        assert!(w(&s, APP_COMPONENT));
        // Review-only blocks everything except .dfw.
        let r = new_state("review-only", "".into()).unwrap();
        assert!(!w(&r, &rel::design(OTHER_HTML_FILE)));
        assert!(w(&r, &rel::join(rel::DFW, NOTES_FILE)));
    }

    #[test]
    fn refusals_say_what_is_missing() {
        let root = PathBuf::from(PROJ);
        let config = defaults();
        let mut s = new_state(NEW_PRODUCT, "".into()).unwrap();
        let reason = |s: &Value, f: &str| {
            decide_write(&root, Some(s), &config, &root.join(f))
                .unwrap()
                .reason
                .unwrap()
        };
        assert_eq!(
            reason(&s, APP_FILE),
            message::not_yet(APP_FILE, &missing_in_draft())
        );
        let frozen = rel::approved("v1/a.html");
        assert_eq!(reason(&s, &frozen), message::frozen(&frozen));
        s["design"]["locked"] = true.into();
        assert_eq!(reason(&s, &rel::design(HTML_FILE)), message::locked("1"));
        assert!(
            decide_write(&root, None, &config, &root.join(APP_FILE))
                .unwrap()
                .allow
        );
        // A relative path is taken from the project root; `..` is resolved before the check.
        assert!(
            !decide_write(
                &root,
                Some(&s),
                &config,
                &Path::new(rel::DESIGN)
                    .join(Component::ParentDir)
                    .join(APP_FILE)
            )
            .unwrap()
            .allow
        );
        assert!(
            decide_write(&root, Some(&s), &config, Path::new(SIBLING_FILE))
                .unwrap()
                .allow
        );
    }

    /// d: the write gate applies to a file whose name starts with two dots
    /// (tests/unit/node-stable.test.mjs)
    #[test]
    fn the_write_gate_applies_to_a_file_whose_name_starts_with_two_dots() {
        let root = crate::core::util::test_dir("gate-dots");
        let s = new_state(NEW_PRODUCT, Value::Null).unwrap();
        let config = defaults();
        let allow = |f: &Path| decide_write(&root, Some(&s), &config, f).unwrap().allow;
        assert!(!allow(Path::new("..x.ts")));
        assert!(!allow(&root.join("..x").join("y.ts")));
        assert!(!allow(Path::new("...")));
        assert!(allow(Path::new("../x.ts")), "{OUTSIDE}");
        assert!(allow(Path::new("..")), "{OUTSIDE}");
    }

    /// What a new project lacks before implementation code: the list of [`message::not_yet`].
    fn missing_in_draft() -> String {
        [
            message::MISSING_G2,
            message::MISSING_G3,
            &message::missing_phase("discovery"),
        ]
        .join(", ")
    }

    /// A frozen state: v1 approved and locked, G3 approved, phase implement.
    fn implementing() -> Value {
        let mut s = new_state(NEW_PRODUCT, "".into()).unwrap();
        s["design"] = json!({ "version": 1, "status": "approved", "locked": true, "approved": [{ "version": "v1" }] });
        s["gates"]["G3"] = json!({ "approvedAt": "x" });
        s["phase"] = "implement".into();
        s
    }

    /// CO-4: a case variant of a frozen or locked path is refused where the volume ignores case.
    #[test]
    fn case_variants_of_frozen_and_locked_paths() {
        /// `design/approved/v1/a.html` with another case.
        const FROZEN_VARIANT: &str = "Approved/v1/a.html";
        /// `design/screens/a.html` with another case.
        const LOCKED_VARIANT: &str = "Design/screens/a.html";
        let config = defaults();
        let draft = new_state(NEW_PRODUCT, "".into()).unwrap();
        let allow = |s: &Value, rel: &str, fold: bool| {
            decide_rel(s, &config, rel, true, fold).unwrap().allow
        };
        assert!(!allow(&draft, &rel::design(FROZEN_VARIANT), true));
        assert!(!allow(&draft, "DESIGN/APPROVED/v1/a.html", true));
        assert!(
            allow(&draft, &rel::design(FROZEN_VARIANT), false),
            "case-sensitive: another folder"
        );
        let s = implementing();
        assert!(!allow(&s, LOCKED_VARIANT, true), "locked under any case");
        assert!(!allow(&s, "design.MD", true));
        assert!(allow(&s, LOCKED_VARIANT, false), "case-sensitive: app code");
        assert!(allow(&s, &rel::join(rel::HANDOFF, HANDOFF_NOTE), true));
        // On the file system of this machine: a reopened project, the frozen file by another case.
        let root = crate::core::util::test_dir("gate-case");
        fs::create_dir_all(root.join(rel::approved("v1"))).unwrap();
        fs::write(root.join(rel::approved("v1/a.html")), "x").unwrap();
        let mut reopened = draft.clone();
        reopened["design"]["version"] = 2.into();
        let written = decide_write(
            &root,
            Some(&reopened),
            &config,
            &root.join(rel::design(FROZEN_VARIANT)),
        )
        .unwrap();
        assert_eq!(written.allow, !FOLD_CASE);
    }

    /// CO-4: a symlink is decided by the file it reaches.
    #[cfg(unix)]
    #[test]
    fn symlinks_are_decided_by_their_target() {
        use crate::core::fixture::{APP_DIR, LINKED_APP_FILE};
        use std::os::unix::fs::symlink;
        const DANGLING: &str = "dangling.html";
        /// A link in `design/` to the snapshots, and a frozen file through it.
        const SNAP: &str = "snap";
        const THROUGH_SNAP: &str = "snap/v1/a.html";
        /// A link in `design/` to the app's folder, and an app file through it.
        const CODE_LINK: &str = "code";
        const THROUGH_CODE: &str = "code/a.ts";
        let config = defaults();
        let root = crate::core::util::test_dir("gate-link");
        fs::create_dir_all(root.join(rel::approved("v1"))).unwrap();
        fs::create_dir_all(root.join(APP_DIR)).unwrap();
        symlink(root.join(rel::APPROVED), root.join(rel::design(SNAP))).unwrap();
        let target = format!("../{LINKED_APP_FILE}");
        symlink(target, root.join(rel::design(DANGLING))).unwrap();
        symlink(root.join(APP_DIR), root.join(rel::design(CODE_LINK))).unwrap();
        let s = new_state(NEW_PRODUCT, "".into()).unwrap();
        let decide = |f: &str| decide_write(&root, Some(&s), &config, &root.join(f)).unwrap();
        let snap = decide(&rel::design(THROUGH_SNAP));
        assert!(!snap.allow);
        assert_eq!(
            snap.reason.unwrap(),
            message::frozen(&rel::approved("v1/a.html"))
        );
        assert!(
            !decide(&rel::design(DANGLING)).allow,
            "a write through a dangling link creates {LINKED_APP_FILE}"
        );
        assert!(!decide(&rel::design(THROUGH_CODE)).allow);
        assert!(decide(&rel::screen(HTML_FILE)).allow);
    }

    /// CO-4: an alias of the project folder (`/tmp` and `/private/tmp` on macOS, a symlinked
    /// root, `CLAUDE_PROJECT_DIR` set to the alias) is the project.
    #[cfg(unix)]
    #[test]
    fn an_alias_of_the_root_is_the_project() {
        use crate::core::fixture::{APP_DIR, IN_LOOP_FILE, LOOP_LINK, NEW_APP_FILE};
        use std::os::unix::fs::symlink;
        let config = defaults();
        let base = crate::core::util::test_dir("gate-alias");
        let root = base.join("project");
        fs::create_dir_all(root.join(APP_DIR)).unwrap();
        let alias = base.join("alias");
        symlink(&root, &alias).unwrap();
        let s = new_state(NEW_PRODUCT, "".into()).unwrap();
        let reason = |r: &Path, f: PathBuf| decide_write(r, Some(&s), &config, &f).unwrap();
        let missing = missing_in_draft();
        for (r, f, file) in [
            (&root, alias.join(APP_FILE), APP_FILE),
            (&alias, root.join(APP_FILE), APP_FILE),
            (&alias, alias.join(NEW_APP_FILE), NEW_APP_FILE),
        ] {
            let d = reason(r, f.clone());
            assert!(!d.allow, "{} in {}", f.display(), r.display());
            assert_eq!(d.reason.unwrap(), message::not_yet(file, &missing));
        }
        assert!(reason(&alias, base.join(ELSEWHERE_FILE)).allow, "{OUTSIDE}");
        // A path that cannot be resolved (a symlink loop) is never "outside".
        let looped = base.join(LOOP_LINK);
        symlink(LOOP_LINK, &looped).unwrap();
        assert_eq!(real_path(&looped.join(IN_LOOP_FILE)), None);
        assert!(!reason(&root, looped.join(IN_LOOP_FILE)).allow);
    }

    #[test]
    fn relative_matches_node_path_relative() {
        let code = format!("{PROJ}/{APP_FILE}");
        let rel = |a: &str, b: &str| relative(Path::new(a), Path::new(b));
        assert_eq!(rel(PROJ, &code), APP_FILE);
        assert_eq!(rel(PROJ, PROJ), "");
        assert_eq!(rel("/proj/design", &code), format!("../{APP_FILE}"));
        assert_eq!(
            rel(PROJ, &format!("/{ELSEWHERE_FILE}")),
            format!("../{ELSEWHERE_FILE}")
        );
        assert_eq!(rel("/a/b/c", "/a"), "../..");
    }

    /// host: normalizes Claude, Copilot PascalCase and Copilot camelCase payloads
    /// (tests/unit/host.test.mjs: the two assertions on filesFromToolInput and filesFromPatch)
    #[test]
    fn host_normalizes_payloads_files_of_a_patch() {
        let page = rel::design(HTML_FILE);
        let body = patch::body(&[(patch::ADD, ADDED_APP_FILE), (patch::UPDATE, &page)]);
        let input = json!({ "toolName": tool::alias::APPLY_PATCH, "toolArgs": body });
        let patch = normalize_hook_input(&input).unwrap();
        assert_eq!(
            files_from_tool_input(patch.tool_name.as_deref(), &patch.tool_input).unwrap(),
            [ADDED_APP_FILE, &page]
        );
        assert_eq!(
            files_from_patch(&patch::lines(&[
                (patch::UPDATE, APP_FILE),
                (patch::MOVE_TO, APP_MODULE)
            ])),
            [APP_FILE, APP_MODULE]
        );
    }

    #[test]
    fn files_of_other_tool_inputs() {
        const A: &str = "a.ts";
        const B: &str = "b.ts";
        const C: &str = "c.ts";
        const NOTEBOOK: &str = "n.ipynb";
        let files = |tool: &str, input: Value| files_from_tool_input(Some(tool), &input).unwrap();
        assert_eq!(files(tool::WRITE, json!({ "file_path": A })), [A]);
        assert_eq!(
            files(tool::NOTEBOOK_EDIT, json!({ "notebook_path": NOTEBOOK })),
            [NOTEBOOK]
        );
        assert_eq!(
            files(tool::EDIT, json!({ "file_path": "", "path": B })),
            [B]
        );
        assert_eq!(files(tool::MULTI_EDIT, json!({ "filePath": C })), [C]);
        assert!(files("Read", json!({ "file_path": A })).is_empty());
        assert!(files(tool::WRITE, json!({})).is_empty());
        assert!(files_from_tool_input(None, &json!({ "file_path": A }))
            .unwrap()
            .is_empty());
        // E2: the field and what happened, not a JavaScript TypeError.
        assert_eq!(
            files_from_tool_input(Some(tool::WRITE), &json!({ "file_path": 5 }))
                .unwrap_err()
                .message(),
            message::not_a_path(tool::WRITE, "file_path", "5")
        );
        assert_eq!(
            files_from_tool_input(Some(tool::WRITE), &Value::Null)
                .unwrap_err()
                .message(),
            message::no_tool_input(tool::WRITE)
        );
        let body = patch::lines(&[
            (patch::DELETE, " old.css \r"),
            (patch::ADD, ""),
            (patch::ADD.trim_end(), "x"),
        ]);
        assert_eq!(
            files(tool::APPLY_PATCH, json!({ "patch": body })),
            ["old.css"]
        );
    }
}
