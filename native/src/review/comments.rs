//! Ports `core/lib/comments.mjs`: the visual feedback store (the tool-independent feedback
//! contract, `schemas/feedback.schema.json`). One JSON file per review session under
//! `.dfw/reviews/`. Any provider (the Studio overlay, the Penpot bridge, the live review proxy)
//! writes the same shape.
//!
//! Over 800 lines only with its tests (the ported cases of the JavaScript tests sit next to the code).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::lock::{self, LockTimes};
use crate::core::paths::{file, project_paths, rel};
use crate::core::util::{iso_from_millis, now_iso, read_json, write_json_inside};
use crate::review::status::{self, field, Status};

/// `locked(root, fn)`: run `f` holding the reviews lock (`.dfw/reviews/.lock`, see
/// `core::lock`). The Studio server and the CLI (hooks, `dfw comments ...`) change the same
/// session files from different processes: every read-modify-write runs under this one lock so
/// no comment or status is lost.
pub fn locked<T>(root: &Path, f: impl FnOnce() -> Result<T, DfwError>) -> Result<T, DfwError> {
    locked_with(root, &lock::STANDARD, f)
}

fn locked_with<T>(
    root: &Path,
    times: &LockTimes,
    f: impl FnOnce() -> Result<T, DfwError>,
) -> Result<T, DfwError> {
    let _held = lock::acquire(&lock_path(root), times)?;
    f()
}

/// [`locked`] for a hook: gives up after `give_up` and returns `None` without running `f`
/// (`process-and-io.md` P8).
pub fn try_locked<T>(
    root: &Path,
    give_up: Duration,
    f: impl FnOnce() -> Result<T, DfwError>,
) -> Result<Option<T>, DfwError> {
    match lock::acquire_within(&lock_path(root), &lock::STANDARD, Some(give_up))? {
        Some(_held) => f().map(Some),
        None => Ok(None),
    }
}

fn lock_path(root: &Path) -> PathBuf {
    project_paths(root).reviews.join(file::REVIEWS_LOCK)
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
        if js::truthy_value(session.get("closedAt")).is_none() {
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
    // The id is the real time to the second (not DFW_NOW), as in Node. When that file exists,
    // move on one second at a time until the name is free, so a second round in the same second
    // never replaces the first.
    let reviews = project_paths(root).reviews;
    let mut t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let id = loop {
        let id = format!("rv_{}", session_stamp(t));
        if !reviews.join(format!("{id}.json")).exists() {
            break id;
        }
        t += 1000;
    };
    let p = project_paths(root);
    let state = read_json(&p.state, Some(json!({})))?;
    if state.is_null() {
        return Err(DfwError::new(
            format!("{} holds null instead of the workflow state. Restore it from version control (or remove it and run \"dfw init\"), then run the command again.", rel::STATE),
        ));
    }
    for file in session_files(root)? {
        let mut session = read_session(&file)?;
        if js::truthy_value(session.get("closedAt")).is_none() {
            if let Value::Object(map) = &mut session {
                map.insert("closedAt".into(), now_iso().into());
            }
            write_json_inside(root, &file, &session)?;
        }
    }
    let version = match design_version.filter(|v| !v.is_empty()) {
        Some(v) => v.to_string(),
        None => {
            let v = state
                .get("design")
                .and_then(|d| d.get("version"))
                .filter(|v| !v.is_null());
            format!("v{}", v.map_or("1".to_string(), |v| js::to_string(Some(v))))
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
    write_json_inside(root, &file, &session)?;
    Ok(Session { file, session })
}

/// `2026-11-18T10:20:30.123Z` as `20261118T102030`.
fn session_stamp(millis: i64) -> String {
    let iso = iso_from_millis(millis);
    iso.split('.')
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !matches!(c, '-' | ':'))
        .collect()
}

fn comments_of<'a>(file: &Path, session: &'a Value) -> Result<&'a Vec<Value>, DfwError> {
    session
        .get("comments")
        .and_then(Value::as_array)
        .ok_or_else(|| not_a_list(file, "\"comments\""))
}

/// A review session file whose list is something else: name the file, the field and the fix.
fn not_a_list(file: &Path, what: &str) -> DfwError {
    DfwError::new(format!(
        "{}: {what} is not a list. Fix that review session file (or move it out of {}/) and run the command again.",
        shown(file),
        rel::REVIEWS
    ))
}

