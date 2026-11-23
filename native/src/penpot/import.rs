//! Ports `tokensFromPlugin`, `importChanges`, `componentComment` and `formatImportChanges` of
//! `core/lib/penpot.mjs`.
//!
//! A change file from the plugin never rewrites dfw's HTML: the change set becomes feedback
//! comments (provider `penpot`) and a token diff, applied by the agent through the normal
//! review loop and gates. A ledger of fingerprints (`.dfw/penpot/imported.json`) keeps a second
//! "Send changes" from adding the same comments again.
//!
//! The change file is untrusted: `exportId` names a file only when it has the shape dfw
//! generates, and the archive name is built from digits, `T` and `Z` alone.
// Over 800 lines: the two comment builders (screen, master) and the summary read the same
// change objects as the import itself.

use std::fs;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use super::diff::{
    describe_change, describe_component_change, diff_changes, dimension_token_index, list,
    master_source, walkable, MasterSource,
};
use super::export::penpot_dir;
use super::live::{continue_thread, thread_key, threads_file};
use super::tokens::{
    import_penpot_tokens, nest, read_dfw_tokens, short_value, NestEntry, TokenImport, RESERVED,
};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::fixed_regex;
use crate::core::util::{now_iso, read_json, sha256, to_json_compact, write_json_inside};
use crate::design::compose::{load_components, Library};
use crate::design::tokens::relative_path;
use crate::review::comments::add_comment;

static EXPORT_ID: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^px_[0-9A-Za-z_]+$"));

fn fingerprint(x: &Value) -> String {
    sha256(to_json_compact(x))[..16].to_string()
}

/// The change file's `tokens.sets` is not a list.
fn sets_not_a_list(what: &str) -> DfwError {
    DfwError::new(format!(
        "The change file's \"tokens.sets\" is {what}, not a list of token sets. Send the changes again from the dfw bridge plugin (\"Send changes to dfw\"), or fix the file."
    ))
}

/// `tokensFromPlugin`: Penpot token sets sent by the plugin as a Penpot token file (same shape
/// as the Tokens panel export). `None` when the plugin sent no set.
pub fn tokens_from_plugin(t: Option<&Value>) -> Result<Option<Value>, DfwError> {
    let Some(t) = t else {
        return Ok(None);
    };
    // `t.sets.length`: an empty list or anything without a length is no token set at all.
    let all = match t.get("sets") {
        Some(Value::Array(sets)) if !sets.is_empty() => sets,
        Some(Value::String(s)) if !s.is_empty() => return Err(sets_not_a_list("text")),
        Some(Value::Object(sets)) if js::truthy(sets.get("length")) => {
            return Err(sets_not_a_list("an object"))
        }
        _ => return Ok(None),
    };
    walkable(Some(&Value::Array(all.clone())), Some("name"), true)?;
    walkable(t.get("themes"), Some("name"), true)?;
    for s in all {
        walkable(s.get("tokens"), Some("name"), true)?;
    }
    for th in list(js::truthy_value(t.get("themes"))) {
        walkable(th.get("sets"), None, true)?;
    }
    // Set and token names are untrusted: reserved names are left out, as `nest` does for
    // token paths.
    let sets: Vec<&Value> = all
        .iter()
        .filter(|s| !RESERVED.contains(&js::to_string(s.get("name")).as_str()))
        .collect();
    let mut doc = Map::new();
    for s in &sets {
        let entries: Vec<NestEntry> = list(js::truthy_value(s.get("tokens")))
            .iter()
            .map(|tok| NestEntry {
                name: js::to_string(tok.get("name")),
                value: tok.get("value").cloned(),
                ty: tok.get("type").cloned(),
                description: tok.get("description").cloned(),
            })
            .collect();
        doc.insert(js::to_string(s.get("name")), nest(&entries));
    }
    let or_empty = |v: Option<&Value>| js::truthy_value(v).cloned().unwrap_or_else(|| "".into());
    let themes: Vec<Value> = list(js::truthy_value(t.get("themes")))
        .iter()
        .map(|th| {
            let selected: Map<String, Value> = list(js::truthy_value(th.get("sets")))
                .iter()
                .map(|s| (js::to_string(Some(s)), Value::from("enabled")))
                .collect();
            js::object(vec![
                ("name", th.get("name").cloned()),
                ("group", Some(or_empty(th.get("group")))),
                ("description", Some(or_empty(th.get("description")))),
                ("isSource", Some(Value::Bool(false))),
                ("selectedTokenSets", Some(Value::Object(selected))),
            ])
        })
        .collect();
    doc.insert("$themes".into(), Value::Array(themes));
    let names = |active_only: bool| -> Vec<Value> {
        sets.iter()
            .filter(|s| !active_only || js::truthy(s.get("active")))
            .map(|s| s.get("name").cloned().unwrap_or(Value::Null))
            .collect()
    };
    doc.insert(
        "$metadata".into(),
        json!({ "tokenSetOrder": names(false), "activeSets": names(true) }),
    );
    Ok(Some(js::key_order(Value::Object(doc))))
}

