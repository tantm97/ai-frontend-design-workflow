//! Ports `core/lib/comments.mjs`: the visual feedback store (the tool-independent feedback
//! contract, `schemas/feedback.schema.json`). One JSON file per review session under
//! `.dfw/reviews/`. Any provider (the Studio overlay, the Penpot bridge, the live review proxy)
//! writes the same shape.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::core::config::{js_spread, js_truthy};
use crate::core::error::DfwError;
use crate::core::paths::project_paths;
use crate::core::util::{iso_from_millis, js_number, now_iso, read_json, write_json};
use crate::workflow::state::{js_str, js_to_number, js_trim, object, truthy, type_error};

/// Every status a comment can have.
pub const STATUSES: [&str; 6] = [
    "open",
    "acknowledged",
    "fixed",
    "resolved",
    "reopened",
    "dismissed",
];

/// Allowed transitions. "fixed" means the agent applied a change; only the user resolves.
const NEXT: [(&str, &[&str]); 6] = [
    ("open", &["acknowledged", "fixed", "dismissed"]),
    ("acknowledged", &["fixed", "dismissed"]),
    ("fixed", &["resolved", "reopened"]),
    ("reopened", &["acknowledged", "fixed", "dismissed"]),
    ("resolved", &["reopened"]),
    ("dismissed", &["reopened"]),
];

/// How long a writer waits for the lock before it takes it over.
const LOCK_WAIT: Duration = Duration::from_millis(5000);
/// A lock older than this was left by a process that died.
const LOCK_STALE: Duration = Duration::from_millis(10_000);
const LOCK_RETRY: Duration = Duration::from_millis(15);

/// `locked(root, fn)`: run `f` holding the reviews lock (`.dfw/reviews/.lock`, a folder). The
/// Studio server and the CLI (hooks, `dfw comments ...`) change the same session files from
/// different processes: every read-modify-write runs under this one lock so no comment or status
/// is lost.
pub fn locked<T>(root: &Path, f: impl FnOnce() -> Result<T, DfwError>) -> Result<T, DfwError> {
    let dir = project_paths(root).reviews;
    fs::create_dir_all(&dir)?;
    let lock = dir.join(".lock");
    let start = Instant::now();
    loop {
        match fs::create_dir(&lock) {
            Ok(()) => break,
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let Ok(modified) = fs::metadata(&lock).and_then(|m| m.modified()) else {
                    continue; // released meanwhile
                };
                let age = SystemTime::now()
                    .duration_since(modified)
                    .unwrap_or_default();
                if age > LOCK_STALE || start.elapsed() > LOCK_WAIT {
                    let _ = fs::remove_dir_all(&lock); // left by a process that died
                    continue;
                }
                thread::sleep(LOCK_RETRY);
            }
            Err(err) => return Err(err.into()),
        }
    }
    let result = f();
    let _ = fs::remove_dir_all(&lock);
    result
}

/// The session files (`rv_*.json`) of a project, oldest first.
pub fn session_files(root: &Path) -> Result<Vec<PathBuf>, DfwError> {
    let dir = project_paths(root).reviews;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut names = Vec::new();
    for entry in fs::read_dir(&dir)? {
        let name = entry?.file_name().to_string_lossy().into_owned();
        if name.starts_with("rv_") && name.ends_with(".json") && name.len() >= "rv_.json".len() {
            names.push(name);
        }
    }
    // JavaScript's default sort compares UTF-16 code units.
    names.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(names.into_iter().map(|n| dir.join(n)).collect())
}

/// A review session and the file it is stored in.
#[derive(Debug, Clone, PartialEq)]
pub struct Session {
    pub file: PathBuf,
    pub session: Value,
}

fn read_session(file: &Path) -> Result<Value, DfwError> {
    read_json(file, None)
}

/// `currentSession(root, { create })`: the open session; with `create`, a new one when the last
/// is closed or there is none. Call it under [`locked`] when `create` is set.
pub fn current_session(root: &Path, create: bool) -> Result<Option<Session>, DfwError> {
    if let Some(last) = session_files(root)?.pop() {
        let session = read_session(&last)?;
        if truthy(session.get("closedAt")).is_none() {
            return Ok(Some(Session {
                file: last,
                session,
            }));
        }
    }
    if !create {
        return Ok(None);
    }
    new_session_unlocked(root, None).map(Some)
}

