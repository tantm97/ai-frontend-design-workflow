//! Ports `core/lib/host.mjs`: what differs between hosts (hook input and output shapes, tool names, wording).
//!
//! AI coding tools ("hosts") that dfw plugs into. Everything that differs between Claude Code,
//! GitHub Copilot and future hosts lives here, so the rest of the crate stays tool-agnostic:
//! - hook input normalization (field names, tool names, JSON-string arguments),
//! - hook output formats (context, deny a tool call, block stopping),
//! - text placeholders rendered into SKILL.md, references and agents,
//! - plugin locations.
//!
//! Adding a host = one entry here + `adapters/<host>/`.
//!
//! Over 800 lines only with its tests (the ported cases of the JavaScript tests sit next to the code).

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use regex::{Captures, Regex};
use serde_json::{json, Map, Value};

use super::error::DfwError;
use super::paths::env_nonempty;
use super::util::{ArgValue, MARKETPLACE_NAME};

/// How far the support for a host goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// A plugin with hooks ships for it.
    Supported,
    /// Text rendering only (the guide served by `dfw mcp`).
    Internal,
    /// Same core, adapter not written yet; until then it uses `dfw mcp install <tool>`.
    Planned,
}

/// One entry of `HOSTS`.
#[derive(Debug)]
pub struct Host {
    pub id: &'static str,
    pub name: &'static str,
    pub status: Status,
    /// The host's command line program.
    pub cli: Option<&'static str>,
    /// Folder under `adapters/`.
    // Read by the build scripts' twin of this table (host.mjs); kept so the rows match.
    #[allow(
        dead_code,
        reason = "Read by the build scripts' twin of this table (host.mjs); kept so the rows match"
    )]
    pub adapter: Option<&'static str>,
    /// Hook events the host fires and whether a hook can add model context there.
    pub context: Option<&'static [(&'static str, bool)]>,
    /// Values of the `{{PLACEHOLDER}}`s in skill, reference and agent text.
    pub text: Option<&'static [(&'static str, &'static str)]>,
    /// Skills are named `plugin:skill` (else by the bare skill name).
    pub qualified_skill_names: bool,
    /// Reached through the dfw MCP server.
    // Read by the build scripts' twin of this table (host.mjs); kept so the rows match.
    #[allow(
        dead_code,
        reason = "Read by the build scripts' twin of this table (host.mjs); kept so the rows match"
    )]
    pub mcp: bool,
}

const fn planned(id: &'static str, name: &'static str, cli: &'static str) -> Host {
    Host {
        id,
        name,
        status: Status::Planned,
        cli: Some(cli),
        adapter: None,
        context: None,
        text: None,
        qualified_skill_names: false,
        mcp: true,
    }
}

