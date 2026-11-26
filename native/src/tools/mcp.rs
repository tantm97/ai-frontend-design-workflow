//! Ports `core/lib/mcp.mjs`: the MCP server on stdio and its installation into other tools.
//!
//! "dfw mcp" is dfw as a Model Context Protocol server (stdio, JSON-RPC 2.0), for AI coding tools
//! without a dfw plugin (OpenCode, Codex, Cursor...). It gives them the workflow text and the CLI.
//! MCP cannot intercept the tool's own file writes, so there is no hard gate here: the
//! instructions ask the model to call `dfw_can_write` before editing.
//! `dfw mcp install|uninstall` is in `tools/mcp_install`.

use std::env;
use std::io::{self, BufRead, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::assets::{self, Folder};
use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::host::render_text;
use crate::core::js;
use crate::core::paths::{find_project_root, resolve, MANIFEST_VERSION};
use crate::core::util::{parse_args, plugin_manifest, to_json_compact};
use crate::workflow::gates::decide_write;
use crate::workflow::state::load_state;

pub(crate) mod names;
use names::{arg, key, tool, SERVER};

/// The value of `{{SKILL_DIR}}` for the host "mcp" (`core::host`).
const SKILL_DIR_MARK: &str = "__DFW_SKILL_DIR__";
/// Commands a model may not run through the `dfw` tool, besides every internal `__*` command
/// (the Studio and review servers, `__debug`) and the forms that write outside the project:
/// see [`blocked`].
/// The CLI's words [`blocked`] looks at: commands, and the subcommands that decide.
mod word {
    pub use crate::core::word::command::{
        CONFIG, HOOK, INSTALL, LANG, MCP, SETUP, STATUS, TELEMETRY, UNINSTALL,
    };
    pub use crate::core::word::sub::{SET, SHOW};
    /// The key `--global` is read by.
    pub const GLOBAL_FLAG: &str = "global";
    /// The form a `config` command with `--global` is refused as.
    pub const GLOBAL_CONFIG: &str = "config --global";
}

const BLOCKED: [&str; 5] = [
    word::HOOK,
    word::MCP,
    word::INSTALL,
    word::UNINSTALL,
    word::SETUP,
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

/// The parts of `core/workflow` the guide serves (the agents are for plugin hosts only).
const GUIDE_FOLDERS: [&str; 3] = ["references", "policies", "stacks"];

fn topics() -> Vec<String> {
    let mut out = Vec::new();
    for sub in GUIDE_FOLDERS {
        let mut names: Vec<String> = assets::file_names(Folder::Workflow, sub)
            .into_iter()
            .filter(|name| name.ends_with(".md"))
            .collect();
        // JavaScript's default sort compares UTF-16 code units.
        names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
        out.extend(names.into_iter().map(|name| format!("{sub}/{name}")));
    }
    out
}

/// A topic as a path inside the guide: `..` is resolved as `path.resolve` does; `None` when the
/// topic leaves the guide or names the guide itself.
fn topic_path(rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for part in rel.split(['/', '\\']) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            _ => parts.push(part),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// `guide(topic)`: the main guide, or one topic file, rendered for MCP from the embedded
/// `core/workflow`. The path must stay inside the guide. The guide has no folder on disk, so the
/// skill folder placeholder becomes nothing: `{{SKILL_DIR}}/references/x.md` reads
/// `references/x.md`, which is the topic to ask for.
fn guide(topic: Option<&Value>) -> Result<String, ToolError> {
    // The parameter default applies to `undefined` only: a null topic prints as "null".
    let shown = topic.map_or(String::new(), |t| js::to_string(Some(t)));
    let unknown = || format!("Unknown topic \"{shown}\". Topics: {}", topics().join(", "));
    let rel = if js::truthy_value(topic).is_some() {
        topic_path(&shown).ok_or_else(unknown)?
    } else {
        "SKILL.md".to_string()
    };
    let served = rel == "SKILL.md"
        || GUIDE_FOLDERS
            .iter()
            .any(|sub| rel.starts_with(&format!("{sub}/")));
    if !served || !rel.ends_with(".md") {
        return Err(unknown());
    }
    let text = assets::text(Folder::Workflow, &rel).ok_or_else(unknown)?;
    let text = render_text(&text, "mcp").map_err(|err| err.message().to_string())?;
    Ok(text
        .replace(&format!("{SKILL_DIR_MARK}/"), "")
        .replace(SKILL_DIR_MARK, "."))
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

/// Everything a pipe of the child delivers, appended to the shared output as it arrives. An
/// error when the reading thread cannot start.
fn collect(
    pipe: Option<impl Read + Send + 'static>,
    output: &Arc<Mutex<Vec<u8>>>,
) -> std::io::Result<thread::JoinHandle<()>> {
    let output = Arc::clone(output);
    thread::Builder::new().spawn(move || {
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

/// The form the `dfw` tool refuses, named for the message; `None` when it may run. Refused: one
/// of [`BLOCKED`], an internal `__*` command (a server started by the CLI itself, never by a
/// model), and every form that writes the user's global config or reaches outside the project:
/// `telemetry` except `status` and `show`, `config ... --global` and `lang set`. The command is
/// found as the CLI finds it (the first word that is not a flag or a flag's value).
fn blocked(list: &[&str]) -> Option<String> {
    if let Some(first) = list.first().filter(|f| f.starts_with("__")) {
        return Some(first.to_string());
    }
    let args = parse_args(list);
    let cmd = args.pos(0).unwrap_or_default();
    let sub = args.pos(1);
    if BLOCKED.contains(&cmd) || cmd.starts_with("__") {
        return Some(cmd.to_string());
    }
    let refused = match cmd {
        word::TELEMETRY => !matches!(sub, None | Some(word::STATUS | word::SHOW)),
        word::CONFIG => args.get(word::GLOBAL_FLAG).is_some(),
        word::LANG => sub == Some(word::SET),
        _ => false,
    };
    refused.then(|| match (cmd, sub) {
        (word::CONFIG, _) => word::GLOBAL_CONFIG.to_string(),
        (_, Some(sub)) => format!("{cmd} {sub}"),
        _ => cmd.to_string(),
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
    if let Some(form) = blocked(&list) {
        return Err(format!(
            "\"dfw {form}\" is not available through MCP; ask the user to run it in a terminal."
        ));
    }
    let dir = base_dir(cwd, "options.cwd")?;
    let exe = env::current_exe().map_err(|err| err.to_string())?;
    // Node's spawn gives the child a stdin pipe and never closes it: kept, so a command that
    // reads stdin waits for the timeout here as it does there.
    #[allow(
        clippy::disallowed_methods,
        reason = "dfw's own executable with a stdin pipe kept open, timed by the caller below"
    )]
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
    let readers = collect(child.stdout.take(), &output)
        .and_then(|out| Ok([out, collect(child.stderr.take(), &output)?]));
    let readers = match readers {
        Ok(readers) => readers,
        // No thread to read the output with (the system is out of them): the child is ended,
        // never left to block on a full pipe, and the call is an error.
        Err(err) => {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(ToolResult {
                text: format!(
                    "dfw could not start a thread to read its output ({err}); try again."
                ),
                is_error: true,
            });
        }
    };
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
            (key::NAME): tool::GUIDE,
            (key::DESCRIPTION): format!("The dfw workflow instructions. No topic: the main guide (read it first). Topics: {}.", topics().join(", ")),
            (key::INPUT_SCHEMA): { (key::TYPE): "object", (key::PROPERTIES): { (arg::TOPIC): { (key::TYPE): "string", (key::DESCRIPTION): "Optional, e.g. \"references/phase-concepts.md\"." } } },
        },
        {
            (key::NAME): tool::CLI,
            (key::DESCRIPTION): "Run a dfw CLI command, e.g. [\"status\"], [\"init\",\"--mode\",\"new-product\"], [\"shoot\",\"--page\",\"borrow\",\"--states\"]. [\"help\"] lists every command.",
            (key::INPUT_SCHEMA): {
                (key::TYPE): "object",
                (key::PROPERTIES): { (arg::ARGS): { (key::TYPE): "array", (key::ITEMS): { (key::TYPE): "string" } }, (arg::CWD): { (key::TYPE): "string", (key::DESCRIPTION): "Project root (absolute path)." } },
                (key::REQUIRED): [arg::ARGS],
            },
        },
        {
            (key::NAME): tool::CAN_WRITE,
            (key::DESCRIPTION): "Check the approval gates before writing files (design files are open in design phases; implementation code needs G2 + G3 + the implement phase).",
            (key::INPUT_SCHEMA): {
                (key::TYPE): "object",
                (key::PROPERTIES): { (arg::FILES): { (key::TYPE): "array", (key::ITEMS): { (key::TYPE): "string" } }, (arg::CWD): { (key::TYPE): "string", (key::DESCRIPTION): "Project root (absolute path)." } },
                (key::REQUIRED): [arg::FILES],
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
            code: rpc_code::INVALID_PARAMS,
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
            code: rpc_code::INTERNAL_ERROR,
            message: format!("Cannot read properties of null (reading '{key}')"),
        }),
        Some(value) => Ok(value.get(key)),
    }
}

/// `handle(method, params)`: handle one JSON-RPC request; returns the result.
fn handle(method: Option<&Value>, params: Option<&Value>) -> Result<Value, RpcError> {
    match method.and_then(Value::as_str) {
        Some("initialize") => {
            let protocol = js::truthy_value(param(params, key::PROTOCOL_VERSION)?)
                .cloned()
                .unwrap_or_else(|| json!("2025-06-18"));
            let manifest = plugin_manifest(None).unwrap_or(Value::Null);
            let version = js::truthy_value(manifest.get(MANIFEST_VERSION))
                .cloned()
                .unwrap_or_else(|| json!("0.0.0"));
            Ok(json!({
                (key::PROTOCOL_VERSION): protocol,
                (key::CAPABILITIES): { (key::TOOLS): {}, (key::PROMPTS): {} },
                (key::SERVER_INFO): { (key::NAME): SERVER, (key::VERSION): version },
                (key::INSTRUCTIONS): INSTRUCTIONS,
            }))
        }
        Some("ping") => Ok(json!({})),
        Some("tools/list") => Ok(json!({ (key::TOOLS): tools() })),
        Some("tools/call") => {
            let a = js::truthy_value(param(params, key::ARGUMENTS)?);
            let arg = |key: &str| a.and_then(|a| a.get(key));
            let name = param(params, key::NAME)?;
            let result = match name.and_then(Value::as_str) {
                Some(tool::GUIDE) => guide(arg(arg::TOPIC)).map(|text| ToolResult {
                    text,
                    is_error: false,
                }),
                Some(tool::CLI) => run_dfw(arg(arg::ARGS), arg(arg::CWD)),
                Some(tool::CAN_WRITE) => can_write(arg(arg::FILES), arg(arg::CWD)),
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
            Ok(
                json!({ (key::CONTENT): [{ (key::TYPE): "text", (key::TEXT): text }], (key::IS_ERROR): is_error }),
            )
        }
        Some("prompts/list") => Ok(json!({
            (key::PROMPTS): [{ (key::NAME): SERVER, (key::DESCRIPTION): "Start or continue the dfw design-first workflow in this project." }]
        })),
        Some("prompts/get") => {
            let name = param(params, key::NAME)?;
            if name.and_then(Value::as_str) != Some(SERVER) {
                return Err(RpcError::invalid_params(format!(
                    "Unknown prompt {}",
                    js::to_string(name)
                )));
            }
            let text = guide(None).map_err(|message| RpcError {
                code: rpc_code::INTERNAL_ERROR,
                message,
            })?;
            Ok(
                json!({ (key::MESSAGES): [{ (key::ROLE): "user", (key::CONTENT): { (key::TYPE): "text", (key::TEXT): text } }] }),
            )
        }
        _ => Err(RpcError {
            code: rpc_code::METHOD_NOT_FOUND,
            message: format!("Method not found: {}", js::to_string(method)),
        }),
    }
}

/// JSON-RPC 2.0 error codes (the specification's names).
mod rpc_code {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

const JSONRPC: &str = "2.0";

/// A JSON-RPC error reply.
fn rpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({ (key::JSONRPC): JSONRPC, (key::ID): id, (key::ERROR): { (key::CODE): code, (key::MESSAGE): message } })
}

/// The reply to one input line, or `None` when it needs none (blank line, notification).
fn reply_to(line: &str) -> Option<Value> {
    let line = js::trim(line);
    if line.is_empty() {
        return None;
    }
    let Ok(msg) = serde_json::from_str::<Value>(line) else {
        return Some(rpc_error(Value::Null, rpc_code::PARSE_ERROR, "Parse error"));
    };
    // Valid JSON that is not a request object (null, a number, a batch array): answer, never crash.
    let Value::Object(msg) = msg else {
        return Some(rpc_error(
            Value::Null,
            rpc_code::INVALID_REQUEST,
            "Invalid Request",
        ));
    };
    let id = msg.get(key::ID)?.clone(); // no id: a notification
    match handle(msg.get(key::METHOD), msg.get(key::PARAMS)) {
        Ok(result) => {
            Some(json!({ (key::JSONRPC): JSONRPC, (key::ID): id, (key::RESULT): result }))
        }
        Err(err) => Some(rpc_error(id, err.code, &err.message)),
    }
}

/// `serveMcp()`: serve MCP over stdio until stdin closes. A last line without its newline is
/// never answered, as in Node.
pub fn serve_mcp() -> Result<(), DfwError> {
    serve_lines(&mut io::stdin().lock(), &mut io::stdout().lock())
}

/// The largest input line (one JSON-RPC message) the server reads. A longer one is answered
/// with an error and skipped without being held in memory; the next line is served.
pub const MAX_LINE: usize = 4 * 1024 * 1024;

/// What one read of an input line gave.
enum Line {
    End,
    TooLong,
    Text,
}

/// Read one line into `line`, at most [`MAX_LINE`] bytes of it; the rest of a longer line is
/// read and dropped.
fn read_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<Line> {
    line.clear();
    let limit = u64::try_from(MAX_LINE).unwrap_or(u64::MAX);
    input.by_ref().take(limit + 1).read_until(b'\n', line)?;
    if line.last() == Some(&b'\n') {
        return Ok(Line::Text);
    }
    if line.len() <= MAX_LINE {
        // Closed, or a last line without its newline (never answered, as in Node).
        return Ok(Line::End);
    }
    loop {
        let buf = input.fill_buf()?;
        if buf.is_empty() {
            return Ok(Line::TooLong);
        }
        match buf.iter().position(|b| *b == b'\n') {
            Some(at) => {
                input.consume(at + 1);
                return Ok(Line::TooLong);
            }
            None => {
                let n = buf.len();
                input.consume(n);
            }
        }
    }
}

/// The server loop over any input and output (stdio in [`serve_mcp`]).
fn serve_lines(input: &mut impl BufRead, output: &mut impl Write) -> Result<(), DfwError> {
    let mut line = Vec::new();
    loop {
        let reply = match read_line(input, &mut line)? {
            Line::End => return Ok(()),
            Line::TooLong => Some(rpc_error(
                Value::Null,
                rpc_code::INVALID_REQUEST,
                &format!(
                    "Request too large: a message may be at most {} MB.",
                    MAX_LINE / (1024 * 1024)
                ),
            )),
            Line::Text => reply_to(&String::from_utf8_lossy(&line)),
        };
        let Some(reply) = reply else {
            continue;
        };
        let sent = output
            .write_all((to_json_compact(&reply) + "\n").as_bytes())
            .and_then(|()| output.flush());
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
        for internal in ["__studio-server", "__review-server", "__debug"] {
            assert_eq!(
                run_dfw(Some(&json!([internal, "--port", "1"])), None).err(),
                Some(format!(
                    "\"dfw {internal}\" is not available through MCP; ask the user to run it in a terminal."
                )),
            );
        }
        assert!(can_write(Some(&json!([])), None).is_err());
        assert!(guide(Some(&json!("../../package.json")))
            .is_err_and(|e| e.starts_with("Unknown topic \"../../package.json\". Topics: ")));
        for refused in [
            "agents/screen-builder.md",
            "references",
            "/",
            "references/../../x.md",
        ] {
            assert!(guide(Some(&json!(refused))).is_err(), "{refused}");
        }
    }

    /// TR-2: every form that writes the user's global config or reaches outside the project is
    /// refused, wherever the command word stands; the project's own commands still run.
    #[test]
    fn forms_that_write_outside_the_project_are_refused() {
        use crate::core::word::{command::DEBUG, flag::GLOBAL, sub::GET};
        use word::{
            CONFIG, GLOBAL_CONFIG, HOOK, INSTALL, LANG, MCP, SET, SETUP, STATUS, TELEMETRY,
            UNINSTALL,
        };
        /// Where a page elsewhere would send the telemetry.
        const FOREIGN_ENDPOINT: &str = "https://evil.example/v1";
        use crate::core::word::sub::ENDPOINT;
        let form = |list: &[&str]| blocked(list);
        let lang_set = format!("{LANG} {SET}");
        let refused = [
            (&[INSTALL][..], INSTALL),
            (&["--root", ".", INSTALL, "--host", "claude"], INSTALL),
            (&[UNINSTALL], UNINSTALL),
            (&[SETUP, "--a11y"], SETUP),
            (&[SETUP, "--marketplace-repo", "/x"], SETUP),
            (&[MCP, INSTALL, "codex"], MCP),
            (&[HOOK, "stop"], HOOK),
            (&[TELEMETRY, "on"], "telemetry on"),
            (&[TELEMETRY, "off"], "telemetry off"),
            (
                &[TELEMETRY, ENDPOINT, FOREIGN_ENDPOINT],
                "telemetry endpoint",
            ),
            (&[TELEMETRY, "flush"], "telemetry flush"),
            (&[TELEMETRY, ""], "telemetry "),
            (
                &[
                    CONFIG,
                    SET,
                    "impeccable.allowUnverifiedEngine",
                    "true",
                    GLOBAL,
                ],
                GLOBAL_CONFIG,
            ),
            (
                &[CONFIG, SET, "env.PATH", "/x", GLOBAL, "yes"],
                GLOBAL_CONFIG,
            ),
            (&[CONFIG, SET, "a", "1", "--global="], GLOBAL_CONFIG),
            (&[CONFIG, GET, TELEMETRY, GLOBAL], GLOBAL_CONFIG),
            (&[LANG, SET, "vi"], &lang_set),
            (&[LANG, SET, "vi", "--session", "s1"], &lang_set),
            (&[DEBUG, "hot"], DEBUG),
        ];
        for (list, name) in refused {
            assert_eq!(form(list).as_deref(), Some(name), "{list:?}");
        }
        for allowed in [
            &[STATUS][..],
            &[TELEMETRY],
            &[TELEMETRY, STATUS],
            &[TELEMETRY, "show", "--raw"],
            &[CONFIG, GET, "gate"],
            &[CONFIG, SET, "checks.onEdit", "full"],
            &[CONFIG, SET, "a", "1", "--local"],
            &[LANG, STATUS],
            &[LANG],
        ] {
            assert_eq!(form(allowed), None, "{allowed:?}");
        }
        assert_eq!(
            run_dfw(Some(&json!([TELEMETRY, ENDPOINT, FOREIGN_ENDPOINT])), None).err(),
            Some("\"dfw telemetry endpoint\" is not available through MCP; ask the user to run it in a terminal.".to_string())
        );
    }

    /// mcp: the guide is served from the binary, rendered for MCP, with no folder on disk
    #[test]
    fn the_guide_is_embedded_and_rendered_for_mcp() {
        let main = guide(None).unwrap();
        assert!(main.contains("dfw_can_write"));
        assert!(!main.contains(SKILL_DIR_MARK) && !main.contains("{{"));
        assert!(
            main.contains("`references/`"),
            "topics are named as dfw_guide takes them"
        );
        let concepts = guide(Some(&json!("references/phase-concepts.md"))).unwrap();
        assert!(concepts.contains("Phase: concepts"));
        assert_eq!(
            guide(Some(&json!("/references/../references/phase-concepts.md"))).unwrap(),
            concepts
        );
        assert!(topics().contains(&"policies/precedence.md".to_string()));
    }

    /// A line over the limit gets a JSON-RPC error and is dropped; the next request is served
    /// (review TR-16: one endless line grew the memory without bound).
    #[test]
    fn a_line_over_the_limit_is_refused_and_the_server_goes_on() {
        let mut input = vec![b'x'; MAX_LINE + 10];
        input.extend_from_slice(b"\n{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"ping\"}\n");
        let mut output = Vec::new();
        serve_lines(&mut io::Cursor::new(input), &mut output).unwrap();
        let lines: Vec<Value> = String::from_utf8(output)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(lines.len(), 2, "{lines:?}");
        let refused = &lines[0]["error"];
        assert_eq!(refused["code"], -32600);
        assert!(refused["message"]
            .as_str()
            .unwrap()
            .contains("at most 4 MB"));
        assert_eq!(lines[1], json!({ "jsonrpc": "2.0", "id": 7, "result": {} }));
        // A line of exactly the limit (newline included) is still read.
        let mut input = vec![b' '; MAX_LINE - 1];
        input.push(b'\n');
        let mut output = Vec::new();
        serve_lines(&mut io::Cursor::new(input), &mut output).unwrap();
        assert!(output.is_empty(), "a blank line needs no reply");
    }
}