/// A session file as the user finds it: `.dfw/reviews/<name>`.
fn shown(file: &Path) -> String {
    let name = file.file_name().unwrap_or_default().to_string_lossy();
    rel::join(rel::REVIEWS, &name)
}

/// `allComments(root)`: every comment of every session, oldest first, each with the id of its
/// session as `session`.
pub fn all_comments(root: &Path) -> Result<Vec<Value>, DfwError> {
    let mut out = Vec::new();
    for file in session_files(root)? {
        let session = read_session(&file)?;
        for c in comments_of(&file, &session)? {
            let mut c = js::spread(Some(c));
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
        let id = js::to_string(c.get("id"));
        let n = js::str_to_number(id.strip_prefix("fb_").unwrap_or(&id));
        if n > max {
            max = n;
        }
    }
    Ok(format!("fb_{:0>3}", js::number(max + 1.0)))
}

/// `safeDesignFile(f)`: a design file name from input (Studio body, Penpot JSON): relative, no
/// `..` segment, no drive or NUL.
pub fn safe_design_file(f: &str) -> bool {
    let b = f.as_bytes();
    let rooted = matches!(b.first(), Some(b'/' | b'\\'));
    let drive = b.len() >= 2 && b[0].is_ascii_alphabetic() && b[1] == b':';
    !f.is_empty()
        && !rooted
        && !drive
        && !f.contains('\0')
        && !f.split(['/', '\\']).any(|p| p == "..")
}

/// `locateSource(root, designFile, dfwId)`: `design/<file>:<line>` of `data-dfw-id="<id>"` in a
/// design file, for source mapping; the file alone when the id is not in it; `None` when the
/// file does not exist or its name is not a safe design file name.
pub fn locate_source(
    root: &Path,
    design_file: Option<&str>,
    dfw_id: Option<&str>,
) -> Result<Option<String>, DfwError> {
    let Some(design_file) = design_file.filter(|f| safe_design_file(f)) else {
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
            return Ok(Some(format!("{}:{line}", rel::design(design_file))));
        }
    }
    Ok(Some(rel::design(design_file)))
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
        Some(Value::String(text)) => js::trim(text),
        Some(_) => {
            return Err(DfwError::new(
                "The comment must be text: give \"comment\" as a string.",
            ))
        }
    };
    if text.is_empty() {
        return Err(DfwError::new("A comment needs text."));
    }
    let Some(Session { file, mut session }) = current_session(root, true)? else {
        return Err(DfwError::internal("No review session."));
    };
    let design_file = input
        .get("file")
        .and_then(Value::as_str)
        .filter(|f| safe_design_file(f));
    let mut target = js::spread(js::truthy_value(input.get("target")));
    if js::truthy_value(target.get("source")).is_none() {
        let dfw_id = js::truthy_value(target.get("dfwId")).map(|id| js::to_string(Some(id)));
        let source = locate_source(root, design_file, dfw_id.as_deref())?;
        target.insert("source".into(), source.map_or(Value::Null, Value::String));
    }
    let or_null = |key: &str| {
        js::truthy_value(input.get(key))
            .cloned()
            .unwrap_or(Value::Null)
    };
    let mut c = js::object(vec![
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
            Some(design_file.map_or(Value::Null, |f| rel::design(f).into())),
        ),
        ("target", Some(Value::Object(target))),
        ("comment", Some(text.into())),
        ("screenshot", Some(or_null("screenshot"))),
        (field::STATUS, Some(Status::Open.as_str().into())),
        ("thread", Some(json!([]))),
        ("resolution", Some(Value::Null)),
        (
            "provider",
            Some(
                js::truthy_value(input.get("provider"))
                    .cloned()
                    .unwrap_or_else(|| "dfw-studio".into()),
            ),
        ),
    ]);
    // A Studio comment is a draft until the user sends the drafts to Claude (send_drafts): the
    // inbox does not deliver it before that.
    if input.get("draft") == Some(&Value::Bool(true)) {
        c["draft"] = Value::Bool(true);
    }
    // Comments on the running app (dfw review) carry the page they were made on.
    if let Some(url) = js::truthy_value(input.get("url")) {
        c["url"] = js::to_string(Some(url)).into();
    }
    match session.get_mut("comments") {
        Some(Value::Array(comments)) => comments.push(c.clone()),
        _ => return Err(not_a_list(&file, "\"comments\"")),
    }
    write_json_inside(root, &file, &session)?;
    Ok(c)
}

