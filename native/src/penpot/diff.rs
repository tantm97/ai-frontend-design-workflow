//! Ports `diffChanges`, `diffComponents`, `dimensionTokenIndex`, `masterSource` and the
//! `describe*` helpers of `core/lib/penpot.mjs`.
//!
//! The plugin sends back what the designer changed (texts, boxes, fills, added layers, comment
//! threads), on the screen boards and on the main components of the "dfw components" page.
//! Here that change file is compared with the export the boards were built from. Pure: no file
//! is written, and the change file is untrusted (every read may be missing or of another type).
// Over 800 lines: the two comparisons (boards, component masters), the source lookup for a
// master and the sentences for every change type belong together, as in the Node file.

use std::collections::HashMap;
use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Value};

use super::export::{resolve_alias, CHANGES_SCHEMA};
use super::tokens::flatten_tokens;

use crate::core::error::DfwError;
use crate::core::js;
use crate::design::compose::{load_components, Library};

/// Positions and sizes within this many px are the same (Penpot rounds, fonts differ).
const TOL: f64 = 1.5;

type Pairs = Vec<(&'static str, Option<Value>)>;

/// A list out of untrusted JSON: anything that is not an array is empty.
pub(super) fn list(v: Option<&Value>) -> &[Value] {
    v.and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// Refuse a list the Node code could not walk: there it threw a `TypeError` before anything
/// was written, so a malformed change file stops here too. `reads` is the property read of
/// every entry (a `null` entry has none); `method` is for a list used through an array method,
/// which a string does not have. A string walked with `for...of` is no list here.
pub(super) fn walkable(
    v: Option<&Value>,
    reads: Option<&str>,
    method: bool,
) -> Result<(), DfwError> {
    match js::truthy_value(v) {
        None => Ok(()),
        Some(Value::String(_)) if !method => Ok(()),
        Some(Value::Array(items)) => match reads.filter(|_| items.iter().any(Value::is_null)) {
            Some(property) => Err(DfwError::internal(format!(
                "Cannot read properties of null (reading '{property}')"
            ))),
            None => Ok(()),
        },
        Some(other) => Err(DfwError::internal(format!(
            "{} is not a list",
            js::slice16(&js::stringify(Some(other)), 0, 60)
        ))),
    }
}

fn kind(l: &Value) -> Option<&str> {
    l.get("kind").and_then(Value::as_str)
}

fn is_text(l: &Value) -> bool {
    kind(l) == Some("text")
}

/// `value ?? fallback` for an opacity or a width that defaults to 1.
fn or_one(v: Option<&Value>) -> Value {
    js::present(v).cloned().unwrap_or_else(|| json!(1))
}

/// `value?.[0]` of what the plugin sent, when it is truthy: the first item of a list, and, as
/// in JavaScript, the first character of a string.
fn first_of(v: Option<&Value>) -> Option<Value> {
    match v? {
        Value::Array(items) => items.first().filter(|x| js::truthy(Some(x))).cloned(),
        Value::String(s) => s.chars().next().map(|c| c.to_string().into()),
        _ => None,
    }
}

fn lower(v: Option<&Value>) -> String {
    js::to_string(v).to_lowercase()
}

/// `[l.x, l.y, l.w, l.h]`: a missing number is `null` in JSON.
fn bbox(l: &Value) -> Value {
    Value::Array(
        ["x", "y", "w", "h"]
            .iter()
            .map(|k| l.get(*k).cloned().unwrap_or(Value::Null))
            .collect(),
    )
}

/// The layers of a tree by path, parents before children (`flattenLayers`).
struct Layers<'a> {
    order: Vec<&'a Value>,
    index: HashMap<String, usize>,
}

impl<'a> Layers<'a> {
    fn of(layers: Option<&'a Value>) -> Self {
        fn walk<'a>(layers: Option<&'a Value>, out: &mut Layers<'a>) {
            for l in list(layers) {
                // The key keeps the type of the path: only the same string finds a layer.
                match out.index.get(&js::stringify(l.get("path"))) {
                    Some(i) => out.order[*i] = l,
                    None => {
                        out.index
                            .insert(js::stringify(l.get("path")), out.order.len());
                        out.order.push(l);
                    }
                }
                walk(l.get("children"), out);
            }
        }
        let mut out = Layers {
            order: Vec::new(),
            index: HashMap::new(),
        };
        walk(layers, &mut out);
        out
    }

    fn get(&self, path: Option<&Value>) -> Option<&'a Value> {
        self.index.get(&js::stringify(path)).map(|i| self.order[*i])
    }

    fn has(&self, path: Option<&Value>) -> bool {
        self.index.contains_key(&js::stringify(path))
    }
}

/// The path of the parent layer (`"0.1"` for `"0.1.3"`), `None` for a top layer.
fn parent_path(path: &str) -> Option<&str> {
    path.rfind('.').map(|i| &path[..i])
}

/// Whether `value + x` joins text in JavaScript instead of adding numbers.
fn joins(v: Option<&Value>) -> bool {
    matches!(
        v,
        Some(Value::String(_) | Value::Array(_) | Value::Object(_))
    )
}

/// `a + b` as a number: JavaScript joins the two as text when one of them is not a number
/// (a coordinate sent as `"20"`), and the comparison that follows reads the text as a number.
fn js_add(a: Option<&Value>, b: Option<&Value>) -> f64 {
    if joins(a) || joins(b) {
        js::str_to_number(&format!("{}{}", js::to_string(a), js::to_string(b)))
    } else {
        js::to_number(a) + js::to_number(b)
    }
}

fn contains(l: &Value, x: f64, y: f64) -> bool {
    let n = |k: &str| js::to_number(l.get(k));
    x >= n("x")
        && x <= js_add(l.get("x"), l.get("w"))
        && y >= n("y")
        && y <= js_add(l.get("y"), l.get("h"))
}

fn area(l: &Value) -> f64 {
    js::to_number(l.get("w")) * js::to_number(l.get("h"))
}

/// The smallest layer with an id (its own or its nearest ancestor's) under a point.
fn deepest_at<'a>(layers: impl Iterator<Item = &'a Value>, x: f64, y: f64) -> Option<&'a Value> {
    let mut best: Option<&Value> = None;
    for l in layers {
        if !(js::truthy(l.get("anchor")) || js::truthy(l.get("dfwId"))) {
            continue;
        }
        if contains(l, x, y) && best.is_none_or(|b| area(l) <= area(b)) {
            best = Some(l);
        }
    }
    best
}

/// The middle of a layer the designer added.
fn centre(a: &Value) -> (f64, f64) {
    let middle = |pos: &str, len: &str| {
        let half = js::truthy_value(a.get(len)).map_or(0.0, |v| js::to_number(Some(v))) / 2.0;
        if joins(a.get(pos)) {
            js::str_to_number(&format!(
                "{}{}",
                js::to_string(a.get(pos)),
                js::number(half)
            ))
        } else {
            js::to_number(a.get(pos)) + half
        }
    };
    (middle("x", "w"), middle("y", "h"))
}

/// `host?.anchor || host?.dfwId || null`.
fn anchor_of(host: Option<&Value>) -> Value {
    host.and_then(|h| js::truthy_value(h.get("anchor")).or(js::truthy_value(h.get("dfwId"))))
        .cloned()
        .unwrap_or(Value::Null)
}

