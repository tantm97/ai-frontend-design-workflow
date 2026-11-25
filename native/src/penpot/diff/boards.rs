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
        (wire::BOARD, b.get(wire::KEY).cloned()),
        (wire::SCREEN, b.get(wire::SCREEN).cloned()),
        (wire::VARIANT, b.get(wire::VARIANT).cloned()),
        (wire::BREAKPOINT, b.get(wire::BREAKPOINT).cloned()),
        (wire::FILE, b.get(wire::FILE).cloned()),
    ]
}

pub(super) fn nowhere() -> Pairs {
    [
        wire::BOARD,
        wire::SCREEN,
        wire::VARIANT,
        wire::BREAKPOINT,
        wire::FILE,
    ]
    .iter()
    .map(|k| (*k, Some(Value::Null)))
    .collect()
}

pub(super) fn change(ty: &str, parts: Vec<Pairs>) -> Value {
    let mut pairs: Pairs = vec![(field::TYPE, Some(ty.into()))];
    for part in parts {
        pairs.extend(part);
    }
    js::object(pairs)
}

/// The boards of the export by key.
type Boards<'a> = HashMap<String, &'a Value>;

fn board_index(baseline: &Value) -> Boards<'_> {
    let mut boards: Boards = HashMap::new();
    for p in list(baseline.get(wire::PAGES)) {
        for b in list(p.get(wire::BOARDS)) {
            boards.insert(js::stringify(b.get(wire::KEY)), b);
        }
    }
    boards
}

fn board_of<'a>(boards: &Boards<'a>, key: Option<&Value>) -> Option<&'a Value> {
    boards.get(&js::stringify(key)).copied()
}

/// `diffChanges`: compare what the plugin sent back with the export it was built from.
/// A layer whose path the export does not know is moved to the board's `added` list in
/// `changes` (the Node code did the same to the object it was given).
/// Boards, deleted boards, masters and comment threads come in the order the change list
/// reports them (the order decides how changes group into comments; the golden tests hold it).
pub fn diff_changes(baseline: &Value, changes: &mut Value, dims: &[Dim]) -> Result<Diff, DfwError> {
    if changes.get(wire::SCHEMA).and_then(Value::as_str) != Some(CHANGES_SCHEMA) {
        return Err(DfwError::new(format!(
            "Not a dfw Penpot change file (schema \"{}\", expected {CHANGES_SCHEMA}).",
            js::to_string(changes.get(wire::SCHEMA))
        )));
    }
    let boards = board_index(baseline);
    let mut out = Vec::new();
    let mut warnings = Vec::new();
    walkable(changes.get(wire::BOARDS), Some(wire::KEY), false)?;
    walkable(changes.get(wire::MISSING_BOARDS), None, false)?;
    walkable(changes.get(wire::COMMENTS), Some(wire::BOARD_KEY), false)?;
    for t in list(changes.get(wire::COMMENTS)) {
        walkable(t.get(wire::COMMENTS), Some(wire::CONTENT), true)?;
    }
    diff_sent_boards(&boards, baseline, changes, &mut out, &mut warnings)?;
    for key in list(changes.get(wire::MISSING_BOARDS)) {
        if let Some(b) = board_of(&boards, Some(key)) {
            out.push(change(
                change_type::BOARD_REMOVED,
                vec![where_board(b), vec![(wire::ANCHOR, Some(Value::Null))]],
            ));
        }
    }
    let current = current_boxes(&boards, changes);
    let comps = components_of(baseline);
    out.extend(diff_components(baseline, changes, dims, &mut warnings)?);
    for t in list(changes.get(wire::COMMENTS)) {
        out.push(comment_change(t, &boards, &current, &comps));
    }
    Ok(Diff {
        changes: out,
        warnings,
    })
}

