//! Ports `preToolUse` and `notePresence` of `core/lib/hooks.mjs`: the write gate on every tool
//! call, and the note that tells the Studio which element the agent is about to edit.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;
use serde_json::{json, Value};

use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult};
use crate::core::js;
use crate::core::paths::{env_nonempty, find_project_root, project_paths, resolve};
use crate::core::util::{now_iso, write_json};
use crate::workflow::gates::{decide_write, files_from_tool_input, relative};
use crate::workflow::state::load_state;

/// The session's working directory from the payload, else the process's. A `cwd` that is not
/// text is an error, as Node cannot resolve a path against it.
fn session_cwd(input: &HookInput) -> Result<PathBuf, DfwError> {
    match js::truthy_value(input.raw.get("cwd")) {
        Some(Value::String(cwd)) => Ok(PathBuf::from(cwd)),
        Some(_) => Err(DfwError::internal(
            "The \"paths[0]\" argument must be of type string.",
        )),
        None => Ok(env::current_dir()?),
    }
}

/// `rootFrom(input)`: the project a hook call is about: `DFW_PROJECT_DIR`, `CLAUDE_PROJECT_DIR`,
/// the payload's `cwd`, then the working directory.
pub fn root_from(input: &HookInput) -> Result<PathBuf, DfwError> {
    let start = match env_nonempty("DFW_PROJECT_DIR").or_else(|| env_nonempty("CLAUDE_PROJECT_DIR"))
    {
        Some(dir) => PathBuf::from(dir),
        None => session_cwd(input)?,
    };
    Ok(find_project_root(Some(&start)))
}

/// `toolFiles(input)`: absolute paths of the files the tool call writes. Relative paths
/// (apply_patch bodies) are relative to the session's cwd, not the project root.
pub fn tool_files(input: &HookInput) -> Result<Vec<PathBuf>, DfwError> {
    let files = files_from_tool_input(input.tool_name.as_deref(), &input.tool_input)?;
    if files.is_empty() {
        return Ok(Vec::new());
    }
    let cwd = session_cwd(input)?;
    Ok(files.iter().map(|f| resolve(cwd.join(f))).collect())
}

/// `preToolUse(input)`: refuse a write the gates do not allow yet; otherwise note what is edited.
pub fn pre_tool_use(input: &HookInput) -> Result<Option<HookResult>, DfwError> {
    let files = tool_files(input)?;
    if files.is_empty() {
        return Ok(None);
    }
    let root = root_from(input)?;
    let Some(state) = load_state(&root)? else {
        return Ok(None);
    };
    let config = load_config(Some(&root))?;
    for f in &files {
        let decision = decide_write(&root, Some(&state), &config, f)?;
        if !decision.allow {
            return Ok(Some(HookResult::Deny(decision.reason.unwrap_or_default())));
        }
    }
    if state.get("active") != Some(&Value::Bool(false)) {
        // Presence is a nicety: it never blocks an edit.
        let _ = note_presence(&root, &input.tool_input, &files[0]);
    }
    Ok(None)
}

