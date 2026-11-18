//! Ports `core/lib/mcp.mjs`: the MCP server on stdio and its installation into other tools.
//!
//! "dfw mcp" is dfw as a Model Context Protocol server (stdio, JSON-RPC 2.0), for AI coding tools
//! without a dfw plugin (OpenCode, Codex, Cursor...). It gives them the workflow text and the CLI.
//! MCP cannot intercept the tool's own file writes, so there is no hard gate here: the
//! instructions ask the model to call `dfw_can_write` before editing.
//! `dfw mcp install|uninstall` is in `tools/mcp_install`.

use std::env;
use std::fs;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{find_project_root, plugin_root, resolve};
use crate::core::util::{plugin_manifest, to_json_compact};
use crate::workflow::gates::decide_write;
use crate::workflow::state::load_state;

/// Rendered into `mcp/` at build time (host.mjs "mcp").
const SKILL_DIR_MARK: &str = "__DFW_SKILL_DIR__";
/// Commands a model may not run through the `dfw` tool.
const BLOCKED: [&str; 6] = [
    "hook",
    "mcp",
    "__studio-server",
    "install",
    "uninstall",
    "setup",
];
const TOOL_TIMEOUT: Duration = Duration::from_secs(10 * 60);

const INSTRUCTIONS: &str = "dfw is a design-first frontend workflow: design, design system and approval gates come before UI code. \
For UI or design work, first call dfw_guide (no topic) and follow it; it lists the reference topics to load per phase. \
Run dfw commands with the dfw tool (args as a list, cwd = the project root). \
This tool has no write hook: before creating or editing any file in a dfw project, call dfw_can_write and never write a DENIED file. \
Only run \"dfw approve\" after the user explicitly approved in the chat.";

/* ---------------------------------------------------------------- the three tools */

/// What a tool call answers: the text and whether it is an error for the model.
struct ToolResult {
    text: String,
    is_error: bool,
}

/// A failed tool call (`throw new Error(message)` inside a tool): reported to the model as text.
type ToolError = String;

/// The `mcp/` text tree built for this plugin (dev checkout: the built Claude Code plugin's copy).
fn guide_dir() -> Option<PathBuf> {
    let root = plugin_root()?;
    let own = root.join("mcp");
    Some(if own.exists() {
        own
    } else {
        resolve(root.join("..").join("plugins").join("dfw").join("mcp"))
    })
}

fn topics() -> Vec<String> {
    let mut out = Vec::new();
    let Some(dir) = guide_dir() else {
        return out;
    };
    for sub in ["references", "policies", "stacks"] {
        let Ok(entries) = fs::read_dir(dir.join(sub)) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.ends_with(".md"))
            .collect();
        // JavaScript's default sort compares UTF-16 code units.
        names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        out.extend(names.into_iter().map(|name| format!("{sub}/{name}")));
    }
    out
}

/// `guide(topic)`: the main guide, or one topic file. The path must stay inside the guide folder.
fn guide(topic: Option<&Value>) -> Result<String, ToolError> {
    // The parameter default applies to `undefined` only: a null topic prints as "null".
    let shown = topic.map_or(String::new(), |t| js::to_string(Some(t)));
    let unknown = || format!("Unknown topic \"{shown}\". Topics: {}", topics().join(", "));
    let dir = guide_dir().ok_or_else(unknown)?;
    let rel = if js::truthy_value(topic).is_some() {
        shown.trim_start_matches('/')
    } else {
        "SKILL.md"
    };
    let file = resolve(dir.join(rel));
    let inside = file != dir && file.starts_with(&dir);
    if !inside || !file.to_string_lossy().ends_with(".md") || !file.exists() {
        return Err(unknown());
    }
    let text = fs::read_to_string(&file).map_err(|err| err.to_string())?;
    Ok(text.replace(SKILL_DIR_MARK, &dir.to_string_lossy()))
}

/// A `cwd` argument: the path given, else this process's working directory. Node passes a
/// truthy value that is not a string on and fails with a `TypeError`; the wording is its own.
fn base_dir(cwd: Option<&Value>, what: &str) -> Result<PathBuf, ToolError> {
    match js::truthy_value(cwd) {
        None => Ok(env::current_dir().unwrap_or_default()),
        Some(Value::String(dir)) => Ok(PathBuf::from(dir)),
        Some(_) => Err(format!("The \"{what}\" argument must be of type string.")),
    }
}

