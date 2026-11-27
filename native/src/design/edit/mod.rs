//! Ports `core/lib/edit.mjs`: direct edits from the Studio, written back to the design SOURCE
//! files, and the undo journal in `.dfw/edits`.
//!
//! Small, surgical changes only: element text, attributes of a component instance (variant,
//! size, declared props), spacing through space tokens, moving an element next to a sibling or
//! into an empty container, and resizing that snaps to the system, written to a dfw-owned
//! `<style data-dfw-layout>` block. Elements are found by `data-dfw-id` (or the id of a
//! `<dfw-use>`/`<dfw-icon>`) with the tokenizer, and an edit replaces byte ranges of the text it
//! read: every other byte is copied as it is.
//!
//! Split by job: `locate` (finding the element), `style` (inline styles and token names),
//! `moves` (reordering), `layout` (the layout block); this file has the boundary checks, the
//! edit operations and the journal.

mod layout;
mod locate;
mod moves;
mod style;
#[cfg(test)]
mod tests;

use crate::core::state::{design_lock, field};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

pub use layout::set_layout_rule;
pub use locate::{locate_own, locate_value, Located};
pub use moves::{move_inside, move_source};
pub use style::{layout_value, load_token_names, set_style_prop, token_var};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::fixed_regex;
use crate::core::lock;
use crate::core::paths::{self, file, inside, project_paths, resolve, to_posix, Place, SafeName};
use crate::core::util::{
    create_first_free, now_iso, read_json, to_json_compact, write_atomic_inside,
};
use crate::design::compose::{attr, load_components, ComponentDef};
use crate::design::html::{escape_text, set_attr, Splice};
use crate::design::tokens::dir_names;
use regex::Regex;
use std::sync::LazyLock;

/// `SPACING_PROPS`: what a spacing edit may set in an inline style.
pub const SPACING_PROPS: [&str; 17] = [
    "gap",
    "row-gap",
    "column-gap",
    "padding",
    "padding-top",
    "padding-right",
    "padding-bottom",
    "padding-left",
    "padding-block",
    "padding-inline",
    "margin",
    "margin-top",
    "margin-right",
    "margin-bottom",
    "margin-left",
    "margin-block",
    "margin-inline",
];

/// The keys of an edit op, as the Studio sends it.
pub mod key {
    /// The design file, relative to the project.
    pub const FILE: &str = "file";
    /// What to do: one of [`ops`](super::ops).
    pub const OP: &str = "op";
    /// The element: its `data-dfw-id` (or the id of an instance).
    pub const ID: &str = "id";
    /// The new text, variant, size, prop value, token or length.
    pub const VALUE: &str = "value";
    /// The prop, style property or layout property to set.
    pub const PROP: &str = "prop";
    /// The element a move drops next to (or into).
    pub const REF: &str = "ref";
    /// Where a move drops: one of [`position`](super::position).
    pub const POSITION: &str = "position";
    /// The breakpoint of a layout edit (`0` for every width).
    pub const MAX_WIDTH: &str = "maxWidth";
}

/// The keys of a record of the undo journal (`.dfw/edits/*.json`).
pub mod record {
    /// When the edit was made.
    pub const AT: &str = "at";
    /// The design file, relative to the project.
    pub const FILE: &str = "file";
    /// The edit op as it was applied (its keys of [`key`](super::key) but the file).
    pub const OP: &str = "op";
    /// The text of the file before the edit.
    pub const BEFORE: &str = "before";
    /// The text of the file after the edit.
    pub const AFTER: &str = "after";
}

/// The keys of what an edit or an undo sends back to the Studio.
pub mod reply {
    /// The design file, relative to the project.
    pub const FILE: &str = "file";
    /// The edit changed the file.
    pub const CHANGED: &str = "changed";
    /// The line of the element after the edit.
    pub const LINE: &str = "line";
    /// The op an undo reverted, as journaled.
    pub const OP: &str = "op";
    /// The records left in the journal after an undo.
    pub const REMAINING: &str = "remaining";
}