/// `sendDrafts(root)`: the user hands the Studio drafts to Claude. Every draft loses its `draft`
/// flag and gets `sentAt`, under the reviews lock; returns the comments sent (the inbox delivers
/// them from now on).
pub fn send_drafts(root: &Path) -> Result<Vec<Value>, DfwError> {
    locked(root, || {
        let mut sent = Vec::new();
        let at = now_iso();
        for file in session_files(root)? {
            let mut session = read_session(&file)?;
            let mut changed = false;
            match session.get_mut("comments") {
                Some(Value::Array(comments)) => {
                    for c in comments.iter_mut() {
                        if let Value::Object(map) = c {
                            if map.get("draft") == Some(&Value::Bool(true)) {
                                map.shift_remove("draft");
                                map.insert("sentAt".into(), at.clone().into());
                                sent.push(Value::Object(map.clone()));
                                changed = true;
                            }
                        }
                    }
                }
                _ => return Err(not_a_list(&file, "\"comments\"")),
            }
            if changed {
                write_json_inside(root, &file, &session)?;
            }
        }
        Ok(sent)
    })
}

/// Where a comment is: its session file, the session and the comment's index in it.
struct Found {
    root: PathBuf,
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
            _ => Err(DfwError::new(format!(
                "{}: comment {} of \"comments\" is not an object. Fix that review session file (or move it out of {}/) and run the command again.",
                shown(&self.file),
                self.index + 1,
                rel::REVIEWS
            ))),
        }
    }

    fn save(mut self) -> Result<Value, DfwError> {
        write_json_inside(&self.root, &self.file, &self.session)?;
        Ok(Value::Object(self.comment()?.clone()))
    }
}

fn find_comment(root: &Path, id: Option<&str>) -> Result<Found, DfwError> {
    for file in session_files(root)? {
        let session = read_session(&file)?;
        let index = comments_of(&file, &session)?
            .iter()
            .position(|c| id.is_some() && c.get("id").and_then(Value::as_str) == id);
        if let Some(index) = index {
            return Ok(Found {
                root: root.to_path_buf(),
                file,
                session,
                index,
            });
        }
    }
    Err(DfwError::new(match id {
        Some(id) => format!("Comment {id} not found."),
        None => {
            "No comment id given: name the comment by its id (\"dfw comments list\" shows the ids)."
                .to_string()
        }
    }))
}

/// `setCommentScreenshot(root, id, file)`: attach a screenshot taken after the comment was
/// posted (dfw review shoots asynchronously).
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
/// and/or add a line to its thread; returns the comment. `id` is `None` when the caller has
/// none (a Penpot thread whose comment lost its id): the error says so.
pub fn update_comment(root: &Path, id: Option<&str>, update: &Update) -> Result<Value, DfwError> {
    locked(root, || update_comment_unlocked(root, id, update))
}

