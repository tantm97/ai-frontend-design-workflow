//! Tests of `dfw mcp` that need the built binary: the stdio server and the install command.
//! Ported from tests/unit/mcp.test.mjs and tests/unit/mcp-robust.test.mjs; each test names its
//! JavaScript original.

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
use layout::rel;

#[path = "../src/core/fixture.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the fixture names"
)]
mod fixture;
use fixture::{APP_COMPONENT, HTML_FILE};

#[path = "../src/core/word.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the command line")]
mod word;
use word::{command, flag, sub};

#[path = "../src/workflow/names.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the workflow names"
)]
mod names;

#[path = "../src/tools/mcp/names.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the MCP names")]
mod mcp_names;
use mcp_names::{key, tool as mcp_tool, SERVER};

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
        "DFW_HOST",
        "DFW_PROJECT_DIR",
        "CLAUDE_PROJECT_DIR",
        "DFW_NOW",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("HOME", dir)
        .env("USERPROFILE", dir)
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
    let out = run(dfw_command(dir, &[command::MCP]), &input);
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
        json!({ (key::NAME): name, "arguments": arguments }),
    )
}

/// mcp server: initialize, guide, CLI tool, write gate
#[test]
fn mcp_server_initialize_guide_cli_tool_write_gate() {
    let root = tmp_dir("server");
    // `dfw init` builds tokens through Node when there is no design/: the state is written here.
    fs::create_dir_all(root.join(rel::DFW)).unwrap();
    fs::write(
        root.join(rel::STATE),
        json!({ "schema": 1, "product": "", "mode": "new-product", "phase": names::phase::DISCOVERY, "active": true,
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
            tool(mcp_tool::GUIDE, json!({})),
            tool(
                mcp_tool::GUIDE,
                json!({ "topic": "references/phase-concepts.md" }),
            ),
            tool(mcp_tool::GUIDE, json!({ "topic": "../../package.json" })),
            tool(mcp_tool::CLI, json!({ "args": ["status"], "cwd": cwd })),
            tool(mcp_tool::CLI, json!({ "args": ["install"] })),
            tool(
                mcp_tool::CAN_WRITE,
                json!({ "files": [APP_COMPONENT, &rel::screen(HTML_FILE)], "cwd": cwd }),
            ),
            ("nope/method", json!({})),
        ],
    );
    let text = |i: usize| {
        replies[i][key::RESULT]["content"][0]["text"]
            .as_str()
            .unwrap()
    };
    let is_error = |i: usize| replies[i][key::RESULT]["isError"].as_bool().unwrap();

    assert_eq!(replies[0][key::RESULT]["serverInfo"][key::NAME], SERVER);
    assert!(replies[0][key::RESULT]["instructions"]
        .as_str()
        .unwrap()
        .contains(mcp_tool::CAN_WRITE));
    let names: Vec<&str> = replies[1][key::RESULT]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t[key::NAME].as_str().unwrap())
        .collect();
    assert_eq!(names, [mcp_tool::GUIDE, mcp_tool::CLI, mcp_tool::CAN_WRITE]);

    assert!(!is_error(2));
    assert!(
        text(2).contains(mcp_tool::CAN_WRITE),
        "guide rendered for MCP"
    );
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
        text(5).to_lowercase().contains(names::phase::DISCOVERY)
            || text(5).to_lowercase().contains("phase")
    );
    assert!(is_error(6));

    assert!(is_error(7));
    assert!(text(7).contains(&format!("DENIED  {APP_COMPONENT}")));
    assert!(text(7).contains(&format!("ALLOWED {}", rel::screen(HTML_FILE))));

    assert_eq!(replies[8]["error"]["code"], -32601);
}

/// mcp server: null, a number or a batch array get an error reply; the next request is still served
#[test]
fn mcp_server_answers_lines_that_are_not_requests() {
    let dir = tmp_dir("robust");
    let out = run(
        dfw_command(&dir, &[command::MCP]),
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
    /// The word for every tool `dfw mcp install` knows.
    const ALL_TOOLS: &str = "all";
    /// Cursor's MCP config and the key of its servers, in the home folder.
    const CURSOR_CONFIG: &str = ".cursor/mcp.json";
    const CURSOR_SERVERS: &str = "mcpServers";
    /// Codex's MCP config and the table of dfw in it.
    const CODEX_CONFIG: &str = ".codex/config.toml";
    const CODEX_ENTRY: &str = "[mcp_servers.dfw]";
    let home = tmp_dir("install");
    fs::create_dir_all(home.join(".cursor")).unwrap();
    fs::write(home.join(CURSOR_CONFIG), "{\"mcpServers\":{}}").unwrap();
    let mcp = |args: &[&str]| {
        let mut all = vec![command::MCP];
        all.extend_from_slice(args);
        run(dfw_command(&home, &all), "")
    };
    let read = |rel: &str| fs::read_to_string(home.join(rel)).unwrap();
    let parsed = |rel: &str| serde_json::from_str::<Value>(&read(rel)).unwrap();

    assert!(mcp(&[sub::INSTALL, ALL_TOOLS, flag::PRINT]).contains(CODEX_ENTRY));
    assert!(!home.join(CODEX_CONFIG).exists(), "--print writes nothing");
    mcp(&[sub::INSTALL, ALL_TOOLS]);
    assert!(parsed(".config/opencode/opencode.json")["mcp"][SERVER].is_object());
    assert!(read(CODEX_CONFIG).contains(CODEX_ENTRY));
    assert!(parsed(CURSOR_CONFIG)[CURSOR_SERVERS][SERVER].is_object());
    assert!(home.join(".cursor/mcp.json.dfw-backup").exists());
    mcp(&[sub::UNINSTALL, "cursor"]);
    assert_eq!(parsed(CURSOR_CONFIG)[CURSOR_SERVERS].get(SERVER), None);
}