/// Replies the plugin posted for dfw (`[dfw] ...`) are not designer messages.
pub(super) fn is_dfw_reply(content: Option<&Value>) -> bool {
    let text = js::truthy_value(content).map_or(String::new(), |c| js::to_string(Some(c)));
    let start = text.trim_start_matches(js::is_space);
    start
        .get(..5)
        .is_some_and(|p| p.eq_ignore_ascii_case("[dfw]"))
}

/// The token of the export with exactly this colour.
fn color_token(baseline: &Value, c: &Value) -> Value {
    let hex = lower(c.get("color"));
    let opacity = js::to_number(Some(&or_one(c.get("opacity"))));
    list(baseline.get("colors"))
        .iter()
        .find(|t| {
            t.get("hex").and_then(Value::as_str) == Some(&hex)
                && (js::to_number(t.get("opacity")) - opacity).abs() < 0.01
        })
        .map_or(Value::Null, |t| js::or_null(t.get("name")))
}

/// Whether two paints differ: another colour, or an opacity more than 1% apart.
fn paint_differs(after: &Value, before_color: Option<&Value>, before: &Value) -> bool {
    let opacity = |v: &Value| js::to_number(Some(&or_one(v.get("opacity"))));
    lower(after.get("color")) != lower(before_color)
        || (opacity(after) - opacity(before)).abs() > 0.01
}

/// The result of [`diff_changes`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Diff {
    pub changes: Vec<Value>,
    pub warnings: Vec<String>,
}

fn where_board(b: &Value) -> Pairs {
    vec![
        ("board", b.get("key").cloned()),
        ("screen", b.get("screen").cloned()),
        ("variant", b.get("variant").cloned()),
        ("breakpoint", b.get("breakpoint").cloned()),
        ("file", b.get("file").cloned()),
    ]
}

fn nowhere() -> Pairs {
    ["board", "screen", "variant", "breakpoint", "file"]
        .iter()
        .map(|k| (*k, Some(Value::Null)))
        .collect()
}

fn change(ty: &str, parts: Vec<Pairs>) -> Value {
    let mut pairs: Pairs = vec![("type", Some(ty.into()))];
    for part in parts {
        pairs.extend(part);
    }
    js::object(pairs)
}

/// `diffChanges`: compare what the plugin sent back with the export it was built from.
/// A layer whose path the export does not know is moved to the board's `added` list in
/// `changes` (the Node code did the same to the object it was given).
// Long: boards, deleted boards, masters and comment threads, in the order the Node function
// reports them (the order decides how changes group into comments).
pub fn diff_changes(baseline: &Value, changes: &mut Value, dims: &[Dim]) -> Result<Diff, DfwError> {
    if changes.get("schema").and_then(Value::as_str) != Some(CHANGES_SCHEMA) {
        return Err(DfwError::new(format!(
            "Not a dfw Penpot change file (schema \"{}\", expected {CHANGES_SCHEMA}).",
            js::to_string(changes.get("schema"))
        )));
    }
    let mut boards: HashMap<String, &Value> = HashMap::new();
    for p in list(baseline.get("pages")) {
        for b in list(p.get("boards")) {
            boards.insert(js::stringify(b.get("key")), b);
        }
    }
    let board_of = |key: Option<&Value>| boards.get(&js::stringify(key)).copied();
    let baseline_id = js::to_string(baseline.get("id"));
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    walkable(changes.get("boards"), Some("key"), false)?;
    walkable(changes.get("missingBoards"), None, false)?;
    walkable(changes.get("comments"), Some("boardKey"), false)?;
    for t in list(changes.get("comments")) {
        walkable(t.get("comments"), Some("content"), true)?;
    }
    let board_count = list(changes.get("boards")).len();
    for bi in 0..board_count {
        let cb = list(changes.get("boards"))[bi].clone();
        let Some(b) = board_of(cb.get("key")) else {
            let name = js::truthy_value(cb.get("name")).or(cb.get("key"));
            warnings.push(format!(
                "Board \"{}\" is not in export {baseline_id}: skipped.",
                js::to_string(name)
            ));
            continue;
        };
        walkable(cb.get("layers"), Some("path"), false)?;
        walkable(cb.get("added"), Some("x"), false)?;
        let unknown = diff_board(b, &cb, baseline, &mut out, &mut warnings);
        if unknown.is_empty() {
            continue;
        }
        // `cb.added.push` of a string is not a function.
        walkable(cb.get("added"), None, true)?;
        // `(cb.added ||= []).push(n)`: the change file itself carries them from now on.
        if let Some(Value::Object(real)) = changes
            .get_mut("boards")
            .and_then(|boards| boards.get_mut(bi))
        {
            let slot = real.entry("added").or_insert(Value::Null);
            if !js::truthy(&*slot) {
                *slot = json!([]);
            }
            if let Value::Array(added) = slot {
                added.extend(unknown);
            }
        }
    }
    for key in list(changes.get("missingBoards")) {
        if let Some(b) = board_of(Some(key)) {
            out.push(change(
                "board-removed",
                vec![where_board(b), vec![("anchor", Some(Value::Null))]],
            ));
        }
    }
    // Comments sit on the board as the designer sees it: resolve them against the pushed
    // (current) boxes.
    let mut current: HashMap<String, Vec<Value>> = HashMap::new();
    for cb in list(changes.get("boards")) {
        let Some(b) = board_of(cb.get("key")) else {
            continue;
        };
        let flat = Layers::of(b.get("layers"));
        let mut moved: Vec<(String, Value)> = Vec::new();
        for n in list(cb.get("layers")) {
            let Some(l) = flat.get(n.get("path")) else {
                continue;
            };
            let mut now = js::spread(Some(l));
            for k in ["x", "y", "w", "h"] {
                match n.get(k) {
                    Some(v) => {
                        now.insert(k.to_string(), v.clone());
                    }
                    None => {
                        now.shift_remove(k);
                    }
                }
            }
            let path = js::stringify(n.get("path"));
            match moved.iter_mut().find(|(p, _)| *p == path) {
                Some(slot) => slot.1 = Value::Object(now),
                None => moved.push((path, Value::Object(now))),
            }
        }
        current.insert(
            js::stringify(cb.get("key")),
            moved.into_iter().map(|(_, l)| l).collect(),
        );
    }
    let comps = components_of(baseline);
    out.extend(diff_components(baseline, changes, dims, &mut warnings)?);
    for t in list(changes.get("comments")) {
        let b = js::truthy_value(t.get("boardKey")).and_then(|k| board_of(Some(k)));
        let (x, y) = (js::to_number(t.get("x")), js::to_number(t.get("y")));
        let host = b.and_then(|b| match current.get(&js::stringify(b.get("key"))) {
            Some(now) => deepest_at(now.iter(), x, y).cloned(),
            None => deepest_at(Layers::of(b.get("layers")).order.into_iter(), x, y).cloned(),
        });
        let comp = match (b, js::truthy_value(t.get("componentKey"))) {
            (None, Some(key)) => comps.get(&js::stringify(Some(key))).copied(),
            _ => None,
        };
        let messages: Vec<Value> = list(t.get("comments"))
            .iter()
            .filter(|c| !is_dfw_reply(c.get("content")))
            .map(|c| {
                js::object(vec![
                    (
                        "author",
                        Some(
                            js::truthy_value(c.get("user"))
                                .cloned()
                                .unwrap_or_else(|| "designer".into()),
                        ),
                    ),
                    ("content", c.get("content").cloned()),
                    ("at", Some(js::or_null(c.get("date")))),
                ])
            })
            .collect();
        out.push(change(
            "comment",
            vec![
                b.map_or_else(nowhere, where_board),
                vec![
                    ("component", comp.map(comp_info)),
                    ("anchor", Some(anchor_of(host.as_ref()))),
                    ("bbox", Some(host.as_ref().map_or(Value::Null, bbox))),
                    (
                        "thread",
                        Some(js::present(t.get("seq")).cloned().unwrap_or(Value::Null)),
                    ),
                    ("resolved", Some(js::truthy(t.get("resolved")).into())),
                    ("messages", Some(Value::Array(messages))),
                ],
            ],
        ));
    }
    Ok(Diff {
        changes: out,
        warnings,
    })
}

