//! The command-line cases of tests/unit/node-stable.test.mjs (the defects fixed in the last Node
//! release before the port), run against the binary. Each test names its JavaScript original by
//! the letter of the defect list.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use serde_json::Value;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A project with a `design/` folder and a fresh workflow (`dfw init`, native there).
fn project(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-align-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(dir.join("design")).unwrap();
    let root = dir.canonicalize().unwrap();
    assert_eq!(dfw(&root, &["init"], "").code, 0);
    root
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

/// `dfw <args>` in `root` with a clean environment and `stdin`.
fn dfw(root: &Path, args: &[&str], stdin: &str) -> Ran {
    let mut cmd = Command::new(DFW);
    cmd.args(args).current_dir(root);
    for name in [
        "DFW_NATIVE",
        "DFW_HOST",
        "DFW_PROJECT_DIR",
        "DFW_SESSION_ID",
        "CLAUDE_SESSION_ID",
        "CLAUDE_ENV_FILE",
        "DFW_NOW",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("DFW_HOME", root.join(".home"))
        .env("CLAUDE_PROJECT_DIR", root)
        .env("DFW_PLUGIN_ROOT", repo().join("core"));
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    Ran {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

/// The error is the user's: exit 1, the message on a line of stderr, no JavaScript error text.
fn user_error(r: &Ran, line: &str) {
    assert_eq!(r.code, 1, "stderr: {}", r.stderr);
    assert!(
        r.stderr.lines().any(|l| l == line),
        "expected {line:?} in {:?}",
        r.stderr
    );
    assert!(!r.stderr.contains("TypeError"), "{}", r.stderr);
}

fn state(root: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(root.join(".dfw/state.json")).unwrap()).unwrap()
}

/// b: --level and --timeout without a value are refused
#[test]
fn level_and_timeout_without_a_value_are_refused() {
    let root = project("b");
    let before = fs::read_to_string(root.join(".dfw/decisions.jsonl")).unwrap();
    user_error(
        &dfw(&root, &["decide", "use", "tabs", "--level"], ""),
        "--level needs a value.",
    );
    assert_eq!(
        fs::read_to_string(root.join(".dfw/decisions.jsonl")).unwrap(),
        before,
        "no level 1 decision was recorded"
    );
    let started = Instant::now();
    user_error(
        &dfw(&root, &["comments", "wait", "--timeout"], ""),
        "--timeout needs a value.",
    );
    assert!(started.elapsed().as_millis() < 900, "it did not wait");
}

/// c: phase --force never writes a missing or unknown phase
#[test]
fn phase_force_never_writes_a_missing_or_unknown_phase() {
    let root = project("c");
    let r = dfw(&root, &["phase", "--force"], "");
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("Unknown phase \"undefined\""),
        "{}",
        r.stderr
    );
    let r = dfw(&root, &["phase", "bogus", "--force"], "");
    assert!(r.stderr.contains("Unknown phase \"bogus\""), "{}", r.stderr);
    assert_eq!(state(&root)["phase"], "discovery");
}

/// e: a comment file or snapshot version from input never leaves design/ (the CLI part)
#[test]
fn a_snapshot_version_from_the_command_line_never_leaves_design() {
    let root = project("e");
    fs::create_dir_all(root.join("snap")).unwrap();
    fs::write(root.join("snap/approval.lock.json"), r#"{"files":[]}"#).unwrap();
    user_error(
        &dfw(&root, &["verify-snapshot", "../../snap"], ""),
        "Unknown snapshot \"../../snap\". A snapshot is named v<number>, for example v1.",
    );
}

/// g: config set without a value is a usage error and writes nothing
#[test]
fn config_set_without_a_value_is_a_usage_error() {
    let root = project("g");
    let before = fs::read_to_string(root.join(".dfw/config.json")).unwrap();
    for args in [&["config", "set", "a.b"][..], &["config", "set"]] {
        user_error(
            &dfw(&root, args, ""),
            "Usage: dfw config set <key> <value> [--global|--project|--local]",
        );
    }
    assert_eq!(
        fs::read_to_string(root.join(".dfw/config.json")).unwrap(),
        before
    );
}

/// h: approve G3 refuses a stack.json that is not an object
#[test]
fn approve_g3_refuses_a_stack_json_that_is_not_an_object() {
    let root = project("h");
    let file = root.join(".dfw/stack.json");
    for content in ["[1,2]", "\"next\"", "5", "null"] {
        fs::write(&file, content).unwrap();
        user_error(
            &dfw(&root, &["approve", "G3", "--note", "ok"], ""),
            "G3 needs a stack proposal: .dfw/stack.json must hold a JSON object (run: dfw stack set ... or dfw stack detect --write).",
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            content,
            "left as it was"
        );
        assert_eq!(state(&root)["gates"]["G3"], Value::Null);
    }
    fs::write(&file, r#"{"framework":{"name":"x"}}"#).unwrap();
    assert_eq!(dfw(&root, &["approve", "G3", "--note", "ok"], "").code, 0);
    let stack: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(stack["status"], "approved");
}

/// j: names that are prototype keys are not ids (the CLI part)
#[test]
fn prototype_names_are_not_ids_on_the_command_line() {
    let root = project("j");
    let r = dfw(&root, &["mode", "constructor"], "");
    assert_eq!(r.code, 1);
    assert!(r
        .stderr
        .starts_with("Unknown mode \"constructor\". Modes: "));
    let before = fs::read_to_string(root.join(".dfw/config.json")).unwrap();
    for (key, reserved) in [
        ("__proto__.x", "__proto__"),
        ("a.constructor", "constructor"),
        ("a.prototype.b", "prototype"),
    ] {
        user_error(
            &dfw(&root, &["config", "set", key, "1"], ""),
            &format!("Invalid key \"{key}\": \"{reserved}\" is a reserved name."),
        );
    }
    assert_eq!(
        fs::read_to_string(root.join(".dfw/config.json")).unwrap(),
        before
    );
    let r = dfw(&root, &["hook", "constructor"], r#"{"block":"x"}"#);
    assert!(
        r.stderr.contains("Unknown hook \"constructor\""),
        "{}",
        r.stderr
    );
    let r = dfw(&root, &["mcp", "install", "constructor", "--print"], "");
    assert!(
        r.stderr
            .contains("Unknown tool \"constructor\". MCP install targets: "),
        "{}",
        r.stderr
    );
}

/// k: a config file that is not a JSON object is a clear error
#[test]
fn a_config_file_that_is_not_a_json_object_is_a_clear_error() {
    let root = project("k");
    let file = root.join(".dfw/config.json");
    for content in ["[1,2]", "\"text\"", "5", "null", "true"] {
        fs::write(&file, content).unwrap();
        user_error(
            &dfw(&root, &["config", "get", "studio.port"], ""),
            &format!(
                "Config file {} must hold a JSON object. Fix it or delete it.",
                file.display()
            ),
        );
        let r = dfw(&root, &["config", "set", "x", "1"], "");
        assert_eq!(r.code, 1);
        assert!(r.stderr.contains("must hold a JSON object"));
        assert_eq!(fs::read_to_string(&file).unwrap(), content);
    }
}

/// l: host helpers answer with their own messages, not JavaScript errors (the CLI part)
#[test]
fn hooks_and_install_answer_with_their_own_messages() {
    let root = project("l");
    let hook = dfw(&root, &["hook", "session-start"], "null");
    assert_eq!(hook.code, 0);
    assert_eq!(hook.stderr, "");
    let out: Value = serde_json::from_str(&hook.stdout).unwrap();
    assert!(out["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("phase=discovery"));
    let bare = dfw(&root, &["hook", "session-start", "--host"], "{}");
    assert_eq!(bare.code, 0, "a hook fails open");
    assert_eq!(
        bare.stderr,
        "dfw hook session-start error: --host needs a value.\n"
    );
    assert_eq!(bare.stdout, "");
    user_error(
        &dfw(&root, &["install", "--host"], ""),
        "--host needs a value.",
    );
}

/// m: a flag that takes a value is refused without one
#[test]
fn a_flag_that_takes_a_value_is_refused_without_one() {
    let root = project("m");
    let cases: [(&str, &[&str]); 8] = [
        ("root", &["status"]),
        ("port", &["studio", "status"]),
        ("note", &["approve", "G1"]),
        ("reason", &["reopen"]),
        ("message", &["comments", "reply", "fb_001"]),
        ("session", &["lang", "set", "vi"]),
        ("status", &["comments", "list"]),
        ("url", &["review", "start"]),
    ];
    for (flag, args) in cases {
        let flag_arg = format!("--{flag}");
        let mut argv = args.to_vec();
        argv.push(&flag_arg);
        user_error(&dfw(&root, &argv, ""), &format!("--{flag} needs a value."));
    }
    assert_eq!(
        state(&root)["gates"]["G1"],
        Value::Null,
        "no approval with the note \"true\""
    );
    let root_text = root.display().to_string();
    assert_eq!(
        dfw(&root, &["status", "--json", "--root", &root_text], "").code,
        0
    );
    assert_eq!(
        dfw(&root, &["comments", "list", "--status=open"], "").code,
        0
    );
}

/// n: config set refuses a key that goes through a list
#[test]
fn config_set_refuses_a_key_that_goes_through_a_list() {
    let root = project("n");
    fs::remove_file(root.join(".dfw/config.json")).unwrap();
    assert_eq!(
        dfw(&root, &["config", "set", "gate.allow", "[\"src/**\"]"], "").code,
        0
    );
    user_error(
        &dfw(&root, &["config", "set", "gate.allow.0.b", "5"], ""),
        "Cannot set \"gate.allow.0.b\": \"gate.allow\" is a list; set the whole list instead.",
    );
    let r = dfw(&root, &["config", "set", "gate.allow.extra", "5"], "");
    assert!(r.stderr.contains("\"gate.allow\" is a list"));
    let config: Value =
        serde_json::from_str(&fs::read_to_string(root.join(".dfw/config.json")).unwrap()).unwrap();
    assert_eq!(
        config,
        serde_json::json!({ "gate": { "allow": ["src/**"] } })
    );
}

/// p: a Studio pid that is not a positive integer is never signalled
#[test]
fn a_studio_pid_that_is_not_a_positive_integer_is_never_signalled() {
    let root = project("p");
    for pid in ["-1", "0", "-5", "1.5", "\"12\"", "true", "null"] {
        fs::write(
            root.join(".dfw/studio.json"),
            format!(r#"{{"pid":{pid},"url":"http://127.0.0.1:4600/"}}"#),
        )
        .unwrap();
        let r = dfw(&root, &["status", "--json"], "");
        let out: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(out["studio"], Value::Null, "{pid}");
    }
    let me = std::process::id();
    fs::write(
        root.join(".dfw/studio.json"),
        format!(r#"{{"pid":{me},"url":"http://127.0.0.1:4600/"}}"#),
    )
    .unwrap();
    let out: Value = serde_json::from_str(&dfw(&root, &["status", "--json"], "").stdout).unwrap();
    assert_eq!(out["studio"]["pid"], me);
}

/// review: .dfw/presence.json is ignored by git in a new project
#[test]
fn presence_json_is_ignored_by_git_in_a_new_project() {
    let root = project("presence");
    let ignore = fs::read_to_string(root.join(".dfw/.gitignore")).unwrap();
    assert!(ignore.lines().any(|l| l == "presence.json"), "{ignore}");
}

/// f: Copilot post-tool-use keeps comments undelivered when the checks throw
#[test]
fn post_tool_use_keeps_comments_undelivered_when_the_checks_throw() {
    let root = project("post-f");
    let added = dfw(
        &root,
        &[
            "comments",
            "add",
            "--json",
            r#"{"comment":"Make it bigger"}"#,
        ],
        "",
    );
    assert_eq!(added.code, 0, "{}", added.stderr);
    // A file path that is not text makes the edited-files step fail.
    let payload = format!(
        r#"{{"cwd":{},"tool_name":"Write","tool_input":{{"file_path":123}}}}"#,
        serde_json::to_string(&root).unwrap()
    );
    let r = dfw(
        &root,
        &["hook", "post-tool-use", "--host", "copilot"],
        &payload,
    );
    assert_eq!(r.code, 0, "the hook fails open");
    assert!(
        r.stderr.contains("dfw hook post-tool-use error"),
        "{}",
        r.stderr
    );
    assert_eq!(r.stdout, "");
    let payload = format!(
        r#"{{"cwd":{},"tool_name":"Read","tool_input":{{}}}}"#,
        serde_json::to_string(&root).unwrap()
    );
    let next = dfw(
        &root,
        &["hook", "post-tool-use", "--host", "copilot"],
        &payload,
    );
    let out: Value = serde_json::from_str(&next.stdout).unwrap();
    assert!(
        out["additionalContext"]
            .as_str()
            .unwrap()
            .contains("Make it bigger"),
        "the comment waited for the next hook: {}",
        next.stdout
    );
}