/// Everything a pipe of the child delivers, appended to the shared output as it arrives.
fn collect(
    pipe: Option<impl Read + Send + 'static>,
    output: &Arc<Mutex<Vec<u8>>>,
) -> thread::JoinHandle<()> {
    let output = Arc::clone(output);
    thread::spawn(move || {
        let Some(mut pipe) = pipe else { return };
        let mut chunk = [0u8; 8192];
        while let Ok(n) = pipe.read(&mut chunk) {
            if n == 0 {
                break;
            }
            if let Ok(mut all) = output.lock() {
                all.extend_from_slice(&chunk[..n]);
            }
        }
    })
}

/// `runDfw(args, cwd)`: run this binary with the given arguments and return what it printed
/// (stdout and stderr in the order they arrived).
fn run_dfw(args: Option<&Value>, cwd: Option<&Value>) -> Result<ToolResult, ToolError> {
    let list: Option<Vec<&str>> = args
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .and_then(|items| items.iter().map(Value::as_str).collect());
    let Some(list) = list else {
        return Err("\"args\" must be a non-empty list of strings, e.g. [\"status\"].".into());
    };
    if BLOCKED.contains(&list[0]) {
        return Err(format!(
            "\"dfw {}\" is not available through MCP; ask the user to run it in a terminal.",
            list[0]
        ));
    }
    let dir = base_dir(cwd, "options.cwd")?;
    let exe = env::current_exe().map_err(|err| err.to_string())?;
    // Node's spawn gives the child a stdin pipe and never closes it: kept, so a command that
    // reads stdin waits for the timeout here as it does there.
    let spawned = Command::new(exe)
        .args(&list)
        .current_dir(dir)
        .env("DFW_HOST", "mcp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        // The 'error' event of Node: its message is the output, and the call is an error.
        Err(err) => {
            return Ok(ToolResult {
                text: err.to_string(),
                is_error: true,
            })
        }
    };
    let output = Arc::new(Mutex::new(Vec::<u8>::new()));
    let readers = [
        collect(child.stdout.take(), &output),
        collect(child.stderr.take(), &output),
    ];
    let started = Instant::now();
    let mut pause = Duration::from_millis(1);
    let mut killed = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {}
            Err(_) => break None,
        }
        if !killed && started.elapsed() > TOOL_TIMEOUT {
            let _ = child.kill();
            killed = true;
        }
        thread::sleep(pause);
        pause = (pause * 2).min(Duration::from_millis(50));
    };
    for reader in readers {
        let _ = reader.join();
    }
    let code = status.and_then(|s| s.code());
    let all = output.lock().map(|all| all.clone()).unwrap_or_default();
    let printed = String::from_utf8_lossy(&all);
    let text = match js::trim(&printed) {
        "" => format!(
            "(exit {})",
            code.map_or("null".to_string(), |c| c.to_string())
        ),
        printed => printed.to_string(),
    };
    Ok(ToolResult {
        text,
        is_error: code != Some(0),
    })
}

/// `canWrite(files, cwd)`: what the write gate says about each file.
fn can_write(files: Option<&Value>, cwd: Option<&Value>) -> Result<ToolResult, ToolError> {
    let Some(files) = files.and_then(Value::as_array).filter(|f| !f.is_empty()) else {
        return Err("\"files\" must be a non-empty list of paths.".into());
    };
    let base = resolve(base_dir(cwd, "paths[0]")?);
    let root = find_project_root(Some(&base));
    let failed = |err: DfwError| err.message().to_string();
    let Some(state) = load_state(&root).map_err(failed)? else {
        return Ok(ToolResult {
            text: "No dfw workflow in this project: writes are not gated.".into(),
            is_error: false,
        });
    };
    let config = load_config(Some(&root)).map_err(failed)?;
    let mut lines = Vec::new();
    let mut denied = false;
    for file in files {
        let Some(f) = file.as_str() else {
            return Err("The \"paths[1]\" argument must be of type string.".into());
        };
        let abs = resolve(base.join(f));
        let decision = decide_write(&root, Some(&state), &config, &abs).map_err(failed)?;
        if decision.allow {
            lines.push(format!("ALLOWED {f}"));
        } else {
            denied = true;
            lines.push(format!(
                "DENIED  {f}: {}",
                decision.reason.unwrap_or_default()
            ));
        }
    }
    Ok(ToolResult {
        text: lines.join("\n"),
        is_error: denied,
    })
}