/// `HOSTS`, in the order of the Node table.
pub static HOSTS: [Host; 6] = [
    Host {
        id: "claude",
        name: "Claude Code",
        status: Status::Supported,
        cli: Some("claude"),
        adapter: Some("claude-code"),
        context: Some(&[("SessionStart", true), ("UserPromptSubmit", true), ("PostToolUse", true)]),
        text: Some(&[
            ("HOST_NAME", "Claude Code"),
            ("SKILL_DIR", "${CLAUDE_SKILL_DIR}"),
            ("ASK_TOOL", "AskUserQuestion"),
            ("BACKGROUND", "Bash `run_in_background`"),
            ("HOST_SETTINGS", "`~/.claude/settings.json`"),
            ("DFW_CLI", "`dfw` is a CLI on your PATH (from this plugin's bin/)."),
            ("INVOKE_SKILL", "Invoke skills with the Skill tool by their full `plugin:skill` name."),
        ]),
        qualified_skill_names: true,
        mcp: false,
    },
    Host {
        id: "copilot",
        name: "GitHub Copilot",
        status: Status::Supported,
        cli: Some("copilot"),
        adapter: Some("copilot"),
        // Copilot ignores additionalContext on userPromptSubmitted: comments ride on PostToolUse and Stop.
        context: Some(&[("SessionStart", true), ("UserPromptSubmit", false), ("PostToolUse", true)]),
        text: Some(&[
            ("HOST_NAME", "GitHub Copilot"),
            ("SKILL_DIR", "."),
            ("ASK_TOOL", "the ask_user tool"),
            ("BACKGROUND", "an async/background shell command"),
            ("HOST_SETTINGS", "your Copilot configuration (`~/.copilot/`)"),
            (
                "DFW_CLI",
                "`dfw` is a CLI. If `dfw` is not on your PATH, run it as `~/.copilot/installed-plugins/dfw-marketplace/dfw/bin/dfw` (`dfw install --host copilot` also links it into `~/.local/bin`).",
            ),
            ("INVOKE_SKILL", "Load a skill by its name (for example `impeccable`, `frontend-design`); custom agents by their name (for example `screen-builder`)."),
        ]),
        qualified_skill_names: false,
        mcp: false,
    },
    // Text rendering only: the guide served by "dfw mcp" to tools without a dfw plugin.
    Host {
        id: "mcp",
        name: "MCP",
        status: Status::Internal,
        cli: None,
        adapter: None,
        context: None,
        text: Some(&[
            ("HOST_NAME", "your AI coding tool (through the dfw MCP server)"),
            ("SKILL_DIR", "__DFW_SKILL_DIR__"),
            ("ASK_TOOL", "your tool for asking the user (or a short numbered question in chat)"),
            ("BACKGROUND", "a background shell command if your tool has one; otherwise check `dfw comments inbox` whenever you pause"),
            ("HOST_SETTINGS", "your tool's own settings"),
            (
                "DFW_CLI",
                "`dfw` is a CLI, also available as the `dfw` MCP tool (arguments as a list, cwd = project root). This tool has no write hook: before creating or editing any file in a dfw project, call the `dfw_can_write` MCP tool and never write a DENIED file.",
            ),
            (
                "INVOKE_SKILL",
                "Design skills named here (impeccable, frontend-design, ...) may not be installed in your tool; when one is missing, follow this guide and its references directly. Agents named here (screen-builder, ...) are roles: do their work yourself.",
            ),
        ]),
        qualified_skill_names: false,
        mcp: false,
    },
    // Planned plugins (hooks): same core, new adapter. Until then they use "dfw mcp install <tool>".
    planned("opencode", "OpenCode", "opencode"),
    planned("codex", "OpenAI Codex", "codex"),
    planned("cursor", "Cursor", "cursor-agent"),
];

impl Host {
    /// The value of a text placeholder (`host.text[key]`).
    pub fn text(&self, key: &str) -> Option<&'static str> {
        self.text?.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
    }

    /// `host.context[event]`: `None` for an event the table does not list.
    pub fn context(&self, event: &str) -> Option<bool> {
        self.context?
            .iter()
            .find(|(e, _)| *e == event)
            .map(|(_, can)| *can)
    }

    /// `installedPluginDirs?.() || []`: folders holding the installed marketplace plugins.
    /// Claude Code uses a versioned cache instead (see review/locate).
    pub fn installed_plugin_dirs(&self) -> Vec<PathBuf> {
        match self.id {
            "copilot" => vec![copilot_plugin_dirs().root],
            _ => Vec::new(),
        }
    }

    /// `dfwPluginDir()`: folder of the installed dfw plugin, for the hosts that have a fixed one.
    pub fn dfw_plugin_dir(&self) -> Option<PathBuf> {
        (self.id == "copilot").then(|| copilot_plugin_dirs().dfw)
    }
}

/// Result of [`copilot_plugin_dirs`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopilotPluginDirs {
    /// Directory holding the plugin folders.
    pub root: PathBuf,
    /// Folder of the dfw plugin.
    pub dfw: PathBuf,
}