/// The changes of one screen board. Returns the layers whose path the export does not know.
// Long: one block per kind of change on a layer (text, box, fill, name), then removed and
// added layers, as in the Node function.
fn diff_board(
    b: &Value,
    cb: &Value,
    baseline: &Value,
    out: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let flat = Layers::of(b.get("layers"));
    let delta = |n: &Value, l: &Value, k: &str| js::to_number(n.get(k)) - js::to_number(l.get(k));
    let mut deltas: HashMap<String, (f64, f64)> = HashMap::new();
    for n in list(cb.get("layers")) {
        if let Some(l) = flat.get(n.get("path")) {
            deltas.insert(
                js::stringify(n.get("path")),
                (delta(n, l, "x"), delta(n, l, "y")),
            );
        }
    }
    // A layer that moved exactly like its parent was not moved itself.
    let own_move = |path: Option<&str>, dx: f64, dy: f64| {
        let parent = path
            .and_then(parent_path)
            .and_then(|p| deltas.get(&js::stringify(Some(&Value::from(p)))));
        !parent.is_some_and(|(px, py)| (px - dx).abs() <= TOL && (py - dy).abs() <= TOL)
    };
    let mut present: Vec<String> = Vec::new();
    let mut unknown = Vec::new();
    for n in list(cb.get("layers")) {
        let Some(l) = flat.get(n.get("path")) else {
            warnings.push(format!(
                "{}: layer path {} unknown in the export; treated as added.",
                js::to_string(b.get("name")),
                js::to_string(n.get("path"))
            ));
            unknown.push(n.clone());
            continue;
        };
        present.push(js::stringify(n.get("path")));
        let target = || -> Pairs {
            vec![
                ("anchor", l.get("anchor").cloned()),
                ("dfwId", l.get("dfwId").cloned()),
                ("path", l.get("path").cloned()),
                ("name", l.get("name").cloned()),
                ("bbox", Some(bbox(l))),
                ("kind", l.get("kind").cloned()),
            ]
        };
        if is_text(l)
            && n.get("characters").is_some()
            && js::norm(n.get("characters")) != js::norm(l.get("characters"))
        {
            out.push(change(
                "text",
                vec![
                    where_board(b),
                    target(),
                    vec![
                        ("from", l.get("characters").cloned()),
                        ("to", n.get("characters").cloned()),
                        ("placeholder", Some(js::truthy(l.get("placeholder")).into())),
                    ],
                ],
            ));
        }
        let (dx, dy) = (delta(n, l, "x"), delta(n, l, "y"));
        let (dw, dh) = (delta(n, l, "w"), delta(n, l, "h"));
        let moved = (dx.abs() > TOL || dy.abs() > TOL)
            && own_move(n.get("path").and_then(Value::as_str), dx, dy);
        // Text boxes follow Penpot's font metrics (auto width/height): their size is not a
        // design change.
        let resized = !is_text(l) && (dw.abs() > TOL || dh.abs() > TOL);
        if moved || resized {
            out.push(change(
                if resized { "resize" } else { "move" },
                vec![
                    where_board(b),
                    target(),
                    vec![
                        ("from", Some(bbox(l))),
                        ("to", Some(bbox(n))),
                        (
                            "delta",
                            Some(json!({ "x": js::num(dx), "y": js::num(dy), "w": js::num(dw), "h": js::num(dh) })),
                        ),
                    ],
                ],
            ));
        }
        let text_color = l.get("text").and_then(|t| t.get("color"));
        let fill = l.get("fills").and_then(|f| f.get(0));
        let before = js::truthy_value(if is_text(l) { text_color } else { fill });
        let after = first_of(n.get("fills"));
        let after = after.as_ref();
        match (after, before) {
            (Some(after), Some(before)) => {
                let before_color = js::truthy_value(before.get("hex")).or(before.get("color"));
                if paint_differs(after, before_color, before) {
                    let token = if is_text(l) {
                        l.get("text").and_then(|t| t.get("colorToken"))
                    } else {
                        fill.and_then(|f| f.get("token"))
                    };
                    out.push(change(
                        "fill",
                        vec![
                            where_board(b),
                            target(),
                            vec![
                                (
                                    "from",
                                    Some(js::object(vec![
                                        ("color", before_color.cloned()),
                                        ("opacity", Some(or_one(before.get("opacity")))),
                                        ("token", token.cloned()),
                                    ])),
                                ),
                                (
                                    "to",
                                    Some(json!({
                                        "color": lower(after.get("color")),
                                        "opacity": or_one(after.get("opacity")),
                                        "token": color_token(baseline, after),
                                    })),
                                ),
                            ],
                        ],
                    ));
                }
            }
            (Some(after), None) if !is_text(l) => out.push(change(
                "fill",
                vec![
                    where_board(b),
                    target(),
                    vec![
                        ("from", Some(Value::Null)),
                        (
                            "to",
                            Some(json!({ "color": lower(after.get("color")), "opacity": or_one(after.get("opacity")) })),
                        ),
                    ],
                ],
            )),
            _ => {}
        }
        if let Some(name) = js::truthy_value(n.get("name")) {
            if !js::strict_eq(Some(name), l.get("name")) && js::truthy(l.get("dfwId")) {
                out.push(change(
                    "rename",
                    vec![
                        where_board(b),
                        target(),
                        vec![("from", l.get("name").cloned()), ("to", Some(name.clone()))],
                    ],
                ));
            }
        }
    }
    // Removed: topmost layers of the export missing from the board.
    let is_present = |path: &str| present.iter().any(|p| p == path);
    for l in &flat.order {
        if is_present(&js::stringify(l.get("path"))) {
            continue;
        }
        let parent = l
            .get("path")
            .and_then(Value::as_str)
            .and_then(parent_path)
            .map(Value::from);
        if let Some(parent) = &parent {
            if !is_present(&js::stringify(Some(parent))) && flat.has(Some(parent)) {
                continue;
            }
        }
        out.push(change(
            "removed",
            vec![
                where_board(b),
                vec![
                    ("anchor", l.get("anchor").cloned()),
                    ("dfwId", l.get("dfwId").cloned()),
                    ("path", l.get("path").cloned()),
                    ("name", l.get("name").cloned()),
                    ("bbox", Some(bbox(l))),
                    ("kind", l.get("kind").cloned()),
                    ("text", l.get("characters").cloned()),
                ],
            ],
        ));
    }
    let sent_added = js::truthy_value(cb.get("added"));
    for a in list(sent_added).iter().chain(&unknown) {
        let (x, y) = centre(a);
        let host = deepest_at(flat.order.iter().copied(), x, y);
        out.push(change(
            "added",
            vec![
                where_board(b),
                vec![
                    ("anchor", Some(anchor_of(host))),
                    ("name", a.get("name").cloned()),
                    (
                        "kind",
                        js::truthy_value(a.get("kind")).or(a.get("type")).cloned(),
                    ),
                    ("bbox", Some(bbox(a))),
                    ("characters", a.get("characters").cloned()),
                ],
            ],
        ));
    }
    unknown
}