fn tools() -> Value {
    json!([
        {
            "name": "dfw_guide",
            "description": format!("The dfw workflow instructions. No topic: the main guide (read it first). Topics: {}.", topics().join(", ")),
            "inputSchema": { "type": "object", "properties": { "topic": { "type": "string", "description": "Optional, e.g. \"references/phase-concepts.md\"." } } },
        },
        {
            "name": "dfw",
            "description": "Run a dfw CLI command, e.g. [\"status\"], [\"init\",\"--mode\",\"new-product\"], [\"shoot\",\"--page\",\"borrow\",\"--states\"]. [\"help\"] lists every command.",
            "inputSchema": {
                "type": "object",
                "properties": { "args": { "type": "array", "items": { "type": "string" } }, "cwd": { "type": "string", "description": "Project root (absolute path)." } },
                "required": ["args"],
            },
        },
        {
            "name": "dfw_can_write",
            "description": "Check the approval gates before writing files (design files are open in design phases; implementation code needs G2 + G3 + the implement phase).",
            "inputSchema": {
                "type": "object",
                "properties": { "files": { "type": "array", "items": { "type": "string" } }, "cwd": { "type": "string", "description": "Project root (absolute path)." } },
                "required": ["files"],
            },
        },
    ])
}

/* ---------------------------------------------------------------- JSON-RPC */

/// A JSON-RPC error reply (`throw Object.assign(new Error(message), { code })`).
#[derive(Debug)]
struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn invalid_params(message: String) -> Self {
        RpcError {
            code: -32602,
            message,
        }
    }
}

/// `params.<key>`: `params` defaults to `{}` when the request has none; reading a property of
/// `null` is the `TypeError` JavaScript throws (answered as an internal error).
fn param<'a>(params: Option<&'a Value>, key: &str) -> Result<Option<&'a Value>, RpcError> {
    match params {
        None => Ok(None),
        Some(Value::Null) => Err(RpcError {
            code: -32603,
            message: format!("Cannot read properties of null (reading '{key}')"),
        }),
        Some(value) => Ok(value.get(key)),
    }
}

/// `handle(method, params)`: handle one JSON-RPC request; returns the result.
fn handle(method: Option<&Value>, params: Option<&Value>) -> Result<Value, RpcError> {
    match method.and_then(Value::as_str) {
        Some("initialize") => {
            let protocol = js::truthy_value(param(params, "protocolVersion")?)
                .cloned()
                .unwrap_or_else(|| json!("2025-06-18"));
            let manifest = plugin_manifest(None).unwrap_or(Value::Null);
            let version = js::truthy_value(manifest.get("version"))
                .cloned()
                .unwrap_or_else(|| json!("0.0.0"));
            Ok(json!({
                "protocolVersion": protocol,
                "capabilities": { "tools": {}, "prompts": {} },
                "serverInfo": { "name": "dfw", "version": version },
                "instructions": INSTRUCTIONS,
            }))
        }
        Some("ping") => Ok(json!({})),
        Some("tools/list") => Ok(json!({ "tools": tools() })),
        Some("tools/call") => {
            let a = js::truthy_value(param(params, "arguments")?);
            let arg = |key: &str| a.and_then(|a| a.get(key));
            let name = param(params, "name")?;
            let result = match name.and_then(Value::as_str) {
                Some("dfw_guide") => guide(arg("topic")).map(|text| ToolResult {
                    text,
                    is_error: false,
                }),
                Some("dfw") => run_dfw(arg("args"), arg("cwd")),
                Some("dfw_can_write") => can_write(arg("files"), arg("cwd")),
                _ => {
                    return Err(RpcError::invalid_params(format!(
                        "Unknown tool {}",
                        js::to_string(name)
                    )))
                }
            };
            let (text, is_error) = match result {
                Ok(r) => (r.text, r.is_error),
                Err(message) => (message, true),
            };
            Ok(json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }))
        }
        Some("prompts/list") => Ok(json!({
            "prompts": [{ "name": "dfw", "description": "Start or continue the dfw design-first workflow in this project." }]
        })),
        Some("prompts/get") => {
            let name = param(params, "name")?;
            if name.and_then(Value::as_str) != Some("dfw") {
                return Err(RpcError::invalid_params(format!(
                    "Unknown prompt {}",
                    js::to_string(name)
                )));
            }
            let text = guide(None).map_err(|message| RpcError {
                code: -32603,
                message,
            })?;
            Ok(
                json!({ "messages": [{ "role": "user", "content": { "type": "text", "text": text } }] }),
            )
        }
        _ => Err(RpcError {
            code: -32601,
            message: format!("Method not found: {}", js::to_string(method)),
        }),
    }
}