fn update_comment_unlocked(
    root: &Path,
    id: Option<&str>,
    update: &Update,
) -> Result<Value, DfwError> {
    let mut found = find_comment(root, id)?;
    // Found, so the id was given.
    let id = id.unwrap_or_default();
    let file = found.file.clone();
    let comment = found.comment()?;
    let at = now_iso();
    let status = update.status.as_deref().filter(|s| !s.is_empty());
    let message = update.message.clone().filter(|v| js::truthy(v));
    let by = update.by.clone().unwrap_or_else(|| "claude".into());
    let current = comment.get(field::STATUS).cloned();
    // The status the comment already has is not a transition: it is refused like any other one
    // not in `Status::next`.
    if let Some(name) = status {
        let Some(status) = Status::parse(name) else {
            return Err(DfwError::new(format!("Unknown status \"{name}\".")));
        };
        let allowed = current
            .as_ref()
            .and_then(Value::as_str)
            .and_then(Status::parse)
            .map(Status::next)
            .ok_or_else(|| {
                DfwError::new(format!(
                    "Comment {id} in {} has the status {}, which is not one of {}. Correct it in that file and run the command again.",
                    shown(&file),
                    current.as_ref().map_or("none".into(), crate::core::util::to_json_compact),
                    status::names(&Status::ALL)
                ))
            })?;
        if !allowed.contains(&status) {
            return Err(DfwError::new(format!(
                "Comment {id}: cannot go from \"{}\" to \"{name}\" (allowed: {}).",
                js::to_string(current.as_ref()),
                status::names(allowed)
            )));
        }
        if status == Status::Resolved && by.as_str() != Some("user") {
            return Err(DfwError::new("Only the user resolves a comment (from the studio, or \"dfw comments resolve --by user\" when they say so)."));
        }
        if status == Status::Fixed && message.is_none() {
            return Err(DfwError::new(
                "Marking a comment fixed needs a short note describing the change.",
            ));
        }
        comment.insert(field::STATUS.into(), name.into());
        if status == Status::Fixed {
            let evidence = update.evidence.clone().filter(|v| js::truthy(v));
            comment.insert(
                "resolution".into(),
                json!({ "at": at, "by": by, "note": message, "evidence": evidence }),
            );
        }
    }
    if let Some(message) = message {
        let line = js::object(vec![
            ("at", Some(at.clone().into())),
            ("by", Some(by)),
            ("message", Some(message)),
            ("status", status.map(Value::from)),
        ]);
        match comment.get_mut("thread") {
            Some(Value::Array(thread)) => thread.push(line),
            _ => return Err(not_a_list(&file, &format!("\"thread\" of comment {id}"))),
        }
    }
    comment.insert("updatedAt".into(), at.into());
    found.save()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{
        index_page, APP_COMPONENT, OUTSIDE_PAGE, REVIEW_SESSION, SCREEN, SCREEN_PAGE,
    };
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
        fs::create_dir_all(root.join(rel::screen(SCREEN))).unwrap();
        fs::write(
            root.join(rel::screen(SCREEN_PAGE)),
            "<html>\n<body>\n<button data-dfw-id=\"home.cta\">Go</button>\n</body></html>",
        )
        .unwrap();
        let c = add_comment(&root, &json!({ "screen": "home", "variant": "a", "file": "screens/home/a.html", "target": { "dfwId": "home.cta" }, "comment": "Bigger" })).unwrap();
        assert_eq!(c["id"], "fb_001");
        assert_eq!(
            c["target"]["source"],
            format!("{}:3", rel::screen(SCREEN_PAGE))
        );
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
        assert!(!root
            .join(rel::join(rel::REVIEWS, file::REVIEWS_LOCK))
            .exists());
    }

    /// layout: ... comment source line points at the element, not its layout rule
    /// (tests/unit/edit.test.mjs: the assertion on locateSource; the rest is design/edit)
    #[test]
    fn comment_source_line_points_at_the_element_not_its_layout_rule() {
        const DASH: &str = "dash";
        let page = rel::screen(&index_page(DASH));
        let root = test_dir("locate-source");
        fs::create_dir_all(root.join(rel::screen(DASH))).unwrap();
        let text = "<html><head>\n<style data-dfw-layout>[data-dfw-id='dash.a'] { width: 50% }</style>\n</head><body>\n<div data-dfw-id=\"dash.a\">A</div>\n</body></html>";
        fs::write(root.join(&page), text).unwrap();
        let at =
            |id: Option<&str>| locate_source(&root, Some("screens/dash/index.html"), id).unwrap();
        assert_eq!(
            at(Some("dash.a")).as_deref(),
            Some(format!("{page}:4").as_str())
        );
        assert_eq!(at(Some("nope")).as_deref(), Some(page.as_str()));
        assert_eq!(at(None).as_deref(), Some(page.as_str()));
        assert_eq!(
            locate_source(&root, Some("screens/none.html"), Some("x")).unwrap(),
            None
        );
        assert_eq!(locate_source(&root, None, Some("x")).unwrap(), None);
    }

    #[test]
    fn a_comment_has_the_node_shape() {
        let root = project("comments-shape");
        let c = add_comment(&root, &json!({ "comment": "  Too wide \n", "url": "http://localhost:3000/a", "provider": "dfw-live", "target": { "selector": "main > p", "source": format!("{APP_COMPONENT}:3:5") } })).unwrap();
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
        assert_eq!(c["target"]["source"], format!("{APP_COMPONENT}:3:5"));
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
        // The same status again is refused like any move not in `Status::next`.
        assert_eq!(
            go("open", None, None).unwrap_err().message(),
            "Comment fb_001: cannot go from \"open\" to \"open\" (allowed: acknowledged, fixed, dismissed)."
        );
        for from_status in Status::ALL {
            let (from, allowed) = (from_status.as_str(), from_status.next());
            for to_status in Status::ALL {
                let to = to_status.as_str();
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
                    allowed.contains(&to_status),
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
            "No comment id given: name the comment by its id (\"dfw comments list\" shows the ids)."
        );
        const SHOT: &str = "fb_001.png";
        let file = rel::join(rel::SHOTS, SHOT);
        let shot = set_comment_screenshot(&root, "fb_001", &file).unwrap();
        assert_eq!(shot["screenshot"], file);
    }

    /// a: review rounds started in the same second get different session files
    /// (tests/unit/node-stable.test.mjs)
    #[test]
    fn review_rounds_started_in_the_same_second_get_different_session_files() {
        let root = project("comments-same-second");
        let first = new_session(&root, None).unwrap();
        add_comment(&root, &json!({ "comment": "kept" })).unwrap();
        let ids: Vec<String> = [
            first,
            new_session(&root, None).unwrap(),
            new_session(&root, None).unwrap(),
        ]
        .iter()
        .map(|s| s.session["id"].as_str().unwrap().to_string())
        .collect();
        let unique: std::collections::HashSet<&String> = ids.iter().collect();
        assert_eq!(unique.len(), 3, "{ids:?}");
        let form = regex::Regex::new(r"^rv_\d{8}T\d{6}$").unwrap();
        assert!(
            ids.iter().all(|id| form.is_match(id)),
            "the id keeps its form"
        );
        let mut sorted = ids.clone();
        sorted.sort();
        assert_eq!(sorted, ids, "file names sort in creation order");
        let comments: Vec<Value> = all_comments(&root)
            .unwrap()
            .iter()
            .map(|c| c["comment"].clone())
            .collect();
        assert_eq!(
            comments,
            [json!("kept")],
            "the first round and its comment survive"
        );
        assert_eq!(session_files(&root).unwrap().len(), 3);
    }

    /// e: a comment file or snapshot version from input never leaves design/
    /// (tests/unit/node-stable.test.mjs; the comment part)
    #[test]
    fn a_comment_file_from_input_never_leaves_design() {
        let root = project("comments-file");
        fs::write(root.join(OUTSIDE_PAGE), "<p data-dfw-id=\"a\">x</p>").unwrap();
        fs::create_dir_all(root.join(rel::screen(SCREEN))).unwrap();
        fs::write(
            root.join(rel::screen(SCREEN_PAGE)),
            "<main>\n<p data-dfw-id=\"a\">x</p>\n</main>",
        )
        .unwrap();
        let abs = root.join(OUTSIDE_PAGE).display().to_string();
        let bad = [
            json!("../outside.html"),
            json!("screens/../../outside.html"),
            json!("screens\\..\\..\\outside.html"),
            json!(abs),
            json!("C:\\outside.html"),
            json!({ "a": 1 }),
        ];
        for file in bad {
            if let Some(f) = file.as_str() {
                assert_eq!(
                    locate_source(&root, Some(f), Some("a")).unwrap(),
                    None,
                    "{f}"
                );
            }
            let c = add_comment(
                &root,
                &json!({ "comment": "x", "file": file, "target": { "dfwId": "a" } }),
            )
            .unwrap();
            assert_eq!(c["file"], Value::Null, "{file}");
            assert_eq!(c["target"]["source"], Value::Null, "{file}");
        }
        let ok = add_comment(
            &root,
            &json!({ "comment": "x", "file": "screens/home/a.html", "target": { "dfwId": "a" } }),
        )
        .unwrap();
        assert_eq!(ok["file"], rel::screen(SCREEN_PAGE));
        assert_eq!(
            ok["target"]["source"],
            format!("{}:2", rel::screen(SCREEN_PAGE))
        );
        assert!(!safe_design_file("a\0b") && !safe_design_file("") && safe_design_file("..x.html"));
    }

    /// i: setting the status a comment already has is refused (tests/unit/node-stable.test.mjs)
    #[test]
    fn setting_the_status_a_comment_already_has_is_refused() {
        let root = project("comments-same-status");
        add_comment(&root, &json!({ "comment": "x" })).unwrap();
        let id = Some("fb_001");
        update_comment(&root, id, &update("fixed", Some("done"), None)).unwrap();
        update_comment(&root, id, &update("resolved", None, Some("user"))).unwrap();
        let err = update_comment(
            &root,
            id,
            &update("resolved", Some("again"), Some("claude")),
        )
        .unwrap_err();
        assert!(err
            .message()
            .contains("cannot go from \"resolved\" to \"resolved\" (allowed: reopened)"));
        let err = update_comment(&root, id, &update("resolved", None, Some("user"))).unwrap_err();
        assert!(err
            .message()
            .contains("cannot go from \"resolved\" to \"resolved\""));
        assert_eq!(
            all_comments(&root).unwrap()[0]["thread"]
                .as_array()
                .unwrap()
                .len(),
            1,
            "no message was added"
        );
        update_comment(
            &root,
            id,
            &update("", Some("a reply without a status still works"), None),
        )
        .unwrap();
        assert_eq!(
            all_comments(&root).unwrap()[0]["thread"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn a_new_round_closes_the_open_session() {
        let root = project("comments-round");
        assert!(current_session(&root, false).unwrap().is_none());
        add_comment(&root, &json!({ "comment": "first" })).unwrap();
        let first = current_session(&root, false).unwrap().unwrap();
        // A session id is the time to the second: write the first one as an older file.
        let old = root.join(rel::join(rel::REVIEWS, REVIEW_SESSION));
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
        let lock = root.join(rel::join(rel::REVIEWS, file::REVIEWS_LOCK));
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

    /// A lock held by a process that never releases it, and 30 writers: after the takeover
    /// every comment is stored, and the old holder's late release removes nothing (review CO-2:
    /// 16 of 31 comments were kept).
    #[test]
    fn a_held_lock_and_many_writers_keep_every_comment() {
        let root = project("comments-held");
        add_comment(&root, &json!({ "comment": "first" })).unwrap();
        let times = LockTimes {
            wait: Duration::from_millis(300),
            stale: Duration::from_secs(10),
            retry: Duration::from_millis(2),
        };
        let held = lock::acquire(&lock_path(&root), &times).unwrap();
        // The holder is a process that died (a live, slow holder keeps its lock).
        lock::mark_dead(&held);
        std::thread::scope(|s| {
            for i in 0..30 {
                let root = &root;
                s.spawn(move || {
                    locked_with(root, &times, || {
                        add_comment_unlocked(root, &json!({ "comment": format!("w{i}") }))
                    })
                    .unwrap();
                });
            }
            std::thread::sleep(Duration::from_millis(350));
            drop(held);
        });
        let all = all_comments(&root).unwrap();
        assert_eq!(all.len(), 31);
        let ids: std::collections::HashSet<String> =
            all.iter().map(|c| js::to_string(c.get("id"))).collect();
        assert_eq!(ids.len(), 31);
        assert_eq!(
            fs::read_dir(root.join(rel::REVIEWS))
                .unwrap()
                .filter(|e| e
                    .as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".lock"))
                .count(),
            0,
            "no lock folder is left"
        );
    }

    /// A session file of the wrong shape and a comment that is not text are user errors that
    /// name the file or field and say what to do (`errors.md` E2; `dfw status` printed
    /// `s.comments is not iterable`).
    #[test]
    fn a_broken_session_file_is_named_with_the_fix() {
        let root = project("comments-broken");
        let err = add_comment(&root, &json!({ "comment": 5 })).unwrap_err();
        assert_eq!(
            err.report(),
            "The comment must be text: give \"comment\" as a string."
        );
        fs::write(
            root.join(rel::join(rel::REVIEWS, REVIEW_SESSION)),
            r#"{"id":"rv_20200101T000000","comments":5}"#,
        )
        .unwrap();
        let err = all_comments(&root).unwrap_err();
        assert_eq!(
            err.report(),
            format!("{}: \"comments\" is not a list. Fix that review session file (or move it out of {}/) and run the command again.", rel::join(rel::REVIEWS, REVIEW_SESSION), rel::REVIEWS)
        );
        fs::write(
            root.join(rel::join(rel::REVIEWS, REVIEW_SESSION)),
            r#"{"id":"rv_20200101T000000","comments":[{"id":"fb_001","status":"odd","thread":[]}]}"#,
        )
        .unwrap();
        let err =
            update_comment(&root, Some("fb_001"), &update("fixed", Some("x"), None)).unwrap_err();
        assert!(
            err.report().starts_with(&format!(
                "Comment fb_001 in {} has the status \"odd\"",
                rel::join(rel::REVIEWS, REVIEW_SESSION)
            )),
            "{err}"
        );
    }
}
