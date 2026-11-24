//! Tests of the Penpot bridge that need the built binary: the command lines of
//! tests/unit/penpot-tokens.test.mjs and penpot-screens.test.mjs. Each test names its
//! JavaScript original.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

#[path = "../src/core/paths/layout.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the project layout"
)]
mod layout;
use layout::{file, rel};

#[path = "../src/core/word.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the command line")]
mod word;
use word::{command, flag, sub};

/// What a dry run of an import says it was.
const DRY_RUN: &str = "Dry run";
/// The changes file an import reads.
const CHANGES: &str = "changes.json";
const DFW: &str = env!("CARGO_BIN_EXE_dfw");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn fix() -> PathBuf {
    repo().join("tests/fixtures/penpot")
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// A copy of the fixture project with a workflow (`dfw init`), under its real path.
fn project(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-penpot-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    let root = dir.canonicalize().unwrap();
    copy_tree(&fix().join("project"), &root);
    ok(&root, &[command::INIT, flag::PRODUCT, "T"]);
    root
}

/// What a run printed and how it ended: exit code, stdout, stderr.
fn cli(root: &Path, args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(DFW);
    cmd.args(args).current_dir(root);
    for name in [
        "DFW_HOST",
        "DFW_PROJECT_DIR",
        "CLAUDE_PROJECT_DIR",
        "DFW_NOW",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("DFW_HOME", root.join(".home"))
        .env("DFW_PLUGIN_ROOT", repo().join("core"));
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The output of a run that must succeed.
fn ok(root: &Path, args: &[&str]) -> String {
    let (code, stdout, stderr) = cli(root, args);
    assert_eq!(code, 0, "dfw {args:?}: {stderr}");
    stdout
}

fn set_locked(root: &Path, locked: bool) {
    let file = root.join(rel::STATE);
    let mut state: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    state["design"]["locked"] = locked.into();
    state["design"]["status"] = if locked { "approved" } else { "draft" }.into();
    fs::write(&file, state.to_string()).unwrap();
}

/// penpot tokens: CLI export/import, refused while frozen, dry run allowed
#[test]
fn tokens_export_and_import_refused_while_frozen_dry_run_allowed() {
    /// The file `--out` names.
    const OUT_FILE: &str = "pp.json";
    let root = project("tokens-cli");
    fs::copy(fix().join("dfw-tokens.json"), root.join(rel::TOKENS_JSON)).unwrap();
    let out = ok(
        &root,
        &[
            command::TOKENS,
            sub::EXPORT,
            flag::FORMAT,
            "penpot",
            flag::OUT,
            OUT_FILE,
        ],
    );
    assert!(out.contains("Wrote pp.json"), "{out}");
    assert!(out.contains("Not exported: base easing.out (cubicBezier)"));
    assert!(root.join(OUT_FILE).exists());
    let edited = fix().join("penpot-edited.tokens.json");
    let edited = edited.to_str().unwrap();
    let dry = ok(
        &root,
        &[command::TOKENS, sub::IMPORT, edited, flag::DRY_RUN],
    );
    let change = dry.find("color.blue.600: #0066ff -> #0055dd").unwrap();
    assert!(dry[change..].contains(DRY_RUN));
    set_locked(&root, true);
    let (code, _, stderr) = cli(&root, &[command::TOKENS, sub::IMPORT, edited]);
    assert_eq!(code, 1);
    let frozen = stderr.find("frozen (G2)").unwrap();
    assert!(stderr[frozen..].contains("dfw reopen"));
    assert!(ok(
        &root,
        &[command::TOKENS, sub::IMPORT, edited, flag::DRY_RUN]
    )
    .contains(DRY_RUN));
    set_locked(&root, false);
    assert!(ok(&root, &[command::TOKENS, sub::IMPORT, edited]).contains("tokens.css rebuilt"));
    let css = fs::read_to_string(root.join(rel::join(rel::TOKENS, file::TOKENS_CSS))).unwrap();
    assert!(css.contains("--color-blue-600: #0055dd;"));
    // Usage errors of the two branches.
    let (code, _, stderr) = cli(
        &root,
        &[command::TOKENS, sub::EXPORT, flag::FORMAT, "figma"],
    );
    assert_eq!(
        (code, stderr.trim_end()),
        (1, "Unknown --format \"figma\" (supported: penpot).")
    );
    let (code, _, stderr) = cli(&root, &[command::TOKENS, sub::IMPORT]);
    assert_eq!(code, 1);
    assert!(stderr.starts_with("Usage: dfw tokens import <penpot-tokens.json|folder>"));
}

/// penpot CLI: import, plugin instructions
#[test]
fn penpot_import_and_plugin_instructions() {
    let root = project("penpot-cli");
    // The fixture export, as "dfw penpot export" would have written it.
    let renders = fix().join("renders.json");
    let tokens = root.join(rel::TOKENS_JSON);
    let doc = ok(
        &root,
        &[
            command::DEBUG,
            "penpot-build-export",
            renders.to_str().unwrap(),
            tokens.to_str().unwrap(),
        ],
    );
    let id = serde_json::from_str::<Value>(&doc).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let exports = root.join(rel::PENPOT).join(file::PENPOT_EXPORTS);
    fs::create_dir_all(&exports).unwrap();
    fs::write(exports.join(format!("{id}.json")), &doc).unwrap();
    fs::write(root.join(rel::PENPOT_EXPORT), &doc).unwrap();
    let mut changes: Value =
        serde_json::from_str(&fs::read_to_string(fix().join(CHANGES)).unwrap()).unwrap();
    changes["exportId"] = id.into();
    fs::write(root.join(CHANGES), changes.to_string()).unwrap();

    let dry = ok(
        &root,
        &[command::PENPOT, sub::IMPORT, CHANGES, flag::DRY_RUN],
    );
    let text = dry.find("1 text").unwrap();
    assert!(dry[text..].contains(DRY_RUN));
    assert!(!root.join(rel::REVIEWS).exists() || !dry.contains("fb_001"));
    let real = ok(&root, &[command::PENPOT, sub::IMPORT, CHANGES]);
    assert!(real.contains("Comments are in the review loop"));
    assert!(real.contains("fb_001 items/a/desktop -> items.add"));
    let json: Value = serde_json::from_str(&ok(
        &root,
        &[command::PENPOT, sub::IMPORT, CHANGES, flag::JSON],
    ))
    .unwrap();
    assert_eq!(json["comments"], serde_json::json!([]));
    assert!(json["skipped"].as_u64().unwrap() >= 5);
    let plugin = ok(&root, &[command::PENPOT, sub::PLUGIN]);
    assert!(plugin.starts_with("The Studio is not running: start it with \"dfw studio start\".\nPenpot plugin manifest: http://127.0.0.1:4600/__dfw/penpot/manifest.json\n"));
    assert!(plugin.contains("No Penpot API token"));
    let (code, _, stderr) = cli(&root, &[command::PENPOT, "nope"]);
    assert_eq!(code, 1);
    assert!(stderr.starts_with("Usage: dfw penpot export"));
    let (code, _, stderr) = cli(&root, &[command::PENPOT, sub::IMPORT, "missing.json"]);
    assert_eq!(
        (code, stderr.trim_end()),
        (1, "missing.json: not found or not JSON.")
    );
}
