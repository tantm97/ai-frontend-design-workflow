//! Ports the second half of `core/lib/mcp.mjs`: `dfw mcp install|uninstall`, which adds the dfw
//! server to (or removes it from) the MCP config of OpenCode, Codex and Cursor.

use std::env;
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{env_nonempty, plugin_root};
use crate::core::util::{to_json_compact, to_json_pretty, write_atomic};

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

/// The keys of a TOML table header line (`[a.b]`, `[ a . "b" ]  # note`), or `None` when the
/// line is not a table header (an array of tables `[[x]]` is not one either). `Err` when it
/// starts like a header but cannot be read.
fn table_header(line: &str) -> Option<Result<Vec<String>, ()>> {
    let rest = line.trim_start().strip_prefix('[')?;
    if rest.starts_with('[') {
        return None;
    }
    let mut keys = Vec::new();
    let mut chars = rest.chars().peekable();
    let skip_space = |chars: &mut std::iter::Peekable<std::str::Chars>| {
        while chars.next_if(|c| *c == ' ' || *c == '\t').is_some() {}
    };
    loop {
        skip_space(&mut chars);
        let mut key = String::new();
        match chars.peek() {
            Some(q @ ('"' | '\'')) => {
                let quote = *q;
                chars.next();
                loop {
                    match chars.next() {
                        Some(c) if c == quote => break,
                        // An escape in a basic string: dfw never writes one, so it is not read.
                        Some('\\') if quote == '"' => return Some(Err(())),
                        Some(c) => key.push(c),
                        None => return Some(Err(())),
                    }
                }
            }
            _ => {
                while let Some(c) =
                    chars.next_if(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                {
                    key.push(c);
                }
                if key.is_empty() {
                    return Some(Err(()));
                }
            }
        }
        keys.push(key);
        skip_space(&mut chars);
        match chars.next() {
            Some('.') => continue,
            Some(']') => break,
            _ => return Some(Err(())),
        }
    }
    skip_space(&mut chars);
    match chars.next() {
        None | Some('#') => Some(Ok(keys)),
        Some(_) => Some(Err(())),
    }
}

/// The `TOML_BLOCK` expression of mcp.mjs by hand (it needs a look-ahead): the
/// `[mcp_servers.dfw]` header line (also written `[ mcp_servers . "dfw" ]  # note`) and every
/// following line up to the next table header. LF or CRLF files. A table header that cannot be
/// read is an error: guessing would add a second dfw table and break the file.
fn toml_block(text: &str) -> Result<Option<Range<usize>>, DfwError> {
    let mut start = 0;
    while start < text.len() {
        let line_end = text[start..]
            .find('\n')
            .map_or(text.len(), |i| start + i + 1);
        let line = text[start..line_end].trim_end_matches('\n');
        let line = line.strip_suffix('\r').unwrap_or(line);
        match table_header(line) {
            Some(Ok(keys)) if keys == ["mcp_servers", "dfw"] => {
                // The block ends at the next line that is a table header (`[x]` or `[[x]]`);
                // an indented array element (`  ["a"],`) is not one.
                let mut end = line_end;
                while end < text.len() {
                    let next = text[end..].find('\n').map_or(text.len(), |i| end + i + 1);
                    let line = text[end..next].trim_end_matches(['\n', '\r']);
                    let header = line.trim_start().starts_with("[[")
                        || matches!(table_header(line), Some(Ok(_)));
                    if header {
                        break;
                    }
                    end = next;
                }
                return Ok(Some(start..end));
            }
            Some(Err(())) => {
                return Err(DfwError::new(format!(
                    "cannot read the table header {line:?} in the Codex config; fix it or add dfw by hand"
                )))
            }
            _ => {}
        }
        start = line_end;
    }
    Ok(None)
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
    // A mark in front keeps js::trim from touching the start.
    let marked = format!("x{text}");
    format!("{}\n", &js::trim(&marked)[1..])
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
        let without = match toml_block(current)? {
            Some(block) => format!("{}{}", &current[..block.start], &current[block.end..]),
            None => current.to_string(),
        };
        let rest = end_with_newline(&collapse_blank_lines(&without));
        let empty = js::trim(&rest).is_empty();
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
    let mut data = if js::trim(current).is_empty() {
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
            let mut servers = js::spread(js::truthy_value(map.get(key)));
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
            if opencode && js::truthy_value(map.get("$schema")).is_none() {
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
        return toml_block(&text).is_ok_and(|b| b.is_some());
    }
    let key = if format == Format::Opencode {
        "mcp"
    } else {
        "mcpServers"
    };
    serde_json::from_str::<Value>(&text)
        .ok()
        .and_then(|v| v.get(key)?.get("dfw").map(js::truthy))
        .unwrap_or(false)
}

/// `mcpInstalled(id)`: whether a tool's config already has the dfw server.
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
    write_atomic(file, next)?;
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

    /// j: names that are prototype keys are not ids (tests/unit/node-stable.test.mjs; MCP targets)
    #[test]
    fn prototype_names_are_not_mcp_targets() {
        let err = mcp_install("constructor", false, true).unwrap_err();
        assert!(err
            .message()
            .starts_with("Unknown tool \"constructor\". MCP install targets: "));
        assert!(!mcp_installed("constructor") && !mcp_installed("toString"));
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
        let block = |t: &str| toml_block(t).unwrap().map(|r| t[r].to_string());
        let text = "a = 1\n[mcp_servers.dfw] \t\nx = 1\n\n[next]\n";
        assert_eq!(
            block(text).as_deref(),
            Some("[mcp_servers.dfw] \t\nx = 1\n\n")
        );
        // A header on the last line without its newline is a block too.
        assert_eq!(
            block("[mcp_servers.dfw]").as_deref(),
            Some("[mcp_servers.dfw]")
        );
        assert_eq!(block("[mcp_servers.dfwx]\n"), None);
        assert_eq!(block("[[mcp_servers.dfw]]\n"), None);
        assert_eq!(block("x = \"[mcp_servers.dfw]\"\n"), None);
        let multi = "[mcp_servers.dfw]\nargs = [\n  [\"a\"],\n]\n  [other]\nk = 1\n";
        assert_eq!(
            block(multi).as_deref(),
            Some("[mcp_servers.dfw]\nargs = [\n  [\"a\"],\n]\n"),
            "an array element is not a header, an indented header is"
        );
    }

    /// mcp install: a header with a comment or spaces is the dfw table (a second one made the
    /// Codex config invalid); a header that cannot be read is refused
    #[test]
    fn toml_header_with_a_comment_is_replaced_and_a_broken_one_refused() {
        let c = command("/new/dfw");
        for header in [
            "[mcp_servers.dfw] # mine",
            "[mcp_servers.dfw]# mine",
            "  [ mcp_servers . dfw ]",
            "[mcp_servers.\"dfw\"]",
            "['mcp_servers'.dfw]  # quoted",
        ] {
            let text =
                format!("model = \"o3\"\n\n{header}\ncommand = \"/old/dfw\"\nargs = [\"mcp\"]\n");
            let next = mcp_config_text(Format::Toml, &text, false, &c).unwrap();
            assert_eq!(next.matches("command = ").count(), 1, "{header}: {next}");
            assert!(next.contains("command = \"/new/dfw\""), "{header}");
            assert!(!next.contains("/old/dfw"), "{header}");
        }
        for broken in [
            "[mcp_servers.dfw] trailing",
            "[mcp_servers.\"dfw]",
            "[mcp_servers..dfw]",
            "[mcp_servers.dfw",
        ] {
            let text = format!("{broken}\ncommand = \"/old/dfw\"\n");
            let err = mcp_config_text(Format::Toml, &text, false, &c).unwrap_err();
            assert!(
                err.message().starts_with("cannot read the table header"),
                "{broken}"
            );
            assert_eq!(err.exit_code(), 1);
        }
        assert_eq!(collapse_blank_lines("a\n\n\n\nb\n\n"), "a\n\nb\n\n");
        assert_eq!(end_with_newline("a \n\u{feff}\n"), "a\n");
        assert_eq!(end_with_newline(""), "\n");
    }
}