/// The edit ops `transform` applies: the `op` field of an edit.
pub mod ops {
    /// The element's text.
    pub const TEXT: &str = "text";
    /// The variant of a component instance.
    pub const VARIANT: &str = "variant";
    /// The size of a component instance.
    pub const SIZE: &str = "size";
    /// A prop of a component instance.
    pub const PROP: &str = "prop";
    /// A gap, padding or margin of the inline style.
    pub const SPACING: &str = "spacing";
    /// The element moved next to or into another one.
    pub const MOVE: &str = "move";
    /// A resize property in the layout block.
    pub const LAYOUT: &str = "layout";
}

/// Where a move drops the element: the `position` field of a move.
pub mod position {
    /// Before the other element.
    pub const BEFORE: &str = "before";
    /// After the other element.
    pub const AFTER: &str = "after";
    /// Into the other element, an empty container.
    pub const INSIDE: &str = "inside";
}

/// The properties a resize sets in the layout block.
pub mod layout_prop {
    /// The columns a grid item spans.
    pub const GRID_COLUMN: &str = "grid-column";
    /// The width: a size token or a percentage.
    pub const WIDTH: &str = "width";
    /// The minimum height: a size or space token.
    pub const MIN_HEIGHT: &str = "min-height";
}

/// `LAYOUT_PROPS`: what a resize may set in the layout block.
pub const LAYOUT_PROPS: [&str; 3] = [
    layout_prop::GRID_COLUMN,
    layout_prop::WIDTH,
    layout_prop::MIN_HEIGHT,
];

/// `MAX_JOURNAL`: undo depth; each record holds two full copies of the file.
pub const MAX_JOURNAL: usize = 200;

/// `lineAt`: 1-based line of byte `i`.
fn line_at(text: &str, i: usize) -> usize {
    text.as_bytes()[..i].iter().filter(|b| **b == b'\n').count() + 1
}

/// A design file an edit may write: its absolute path and its path from the project root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignFile {
    /// Canonical absolute path.
    pub abs: PathBuf,
    /// `design/...` with forward slashes.
    pub rel: String,
}

/// `resolveDesignFile`: a design file named relative to `design/` or to the project root,
/// with the edit boundary enforced on real paths: inside `design/`, not inside
/// `design/approved/`, an existing `.html` file.
pub fn resolve_design_file(root: &Path, file: Option<&Value>) -> Result<DesignFile, DfwError> {
    let file = match file {
        Some(Value::String(file)) if !file.is_empty() => file,
        _ => return Err(DfwError::new("No source file for this frame.")),
    };
    let p = project_paths(root);
    let rel = file.trim_start_matches('/');
    let rel = paths::rel::inside(paths::rel::DESIGN, rel).unwrap_or(rel);
    let missing = || {
        DfwError::new(format!(
            "{} does not exist.",
            paths::rel::design(&to_posix(rel))
        ))
    };
    // Real paths: on a case-insensitive file system "Approved/x.html" IS the frozen file, and a
    // symlink may leave design/.
    let (abs, in_design) = match inside(&p.design, &p.design.join(rel)) {
        Place::Inside { real, rel } if !rel.as_os_str().is_empty() => (real, rel),
        // A cycle of links is no file.
        Place::Unresolved => return Err(missing()),
        _ => {
            return Err(DfwError::new(format!(
                "Edits are limited to files under {}/.",
                paths::rel::DESIGN
            )))
        }
    };
    if !matches!(inside(&p.approved, &abs), Place::Outside(_)) {
        return Err(DfwError::new(format!(
            "{}/ is frozen; edits there are refused.",
            paths::rel::APPROVED
        )));
    }
    if !abs.to_string_lossy().ends_with(".html") {
        return Err(DfwError::new(
            "Only HTML design files can be edited from the Studio.",
        ));
    }
    if !abs.exists() {
        return Err(missing());
    }
    let rel = paths::rel::design(&to_posix(&in_design.to_string_lossy()));
    Ok(DesignFile { abs, rel })
}

/// `assertEditable`: Studio edits follow the write gate: refused once the design is approved
/// and locked (G2), or in review-only mode.
pub fn assert_editable(root: &Path) -> Result<(), DfwError> {
    let state = read_json(&project_paths(root).state, Some(Value::Null))?;
    if !js::truthy(Some(&state)) || state.get(field::ACTIVE) == Some(&Value::Bool(false)) {
        return Ok(());
    }
    if let Some(version) = design_lock(&state) {
        return Err(DfwError::new(format!(
            "Design v{version} is approved and locked (G2); Studio edits are refused. Changing it needs your approval and \"dfw reopen --reason <why>\".",
        )));
    }
    if state.get(field::MODE).and_then(Value::as_str) == Some("review-only") {
        return Err(DfwError::new(
            "This project is in review-only mode; Studio edits are refused. Leave a comment instead.",
        ));
    }
    Ok(())
}