/// How [`import_changes`] runs.
#[derive(Debug, Clone, Copy, Default)]
pub struct ImportOptions<'a> {
    /// Report only: no comment, ledger or file is written.
    pub dry_run: bool,
    /// The workflow state, for the note on a frozen design.
    pub state: Option<&'a Value>,
    /// The live comment loop: no token diff, no archive of the change file.
    pub live: bool,
}

/// The result of [`import_changes`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChangeImport {
    /// The id of the export the changes were compared with.
    pub baseline: Option<Value>,
    pub changes: Vec<Value>,
    /// The comments written (in a dry run: what would be written, with the id `(dry run)`).
    pub comments: Vec<Value>,
    /// `{ id, status }` of comments a linked thread changed.
    pub updated: Vec<Value>,
    pub skipped: usize,
    pub warnings: Vec<String>,
    pub token_diff: Option<TokenImport>,
    pub token_file: Option<String>,
    pub frozen: bool,
}

impl ChangeImport {
    /// The result as the object the Node code printed.
    pub fn to_json(&self) -> Value {
        js::object(vec![
            ("baseline", self.baseline.clone()),
            ("changes", Some(Value::Array(self.changes.clone()))),
            ("comments", Some(Value::Array(self.comments.clone()))),
            ("updated", Some(Value::Array(self.updated.clone()))),
            ("skipped", Some(self.skipped.into())),
            ("warnings", Some(json!(self.warnings))),
            (
                "tokenDiff",
                Some(
                    self.token_diff
                        .as_ref()
                        .map_or(Value::Null, TokenImport::to_json),
                ),
            ),
            ("tokenFile", Some(json!(self.token_file))),
            ("frozen", Some(self.frozen.into())),
        ])
    }
}

/// A comment waiting to be written: its fingerprint, the feedback input, and the Penpot thread
/// it comes from with the number of designer messages read.
struct Pending {
    fingerprint: String,
    input: Value,
    thread: Option<String>,
    seen: usize,
}

/// `design/<file>:<line>` of the element (`data-dfw-id`) or instance (`<dfw-use id>`) with this
/// id; the file alone when the id is not found.
fn source_line(root: &Path, file: Option<&Value>, id: Option<&Value>) -> Value {
    let Some(file) = js::truthy_value(file) else {
        return Value::Null;
    };
    let name = js::to_string(Some(file));
    let abs = root.join(&name);
    let Some(id) = js::truthy_value(id).filter(|_| abs.exists()) else {
        return file.clone();
    };
    let found = js::read_text(&abs).ok().and_then(|text| {
        let re = Regex::new(&format!(
            r#"(?:data-dfw-id|<dfw-use(?-u:\b)[^>]*(?-u:\b)id)="{}""#,
            regex::escape(&js::to_string(Some(id)))
        ))
        .ok()?;
        let m = re.find(&text)?;
        Some(text[..m.start()].matches('\n').count() + 1)
    });
    match found {
        Some(line) => format!("{name}:{line}").into(),
        None => file.clone(),
    }
}

/// `value.replace(/^design\//, '')`.
fn design_relative(file: &str) -> &str {
    file.strip_prefix("design/").unwrap_or(file)
}

/// `bbox` of a comment target: only a box of four numbers, rounded.
fn rounded_bbox(c: &Value) -> Option<Value> {
    let bbox = js::truthy_value(c.get("bbox"))?.as_array()?;
    bbox.iter().all(Value::is_number).then(|| {
        Value::Array(
            bbox.iter()
                .map(|v| js::num(js::round(js::to_number(Some(v)))))
                .collect(),
        )
    })
}

