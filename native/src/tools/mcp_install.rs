//! Ports the second half of `core/lib/mcp.mjs`: `dfw mcp install|uninstall`, which adds the dfw
//! server to (or removes it from) the MCP config of OpenCode, Codex and Cursor.

use std::env;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::config::{js_spread, js_truthy};
use crate::core::error::DfwError;
use crate::core::paths::{env_nonempty, plugin_root};
use crate::core::util::{to_json_compact, to_json_pretty};
use crate::workflow::state::{js_trim, truthy};

/// How a tool's config file holds its MCP servers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// OpenCode: `mcp.<name>` with the command as one list.
    Opencode,
    /// Codex: a `[mcp_servers.<name>]` TOML table.
    Toml,
    /// Cursor and others: `mcpServers.<name>`.
    McpServers,
}

/// A tool `dfw mcp install` knows.
pub struct McpTarget {
    pub id: &'static str,
    pub name: &'static str,
    pub format: Format,
}

/// `MCP_TARGETS`.
pub const MCP_TARGETS: [McpTarget; 3] = [
    McpTarget {
        id: "opencode",
        name: "OpenCode",
        format: Format::Opencode,
    },
    McpTarget {
        id: "codex",
        name: "OpenAI Codex",
        format: Format::Toml,
    },
    McpTarget {
        id: "cursor",
        name: "Cursor",
        format: Format::McpServers,
    },
];

impl McpTarget {
    /// The tool's config file for this user.
    pub fn file(&self) -> PathBuf {
        let home = env::home_dir().unwrap_or_default();
        match self.format {
            Format::Opencode => env_nonempty("XDG_CONFIG_HOME")
                .map_or_else(|| home.join(".config"), PathBuf::from)
                .join("opencode")
                .join("opencode.json"),
            Format::Toml => env_nonempty("CODEX_HOME")
                .map_or_else(|| home.join(".codex"), PathBuf::from)
                .join("config.toml"),
            Format::McpServers => home.join(".cursor").join("mcp.json"),
        }
    }
}

fn target_ids() -> String {
    let ids: Vec<&str> = MCP_TARGETS.iter().map(|t| t.id).collect();
    ids.join(", ")
}

/// The words of `dfw mcp install` when no tool was named.
pub fn which_tool() -> String {
    format!("Which tool? {}, all", target_ids())
}

/// The command a tool runs to start the server.
#[derive(Debug, Clone)]
pub struct ServerCommand {
    pub cmd: String,
    pub args: Vec<String>,
}

/// `serverCommand()`: the plugin's sh launcher (it survives updates of the binary), else this binary.
fn server_command() -> Result<ServerCommand, DfwError> {
    let launcher = plugin_root().map(|root| root.join("bin").join("dfw"));
    let cmd = match launcher {
        Some(launcher) if !cfg!(windows) && launcher.exists() => launcher,
        _ => env::current_exe()?,
    };
    Ok(ServerCommand {
        cmd: cmd.to_string_lossy().into_owned(),
        args: Vec::new(),
    })
}

/// The `TOML_BLOCK` expression of mcp.mjs by hand (it needs a look-ahead): the
/// `[mcp_servers.dfw]` header line and every following line up to the next table header.
/// LF or CRLF files; a header without its newline is not a block.
fn toml_block(text: &str) -> Option<Range<usize>> {
    let mut start = 0;
    loop {
        let line_end = start + text[start..].find('\n')? + 1;
        let line = &text[start..line_end - 1];
        let line = line.strip_suffix('\r').unwrap_or(line);
        let header = line
            .strip_prefix("[mcp_servers.dfw]")
            .is_some_and(|rest| rest.chars().all(|c| c == ' ' || c == '\t'));
        if header {
            let mut end = line_end;
            while end < text.len() && !text[end..].starts_with('[') {
                end = text[end..].find('\n').map_or(text.len(), |i| end + i + 1);
            }
            return Some(start..end);
        }
        start = line_end;
    }
}