/// The changes of every board the plugin sent. Layers of unknown path join the board's
/// `added` list in `changes`.
fn diff_sent_boards(
    boards: &Boards,
    baseline: &Value,
    changes: &mut Value,
    out: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) -> Result<(), DfwError> {
    let baseline_id = js::to_string(baseline.get(wire::ID));
    let board_count = list(changes.get(wire::BOARDS)).len();
    for bi in 0..board_count {
        let cb = list(changes.get(wire::BOARDS))[bi].clone();
        let Some(b) = board_of(boards, cb.get(wire::KEY)) else {
            let name = js::truthy_value(cb.get(wire::NAME)).or(cb.get(wire::KEY));
            warnings.push(format!(
                "Board \"{}\" is not in export {baseline_id}: skipped.",
                js::to_string(name)
            ));
            continue;
        };
        walkable(cb.get(wire::LAYERS), Some(wire::PATH), false)?;
        walkable(cb.get(wire::ADDED), Some(wire::X), false)?;
        let unknown = diff_board(b, &cb, baseline, out, warnings);
        if unknown.is_empty() {
            continue;
        }
        // `cb.added.push` of a string is not a function.
        walkable(cb.get(wire::ADDED), None, true)?;
        // `(cb.added ||= []).push(n)`: the change file itself carries them from now on.
        if let Some(Value::Object(real)) = changes
            .get_mut(wire::BOARDS)
            .and_then(|boards| boards.get_mut(bi))
        {
            let slot = real.entry(wire::ADDED).or_insert(Value::Null);
            if !js::truthy(&*slot) {
                *slot = json!([]);
            }
            if let Value::Array(added) = slot {
                added.extend(unknown);
            }
        }
    }
    Ok(())
}

/// The layers of every sent board with the boxes the plugin pushed (current), by board key.
/// Comments sit on the board as the designer sees it: they are resolved against these.
fn current_boxes(boards: &Boards, changes: &Value) -> HashMap<String, Vec<Value>> {
    let mut current: HashMap<String, Vec<Value>> = HashMap::new();
    for cb in list(changes.get(wire::BOARDS)) {
        let Some(b) = board_of(boards, cb.get(wire::KEY)) else {
            continue;
        };
        let flat = Layers::of(b.get(wire::LAYERS));
        let mut moved: Vec<(String, Value)> = Vec::new();
        for n in list(cb.get(wire::LAYERS)) {
            let Some(l) = flat.get(n.get(wire::PATH)) else {
                continue;
            };
            let mut now = js::spread(Some(l));
            for k in [wire::X, wire::Y, wire::W, wire::H] {
                match n.get(k) {
                    Some(v) => {
                        now.insert(k.to_string(), v.clone());
                    }
                    None => {
                        now.shift_remove(k);
                    }
                }
            }
            let path = js::stringify(n.get(wire::PATH));
            match moved.iter_mut().find(|(p, _)| *p == path) {
                Some(slot) => slot.1 = Value::Object(now),
                None => moved.push((path, Value::Object(now))),
            }
        }
        current.insert(
            js::stringify(cb.get(wire::KEY)),
            moved.into_iter().map(|(_, l)| l).collect(),
        );
    }
    current
}

/// The change of one comment thread: where it sits (board and layer, or a master) and the
/// designer's messages.
fn comment_change(
    t: &Value,
    boards: &Boards,
    current: &HashMap<String, Vec<Value>>,
    comps: &HashMap<String, &Value>,
) -> Value {
    let b = js::truthy_value(t.get(wire::BOARD_KEY)).and_then(|k| board_of(boards, Some(k)));
    let (x, y) = (js::to_number(t.get(wire::X)), js::to_number(t.get(wire::Y)));
    let host = b.and_then(|b| match current.get(&js::stringify(b.get(wire::KEY))) {
        Some(now) => deepest_at(now.iter(), x, y).cloned(),
        None => deepest_at(Layers::of(b.get(wire::LAYERS)).order.into_iter(), x, y).cloned(),
    });
    let comp = match (b, js::truthy_value(t.get(wire::COMPONENT_KEY))) {
        (None, Some(key)) => comps.get(&js::stringify(Some(key))).copied(),
        _ => None,
    };
    let messages: Vec<Value> = list(t.get(wire::COMMENTS))
        .iter()
        .filter(|c| !is_dfw_reply(c.get(wire::CONTENT)))
        .map(|c| {
            js::object(vec![
                (
                    wire::AUTHOR,
                    Some(
                        js::truthy_value(c.get(wire::USER))
                            .cloned()
                            .unwrap_or_else(|| "designer".into()),
                    ),
                ),
                (wire::CONTENT, c.get(wire::CONTENT).cloned()),
                (wire::AT, Some(js::or_null(c.get(wire::DATE)))),
            ])
        })
        .collect();
    change(
        change_type::COMMENT,
        vec![
            b.map_or_else(nowhere, where_board),
            vec![
                (field::COMPONENT, comp.map(comp_info)),
                (wire::ANCHOR, Some(anchor_of(host.as_ref()))),
                (wire::BBOX, Some(host.as_ref().map_or(Value::Null, bbox))),
                (
                    wire::THREAD,
                    Some(
                        js::present(t.get(wire::SEQ))
                            .cloned()
                            .unwrap_or(Value::Null),
                    ),
                ),
                (
                    wire::RESOLVED,
                    Some(js::truthy(t.get(wire::RESOLVED)).into()),
                ),
                (wire::MESSAGES, Some(Value::Array(messages))),
            ],
        ],
    )
}