/// `copilotPluginDirs()`: where Copilot keeps dfw-marketplace plugins. A marketplace added from a
/// local directory is used in place (`<dir>/plugins/dfw`, the one folder every host installs); a
/// GitHub one is copied to `installed-plugins/`.
pub fn copilot_plugin_dirs() -> CopilotPluginDirs {
    let home = env_nonempty("COPILOT_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| env::home_dir().unwrap_or_default().join(".copilot"));
    copilot_plugin_dirs_in(&home)
}

/// [`copilot_plugin_dirs`] for a given Copilot home, so tests need no environment variable.
fn copilot_plugin_dirs_in(home: &Path) -> CopilotPluginDirs {
    // An unreadable or broken settings file means "not a local marketplace", as in Node.
    let settings: Option<Value> = fs::read_to_string(home.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    let source = settings.as_ref().and_then(|s| {
        s.get("extraKnownMarketplaces")?
            .get(MARKETPLACE_NAME)?
            .get("source")
    });
    let local = source
        .filter(|s| s.get("source").and_then(Value::as_str) == Some("directory"))
        .and_then(|s| s.get("path")?.as_str())
        .filter(|p| !p.is_empty());
    if let Some(dir) = local {
        let root = Path::new(dir).join("plugins");
        return CopilotPluginDirs {
            dfw: root.join("dfw"),
            root,
        };
    }
    let root = home.join("installed-plugins").join(MARKETPLACE_NAME);
    CopilotPluginDirs {
        dfw: root.join("dfw"),
        root,
    }
}

/// `SUPPORTED_HOSTS`: ids of the hosts a plugin ships for.
pub fn supported_hosts() -> Vec<&'static str> {
    HOSTS
        .iter()
        .filter(|h| h.status == Status::Supported)
        .map(|h| h.id)
        .collect()
}

/// `getHost(id)`: no id (or an empty one) is Claude Code; an unknown id is an error.
pub fn get_host(id: Option<&str>) -> Result<&'static Host, DfwError> {
    let id = id.filter(|i| !i.is_empty()).unwrap_or("claude");
    HOSTS.iter().find(|h| h.id == id).ok_or_else(|| {
        let known: Vec<&str> = HOSTS.iter().map(|h| h.id).collect();
        // The user's to fix (a --host flag or DFW_HOST), so a user error.
        DfwError::new(format!(
            "Unknown host \"{id}\" (from --host or DFW_HOST). Use one of: {}.",
            known.join(", ")
        ))
    })
}

/// `currentHost(flag)`: host for this process: the `--host` flag, then `DFW_HOST`, then Claude Code.
pub fn current_host(flag: Option<&ArgValue>) -> Result<&'static Host, DfwError> {
    current_host_from(flag, env_nonempty("DFW_HOST").as_deref())
}

fn current_host_from(
    flag: Option<&ArgValue>,
    env_host: Option<&str>,
) -> Result<&'static Host, DfwError> {
    match flag {
        // `--host` without a value is `true` in Node, which no host is called.
        Some(ArgValue::Flag) => get_host(Some("true")),
        Some(ArgValue::Text(t)) if !t.is_empty() => get_host(Some(t)),
        _ => get_host(env_host),
    }
}

/// Runtime tool names that write files, mapped to dfw's canonical names.
const TOOL_ALIASES: [(&str, &str); 10] = [
    ("Edit", "Edit"),
    ("Write", "Write"),
    ("MultiEdit", "MultiEdit"),
    ("NotebookEdit", "NotebookEdit"),
    ("edit", "Edit"),
    ("str_replace_editor", "Edit"),
    ("create", "Write"),
    ("write", "Write"),
    ("apply_patch", "ApplyPatch"),
    ("ApplyPatch", "ApplyPatch"),
];

/// A hook payload in dfw's shape, whichever host sent it.
#[derive(Debug, Clone, PartialEq)]
pub struct HookInput {
    pub event: Option<String>,
    pub session_id: Option<String>,
    pub cwd: Option<String>,
    /// Canonical tool name (`Edit`, `Write`, `ApplyPatch`, ...), or the host's own name.
    pub tool_name: Option<String>,
    /// The tool's arguments; `{}` when there are none, `{ "input": text }` for text that is not JSON.
    pub tool_input: Value,
    pub stop_hook_active: bool,
    pub prompt: Option<String>,
    /// The whole normalized object: the payload's own fields plus the ones above, as Node
    /// returns it. The typed fields above hold text only; a value of another type is here.
    pub raw: Value,
}

fn parse_maybe_json(v: Option<Value>) -> Value {
    match v {
        // e.g. a raw apply_patch body
        Some(Value::String(text)) => {
            serde_json::from_str(&text).unwrap_or_else(|_| json!({ "input": text }))
        }
        Some(other) if js::truthy(&other) => other,
        _ => json!({}),
    }
}

