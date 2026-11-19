//! Tests of the hot path that need the built binary: hook calls as a host makes them, the
//! reviews lock under several writer processes, and the command line from `dfw init` on.
//! Ported from tests/unit/*.test.mjs; each test names its JavaScript original.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

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
        "IMPECCABLE_BIN",
        "IMPECCABLE_HOME",
    ] {
        cmd.env_remove(name);
    }
    // An empty user home: no engine or tool the machine has installed is found.
    let user_home = root.join(".user");
    cmd.env("DFW_HOME", root.join(".home"))
        .env("HOME", &user_home)
        .env("USERPROFILE", &user_home)
        .env("DFW_PLUGIN_ROOT", repo().join("core"));
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
    let mut cmd = dfw_command(root, &["hook", name]);
    cmd.env("CLAUDE_PROJECT_DIR", root);
    run(cmd, &input.to_string()).ok()
}

fn node_available() -> bool {
    Command::new("node")
        .arg("--version")
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
    cli(&root, &["init"]).ok();
    let run_hook = |tool: &str, file: &str| {
        hook(
            &root,
            "pre-tool-use",
            &json!({ "tool_name": tool, "tool_input": { "file_path": root.join(file) }, "cwd": root }),
        )
    };
    let deny: Value = serde_json::from_str(&run_hook("Write", "src/main.ts")).unwrap();
    assert_eq!(deny["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(run_hook("Write", "design/x.html"), "");
    assert_eq!(run_hook("Read", "src/main.ts"), "");
    let mut bad = dfw_command(&root, &["hook", "pre-tool-use"]);
    bad.env("CLAUDE_PROJECT_DIR", &root);
    let bad = run(bad, "not json");
    assert_eq!((bad.code, bad.stdout.as_str()), (0, ""));
}

/// hook CLI: --host copilot denies in Copilot format (tests/unit/host.test.mjs)
#[test]
fn hook_cli_host_copilot_denies_in_copilot_format() {
    let root = fixture_project("copilot");
    cli(&root, &["init", "--root", &path_str(&root)]).ok();
    // No project folder in the environment: the payload's cwd names the project.
    let run_hook = |input: Value, host: Option<&str>| {
        let mut args = vec!["hook", "pre-tool-use"];
        if let Some(host) = host {
            args.extend(["--host", host]);
        }
        run(dfw_command(&root, &args), &input.to_string()).ok()
    };
    let create = |path: &str| json!({ "cwd": root, "toolName": "create", "toolArgs": json!({ "path": path }).to_string() });
    let out: Value = serde_json::from_str(&run_hook(create("src/a.ts"), Some("copilot"))).unwrap();
    assert_eq!(out["permissionDecision"], "deny");
    assert_eq!(run_hook(create("design/a.html"), Some("copilot")), "");
    let claude: Value = serde_json::from_str(&run_hook(
        json!({ "cwd": root, "tool_name": "Write", "tool_input": { "file_path": root.join("src/a.ts") } }),
        None,
    ))
    .unwrap();
    assert_eq!(claude["hookSpecificOutput"]["permissionDecision"], "deny");
    // Relative patch paths resolve against the session cwd (here design/), not the project root.
    let patch = |cwd: &Path, file: &str| {
        run_hook(
            json!({ "cwd": cwd, "toolName": "apply_patch", "toolArgs": format!("*** Begin Patch\n*** Add File: {file}\n*** End Patch") }),
            Some("copilot"),
        )
    };
    assert_eq!(patch(&root.join("design"), "screens/a.html"), "");
    let denied: Value = serde_json::from_str(&patch(&root, "screens/a.html")).unwrap();
    assert_eq!(denied["permissionDecision"], "deny");
}

/// hooks: user-prompt-submit and stop inject undelivered comments once (tests/unit/inbox.test.mjs)
#[test]
fn hooks_user_prompt_submit_and_stop_inject_undelivered_comments_once() {
    let root = fixture_project("inbox");
    cli(&root, &["init"]).ok();
    let add = |input: Value| {
        cli(&root, &["comments", "add", "--json", &input.to_string()]).ok();
    };
    let prompt = json!({ "cwd": root, "prompt": "hi" });
    assert_eq!(hook(&root, "user-prompt-submit", &prompt).trim(), "");
    add(
        json!({ "comment": "Too much padding", "screen": "history", "target": { "dfwId": "history.table" } }),
    );
    let ups: Value = serde_json::from_str(&hook(&root, "user-prompt-submit", &prompt)).unwrap();
    assert!(ups["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap()
        .contains("Too much padding"));
    assert_eq!(hook(&root, "stop", &json!({ "cwd": root })).trim(), "");
    add(json!({ "comment": "Use the danger variant", "screen": "assets" }));
    let st: Value = serde_json::from_str(&hook(&root, "stop", &json!({ "cwd": root }))).unwrap();
    assert_eq!(st["decision"], "block");
    assert!(st["reason"].as_str().unwrap().contains("danger variant"));
    add(json!({ "comment": "x" }));
    let looping = json!({ "cwd": root, "stop_hook_active": true });
    assert_eq!(hook(&root, "stop", &looping).trim(), "", "never loops");
}

/// presence: written only for an edit the gate allows (tests/unit/studio-presence.test.mjs)
#[test]
fn presence_written_only_for_an_edit_the_gate_allows() {
    let root = fixture_project("presence");
    fs::create_dir_all(root.join(".dfw/reviews")).unwrap();
    fs::write(
        root.join(".dfw/state.json"),
        json!({ "mode": "new-product", "phase": "concepts", "active": true, "design": { "version": 1, "status": "draft", "locked": false, "approved": [] }, "gates": {} }).to_string(),
    )
    .unwrap();
    let file = root.join("design/screens/demo/a.html");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, "<main data-dfw-id=\"demo.main\">\n  <h1 data-dfw-id=\"demo.title\">Hello</h1>\n  <p>Plain line</p>\n  <dfw-use c=\"button\" id=\"demo.cta\">Go</dfw-use>\n  <span>After</span>\n</main>\n").unwrap();
    let note = root.join(".dfw/presence.json");
    let edit = |f: &Path| {
        let mut cmd = dfw_command(&root, &["hook", "pre-tool-use"]);
        cmd.env("DFW_PROJECT_DIR", &root);
        let input = json!({ "cwd": root, "tool_name": "Edit", "tool_input": { "file_path": f, "old_string": "Hello", "new_string": "Hi" } });
        run(cmd, &input.to_string()).ok()
    };
    assert!(
        edit(&root.join("src/app.ts")).contains("\"permissionDecision\":\"deny\""),
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
    fs::create_dir_all(root.join(".dfw/reviews")).unwrap();
    fs::write(
        root.join(".dfw/state.json"),
        r#"{"design":{"version":1,"status":"draft","locked":false}}"#,
    )
    .unwrap();
    cli(
        &root,
        &["comments", "add", "--json", r#"{"comment":"first"}"#],
    )
    .ok();
    let root_arg = path_str(&root);
    // Three writer processes at once, 40 comments each, as the Studio server and the CLI do.
    let writers: Vec<_> = ["a", "b", "c"]
        .iter()
        .map(|tag| {
            let args = ["__debug", "hot", "add-comments", &root_arg, tag, "40"];
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
    let all = cli(&root, &["comments", "list", "--json"]).json();
    let all = all.as_array().unwrap();
    assert_eq!(all.len(), 121);
    let ids: std::collections::HashSet<&str> =
        all.iter().map(|c| c["id"].as_str().unwrap()).collect();
    assert_eq!(ids.len(), 121);
    assert!(!root.join(".dfw/reviews/.lock").exists());
}

/// language: asked once per session, last used remembered (tests/unit/dfw.test.mjs)
#[test]
fn language_asked_once_per_session_last_used_remembered() {
    let root = tmp_dir("lang");
    let st = cli(&root, &["lang", "status", "--session", "abc"]).json();
    assert_eq!(st["chosen"], false);
    assert_eq!(st["lastUsed"], "en");
    cli(&root, &["lang", "set", "vi", "--session", "abc"]).ok();
    assert_eq!(
        cli(&root, &["lang", "status", "--session", "abc"]).json()["chosen"],
        true
    );
    let other = cli(&root, &["lang", "status", "--session", "def"]).json();
    assert_eq!(other["chosen"], false, "new session asks again");
    assert_eq!(other["lastUsed"], "vi", "offers last used as default");
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
    ok(&["init", "--product", "Test"]);
    assert!(root.join("PRODUCT.md").exists());
    assert!(root.join("design/tokens/tokens.css").exists());
    fails(&["phase", "concepts"], "G1");
    ok(&["phase", "direction"]);
    ok(&["approve", "G1", "--note", "Direction A"]);
    fails(&["phase", "concepts"], "DS");
    ok(&["phase", "system"]);
    fails(&["approve", "DS", "--note", "x"], "needs a design system");
    fails(
        &["system", "init", "--only", "button"],
        "--only was removed",
    );
    assert!(ok(&["system", "init", "--preset", "industrial"]).contains("No components were copied"));
    assert!(ok(&["system", "ref"]).contains("button"));
    fs::write(root.join("design/system/components/button.html"), "<dfw-component name=\"button\" variants=\"primary\"><style>.b { min-height: var(--size-control); color: var(--color-on-accent); background: var(--color-accent); }\n.b:focus-visible { outline: var(--focus-width) solid var(--color-focus); }</style><template><button class=\"b\" type=\"button\"><dfw-slot>Go</dfw-slot></button></template><dfw-examples><dfw-use c=\"button\">Issue tool</dfw-use></dfw-examples></dfw-component>").unwrap();
    fails(&["approve", "DS", "--note", "x"], "inventory");
    fs::write(
        root.join("design/system/inventory.json"),
        json!({ "components": [{ "name": "button", "purpose": "Confirm the issue at the counter", "usedBy": ["counter"] }] }).to_string(),
    )
    .unwrap();
    assert!(ok(&["system", "inventory"]).contains("Inventory and masters agree"));
    fails(&["approve", "DS", "--note", "x"], "Rationale");
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
    let design_md = fs::read_to_string(root.join("DESIGN.md")).unwrap();
    fs::write(
        root.join("DESIGN.md"),
        format!("{design_md}\n## Rationale\n{rationale}"),
    )
    .unwrap();
    ok(&["approve", "DS", "--note", "Industrial preset, button"]);
    ok(&["phase", "concepts"]);
    let screen = |img: &str| {
        format!("<!doctype html><html lang=\"en\"><head><link rel=\"stylesheet\" href=\"../../tokens/tokens.css\"></head><body>{img}</body></html>")
    };
    fs::create_dir_all(root.join("design/screens/home")).unwrap();
    fs::write(
        root.join("design/screens/home/a.html"),
        screen("<img src=\"x.png\" data-dfw-id=\"hero.img\">"),
    )
    .unwrap();
    let manifest = root.join("design/design.json");
    let mut m: Value = serde_json::from_str(&fs::read_to_string(&manifest).unwrap()).unwrap();
    m["screens"] =
        json!([{ "id": "home", "variants": [{ "id": "a", "file": "screens/home/a.html" }] }]);
    fs::write(&manifest, m.to_string()).unwrap();
    fails(&["approve", "G2", "--note", "v1"], "MUST finding");
    fs::write(
        root.join("design/screens/home/a.html"),
        screen("<img src=\"x.png\" alt=\"\" data-dfw-id=\"hero.img\">"),
    )
    .unwrap();
    // Neither axe nor the Impeccable engine is downloaded in these empty homes: the freeze needs
    // the user's acceptance (recorded), and both are reported as not verified.
    fails(&["approve", "G2", "--note", "v1"], "--accept-unverified");
    let frozen = ok(&["approve", "G2", "--note", "v1", "--accept-unverified"]);
    assert!(frozen.contains("Design frozen as v1"), "{frozen}");
    assert!(
        frozen.contains("NOT VERIFIED: impeccable (Engine not downloaded yet")
            && frozen.contains("; axe (axe-core not downloaded"),
        "{frozen}"
    );
    fails(&["phase", "implement"], "G3");
    ok(&["stack", "set", "--json", r#"{"framework":{"name":"Vue"}}"#]);
    ok(&["approve", "G3", "--note", "Vue 3 + plain CSS"]);
    ok(&["phase", "implement"]);
    assert!(ok(&["status"]).contains("phase=implement"));
    assert!(ok(&["handoff"]).contains("design/handoff/v1.md"));
    let decisions = ok(&["decisions"]);
    assert!(decisions.contains("gate:G1"));
    assert!(decisions.contains("gate:G3"));
    assert!(decisions.contains("gate:G2:accepted-unverified"));
    // The snapshot the Node side froze is intact for the native check.
    assert_eq!(ok(&["verify-snapshot"]), "Snapshot v1 intact.\n");
}