/// `newSession(root, { designVersion })`: close every open session and start a new one
/// (`dfw comments new-round`).
pub fn new_session(root: &Path, design_version: Option<&str>) -> Result<Session, DfwError> {
    locked(root, || new_session_unlocked(root, design_version))
}

fn new_session_unlocked(root: &Path, design_version: Option<&str>) -> Result<Session, DfwError> {
    // The id is the real time to the second (not DFW_NOW), as in Node: two sessions started in
    // the same second share a file, and the second one replaces the first.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let iso = iso_from_millis(now);
    let stamp: String = iso
        .split('.')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect();
    let id = format!("rv_{stamp}");
    let p = project_paths(root);
    let state = read_json(&p.state, Some(json!({})))?;
    if state.is_null() {
        return Err(DfwError::internal(
            "Cannot read properties of null (reading 'design')",
        ));
    }
    for file in session_files(root)? {
        let mut session = read_session(&file)?;
        if truthy(session.get("closedAt")).is_none() {
            if let Value::Object(map) = &mut session {
                map.insert("closedAt".into(), now_iso().into());
            }
            write_json(&file, &session)?;
        }
    }
    let version = match design_version.filter(|v| !v.is_empty()) {
        Some(v) => v.to_string(),
        None => {
            let v = state
                .get("design")
                .and_then(|d| d.get("version"))
                .filter(|v| !v.is_null());
            format!("v{}", v.map_or("1".to_string(), |v| js_str(Some(v))))
        }
    };
    let session = json!({
        "schema": 1,
        "id": id,
        "startedAt": now_iso(),
        "designVersion": version,
        "comments": [],
    });
    let file = p.reviews.join(format!("{id}.json"));
    write_json(&file, &session)?;
    Ok(Session { file, session })
}

fn comments_of(session: &Value) -> Result<&Vec<Value>, DfwError> {
    session
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| DfwError::internal("s.comments is not iterable"))
}

/// `allComments(root)`: every comment of every session, oldest first, each with the id of its
/// session as `session`.
pub fn all_comments(root: &Path) -> Result<Vec<Value>, DfwError> {
    let mut out = Vec::new();
    for file in session_files(root)? {
        let session = read_session(&file)?;
        for c in comments_of(&session)? {
            let mut c = js_spread(Some(c));
            match session.get("id") {
                Some(id) => {
                    c.insert("session".into(), id.clone());
                }
                None => {
                    c.shift_remove("session");
                }
            }
            out.push(Value::Object(c));
        }
    }
    Ok(out)
}

fn next_id(root: &Path) -> Result<String, DfwError> {
    let mut max = 0.0_f64;
    for c in all_comments(root)? {
        let id = js_str(c.get("id"));
        let n = js_to_number(id.strip_prefix("fb_").unwrap_or(&id));
        if n > max {
            max = n;
        }
    }
    Ok(format!("fb_{:0>3}", js_number(max + 1.0)))
}

/// `locateSource(root, designFile, dfwId)`: `design/<file>:<line>` of `data-dfw-id="<id>"` in a
/// design file, for source mapping; the file alone when the id is not in it; `None` when the
/// file does not exist. Only reads; the file name is not checked against the project.
pub fn locate_source(
    root: &Path,
    design_file: Option<&str>,
    dfw_id: Option<&str>,
) -> Result<Option<String>, DfwError> {
    let Some(design_file) = design_file.filter(|f| !f.is_empty()) else {
        return Ok(None);
    };
    let abs = project_paths(root).design.join(design_file);
    if !abs.exists() {
        return Ok(None);
    }
    let text = String::from_utf8_lossy(&fs::read(&abs)?).into_owned();
    if let Some(id) = dfw_id.filter(|id| !id.is_empty()) {
        if let Some(i) = text.find(&format!("data-dfw-id=\"{id}\"")) {
            let line = text[..i].matches('\n').count() + 1;
            return Ok(Some(format!("design/{design_file}:{line}")));
        }
    }
    Ok(Some(format!("design/{design_file}")))
}

