//! The edit lock and the reviews lock across processes of the built binary: Studio edits from
//! several processes are all kept and journaled, and writers behind a lock whose holder died
//! take it over once and keep every comment.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

/// A fresh, empty folder (its real path: macOS reaches the temporary directory through a symlink).
fn tmp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-locks-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// `dfw <args>` in `root`, nothing of the session that runs the tests reaching the binary.
fn dfw(root: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(DFW);
    cmd.args(args).current_dir(root);
    for name in [
        "DFW_HOST",
        "DFW_PROJECT_DIR",
        "CLAUDE_PROJECT_DIR",
        "DFW_SESSION_ID",
        "CLAUDE_SESSION_ID",
        "CLAUDE_ENV_FILE",
        "DFW_NOW",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("DFW_HOME", root.join(".home"))
        .env("HOME", root.join(".home"))
        .env("DFW_TELEMETRY", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

fn spawn_with(root: &Path, args: &[&str], stdin: &str) -> Child {
    let mut child = dfw(root, args).spawn().unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    child
}

fn finish(child: Child) -> String {
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Twelve processes edit one screen at once (as Studio threads of two Studios would): every
/// edit is kept, each has a journal record, and undoing them all restores the original bytes.
#[test]
fn edits_from_concurrent_processes_are_all_kept_and_journaled() {
    let root = tmp_dir("edits");
    let paras: String = (0..12)
        .map(|i| format!("<p data-dfw-id=\"p.{i}\">old {i}</p>\n"))
        .collect();
    let source = format!("<!doctype html><html><body>\n{paras}</body></html>\n");
    let screen = root.join("design/screens/many/a.html");
    fs::create_dir_all(screen.parent().unwrap()).unwrap();
    fs::write(&screen, &source).unwrap();
    let root_arg = root.to_string_lossy().into_owned();
    let editors: Vec<Child> = (0..12)
        .map(|i| {
            let op = json!({ "file": "screens/many/a.html", "op": "text", "id": format!("p.{i}"), "value": format!("new {i}") });
            spawn_with(&root, &["__debug", "edit", &root_arg], &op.to_string())
        })
        .collect();
    for editor in editors {
        let done: Value = serde_json::from_str(finish(editor).trim()).unwrap();
        assert_eq!(done["changed"], true);
    }
    let text = fs::read_to_string(&screen).unwrap();
    for i in 0..12 {
        assert!(text.contains(&format!(">new {i}<")), "edit {i} lost");
    }
    let records = fs::read_dir(root.join(".dfw/edits")).unwrap().count();
    assert_eq!(records, 12, "one record per edit, no lock left");
    for left in (0..12).rev() {
        let undone: Value = serde_json::from_str(
            finish(spawn_with(&root, &["__debug", "undo", &root_arg], "")).trim(),
        )
        .unwrap();
        assert_eq!(undone["remaining"], left);
    }
    assert_eq!(fs::read_to_string(&screen).unwrap(), source);
    fs::remove_dir_all(&root).unwrap();
}

/// A reviews lock whose holder died (an owner file nobody releases) and ten writer processes:
/// one takes it over, the others wait their turn, and every comment is stored.
#[test]
fn writers_behind_a_dead_holder_keep_every_comment() {
    let root = tmp_dir("held");
    fs::create_dir_all(root.join(".dfw/reviews/.lock")).unwrap();
    fs::write(
        root.join(".dfw/state.json"),
        r#"{"design":{"version":1,"status":"draft","locked":false}}"#,
    )
    .unwrap();
    fs::write(root.join(".dfw/reviews/.lock/owner"), "a process that died").unwrap();
    let writers: Vec<Child> = (0..10)
        .map(|i| {
            let input = json!({ "comment": format!("w{i}") }).to_string();
            spawn_with(&root, &["comments", "add", "--json", &input], "")
        })
        .collect();
    for writer in writers {
        finish(writer);
    }
    let list = finish(spawn_with(&root, &["comments", "list", "--json"], ""));
    let all: Value = serde_json::from_str(&list).unwrap();
    let ids: std::collections::HashSet<&str> = all
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 10);
    let left: Vec<String> = fs::read_dir(root.join(".dfw/reviews"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with(".lock"))
        .collect();
    assert!(left.is_empty(), "{left:?}");
    fs::remove_dir_all(&root).unwrap();
}
