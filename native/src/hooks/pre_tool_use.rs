//! Ports `preToolUse` and `notePresence` of `core/lib/hooks.mjs`: the write gate on every tool
//! call, and the note that tells the Studio which element the agent is about to edit.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::host::{HookInput, HookResult};
use crate::core::js;
use crate::core::paths::{env_nonempty, file, find_project_root, project_paths, rel, resolve};
use crate::core::util::{now_iso, write_json_inside};
use crate::design::html::{Attr, Quote, Token, Tokenizer};
use crate::workflow::gates::{decide_write, files_from_tool_input, relative};
use crate::workflow::state::{field, load_state};

use super::payload;

/// An element id as the regular expression of Node found it
/// (`(?:data-dfw-id|<dfw-use\b[^>]*?\bid)=(?:"([^"']+)"|'([^"']+)')`): where that match
/// started and ended, and the id.
struct IdAt {
    start: usize,
    end: usize,
    id: String,
}

/// The value of `name="v"` or `name='v'` written without spaces around `=`, when it is not
/// empty and holds no quote (what `[^"']+` accepted).
fn quoted_id(src: &str, attr: &Attr) -> Option<String> {
    let value = attr.value.clone()?;
    let quoted = matches!(attr.quote, Quote::Double | Quote::Single);
    let value = (quoted && value.start == attr.name.end + 2).then(|| &src[value])?;
    (!value.is_empty() && !value.contains(['"', '\''])).then(|| value.to_string())
}

/// The element ids of a design file in source order, read with the tokenizer: ids in comments
/// are not elements, and a comment that never closes hides the rest of the file. On a
/// `<dfw-use>` the first `id` (or `*-id`) attribute names the instance, as the Node pattern
/// matched from the `<`; on every element `data-dfw-id` names it.
fn element_ids(src: &str) -> Vec<IdAt> {
    let mut out = Vec::new();
    for token in Tokenizer::new(src) {
        let tag = match token {
            Token::Text(range) if src[range.clone()].starts_with("<!--") => break,
            Token::Open(tag) => tag,
            _ => continue,
        };
        let mut instance = tag.name(src) == "dfw-use";
        for attr in &tag.attrs {
            let name = &src[attr.name.clone()];
            let own = name.ends_with("data-dfw-id");
            if !(own || (instance && (name == "id" || name.ends_with("-id")))) {
                continue;
            }
            let Some(id) = quoted_id(src, attr) else {
                continue;
            };
            let start = if own {
                attr.span.start
            } else {
                tag.range.start
            };
            instance = false;
            out.push(IdAt {
                start,
                end: attr.span.end,
                id,
            });
        }
    }
    out
}

