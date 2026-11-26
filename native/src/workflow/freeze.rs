//! Ports `core/lib/freeze.mjs`: design freeze (G2), snapshot verification and re-open for a new version.

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, project_paths, rel, resolve};
use crate::core::util::{
    copy_dir, ensure_inside, list_files, now_iso, read_json, sha256, write_json_inside,
};

use super::gates::relative;
use super::state::field as state_key;
use super::state::phase::{CONCEPTS, DONE, IMPLEMENT, STACK, VERIFY};
use super::state::{approve_gate, gate, push_history, save_state, state_field_error};

/// The keys of a snapshot's lock file (`design/approved/v<N>/approval.lock.json`):
/// `{ version, approvedAt, hash, note?, files: [{ path, sha256 }] }`.
pub mod field {
    /// The snapshot name (`v1`).
    pub const VERSION: &str = "version";
    pub const APPROVED_AT: &str = "approvedAt";
    /// The hash over every file of the snapshot.
    pub const HASH: &str = "hash";
    /// What the user approved.
    pub const NOTE: &str = "note";
    /// One `{ path, sha256 }` per file of the snapshot.
    pub const FILES: &str = "files";
    /// A file of the snapshot, relative to its folder.
    pub const PATH: &str = "path";
    pub const SHA256: &str = "sha256";
}

const EXCLUDE: [&str; 2] = [file::APPROVED, file::HANDOFF];

/// Canonical product/design documents live at the project root because Impeccable and Hallmark
/// read them there; they are frozen with the design.
pub const ROOT_DOCS: [&str; 2] = [rel::PRODUCT_DOC, rel::DESIGN_DOC];

/// One file of a design hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HashedFile {
    pub path: String,
    pub sha256: String,
}

/// Result of [`hash_design`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignHash {
    pub files: Vec<HashedFile>,
    pub hash: String,
}

/// `hashDesign(root)`: a hash per file of `design/` (without snapshots and handoff) and of the
/// root documents, and one hash over all of them.
pub fn hash_design(root: &Path) -> Result<DesignHash, DfwError> {
    let design_dir = project_paths(root).design;
    let mut files = Vec::new();
    for rel in list_files(&design_dir, &EXCLUDE)? {
        let sha256 = sha256(fs::read(design_dir.join(&rel))?);
        files.push(HashedFile { path: rel, sha256 });
    }
    for doc in ROOT_DOCS {
        let abs = root.join(doc);
        if abs.exists() {
            files.push(HashedFile {
                path: format!("docs/{doc}"),
                sha256: sha256(fs::read(abs)?),
            });
        }
    }
    let lines: Vec<String> = files
        .iter()
        .map(|f| format!("{}  {}", f.sha256, f.path))
        .collect();
    Ok(DesignHash {
        hash: sha256(lines.join("\n")),
        files,
    })
}

fn design_of(state: &mut Value) -> Result<&mut Map<String, Value>, DfwError> {
    match state.get_mut(state_key::DESIGN) {
        Some(Value::Object(design)) => Ok(design),
        _ => Err(state_field_error(state_key::DESIGN, "an object")),
    }
}