fn component_def(root: &Path, el: &Located) -> Result<ComponentDef, DfwError> {
    let name = el.attrs.nonempty("c").or(el.attrs.get(attr::COMPONENT));
    load_components(root)?.get(name).cloned().ok_or_else(|| {
        DfwError::new(format!(
            "Unknown component \"{}\".",
            name.unwrap_or("undefined")
        ))
    })
}

/// `text.replace(/\s*\n\s*/g, ' ').trim()`: a line break and the white space around it is one
/// space.
fn one_line(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find(js::is_space) {
        let run_len = rest[start..]
            .find(|c: char| !js::is_space(c))
            .unwrap_or(rest.len() - start);
        let run = &rest[start..start + run_len];
        out.push_str(&rest[..start]);
        out.push_str(if run.contains('\n') { " " } else { run });
        rest = &rest[start + run_len..];
    }
    out.push_str(rest);
    js::trim(&out).to_string()
}

fn not_an_instance(id: &str) -> DfwError {
    DfwError::new(format!("\"{id}\" is not a component instance (<dfw-use>)."))
}

/// The text with one attribute of the element's opening tag set or removed.
fn with_attr(text: &str, el: &Located, name: &str, value: Option<&str>) -> String {
    match set_attr(text, &el.open, name, value) {
        Some(splice) => splice.apply(text),
        None => text.to_string(),
    }
}

/// `transform`: the new source text for an edit op `{op, id, value, prop, ref, position,
/// maxWidth}`; a refusal is an error and nothing is written.
pub fn transform(root: &Path, text: &str, op: &Value) -> Result<String, DfwError> {
    let field = |key: &str| op.get(key);
    let shown = |key: &str| js::to_string(field(key));
    let el = locate_own(root, text, field(key::ID))?;
    let id = shown(key::ID);
    // `value == null || value === ''`
    let value = js::present(field(key::VALUE)).filter(|v| v.as_str() != Some(""));
    let kind = field(key::OP).and_then(Value::as_str).unwrap_or_default();
    match kind {
        ops::TEXT => edit_text(text, &el, &id, op),
        ops::VARIANT | ops::SIZE => edit_choice(root, text, &el, &id, kind, op),
        ops::PROP => edit_prop(root, text, &el, &id, value, op),
        ops::SPACING => edit_spacing(root, text, &el, value, op),
        ops::MOVE => edit_move(root, text, &el, op),
        ops::LAYOUT => edit_layout(root, text, &id, op),
        _ => Err(DfwError::new(format!(
            "Unknown edit op \"{}\".",
            shown(key::OP)
        ))),
    }
}

/// Op "text": the element's text, when it has no child elements; the white space around it
/// stays.
fn edit_text(text: &str, el: &Located, id: &str, op: &Value) -> Result<String, DfwError> {
    if el.is_empty_tag() {
        return Err(DfwError::new(format!(
            "<{}> has no text content to edit.",
            el.tag
        )));
    }
    let inner = &text[el.inner_start..el.inner_end];
    let has_child = inner
        .match_indices('<')
        .any(|(i, _)| inner[i + 1..].starts_with(|c: char| c.is_ascii_alphabetic() || c == '!'));
    if has_child {
        return Err(DfwError::new(format!(
            "\"{id}\" contains child elements in the source; edit it in code or leave a comment."
        )));
    }
    let value = one_line(
        &js::present(op.get(key::VALUE)).map_or(String::new(), |v| js::to_string(Some(v))),
    );
    if value.is_empty() {
        return Err(DfwError::new("Text cannot be empty."));
    }
    // The white space around the old text stays (all of it leads when there is no text).
    let lead = inner.len() - inner.trim_start_matches(js::is_space).len();
    let trail = inner[lead..].len() - inner[lead..].trim_end_matches(js::is_space).len();
    let splice = Splice {
        range: el.inner_start + lead..el.inner_end - trail,
        text: escape_text(&value),
    };
    Ok(splice.apply(text))
}