/* ------------------------------------------------------------------ component masters (dfw components page) */

static RADIUS_NAME: LazyLock<Regex> =
    // The patterns of this file are constants: known to compile.
    LazyLock::new(|| Regex::new(r"(^|\.)radius(\.|$)").expect("constant pattern"));
static BORDER_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(border|stroke|focus)[.-]?width").expect("constant pattern"));
static SPACE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|\.)(space|spacing|gap|inset|padding)(\.|$)").expect("constant pattern")
});
static SIZE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|\.)(size|sizing|width|height)(\.|$)").expect("constant pattern")
});
static MAIN_PREFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^main:[^#]*#").expect("constant pattern"));

/// A dimension token in px, with what it is for.
#[derive(Debug, Clone, PartialEq)]
pub struct Dim {
    pub name: String,
    pub px: f64,
    /// `radius`, `border`, `space` or `size`
    pub role: &'static str,
    pub alias: bool,
}

/// `dimensionTokenIndex`: dimension tokens in px (aliases followed; rem = 16px), by role.
pub fn dimension_token_index(tree: Option<&Value>) -> Vec<Dim> {
    let empty = json!({});
    let flat = flatten_tokens(js::truthy_value(tree).unwrap_or(&empty));
    let mut out = Vec::new();
    for (name, t) in flat.iter() {
        if t.type_name() != Some("dimension") || name.starts_with("font.") {
            continue;
        }
        let value = resolve_alias(&flat, name).map(|r| &r.value);
        let unit = value.and_then(|v| v.get("unit")).and_then(Value::as_str);
        let amount = value.and_then(|v| v.get("value"));
        let px = match unit {
            Some("px") => amount.and_then(Value::as_f64),
            Some("rem") => Some(js::to_number(amount) * 16.0),
            _ => None,
        };
        let Some(px) = px else {
            continue;
        };
        let role = if RADIUS_NAME.is_match(name) {
            "radius"
        } else if BORDER_NAME.is_match(name) {
            "border"
        } else if SPACE_NAME.is_match(name) {
            "space"
        } else if SIZE_NAME.is_match(name) {
            "size"
        } else {
            continue;
        };
        out.push(Dim {
            name: name.to_string(),
            px,
            role,
            alias: t.value.is_string(),
        });
    }
    // Prefer semantic aliases over scale steps, like colours.
    out.sort_by_key(|d| !d.alias);
    out
}

/// The token of a role with this px value; `px` is `None` for a value that is not a number.
fn find_dim(dims: &[Dim], role: &str, px: Option<f64>) -> Value {
    px.and_then(|px| {
        dims.iter()
            .find(|d| d.role == role && (d.px - px).abs() < 0.5)
    })
    .filter(|d| !d.name.is_empty())
    .map_or(Value::Null, |d| d.name.as_str().into())
}

fn components_of(baseline: &Value) -> HashMap<String, &Value> {
    list(baseline.get("components"))
        .iter()
        .map(|c| (js::stringify(c.get("key")), c))
        .collect()
}

fn comp_info(c: &Value) -> Value {
    js::object(vec![
        ("key", c.get("key").cloned()),
        ("name", c.get("component").cloned()),
        ("variant", Some(js::or_null(c.get("variant")))),
        ("size", Some(js::or_null(c.get("size")))),
        ("title", c.get("name").cloned()),
    ])
}

/// Insets of a frame's children: left/top from all children, right/bottom from non-text ones
/// (Penpot's font metrics differ). `[top, right, bottom, left]`; `None` is no such inset.
fn insets(parent: &Value, kids: &[&Value]) -> [Option<f64>; 4] {
    let n = |l: &Value, k: &str| js::to_number(l.get(k));
    let boxes: Vec<&&Value> = kids.iter().filter(|k| !is_text(k)).collect();
    let min = |k: &str| kids.iter().fold(f64::INFINITY, |m, l| js::min(m, n(l, k)));
    let far = |pos: &str, len: &str| {
        boxes
            .iter()
            .fold(f64::NEG_INFINITY, |m, l| js::max(m, n(l, pos) + n(l, len)))
    };
    let has_boxes = !boxes.is_empty();
    [
        Some(min("y") - n(parent, "y")),
        has_boxes.then(|| n(parent, "x") + n(parent, "w") - far("x", "w")),
        has_boxes.then(|| n(parent, "y") + n(parent, "h") - far("y", "h")),
        Some(min("x") - n(parent, "x")),
    ]
}

/// `[a, b]` of two raw values: a missing one is `null`.
fn pair(a: Option<&Value>, b: Option<&Value>) -> Value {
    json!([a, b])
}

/// Edits on the "dfw components" page: each Penpot main component (keyed by the master,
/// variant and size stored in its plugin data) compared with the master as exported.
fn diff_components(
    baseline: &Value,
    changes: &Value,
    dims: &[Dim],
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>, DfwError> {
    let comps = components_of(baseline);
    let baseline_id = js::to_string(baseline.get("id"));
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    walkable(changes.get("components"), Some("key"), false)?;
    walkable(changes.get("missingComponents"), None, false)?;
    for cc in list(changes.get("components")) {
        let Some(c) = comps.get(&js::stringify(cc.get("key"))).copied() else {
            let name = js::truthy_value(cc.get("name")).or(cc.get("key"));
            warnings.push(format!(
                "Component \"{}\" is not in export {baseline_id}: skipped.",
                js::to_string(name)
            ));
            continue;
        };
        seen.push(js::stringify(c.get("key")));
        walkable(cc.get("layers"), Some("root"), false)?;
        walkable(cc.get("added"), Some("x"), false)?;
        diff_component(c, cc, baseline, dims, &mut out, warnings);
    }
    for key in list(changes.get("missingComponents")) {
        let key = js::stringify(Some(key));
        let Some(c) = comps.get(&key).filter(|_| !seen.contains(&key)) else {
            continue;
        };
        let root = c.get("root");
        let mut pairs = nowhere();
        pairs.extend([
            ("component", Some(comp_info(c))),
            ("path", root.and_then(|r| r.get("path")).cloned()),
            ("name", c.get("name").cloned()),
            ("kind", root.and_then(|r| r.get("kind")).cloned()),
            ("isRoot", Some(Value::Bool(true))),
            ("tag", Some(js::or_null(root.and_then(|r| r.get("tag"))))),
        ]);
        out.push(change("component-removed", vec![pairs]));
    }
    Ok(out)
}

/// The changes of one main component.
// Long: one block per property the designer can change, as in the Node function.
fn diff_component(
    c: &Value,
    cc: &Value,
    baseline: &Value,
    dims: &[Dim],
    out: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) {
    let null = Value::Null;
    let root = c.get("root").unwrap_or(&null);
    let root_path = root.get("path");
    let location = || -> Pairs {
        let mut pairs = nowhere();
        pairs.push(("component", Some(comp_info(c))));
        pairs
    };
    let roots = json!([root]);
    let flat = Layers::of(Some(&roots));
    let mut sent: Vec<(String, Value)> = Vec::new();
    let mut added: Vec<Value> = list(js::truthy_value(cc.get("added"))).to_vec();
    for n in list(cc.get("layers")) {
        let path: Value = if js::truthy(n.get("root")) {
            root_path.cloned().unwrap_or(Value::Null)
        } else {
            let raw = js::present(n.get("path")).map_or(String::new(), |p| js::to_string(Some(p)));
            MAIN_PREFIX.replace(&raw, "").into_owned().into()
        };
        // The baseline's own layers are borrowed from `roots`, not from `c`: look them up by key.
        if flat.has(Some(&path)) {
            let mut layer = js::spread(Some(n));
            layer.insert("path".into(), path.clone());
            let key = js::stringify(Some(&path));
            match sent.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = Value::Object(layer),
                None => sent.push((key, Value::Object(layer))),
            }
        } else {
            warnings.push(format!(
                "{}: layer path {} unknown in the export; treated as added.",
                js::to_string(c.get("name")),
                js::to_string(n.get("path"))
            ));
            added.push(n.clone());
        }
    }
    let sent_layer = |path: Option<&Value>| {
        let key = js::stringify(path);
        sent.iter().find(|(k, _)| *k == key).map(|(_, n)| n)
    };
    let is_root = |l: &Value| js::strict_eq(l.get("path"), root_path);
    let label = |l: &Value| -> Option<Value> {
        if is_root(l) {
            c.get("name").cloned()
        } else if is_text(l) {
            let shown = js::slice16(&js::norm(l.get("characters")), 0, 40);
            Some(format!("text \"{shown}\"").into())
        } else {
            l.get("name").cloned()
        }
    };
    let target = |l: &Value| -> Pairs {
        vec![
            ("path", l.get("path").cloned()),
            ("name", label(l)),
            ("kind", l.get("kind").cloned()),
            ("tag", Some(js::or_null(l.get("tag")))),
            ("isRoot", Some(is_root(l).into())),
            ("bbox", Some(bbox(l))),
        ]
    };
    if let Some(name) = js::truthy_value(cc.get("name")) {
        if js::norm(Some(name)) != js::norm(c.get("name")) {
            out.push(change(
                "rename",
                vec![
                    location(),
                    target(root),
                    vec![("from", c.get("name").cloned()), ("to", Some(name.clone()))],
                ],
            ));
        }
    }
    for l in &flat.order {
        let Some(n) = sent_layer(l.get("path")) else {
            continue;
        };
        let mut push =
            |ty: &str, extra: Pairs| out.push(change(ty, vec![location(), target(l), extra]));
        if is_text(l)
            && n.get("characters").is_some()
            && js::norm(n.get("characters")) != js::norm(l.get("characters"))
        {
            push(
                "text",
                vec![
                    ("from", l.get("characters").cloned()),
                    ("to", n.get("characters").cloned()),
                ],
            );
        }
        // Fill (text colour for text layers).
        let fill = l.get("fills").and_then(|f| f.get(0));
        let before = js::truthy_value(if is_text(l) {
            l.get("text").and_then(|t| t.get("color"))
        } else {
            fill
        });
        let after = first_of(n.get("fills"));
        let after = after.as_ref();
        let before_color = before.and_then(|b| js::truthy_value(b.get("hex")).or(b.get("color")));
        let fill_changed = match (after, before) {
            (Some(_), None) => !is_text(l),
            (Some(after), Some(before)) => paint_differs(after, before_color, before),
            (None, _) => false,
        };
        if let (Some(after), true) = (after, fill_changed) {
            let from = before.map_or(Value::Null, |before| {
                let token = if is_text(l) {
                    l.get("text").and_then(|t| t.get("colorToken"))
                } else {
                    before.get("token")
                };
                js::object(vec![
                    ("color", before_color.cloned()),
                    ("opacity", Some(or_one(before.get("opacity")))),
                    ("token", token.cloned()),
                ])
            });
            push(
                "fill",
                vec![
                    ("from", Some(from)),
                    (
                        "to",
                        Some(json!({
                            "color": lower(after.get("color")),
                            "opacity": or_one(after.get("opacity")),
                            "token": color_token(baseline, after),
                        })),
                    ),
                ],
            );
        } else if let (None, Some(before), false, true) =
            (after, before, is_text(l), n.get("fills").is_some())
        {
            push(
                "fill",
                vec![
                    (
                        "from",
                        Some(js::object(vec![
                            ("color", before.get("color").cloned()),
                            ("opacity", Some(or_one(before.get("opacity")))),
                            ("token", before.get("token").cloned()),
                        ])),
                    ),
                    ("to", Some(Value::Null)),
                ],
            );
        }
        // Stroke (border): colour, width, added or removed.
        let sb = js::truthy_value(l.get("strokes").and_then(|s| s.get(0)));
        let sa = first_of(n.get("strokes"));
        let sa = sa.as_ref();
        if n.get("strokes").is_some() && (sb.is_some() || sa.is_some()) {
            let width = |s: &Value| js::to_number(Some(&or_one(s.get("width"))));
            let changed = match (sb, sa) {
                (Some(sb), Some(sa)) => {
                    paint_differs(sa, sb.get("color"), sb)
                        || (width(sa) - js::to_number(sb.get("width"))).abs() > 0.25
                }
                _ => true,
            };
            if changed {
                let number = |v: Option<&Value>| v.and_then(Value::as_f64);
                let from = sb.map_or(Value::Null, |sb| {
                    js::object(vec![
                        ("color", sb.get("color").cloned()),
                        ("opacity", Some(or_one(sb.get("opacity")))),
                        ("width", sb.get("width").cloned()),
                        ("token", Some(js::or_null(sb.get("token")))),
                        (
                            "widthToken",
                            Some(find_dim(dims, "border", number(sb.get("width")))),
                        ),
                    ])
                });
                let to = sa.map_or(Value::Null, |sa| {
                    let w = or_one(sa.get("width"));
                    json!({
                        "color": lower(sa.get("color")),
                        "opacity": or_one(sa.get("opacity")),
                        "width": w,
                        "token": color_token(baseline, sa),
                        "widthToken": find_dim(dims, "border", w.as_f64()),
                    })
                });
                push("stroke", vec![("from", Some(from)), ("to", Some(to))]);
            }
        }
        // Corner radius.
        if let (Some(radius), false) = (n.get("radius").and_then(Value::as_array), is_text(l)) {
            let zeros = json!([0, 0, 0, 0]);
            let rb = js::truthy_value(l.get("radius")).unwrap_or(&zeros);
            let or_zero =
                |v: Option<&Value>| js::truthy_value(v).map_or(0.0, |v| js::to_number(Some(v)));
            let differs = radius
                .iter()
                .enumerate()
                .any(|(i, v)| (or_zero(Some(v)) - or_zero(rb.get(i))).abs() > 0.5);
            if differs {
                let first = or_zero(radius.first());
                let uniform = radius
                    .iter()
                    .all(|v| (or_zero(Some(v)) - first).abs() < 0.01);
                let corners: Vec<Value> = radius
                    .iter()
                    .map(|v| js::truthy_value(Some(v)).cloned().unwrap_or(json!(0)))
                    .collect();
                // `findDim` wants a number: a truthy corner that is not one finds no token.
                let first_number = match js::truthy_value(radius.first()) {
                    None => Some(0.0),
                    Some(v) => v.as_f64(),
                };
                let token = if uniform {
                    find_dim(dims, "radius", first_number)
                } else {
                    Value::Null
                };
                push(
                    "radius",
                    vec![
                        (
                            "from",
                            Some(
                                json!({ "radius": rb, "token": js::or_null(l.get("radiusToken")) }),
                            ),
                        ),
                        ("to", Some(json!({ "radius": corners, "token": token }))),
                    ],
                );
            }
        }
        // Size (not for text: Penpot's font metrics differ).
        let grew = |k: &str| (js::to_number(n.get(k)) - js::to_number(l.get(k))).abs() > TOL;
        if !is_text(l) && (grew("w") || grew("h")) {
            let token = |k: &str| {
                if grew(k) {
                    find_dim(dims, "size", n.get(k).and_then(Value::as_f64))
                } else {
                    Value::Null
                }
            };
            push(
                "size",
                vec![
                    ("from", Some(pair(l.get("w"), l.get("h")))),
                    ("to", Some(pair(n.get("w"), n.get("h")))),
                    ("tokens", Some(json!({ "w": token("w"), "h": token("h") }))),
                ],
            );
        }
        // Spacing: the insets of the children inside this frame (padding), compared side by side.
        let kids: Vec<&Value> = list(l.get("children"))
            .iter()
            .filter(|k| sent_layer(k.get("path")).is_some())
            .collect();
        if !kids.is_empty() {
            let now: Vec<&Value> = kids
                .iter()
                .filter_map(|k| sent_layer(k.get("path")))
                .collect();
            let (ib, ia) = (insets(l, &kids), insets(n, &now));
            let sw = sb.map_or(json!(0), |sb| {
                sb.get("width").cloned().unwrap_or(Value::Null)
            });
            let border = js::to_number(Some(&sw));
            let mut sides = Vec::new();
            for (i, side) in ["top", "right", "bottom", "left"].into_iter().enumerate() {
                let (Some(from), Some(to)) = (ib[i], ia[i]) else {
                    continue;
                };
                if (to - from).abs() > TOL {
                    let token = match find_dim(dims, "space", Some(to - border)) {
                        Value::Null => find_dim(dims, "space", Some(to)),
                        token => token,
                    };
                    sides.push((
                        side,
                        Some(json!({ "from": js::num(from), "to": js::num(to), "token": token })),
                    ));
                }
            }
            if !sides.is_empty() {
                // The stroke of a layer that has one always carries a width; without it the
                // Node object held `undefined` and JSON dropped the key.
                let border = sb.map_or(Some(json!(0)), |sb| sb.get("width").cloned());
                push(
                    "spacing",
                    vec![("sides", Some(js::object(sides))), ("border", border)],
                );
            }
        }
        if let Some(name) = js::truthy_value(n.get("name")) {
            if !is_root(l) && !js::strict_eq(Some(name), l.get("name")) {
                push(
                    "rename",
                    vec![("from", l.get("name").cloned()), ("to", Some(name.clone()))],
                );
            }
        }
    }
    for l in &flat.order {
        if sent_layer(l.get("path")).is_some() {
            continue;
        }
        let parent = l
            .get("path")
            .and_then(Value::as_str)
            .and_then(parent_path)
            .map(Value::from);
        if let Some(parent) = &parent {
            if sent_layer(Some(parent)).is_none() && flat.has(Some(parent)) {
                continue;
            }
        }
        out.push(change(
            "removed",
            vec![
                location(),
                target(l),
                vec![("text", l.get("characters").cloned())],
            ],
        ));
    }
    for a in &added {
        let (cx, cy) = centre(a);
        let mut host = root;
        for l in &flat.order {
            if !is_text(l) && contains(l, cx, cy) && area(l) <= area(host) {
                host = l;
            }
        }
        out.push(change(
            "added",
            vec![
                location(),
                target(host),
                vec![
                    (
                        "kind",
                        js::truthy_value(a.get("kind")).or(a.get("type")).cloned(),
                    ),
                    ("layer", a.get("name").cloned()),
                    ("bbox", Some(bbox(a))),
                    ("characters", a.get("characters").cloned()),
                ],
            ],
        ));
    }
}

/* ------------------------------------------------------------------ where a master change lands */

/// The CSS properties a change type is made with.
fn css_property(change: &Value) -> Option<&'static str> {
    Some(match change.get("type").and_then(Value::as_str)? {
        "fill" if kind(change) == Some("text") => "color",
        "fill" => "background(-color)?",
        "stroke" => "border(-color|-width|-style)?|outline",
        "radius" => "border-radius|border-(top|bottom)-(left|right)-radius",
        "spacing" => "padding(-top|-right|-bottom|-left|-inline|-block)?|gap",
        "size" => "height|width|min-height|min-width|block-size|inline-size",
        _ => return None,
    })
}

