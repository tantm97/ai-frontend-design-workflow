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
    change_type, describe_change, describe_component_change, diff_changes, dimension_token_index,
    field, list, master_source, walkable, MasterSource,
};
use super::export::{export_file, penpot_dir, EXPORT_ID};
use super::threads::{self, continue_thread, thread_key, threads_file};
use super::tokens::{
    import_penpot_tokens, key, nest, read_dfw_tokens, short_value, NestEntry, TokenImport, RESERVED,
};
use super::wire;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::fixed_regex;
use crate::core::paths::{file, rel, SafeName};
use crate::core::util::{
    create_unique, now_iso, read_json, sha256, to_json_compact, to_json_pretty, write_json_inside,
};
use crate::design::compose::{attr, load_components, Library};
use crate::design::html::{self, Token, Tokenizer};
use crate::design::tokens::relative_path;
use crate::review::comments::add_comment;
use crate::review::status;

/// The archive name of a change file (its push time, or [`UNSTAMPED`]): letters and digits only.
static ARCHIVE_NAME: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^[0-9A-Za-z]+$"));

/// The feedback comments an import writes.
pub(super) mod comment {
    /// The provider of every comment from Penpot.
    pub(in crate::penpot) const PROVIDER: &str = "penpot";
    /// The field of the element a comment is on, and its `file:line` in there.
    pub(in crate::penpot) use crate::review::status::field::{SOURCE, TARGET};
    /// The first line of a comment that carries a Penpot comment thread.
    pub(in crate::penpot) const THREAD_HEADER: &str = "[Penpot comment]";
}

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
    let all = match t.get(wire::token::SETS) {
        Some(Value::Array(sets)) if !sets.is_empty() => sets,
        Some(Value::String(s)) if !s.is_empty() => return Err(sets_not_a_list("text")),
        Some(Value::Object(sets)) if js::truthy(sets.get(wire::token::LENGTH)) => {
            return Err(sets_not_a_list("an object"))
        }
        _ => return Ok(None),
    };
    walkable(Some(&Value::Array(all.clone())), Some(wire::NAME), true)?;
    walkable(t.get(wire::token::THEMES), Some(wire::NAME), true)?;
    for s in all {
        walkable(s.get(wire::token::TOKENS), Some(wire::NAME), true)?;
    }
    for th in list(js::truthy_value(t.get(wire::token::THEMES))) {
        walkable(th.get(wire::token::SETS), None, true)?;
    }
    // Set and token names are untrusted: reserved names are left out, as `nest` does for
    // token paths.
    let sets: Vec<&Value> = all
        .iter()
        .filter(|s| !RESERVED.contains(&js::to_string(s.get(wire::NAME)).as_str()))
        .collect();
    let mut doc = Map::new();
    for s in &sets {
        let entries: Vec<NestEntry> = list(js::truthy_value(s.get(wire::token::TOKENS)))
            .iter()
            .map(|tok| NestEntry {
                name: js::to_string(tok.get(wire::NAME)),
                value: tok.get(wire::token::VALUE).cloned(),
                ty: tok.get(wire::token::TYPE).cloned(),
                description: tok.get(wire::token::DESCRIPTION).cloned(),
            })
            .collect();
        doc.insert(js::to_string(s.get(wire::NAME)), nest(&entries));
    }
    let or_empty = |v: Option<&Value>| js::truthy_value(v).cloned().unwrap_or_else(|| "".into());
    let themes: Vec<Value> = list(js::truthy_value(t.get(wire::token::THEMES)))
        .iter()
        .map(|th| {
            let selected: Map<String, Value> = list(js::truthy_value(th.get(wire::token::SETS)))
                .iter()
                .map(|s| (js::to_string(Some(s)), Value::from("enabled")))
                .collect();
            js::object(vec![
                (wire::NAME, th.get(wire::NAME).cloned()),
                (
                    wire::token::GROUP,
                    Some(or_empty(th.get(wire::token::GROUP))),
                ),
                (
                    wire::token::DESCRIPTION,
                    Some(or_empty(th.get(wire::token::DESCRIPTION))),
                ),
                (wire::token::IS_SOURCE, Some(Value::Bool(false))),
                (
                    wire::token::SELECTED_TOKEN_SETS,
                    Some(Value::Object(selected)),
                ),
            ])
        })
        .collect();
    doc.insert(key::THEMES.into(), Value::Array(themes));
    let names = |active_only: bool| -> Vec<Value> {
        sets.iter()
            .filter(|s| !active_only || js::truthy(s.get(wire::token::ACTIVE)))
            .map(|s| s.get(wire::NAME).cloned().unwrap_or(Value::Null))
            .collect()
    };
    doc.insert(
        key::METADATA.into(),
        json!({ (wire::token::TOKEN_SET_ORDER): names(false), (wire::token::ACTIVE_SETS): names(true) }),
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
            (wire::report::BASELINE, self.baseline.clone()),
            (
                wire::report::CHANGES,
                Some(Value::Array(self.changes.clone())),
            ),
            (wire::COMMENTS, Some(Value::Array(self.comments.clone()))),
            (wire::UPDATED, Some(Value::Array(self.updated.clone()))),
            (wire::report::SKIPPED, Some(self.skipped.into())),
            (wire::report::WARNINGS, Some(json!(self.warnings))),
            (
                wire::report::TOKEN_DIFF,
                Some(
                    self.token_diff
                        .as_ref()
                        .map_or(Value::Null, TokenImport::to_json),
                ),
            ),
            (wire::report::TOKEN_FILE, Some(json!(self.token_file))),
            (wire::report::FROZEN, Some(self.frozen.into())),
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
    let id = js::to_string(Some(id));
    let found = js::read_text(&abs)
        .ok()
        .and_then(|text| id_offset(&text, &id).map(|at| html::line_of(&text, at)));
    match found {
        Some(line) => format!("{name}:{line}").into(),
        None => file.clone(),
    }
}

