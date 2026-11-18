//! Tests of `dfw mcp` that need the built binary: the stdio server and the install command.
//! Ported from tests/unit/mcp.test.mjs and tests/unit/mcp-robust.test.mjs; each test names its
//! JavaScript original.

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
        "dfw-native-misc-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// `dfw <args>` in `dir` with a home of its own and nothing of the session that runs the tests.
fn dfw_command(dir: &Path, args: &[&str]) -> Command {
    let mut cmd = Command::new(DFW);
    cmd.args(args).current_dir(dir);
    for name in [
        "DFW_NATIVE",
        "DFW_HOST",
        "DFW_PROJECT_DIR",
        "CLAUDE_PROJECT_DIR",
        "DFW_NOW",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("HOME", dir)
        .env("XDG_CONFIG_HOME", dir.join(".config"))
        .env("CODEX_HOME", dir.join(".codex"))
        .env("DFW_HOME", dir.join(".home"))
        .env("DFW_PLUGIN_ROOT", repo().join("core"));
    cmd
}

/// Run to the end with `stdin`; stdout of a run that must have succeeded.
fn run(mut cmd: Command, stdin: &str) -> String {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// Send the requests (ids 1, 2, ...) to `dfw mcp` and return the replies in order.
fn session(dir: &Path, requests: &[(&str, Value)]) -> Vec<Value> {
    let mut input = String::new();
    for (i, (method, params)) in requests.iter().enumerate() {
        let line = json!({ "jsonrpc": "2.0", "id": i + 1, "method": method, "params": params });
        input.push_str(&line.to_string());
        input.push('\n');
        if i == 0 {
            // A notification: no reply expected.
            input.push_str("{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n");
        }
    }
    let out = run(dfw_command(dir, &["mcp"]), &input);
    let replies: Vec<Value> = out
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(replies.len(), requests.len());
    for (i, reply) in replies.iter().enumerate() {
        assert_eq!(reply["id"], json!(i + 1));
    }
    replies
}

fn tool(name: &str, arguments: Value) -> (&'static str, Value) {
    (
        "tools/call",
        json!({ "name": name, "arguments": arguments }),
    )
}

/// mcp server: initialize, guide, CLI tool, write gate
#[test]
fn mcp_server_initialize_guide_cli_tool_write_gate() {
    let root = tmp_dir("server");
    // `dfw init` builds tokens through Node when there is no design/: the state is written here.
    fs::create_dir_all(root.join(".dfw")).unwrap();
    fs::write(
        root.join(".dfw/state.json"),
        json!({ "schema": 1, "product": "", "mode": "new-product", "phase": "discovery", "active": true,
            "design": { "version": 1, "status": "draft", "locked": false, "approved": [] },
            "gates": { "G1": null, "DS": null, "G2": null, "G3": null, "G4": null }, "waived": {}, "history": [] })
        .to_string(),
    )
    .unwrap();
    let cwd = root.to_string_lossy().into_owned();
    let replies = session(
        &root,
        &[
            (
                "initialize",
                json!({ "protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": { "name": "t", "version": "1" } }),
            ),
            ("tools/list", json!({})),
            tool("dfw_guide", json!({})),
            tool(
                "dfw_guide",
                json!({ "topic": "references/phase-concepts.md" }),
            ),
            tool("dfw_guide", json!({ "topic": "../../package.json" })),
            tool("dfw", json!({ "args": ["status"], "cwd": cwd })),
            tool("dfw", json!({ "args": ["install"] })),
            tool(
                "dfw_can_write",
                json!({ "files": ["src/App.tsx", "design/screens/a.html"], "cwd": cwd }),
            ),
            ("nope/method", json!({})),
        ],
    );
    let text = |i: usize| replies[i]["result"]["content"][0]["text"].as_str().unwrap();
    let is_error = |i: usize| replies[i]["result"]["isError"].as_bool().unwrap();

    assert_eq!(replies[0]["result"]["serverInfo"]["name"], "dfw");
    assert!(replies[0]["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("dfw_can_write"));
    let names: Vec<&str> = replies[1]["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["dfw_guide", "dfw", "dfw_can_write"]);

    assert!(!is_error(2));
    assert!(text(2).contains("dfw_can_write"), "guide rendered for MCP");
    for leftover in [
        "__DFW_SKILL_DIR__",
        "CLAUDE_SKILL_DIR",
        "AskUserQuestion",
        "{{ASK_TOOL}}",
        "{{SKILL_DIR}}",
    ] {
        assert!(!text(2).contains(leftover), "{leftover}");
    }
    assert!(text(3).contains("Phase: concepts"));
    assert!(is_error(4));

    assert!(!is_error(5), "{}", text(5));
    assert!(
        text(5).to_lowercase().contains("discovery") || text(5).to_lowercase().contains("phase")
    );
    assert!(is_error(6));

    assert!(is_error(7));
    assert!(text(7).contains("DENIED  src/App.tsx"));
    assert!(text(7).contains("ALLOWED design/screens/a.html"));

    assert_eq!(replies[8]["error"]["code"], -32601);
}

/// mcp server: null, a number or a batch array get an error reply; the next request is still served
#[test]
fn mcp_server_answers_lines_that_are_not_requests() {
    let dir = tmp_dir("robust");
    let out = run(
        dfw_command(&dir, &["mcp"]),
        "null\n42\n[{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"ping\"}]\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"ping\"}\n",
    );
    let replies: Vec<(Value, Value)> = out
        .lines()
        .map(|l| serde_json::from_str::<Value>(l).unwrap())
        .map(|m| {
            let outcome = m.get("error").map_or(json!("ok"), |e| e["code"].clone());
            (m["id"].clone(), outcome)
        })
        .collect();
    assert_eq!(
        replies,
        [
            (Value::Null, json!(-32600)),
            (Value::Null, json!(-32600)),
            (Value::Null, json!(-32600)),
            (json!(2), json!("ok"))
        ]
    );
}

/// mcp install CLI writes each tool config and backs up an existing one
#[test]
fn mcp_install_cli_writes_each_tool_config_and_backs_up() {
    let home = tmp_dir("install");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(home.join(".cursor/mcp.json"), "{\"mcpServers\":{}}").unwrap();
    let mcp = |args: &[&str]| {
        let mut all = vec!["mcp"];
        all.extend_from_slice(args);
        run(dfw_command(&home, &all), "")
    };
    let read = |rel: &str| fs::read_to_string(home.join(rel)).unwrap();
    let parsed = |rel: &str| serde_json::from_str::<Value>(&read(rel)).unwrap();

    assert!(mcp(&["install", "all", "--print"]).contains("[mcp_servers.dfw]"));
    assert!(
        !home.join(".codex/config.toml").exists(),
        "--print writes nothing"
    );
    mcp(&["install", "all"]);
    assert!(parsed(".config/opencode/opencode.json")["mcp"]["dfw"].is_object());
    assert!(read(".codex/config.toml").contains("[mcp_servers.dfw]"));
    assert!(parsed(".cursor/mcp.json")["mcpServers"]["dfw"].is_object());
    assert!(home.join(".cursor/mcp.json.dfw-backup").exists());
    mcp(&["uninstall", "cursor"]);
    assert_eq!(parsed(".cursor/mcp.json")["mcpServers"].get("dfw"), None);
}