/// The reply to one input line, or `None` when it needs none (blank line, notification).
fn reply_to(line: &str) -> Option<Value> {
    let line = js::trim(line);
    if line.is_empty() {
        return None;
    }
    let error = |id: Value, code: i64, message: &str| {
        Some(json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }))
    };
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return error(Value::Null, -32700, "Parse error");
    };
    // Valid JSON that is not a request object (null, a number, a batch array): answer, never crash.
    let Value::Object(msg) = msg else {
        return error(Value::Null, -32600, "Invalid Request");
    };
    let id = msg.get("id")?.clone(); // no id: a notification
    match handle(msg.get("method"), msg.get("params")) {
        Ok(result) => Some(json!({ "jsonrpc": "2.0", "id": id, "result": result })),
        Err(err) => error(id, err.code, &err.message),
    }
}

/// `serveMcp()`: serve MCP over stdio until stdin closes. A last line without its newline is
/// never answered, as in Node.
pub fn serve_mcp() -> Result<(), DfwError> {
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let mut line = Vec::new();
    loop {
        line.clear();
        if stdin.read_until(b'\n', &mut line)? == 0 || line.last() != Some(&b'\n') {
            return Ok(());
        }
        let Some(reply) = reply_to(&String::from_utf8_lossy(&line)) else {
            continue;
        };
        let sent = stdout
            .write_all((to_json_compact(&reply) + "\n").as_bytes())
            .and_then(|()| stdout.flush());
        if sent.is_err() {
            return Ok(()); // the client is gone
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// mcp server: null, a number or a batch array get an error reply; the next request is still served
    #[test]
    fn lines_that_are_not_requests_get_an_error_reply() {
        let code =
            |line: &str| reply_to(line).map(|r| (r["id"].clone(), r["error"]["code"].clone()));
        assert_eq!(code("null"), Some((Value::Null, json!(-32600))));
        assert_eq!(code("42"), Some((Value::Null, json!(-32600))));
        assert_eq!(
            code(r#"[{"jsonrpc":"2.0","id":1,"method":"ping"}]"#),
            Some((Value::Null, json!(-32600)))
        );
        assert_eq!(code("{broken"), Some((Value::Null, json!(-32700))));
        assert_eq!(code("   "), None);
        assert_eq!(
            code(r#"{"jsonrpc":"2.0","method":"ping"}"#),
            None,
            "a notification"
        );
        assert_eq!(
            to_json_compact(&reply_to(r#"{"jsonrpc":"2.0","id":2,"method":"ping"}"#).unwrap()),
            r#"{"jsonrpc":"2.0","id":2,"result":{}}"#
        );
        assert_eq!(
            to_json_compact(&reply_to(r#"{"id":"a","method":7}"#).unwrap()),
            r#"{"jsonrpc":"2.0","id":"a","error":{"code":-32601,"message":"Method not found: 7"}}"#
        );
        assert_eq!(
            to_json_compact(
                &reply_to(r#"{"id":null,"method":"tools/call","params":null}"#).unwrap()
            ),
            r#"{"jsonrpc":"2.0","id":null,"error":{"code":-32603,"message":"Cannot read properties of null (reading 'arguments')"}}"#
        );
        assert_eq!(
            code(r#"{"id":3,"method":"tools/call","params":{"name":"nope"}}"#),
            Some((json!(3), json!(-32602)))
        );
    }

    #[test]
    fn tool_arguments_are_checked() {
        assert!(run_dfw(Some(&json!([])), None).is_err());
        assert!(run_dfw(Some(&json!(["status", 1])), None).is_err());
        assert!(run_dfw(Some(&json!(["install"])), None)
            .is_err_and(|e| e.contains("not available through MCP")));
        assert!(can_write(Some(&json!([])), None).is_err());
        assert!(guide(Some(&json!("../../package.json")))
            .is_err_and(|e| e.starts_with("Unknown topic \"../../package.json\". Topics: ")));
    }
}