static CSS_RULE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"([^{}<>]+)\{([^{}]*)\}").expect("constant pattern"));
static STATE_SELECTOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r":(hover|focus|focus-visible|focus-within|active|disabled|checked)|\[(aria|data)-")
        .expect("constant pattern")
});
static TEXT_SELECTOR: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?-u:\b)(span|p|label|strong)(?-u:\b)").expect("constant pattern")
});
static CLASS_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^[a-z][0-9A-Za-z_-]*\.([0-9A-Za-z_-]+)$").expect("constant pattern")
});

/// Where a master change is made: the file, and `file:line` when the file exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MasterSource {
    pub file: String,
    pub source: Option<String>,
}

fn find_from(text: &str, needle: &str, from: usize) -> Option<usize> {
    text.get(from..)?.find(needle).map(|i| i + from)
}

/// `/<(?!template\b)[a-z]/i`: the first tag that is not a template tag.
fn first_element(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len()).find(|&i| {
        if bytes[i] != b'<' || !bytes.get(i + 1).is_some_and(u8::is_ascii_alphabetic) {
            return false;
        }
        let name = &bytes[i + 1..];
        let template = name.len() >= 8
            && name[..8].eq_ignore_ascii_case(b"template")
            && !name
                .get(8)
                .is_some_and(|b| b.is_ascii_alphanumeric() || *b == b'_');
        !template
    })
}