/// `normalizeHookInput(raw)`: normalize a hook payload from any host into dfw's shape.
/// Claude Code and Copilot's PascalCase (VS Code compatible) format already use snake_case;
/// Copilot's camelCase format uses sessionId/toolName/toolArgs.
pub fn normalize_hook_input(raw: &Value) -> Result<HookInput, DfwError> {
    // A payload that is not a JSON object (null, a list, a string) is an empty payload.
    let src = match raw {
        Value::Object(map) => map.clone(),
        _ => Map::new(),
    };
    // `raw.a ?? raw.b`: `None` is JavaScript's undefined.
    let either = |a: &str, b: &str| match src.get(a) {
        Some(v) if !v.is_null() => Some(v.clone()),
        _ => src.get(b).cloned(),
    };
    // Only a string names a tool; the alias table is read by own key ("toString" is not an alias).
    let tool = either("tool_name", "toolName")
        .and_then(|tool| tool.as_str().filter(|t| !t.is_empty()).map(str::to_string))
        .map(|name| {
            let alias = TOOL_ALIASES.iter().find(|(from, _)| *from == name);
            Value::String(alias.map_or(name.clone(), |(_, to)| to.to_string()))
        });
    let fields = [
        ("event", either("hook_event_name", "hookEventName")),
        ("session_id", either("session_id", "sessionId")),
        ("cwd", src.get("cwd").cloned()),
        ("tool_name", tool),
        (
            "tool_input",
            Some(parse_maybe_json(either("tool_input", "toolArgs"))),
        ),
        (
            "stop_hook_active",
            Some(Value::Bool(
                either("stop_hook_active", "stopHookActive").is_some_and(|v| js::truthy(&v)),
            )),
        ),
        ("prompt", src.get("prompt").cloned()),
    ];
    let mut out = src.clone();
    for (key, value) in fields {
        match value {
            Some(value) => {
                out.insert(key.to_string(), value);
            }
            // An undefined field hides the payload's own one and is not printed as JSON.
            None => {
                out.shift_remove(key);
            }
        }
    }
    let text = |key: &str| out.get(key).and_then(Value::as_str).map(str::to_string);
    Ok(HookInput {
        event: text("event"),
        session_id: text("session_id"),
        cwd: text("cwd"),
        tool_name: text("tool_name"),
        tool_input: out.get("tool_input").cloned().unwrap_or_else(|| json!({})),
        stop_hook_active: out.get("stop_hook_active") == Some(&Value::Bool(true)),
        prompt: text("prompt"),
        raw: Value::Object(out),
    })
}

/// What a hook handler decided, before it is put in a host's words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookResult {
    /// Text added to the model's context.
    Context(String),
    /// The tool call is refused, with the reason.
    Deny(String),
    /// The agent may not stop yet, with the reason.
    Block(String),
}

impl HookResult {
    /// The neutral object of Node: `{ context }`, `{ deny }` or `{ block }`.
    pub fn to_value(&self) -> Value {
        match self {
            HookResult::Context(text) => json!({ "context": text }),
            HookResult::Deny(text) => json!({ "deny": text }),
            HookResult::Block(text) => json!({ "block": text }),
        }
    }
}

/// `formatHookOutput(hostId, event, result)`: turn a neutral hook result (`{ context }`,
/// `{ deny }`, `{ block }` or null) into the host's output JSON; `None` when nothing is printed.
pub fn format_hook_output(
    host_id: &str,
    event: &str,
    result: &Value,
) -> Result<Option<Value>, DfwError> {
    if !js::truthy(result) {
        return Ok(None);
    }
    let host = get_host(Some(host_id))?;
    let field = |key: &str| result.get(key).filter(|v| js::truthy(v));
    if let Some(block) = field("block") {
        return Ok(Some(json!({ "decision": "block", "reason": block })));
    }
    if let Some(deny) = field("deny") {
        return Ok(Some(if host.id == "claude" {
            json!({ "hookSpecificOutput": { "hookEventName": event, "permissionDecision": "deny", "permissionDecisionReason": deny } })
        } else {
            json!({ "permissionDecision": "deny", "permissionDecisionReason": deny })
        }));
    }
    if let Some(context) = field("context") {
        // A host without a context table (mcp, planned hosts) gets the generic form.
        if host.context(event) == Some(false) {
            return Ok(None);
        }
        return Ok(Some(if host.id == "claude" {
            json!({ "hookSpecificOutput": { "hookEventName": event, "additionalContext": context } })
        } else {
            json!({ "additionalContext": context })
        }));
    }
    Ok(None)
}

/// JavaScript's `\s`, which is not the `\s` of the regex crate (U+FEFF is in, U+0085 is out).
use crate::core::js;
use crate::core::js::SPACE_CLASS as JS_SPACE;

fn regex(pattern: &str) -> Result<Regex, DfwError> {
    Regex::new(pattern).map_err(|err| DfwError::internal(err.to_string()))
}