/// `addComment(root, input)`: store a new open comment in the current session and return it.
/// `input` is the feedback a provider posts: `comment` (required), `screen`, `variant`,
/// `breakpoint`, `state`, `theme`, `file` (relative to `design/`), `target`, `screenshot`,
/// `provider`, `url`.
pub fn add_comment(root: &Path, input: &Value) -> Result<Value, DfwError> {
    locked(root, || add_comment_unlocked(root, input))
}

fn add_comment_unlocked(root: &Path, input: &Value) -> Result<Value, DfwError> {
    let text = match input.get("comment") {
        None | Some(Value::Null) => "",
        Some(Value::String(text)) => js_trim(text),
        Some(_) => return Err(DfwError::internal("input?.comment?.trim is not a function")),
    };
    if text.is_empty() {
        return Err(DfwError::new("A comment needs text."));
    }
    let Some(Session { file, mut session }) = current_session(root, true)? else {
        return Err(DfwError::internal("No review session."));
    };
    let design_file = match truthy(input.get("file")) {
        None => None,
        Some(Value::String(f)) => Some(f.as_str()),
        Some(_) => {
            return Err(DfwError::internal(
                "The \"path\" argument must be of type string.",
            ))
        }
    };
    let mut target = js_spread(truthy(input.get("target")));
    if truthy(target.get("source")).is_none() {
        let dfw_id = truthy(target.get("dfwId")).map(|id| js_str(Some(id)));
        let source = locate_source(root, design_file, dfw_id.as_deref())?;
        target.insert("source".into(), source.map_or(Value::Null, Value::String));
    }
    let or_null = |key: &str| truthy(input.get(key)).cloned().unwrap_or(Value::Null);
    let mut c = object(vec![
        ("id", Some(next_id(root)?.into())),
        ("createdAt", Some(now_iso().into())),
        ("designVersion", session.get("designVersion").cloned()),
        ("screen", Some(or_null("screen"))),
        ("variant", Some(or_null("variant"))),
        ("breakpoint", Some(or_null("breakpoint"))),
        ("state", Some(or_null("state"))),
        ("theme", Some(or_null("theme"))),
        (
            "file",
            Some(design_file.map_or(Value::Null, |f| format!("design/{f}").into())),
        ),
        ("target", Some(Value::Object(target))),
        ("comment", Some(text.into())),
        ("screenshot", Some(or_null("screenshot"))),
        ("status", Some("open".into())),
        ("thread", Some(json!([]))),
        ("resolution", Some(Value::Null)),
        (
            "provider",
            Some(
                truthy(input.get("provider"))
                    .cloned()
                    .unwrap_or_else(|| "dfw-studio".into()),
            ),
        ),
    ]);
    // Comments on the running app (dfw review) carry the page they were made on.
    if let Some(url) = truthy(input.get("url")) {
        c["url"] = js_str(Some(url)).into();
    }
    match session.get_mut("comments") {
        Some(Value::Array(comments)) => comments.push(c.clone()),
        _ => return Err(type_error("push")),
    }
    write_json(&file, &session)?;
    Ok(c)
}

/// Where a comment is: its session file, the session and the comment's index in it.
struct Found {
    file: PathBuf,
    session: Value,
    index: usize,
}

impl Found {
    fn comment(&mut self) -> Result<&mut Map<String, Value>, DfwError> {
        match self
            .session
            .get_mut("comments")
            .and_then(|c| c.get_mut(self.index))
        {
            Some(Value::Object(comment)) => Ok(comment),
            _ => Err(DfwError::internal("The comment is not an object.")),
        }
    }

    fn save(mut self) -> Result<Value, DfwError> {
        write_json(&self.file, &self.session)?;
        Ok(Value::Object(self.comment()?.clone()))
    }
}

fn find_comment(root: &Path, id: Option<&str>) -> Result<Found, DfwError> {
    for file in session_files(root)? {
        let session = read_session(&file)?;
        let index = comments_of(&session)?
            .iter()
            .position(|c| id.is_some() && c.get("id").and_then(Value::as_str) == id);
        if let Some(index) = index {
            return Ok(Found {
                file,
                session,
                index,
            });
        }
    }
    Err(DfwError::new(format!(
        "Comment {} not found.",
        id.unwrap_or("undefined")
    )))
}