/// `masterSource`: `design/system/components/<file>:<line>` for a master change: the CSS rule
/// that sets the property (variant/size rules first), else the element in the template (by
/// tag/class), else the component tag. `None` when the change is not on a component.
// Long: three lookups in one text (CSS rule, template element, component tag) that share
// the offsets of the master block.
pub fn master_source(root: &Path, change: &Value, lib: Option<&Library>) -> Option<MasterSource> {
    let comp = js::truthy_value(change.get("component"))?;
    let comp_name = comp.get("name").and_then(Value::as_str);
    let loaded;
    let lib = match lib {
        Some(lib) => Some(lib),
        None => {
            loaded = load_components(root).ok();
            loaded.as_ref()
        }
    };
    let def = lib.and_then(|lib| lib.get(comp_name));
    let rel = match def.map(|d| d.file.as_str()).filter(|f| !f.is_empty()) {
        Some(file) => file.to_string(),
        None => format!(
            "design/system/components/{}.html",
            js::to_string(comp.get("name"))
        ),
    };
    let abs = root.join(&rel);
    if !abs.exists() {
        return Some(MasterSource {
            file: rel,
            source: None,
        });
    }
    let text = js::read_text(&abs).ok()?;
    let start = match def.map(|d| d.line).filter(|line| *line > 0) {
        // The end of the line before the master's, as the Node code counted it.
        Some(1) => 0,
        Some(line) => text
            .match_indices('\n')
            .nth(line - 2)
            .map_or(text.len(), |(i, _)| i),
        None => text.find("<dfw-component").unwrap_or(0),
    };
    let block = &text[..find_from(&text, "</dfw-component>", start).unwrap_or(text.len())];
    let at = |i: usize| {
        let line = text[..i.min(text.len())].matches('\n').count() + 1;
        Some(MasterSource {
            file: rel.clone(),
            source: Some(format!("{rel}:{line}")),
        })
    };
    let name =
        js::truthy_value(change.get("name")).map_or(String::new(), |n| js::to_string(Some(n)));
    let cls = CLASS_NAME.captures(&name).map(|m| m[1].to_string());
    let is_text_change = kind(change) == Some("text");
    if let Some(prop) = css_property(change) {
        let style_at = find_from(block, "<style", start);
        let style_end = style_at.and_then(|s| find_from(block, "</style>", s));
        if let (Some(style_at), Some(style_end)) = (style_at, style_end.filter(|e| *e > 0)) {
            let css = &block[style_at..style_end];
            let decl = Regex::new(&format!(r"(^|[;{{{s}])({prop})[{s}]*:", s = js::SPACE)).ok()?;
            let named = |value: Option<&Value>| {
                js::truthy_value(value).and_then(|v| {
                    Regex::new(&format!(
                        r#"(--|[-_.{s}="']){}(?-u:\b)"#,
                        regex::escape(&js::to_string(Some(v))),
                        s = js::SPACE
                    ))
                    .ok()
                })
            };
            let (variant, size) = (named(comp.get("variant")), named(comp.get("size")));
            let mut best: Option<(i32, usize)> = None;
            for m in CSS_RULE.captures_iter(css) {
                let (whole, selector) = (m.get(0)?, js::trim(m.get(1)?.as_str()));
                if !decl.is_match(&format!("{{{}", &m[2])) || selector.contains('@') {
                    continue;
                }
                let mut score = 0;
                if STATE_SELECTOR.is_match(selector) {
                    score -= 10;
                }
                if variant.as_ref().is_some_and(|re| re.is_match(selector)) {
                    score += 4;
                }
                if size.as_ref().is_some_and(|re| re.is_match(selector)) {
                    score += 2;
                }
                if cls
                    .as_ref()
                    .is_some_and(|cls| selector.contains(&format!(".{cls}")))
                {
                    score += 1;
                }
                if is_text_change && TEXT_SELECTOR.is_match(selector) {
                    score += 1;
                }
                if best.is_none_or(|(s, _)| score > s) {
                    let offset = whole.as_str().find(selector).unwrap_or(0);
                    best = Some((score, style_at + whole.start() + offset));
                }
            }
            if let Some((_, i)) = best.filter(|(score, _)| *score > -10) {
                return at(i);
            }
        }
    }
    if let Some(tpl_at) = find_from(block, "<template", start) {
        let tpl_end = find_from(block, "</template>", tpl_at)
            .filter(|e| *e > 0)
            .unwrap_or(block.len());
        let tpl = &block[tpl_at..tpl_end];
        if change.get("type").and_then(Value::as_str) == Some("text") {
            if let Some(slot) = tpl.find("<dfw-slot") {
                return at(tpl_at + slot);
            }
        }
        if !js::truthy(change.get("isRoot")) {
            let tag = js::truthy_value(change.get("tag")).map(|t| js::to_string(Some(t)));
            let re = match (&cls, tag) {
                (Some(cls), _) => Regex::new(&format!(
                    r#"(?i)<[a-z][0-9A-Za-z_-]*[^>]*class="[^"]*(?-u:\b){}(?-u:\b)"#,
                    regex::escape(cls)
                ))
                .ok(),
                (None, Some(tag)) => Regex::new(&format!(r"(?i)<{tag}(?-u:\b)")).ok(),
                (None, None) => None,
            };
            if let Some(m) = re.and_then(|re| re.find(tpl)) {
                return at(tpl_at + m.start());
            }
        }
        if let Some(first) = tpl.get(1..).and_then(first_element) {
            return at(tpl_at + 1 + first);
        }
    }
    at(start)
}

