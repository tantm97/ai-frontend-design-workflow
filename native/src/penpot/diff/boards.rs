//! The screen boards (`diffChanges`): texts, boxes, fills, names, added and removed layers
//! and comment threads of every board, compared with the export the boards were built from.

use super::*;

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

pub(super) fn nowhere() -> Pairs {
    ["board", "screen", "variant", "breakpoint", "file"]
        .iter()
        .map(|k| (*k, Some(Value::Null)))
        .collect()
}

pub(super) fn change(ty: &str, parts: Vec<Pairs>) -> Value {
    let mut pairs: Pairs = vec![("type", Some(ty.into()))];
    for part in parts {
        pairs.extend(part);
    }
    js::object(pairs)
}

/// `diffChanges`: compare what the plugin sent back with the export it was built from.
/// A layer whose path the export does not know is moved to the board's `added` list in
/// `changes` (the Node code did the same to the object it was given).
// Long: boards, deleted boards, masters and comment threads, in the order the change list
// reports them (the order decides how changes group into comments; the golden tests hold it).
#[allow(clippy::too_many_lines)]
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
// added layers, in the order the change list reports them.
#[allow(clippy::too_many_lines)]
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