/// The changes of one screen board. Returns the layers whose path the export does not know.
/// Per layer: text, box, fill and name, then removed and added layers, in the order the change
/// list reports them.
fn diff_board(
    b: &Value,
    cb: &Value,
    baseline: &Value,
    out: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) -> Vec<Value> {
    let flat = Layers::of(b.get(wire::LAYERS));
    let deltas = layer_deltas(&flat, cb);
    let mut present: Vec<String> = Vec::new();
    let mut unknown = Vec::new();
    for n in list(cb.get(wire::LAYERS)) {
        let Some(l) = flat.get(n.get(wire::PATH)) else {
            warnings.push(format!(
                "{}: layer path {} unknown in the export; treated as added.",
                js::to_string(b.get(wire::NAME)),
                js::to_string(n.get(wire::PATH))
            ));
            unknown.push(n.clone());
            continue;
        };
        present.push(js::stringify(n.get(wire::PATH)));
        text_change(b, l, n, out);
        box_change(b, l, n, &deltas, out);
        fill_change(b, l, n, baseline, out);
        if let Some(name) = js::truthy_value(n.get(wire::NAME)) {
            if !js::strict_eq(Some(name), l.get(wire::NAME)) && js::truthy(l.get(wire::DFW_ID)) {
                out.push(layer_change(
                    change_type::RENAME,
                    b,
                    l,
                    vec![
                        (field::FROM, l.get(wire::NAME).cloned()),
                        (field::TO, Some(name.clone())),
                    ],
                ));
            }
        }
    }
    removed_layers(b, &flat, &present, out);
    let sent_added = js::truthy_value(cb.get(wire::ADDED));
    for a in list(sent_added).iter().chain(&unknown) {
        let (x, y) = centre(a);
        let host = deepest_at(flat.order.iter().copied(), x, y);
        out.push(change(
            change_type::ADDED,
            vec![
                where_board(b),
                vec![
                    (wire::ANCHOR, Some(anchor_of(host))),
                    (wire::NAME, a.get(wire::NAME).cloned()),
                    (
                        wire::KIND,
                        js::truthy_value(a.get(wire::KIND))
                            .or(a.get(field::TYPE))
                            .cloned(),
                    ),
                    (wire::BBOX, Some(bbox(a))),
                    (wire::CHARACTERS, a.get(wire::CHARACTERS).cloned()),
                ],
            ],
        ));
    }
    unknown
}

fn delta(n: &Value, l: &Value, k: &str) -> f64 {
    js::to_number(n.get(k)) - js::to_number(l.get(k))
}

/// How far each sent layer moved (x, y), by path.
fn layer_deltas(flat: &Layers, cb: &Value) -> HashMap<String, (f64, f64)> {
    let mut deltas: HashMap<String, (f64, f64)> = HashMap::new();
    for n in list(cb.get(wire::LAYERS)) {
        if let Some(l) = flat.get(n.get(wire::PATH)) {
            deltas.insert(
                js::stringify(n.get(wire::PATH)),
                (delta(n, l, wire::X), delta(n, l, wire::Y)),
            );
        }
    }
    deltas
}

/// A layer that moved exactly like its parent was not moved itself.
fn own_move(deltas: &HashMap<String, (f64, f64)>, path: Option<&str>, dx: f64, dy: f64) -> bool {
    let parent = path
        .and_then(parent_path)
        .and_then(|p| deltas.get(&js::stringify(Some(&Value::from(p)))));
    !parent.is_some_and(|(px, py)| (px - dx).abs() <= TOL && (py - dy).abs() <= TOL)
}

/// The export layer a change is on.
fn layer_target(l: &Value) -> Pairs {
    vec![
        (wire::ANCHOR, l.get(wire::ANCHOR).cloned()),
        (wire::DFW_ID, l.get(wire::DFW_ID).cloned()),
        (wire::PATH, l.get(wire::PATH).cloned()),
        (wire::NAME, l.get(wire::NAME).cloned()),
        (wire::BBOX, Some(bbox(l))),
        (wire::KIND, l.get(wire::KIND).cloned()),
    ]
}

fn layer_change(ty: &str, b: &Value, l: &Value, extra: Pairs) -> Value {
    change(ty, vec![where_board(b), layer_target(l), extra])
}