/// Ops "variant" and "size" (`kind`): one of the values the instance's component declares.
fn edit_choice(
    root: &Path,
    text: &str,
    el: &Located,
    id: &str,
    kind: &str,
    op: &Value,
) -> Result<String, DfwError> {
    if !el.is_instance {
        return Err(not_an_instance(id));
    }
    let def = component_def(root, el)?;
    let name = def.display_name();
    let allowed = if kind == ops::VARIANT {
        &def.variants
    } else {
        &def.sizes
    };
    if allowed.is_empty() {
        return Err(DfwError::new(format!(
            "Component \"{name}\" has no {kind}s."
        )));
    }
    match op.get(key::VALUE).and_then(Value::as_str) {
        Some(v) if allowed.iter().any(|a| a == v) => Ok(with_attr(text, el, kind, Some(v))),
        _ => Err(DfwError::new(format!(
            "Component \"{name}\" has no {kind} \"{}\" ({}).",
            js::to_string(op.get(key::VALUE)),
            allowed.join(", ")
        ))),
    }
}

/// Op "prop": a prop the instance's component declares, set or (no value) removed.
fn edit_prop(
    root: &Path,
    text: &str,
    el: &Located,
    id: &str,
    value: Option<&Value>,
    op: &Value,
) -> Result<String, DfwError> {
    if !el.is_instance {
        return Err(not_an_instance(id));
    }
    let def = component_def(root, el)?;
    let prop = op.get(key::PROP).and_then(Value::as_str);
    let Some(prop) = prop.filter(|p| def.props.iter().any(|d| d == p)) else {
        let declared = def.props.join(", ");
        return Err(DfwError::new(format!(
            "Component \"{}\" has no prop \"{}\" ({}).",
            def.display_name(),
            js::to_string(op.get(key::PROP)),
            if declared.is_empty() {
                "none"
            } else {
                &declared
            }
        )));
    };
    let value = value.map(|v| js::to_string(Some(v)));
    Ok(with_attr(text, el, prop, value.as_deref()))
}

/// Op "spacing": a gap, padding or margin of the inline style, set to a space token or (no
/// value) removed.
fn edit_spacing(
    root: &Path,
    text: &str,
    el: &Located,
    value: Option<&Value>,
    op: &Value,
) -> Result<String, DfwError> {
    if el.is_instance {
        return Err(DfwError::new(
            "Spacing of a component instance is defined by its master; change the component or leave a comment.",
        ));
    }
    let prop = op.get(key::PROP).and_then(Value::as_str);
    let Some(prop) = prop.filter(|p| SPACING_PROPS.contains(p)) else {
        return Err(DfwError::new(format!(
            "\"{}\" is not editable here; only gap, padding and margin.",
            js::to_string(op.get(key::PROP))
        )));
    };
    let decl = match value {
        Some(v) => Some(token_var(root, &js::to_string(Some(v)), &["space."])?),
        None => None,
    };
    let style = set_style_prop(
        el.attrs.get(attr::STYLE).unwrap_or_default(),
        prop,
        decl.as_deref(),
    );
    let style = Some(style.as_str()).filter(|s| !s.is_empty());
    Ok(with_attr(text, el, attr::STYLE, style))
}

/// Op "move": the element before or after another element, or inside an empty container.
fn edit_move(root: &Path, text: &str, el: &Located, op: &Value) -> Result<String, DfwError> {
    let position = op.get(key::POSITION).and_then(Value::as_str);
    let Some(position) =
        position.filter(|p| [position::BEFORE, position::AFTER, position::INSIDE].contains(p))
    else {
        return Err(DfwError::new(
            "Move position must be \"before\", \"after\" or \"inside\".",
        ));
    };
    if !js::truthy(op.get(key::REF)) || op.get(key::REF) == op.get(key::ID) {
        return Err(DfwError::new(
            "Move needs another element to drop next to (or a container to drop into).",
        ));
    }
    let target = locate_own(root, text, op.get(key::REF))?;
    if position == position::INSIDE {
        move_inside(text, el, &target)
    } else {
        move_source(text, el, &target, position == position::BEFORE)
    }
}