/// `setCommentScreenshot(root, id, file)`: attach a screenshot taken after the comment was
/// posted (dfw review shoots asynchronously).
// Its caller is the live review proxy (studio group): this allow goes when that is ported.
#[allow(dead_code)]
pub fn set_comment_screenshot(root: &Path, id: &str, file: &str) -> Result<Value, DfwError> {
    locked(root, || {
        let mut found = find_comment(root, Some(id))?;
        found.comment()?.insert("screenshot".into(), file.into());
        found.save()
    })
}

/// What [`update_comment`] changes. A field that is `None` (or not truthy in JavaScript) is
/// left alone; `by` then is `claude`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Update {
    /// The new status; the transition must be allowed.
    pub status: Option<String>,
    /// A line for the comment's thread; the note of the change for `fixed`.
    pub message: Option<Value>,
    /// Who makes the change (`user`, `claude`, a provider).
    pub by: Option<Value>,
    /// Proof for `fixed` (a screenshot path, a check result).
    pub evidence: Option<Value>,
}

/// `updateComment(root, id, { status, message, by, evidence })`: change a comment's status
/// and/or add a line to its thread; returns the comment. `id` is `None` when the command line
/// named none (the error then says `undefined`, as in Node).
pub fn update_comment(root: &Path, id: Option<&str>, update: &Update) -> Result<Value, DfwError> {
    locked(root, || update_comment_unlocked(root, id, update))
}