/// `freezeDesign(root, state, { note })`: copy the design to `design/approved/v<n>/`, write its
/// lock file, lock the design and record G2.
pub fn freeze_design(root: &Path, state: &mut Value, note: Option<Value>) -> Result<(), DfwError> {
    let p = project_paths(root);
    let note = note.filter(|v| js::truthy(v));
    if !p.manifest.exists() {
        return Err(DfwError::new(format!(
            "Cannot freeze: {} does not exist.",
            rel::MANIFEST
        )));
    }
    let design = design_of(state)?;
    let version = format!("v{}", js::to_string(design.get(state_key::VERSION)));
    // The state is the user's file: its version names a folder only in the form freeze writes.
    check_snapshot_name(&version).map_err(|_| {
        DfwError::new(format!(
            "Cannot freeze: design.version in {} is {}, not a whole number. Set it to the number of this design version (1, 2, ...) and run the command again.",
            rel::STATE,
            js::to_string(design.get(state_key::VERSION))
        ))
    })?;
    if design.get(state_key::STATUS).and_then(Value::as_str) == Some("approved") {
        return Err(DfwError::new(format!(
            "Version {version} is already approved. Use \"dfw reopen\" to start a new version."
        )));
    }
    let dest = p.approved.join(&version);
    if dest.exists() {
        return Err(DfwError::new(format!(
            "Snapshot {} already exists; refusing to overwrite.",
            relative(&resolve(root), &resolve(&dest))
        )));
    }
    let DesignHash { files, hash } = hash_design(root)?;
    // The snapshot is new: `design/approved` linked out of the project must not receive it.
    ensure_inside(root, &dest)?;
    copy_dir(&p.design, &dest, &EXCLUDE)?;
    for doc in ROOT_DOCS {
        let abs = root.join(doc);
        if abs.exists() {
            fs::create_dir_all(dest.join("docs"))?;
            fs::copy(abs, dest.join("docs").join(doc))?;
        }
    }
    let at = now_iso();
    let files: Vec<Value> = files
        .iter()
        .map(|f| json!({ (field::PATH): f.path, (field::SHA256): f.sha256 }))
        .collect();
    let lock = js::object(vec![
        (field::VERSION, Some(version.clone().into())),
        (field::APPROVED_AT, Some(at.clone().into())),
        (field::HASH, Some(hash.clone().into())),
        (field::NOTE, note.clone()),
        (field::FILES, Some(Value::Array(files))),
    ]);
    write_json_inside(root, &dest.join(file::APPROVAL_LOCK), &lock)?;
    design.insert(state_key::STATUS.into(), "approved".into());
    design.insert(state_key::LOCKED.into(), true.into());
    let entry = js::object(vec![
        (state_key::VERSION, Some(version.clone().into())),
        (state_key::HASH, Some(hash.clone().into())),
        (state_key::AT, Some(at.into())),
        (state_key::NOTE, note.clone()),
    ]);
    match design.get_mut(state_key::APPROVED) {
        Some(Value::Array(approved)) => approved.push(entry),
        _ => return Err(state_field_error("design.approved", "a list")),
    }
    let mut extra = Map::new();
    extra.insert(state_key::VERSION.into(), version.into());
    extra.insert(state_key::HASH.into(), hash.into());
    approve_gate(root, state, gate::G2, note, extra)
}

/// The version names a folder: only the form `freezeDesign` writes (v1, v2, ...) is joined onto
/// `design/approved`.
pub fn check_snapshot_name(version: &str) -> Result<(), DfwError> {
    let digits = version.strip_prefix('v').unwrap_or_default();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(DfwError::new(format!(
            "Unknown snapshot \"{version}\". A snapshot is named v<number>, for example v1."
        )));
    }
    Ok(())
}

/// `verifySnapshot(root, version)`: verify an approved snapshot is intact (nobody edited it).
/// Returns one line per missing, modified or added file.
pub fn verify_snapshot(root: &Path, version: &str) -> Result<Vec<String>, DfwError> {
    check_snapshot_name(version)?;
    let dir = project_paths(root).approved.join(version);
    let lock = read_json(&dir.join(file::APPROVAL_LOCK), Some(Value::Null))?;
    if !js::truthy(&lock) {
        return Err(DfwError::new(format!(
            "No approval.lock.json in {}",
            dir.display()
        )));
    }
    let listed = lock
        .get(field::FILES)
        .and_then(Value::as_array)
        .ok_or_else(|| DfwError::internal("lock.files is not iterable"))?;
    let path_of = |f: &Value| {
        f.get(field::PATH)
            .and_then(Value::as_str)
            .map(str::to_string)
    };
    let mut problems = Vec::new();
    for f in listed {
        let path = path_of(f).ok_or_else(|| {
            DfwError::internal("The \"path\" argument must be of type string. Received undefined")
        })?;
        let abs = dir.join(&path);
        if !abs.exists() {
            problems.push(format!("missing: {path}"));
        } else if Some(sha256(fs::read(&abs)?).as_str())
            != f.get(field::SHA256).and_then(Value::as_str)
        {
            problems.push(format!("modified: {path}"));
        }
    }
    for rel in list_files(&dir, &[])? {
        let known = listed.iter().any(|f| path_of(f).as_deref() == Some(&rel));
        if rel != file::APPROVAL_LOCK && !known {
            problems.push(format!("added: {rel}"));
        }
    }
    Ok(problems)
}

