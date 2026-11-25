//! Tests of the hot path that need the built binary: hook calls as a host makes them, the
//! reviews lock under several writer processes, and the command line from `dfw init` on.
//! Ported from tests/unit/*.test.mjs; each test names its JavaScript original.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

#[path = "../src/core/paths/layout.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the project layout"
)]
mod layout;
use layout::{file, rel};

#[path = "../src/core/host/wire.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of what a host sends"
)]
mod wire;
use wire::{patch, reply, tool};

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

#[path = "../src/core/fixture.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the fixture names"
)]
mod fixture;
use fixture::{
    APP_ENTRY, APP_FILE, APP_MODULE, BUTTON_MASTER, DEMO_PAGE, HANDOFF_NOTE, HTML_FILE,
    OTHER_HTML_FILE, SCREEN, SCREEN_PAGE,
};

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");
/// The component the CLI test copies and lists (its master is [`BUTTON_MASTER`]).
const BUTTON: &str = "button";
/// The keys of `dfw lang status`: whether this session chose a language, and the last one used.
const CHOSEN: &str = "chosen";
const LAST_USED: &str = "lastUsed";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A fresh, empty folder (its real path: macOS reaches the temporary directory through a symlink).
fn tmp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-hot-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
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

/// A copy of the fixture project (it has `design/`, so `dfw init` runs natively).
fn fixture_project(label: &str) -> PathBuf {
    let root = tmp_dir(label);
    copy_tree(&repo().join("tests/fixtures/penpot/project"), &root);
    root
}

/// What a run printed and how it ended.
struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Ran {
    /// The output of a run that must have succeeded.
    fn ok(self) -> String {
        assert_eq!(self.code, 0, "stderr: {}", self.stderr);
        self.stdout
    }

    fn json(self) -> Value {
        serde_json::from_str(&self.ok()).unwrap()
    }
}

/// `dfw <args>` in `root` with a clean environment: nothing of the session that runs the tests
/// (project folder, host, session id, env file) reaches the binary.
fn dfw_command(root: &Path, args: &[&str]) -> Command {
    // An empty user home: no engine or tool the machine has installed is found.
    let mut cmd = home::command(DFW, root, &root.join(".user"), &root.join(".home"));
    cmd.args(args);
    for name in ["IMPECCABLE_BIN", "IMPECCABLE_HOME"] {
        cmd.env_remove(name);
    }
    cmd.env("DFW_PLUGIN_ROOT", repo().join("core"));
    cmd
}

