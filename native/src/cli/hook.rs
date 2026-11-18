//! The `hook` command of `core/lib/cli.mjs`: reads the host's JSON on stdin, runs a handler of `hooks/`,
//! prints the host-specific result; fails open.

use std::io::{self, IsTerminal, Read, Write};
use std::process::{Command, Stdio};
use std::time::Instant;

use serde_json::{json, Value};

use super::{Ctx, Handler};
use crate::core::error::DfwError;
use crate::core::host::{current_host, format_hook_output, normalize_hook_input, HookResult};
use crate::core::paths::plugin_root;
use crate::core::util::to_json_compact;
use crate::hooks::post_tool_use::{post_tool_use, Outcome};
use crate::hooks::pre_tool_use::pre_tool_use;
use crate::hooks::session_start::session_start;
use crate::hooks::stop::stop;
use crate::hooks::user_prompt_submit::user_prompt_submit;
use crate::telemetry::events::HookOutcome;

/// The commands of this group. `session-start`, `pre-tool-use` and `user-prompt-submit` always
/// run here; `post-tool-use` and `stop` hand the call to Node when it needs the checks of the
/// design or browser group (see `hooks/post_tool_use.rs`, `hooks/stop.rs`).
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![("hook", Handler::Native(hook))]
}

/// Handler name and the host event its output is formatted for (`HANDLERS`, `HOOK_EVENTS`).
const HOOKS: [(&str, &str); 5] = [
    ("session-start", "SessionStart"),
    ("pre-tool-use", "PreToolUse"),
    ("post-tool-use", "PostToolUse"),
    ("user-prompt-submit", "UserPromptSubmit"),
    ("stop", "Stop"),
];

/// All of stdin as it arrived; nothing when stdin is a terminal.
fn read_stdin_bytes() -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut stdin = io::stdin();
    if !stdin.is_terminal() {
        // A stdin that cannot be read is an empty payload, as for Node.
        let _ = stdin.read_to_end(&mut bytes);
    }
    bytes
}

fn hook(ctx: &Ctx) -> Result<i32, DfwError> {
    let name = ctx.args.pos(1);
    let Some((name, event)) = HOOKS.iter().find(|(n, _)| Some(*n) == name) else {
        return Err(DfwError::new(format!(
            "Unknown hook \"{}\"",
            name.unwrap_or("undefined")
        )));
    };
    let started = Instant::now();
    let stdin = read_stdin_bytes();
    let text = String::from_utf8_lossy(&stdin);
    // A payload that is not JSON is an empty one.
    let payload: Value = if text.is_empty() {
        json!({})
    } else {
        serde_json::from_str(&text).unwrap_or_else(|_| json!({}))
    };
    let (kind, failed) = match run(ctx, name, event, &payload) {
        Ok((kind, Outcome::Done(output))) => {
            if let Some(output) = output {
                let mut stdout = io::stdout().lock();
                let _ = stdout
                    .write_all(to_json_compact(&output).as_bytes())
                    .and_then(|()| stdout.flush());
            }
            (kind, None)
        }
        Ok((kind, Outcome::Forward)) => (kind, forward_with_stdin(&ctx.argv, &stdin).err()),
        Err(err) => (HookOutcome::Error, Some(err)),
    };
    if let Some(err) = failed {
        // Fail open: never break the session because of a dfw bug.
        eprintln!("dfw hook {name} error: {}", err.message());
    }
    // After the host has its answer; telemetry cannot change it or the exit code.
    crate::telemetry::hook_finished(name, ctx.args.get("host"), started, kind);
    Ok(0)
}

/// Run the handler and put its result in the host's words: the JSON to print, or `Forward`;
/// with it, the kind of result for telemetry.
fn run(
    ctx: &Ctx,
    name: &str,
    event: &str,
    payload: &Value,
) -> Result<(HookOutcome, Outcome<Value>), DfwError> {
    let host = current_host(ctx.args.get("host"))?;
    let input = normalize_hook_input(payload)?;
    let result: Option<HookResult> = match name {
        "session-start" => session_start(&input)?,
        "pre-tool-use" => pre_tool_use(&input)?,
        "user-prompt-submit" => user_prompt_submit(&input, host)?,
        "post-tool-use" => match post_tool_use(&input, host)? {
            Outcome::Done(result) => result,
            Outcome::Forward => return Ok((HookOutcome::Forwarded, Outcome::Forward)),
        },
        _ => match stop(&input)? {
            Outcome::Done(result) => result,
            Outcome::Forward => return Ok((HookOutcome::Forwarded, Outcome::Forward)),
        },
    };
    let kind = HookOutcome::of(result.as_ref());
    let neutral = result.map_or(Value::Null, |r| r.to_value());
    let output = format_hook_output(host.id, event, &neutral)?;
    Ok((kind, Outcome::Done(output)))
}

/// Run the Node CLI with the same arguments and give it the stdin this process already read.
/// The child's exit code is not used: a hook always exits with 0.
// TODO(merge): use cli::forward once it can take the bytes of a stdin that was already read.
fn forward_with_stdin(argv: &[String], stdin: &[u8]) -> Result<(), DfwError> {
    let script = plugin_root()
        .map(|root| root.join("bin").join("dfw.mjs"))
        .filter(|f| f.is_file())
        .ok_or_else(|| DfwError::new("this hook still runs on the Node.js CLI, and bin/dfw.mjs was not found (set DFW_PLUGIN_ROOT to the dfw plugin folder)"))?;
    let mut child = Command::new("node")
        .arg(&script)
        .args(argv)
        .env("DFW_FORWARDED", "1")
        .stdin(Stdio::piped())
        .spawn()
        .map_err(|err| DfwError::internal(format!("could not start node: {err}")))?;
    if let Some(mut pipe) = child.stdin.take() {
        // A child that exits without reading closes the pipe: not an error of this hook.
        let _ = pipe.write_all(stdin);
    }
    child.wait()?;
    Ok(())
}