/// `reopenDesign(root, state, { reason })`: start the next design version as a draft. G2 and G4
/// are cleared; a phase after the design goes back to `concepts`.
pub fn reopen_design(
    root: &Path,
    state: &mut Value,
    reason: Option<Value>,
) -> Result<(), DfwError> {
    let Some(reason) = reason.filter(|v| js::truthy(v)) else {
        return Err(DfwError::new(
            "Re-opening an approved design needs --reason (it is recorded as a change proposal).",
        ));
    };
    let design = design_of(state)?;
    if design.get(state_key::STATUS).and_then(Value::as_str) != Some("approved") {
        return Err(DfwError::new(
            "The current design version is not approved; nothing to re-open.",
        ));
    }
    let from = design.get(state_key::VERSION).cloned();
    // `from + 1` of JavaScript: a number is added to, text is appended to.
    let next = match &from {
        Some(Value::Number(n)) => match n.as_i64() {
            Some(i) => Value::from(i + 1),
            None => Value::from(n.as_f64().unwrap_or(f64::NAN) + 1.0),
        },
        other => Value::String(format!("{}1", js::to_string(other.as_ref()))),
    };
    design.insert(state_key::VERSION.into(), next.clone());
    design.insert(state_key::STATUS.into(), "draft".into());
    design.insert(state_key::LOCKED.into(), false.into());
    match state.get_mut(state_key::GATES) {
        Some(Value::Object(gates)) => {
            gates.insert(gate::G2.into(), Value::Null);
            gates.insert(gate::G4.into(), Value::Null);
        }
        _ => return Err(state_field_error(state_key::GATES, "an object")),
    }
    let late = matches!(
        state.get(state_key::PHASE).and_then(Value::as_str),
        Some(IMPLEMENT | VERIFY | DONE | STACK)
    );
    if late {
        state[state_key::PHASE] = CONCEPTS.into();
    }
    push_history(
        state,
        json!({
            (state_key::AT): now_iso(),
            (state_key::EVENT): "reopen",
            (state_key::FROM): format!("v{}", js::to_string(from.as_ref())),
            (state_key::TO): format!("v{}", js::to_string(Some(&next))),
            (state_key::REASON): reason,
        }),
    )?;
    save_state(root, state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{SCREEN, SCREEN_PAGE};
    use crate::core::util::{test_dir, write_json};
    use crate::workflow::state::mode_name::NEW_PRODUCT;
    use crate::workflow::state::{load_state, new_state};

    /// e: a comment file or snapshot version from input never leaves design/
    /// (tests/unit/node-stable.test.mjs; the snapshot part)
    #[test]
    fn a_snapshot_version_from_input_never_leaves_design() {
        let root = test_dir("freeze-name");
        std::fs::create_dir_all(root.join("snap")).unwrap();
        std::fs::write(root.join("snap/approval.lock.json"), r#"{"files":[]}"#).unwrap();
        for bad in ["../../snap", "v", "v1/../../x", "1", "V1", "v1\n", ""] {
            let err = verify_snapshot(&root, bad).unwrap_err();
            assert_eq!(
                err.report(),
                format!(
                    "Unknown snapshot \"{bad}\". A snapshot is named v<number>, for example v1."
                )
            );
        }
        assert!(verify_snapshot(&root, "v12")
            .unwrap_err()
            .message()
            .starts_with("No approval.lock.json in "));
    }

    /// RV-1: a version in the state that is not a number never names the snapshot folder.
    #[test]
    fn a_version_from_the_state_never_leaves_design_approved() {
        let root = test_dir("freeze-escape");
        fs::create_dir_all(root.join(rel::DESIGN)).unwrap();
        fs::write(root.join(rel::MANIFEST), "{}").unwrap();
        for bad in [
            json!(format!("1/../../../../{ESCAPED}")),
            json!("../x"),
            json!(1.5),
            json!(null),
            json!(""),
        ] {
            let mut s = new_state(NEW_PRODUCT, "".into()).unwrap();
            s[state_key::DESIGN][state_key::VERSION] = bad.clone();
            let err = freeze_design(&root, &mut s, None).unwrap_err();
            assert!(
                err.message().starts_with(&format!(
                    "Cannot freeze: design.version in {} is ",
                    rel::STATE
                )),
                "{bad}"
            );
            assert_eq!(s[state_key::DESIGN]["status"], "draft", "{bad}");
        }
        /// The folder a version with `..` would name.
        const ESCAPED: &str = "escaped";
        assert!(!root.join(ESCAPED).exists() && !root.parent().unwrap().join(ESCAPED).exists());
        assert!(!root.join(rel::APPROVED).exists());
    }

    /// freeze: snapshot, lock, tamper detection, reopen (tests/unit/dfw.test.mjs)
    #[test]
    fn freeze_snapshot_lock_tamper_detection_reopen() {
        let root = test_dir("freeze");
        let mut s = new_state(NEW_PRODUCT, "".into()).unwrap();
        s[state_key::GATES]["G1"] = json!({ "approvedAt": "x" });
        s[state_key::PHASE] = CONCEPTS.into();
        save_state(&root, &mut s).unwrap();
        fs::create_dir_all(root.join(rel::screen(SCREEN))).unwrap();
        fs::write(root.join(rel::MANIFEST), r#"{"schema":1,"screens":[]}"#).unwrap();
        fs::write(root.join(rel::screen(SCREEN_PAGE)), "<html></html>").unwrap();
        fs::write(root.join(rel::DESIGN_DOC), "# Design").unwrap();
        let mut s = load_state(&root).unwrap().unwrap();
        freeze_design(&root, &mut s, Some("ok".into())).unwrap();
        let design = &s[state_key::DESIGN];
        assert_eq!(design["status"], "approved");
        assert!(root.join(rel::approved("v1/docs/DESIGN.md")).exists());
        assert!(verify_snapshot(&root, "v1").unwrap().is_empty());
        fs::write(
            root.join(rel::approved("v1/screens/home/a.html")),
            "<html>x</html>",
        )
        .unwrap();
        assert_eq!(
            verify_snapshot(&root, "v1").unwrap(),
            ["modified: screens/home/a.html"]
        );
        let mut again = load_state(&root).unwrap().unwrap();
        assert!(freeze_design(&root, &mut again, None)
            .unwrap_err()
            .message()
            .contains("already approved"));
        let mut s = load_state(&root).unwrap().unwrap();
        reopen_design(&root, &mut s, Some("client feedback".into())).unwrap();
        let design = &s[state_key::DESIGN];
        assert_eq!(design[state_key::VERSION], 2);
        assert_eq!(design["locked"], false);
        assert_eq!(s[state_key::GATES]["G2"], Value::Null);
    }

    #[test]
    fn the_lock_file_and_the_records_have_the_node_shape() {
        let root = test_dir("freeze-shape");
        fs::create_dir_all(root.join(rel::approved("old"))).unwrap();
        fs::create_dir_all(root.join(rel::HANDOFF)).unwrap();
        fs::write(root.join(rel::MANIFEST), "{}").unwrap();
        /// A stylesheet at the top of `design/`.
        const CSS: &str = "b.css";
        /// A handoff file of an earlier version.
        const OLD_HANDOFF: &str = "v0.md";
        fs::write(root.join(rel::design(CSS)), "b").unwrap();
        fs::write(
            root.join(rel::join(rel::HANDOFF, OLD_HANDOFF)),
            "not frozen",
        )
        .unwrap();
        fs::write(root.join(rel::PRODUCT_DOC), "# Product").unwrap();
        let h = hash_design(&root).unwrap();
        let paths: Vec<&str> = h.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, [CSS, file::MANIFEST, "docs/PRODUCT.md"]);
        let lines: Vec<String> = h
            .files
            .iter()
            .map(|f| format!("{}  {}", f.sha256, f.path))
            .collect();
        assert_eq!(h.hash, sha256(lines.join("\n")));

        let mut s = new_state(NEW_PRODUCT, "".into()).unwrap();
        freeze_design(&root, &mut s, None).unwrap();
        let lock = fs::read_to_string(root.join(rel::approved("v1/approval.lock.json"))).unwrap();
        assert!(lock.starts_with("{\n  \"version\": \"v1\",\n  \"approvedAt\": \""));
        assert!(!lock.contains("\"note\""), "no note, no field");
        assert!(!root.join(rel::approved("v1/handoff")).exists());
        assert_eq!(s[state_key::GATES]["G2"]["hash"], h.hash.as_str());
        /// The field of an approval record and of a history entry that names the version.
        const VERSION: &str = "version";
        assert_eq!(s[state_key::DESIGN][state_key::APPROVED][0][VERSION], "v1");
        let last = s[state_key::HISTORY].as_array().unwrap().last().unwrap();
        assert_eq!(last["gate"], "G2");
        assert_eq!(last[VERSION], "v1");

        fs::write(root.join(rel::approved("v1/extra.txt")), "x").unwrap();
        fs::remove_file(root.join(rel::approved("v1/b.css"))).unwrap();
        assert_eq!(
            verify_snapshot(&root, "v1").unwrap(),
            ["missing: b.css", "added: extra.txt"]
        );
        assert!(verify_snapshot(&root, "v9")
            .unwrap_err()
            .message()
            .starts_with("No approval.lock.json in "));
        let mut draft = new_state(NEW_PRODUCT, "".into()).unwrap();
        assert!(reopen_design(&root, &mut draft, None)
            .unwrap_err()
            .message()
            .contains("needs --reason"));
        const REASON: &str = "why";
        assert!(reopen_design(&root, &mut draft, Some(REASON.into()))
            .unwrap_err()
            .message()
            .contains("nothing to re-open"));
        s[state_key::PHASE] = VERIFY.into();
        reopen_design(&root, &mut s, Some(REASON.into())).unwrap();
        assert_eq!(s[state_key::PHASE], CONCEPTS);
        let last = s[state_key::HISTORY].as_array().unwrap().last().unwrap();
        assert_eq!(
            (last["from"].as_str(), last["to"].as_str()),
            (Some("v1"), Some("v2"))
        );
    }

    /// Moved from cli/tools (doctor's copy of verifySnapshot).
    #[test]
    fn snapshot_problems_are_missing_modified_and_added() {
        let root = test_dir("doctor-snapshot");
        let dir = project_paths(&root).approved.join("v1");
        fs::create_dir_all(dir.join(file::SCREENS)).unwrap();
        /// A file whose content is what the lock says.
        const KEPT: &str = "kept.html";
        const KEPT_TEXT: &str = "kept";
        /// A file whose content changed since the lock.
        const CHANGED: &str = "screens/changed.html";
        fs::write(dir.join(KEPT), KEPT_TEXT).unwrap();
        fs::write(dir.join(CHANGED), "now").unwrap();
        fs::write(dir.join("extra.css"), "x").unwrap();
        assert_eq!(
            verify_snapshot(&root, "v1").unwrap_err().message(),
            format!("No approval.lock.json in {}", dir.display())
        );
        write_json(
            &dir.join("approval.lock.json"),
            &json!({ "files": [
                { "path": KEPT, "sha256": sha256(KEPT_TEXT) },
                { "path": CHANGED, "sha256": sha256("before") },
                { "path": "gone.html", "sha256": sha256("gone") },
            ] }),
        )
        .unwrap();
        assert_eq!(
            verify_snapshot(&root, "v1").unwrap(),
            [
                format!("modified: {CHANGED}").as_str(),
                "missing: gone.html",
                "added: extra.css"
            ]
        );
    }
}