/// Where the element with `data-dfw-id` `id`, or the `<dfw-use>` with that `id`, is written: the
/// attribute of an element, the `<` of an instance. Tags inside comments do not count.
fn id_offset(text: &str, id: &str) -> Option<usize> {
    let is = |a: &&html::Attr, name: &str| {
        &text[a.name.clone()] == name && html::attr_value(text, a) == id
    };
    Tokenizer::new(text).find_map(|token| {
        let Token::Open(tag) = token else {
            return None;
        };
        if tag.is(text, "dfw-use")
            && tag
                .attrs
                .iter()
                .any(|a| is(&a, attr::ID) || is(&a, attr::DFW_ID))
        {
            return Some(tag.range.start);
        }
        tag.attrs
            .iter()
            .find(|a| is(a, attr::DFW_ID))
            .map(|a| a.span.start)
    })
}

/// `value.replace(/^design\//, '')`.
fn design_relative(file: &str) -> &str {
    rel::inside(rel::DESIGN, file).unwrap_or(file)
}

/// `bbox` of a comment target: only a box of four numbers, rounded.
fn rounded_bbox(c: &Value) -> Option<Value> {
    let bbox = js::truthy_value(c.get(wire::BBOX))?.as_array()?;
    bbox.iter().all(Value::is_number).then(|| {
        Value::Array(
            bbox.iter()
                .map(|v| js::num(js::round(js::to_number(Some(v)))))
                .collect(),
        )
    })
}

fn is_comment(c: &Value) -> bool {
    c.get(field::TYPE).and_then(Value::as_str) == Some(change_type::COMMENT)
}

fn message_count(c: &Value) -> usize {
    list(c.get(wire::MESSAGES)).len()
}