/// `text.replace(/\n{3,}/g, '\n\n')`.
fn collapse_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut run = 0;
    for c in text.chars() {
        run = if c == '\n' { run + 1 } else { 0 };
        if run <= 2 {
            out.push(c);
        }
    }
    out
}

/// `text.replace(/\s*$/, '\n')`: JavaScript's white space at the end becomes one newline.
fn end_with_newline(text: &str) -> String {
    // A mark in front keeps js_trim from touching the start.
    let marked = format!("x{text}");
    format!("{}\n", &js_trim(&marked)[1..])
}

/// `mcpConfigText(format, current, { remove, command })`: new text of a tool's config with the
/// dfw server added (or removed). An error on a config it cannot parse.
pub fn mcp_config_text(
    format: Format,
    current: &str,
    remove: bool,
    command: &ServerCommand,
) -> Result<String, DfwError> {
    let mut cmd = vec![command.cmd.clone()];
    cmd.extend(command.args.iter().cloned());
    cmd.push("mcp".into());
    if format == Format::Toml {
        let without = match toml_block(current) {
            Some(block) => format!("{}{}", &current[..block.start], &current[block.end..]),
            None => current.to_string(),
        };
        let rest = end_with_newline(&collapse_blank_lines(&without));
        let empty = js_trim(&rest).is_empty();
        if remove {
            return Ok(if empty { String::new() } else { rest });
        }
        let quoted = |s: &String| to_json_compact(&Value::String(s.clone()));
        let args: Vec<String> = cmd[1..].iter().map(quoted).collect();
        let block = format!(
            "[mcp_servers.dfw]\ncommand = {}\nargs = [{}]\n",
            quoted(&cmd[0]),
            args.join(", ")
        );
        return Ok(if empty {
            block
        } else {
            format!("{rest}\n{block}")
        });
    }
    let mut data = if js_trim(current).is_empty() {
        json!({})
    } else {
        serde_json::from_str::<Value>(current).map_err(|err| {
            DfwError::internal(format!(
                "cannot parse the existing config ({err}); add dfw by hand or fix the file"
            ))
        })?
    };
    let opencode = format == Format::Opencode;
    let key = if opencode { "mcp" } else { "mcpServers" };
    match &mut data {
        Value::Object(map) => {
            let mut servers = js_spread(truthy(map.get(key)));
            if remove {
                servers.shift_remove("dfw");
            } else if opencode {
                servers.insert(
                    "dfw".into(),
                    json!({ "type": "local", "command": cmd, "enabled": true }),
                );
            } else {
                servers.insert("dfw".into(), json!({ "command": cmd[0], "args": cmd[1..] }));
            }
            map.insert(key.into(), Value::Object(servers));
            if opencode && truthy(map.get("$schema")).is_none() {
                map.insert("$schema".into(), json!("https://opencode.ai/config.json"));
            }
        }
        Value::Null => {
            return Err(DfwError::internal(format!(
                "Cannot read properties of null (reading '{key}')"
            )))
        }
        // A property set on an array or a scalar does not reach JSON.stringify: unchanged.
        _ => {}
    }
    Ok(to_json_pretty(&data) + "\n")
}

/// Whether the config file of a format already has the dfw server.
fn installed_in(format: Format, file: &Path) -> bool {
    let Ok(text) = fs::read_to_string(file) else {
        return false;
    };
    if format == Format::Toml {
        return toml_block(&text).is_some();
    }
    let key = if format == Format::Opencode {
        "mcp"
    } else {
        "mcpServers"
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get(key)?.get("dfw").map(js_truthy))
        .unwrap_or(false)
}

/// `mcpInstalled(id)`: whether a tool's config already has the dfw server.
// The doctor line of cli/tools.rs has its own copy until the wave is merged.
#[allow(dead_code)]
pub fn mcp_installed(id: &str) -> bool {
    MCP_TARGETS
        .iter()
        .find(|t| t.id == id)
        .is_some_and(|t| installed_in(t.format, &t.file()))
}