fn text_change(b: &Value, l: &Value, n: &Value, out: &mut Vec<Value>) {
    if is_text(l)
        && n.get(wire::CHARACTERS).is_some()
        && js::norm(n.get(wire::CHARACTERS)) != js::norm(l.get(wire::CHARACTERS))
    {
        out.push(layer_change(
            change_type::TEXT,
            b,
            l,
            vec![
                (field::FROM, l.get(wire::CHARACTERS).cloned()),
                (field::TO, n.get(wire::CHARACTERS).cloned()),
                (
                    wire::PLACEHOLDER,
                    Some(js::truthy(l.get(wire::PLACEHOLDER)).into()),
                ),
            ],
        ));
    }
}

/// A layer moved on its own, or resized.
fn box_change(
    b: &Value,
    l: &Value,
    n: &Value,
    deltas: &HashMap<String, (f64, f64)>,
    out: &mut Vec<Value>,
) {
    let (dx, dy) = (delta(n, l, wire::X), delta(n, l, wire::Y));
    let (dw, dh) = (delta(n, l, wire::W), delta(n, l, wire::H));
    let moved = (dx.abs() > TOL || dy.abs() > TOL)
        && own_move(deltas, n.get(wire::PATH).and_then(Value::as_str), dx, dy);
    // Text boxes follow Penpot's font metrics (auto width/height): their size is not a
    // design change.
    let resized = !is_text(l) && (dw.abs() > TOL || dh.abs() > TOL);
    if moved || resized {
        out.push(layer_change(
            if resized {
                change_type::RESIZE
            } else {
                change_type::MOVE
            },
            b,
            l,
            vec![
                (field::FROM, Some(bbox(l))),
                (field::TO, Some(bbox(n))),
                (
                    wire::DELTA,
                    Some(json!({ (wire::X): js::num(dx), (wire::Y): js::num(dy), (wire::W): js::num(dw), (wire::H): js::num(dh) })),
                ),
            ],
        ));
    }
}

/// Fill (text colour for text layers): changed or added.
fn fill_change(b: &Value, l: &Value, n: &Value, baseline: &Value, out: &mut Vec<Value>) {
    let text_color = l.get(wire::TEXT_STYLE).and_then(|t| t.get(wire::COLOR));
    let fill = l.get(wire::FILLS).and_then(|f| f.get(0));
    let before = js::truthy_value(if is_text(l) { text_color } else { fill });
    let after = first_of(n.get(wire::FILLS));
    let after = after.as_ref();
    match (after, before) {
        (Some(after), Some(before)) => {
            let before_color = js::truthy_value(before.get(wire::HEX)).or(before.get(wire::COLOR));
            if paint_differs(after, before_color, before) {
                let token = if is_text(l) {
                    l.get(wire::TEXT_STYLE).and_then(|t| t.get(wire::COLOR_TOKEN))
                } else {
                    fill.and_then(|f| f.get(field::TOKEN))
                };
                out.push(layer_change(
                    change_type::FILL,
                    b,
                    l,
                    vec![
                        (
                            field::FROM,
                            Some(js::object(vec![
                                (wire::COLOR, before_color.cloned()),
                                (wire::OPACITY, Some(or_one(before.get(wire::OPACITY)))),
                                (field::TOKEN, token.cloned()),
                            ])),
                        ),
                        (
                            field::TO,
                            Some(json!({
                                (wire::COLOR): lower(after.get(wire::COLOR)),
                                (wire::OPACITY): or_one(after.get(wire::OPACITY)),
                                (field::TOKEN): color_token(baseline, after),
                            })),
                        ),
                    ],
                ));
            }
        }
        (Some(after), None) if !is_text(l) => out.push(layer_change(
            change_type::FILL,
            b,
            l,
            vec![
                (field::FROM, Some(Value::Null)),
                (
                    field::TO,
                    Some(json!({ (wire::COLOR): lower(after.get(wire::COLOR)), (wire::OPACITY): or_one(after.get(wire::OPACITY)) })),
                ),
            ],
        )),
        _ => {}
    }
}

/// Removed: topmost layers of the export missing from the board.
fn removed_layers(b: &Value, flat: &Layers, present: &[String], out: &mut Vec<Value>) {
    let is_present = |path: &str| present.iter().any(|p| p == path);
    for l in &flat.order {
        if is_present(&js::stringify(l.get(wire::PATH))) {
            continue;
        }
        let parent = l
            .get(wire::PATH)
            .and_then(Value::as_str)
            .and_then(parent_path)
            .map(Value::from);
        if let Some(parent) = &parent {
            if !is_present(&js::stringify(Some(parent))) && flat.has(Some(parent)) {
                continue;
            }
        }
        out.push(layer_change(
            change_type::REMOVED,
            b,
            l,
            vec![(wire::TEXT, l.get(wire::CHARACTERS).cloned())],
        ));
    }
}
