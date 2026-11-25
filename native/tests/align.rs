//! The command-line cases of tests/unit/node-stable.test.mjs (the defects fixed in the last Node
//! release before the port), run against the binary. Each test names its JavaScript original by
//! the letter of the defect list.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

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
use word::{command, flag, hook_name, sub};

#[path = "../src/core/host/id.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the host ids")]
mod host_id;

#[path = "../src/workflow/names.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the workflow names"
)]
mod names;

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");
/// A name JavaScript objects already have: never a mode, a hook, a tool or a config key.
const RESERVED: &str = "constructor";

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
    fs::create_dir_all(dir.join(rel::DESIGN)).unwrap();
    let root = dir.canonicalize().unwrap();
    assert_eq!(dfw(&root, &[command::INIT], "").code, 0);
    root
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

/// `dfw <args>` in `root` with a clean environment and `stdin`.
fn dfw(root: &Path, args: &[&str], stdin: &str) -> Ran {
    let mut cmd = home::command(DFW, root, &root.join(".user"), &root.join(".home"));
    cmd.args(args)
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
    serde_json::from_str(&fs::read_to_string(root.join(rel::STATE)).unwrap()).unwrap()
}

/// b: --level and --timeout without a value are refused
#[test]
fn level_and_timeout_without_a_value_are_refused() {
    let root = project("b");
    let before = fs::read_to_string(root.join(rel::DECISIONS)).unwrap();
    user_error(
        &dfw(&root, &[command::DECIDE, "use", "tabs", "--level"], ""),
        "--level needs a value.",
    );
    assert_eq!(
        fs::read_to_string(root.join(rel::DECISIONS)).unwrap(),
        before,
        "no level 1 decision was recorded"
    );
    let started = Instant::now();
    user_error(
        &dfw(&root, &[command::COMMENTS, "wait", "--timeout"], ""),
        "--timeout needs a value.",
    );
    assert!(started.elapsed().as_millis() < 900, "it did not wait");
}

/// c: phase --force never writes a missing or unknown phase
#[test]
fn phase_force_never_writes_a_missing_or_unknown_phase() {
    let root = project("c");
    let r = dfw(&root, &[command::PHASE, flag::FORCE], "");
    assert_eq!(r.code, 1);
    assert!(
        r.stderr.contains("Unknown phase \"undefined\""),
        "{}",
        r.stderr
    );
    let r = dfw(&root, &[command::PHASE, "bogus", flag::FORCE], "");
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
        &dfw(&root, &[command::VERIFY_SNAPSHOT, "../../snap"], ""),
        "Unknown snapshot \"../../snap\". A snapshot is named v<number>, for example v1.",
    );
}

/// g: config set without a value is a usage error and writes nothing
#[test]
fn config_set_without_a_value_is_a_usage_error() {
    let root = project("g");
    let before = fs::read_to_string(root.join(rel::CONFIG)).unwrap();
    for args in [
        &[command::CONFIG, sub::SET, "a.b"][..],
        &[command::CONFIG, sub::SET],
    ] {
        user_error(
            &dfw(&root, args, ""),
            "Usage: dfw config set <key> <value> [--global|--project|--local]",
        );
    }
    assert_eq!(fs::read_to_string(root.join(rel::CONFIG)).unwrap(), before);
}

/// h: approve G3 refuses a stack.json that is not an object
#[test]
fn approve_g3_refuses_a_stack_json_that_is_not_an_object() {
    let root = project("h");
    let file = root.join(rel::STACK);
    for content in ["[1,2]", "\"next\"", "5", "null"] {
        fs::write(&file, content).unwrap();
        user_error(
            &dfw(&root, &[command::APPROVE, "G3", flag::NOTE, "ok"], ""),
            &format!("G3 needs a stack proposal: {} must hold a JSON object (run: dfw stack set ... or dfw stack detect --write).", rel::STACK),
        );
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            content,
            "left as it was"
        );
        assert_eq!(state(&root)["gates"]["G3"], Value::Null);
    }
    fs::write(&file, r#"{"framework":{"name":"x"}}"#).unwrap();
    assert_eq!(
        dfw(&root, &[command::APPROVE, "G3", flag::NOTE, "ok"], "").code,
        0
    );
    let stack: Value = serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
    assert_eq!(stack["status"], "approved");
}