/// `notePresence(root, input, absFile)`: tell the Studio what the agent is about to edit
/// (`.dfw/presence.json`), so it can show a cursor on that element before the change lands.
/// Only for design files; the element is the `data-dfw-id` inside the text being replaced, or
/// the nearest one before it. Returns the note written, `None` for a file that is not a design file.
pub fn note_presence(
    root: &Path,
    tool_input: &Value,
    abs_file: &Path,
) -> Result<Option<Value>, DfwError> {
    let p = project_paths(root);
    let rel = relative(&resolve(&p.design), &resolve(abs_file));
    // "..x.html" is a design file; only ".." and "../..." are outside.
    if rel == ".." || rel.starts_with("../") || rel.starts_with("approved/") {
        return Ok(None);
    }
    let first_edit = tool_input
        .get("edits")
        .and_then(|e| e.get(0))
        .and_then(|e| e.get("old_string"));
    let old = js::truthy_value(tool_input.get("old_string"))
        .or(js::truthy_value(first_edit))
        .map(|o| js::to_string(Some(o)))
        .unwrap_or_default();
    let mut dfw_id = None;
    if !old.is_empty() && rel.ends_with(".html") && abs_file.exists() {
        let text = String::from_utf8_lossy(&fs::read(abs_file)?).into_owned();
        // `\b` of JavaScript is ASCII. Either quote (JS: a back-reference to the opening one).
        let id = Regex::new(
            r#"(?:data-dfw-id|<dfw-use(?-u:\b)[^>]*?(?-u:\b)id)=(?:"([^"']+)"|'([^"']+)')"#,
        )
        .map_err(|err| DfwError::internal(err.to_string()))?;
        // Ids inside HTML comments are not elements: blank them (same length, so offsets hold).
        // An unclosed comment runs to the end.
        let comment = Regex::new(r"(?s)<!--.*?(?:-->|$)")
            .map_err(|err| DfwError::internal(err.to_string()))?;
        let blank = |s: &str| -> String {
            comment
                .replace_all(s, |c: &regex::Captures| " ".repeat(c[0].len()))
                .into_owned()
        };
        if let Some(at) = text.find(&old) {
            let name = |c: regex::Captures| {
                c.get(1)
                    .or_else(|| c.get(2))
                    .map(|m| m.as_str().to_string())
            };
            let within = id.captures(&blank(&old)).and_then(name);
            dfw_id = within.or_else(|| {
                let blanked = blank(&text);
                id.captures_iter(&blanked[..at]).last().and_then(name)
            });
        }
    }
    let note = json!({ "file": rel, "dfwId": dfw_id, "at": now_iso() });
    write_json(&p.dfw.join("presence.json"), &note)?;
    Ok(Some(note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    /// presence: the hook names the element the agent is about to edit
    /// (tests/unit/studio-presence.test.mjs)
    #[test]
    fn presence_the_hook_names_the_element_the_agent_is_about_to_edit() {
        let root = test_dir("presence");
        let file = root.join("design/screens/demo/a.html");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "<main data-dfw-id=\"demo.main\">\n  <h1 data-dfw-id=\"demo.title\">Hello</h1>\n  <p>Plain line</p>\n  <dfw-use c=\"button\" id=\"demo.cta\">Go</dfw-use>\n  <span>After</span>\n</main>\n").unwrap();
        let note = |input: Value, f: &Path| note_presence(&root, &input, f).unwrap();
        let at = |old: &str| note(json!({ "old_string": old }), &file).unwrap();
        let title = at("<h1 data-dfw-id=\"demo.title\">Hello</h1>");
        assert_eq!(title["file"], "screens/demo/a.html");
        assert_eq!(title["dfwId"], "demo.title");
        assert_eq!(
            at("Plain line")["dfwId"],
            "demo.title",
            "nearest id before the text"
        );
        assert_eq!(at("After")["dfwId"], "demo.cta", "an instance id counts");
        assert_eq!(at("not in the file")["dfwId"], Value::Null);
        assert_eq!(
            note(json!({ "edits": [{ "old_string": "Hello" }] }), &file).unwrap()["dfwId"],
            "demo.title",
            "MultiEdit: the first edit"
        );
        assert_eq!(
            note(json!({}), &file).unwrap()["dfwId"],
            Value::Null,
            "Write: the file only"
        );
        assert_eq!(
            note(json!({}), &root.join("src/app.ts")),
            None,
            "not a design file"
        );
        assert_eq!(
            note(json!({}), &root.join("design/approved/v1/a.html")),
            None
        );
        // The note on disk is the one returned.
        let written = fs::read_to_string(root.join(".dfw/presence.json")).unwrap();
        assert!(written.starts_with(
            "{\n  \"file\": \"screens/demo/a.html\",\n  \"dfwId\": null,\n  \"at\": \""
        ));
    }

    /// presence: ids in HTML comments are skipped and single quotes are read
    /// (tests/unit/studio-presence.test.mjs)
    #[test]
    fn presence_ids_in_html_comments_are_skipped_and_single_quotes_are_read() {
        let root = test_dir("presence-comments");
        let file = root.join("design/screens/demo/a.html");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "<main data-dfw-id=\"page\">\n<!-- <section data-dfw-id=\"old.hero\"> removed -->\n<header data-dfw-id='nav'><a>Home</a></header>\n<section data-dfw-id='hero'><h1>Title</h1></section>\n<!-- <p data-dfw-id=\"never.closed\">\n</main>\n").unwrap();
        let at = |old: &str| {
            note_presence(&root, &json!({ "old_string": old }), &file)
                .unwrap()
                .unwrap()["dfwId"]
                .clone()
        };
        assert_eq!(at("<section data-dfw-id='hero'><h1>Title</h1>"), "hero");
        assert_eq!(
            at("<a>Home</a>"),
            "nav",
            "the commented-out id before it is not an element"
        );
        assert_eq!(at("</main>"), "hero", "an unclosed comment hides the rest");
        // "..x.html" is inside design/ (node-stable d: two leading dots are not "outside").
        let dots = root.join("design/..x.html");
        assert_eq!(
            note_presence(&root, &json!({}), &dots).unwrap().unwrap()["file"],
            "..x.html"
        );
    }
}