fn update_comment_unlocked(
    root: &Path,
    id: Option<&str>,
    update: &Update,
) -> Result<Value, DfwError> {
    let mut found = find_comment(root, id)?;
    let id = id.unwrap_or("undefined");
    let comment = found.comment()?;
    let at = now_iso();
    let status = update.status.as_deref().filter(|s| !s.is_empty());
    let message = update.message.clone().filter(js_truthy);
    let by = update.by.clone().unwrap_or_else(|| "claude".into());
    let current = comment.get("status").cloned();
    if let Some(status) = status.filter(|s| current.as_ref().and_then(Value::as_str) != Some(s)) {
        if !STATUSES.contains(&status) {
            return Err(DfwError::new(format!("Unknown status \"{status}\".")));
        }
        let allowed = current
            .as_ref()
            .and_then(Value::as_str)
            .and_then(|c| NEXT.iter().find(|(from, _)| *from == c))
            .map(|(_, to)| *to)
            .ok_or_else(|| type_error("includes"))?;
        if !allowed.contains(&status) {
            return Err(DfwError::new(format!(
                "Comment {id}: cannot go from \"{}\" to \"{status}\" (allowed: {}).",
                js_str(current.as_ref()),
                allowed.join(", ")
            )));
        }
        if status == "resolved" && by.as_str() != Some("user") {
            return Err(DfwError::new("Only the user resolves a comment (from the studio, or \"dfw comments resolve --by user\" when they say so)."));
        }
        if status == "fixed" && message.is_none() {
            return Err(DfwError::new(
                "Marking a comment fixed needs a short note describing the change.",
            ));
        }
        comment.insert("status".into(), status.into());
        if status == "fixed" {
            let evidence = update.evidence.clone().filter(js_truthy);
            comment.insert(
                "resolution".into(),
                json!({ "at": at, "by": by, "note": message, "evidence": evidence }),
            );
        }
    }
    if let Some(message) = message {
        let line = object(vec![
            ("at", Some(at.clone().into())),
            ("by", Some(by)),
            ("message", Some(message)),
            ("status", status.map(Value::from)),
        ]);
        match comment.get_mut("thread") {
            Some(Value::Array(thread)) => thread.push(line),
            _ => return Err(type_error("push")),
        }
    }
    comment.insert("updatedAt".into(), at.into());
    found.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use crate::workflow::state::{new_state, save_state};

    fn project(label: &str) -> PathBuf {
        let root = test_dir(label);
        save_state(&root, &mut new_state("new-product", "".into()).unwrap()).unwrap();
        root
    }

    fn update(status: &str, message: Option<&str>, by: Option<&str>) -> Update {
        Update {
            status: Some(status.into()).filter(|s: &String| !s.is_empty()),
            message: message.map(Value::from),
            by: by.map(Value::from),
            evidence: None,
        }
    }

    /// feedback lifecycle: only the user resolves (tests/unit/dfw.test.mjs)
    #[test]
    fn feedback_lifecycle_only_the_user_resolves() {
        let root = project("comments");
        fs::create_dir_all(root.join("design/screens/home")).unwrap();
        fs::write(
            root.join("design/screens/home/a.html"),
            "<html>\n<body>\n<button data-dfw-id=\"home.cta\">Go</button>\n</body></html>",
        )
        .unwrap();
        let c = add_comment(&root, &json!({ "screen": "home", "variant": "a", "file": "screens/home/a.html", "target": { "dfwId": "home.cta" }, "comment": "Bigger" })).unwrap();
        assert_eq!(c["id"], "fb_001");
        assert_eq!(c["target"]["source"], "design/screens/home/a.html:3");
        let id = Some("fb_001");
        let fails = |u: Update| {
            update_comment(&root, id, &u)
                .unwrap_err()
                .message()
                .to_string()
        };
        assert!(fails(update("resolved", None, Some("user"))).contains("cannot go from \"open\""));
        assert!(fails(update("fixed", None, None)).contains("needs a short note"));
        update_comment(&root, id, &update("fixed", Some("done"), None)).unwrap();
        assert!(fails(update("resolved", None, None)).contains("Only the user"));
        assert!(fails(update("dismissed", None, None)).contains("cannot go from"));
        update_comment(&root, id, &update("resolved", None, Some("user"))).unwrap();
        assert_eq!(all_comments(&root).unwrap()[0]["status"], "resolved");
        assert_eq!(
            add_comment(&root, &json!({ "comment": "second" })).unwrap()["id"],
            "fb_002"
        );
        assert!(!root.join(".dfw/reviews/.lock").exists());
    }

    /// layout: ... comment source line points at the element, not its layout rule
    /// (tests/unit/edit.test.mjs: the assertion on locateSource; the rest is design/edit)
    #[test]
    fn comment_source_line_points_at_the_element_not_its_layout_rule() {
        let root = test_dir("locate-source");
        fs::create_dir_all(root.join("design/screens/dash")).unwrap();
        let text = "<html><head>\n<style data-dfw-layout>[data-dfw-id='dash.a'] { width: 50% }</style>\n</head><body>\n<div data-dfw-id=\"dash.a\">A</div>\n</body></html>";
        fs::write(root.join("design/screens/dash/index.html"), text).unwrap();
        let at =
            |id: Option<&str>| locate_source(&root, Some("screens/dash/index.html"), id).unwrap();
        assert_eq!(
            at(Some("dash.a")).as_deref(),
            Some("design/screens/dash/index.html:4")
        );
        assert_eq!(
            at(Some("nope")).as_deref(),
            Some("design/screens/dash/index.html")
        );
        assert_eq!(at(None).as_deref(), Some("design/screens/dash/index.html"));
        assert_eq!(
            locate_source(&root, Some("screens/none.html"), Some("x")).unwrap(),
            None
        );
        assert_eq!(locate_source(&root, None, Some("x")).unwrap(), None);
    }

    #[test]
    fn a_comment_has_the_node_shape() {
        let root = project("comments-shape");
        let c = add_comment(&root, &json!({ "comment": "  Too wide \n", "url": "http://localhost:3000/a", "provider": "dfw-live", "target": { "selector": "main > p", "source": "src/App.tsx:3:5" } })).unwrap();
        let keys: Vec<&str> = c.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "id",
                "createdAt",
                "designVersion",
                "screen",
                "variant",
                "breakpoint",
                "state",
                "theme",
                "file",
                "target",
                "comment",
                "screenshot",
                "status",
                "thread",
                "resolution",
                "provider",
                "url"
            ]
        );
        assert_eq!(c["comment"], "Too wide");
        assert_eq!(c["designVersion"], "v1");
        assert_eq!(c["target"]["source"], "src/App.tsx:3:5");
        assert_eq!(c["file"], Value::Null);
        let plain = add_comment(&root, &json!({ "comment": "x" })).unwrap();
        assert_eq!(plain["target"], json!({ "source": null }));
        assert_eq!(plain["provider"], "dfw-studio");
        for input in [
            json!({}),
            json!({ "comment": "  " }),
            json!(true),
            Value::Null,
        ] {
            assert_eq!(
                add_comment(&root, &input).unwrap_err().message(),
                "A comment needs text."
            );
        }
        let all = all_comments(&root).unwrap();
        assert_eq!(all.len(), 2);
        assert!(all[0]["session"].as_str().unwrap().starts_with("rv_"));
    }

    #[test]
    fn every_status_rule() {
        let root = project("comments-status");
        add_comment(&root, &json!({ "comment": "x" })).unwrap();
        let id = Some("fb_001");
        let go = |status: &str, message: Option<&str>, by: Option<&str>| {
            update_comment(&root, id, &update(status, message, by))
        };
        let status = || all_comments(&root).unwrap()[0]["status"].clone();
        // A reply keeps the status and adds a thread line without one.
        let c = go("", Some("Which one?"), None).unwrap();
        assert_eq!(
            c["thread"][0],
            json!({ "at": c["updatedAt"], "by": "claude", "message": "Which one?" })
        );
        assert_eq!(status(), "open");
        // The same status again is not a transition.
        go("open", None, None).unwrap();
        for (from, allowed) in NEXT {
            for to in STATUSES {
                // Bring the comment to `from` by a legal path, then try `to`.
                let path: &[&str] = match from {
                    "open" => &[],
                    "acknowledged" => &["acknowledged"],
                    "fixed" => &["fixed"],
                    "reopened" => &["fixed", "reopened"],
                    "resolved" => &["fixed", "resolved"],
                    _ => &["dismissed"],
                };
                let fresh =
                    add_comment(&root, &json!({ "comment": format!("{from}>{to}") })).unwrap();
                let cid = fresh["id"].as_str();
                for step in path {
                    update_comment(&root, cid, &update(step, Some("m"), Some("user"))).unwrap();
                }
                let result = update_comment(&root, cid, &update(to, Some("m"), Some("user")));
                assert_eq!(
                    result.is_ok(),
                    to == from || allowed.contains(&to),
                    "{from} -> {to}"
                );
            }
        }
        let fixed = go("fixed", Some("lg size"), None).unwrap();
        assert_eq!(
            fixed["resolution"],
            json!({ "at": fixed["updatedAt"], "by": "claude", "note": "lg size", "evidence": null })
        );
        assert_eq!(fixed["thread"][1]["status"], "fixed");
        assert_eq!(
            update_comment(&root, Some("fb_999"), &Update::default())
                .unwrap_err()
                .message(),
            "Comment fb_999 not found."
        );
        assert_eq!(
            update_comment(&root, None, &Update::default())
                .unwrap_err()
                .message(),
            "Comment undefined not found."
        );
        let shot = set_comment_screenshot(&root, "fb_001", ".dfw/shots/fb_001.png").unwrap();
        assert_eq!(shot["screenshot"], ".dfw/shots/fb_001.png");
    }

    #[test]
    fn a_new_round_closes_the_open_session() {
        let root = project("comments-round");
        assert!(current_session(&root, false).unwrap().is_none());
        add_comment(&root, &json!({ "comment": "first" })).unwrap();
        let first = current_session(&root, false).unwrap().unwrap();
        // A session id is the time to the second: write the first one as an older file.
        let old = root.join(".dfw/reviews/rv_20200101T000000.json");
        fs::rename(&first.file, &old).unwrap();
        let second = new_session(&root, Some("v7")).unwrap();
        assert_eq!(second.session["designVersion"], "v7");
        assert!(read_session(&old).unwrap()["closedAt"].is_string());
        assert_eq!(
            add_comment(&root, &json!({ "comment": "second" })).unwrap()["id"],
            "fb_002",
            "ids count across sessions"
        );
        assert_eq!(session_files(&root).unwrap().len(), 2);
    }

    // Setting the time of a folder needs a handle to it, which Windows does not give this way.
    #[cfg(unix)]
    #[test]
    fn a_stale_lock_is_taken_over() {
        let root = project("comments-lock");
        let lock = root.join(".dfw/reviews/.lock");
        fs::create_dir_all(&lock).unwrap();
        let long_ago = SystemTime::now() - Duration::from_secs(60);
        fs::File::open(&lock)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        add_comment(&root, &json!({ "comment": "after a crash" })).unwrap();
        assert!(!lock.exists());
        // The lock is released when the work under it fails.
        assert!(locked(&root, || Err::<(), _>(DfwError::new("boom"))).is_err());
        assert!(!lock.exists());
    }
}