/// j: names that are prototype keys are not ids (the CLI part)
#[test]
fn prototype_names_are_not_ids_on_the_command_line() {
    let root = project("j");
    let r = dfw(&root, &[command::MODE, RESERVED], "");
    assert_eq!(r.code, 1);
    assert!(r
        .stderr
        .starts_with("Unknown mode \"constructor\". Modes: "));
    let before = fs::read_to_string(root.join(rel::CONFIG)).unwrap();
    for (key, reserved) in [
        ("__proto__.x", "__proto__"),
        ("a.constructor", RESERVED),
        ("a.prototype.b", "prototype"),
    ] {
        user_error(
            &dfw(&root, &[command::CONFIG, sub::SET, key, "1"], ""),
            &format!("Invalid key \"{key}\": \"{reserved}\" is a reserved name."),
        );
    }
    assert_eq!(fs::read_to_string(root.join(rel::CONFIG)).unwrap(), before);
    let r = dfw(&root, &[command::HOOK, RESERVED], r#"{"block":"x"}"#);
    assert!(
        r.stderr.contains("Unknown hook \"constructor\""),
        "{}",
        r.stderr
    );
    let r = dfw(
        &root,
        &[command::MCP, sub::INSTALL, RESERVED, flag::PRINT],
        "",
    );
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
    let file = root.join(rel::CONFIG);
    for content in ["[1,2]", "\"text\"", "5", "null", "true"] {
        fs::write(&file, content).unwrap();
        user_error(
            &dfw(&root, &[command::CONFIG, sub::GET, "studio.port"], ""),
            &format!(
                "Config file {} must hold a JSON object. Fix it or delete it.",
                file.display()
            ),
        );
        let r = dfw(&root, &[command::CONFIG, sub::SET, "x", "1"], "");
        assert_eq!(r.code, 1);
        assert!(r.stderr.contains("must hold a JSON object"));
        assert_eq!(fs::read_to_string(&file).unwrap(), content);
    }
}

/// l: host helpers answer with their own messages, not JavaScript errors (the CLI part)
#[test]
fn hooks_and_install_answer_with_their_own_messages() {
    let root = project("l");
    let hook = dfw(&root, &[command::HOOK, hook_name::SESSION_START], "null");
    assert_eq!(hook.code, 0);
    assert_eq!(hook.stderr, "");
    let out: Value = serde_json::from_str(&hook.stdout).unwrap();
    assert!(out["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("phase=discovery"));
    let bare = dfw(
        &root,
        &[command::HOOK, hook_name::SESSION_START, flag::HOST],
        "{}",
    );
    assert_eq!(bare.code, 0, "a hook fails open");
    assert_eq!(
        bare.stderr,
        "dfw hook session-start error: --host needs a value.\n"
    );
    assert_eq!(bare.stdout, "");
    user_error(
        &dfw(&root, &[command::INSTALL, flag::HOST], ""),
        "--host needs a value.",
    );
}

/// m: a flag that takes a value is refused without one
#[test]
fn a_flag_that_takes_a_value_is_refused_without_one() {
    let root = project("m");
    let cases: [(&str, &[&str]); 8] = [
        ("root", &[command::STATUS]),
        ("port", &[command::STUDIO, sub::STATUS]),
        ("note", &[command::APPROVE, "G1"]),
        ("reason", &[command::REOPEN]),
        ("message", &[command::COMMENTS, "reply", "fb_001"]),
        ("session", &[command::LANG, sub::SET, "vi"]),
        ("status", &[command::COMMENTS, sub::LIST]),
        ("url", &[command::REVIEW, sub::START]),
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
        dfw(
            &root,
            &[command::STATUS, flag::JSON, flag::ROOT, &root_text],
            ""
        )
        .code,
        0
    );
    assert_eq!(
        dfw(&root, &[command::COMMENTS, sub::LIST, "--status=open"], "").code,
        0
    );
}

/// n: config set refuses a key that goes through a list
#[test]
fn config_set_refuses_a_key_that_goes_through_a_list() {
    let root = project("n");
    fs::remove_file(root.join(rel::CONFIG)).unwrap();
    assert_eq!(
        dfw(
            &root,
            &[command::CONFIG, sub::SET, "gate.allow", "[\"src/**\"]"],
            ""
        )
        .code,
        0
    );
    user_error(
        &dfw(
            &root,
            &[command::CONFIG, sub::SET, "gate.allow.0.b", "5"],
            "",
        ),
        "Cannot set \"gate.allow.0.b\": \"gate.allow\" is a list; set the whole list instead.",
    );
    let r = dfw(
        &root,
        &[command::CONFIG, sub::SET, "gate.allow.extra", "5"],
        "",
    );
    assert!(r.stderr.contains("\"gate.allow\" is a list"));
    let config: Value =
        serde_json::from_str(&fs::read_to_string(root.join(rel::CONFIG)).unwrap()).unwrap();
    assert_eq!(
        config,
        serde_json::json!({ "gate": { "allow": ["src/**"] } })
    );
}

/// p: a Studio pid that is not a positive integer is never signalled
#[test]
fn a_studio_pid_that_is_not_a_positive_integer_is_never_signalled() {
    /// The key of `dfw status --json` that holds the running Studio.
    const STUDIO: &str = "studio";
    let root = project("p");
    for pid in ["-1", "0", "-5", "1.5", "\"12\"", "true", "null"] {
        fs::write(
            root.join(rel::join(rel::DFW, file::STUDIO)),
            format!(r#"{{"pid":{pid},"url":"http://127.0.0.1:4600/"}}"#),
        )
        .unwrap();
        let r = dfw(&root, &[command::STATUS, flag::JSON], "");
        let out: Value = serde_json::from_str(&r.stdout).unwrap();
        assert_eq!(out[STUDIO], Value::Null, "{pid}");
    }
    let me = std::process::id();
    fs::write(
        root.join(rel::join(rel::DFW, file::STUDIO)),
        format!(r#"{{"pid":{me},"url":"http://127.0.0.1:4600/"}}"#),
    )
    .unwrap();
    let out: Value =
        serde_json::from_str(&dfw(&root, &[command::STATUS, flag::JSON], "").stdout).unwrap();
    assert_eq!(out[STUDIO]["pid"], me);
}

/// review: .dfw/presence.json is ignored by git in a new project
#[test]
fn presence_json_is_ignored_by_git_in_a_new_project() {
    let root = project("presence");
    let ignore = fs::read_to_string(root.join(rel::join(rel::DFW, file::GITIGNORE))).unwrap();
    assert!(ignore.lines().any(|l| l == "presence.json"), "{ignore}");
}

/// f: Copilot post-tool-use keeps comments undelivered when the checks throw
#[test]
fn post_tool_use_keeps_comments_undelivered_when_the_checks_throw() {
    let root = project("post-f");
    let added = dfw(
        &root,
        &[
            command::COMMENTS,
            sub::ADD,
            flag::JSON,
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
        &[
            command::HOOK,
            hook_name::POST_TOOL_USE,
            flag::HOST,
            host_id::COPILOT,
        ],
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
        &[
            command::HOOK,
            hook_name::POST_TOOL_USE,
            flag::HOST,
            host_id::COPILOT,
        ],
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