/* ------------------------------------------------------------------ sentences */

/// `Math.round(value)` as text.
fn rounded(v: Option<&Value>) -> String {
    js::number(js::round(js::to_number(v)))
}

/// `Math.round(value || 0)` of every item, joined.
fn rounded_list(v: Option<&Value>) -> String {
    list(v)
        .iter()
        .map(|x| rounded(js::truthy_value(Some(x)).or(Some(&json!(0)))))
        .collect::<Vec<_>>()
        .join(", ")
}

fn fmt_color(c: Option<&Value>) -> String {
    let Some(c) = js::truthy_value(c) else {
        return "none".to_string();
    };
    let opacity = match c.get("opacity") {
        Some(o) if js::to_number(Some(o)) < 1.0 => {
            format!(
                " @{}%",
                js::number(js::round(js::to_number(Some(o)) * 100.0))
            )
        }
        _ => String::new(),
    };
    let token = js::truthy_value(c.get("token"))
        .map_or(String::new(), |t| format!(" ({})", js::to_string(Some(t))));
    format!("{}{opacity}{token}", js::to_string(c.get("color")))
}

fn has_token(c: Option<&Value>) -> bool {
    js::truthy(c.and_then(|c| c.get("token")))
}

/// `("<text>")` for a removed or added text, cut at `max` characters.
fn quoted(v: Option<&Value>, max: usize) -> String {
    js::slice16(&js::norm(v), 0, max)
}

fn moved_beyond(c: &Value) -> bool {
    let d = |k: &str| js::to_number(c.get("delta").and_then(|d| d.get(k))).abs();
    d("x") > TOL || d("y") > TOL
}