/// One tool of [`mcp_install`]: the report line for its config file.
fn install_into(
    target: &McpTarget,
    file: &Path,
    remove: bool,
    print: bool,
    command: &ServerCommand,
) -> Result<String, DfwError> {
    let shown = file.display();
    let current = if file.exists() {
        fs::read_to_string(file)?
    } else {
        String::new()
    };
    let next = mcp_config_text(target.format, &current, remove, command)?;
    if print {
        return Ok(format!("# {}: {shown}\n{next}", target.name));
    }
    if next == current {
        return Ok(format!("{}: unchanged ({shown})", target.name));
    }
    let base = file
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    // Keep the user's config from before dfw first touched it: a later run must not replace that copy.
    let backup = file.with_file_name(format!("{base}.dfw-backup"));
    if !current.is_empty() && !backup.exists() {
        fs::copy(file, &backup)?;
    }
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    fs::write(file, next)?;
    let action = if remove {
        "removed dfw from"
    } else {
        "added dfw to"
    };
    let kept = if current.is_empty() {
        String::new()
    } else {
        format!(" (backup: {base}.dfw-backup)")
    };
    Ok(format!("{}: {action} {shown}{kept}", target.name))
}

/// `mcpInstall(target, { remove, print })`: `dfw mcp install|uninstall <tool|all> [--print]`.
/// Returns a report.
pub fn mcp_install(target: &str, remove: bool, print: bool) -> Result<String, DfwError> {
    let targets: Vec<&McpTarget> = if target == "all" {
        MCP_TARGETS.iter().collect()
    } else {
        let found = MCP_TARGETS.iter().find(|t| t.id == target);
        vec![found.ok_or_else(|| {
            DfwError::internal(format!(
                "Unknown tool \"{target}\". MCP install targets: {}, all.",
                target_ids()
            ))
        })?]
    };
    let command = server_command()?;
    let mut lines = Vec::new();
    for t in targets {
        lines.push(install_into(t, &t.file(), remove, print, &command)?);
    }
    if !print && !remove {
        lines.push("Restart the tool, then ask it for UI work: it loads the guide through the dfw_guide tool. After moving or updating dfw, run this again.".into());
    }
    Ok(lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn command(cmd: &str) -> ServerCommand {
        ServerCommand {
            cmd: cmd.into(),
            args: Vec::new(),
        }
    }

    fn parsed(text: &str) -> Value {
        serde_json::from_str(text).unwrap()
    }

    /// mcp install: config text for OpenCode, Codex, Cursor (add, keep other servers, remove)
    #[test]
    fn config_text_for_opencode_codex_cursor() {
        let c = command("/x/dfw");
        let oc = parsed(
            &mcp_config_text(
                Format::Opencode,
                r#"{"mcp":{"other":{"type":"local","command":["o"]}}}"#,
                false,
                &c,
            )
            .unwrap(),
        );
        assert_eq!(
            oc["mcp"]["dfw"],
            json!({ "type": "local", "command": ["/x/dfw", "mcp"], "enabled": true })
        );
        assert!(oc["mcp"]["other"].is_object(), "other servers kept");
        assert_eq!(oc["$schema"], "https://opencode.ai/config.json");
        let cursor = mcp_config_text(Format::McpServers, "", false, &c).unwrap();
        assert_eq!(
            cursor,
            "{\n  \"mcpServers\": {\n    \"dfw\": {\n      \"command\": \"/x/dfw\",\n      \"args\": [\n        \"mcp\"\n      ]\n    }\n  }\n}\n"
        );
        let removed = parsed(&mcp_config_text(Format::McpServers, &cursor, true, &c).unwrap());
        assert_eq!(removed["mcpServers"].get("dfw"), None);
        let broken = mcp_config_text(Format::McpServers, "{broken", false, &c).unwrap_err();
        assert!(broken.message().contains("cannot parse"));

        let toml = "model = \"o3\"\n\n[mcp_servers.other]\ncommand = \"o\"\n";
        let added = mcp_config_text(Format::Toml, toml, false, &c).unwrap();
        assert!(added.contains("[mcp_servers.dfw]\ncommand = \"/x/dfw\"\nargs = [\"mcp\"]"));
        assert!(added.contains("[mcp_servers.other]"));
        assert_eq!(
            mcp_config_text(Format::Toml, &added, false, &c).unwrap(),
            added,
            "idempotent"
        );
        assert_eq!(
            mcp_config_text(Format::Toml, &added, true, &c)
                .unwrap()
                .trim(),
            toml.trim()
        );
        assert_eq!(mcp_config_text(Format::Toml, "", true, &c).unwrap(), "");
    }

    /// mcp install: a CRLF config.toml that already has dfw is replaced, not duplicated
    #[test]
    fn crlf_toml_block_is_replaced_not_duplicated() {
        let crlf = "[model]\r\nname = \"x\"\r\n\r\n[mcp_servers.dfw]\r\ncommand = \"/old/dfw\"\r\nargs = [\"mcp\"]\r\n\r\n[mcp_servers.other]\r\ncommand = \"o\"\r\n";
        let c = command("/opt/dfw/bin/dfw");
        let next = mcp_config_text(Format::Toml, crlf, false, &c).unwrap();
        assert_eq!(next.matches("[mcp_servers.dfw]").count(), 1);
        assert!(!next.contains("/old/dfw"));
        assert!(next.contains("[mcp_servers.other]"));
        assert!(!mcp_config_text(Format::Toml, crlf, true, &c)
            .unwrap()
            .contains("mcp_servers.dfw"));
    }

    /// mcp install: the first backup of the user config survives later installs
    #[test]
    fn first_backup_survives_later_installs() {
        let home = test_dir("mcp-backup");
        let file = home.join("config.toml");
        let backup = home.join("config.toml.dfw-backup");
        let original = "[model]\nname = \"x\"\n";
        fs::write(&file, original).unwrap();
        let codex = &MCP_TARGETS[1];
        let c = command("/opt/dfw/bin/dfw");
        let line = install_into(codex, &file, false, false, &c).unwrap();
        assert!(line.starts_with("OpenAI Codex: added dfw to "));
        assert!(line.ends_with(" (backup: config.toml.dfw-backup)"));
        assert!(installed_in(Format::Toml, &file));
        assert_eq!(fs::read_to_string(&backup).unwrap(), original);
        install_into(codex, &file, true, false, &c).unwrap();
        assert_eq!(
            fs::read_to_string(&backup).unwrap(),
            original,
            "uninstall does not replace it with a config that has dfw"
        );
        assert!(!installed_in(Format::Toml, &file));
        install_into(codex, &file, false, false, &c).unwrap();
        assert_eq!(
            fs::read_to_string(&backup).unwrap(),
            original,
            "still the config from before dfw"
        );
        let again = install_into(codex, &file, false, false, &c).unwrap();
        assert!(again.starts_with("OpenAI Codex: unchanged ("));
    }

    #[test]
    fn toml_block_ends_at_the_next_table() {
        let text = "a = 1\n[mcp_servers.dfw] \t\nx = 1\n\n[next]\n";
        assert_eq!(
            &text[toml_block(text).unwrap()],
            "[mcp_servers.dfw] \t\nx = 1\n\n"
        );
        assert_eq!(toml_block("[mcp_servers.dfw]"), None);
        assert_eq!(toml_block("[mcp_servers.dfwx]\n"), None);
        assert_eq!(collapse_blank_lines("a\n\n\n\nb\n\n"), "a\n\nb\n\n");
        assert_eq!(end_with_newline("a \n\u{feff}\n"), "a\n");
        assert_eq!(end_with_newline(""), "\n");
    }
}