fn run(mut cmd: Command, stdin: &str) -> Ran {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A command that does not read its stdin may have closed it already.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    Ran {
        code: out.status.code().unwrap_or(-1),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn cli(root: &Path, args: &[&str]) -> Ran {
    run(dfw_command(root, args), "")
}

/// `dfw hook <name>` with a payload, the project given the way Claude Code gives it.
fn hook(root: &Path, name: &str, input: &Value) -> String {
    let mut cmd = dfw_command(root, &[command::HOOK, name]);
    cmd.env("CLAUDE_PROJECT_DIR", root);
    run(cmd, &input.to_string()).ok()
}

fn node_available() -> bool {
    Command::new("node")
        .arg(flag::VERSION)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn path_str(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// hooks: pre-tool-use denies code before gates, fails open on bad input (tests/unit/dfw.test.mjs)
#[test]
fn hooks_pre_tool_use_denies_code_before_gates_fails_open_on_bad_input() {
    let root = fixture_project("pre");
    cli(&root, &[command::INIT]).ok();
    let run_hook = |tool: &str, file: &str| {
        hook(
            &root,
            hook_name::PRE_TOOL_USE,
            &json!({ "tool_name": tool, "tool_input": { "file_path": root.join(file) }, "cwd": root }),
        )
    };
    let deny: Value = serde_json::from_str(&run_hook(tool::WRITE, APP_ENTRY)).unwrap();
    assert_eq!(
        deny[reply::HOOK_SPECIFIC_OUTPUT][reply::PERMISSION_DECISION],
        reply::DENY
    );
    assert_eq!(run_hook(tool::WRITE, &rel::design(OTHER_HTML_FILE)), "");
    assert_eq!(run_hook("Read", APP_ENTRY), "");
    let mut bad = dfw_command(&root, &[command::HOOK, hook_name::PRE_TOOL_USE]);
    bad.env("CLAUDE_PROJECT_DIR", &root);
    let bad = run(bad, "not json");
    assert_eq!((bad.code, bad.stdout.as_str()), (0, ""));
}

/// hook CLI: --host copilot denies in Copilot format (tests/unit/host.test.mjs)
#[test]
fn hook_cli_host_copilot_denies_in_copilot_format() {
    let root = fixture_project(host_id::COPILOT);
    cli(&root, &[command::INIT, flag::ROOT, &path_str(&root)]).ok();
    // No project folder in the environment: the payload's cwd names the project.
    let run_hook = |input: Value, host: Option<&str>| {
        let mut args = vec![command::HOOK, hook_name::PRE_TOOL_USE];
        if let Some(host) = host {
            args.extend([flag::HOST, host]);
        }
        run(dfw_command(&root, &args), &input.to_string()).ok()
    };
    let create = |path: &str| json!({ "cwd": root, "toolName": tool::alias::CREATE, "toolArgs": json!({ "path": path }).to_string() });
    let out: Value =
        serde_json::from_str(&run_hook(create(APP_FILE), Some(host_id::COPILOT))).unwrap();
    assert_eq!(out[reply::PERMISSION_DECISION], reply::DENY);
    assert_eq!(
        run_hook(create(&rel::design(HTML_FILE)), Some(host_id::COPILOT)),
        ""
    );
    let claude: Value = serde_json::from_str(&run_hook(
        json!({ "cwd": root, "tool_name": tool::WRITE, "tool_input": { "file_path": root.join(APP_FILE) } }),
        None,
    ))
    .unwrap();
    assert_eq!(
        claude[reply::HOOK_SPECIFIC_OUTPUT][reply::PERMISSION_DECISION],
        reply::DENY
    );
    // Relative patch paths resolve against the session cwd (here design/), not the project root.
    let patch = |cwd: &Path, file: &str| {
        run_hook(
            json!({ "cwd": cwd, "toolName": tool::alias::APPLY_PATCH, "toolArgs": patch::body(&[(patch::ADD, file)]) }),
            Some(host_id::COPILOT),
        )
    };
    let page = rel::join(file::SCREENS, HTML_FILE);
    assert_eq!(patch(&root.join(rel::DESIGN), &page), "");
    let denied: Value = serde_json::from_str(&patch(&root, &page)).unwrap();
    assert_eq!(denied[reply::PERMISSION_DECISION], reply::DENY);
}

/// hooks: user-prompt-submit and stop inject undelivered comments once (tests/unit/inbox.test.mjs)
#[test]
fn hooks_user_prompt_submit_and_stop_inject_undelivered_comments_once() {
    let root = fixture_project("inbox");
    cli(&root, &[command::INIT]).ok();
    let add = |input: Value| {
        cli(
            &root,
            &[command::COMMENTS, sub::ADD, flag::JSON, &input.to_string()],
        )
        .ok();
    };
    let prompt = json!({ "cwd": root, "prompt": "hi" });
    assert_eq!(
        hook(&root, hook_name::USER_PROMPT_SUBMIT, &prompt).trim(),
        ""
    );
    const COMMENT: &str = "Too much padding";
    add(json!({ "comment": COMMENT, "screen": "history", "target": { "dfwId": "history.table" } }));
    let ups: Value =
        serde_json::from_str(&hook(&root, hook_name::USER_PROMPT_SUBMIT, &prompt)).unwrap();
    assert!(ups[reply::HOOK_SPECIFIC_OUTPUT][reply::ADDITIONAL_CONTEXT]
        .as_str()
        .unwrap()
        .contains(COMMENT));
    assert_eq!(
        hook(&root, hook_name::STOP, &json!({ "cwd": root })).trim(),
        ""
    );
    add(json!({ "comment": "Use the danger variant", "screen": "assets" }));
    let st: Value =
        serde_json::from_str(&hook(&root, hook_name::STOP, &json!({ "cwd": root }))).unwrap();
    assert_eq!(st[reply::DECISION], reply::BLOCK);
    assert!(st[reply::REASON]
        .as_str()
        .unwrap()
        .contains("danger variant"));
    add(json!({ "comment": "x" }));
    let looping = json!({ "cwd": root, "stop_hook_active": true });
    assert_eq!(
        hook(&root, hook_name::STOP, &looping).trim(),
        "",
        "never loops"
    );
}

/// presence: written only for an edit the gate allows (tests/unit/studio-presence.test.mjs)
#[test]
fn presence_written_only_for_an_edit_the_gate_allows() {
    let root = fixture_project("presence");
    fs::create_dir_all(root.join(rel::REVIEWS)).unwrap();
    fs::write(
        root.join(rel::STATE),
        json!({ "mode": "new-product", "phase": "concepts", "active": true, "design": { "version": 1, "status": "draft", "locked": false, "approved": [] }, "gates": {} }).to_string(),
    )
    .unwrap();
    let file = root.join(rel::screen(DEMO_PAGE));
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "<main data-dfw-id=\"demo.main\">\n  <h1 data-dfw-id=\"demo.title\">Hello</h1>\n  <p>Plain line</p>\n  <dfw-use c=\"button\" id=\"demo.cta\">Go</dfw-use>\n  <span>After</span>\n</main>\n").unwrap();
    let note = root.join(rel::join(rel::DFW, file::PRESENCE));
    let edit = |f: &Path| {
        let mut cmd = dfw_command(&root, &[command::HOOK, hook_name::PRE_TOOL_USE]);
        cmd.env("DFW_PROJECT_DIR", &root);
        let input = json!({ "cwd": root, "tool_name": tool::EDIT, "tool_input": { "file_path": f, "old_string": "Hello", "new_string": "Hi" } });
        run(cmd, &input.to_string()).ok()
    };
    assert!(
        edit(&root.join(APP_MODULE)).contains("\"permissionDecision\":\"deny\""),
        "implementation is gated"
    );
    assert!(!note.exists());
    assert_eq!(edit(&file), "");
    let written: Value = serde_json::from_str(&fs::read_to_string(&note).unwrap()).unwrap();
    assert_eq!(written["dfwId"], "demo.title");
}

/// comments from concurrent processes are all stored with unique ids (tests/unit/studio-guard.test.mjs)
#[test]
fn comments_from_concurrent_processes_are_all_stored_with_unique_ids() {
    let root = fixture_project("lock");
    fs::create_dir_all(root.join(rel::REVIEWS)).unwrap();
    fs::write(
        root.join(rel::STATE),
        r#"{"design":{"version":1,"status":"draft","locked":false}}"#,
    )
    .unwrap();
    cli(
        &root,
        &[
            command::COMMENTS,
            sub::ADD,
            flag::JSON,
            r#"{"comment":"first"}"#,
        ],
    )
    .ok();
    let root_arg = path_str(&root);
    // Three writer processes at once, 40 comments each, as the Studio server and the CLI do.
    let writers: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|tag| {
            let args = [command::DEBUG, "hot", "add-comments", &root_arg, tag, "40"];
            dfw_command(&root, &args)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        })
        .collect();
    for writer in writers {
        let out = writer.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let all = cli(&root, &[command::COMMENTS, sub::LIST, flag::JSON]).json();
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 121);
    let ids: std::collections::HashSet<&str> =
        all.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 121);
    assert!(!root
        .join(rel::join(rel::REVIEWS, file::REVIEWS_LOCK))
        .exists());
}

/// language: asked once per session, last used remembered (tests/unit/dfw.test.mjs)
#[test]
fn language_asked_once_per_session_last_used_remembered() {
    let root = tmp_dir("lang");
    const SESSION: &str = "abc";
    let st = cli(&root, &[command::LANG, sub::STATUS, flag::SESSION, SESSION]).json();
    assert_eq!(st[CHOSEN], false);
    assert_eq!(st[LAST_USED], "en");
    cli(
        &root,
        &[command::LANG, sub::SET, "vi", flag::SESSION, SESSION],
    )
    .ok();
    assert_eq!(
        cli(&root, &[command::LANG, sub::STATUS, flag::SESSION, SESSION]).json()[CHOSEN],
        true
    );
    let other = cli(&root, &[command::LANG, sub::STATUS, flag::SESSION, "def"]).json();
    assert_eq!(other[CHOSEN], false, "new session asks again");
    assert_eq!(other[LAST_USED], "vi", "offers last used as default");
}

/// cli: init, gates, approve G2 blocked by MUST findings (tests/unit/dfw.test.mjs)
///
/// The native commands (phase, approve G1/G3, status, decisions) run between the ones still
/// forwarded to Node (init of an empty folder, system, approve DS/G2, stack, handoff), on the
/// same state file.
#[test]
fn cli_init_gates_approve_g2_blocked_by_must_findings() {
    if !node_available() {
        eprintln!("skipped: node is not on PATH (this case also runs the forwarded commands)");
        return;
    }
    let root = tmp_dir("cli");
    let ok = |args: &[&str]| cli(&root, args).ok();
    let fails = |args: &[&str], with: &str| {
        let r = cli(&root, args);
        assert_ne!(r.code, 0, "{args:?} should fail");
        assert!(r.stderr.contains(with), "{args:?}: {}", r.stderr);
    };
    ok(&[command::INIT, flag::PRODUCT, "Test"]);
    assert!(root.join(rel::PRODUCT_DOC).exists());
    assert!(root.join(rel::join(rel::TOKENS, file::TOKENS_CSS)).exists());
    fails(&[command::PHASE, names::phase::CONCEPTS], "G1");
    ok(&[command::PHASE, names::phase::DIRECTION]);
    ok(&[command::APPROVE, "G1", flag::NOTE, "Direction A"]);
    fails(&[command::PHASE, names::phase::CONCEPTS], "DS");
    ok(&[command::PHASE, names::phase::SYSTEM]);
    fails(
        &[command::APPROVE, "DS", flag::NOTE, "x"],
        "needs a design system",
    );
    fails(
        &[command::SYSTEM, sub::INIT, "--only", BUTTON],
        "--only was removed",
    );
    assert!(ok(&[command::SYSTEM, sub::INIT, "--preset", "industrial"])
        .contains("No components were copied"));
    assert!(ok(&[command::SYSTEM, "ref"]).contains(BUTTON));
    fs::write(root.join(rel::component(BUTTON_MASTER)), "<dfw-component name=\"button\" variants=\"primary\"><style>.b { min-height: var(--size-control); color: var(--color-on-accent); background: var(--color-accent); }\n.b:focus-visible { outline: var(--focus-width) solid var(--color-focus); }</style><template><button class=\"b\" type=\"button\"><dfw-slot>Go</dfw-slot></button></template><dfw-examples><dfw-use c=\"button\">Issue tool</dfw-use></dfw-examples></dfw-component>").unwrap();
    fails(&[command::APPROVE, "DS", flag::NOTE, "x"], "inventory");
    fs::write(
        root.join(rel::INVENTORY),
        json!({ "components": [{ "name": BUTTON, "purpose": "Confirm the issue at the counter", "usedBy": ["counter"] }] }).to_string(),
    )
    .unwrap();
    assert!(ok(&[command::SYSTEM, sub::INVENTORY]).contains("Inventory and masters agree"));
    fails(&[command::APPROVE, "DS", flag::NOTE, "x"], "Rationale");
    let rationale: String = [
        "Accent is signal blue because red/green/yellow already mean stop/go/caution on the floor.",
        "Neutrals are cool steel because the counter sits under fluorescent light.",
        "Base 16 with a 1.2 UI ratio because operators read from one metre.",
        "Large density because users wear gloves at the counter.",
        "Hazard tape motif only on the scan zone because it marks where to act.",
    ]
    .iter()
    .map(|x| format!("- {x}\n"))
    .collect();
    let design_md = fs::read_to_string(root.join(rel::DESIGN_DOC)).unwrap();
    fs::write(
        root.join(rel::DESIGN_DOC),
        format!("{design_md}\n## Rationale\n{rationale}"),
    )
    .unwrap();
    ok(&[
        command::APPROVE,
        "DS",
        flag::NOTE,
        "Industrial preset, button",
    ]);
    ok(&[command::PHASE, names::phase::CONCEPTS]);
    let screen = |img: &str| {
        format!("<!doctype html><html lang=\"en\"><head><link rel=\"stylesheet\" href=\"../../tokens/tokens.css\"></head><body>{img}</body></html>")
    };
    fs::create_dir_all(root.join(rel::screen(SCREEN))).unwrap();
    fs::write(
        root.join(rel::screen(SCREEN_PAGE)),
        screen("<img src=\"x.png\" data-dfw-id=\"hero.img\">"),
    )
    .unwrap();
    let manifest = root.join(rel::MANIFEST);
    let mut m: Value = serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    m["screens"] =
        json!([{ "id": "home", "variants": [{ "id": "a", "file": "screens/home/a.html" }] }]);
    fs::write(&manifest, m.to_string()).unwrap();
    fails(&[command::APPROVE, "G2", flag::NOTE, "v1"], "MUST finding");
    fs::write(
        root.join(rel::screen(SCREEN_PAGE)),
        screen("<img src=\"x.png\" alt=\"\" data-dfw-id=\"hero.img\">"),
    )
    .unwrap();
    // Neither axe nor the Impeccable engine is downloaded in these empty homes: the freeze needs
    // the user's acceptance (recorded), and both are reported as not verified.
    fails(
        &[command::APPROVE, "G2", flag::NOTE, "v1"],
        flag::ACCEPT_UNVERIFIED,
    );
    let frozen = ok(&[
        command::APPROVE,
        "G2",
        flag::NOTE,
        "v1",
        flag::ACCEPT_UNVERIFIED,
    ]);
    assert!(frozen.contains("Design frozen as v1"), "{frozen}");
    assert!(
        frozen.contains("NOT VERIFIED: impeccable (Engine not downloaded yet")
            && frozen.contains("; axe (axe-core not downloaded"),
        "{frozen}"
    );
    fails(&[command::PHASE, names::phase::IMPLEMENT], "G3");
    ok(&[
        command::STACK,
        sub::SET,
        flag::JSON,
        r#"{"framework":{"name":"Vue"}}"#,
    ]);
    ok(&[command::APPROVE, "G3", flag::NOTE, "Vue 3 + plain CSS"]);
    ok(&[command::PHASE, names::phase::IMPLEMENT]);
    assert!(ok(&[command::STATUS]).contains("phase=implement"));
    assert!(ok(&[command::HANDOFF]).contains(&rel::join(rel::HANDOFF, HANDOFF_NOTE)));
    let decisions = ok(&[command::DECISIONS]);
    assert!(decisions.contains("gate:G1"));
    assert!(decisions.contains("gate:G3"));
    assert!(decisions.contains("gate:G2:accepted-unverified"));
    // The snapshot the Node side froze is intact for the native check.
    assert_eq!(ok(&[command::VERIFY_SNAPSHOT]), "Snapshot v1 intact.\n");
}