fn is_comment(c: &Value) -> bool {
    c.get("type").and_then(Value::as_str) == Some("comment")
}

fn message_count(c: &Value) -> usize {
    list(c.get("messages")).len()
}

/// The fingerprint of a group of changes on one element. A Penpot thread is the same thread
/// wherever the layer under its pin sits now: no bbox or anchor in its fingerprint.
fn group_fingerprint(cs: &[Value]) -> String {
    let c0 = &cs[0];
    let comp_key = |c: &Value| c.get("component").and_then(|comp| comp.get("key")).cloned();
    if is_comment(c0) {
        return fingerprint(&js::object(vec![
            ("type", Some("comment".into())),
            ("board", c0.get("board").cloned()),
            ("comp", comp_key(c0)),
            ("thread", c0.get("thread").cloned()),
            ("messages", c0.get("messages").cloned()),
        ]));
    }
    let items: Vec<Value> = cs
        .iter()
        .map(|c| {
            js::object(vec![
                ("type", c.get("type").cloned()),
                ("board", c.get("board").cloned()),
                ("p", c.get("path").cloned()),
                ("anchor", c.get("anchor").cloned()),
                ("from", c.get("from").cloned()),
                ("to", c.get("to").cloned()),
                ("messages", c.get("messages").cloned()),
                ("characters", c.get("characters").cloned()),
                ("bbox", c.get("bbox").cloned()),
                ("comp", comp_key(c)),
                ("sides", c.get("sides").cloned()),
                ("layer", c.get("layer").cloned()),
            ])
        })
        .collect();
    fingerprint(&Value::Array(items))
}

/// The key that groups design changes per element (board + anchor) into one comment each;
/// Penpot comments stay separate.
fn group_key(c: &Value) -> String {
    let s = |key: &str| js::to_string(c.get(key));
    if is_comment(c) {
        return format!(
            "comment|{}|{}|{}",
            s("board"),
            s("thread"),
            message_count(c)
        );
    }
    if let Some(comp) = js::truthy_value(c.get("component")) {
        return format!("component|{}|{}", js::to_string(comp.get("key")), s("path"));
    }
    let element = js::truthy_value(c.get("anchor"))
        .or(js::truthy_value(c.get("path")))
        .map_or(String::new(), |v| js::to_string(Some(v)));
    format!("{}|{element}", s("board"))
}