/// Op "layout": one resize property of the element's rule in the layout block at a
/// breakpoint, or (no prop) the whole rule reset.
fn edit_layout(root: &Path, text: &str, id: &str, op: &Value) -> Result<String, DfwError> {
    let shown = |key: &str| js::to_string(op.get(key));
    // No prop: reset the element's whole rule at this breakpoint scope.
    let prop = match js::present(op.get(key::PROP)) {
        None => None,
        Some(Value::String(p)) if LAYOUT_PROPS.contains(&p.as_str()) => Some(p.as_str()),
        Some(_) => {
            return Err(DfwError::new(format!(
                "\"{}\" is not resizable here; only {}.",
                shown(key::PROP),
                LAYOUT_PROPS.join(", ")
            )))
        }
    };
    if id.contains(['"', '\'', '\\']) {
        return Err(DfwError::new(format!(
            "\"{id}\" cannot be used in a layout rule."
        )));
    }
    let max_width = match js::present(op.get(key::MAX_WIDTH)) {
        Some(v) if v.as_str() != Some("") => js::to_number(Some(v)),
        _ => 0.0,
    };
    if max_width.fract() != 0.0 || !(0.0..=10000.0).contains(&max_width) {
        return Err(DfwError::new(format!(
            "Invalid breakpoint width \"{}\".",
            shown(key::MAX_WIDTH)
        )));
    }
    let css = match prop {
        Some(prop) => layout_value(root, prop, op.get(key::VALUE))?,
        None => None,
    };
    set_layout_rule(text, id, prop, css.as_deref(), max_width)
}

fn edits_dir(root: &Path) -> PathBuf {
    project_paths(root).edits
}

/// The journal records, oldest first (`readdirSync(...).filter(.json).sort()`).
fn journal(root: &Path) -> Result<Vec<String>, DfwError> {
    let mut files: Vec<String> = dir_names(&edits_dir(root))?
        .into_iter()
        .filter(|f| f.ends_with(".json"))
        .collect();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    Ok(files)
}

/// Name of the next journal record: `<milliseconds, 14 digits>-<sequence, 4 digits>`. The
/// sequence continues from the newest record, so records keep their order without a counter in
/// memory; the edit lock makes that safe across threads and processes, and the record is made
/// with `create_new`, so a name in use is never overwritten (`seq_after` moves on).
fn next_stamp(journal: &[String], seq_after: Option<u32>) -> String {
    let last = journal
        .last()
        .and_then(|f| f.strip_suffix(".json"))
        .and_then(|f| f.rsplit_once('-'))
        .and_then(|(_, seq)| seq.parse::<u32>().ok());
    let seq = last.max(seq_after).map_or(0, |n| (n + 1) % 10000);
    format!(
        "{:014}-{seq:04}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    )
}

/// The edit lock of a project (`.dfw/edits/.lock`): the Studio serves one thread per
/// connection, and the read, transform, write and journal of an edit or an undo run as one.
pub(crate) fn edit_lock(root: &Path) -> Result<lock::Lock, DfwError> {
    lock::acquire(&edits_dir(root).join(".lock"), &lock::STANDARD)
}

/// The name of a journal record ([`next_stamp`]).
static STAMP: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^[0-9]+-[0-9]{4}$"));

/// Write a journal record under a new name; never replaces a record.
fn write_record(root: &Path, record: &Value) -> Result<(), DfwError> {
    let text = to_json_compact(record);
    let journal = journal(root)?;
    let mut seq_after = None;
    let stamps = |_| {
        let stamp = next_stamp(&journal, seq_after);
        // The next try continues after this one.
        seq_after = stamp.rsplit_once('-').and_then(|(_, s)| s.parse().ok());
        SafeName::matching(&STAMP, &stamp)
    };
    create_first_free(root, &edits_dir(root), file::JSON_EXT, stamps, |_| {
        text.clone().into_bytes()
    })?;
    Ok(())
}

/// What [`apply_edit`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditResult {
    /// The edited file, `design/...`.
    pub file: String,
    /// False when the edit left the file as it was (nothing is written or journaled).
    pub changed: bool,
    /// 1-based line of the element after the edit.
    pub line: usize,
}

impl EditResult {
    /// The object the Studio sends back: `{file, changed, line}`.
    pub fn to_json(&self) -> Value {
        json!({ (reply::FILE): self.file, (reply::CHANGED): self.changed, (reply::LINE): self.line })
    }
}