/// The session's working directory from the payload, else the process's. A `cwd` that is not
/// text is an error (the hook fails open with it on stderr).
fn session_cwd(input: &HookInput) -> Result<PathBuf, DfwError> {
    match js::truthy_value(input.raw.get(payload::CWD)) {
        Some(Value::String(cwd)) => Ok(PathBuf::from(cwd)),
        Some(other) => Err(DfwError::new(format!(
            "the host sent a \"cwd\" that is not a path ({}); the write gate could not check this call. Update the host CLI or report it to the dfw maintainers.",
            js::to_string(Some(other))
        ))),
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
    if state.get(field::ACTIVE) != Some(&Value::Bool(false)) {
        // Presence is a nicety: it never blocks an edit.
        let _ = note_presence(&root, &input.tool_input, &files[0]);
    }
    Ok(None)
}

/// The key of the presence note that names the element (`data-dfw-id`).
pub(crate) const DFW_ID: &str = "dfwId";

/// The other keys of the presence note (`.dfw/presence.json`).
pub mod presence {
    /// The design file, relative to `design/`.
    pub const FILE: &str = "file";
    /// When the note was written.
    pub const AT: &str = "at";
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
    if rel == ".." || rel.starts_with("../") || rel::under(file::APPROVED, &rel) {
        return Ok(None);
    }
    let first_edit = tool_input
        .get(payload::EDITS)
        .and_then(|e| e.get(0))
        .and_then(|e| e.get(payload::OLD_STRING));
    let old = js::truthy_value(tool_input.get(payload::OLD_STRING))
        .or(js::truthy_value(first_edit))
        .map(|o| js::to_string(Some(o)))
        .unwrap_or_default();
    let mut dfw_id = None;
    if !old.is_empty() && rel.ends_with(".html") && abs_file.exists() {
        let text = String::from_utf8_lossy(&fs::read(abs_file)?).into_owned();
        if let Some(at) = text.find(&old) {
            let ids = element_ids(&text);
            let end = at + old.len();
            dfw_id = ids
                .iter()
                .find(|m| m.start >= at && m.end <= end)
                .or_else(|| ids.iter().rev().find(|m| m.end <= at))
                .map(|m| m.id.clone());
        }
    }
    let note = json!({ (presence::FILE): rel, DFW_ID: dfw_id, (presence::AT): now_iso() });
    write_json_inside(root, &p.presence, &note)?;
    Ok(Some(note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{APP_MODULE, DEMO_PAGE};
    use crate::core::util::test_dir;

    /// presence: the hook names the element the agent is about to edit
    /// (tests/unit/studio-presence.test.mjs)
    #[test]
    fn presence_the_hook_names_the_element_the_agent_is_about_to_edit() {
        let root = test_dir("presence");
        let file = root.join(rel::screen(DEMO_PAGE));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "<main data-dfw-id=\"demo.main\">\n  <h1 data-dfw-id=\"demo.title\">Hello</h1>\n  <p>Plain line</p>\n  <dfw-use c=\"button\" id=\"demo.cta\">Go</dfw-use>\n  <span>After</span>\n</main>\n").unwrap();
        let note = |input: Value, f: &Path| note_presence(&root, &input, f).unwrap();
        let at = |old: &str| note(json!({ "old_string": old }), &file).unwrap();
        /// The id of the heading the edits land in.
        const TITLE: &str = "demo.title";
        let title = at("<h1 data-dfw-id=\"demo.title\">Hello</h1>");
        assert_eq!(title["file"], "screens/demo/a.html");
        assert_eq!(title[DFW_ID], TITLE);
        assert_eq!(
            at("Plain line")[DFW_ID],
            TITLE,
            "nearest id before the text"
        );
        assert_eq!(at("After")[DFW_ID], "demo.cta", "an instance id counts");
        assert_eq!(at("not in the file")[DFW_ID], Value::Null);
        assert_eq!(
            note(json!({ "edits": [{ "old_string": "Hello" }] }), &file).unwrap()[DFW_ID],
            TITLE,
            "MultiEdit: the first edit"
        );
        assert_eq!(
            note(json!({}), &file).unwrap()[DFW_ID],
            Value::Null,
            "Write: the file only"
        );
        assert_eq!(
            note(json!({}), &root.join(APP_MODULE)),
            None,
            "not a design file"
        );
        assert_eq!(
            note(json!({}), &root.join(rel::approved("v1/a.html"))),
            None
        );
        // The note on disk is the one returned.
        let written = fs::read_to_string(root.join(rel::join(rel::DFW, file::PRESENCE))).unwrap();
        assert!(written.starts_with(
            "{\n  \"file\": \"screens/demo/a.html\",\n  \"dfwId\": null,\n  \"at\": \""
        ));
    }

    /// presence: ids in HTML comments are skipped and single quotes are read
    /// (tests/unit/studio-presence.test.mjs)
    #[test]
    fn presence_ids_in_html_comments_are_skipped_and_single_quotes_are_read() {
        let root = test_dir("presence-comments");
        let file = root.join(rel::screen(DEMO_PAGE));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "<main data-dfw-id=\"page\">\n<!-- <section data-dfw-id=\"old.hero\"> removed -->\n<header data-dfw-id='nav'><a>Home</a></header>\n<section data-dfw-id='hero'><h1>Title</h1></section>\n<!-- <p data-dfw-id=\"never.closed\">\n</main>\n").unwrap();
        let at = |old: &str| {
            note_presence(&root, &json!({ "old_string": old }), &file)
                .unwrap()
                .unwrap()[DFW_ID]
                .clone()
        };
        const HERO: &str = "hero";
        assert_eq!(at("<section data-dfw-id='hero'><h1>Title</h1>"), HERO);
        assert_eq!(
            at("<a>Home</a>"),
            "nav",
            "the commented-out id before it is not an element"
        );
        assert_eq!(at("</main>"), HERO, "an unclosed comment hides the rest");
        // "..x.html" is inside design/ (node-stable d: two leading dots are not "outside").
        const DOTS: &str = "..x.html";
        let dots = root.join(rel::design(DOTS));
        assert_eq!(
            note_presence(&root, &json!({}), &dots).unwrap().unwrap()["file"],
            DOTS
        );
    }

    /// The ids the Node pattern read, now read from tags: the first `id` of an instance, every
    /// `data-dfw-id`, no empty or quoted values, nothing after an unclosed comment.
    #[test]
    fn element_ids_read_like_the_node_pattern() {
        let src = "<dfw-use c=\"x\" id=\"\" data-id=\"a\" id=\"b\" data-dfw-id='c'><p data-dfw-id=\"it's\" data-dfw-id = \"sp\" data-dfw-id=\"d\">p</p><!-- <i data-dfw-id=\"e\"> --><b data-dfw-id=\"f\"></b><!-- <i data-dfw-id=\"g\">";
        let ids: Vec<String> = element_ids(src).into_iter().map(|m| m.id).collect();
        assert_eq!(ids, ["a", "c", "d", "f"]);
        let first = &element_ids(src)[0];
        assert_eq!((first.start, &src[first.end - 4..first.end]), (0, "=\"a\""));
    }
}