/// The fingerprint of a group of changes on one element. A Penpot thread is the same thread
/// wherever the layer under its pin sits now: no bbox or anchor in its fingerprint.
fn group_fingerprint(cs: &[Value]) -> String {
    let c0 = &cs[0];
    let comp_key = |c: &Value| {
        c.get(field::COMPONENT)
            .and_then(|comp| comp.get(wire::KEY))
            .cloned()
    };
    if is_comment(c0) {
        return fingerprint(&js::object(vec![
            (field::TYPE, Some(change_type::COMMENT.into())),
            (wire::BOARD, c0.get(wire::BOARD).cloned()),
            (wire::fingerprint::COMP, comp_key(c0)),
            (wire::THREAD, c0.get(wire::THREAD).cloned()),
            (wire::MESSAGES, c0.get(wire::MESSAGES).cloned()),
        ]));
    }
    let items: Vec<Value> = cs
        .iter()
        .map(|c| {
            js::object(vec![
                (field::TYPE, c.get(field::TYPE).cloned()),
                (wire::BOARD, c.get(wire::BOARD).cloned()),
                (wire::fingerprint::P, c.get(wire::PATH).cloned()),
                (wire::ANCHOR, c.get(wire::ANCHOR).cloned()),
                (field::FROM, c.get(field::FROM).cloned()),
                (field::TO, c.get(field::TO).cloned()),
                (wire::MESSAGES, c.get(wire::MESSAGES).cloned()),
                (wire::CHARACTERS, c.get(wire::CHARACTERS).cloned()),
                (wire::BBOX, c.get(wire::BBOX).cloned()),
                (wire::fingerprint::COMP, comp_key(c)),
                (field::SIDES, c.get(field::SIDES).cloned()),
                (wire::LAYER, c.get(wire::LAYER).cloned()),
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
            s(wire::BOARD),
            s(wire::THREAD),
            message_count(c)
        );
    }
    if let Some(comp) = js::truthy_value(c.get(field::COMPONENT)) {
        return format!(
            "component|{}|{}",
            js::to_string(comp.get(wire::KEY)),
            s(wire::PATH)
        );
    }
    let element = js::truthy_value(c.get(wire::ANCHOR))
        .or(js::truthy_value(c.get(wire::PATH)))
        .map_or(String::new(), |v| js::to_string(Some(v)));
    format!("{}|{element}", s(wire::BOARD))
}

/// The feedback input for changes on one element of a screen board.
fn screen_comment(root: &Path, cs: &[Value], gate_note: &str) -> Value {
    let c0 = &cs[0];
    let anchor = js::truthy_value(c0.get(wire::ANCHOR));
    let header = if is_comment(c0) {
        comment::THREAD_HEADER.to_string()
    } else {
        let what = match (anchor, js::truthy_value(c0.get(wire::NAME))) {
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
    let file = js::truthy_value(c0.get(wire::FILE));
    let exact = anchor.is_some()
        && js::truthy(c0.get(wire::DFW_ID))
        && js::strict_eq(anchor, c0.get(wire::DFW_ID));
    let is_text = c0.get(field::TYPE).and_then(Value::as_str) == Some(change_type::TEXT);
    js::object(vec![
        (
            status::field::COMMENT,
            Some(format!("{header}\n{body}{gate_note}").into()),
        ),
        (wire::SCREEN, c0.get(wire::SCREEN).cloned()),
        (wire::VARIANT, c0.get(wire::VARIANT).cloned()),
        (wire::BREAKPOINT, c0.get(wire::BREAKPOINT).cloned()),
        (
            status::field::FILE,
            Some(file.map_or(Value::Null, |f| {
                design_relative(&js::to_string(Some(f))).into()
            })),
        ),
        (status::field::PROVIDER, Some(comment::PROVIDER.into())),
        (
            comment::TARGET,
            Some(js::object(vec![
                (wire::DFW_ID, Some(js::or_null(anchor))),
                (
                    comment::SOURCE,
                    Some(source_line(root, c0.get(wire::FILE), anchor)),
                ),
                (wire::EXACT, Some(exact.into())),
                (
                    wire::SELECTOR,
                    anchor.map(|a| format!("[data-dfw-id=\"{}\"]", js::to_string(Some(a))).into()),
                ),
                (wire::BBOX, rounded_bbox(c0)),
                (
                    wire::TEXT,
                    is_text.then(|| js::norm(c0.get(field::FROM)).into()),
                ),
            ])),
        ),
    ])
}

/// `componentComment`: the feedback input for changes on one element of a component master (or
/// a Penpot comment on a main component).
fn component_comment(root: &Path, cs: &[Value], lib: Option<&Library>, gate_note: &str) -> Value {
    let c0 = &cs[0];
    let null = Value::Null;
    let comp = c0.get(field::COMPONENT).unwrap_or(&null);
    let part = |key: &str| js::to_string(comp.get(key));
    let location = |c: &Value| {
        master_source(root, c, lib).unwrap_or_else(|| MasterSource {
            file: rel::master(&part(wire::NAME)),
            source: None,
        })
    };
    let loc = location(c0);
    let comment = is_comment(c0);
    let header = if comment {
        format!("[Penpot comment on master {}]", part(wire::TITLE))
    } else if js::truthy(c0.get(wire::IS_ROOT)) {
        format!("[Penpot master] {}:", part(wire::TITLE))
    } else {
        format!(
            "[Penpot master] {} > {}:",
            part(wire::TITLE),
            js::to_string(c0.get(wire::NAME))
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
        part(wire::NAME),
        named(wire::VARIANT, "variant"),
        named(wire::SIZE, "size")
    );
    let is_text = c0.get(field::TYPE).and_then(Value::as_str) == Some(change_type::TEXT);
    js::object(vec![
        (
            status::field::COMMENT,
            Some(format!("{header}\n{body}\n{master_note}{gate_note}").into()),
        ),
        (wire::SCREEN, Some(Value::Null)),
        (wire::VARIANT, Some(Value::Null)),
        (wire::BREAKPOINT, Some(Value::Null)),
        (status::field::FILE, Some(design_relative(&loc.file).into())),
        (status::field::PROVIDER, Some(comment::PROVIDER.into())),
        (
            comment::TARGET,
            Some(js::object(vec![
                (wire::DFW_ID, Some(Value::Null)),
                (
                    field::COMPONENT,
                    Some(js::object(vec![
                        (wire::NAME, comp.get(wire::NAME).cloned()),
                        (wire::VARIANT, comp.get(wire::VARIANT).cloned()),
                        (wire::SIZE, comp.get(wire::SIZE).cloned()),
                    ])),
                ),
                (
                    comment::SOURCE,
                    Some(
                        loc.source
                            .clone()
                            .unwrap_or_else(|| loc.file.clone())
                            .into(),
                    ),
                ),
                (
                    wire::LAYER,
                    c0.get(wire::PATH).filter(|_| !comment).cloned(),
                ),
                (
                    wire::TAG,
                    js::truthy_value(c0.get(wire::TAG))
                        .filter(|_| !comment)
                        .cloned(),
                ),
                (wire::BBOX, rounded_bbox(c0)),
                (
                    wire::TEXT,
                    is_text.then(|| js::norm(c0.get(field::FROM)).into()),
                ),
            ])),
        ),
    ])
}

/// The export the change file was built from: the one it names, when the name has the shape dfw
/// generates and the file is there; else the latest. The id never becomes a path otherwise.
fn load_baseline(dir: &Path, export_id: Option<&SafeName>) -> Result<Value, DfwError> {
    let named = match export_id {
        Some(id) => read_json(
            &dir.join(file::PENPOT_EXPORTS).join(export_file(id)),
            Some(Value::Null),
        )?,
        None => Value::Null,
    };
    if js::truthy(Some(&named)) {
        return Ok(named);
    }
    let latest = read_json(&dir.join(file::PENPOT_EXPORT), Some(Value::Null))?;
    if js::truthy(Some(&latest)) {
        return Ok(latest);
    }
    Err(DfwError::new(format!("No Penpot export found ({}). Run \"dfw penpot export\" first; the change file must come from boards built from it.", rel::PENPOT_EXPORT)))
}

/// The token diff of a change file: the plugin's sets written as a Penpot token file and
/// compared with `design/tokens` (a dry run; never applied here).
fn token_diff(
    root: &Path,
    dir: &Path,
    doc: &Value,
    dry_run: bool,
) -> Result<(TokenImport, String), DfwError> {
    let file = dir.join(file::PENPOT_TOKENS_IMPORTED);
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
            if !js::truthy(c.get(wire::token::DESCRIPTION_ONLY)) {
                lines.push(format!(
                    "- {set} {}: {} -> {}",
                    s(c, wire::token::TOKEN),
                    short_value(c.get(wire::token::FROM)),
                    short_value(c.get(wire::token::TO))
                ));
            }
        }
        for c in &d.added {
            lines.push(format!(
                "- {set} {} added ({}): {}",
                s(c, wire::token::TOKEN),
                s(c, wire::token::TYPE),
                short_value(c.get(wire::token::VALUE))
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
/// ledger, thread links and the token diff file; never edits `design/`. The steps run in the
/// order files are written in (comments, ledger, threads, archive).
pub fn import_changes(
    root: &Path,
    changes: &mut Value,
    opts: ImportOptions,
) -> Result<ChangeImport, DfwError> {
    // The ledgers are read, changed and written as one (the plugin's sync and "Send changes"
    // run on Studio connection threads at the same time).
    threads::locked(root, |held| import_locked(held, root, changes, opts))
}

fn import_locked(
    held: &threads::Held,
    root: &Path,
    changes: &mut Value,
    opts: ImportOptions,
) -> Result<ChangeImport, DfwError> {
    let dir = penpot_dir(root);
    let mut result = compare_with_export(root, &dir, changes, &opts)?;
    add_token_diff(root, &dir, changes, &opts, &mut result)?;
    let groups = group_changes(&result.changes);
    let ledger_file = dir.join(file::PENPOT_IMPORTED);
    let mut ledger = read_ledger(&ledger_file)?;
    let gate_note = gate_note(&opts, result.frozen);
    let mut threads = read_json(&threads_file(root), Some(json!({})))?;
    let (mut pending, linked) = plan_comments(
        root,
        changes,
        &groups,
        &ledger,
        &threads,
        &gate_note,
        &mut result.skipped,
    );
    if let Some(p) = token_comment(&result, &gate_note) {
        if ledger_has(&ledger, &p.fingerprint) {
            result.skipped += 1;
        } else {
            pending.push(p);
        }
    }
    if opts.dry_run {
        result.comments = pending
            .iter()
            .map(|p| {
                let mut c = Map::new();
                c.insert(status::field::ID.into(), "(dry run)".into());
                c.extend(js::spread(Some(&p.input)));
                Value::Object(c)
            })
            .collect();
        return Ok(result);
    }
    let threads_written = pending.iter().any(|p| p.thread.is_some()) || !linked.is_empty();
    write_comments(root, &pending, &mut ledger, &mut threads, &mut result)?;
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
    threads::write_ledger(held, root, &ledger_file, &Value::Array(ledger))?;
    if threads_written {
        threads::write_ledger(held, root, &threads_file(root), &threads)?;
    }
    if !opts.live {
        archive_changes(root, &dir, changes)?;
    }
    Ok(result)
}

/// Compare the change file with the export it names (else the latest one): the changes, the
/// warnings and whether the design is frozen.
fn compare_with_export(
    root: &Path,
    dir: &Path,
    changes: &mut Value,
    opts: &ImportOptions,
) -> Result<ChangeImport, DfwError> {
    // The id names a file under exports/: only the shape dfw generates (never a path from the
    // change file).
    let export_id = changes
        .get(wire::EXPORT_ID)
        .and_then(Value::as_str)
        .and_then(|id| SafeName::matching(&EXPORT_ID, id));
    let baseline = load_baseline(dir, export_id.as_ref())?;
    let dims = read_dfw_tokens(root, false)
        .map(|t| dimension_token_index(Some(&t.base)))
        .unwrap_or_default();
    let diff = diff_changes(&baseline, changes, &dims)?;
    let mut warnings = diff.warnings;
    let baseline_id = baseline.get(wire::ID);
    if let Some(id) = &export_id {
        if baseline_id.and_then(Value::as_str) != Some(id.as_str()) {
            warnings.push(format!(
                "Change file refers to export {id}, which is not on disk; compared with the latest export {}.",
                js::to_string(baseline_id)
            ));
        }
    }
    let design = opts.state.and_then(|s| s.get(wire::state::DESIGN));
    Ok(ChangeImport {
        baseline: baseline_id.cloned(),
        changes: diff.changes,
        warnings,
        frozen: js::truthy(design.and_then(|d| d.get(wire::state::LOCKED))),
        ..ChangeImport::default()
    })
}

/// Token diff (dry run against design/tokens; never applied here). A diff that fails is a
/// warning; a malformed token list in the change file is an error.
fn add_token_diff(
    root: &Path,
    dir: &Path,
    changes: &Value,
    opts: &ImportOptions,
    result: &mut ChangeImport,
) -> Result<(), DfwError> {
    let token_doc = if opts.live {
        None
    } else {
        tokens_from_plugin(changes.get(wire::TOKENS))?.or_else(|| {
            changes
                .get(wire::TOKEN_FILE)
                .filter(|f| f.is_object() || f.is_array())
                .cloned()
        })
    };
    if let Some(doc) = token_doc {
        match token_diff(root, dir, &doc, opts.dry_run) {
            Ok((diff, file)) => {
                result.token_diff = Some(diff);
                result.token_file = Some(file);
            }
            Err(err) => result
                .warnings
                .push(format!("Token diff skipped: {}", err.message())),
        }
    }
    Ok(())
}

/// The changes grouped into one comment each, in the order of their first change.
fn group_changes(changes: &[Value]) -> Vec<(String, Vec<Value>)> {
    let mut groups: Vec<(String, Vec<Value>)> = Vec::new();
    for c in changes {
        let key = group_key(c);
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some(slot) => slot.1.push(c.clone()),
            None => groups.push((key, vec![c.clone()])),
        }
    }
    groups
}

/// The fingerprints already imported (`imported.json`), primitives once.
fn read_ledger(ledger_file: &Path) -> Result<Vec<Value>, DfwError> {
    let mut ledger: Vec<Value> = Vec::new();
    let stored = read_json(ledger_file, Some(json!([])))?;
    walkable(Some(&stored), None, false)?;
    for entry in list(Some(&stored)) {
        let primitive = !(entry.is_object() || entry.is_array());
        if !(primitive && ledger.contains(entry)) {
            ledger.push(entry.clone());
        }
    }
    Ok(ledger)
}

/// The note every comment carries while the design is frozen.
fn gate_note(opts: &ImportOptions, frozen: bool) -> String {
    let design = opts.state.and_then(|s| s.get(wire::state::DESIGN));
    if frozen {
        format!(
            "\n(Design v{} is approved and frozen: discuss with the user; applying this needs \"dfw reopen\".)",
            js::to_string(design.and_then(|d| d.get(wire::state::VERSION)))
        )
    } else {
        String::new()
    }
}

/// The comments to write, one per group of changes, and the groups that continue a thread
/// already linked to a dfw comment. Groups already imported, and resolved threads, are
/// counted in `skipped`.
fn plan_comments(
    root: &Path,
    changes: &Value,
    groups: &[(String, Vec<Value>)],
    ledger: &[Value],
    threads: &Value,
    gate_note: &str,
    skipped: &mut usize,
) -> (Vec<Pending>, Vec<(String, Value)>) {
    let mut pending: Vec<Pending> = Vec::new();
    let mut linked: Vec<(String, Value)> = Vec::new();
    let lib = load_components(root).ok();
    for (_, cs) in groups {
        let c0 = &cs[0];
        let fp = group_fingerprint(cs);
        // A thread already linked to a dfw comment continues that comment (new messages,
        // resolve, reopen).
        let thread = js::present(c0.get(wire::THREAD))
            .filter(|_| is_comment(c0))
            .map(|seq| thread_key(changes.get(wire::FILE_ID), Some(seq)));
        if let Some(tk) = &thread {
            if js::truthy(threads.get(tk)) {
                linked.push((tk.clone(), c0.clone()));
                continue;
            }
        }
        if ledger_has(ledger, &fp) || (is_comment(c0) && js::truthy(c0.get(wire::RESOLVED))) {
            *skipped += 1;
            continue;
        }
        let input = if js::truthy(c0.get(field::COMPONENT)) {
            component_comment(root, cs, lib.as_ref(), gate_note)
        } else {
            screen_comment(root, cs, gate_note)
        };
        pending.push(Pending {
            fingerprint: fp,
            input,
            thread,
            seen: message_count(c0),
        });
    }
    (pending, linked)
}

/// The comment that reports token values changed in Penpot; `None` when there is no such
/// change.
fn token_comment(result: &ChangeImport, gate_note: &str) -> Option<Pending> {
    let lines = token_lines(result.token_diff.as_ref()?);
    if lines.is_empty() {
        return None;
    }
    let fp = fingerprint(&json!(lines));
    let more = if lines.len() > 40 {
        format!("\n- ... {} more", lines.len() - 40)
    } else {
        String::new()
    };
    let shown: Vec<&str> = lines.iter().take(40).map(String::as_str).collect();
    let file = result.token_file.as_deref().unwrap_or("null");
    Some(Pending {
        fingerprint: fp,
        input: json!({
            (status::field::COMMENT): format!(
                "[Penpot] Token values changed in Penpot:\n{}{more}\nReview with \"dfw tokens import {file} --dry-run\"; apply only with the user's agreement (the import is refused while the design is frozen).{gate_note}",
                shown.join("\n")
            ),
            (status::field::PROVIDER): comment::PROVIDER,
            (comment::TARGET): {},
        }),
        thread: None,
        seen: 0,
    })
}

/// Write the pending comments, each with its fingerprint in the ledger and its thread link.
fn write_comments(
    root: &Path,
    pending: &[Pending],
    ledger: &mut Vec<Value>,
    threads: &mut Value,
    result: &mut ChangeImport,
) -> Result<(), DfwError> {
    for p in pending {
        let c = add_comment(root, &p.input)?;
        if !ledger_has(ledger, &p.fingerprint) {
            ledger.push(p.fingerprint.as_str().into());
        }
        if let (Some(tk), Value::Object(links)) = (&p.thread, &mut *threads) {
            links.insert(
                tk.clone(),
                js::object(vec![
                    (wire::link::COMMENT, c.get(status::field::ID).cloned()),
                    (wire::link::SEEN, Some(p.seen.into())),
                    (wire::link::RESOLVED, Some(Value::Bool(false))),
                    (wire::link::POSTED, Some(json!([]))),
                ]),
            );
        }
        result.comments.push(c);
    }
    Ok(())
}

/// The archive name of a change file whose push time has no digits.
pub(super) const UNSTAMPED: &str = "changes";

/// Keep the change file under `changes/`, named after its push time; a second change file with
/// the same push time (or none) is kept next to the first (`<name>-2.json`), never over it.
fn archive_changes(root: &Path, dir: &Path, changes: &Value) -> Result<(), DfwError> {
    // The archive name comes from the change file: digits, T and Z only.
    let pushed_at = js::truthy_value(changes.get(wire::PUSHED_AT))
        .map_or_else(now_iso, |p| js::to_string(Some(p)));
    let stamp: String = pushed_at
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == 'T' || *c == 'Z')
        .collect();
    let name = if stamp.is_empty() { UNSTAMPED } else { &stamp };
    let Some(name) = SafeName::matching(&ARCHIVE_NAME, name) else {
        return Err(DfwError::internal("No archive name for the change file."));
    };
    create_unique(
        root,
        &dir.join(file::PENPOT_CHANGES),
        &name,
        file::JSON_EXT,
        (to_json_pretty(changes) + "\n").as_bytes(),
    )?;
    Ok(())
}

/// `formatImportChanges`: what `dfw penpot import` prints.
pub fn format_import_changes(r: &ChangeImport, dry_run: bool) -> String {
    let mut by: Vec<(String, usize)> = Vec::new();
    for c in &r.changes {
        let ty = js::to_string(c.get(field::TYPE));
        let key = if js::truthy(c.get(field::COMPONENT)) && !is_comment(c) {
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
        let target = c.get(comment::TARGET);
        let component = js::truthy_value(target.and_then(|t| t.get(field::COMPONENT)));
        let mut place = joined(vec![
            c.get(wire::SCREEN),
            c.get(wire::VARIANT),
            c.get(wire::BREAKPOINT),
        ]);
        if place.is_empty() {
            place = if component.is_some() {
                "master"
            } else {
                "(design)"
            }
            .to_string();
        }
        let element = match (
            js::truthy_value(target.and_then(|t| t.get(wire::DFW_ID))),
            component,
        ) {
            (Some(id), _) => js::to_string(Some(id)),
            (None, Some(comp)) => joined(vec![
                comp.get(wire::NAME),
                comp.get(wire::VARIANT),
                comp.get(wire::SIZE),
            ]),
            (None, None) => "?".to_string(),
        };
        lines.push(format!(
            "{} {place} -> {element}\n    {}",
            js::to_string(c.get(status::field::ID)),
            js::to_string(c.get(status::field::COMMENT))
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
    lines.push(if dry_run {
        "Dry run: no comments written.".to_string()
    } else if r.comments.is_empty() {
        "No new comments.".to_string()
    } else {
        format!(
            "Comments are in the review loop (dfw comments list); nothing in {}/ was changed.",
            rel::DESIGN
        )
    });
    lines.join("\n")
}

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;