/// The feedback input for changes on one element of a screen board.
fn screen_comment(root: &Path, cs: &[Value], gate_note: &str) -> Value {
    let c0 = &cs[0];
    let anchor = js::truthy_value(c0.get("anchor"));
    let header = if is_comment(c0) {
        "[Penpot comment]".to_string()
    } else {
        let what = match (anchor, js::truthy_value(c0.get("name"))) {
            (Some(anchor), _) => format!("\"{}\"", js::to_string(Some(anchor))),
            (None, Some(name)) => format!("layer \"{}\"", js::to_string(Some(name))),
            (None, None) => "board".to_string(),
        };
        format!("[Penpot] {what}:")
    };
    let body = if is_comment(c0) {
        describe_change(c0)
    } else {
        cs.iter()
            .map(|c| format!("- {}", describe_change(c)))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let file = js::truthy_value(c0.get("file"));
    let exact =
        anchor.is_some() && js::truthy(c0.get("dfwId")) && js::strict_eq(anchor, c0.get("dfwId"));
    let is_text = c0.get("type").and_then(Value::as_str) == Some("text");
    js::object(vec![
        (
            "comment",
            Some(format!("{header}\n{body}{gate_note}").into()),
        ),
        ("screen", c0.get("screen").cloned()),
        ("variant", c0.get("variant").cloned()),
        ("breakpoint", c0.get("breakpoint").cloned()),
        (
            "file",
            Some(file.map_or(Value::Null, |f| {
                design_relative(&js::to_string(Some(f))).into()
            })),
        ),
        ("provider", Some("penpot".into())),
        (
            "target",
            Some(js::object(vec![
                ("dfwId", Some(js::or_null(anchor))),
                ("source", Some(source_line(root, c0.get("file"), anchor))),
                ("exact", Some(exact.into())),
                (
                    "selector",
                    anchor.map(|a| format!("[data-dfw-id=\"{}\"]", js::to_string(Some(a))).into()),
                ),
                ("bbox", rounded_bbox(c0)),
                ("text", is_text.then(|| js::norm(c0.get("from")).into())),
            ])),
        ),
    ])
}

/// `componentComment`: the feedback input for changes on one element of a component master (or
/// a Penpot comment on a main component).
fn component_comment(root: &Path, cs: &[Value], lib: Option<&Library>, gate_note: &str) -> Value {
    let c0 = &cs[0];
    let null = Value::Null;
    let comp = c0.get("component").unwrap_or(&null);
    let part = |key: &str| js::to_string(comp.get(key));
    let location = |c: &Value| {
        master_source(root, c, lib).unwrap_or_else(|| MasterSource {
            file: format!("design/system/components/{}.html", part("name")),
            source: None,
        })
    };
    let loc = location(c0);
    let comment = is_comment(c0);
    let header = if comment {
        format!("[Penpot comment on master {}]", part("title"))
    } else if js::truthy(c0.get("isRoot")) {
        format!("[Penpot master] {}:", part("title"))
    } else {
        format!(
            "[Penpot master] {} > {}:",
            part("title"),
            js::to_string(c0.get("name"))
        )
    };
    let body = if comment {
        describe_change(c0)
    } else {
        cs.iter()
            .map(|c| {
                let source = location(c)
                    .source
                    .filter(|s| Some(s) != loc.source.as_ref());
                let place = source.map_or(String::new(), |s| format!(" [{s}]"));
                format!("- {}{place}", describe_component_change(c))
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let named = |key: &str, word: &str| {
        js::truthy_value(comp.get(key)).map_or(String::new(), |v| {
            format!(", {word} \"{}\"", js::to_string(Some(v)))
        })
    };
    let master_note = format!(
        "Change the master {} (component \"{}\"{}{}): it affects every screen using it. Do not patch single screens.",
        loc.file,
        part("name"),
        named("variant", "variant"),
        named("size", "size")
    );
    let is_text = c0.get("type").and_then(Value::as_str) == Some("text");
    js::object(vec![
        (
            "comment",
            Some(format!("{header}\n{body}\n{master_note}{gate_note}").into()),
        ),
        ("screen", Some(Value::Null)),
        ("variant", Some(Value::Null)),
        ("breakpoint", Some(Value::Null)),
        ("file", Some(design_relative(&loc.file).into())),
        ("provider", Some("penpot".into())),
        (
            "target",
            Some(js::object(vec![
                ("dfwId", Some(Value::Null)),
                (
                    "component",
                    Some(js::object(vec![
                        ("name", comp.get("name").cloned()),
                        ("variant", comp.get("variant").cloned()),
                        ("size", comp.get("size").cloned()),
                    ])),
                ),
                (
                    "source",
                    Some(
                        loc.source
                            .clone()
                            .unwrap_or_else(|| loc.file.clone())
                            .into(),
                    ),
                ),
                ("layer", c0.get("path").filter(|_| !comment).cloned()),
                (
                    "tag",
                    js::truthy_value(c0.get("tag"))
                        .filter(|_| !comment)
                        .cloned(),
                ),
                ("bbox", rounded_bbox(c0)),
                ("text", is_text.then(|| js::norm(c0.get("from")).into())),
            ])),
        ),
    ])
}

/// The export the change file was built from: the one it names, when the name has the shape dfw
/// generates and the file is there; else the latest. The id never becomes a path otherwise.
fn load_baseline(dir: &Path, export_id: Option<&str>) -> Result<Value, DfwError> {
    let named = match export_id {
        Some(id) => read_json(
            &dir.join("exports").join(format!("{id}.json")),
            Some(Value::Null),
        )?,
        None => Value::Null,
    };
    if js::truthy(Some(&named)) {
        return Ok(named);
    }
    let latest = read_json(&dir.join("export.json"), Some(Value::Null))?;
    if js::truthy(Some(&latest)) {
        return Ok(latest);
    }
    Err(DfwError::new("No Penpot export found (.dfw/penpot/export.json). Run \"dfw penpot export\" first; the change file must come from boards built from it."))
}

/// The token diff of a change file: the plugin's sets written as a Penpot token file and
/// compared with `design/tokens` (a dry run; never applied here).
fn token_diff(
    root: &Path,
    dir: &Path,
    doc: &Value,
    dry_run: bool,
) -> Result<(TokenImport, String), DfwError> {
    let file = dir.join("tokens.from-penpot.json");
    if !dry_run {
        write_json_inside(root, &file, doc)?;
    }
    // A dry run compares through a temporary file, removed on every path (also when the
    // comparison fails; the Node code left it behind then).
    let tmp = dry_run.then(|| TempFile(dir.join(format!(".tokens-{}.json", std::process::id()))));
    let read_from = match &tmp {
        Some(t) => {
            write_json_inside(root, &t.0, doc)?;
            t.0.clone()
        }
        None => file.clone(),
    };
    let result = import_penpot_tokens(root, &read_from, None, true, false)?;
    Ok((result, relative_path(root, &file)))
}

/// A temporary file removed when dropped.
struct TempFile(std::path::PathBuf);

impl Drop for TempFile {
    fn drop(&mut self) {
        // Cleanup of a temporary file: the import's result does not depend on it.
        let _ = fs::remove_file(&self.0);
    }
}

/// The lines of the comment that reports token values changed in Penpot.
fn token_lines(diff: &TokenImport) -> Vec<String> {
    let s = |c: &Value, key: &str| js::to_string(c.get(key));
    let mut lines = Vec::new();
    for (set, d) in &diff.diff {
        for c in &d.changed {
            if !js::truthy(c.get("descriptionOnly")) {
                lines.push(format!(
                    "- {set} {}: {} -> {}",
                    s(c, "token"),
                    short_value(c.get("from")),
                    short_value(c.get("to"))
                ));
            }
        }
        for c in &d.added {
            lines.push(format!(
                "- {set} {} added ({}): {}",
                s(c, "token"),
                s(c, "type"),
                short_value(c.get("value"))
            ));
        }
    }
    lines
}

/// A JavaScript `Set` of what `imported.json` holds: primitives once, in insertion order.
fn ledger_has(ledger: &[Value], fingerprint: &str) -> bool {
    ledger.iter().any(|v| v.as_str() == Some(fingerprint))
}

/// `importChanges`: import a change file. Writes feedback comments (provider `penpot`), the
/// ledger, thread links and the token diff file; never edits `design/`.
// Long: the steps share the ledger, the thread links and the result, and their order is the
// order files are written in (comments, ledger, threads, archive).
#[allow(clippy::too_many_lines, reason = "A10 debt")]
pub fn import_changes(
    root: &Path,
    changes: &mut Value,
    opts: ImportOptions,
) -> Result<ChangeImport, DfwError> {
    let dir = penpot_dir(root);
    // The id names a file under exports/: only the shape dfw generates (never a path from the
    // change file).
    let export_id = changes
        .get("exportId")
        .and_then(Value::as_str)
        .filter(|id| EXPORT_ID.is_match(id))
        .map(str::to_string);
    let baseline = load_baseline(&dir, export_id.as_deref())?;
    let dims = read_dfw_tokens(root, false)
        .map(|t| dimension_token_index(Some(&t.base)))
        .unwrap_or_default();
    let diff = diff_changes(&baseline, changes, &dims)?;
    let mut warnings = diff.warnings;
    let baseline_id = baseline.get("id");
    if let Some(id) = &export_id {
        if baseline_id.and_then(Value::as_str) != Some(id) {
            warnings.push(format!(
                "Change file refers to export {id}, which is not on disk; compared with the latest export {}.",
                js::to_string(baseline_id)
            ));
        }
    }
    let design = opts.state.and_then(|s| s.get("design"));
    let frozen = js::truthy(design.and_then(|d| d.get("locked")));
    let mut result = ChangeImport {
        baseline: baseline_id.cloned(),
        changes: diff.changes,
        warnings,
        frozen,
        ..ChangeImport::default()
    };

    // Token diff (dry run against design/tokens; never applied here).
    let token_doc = if opts.live {
        None
    } else {
        tokens_from_plugin(changes.get("tokens"))?.or_else(|| {
            changes
                .get("tokenFile")
                .filter(|f| f.is_object() || f.is_array())
                .cloned()
        })
    };
    if let Some(doc) = token_doc {
        match token_diff(root, &dir, &doc, opts.dry_run) {
            Ok((diff, file)) => {
                result.token_diff = Some(diff);
                result.token_file = Some(file);
            }
            Err(err) => result
                .warnings
                .push(format!("Token diff skipped: {}", err.message())),
        }
    }

    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for c in &result.changes {
        let key = group_key(c);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1.push(c.clone()),
            None => groups.push((key, vec![c.clone()])),
        }
    }
    let ledger_file = dir.join("imported.json");
    let mut ledger: Vec<Value> = Vec::new();
    let stored = read_json(&ledger_file, Some(json!([])))?;
    walkable(Some(&stored), None, false)?;
    for entry in list(Some(&stored)) {
        let primitive = !(entry.is_object() || entry.is_array());
        if !(primitive && ledger.contains(entry)) {
            ledger.push(entry.clone());
        }
    }
    let gate_note = if frozen {
        format!(
            "\n(Design v{} is approved and frozen: discuss with the user; applying this needs \"dfw reopen\".)",
            js::to_string(design.and_then(|d| d.get("version")))
        )
    } else {
        String::new()
    };
    let mut pending: Vec<Pending> = Vec::new();
    let mut threads = read_json(&threads_file(root), Some(json!({})))?;
    let mut linked: Vec<(String, Value)> = Vec::new();
    let lib = load_components(root).ok();
    for (_, cs) in &groups {
        let c0 = &cs[0];
        let fp = group_fingerprint(cs);
        // A thread already linked to a dfw comment continues that comment (new messages,
        // resolve, reopen).
        let thread = js::present(c0.get("thread"))
            .filter(|_| is_comment(c0))
            .map(|seq| thread_key(changes.get("fileId"), Some(seq)));
        if let Some(tk) = &thread {
            if js::truthy(threads.get(tk)) {
                linked.push((tk.clone(), c0.clone()));
                continue;
            }
        }
        if ledger_has(&ledger, &fp) || (is_comment(c0) && js::truthy(c0.get("resolved"))) {
            result.skipped += 1;
            continue;
        }
        let input = if js::truthy(c0.get("component")) {
            component_comment(root, cs, lib.as_ref(), &gate_note)
        } else {
            screen_comment(root, cs, &gate_note)
        };
        pending.push(Pending {
            fingerprint: fp,
            input,
            thread,
            seen: message_count(c0),
        });
    }
    if let Some(diff) = &result.token_diff {
        let lines = token_lines(diff);
        if !lines.is_empty() {
            let fp = fingerprint(&json!(lines));
            if ledger_has(&ledger, &fp) {
                result.skipped += 1;
            } else {
                let more = if lines.len() > 40 {
                    format!("\n- ... {} more", lines.len() - 40)
                } else {
                    String::new()
                };
                let shown: Vec<&str> = lines.iter().take(40).map(String::as_str).collect();
                let file = result.token_file.as_deref().unwrap_or("null");
                pending.push(Pending {
                    fingerprint: fp,
                    input: json!({
                        "comment": format!(
                            "[Penpot] Token values changed in Penpot:\n{}{more}\nReview with \"dfw tokens import {file} --dry-run\"; apply only with the user's agreement (the import is refused while the design is frozen).{gate_note}",
                            shown.join("\n")
                        ),
                        "provider": "penpot",
                        "target": {},
                    }),
                    thread: None,
                    seen: 0,
                });
            }
        }
    }
    if opts.dry_run {
        result.comments = pending
            .iter()
            .map(|p| {
                let mut c = Map::new();
                c.insert("id".into(), "(dry run)".into());
                c.extend(js::spread(Some(&p.input)));
                Value::Object(c)
            })
            .collect();
        return Ok(result);
    }
    let threads_written = pending.iter().any(|p| p.thread.is_some()) || !linked.is_empty();
    for p in &pending {
        let c = add_comment(root, &p.input)?;
        if !ledger_has(&ledger, &p.fingerprint) {
            ledger.push(p.fingerprint.as_str().into());
        }
        if let (Some(tk), Value::Object(links)) = (&p.thread, &mut threads) {
            links.insert(
                tk.clone(),
                js::object(vec![
                    ("comment", c.get("id").cloned()),
                    ("seen", Some(p.seen.into())),
                    ("resolved", Some(Value::Bool(false))),
                    ("posted", Some(json!([]))),
                ]),
            );
        }
        result.comments.push(c);
    }
    for (tk, c0) in &linked {
        continue_thread(
            root,
            &mut threads,
            tk,
            c0,
            &mut result.updated,
            &mut result.warnings,
        )?;
    }
    write_json_inside(root, &ledger_file, &Value::Array(ledger))?;
    if threads_written {
        write_json_inside(root, &threads_file(root), &threads)?;
    }
    if !opts.live {
        // The archive name comes from the change file: digits, T and Z only.
        let pushed_at = js::truthy_value(changes.get("pushedAt"))
            .map_or_else(now_iso, |p| js::to_string(Some(p)));
        let stamp: String = pushed_at
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == 'T' || *c == 'Z')
            .collect();
        let name = if stamp.is_empty() { "changes" } else { &stamp };
        write_json_inside(
            root,
            &dir.join("changes").join(format!("{name}.json")),
            changes,
        )?;
    }
    Ok(result)
}

/// `formatImportChanges`: what `dfw penpot import` prints.
pub fn format_import_changes(r: &ChangeImport, dry_run: bool) -> String {
    let mut by: Vec<(String, usize)> = Vec::new();
    for c in &r.changes {
        let ty = js::to_string(c.get("type"));
        let key = if js::truthy(c.get("component")) && !is_comment(c) {
            format!("component {ty}")
        } else {
            ty
        };
        match by.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1 += 1,
            None => by.push((key, 1)),
        }
    }
    let counts = if r.changes.is_empty() {
        "no changes".to_string()
    } else {
        js::key_order_pairs(by)
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut lines = vec![format!(
        "Compared with export {}: {counts}.",
        js::to_string(r.baseline.as_ref())
    )];
    let joined = |values: Vec<Option<&Value>>| -> String {
        values
            .into_iter()
            .filter_map(js::truthy_value)
            .map(|v| js::to_string(Some(v)))
            .collect::<Vec<_>>()
            .join("/")
    };
    for c in &r.comments {
        let target = c.get("target");
        let component = js::truthy_value(target.and_then(|t| t.get("component")));
        let mut place = joined(vec![c.get("screen"), c.get("variant"), c.get("breakpoint")]);
        if place.is_empty() {
            place = if component.is_some() {
                "master"
            } else {
                "(design)"
            }
            .to_string();
        }
        let element = match (
            js::truthy_value(target.and_then(|t| t.get("dfwId"))),
            component,
        ) {
            (Some(id), _) => js::to_string(Some(id)),
            (None, Some(comp)) => joined(vec![
                comp.get("name"),
                comp.get("variant"),
                comp.get("size"),
            ]),
            (None, None) => "?".to_string(),
        };
        lines.push(format!(
            "{} {place} -> {element}\n    {}",
            js::to_string(c.get("id")),
            js::to_string(c.get("comment"))
                .split('\n')
                .collect::<Vec<_>>()
                .join("\n    ")
        ));
    }
    if r.skipped > 0 {
        lines.push(format!(
            "{} skipped (already imported, or a resolved Penpot thread).",
            r.skipped
        ));
    }
    if let Some(diff) = &r.token_diff {
        let n: usize = diff
            .diff
            .iter()
            .map(|(_, d)| d.changed.len() + d.added.len())
            .sum();
        let file = r
            .token_file
            .as_deref()
            .filter(|f| !f.is_empty())
            .map_or(String::new(), |f| {
                format!("; file {f} (dfw tokens import {f} --dry-run)")
            });
        lines.push(format!(
            "Token sets from Penpot: {n} token change(s){file}."
        ));
    }
    for w in &r.warnings {
        lines.push(format!("Note: {w}"));
    }
    if r.frozen {
        lines.push(
            "The design is frozen (G2): the changes are recorded as comments only.".to_string(),
        );
    }
    lines.push(
        if dry_run {
            "Dry run: no comments written."
        } else if r.comments.is_empty() {
            "No new comments."
        } else {
            "Comments are in the review loop (dfw comments list); nothing in design/ was changed."
        }
        .to_string(),
    );
    lines.join("\n")
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