/// `applyEdit`: apply one edit op `{file, op, id, value, prop, ref, position, maxWidth}` to
/// its design file and journal it for undo.
pub fn apply_edit(root: &Path, op: &Value) -> Result<EditResult, DfwError> {
    let _held = edit_lock(root)?;
    assert_editable(root)?;
    let DesignFile { abs, rel } = resolve_design_file(root, op.get(key::FILE))?;
    let before = js::read_text(&abs)?;
    let after = transform(root, &before, op)?;
    if after == before {
        let line = line_at(&before, locate_value(&before, op.get(key::ID))?.start);
        return Ok(EditResult {
            file: rel,
            changed: false,
            line,
        });
    }
    write_atomic_inside(root, &abs, &after)?;
    let mut kept = Map::new();
    for name in [
        key::OP,
        key::ID,
        key::PROP,
        key::VALUE,
        key::REF,
        key::POSITION,
        key::MAX_WIDTH,
    ] {
        if let Some(v) = op.get(name) {
            kept.insert(name.to_string(), v.clone());
        }
    }
    let record = json!({ (record::AT): now_iso(), (record::FILE): rel, (record::OP): kept, (record::BEFORE): before, (record::AFTER): after });
    let dir = edits_dir(root);
    write_record(root, &record)?;
    let records = journal(root)?;
    for old in &records[..records.len().saturating_sub(MAX_JOURNAL)] {
        match fs::remove_file(dir.join(old)) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err.into()),
            _ => {}
        }
    }
    let line = line_at(&after, locate_value(&after, op.get(key::ID))?.start);
    Ok(EditResult {
        file: rel,
        changed: true,
        line,
    })
}

/// What [`undo_edit`] restored.
#[derive(Debug, Clone, PartialEq)]
pub struct UndoResult {
    /// The restored file, `design/...`.
    pub file: String,
    /// The op that was undone, as journaled.
    pub op: Option<Value>,
    /// Records left in the journal.
    pub remaining: usize,
}

impl UndoResult {
    /// The object the Studio sends back: `{file, op, remaining}`.
    pub fn to_json(&self) -> Value {
        let mut map = Map::new();
        map.insert(reply::FILE.into(), json!(self.file));
        if let Some(op) = &self.op {
            map.insert(reply::OP.into(), op.clone());
        }
        map.insert(reply::REMAINING.into(), json!(self.remaining));
        Value::Object(map)
    }
}

/// `undoEdit`: undo the most recent edit; refused when the file changed since.
pub fn undo_edit(root: &Path) -> Result<UndoResult, DfwError> {
    let _held = edit_lock(root)?;
    assert_editable(root)?;
    let dir = edits_dir(root);
    let mut files = journal(root)?;
    // An edit to a file that was deleted since cannot be undone and must not block the older
    // edits behind it.
    let found = loop {
        let Some(name) = files.last() else {
            return Err(DfwError::new("Nothing to undo."));
        };
        let rec_file = dir.join(name);
        let rec: Value = serde_json::from_str(&js::read_text(&rec_file)?)?;
        let file = rec
            .get(record::FILE)
            .and_then(Value::as_str)
            .ok_or_else(|| {
                DfwError::internal(format!("{}: no file in the record", rec_file.display()))
            })?;
        // `path.join(root, file)`: the name is taken as relative even when it starts with `/`.
        if resolve(root.join(file.trim_start_matches('/'))).exists() {
            break (rec_file, rec);
        }
        fs::remove_file(&rec_file)?;
        files.pop();
    };
    let (rec_file, rec) = found;
    let file = rec
        .get(record::FILE)
        .and_then(Value::as_str)
        .unwrap_or_default();
    let target = resolve_design_file(root, rec.get(record::FILE))?;
    if Some(js::read_text(&target.abs)?.as_str()) != rec.get(record::AFTER).and_then(Value::as_str)
    {
        return Err(DfwError::new(format!(
            "{file} changed after that edit (by the AI or an editor); undo refused to avoid losing work."
        )));
    }
    let before = rec
        .get(record::BEFORE)
        .and_then(Value::as_str)
        .ok_or_else(|| DfwError::internal(format!("{}: no text to restore", rec_file.display())))?;
    write_atomic_inside(root, &target.abs, before)?;
    fs::remove_file(&rec_file)?;
    Ok(UndoResult {
        file: file.to_string(),
        op: rec.get(record::OP).cloned(),
        remaining: files.len() - 1,
    })
}