/// `renderText(text, hostId)`: render host placeholders (`{{SKILL_DIR}}`, `<!-- host:id -->`
/// blocks) in skill, reference and agent text.
pub fn render_text(text: &str, host_id: &str) -> Result<String, DfwError> {
    let host = get_host(Some(host_id))?;
    if host.text.is_none() {
        let with_text: Vec<&str> = HOSTS
            .iter()
            .filter(|h| h.text.is_some())
            .map(|h| h.id)
            .collect();
        return Err(DfwError::internal(format!(
            "Host \"{}\" has no text placeholders. Hosts with text: {}",
            host.id,
            with_text.join(", ")
        )));
    }
    // JS: /<!--\s*host:([\w,]+)\s*-->\n?([\s\S]*?)<!--\s*\/host\s*-->\n?/g
    let blocks = regex(&format!(
        r"<!--{JS_SPACE}*host:([A-Za-z0-9_,]+){JS_SPACE}*-->\n?((?s:.*?))<!--{JS_SPACE}*/host{JS_SPACE}*-->\n?"
    ))?;
    let out = blocks.replace_all(text, |c: &Captures| {
        if c[1].split(',').any(|id| id == host.id) {
            c[2].to_string()
        } else {
            String::new()
        }
    });
    // JS: /\{\{([A-Z_]+)\}\}/g
    let placeholders = regex(r"\{\{([A-Z_]+)\}\}")?;
    let out = placeholders.replace_all(&out, |c: &Captures| {
        host.text(&c[1]).unwrap_or(&c[0]).to_string()
    });
    if host.qualified_skill_names {
        return Ok(out.into_owned());
    }
    let qualified = regex(
        r"`(?:frontend-design|ux-research|ui-ux-pro-max|impeccable|emil-design-eng|hallmark|taste-reference|dfw):([a-z0-9-]+)`",
    )?;
    Ok(qualified.replace_all(&out, "`${1}`").into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::paths::rel;
    use crate::core::util::{test_dir, to_json_compact};

    fn format(host: &str, event: &str, result: Value) -> Option<Value> {
        format_hook_output(host, event, &result).unwrap()
    }

    /// host: supported hosts are Claude Code and GitHub Copilot; others planned (tests/unit/host.test.mjs)
    #[test]
    fn supported_hosts_are_claude_code_and_github_copilot_others_planned() {
        assert_eq!(supported_hosts(), ["claude", "copilot"]);
        for id in ["opencode", "codex", "cursor"] {
            assert_eq!(get_host(Some(id)).unwrap().status, Status::Planned);
        }
    }

    /// host: normalizes Claude, Copilot PascalCase and Copilot camelCase payloads
    /// (tests/unit/host.test.mjs; the two assertions on filesFromToolInput and filesFromPatch
    /// belong to workflow/gates and are ported there)
    #[test]
    fn normalizes_claude_copilot_pascal_case_and_copilot_camel_case_payloads() {
        let claude = normalize_hook_input(&json!({ "session_id": "s", "tool_name": "Write", "tool_input": { "file_path": "a.ts" } })).unwrap();
        assert_eq!(claude.tool_name.as_deref(), Some("Write"));
        let camel = normalize_hook_input(&json!({ "sessionId": "s", "toolName": "create", "toolArgs": "{\"path\":\"src/a.ts\"}", "stopHookActive": true })).unwrap();
        assert_eq!(camel.session_id.as_deref(), Some("s"));
        assert_eq!(camel.tool_name.as_deref(), Some("Write"));
        assert_eq!(camel.tool_input, json!({ "path": "src/a.ts" }));
        assert!(camel.stop_hook_active);
        assert_eq!(
            normalize_hook_input(&json!({ "toolName": "str_replace_editor" }))
                .unwrap()
                .tool_name
                .as_deref(),
            Some("Edit")
        );
        let body = format!(
            "*** Begin Patch\n*** Add File: src/x.ts\n*** Update File: {}\n*** End Patch",
            rel::design("a.html")
        );
        let patch =
            normalize_hook_input(&json!({ "toolName": "apply_patch", "toolArgs": body })).unwrap();
        assert_eq!(patch.tool_name.as_deref(), Some("ApplyPatch"));
        assert_eq!(patch.tool_input, json!({ "input": body }));
    }

    /// host: output formats per host (tests/unit/host.test.mjs)
    #[test]
    fn output_formats_per_host() {
        assert_eq!(
            format("claude", "PreToolUse", json!({ "deny": "no" })),
            Some(
                json!({ "hookSpecificOutput": { "hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": "no" } })
            )
        );
        assert_eq!(
            format("copilot", "PreToolUse", json!({ "deny": "no" })),
            Some(json!({ "permissionDecision": "deny", "permissionDecisionReason": "no" }))
        );
        assert_eq!(
            format("copilot", "SessionStart", json!({ "context": "c" })),
            Some(json!({ "additionalContext": "c" }))
        );
        assert_eq!(
            format("copilot", "UserPromptSubmit", json!({ "context": "c" })),
            None
        );
        assert_eq!(
            format("copilot", "Stop", json!({ "block": "r" })),
            Some(json!({ "decision": "block", "reason": "r" }))
        );
        assert_eq!(format("claude", "Stop", Value::Null), None);
    }

    /// host: renders skill text per host (tests/unit/host.test.mjs)
    #[test]
    fn renders_skill_text_per_host() {
        let src = "Dir {{SKILL_DIR}}/references. Ask with {{ASK_TOOL}}. Use `impeccable:impeccable` and `dfw:screen-builder`.\n<!-- host:claude -->\nclaude only\n<!-- /host -->\n<!-- host:copilot -->\ncopilot only\n<!-- /host -->\n";
        let c = render_text(src, "claude").unwrap();
        assert!(c.contains("${CLAUDE_SKILL_DIR}/references"));
        assert!(c.contains("AskUserQuestion"));
        assert!(c.contains("`impeccable:impeccable`"));
        assert!(c.contains("claude only"));
        assert!(!c.contains("copilot only"));
        let g = render_text(src, "copilot").unwrap();
        assert!(g.contains("Dir ./references"));
        assert!(g.contains("ask_user"));
        assert!(g.contains("`impeccable` and `screen-builder`"));
        assert!(g.contains("copilot only"));
        assert!(!g.contains("claude only") && !g.contains("CLAUDE_SKILL_DIR"));
        assert_eq!(g, "Dir ./references. Ask with the ask_user tool. Use `impeccable` and `screen-builder`.\ncopilot only\n");
    }

    #[test]
    fn render_text_edge_cases() {
        assert_eq!(
            render_text("{{NOPE}} {{lower}} {{HOST_NAME}}", "mcp").unwrap(),
            "{{NOPE}} {{lower}} your AI coding tool (through the dfw MCP server)"
        );
        assert_eq!(
            render_text(
                "a<!--host:mcp,copilot-->x<!--/host-->b<!-- host:claude -->y<!-- /host -->",
                "copilot"
            )
            .unwrap(),
            "axb"
        );
        // A planned host has no texts: refused with its own message, placeholders or not.
        assert_eq!(
            render_text("only", "codex").unwrap_err().message(),
            "Host \"codex\" has no text placeholders. Hosts with text: claude, copilot, mcp"
        );
        assert!(render_text("x", "nope").is_err());
    }

    #[test]
    fn get_host_and_current_host() {
        assert_eq!(get_host(None).unwrap().id, "claude");
        assert_eq!(get_host(Some("")).unwrap().id, "claude");
        assert_eq!(
            get_host(Some("nope")).unwrap_err().report(),
            "Unknown host \"nope\" (from --host or DFW_HOST). Use one of: claude, copilot, mcp, opencode, codex, cursor."
        );
        let text = |t: &str| ArgValue::Text(t.to_string());
        assert_eq!(
            current_host_from(Some(&text("copilot")), Some("mcp"))
                .unwrap()
                .id,
            "copilot"
        );
        assert_eq!(
            current_host_from(None, Some("copilot")).unwrap().id,
            "copilot"
        );
        assert_eq!(
            current_host_from(Some(&text("")), Some("copilot"))
                .unwrap()
                .id,
            "copilot",
            "an empty flag is falsy"
        );
        assert_eq!(current_host_from(None, None).unwrap().id, "claude");
        assert!(current_host_from(Some(&ArgValue::Flag), Some("copilot"))
            .unwrap_err()
            .message()
            .starts_with("Unknown host \"true\""));
        assert!(current_host_from(None, Some("nope")).is_err());
    }

    #[test]
    fn host_table_matches_node() {
        let ids: Vec<&str> = HOSTS.iter().map(|h| h.id).collect();
        assert_eq!(
            ids,
            ["claude", "copilot", "mcp", "opencode", "codex", "cursor"]
        );
        let copilot = get_host(Some("copilot")).unwrap();
        assert_eq!(
            (copilot.name, copilot.cli, copilot.adapter),
            ("GitHub Copilot", Some("copilot"), Some("copilot"))
        );
        assert_eq!(copilot.context("UserPromptSubmit"), Some(false));
        assert_eq!(copilot.context("Stop"), None);
        assert_eq!(copilot.text("SKILL_DIR"), Some("."));
        assert_eq!(
            get_host(Some("claude")).unwrap().adapter,
            Some("claude-code")
        );
        assert!(get_host(Some("claude"))
            .unwrap()
            .installed_plugin_dirs()
            .is_empty());
        assert_eq!(get_host(Some("claude")).unwrap().dfw_plugin_dir(), None);
        let mcp = get_host(Some("mcp")).unwrap();
        assert_eq!(
            (mcp.status, mcp.cli, mcp.mcp),
            (Status::Internal, None, false)
        );
        assert_eq!(mcp.text("SKILL_DIR"), Some("__DFW_SKILL_DIR__"));
        let cursor = get_host(Some("cursor")).unwrap();
        assert_eq!(
            (cursor.name, cursor.cli, cursor.mcp, cursor.text("ASK_TOOL")),
            ("Cursor", Some("cursor-agent"), true, None)
        );
    }

    #[test]
    fn normalized_input_keeps_the_payload_and_the_node_key_order() {
        let n = normalize_hook_input(&json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "cwd": "/p", "extra": 1, "session_id": null, "prompt": "hi" })).unwrap();
        assert_eq!(
            to_json_compact(&n.raw),
            r#"{"hook_event_name":"PreToolUse","tool_name":"Bash","cwd":"/p","extra":1,"prompt":"hi","event":"PreToolUse","tool_input":{},"stop_hook_active":false}"#,
            "a null session_id with no sessionId is undefined: not printed"
        );
        assert_eq!(
            (
                n.event.as_deref(),
                n.cwd.as_deref(),
                n.prompt.as_deref(),
                n.session_id
            ),
            (Some("PreToolUse"), Some("/p"), Some("hi"), None)
        );
        let camel = normalize_hook_input(&json!({ "hookEventName": "Stop", "sessionId": 7, "stopHookActive": "false", "toolArgs": "" })).unwrap();
        assert_eq!(
            to_json_compact(&camel.raw),
            r#"{"hookEventName":"Stop","sessionId":7,"stopHookActive":"false","toolArgs":"","event":"Stop","session_id":7,"tool_input":{"input":""},"stop_hook_active":true}"#
        );
        assert_eq!(camel.session_id, None, "typed fields hold text only");
        // Arguments that parse as JSON are used as parsed, whatever they are; falsy ones are {}.
        assert_eq!(
            normalize_hook_input(&json!({ "toolArgs": "[1]" }))
                .unwrap()
                .tool_input,
            json!([1])
        );
        assert_eq!(
            normalize_hook_input(&json!({ "tool_input": 0, "toolArgs": "{\"a\":1}" }))
                .unwrap()
                .tool_input,
            json!({})
        );
        assert_eq!(
            normalize_hook_input(&json!({ "tool_input": null, "toolArgs": "{\"a\":1}" }))
                .unwrap()
                .tool_input,
            json!({ "a": 1 })
        );
        assert_eq!(
            normalize_hook_input(&json!({ "tool_name": "", "toolName": "edit" }))
                .unwrap()
                .tool_name,
            None,
            "an empty name is not nullish"
        );
        assert_eq!(
            normalize_hook_input(&json!({ "tool_name": "Bash" }))
                .unwrap()
                .tool_name
                .as_deref(),
            Some("Bash")
        );
        assert_eq!(
            to_json_compact(&normalize_hook_input(&json!(5)).unwrap().raw),
            r#"{"tool_input":{},"stop_hook_active":false}"#
        );
    }

    /// l: host helpers answer with their own messages, not JavaScript errors (tests/unit/node-stable.test.mjs)
    #[test]
    fn host_helpers_answer_with_their_own_messages() {
        assert_eq!(
            format("mcp", "SessionStart", json!({ "context": "x" })),
            Some(json!({ "additionalContext": "x" }))
        );
        assert_eq!(
            format("opencode", "PostToolUse", json!({ "context": "x" })),
            Some(json!({ "additionalContext": "x" }))
        );
        assert_eq!(
            format("copilot", "UserPromptSubmit", json!({ "context": "x" })),
            None,
            "a context table still decides"
        );
        assert_eq!(
            render_text("a {{ASK_TOOL}}", "opencode").unwrap_err().report(),
            "Error: Host \"opencode\" has no text placeholders. Hosts with text: claude, copilot, mcp"
        );
        for raw in [Value::Null, json!([]), json!("text"), json!(5)] {
            let n = normalize_hook_input(&raw).unwrap();
            assert_eq!(n.tool_name, None);
            assert_eq!(n.tool_input, json!({}));
            assert!(!n.stop_hook_active);
        }
        assert_eq!(
            to_json_compact(&normalize_hook_input(&json!("ab")).unwrap().raw),
            r#"{"tool_input":{},"stop_hook_active":false}"#
        );
    }

    /// o: a tool name that is not a string names no tool (tests/unit/node-stable.test.mjs)
    #[test]
    fn a_tool_name_that_is_not_a_string_names_no_tool() {
        for tool in [
            json!(["Write"]),
            json!({ "name": "Write" }),
            json!(5),
            json!(true),
        ] {
            let a = normalize_hook_input(&json!({ "tool_name": tool.clone() })).unwrap();
            assert_eq!(a.tool_name, None, "{tool}");
            assert!(a.raw.get("tool_name").is_none());
            let b = normalize_hook_input(&json!({ "toolName": tool.clone() })).unwrap();
            assert_eq!(b.tool_name, None, "{tool}");
        }
        let name = |raw: Value| normalize_hook_input(&raw).unwrap().tool_name;
        assert_eq!(
            name(json!({ "tool_name": "create" })).as_deref(),
            Some("Write")
        );
        assert_eq!(name(json!({ "toolName": "Bash" })).as_deref(), Some("Bash"));
        for proto in ["constructor", "toString", "__proto__", "hasOwnProperty"] {
            assert_eq!(name(json!({ "tool_name": proto })).as_deref(), Some(proto));
        }
    }

    #[test]
    fn output_format_edge_cases() {
        // block wins over deny, deny over context; empty text is no result.
        assert_eq!(
            format(
                "claude",
                "Stop",
                json!({ "context": "c", "deny": "d", "block": "b" })
            ),
            Some(json!({ "decision": "block", "reason": "b" }))
        );
        assert_eq!(
            format(
                "copilot",
                "PreToolUse",
                json!({ "context": "c", "deny": "d", "block": "" })
            ),
            Some(json!({ "permissionDecision": "deny", "permissionDecisionReason": "d" }))
        );
        assert_eq!(format("claude", "Stop", json!({ "context": "" })), None);
        assert_eq!(format("claude", "Stop", json!({})), None);
        assert_eq!(
            to_json_compact(
                &format("claude", "Stop", HookResult::Context("c".into()).to_value()).unwrap()
            ),
            r#"{"hookSpecificOutput":{"hookEventName":"Stop","additionalContext":"c"}}"#,
            "an event outside the table still gets context"
        );
        assert_eq!(
            format("", "PreToolUse", HookResult::Deny("d".into()).to_value()),
            format("claude", "PreToolUse", json!({ "deny": "d" }))
        );
        assert_eq!(
            format("cursor", "Stop", HookResult::Block("b".into()).to_value()),
            Some(json!({ "decision": "block", "reason": "b" }))
        );
        // No result is decided before the host is looked up.
        assert_eq!(format("nope", "Stop", Value::Null), None);
        assert!(format_hook_output("nope", "Stop", &json!({})).is_err());
        assert_eq!(
            format("mcp", "SessionStart", json!({ "context": "c" })),
            Some(json!({ "additionalContext": "c" })),
            "a host without a context table gets the generic form"
        );
    }

    #[test]
    fn copilot_plugin_dirs_follow_the_marketplace_source() {
        let home = test_dir("copilot-home");
        let installed = home.join("installed-plugins").join("dfw-marketplace");
        assert_eq!(
            copilot_plugin_dirs_in(&home),
            CopilotPluginDirs {
                dfw: installed.join("dfw"),
                root: installed.clone()
            }
        );
        fs::write(home.join("settings.json"), "{ broken").unwrap();
        assert_eq!(copilot_plugin_dirs_in(&home).root, installed);
        fs::write(home.join("settings.json"), r#"{"extraKnownMarketplaces":{"dfw-marketplace":{"source":{"source":"github","repo":"x/y"}}}}"#).unwrap();
        assert_eq!(copilot_plugin_dirs_in(&home).root, installed);
        fs::write(home.join("settings.json"), r#"{"extraKnownMarketplaces":{"dfw-marketplace":{"source":{"source":"directory","path":"/repo"}}}}"#).unwrap();
        let local = Path::new("/repo").join("plugins");
        assert_eq!(
            copilot_plugin_dirs_in(&home),
            CopilotPluginDirs {
                dfw: local.join("dfw"),
                root: local
            }
        );
        fs::remove_dir_all(&home).unwrap();
    }
}