/// `describeChange`: one line for a change on a screen board.
pub fn describe_change(c: &Value) -> String {
    let at = |key: &str, i: usize| rounded(c.get(key).and_then(|v| v.get(i)));
    // A delta is computed: `null` in it is what JSON keeps of NaN.
    let delta = |k: &str| match c.get("delta").and_then(|d| d.get(k)) {
        Some(Value::Null) => "NaN".to_string(),
        other => rounded(other),
    };
    let s = |key: &str| js::to_string(c.get(key));
    match c.get("type").and_then(Value::as_str) {
        Some("text") => format!(
            "{} changed: \"{}\" -> \"{}\"",
            if js::truthy(c.get("placeholder")) {
                "Placeholder"
            } else {
                "Text"
            },
            js::norm(c.get("from")),
            js::norm(c.get("to"))
        ),
        Some("move") => format!(
            "Moved by {}, {} px (now x {}, y {})",
            delta("x"),
            delta("y"),
            at("to", 0),
            at("to", 1)
        ),
        Some("resize") => format!(
            "Resized {}x{} -> {}x{} px{}",
            at("from", 2),
            at("from", 3),
            at("to", 2),
            at("to", 3),
            if moved_beyond(c) {
                format!(", moved by {}, {}", delta("x"), delta("y"))
            } else {
                String::new()
            }
        ),
        Some("fill") => format!(
            "Colour changed: {} -> {}{}",
            fmt_color(c.get("from")),
            fmt_color(c.get("to")),
            if js::truthy(c.get("to")) && !has_token(c.get("to")) {
                " (no token has this value: use a token or propose one)"
            } else {
                ""
            }
        ),
        Some("rename") => format!(
            "Layer renamed \"{}\" -> \"{}\" (data-dfw-id stays; rename only if intended)",
            s("from"),
            s("to")
        ),
        Some("removed") => removed_line(c),
        Some("added") => format!(
            "Added in Penpot: {} \"{}\" at {}{}",
            s("kind"),
            s("name"),
            rounded_list(c.get("bbox")),
            added_text(c)
        ),
        Some("board-removed") => "Board deleted in Penpot".to_string(),
        Some("comment") => list(c.get("messages"))
            .iter()
            .map(|m| {
                format!(
                    "{}: {}",
                    js::to_string(m.get("author")),
                    js::to_string(m.get("content"))
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => s("type"),
    }
}

fn removed_line(c: &Value) -> String {
    let text = if js::truthy(c.get("text")) {
        format!(" (\"{}\")", quoted(c.get("text"), 60))
    } else {
        String::new()
    };
    format!(
        "Removed in Penpot: {} \"{}\"{text}",
        js::to_string(c.get("kind")),
        js::to_string(c.get("name"))
    )
}

fn added_text(c: &Value) -> String {
    if js::truthy(c.get("characters")) {
        format!(" with text \"{}\"", quoted(c.get("characters"), 80))
    } else {
        String::new()
    }
}

/// `${Math.round(v * 100) / 100}px`.
fn fmt_px(v: Option<&Value>) -> String {
    format!(
        "{}px",
        js::number(js::round(js::to_number(v) * 100.0) / 100.0)
    )
}

fn token_or(token: Option<&Value>, what: &str) -> String {
    match js::truthy_value(token) {
        Some(t) => format!(" ({})", js::to_string(Some(t))),
        None => format!(" (no {what} token has this value: use a token or propose one)"),
    }
}

/// `describeComponentChange`: one line for a change on a component master (dfw components
/// page).
// Long: one sentence per change type.
pub fn describe_component_change(c: &Value) -> String {
    let s = |key: &str| js::to_string(c.get(key));
    let (from, to) = (c.get("from"), c.get("to"));
    let sub = |v: Option<&Value>, key: &str| js::to_string(v.and_then(|v| v.get(key)));
    let no_colour_token = |v: Option<&Value>| {
        if has_token(v) {
            ""
        } else {
            " (no colour token has this value)"
        }
    };
    match c.get("type").and_then(Value::as_str) {
        Some("text") => format!(
            "Example text changed: \"{}\" -> \"{}\" (slot/default content; screens pass their own text)",
            js::norm(from),
            js::norm(to)
        ),
        Some("fill") => format!(
            "{} changed: {} -> {}{}",
            if kind(c) == Some("text") {
                "Text colour"
            } else {
                "Fill"
            },
            fmt_color(from),
            fmt_color(to),
            if js::truthy(to) && !has_token(to) {
                " (no colour token has this value: use a token or propose one)"
            } else {
                ""
            }
        ),
        Some("stroke") => {
            if !js::truthy(from) {
                return format!(
                    "Border added: {}px {}{}",
                    sub(to, "width"),
                    fmt_color(to),
                    no_colour_token(to)
                );
            }
            if !js::truthy(to) {
                return format!(
                    "Border removed (was {}px {})",
                    sub(from, "width"),
                    fmt_color(from)
                );
            }
            fn width(v: Option<&Value>) -> Option<&Value> {
                v.and_then(|v| v.get("width"))
            }
            let width_token = js::truthy_value(to.and_then(|t| t.get("widthToken")));
            let width_note = match width_token {
                Some(token) if !js::strict_eq(width(to), width(from)) => {
                    format!(" [width {}]", js::to_string(Some(token)))
                }
                _ => String::new(),
            };
            format!(
                "Border changed: {}px {} -> {}px {}{}{width_note}",
                sub(from, "width"),
                fmt_color(from),
                sub(to, "width"),
                fmt_color(to),
                no_colour_token(to)
            )
        }
        Some("radius") => {
            let corners = |v: Option<&Value>| {
                list(v.and_then(|v| v.get("radius")))
                    .iter()
                    .map(|x| fmt_px(Some(x)))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let from_token = js::truthy_value(from.and_then(|f| f.get("token")))
                .map_or(String::new(), |t| format!(" ({})", js::to_string(Some(t))));
            let now = list(to.and_then(|t| t.get("radius")));
            let uniform = now.iter().all(|v| js::strict_eq(Some(v), now.first()));
            format!(
                "Corner radius changed: {}{from_token} -> {}{}",
                corners(from),
                corners(to),
                if uniform {
                    token_or(to.and_then(|t| t.get("token")), "radius")
                } else {
                    String::new()
                }
            )
        }
        Some("size") => {
            let at = |v: Option<&Value>, i: usize| rounded(v.and_then(|v| v.get(i)));
            let token = |k: &str| js::truthy_value(c.get("tokens").and_then(|t| t.get(k)));
            let tokens: Vec<String> = [("w", "width"), ("h", "height")]
                .iter()
                .filter_map(|(k, word)| {
                    token(k).map(|t| format!("{word} {}", js::to_string(Some(t))))
                })
                .collect();
            format!(
                "Size changed: {}x{} -> {}x{} px{}",
                at(from, 0),
                at(from, 1),
                at(to, 0),
                at(to, 1),
                if tokens.is_empty() {
                    String::new()
                } else {
                    format!(" ({})", tokens.join(", "))
                }
            )
        }
        Some("spacing") => {
            let sides: Vec<String> = c
                .get("sides")
                .and_then(Value::as_object)
                .map(js::entries)
                .unwrap_or_default()
                .into_iter()
                .map(|(side, v)| {
                    format!(
                        "{side} {} -> {}{}",
                        fmt_px(v.get("from")),
                        fmt_px(v.get("to")),
                        token_or(v.get("token"), "space")
                    )
                })
                .collect();
            format!(
                "Spacing (padding) changed: {}{}",
                sides.join("; "),
                if js::truthy(c.get("border")) {
                    format!(" (insets include the {}px border)", s("border"))
                } else {
                    String::new()
                }
            )
        }
        Some("rename") => {
            if js::truthy(c.get("isRoot")) {
                format!(
                    "Component renamed \"{}\" -> \"{}\" (the master's name/variant ids stay; rename only if intended)",
                    s("from"),
                    s("to")
                )
            } else {
                format!("Layer renamed \"{}\" -> \"{}\"", s("from"), s("to"))
            }
        }
        Some("removed") => removed_line(c),
        Some("added") => format!(
            "Added in Penpot inside \"{}\": {} \"{}\" at {}{}",
            s("name"),
            s("kind"),
            s("layer"),
            rounded_list(c.get("bbox")),
            added_text(c)
        ),
        Some("component-removed") => {
            "Main component deleted in Penpot (remove the variant only if intended: screens may use it)"
                .to_string()
        }
        _ => describe_change(c),
    }
}

#[cfg(test)]
#[path = "diff_tests.rs"]
mod tests;
