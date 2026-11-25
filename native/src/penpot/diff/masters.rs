//! The component masters of the "dfw components" page (`diffComponents`) and the dimension
//! tokens a changed size or radius is matched to (`dimensionTokenIndex`).

use super::*;
use crate::core::js::regex::fixed_regex;

static RADIUS_NAME: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"(^|\.)radius(\.|$)"));
static BORDER_NAME: LazyLock<Regex> =
    LazyLock::new(|| fixed_regex(r"(border|stroke|focus)[.-]?width"));
static SPACE_NAME: LazyLock<Regex> =
    LazyLock::new(|| fixed_regex(r"(^|\.)(space|spacing|gap|inset|padding)(\.|$)"));
static SIZE_NAME: LazyLock<Regex> =
    LazyLock::new(|| fixed_regex(r"(^|\.)(size|sizing|width|height)(\.|$)"));
static MAIN_PREFIX: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^main:[^#]*#"));

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
        let unit = value
            .and_then(|v| v.get(wire::dtcg::UNIT))
            .and_then(Value::as_str);
        let amount = value.and_then(|v| v.get(wire::dtcg::VALUE));
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

pub(super) fn components_of(baseline: &Value) -> HashMap<String, &Value> {
    list(baseline.get(wire::COMPONENTS))
        .iter()
        .map(|c| (js::stringify(c.get(wire::KEY)), c))
        .collect()
}

pub(super) fn comp_info(c: &Value) -> Value {
    js::object(vec![
        (wire::KEY, c.get(wire::KEY).cloned()),
        (wire::NAME, c.get(wire::COMPONENT).cloned()),
        (wire::VARIANT, Some(js::or_null(c.get(wire::VARIANT)))),
        (wire::SIZE, Some(js::or_null(c.get(wire::SIZE)))),
        (wire::TITLE, c.get(wire::NAME).cloned()),
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
        Some(min(wire::Y) - n(parent, wire::Y)),
        has_boxes.then(|| n(parent, wire::X) + n(parent, wire::W) - far(wire::X, wire::W)),
        has_boxes.then(|| n(parent, wire::Y) + n(parent, wire::H) - far(wire::Y, wire::H)),
        Some(min(wire::X) - n(parent, wire::X)),
    ]
}

/// `[a, b]` of two raw values: a missing one is `null`.
fn pair(a: Option<&Value>, b: Option<&Value>) -> Value {
    json!([a, b])
}

/// Edits on the "dfw components" page: each Penpot main component (keyed by the master,
/// variant and size stored in its plugin data) compared with the master as exported.
pub(super) fn diff_components(
    baseline: &Value,
    changes: &Value,
    dims: &[Dim],
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>, DfwError> {
    let comps = components_of(baseline);
    let baseline_id = js::to_string(baseline.get(wire::ID));
    let mut out = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    walkable(changes.get(wire::COMPONENTS), Some(wire::KEY), false)?;
    walkable(changes.get(wire::MISSING_COMPONENTS), None, false)?;
    for cc in list(changes.get(wire::COMPONENTS)) {
        let Some(c) = comps.get(&js::stringify(cc.get(wire::KEY))).copied() else {
            let name = js::truthy_value(cc.get(wire::NAME)).or(cc.get(wire::KEY));
            warnings.push(format!(
                "Component \"{}\" is not in export {baseline_id}: skipped.",
                js::to_string(name)
            ));
            continue;
        };
        seen.push(js::stringify(c.get(wire::KEY)));
        walkable(cc.get(wire::LAYERS), Some(wire::ROOT), false)?;
        walkable(cc.get(wire::ADDED), Some(wire::X), false)?;
        diff_component(c, cc, baseline, dims, &mut out, warnings);
    }
    for key in list(changes.get(wire::MISSING_COMPONENTS)) {
        let key = js::stringify(Some(key));
        let Some(c) = comps.get(&key).filter(|_| !seen.contains(&key)) else {
            continue;
        };
        let root = c.get(wire::ROOT);
        let mut pairs = nowhere();
        pairs.extend([
            (field::COMPONENT, Some(comp_info(c))),
            (wire::PATH, root.and_then(|r| r.get(wire::PATH)).cloned()),
            (wire::NAME, c.get(wire::NAME).cloned()),
            (wire::KIND, root.and_then(|r| r.get(wire::KIND)).cloned()),
            (wire::IS_ROOT, Some(Value::Bool(true))),
            (
                wire::TAG,
                Some(js::or_null(root.and_then(|r| r.get(wire::TAG)))),
            ),
        ]);
        out.push(change(change_type::COMPONENT_REMOVED, vec![pairs]));
    }
    Ok(out)
}

/// The changes of one main component.
fn diff_component(
    c: &Value,
    cc: &Value,
    baseline: &Value,
    dims: &[Dim],
    out: &mut Vec<Value>,
    warnings: &mut Vec<String>,
) {
    let null = Value::Null;
    let root = c.get(wire::ROOT).unwrap_or(&null);
    let roots = json!([root]);
    let flat = Layers::of(Some(&roots));
    let root_path = root.get(wire::PATH);
    let (sent, added) = sent_layers(c, cc, root_path, &flat, warnings);
    let master = Master {
        c,
        root_path,
        sent,
        baseline,
        dims,
    };
    master.rename_master(cc, root, out);
    for l in &flat.order {
        if let Some(n) = master.sent_layer(l.get(wire::PATH)) {
            master.layer_changes(l, n, out);
        }
    }
    master.removed_layers(&flat, out);
    master.added_layers(root, &flat, &added, out);
}

/// The layers the plugin sent for a master, by baseline path (the last one sent wins), and
/// the layers to treat as added: those sent as added, then those whose path the export does
/// not know.
fn sent_layers(
    c: &Value,
    cc: &Value,
    root_path: Option<&Value>,
    flat: &Layers,
    warnings: &mut Vec<String>,
) -> (Vec<(String, Value)>, Vec<Value>) {
    let mut sent: Vec<(String, Value)> = Vec::new();
    let mut added: Vec<Value> = list(js::truthy_value(cc.get(wire::ADDED))).to_vec();
    for n in list(cc.get(wire::LAYERS)) {
        let path: Value = if js::truthy(n.get(wire::ROOT)) {
            root_path.cloned().unwrap_or(Value::Null)
        } else {
            let raw =
                js::present(n.get(wire::PATH)).map_or(String::new(), |p| js::to_string(Some(p)));
            MAIN_PREFIX.replace(&raw, "").into_owned().into()
        };
        // The baseline's own layers are borrowed from `roots`, not from `c`: look them up by key.
        if flat.has(Some(&path)) {
            let mut layer = js::spread(Some(n));
            layer.insert(wire::PATH.into(), path.clone());
            let key = js::stringify(Some(&path));
            match sent.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = Value::Object(layer),
                None => sent.push((key, Value::Object(layer))),
            }
        } else {
            warnings.push(format!(
                "{}: layer path {} unknown in the export; treated as added.",
                js::to_string(c.get(wire::NAME)),
                js::to_string(n.get(wire::PATH))
            ));
            added.push(n.clone());
        }
    }
    (sent, added)
}

/// One main component as exported, with the layers the plugin sent for it.
struct Master<'a> {
    c: &'a Value,
    root_path: Option<&'a Value>,
    sent: Vec<(String, Value)>,
    baseline: &'a Value,
    dims: &'a [Dim],
}

impl Master<'_> {
    fn sent_layer(&self, path: Option<&Value>) -> Option<&Value> {
        let key = js::stringify(path);
        self.sent.iter().find(|(k, _)| *k == key).map(|(_, n)| n)
    }

    fn is_root(&self, l: &Value) -> bool {
        js::strict_eq(l.get(wire::PATH), self.root_path)
    }

    fn location(&self) -> Pairs {
        let mut pairs = nowhere();
        pairs.push((field::COMPONENT, Some(comp_info(self.c))));
        pairs
    }

    fn label(&self, l: &Value) -> Option<Value> {
        if self.is_root(l) {
            self.c.get(wire::NAME).cloned()
        } else if is_text(l) {
            let shown = js::slice16(&js::norm(l.get(wire::CHARACTERS)), 0, 40);
            Some(format!("text \"{shown}\"").into())
        } else {
            l.get(wire::NAME).cloned()
        }
    }

    fn target(&self, l: &Value) -> Pairs {
        vec![
            (wire::PATH, l.get(wire::PATH).cloned()),
            (wire::NAME, self.label(l)),
            (wire::KIND, l.get(wire::KIND).cloned()),
            (wire::TAG, Some(js::or_null(l.get(wire::TAG)))),
            (wire::IS_ROOT, Some(self.is_root(l).into())),
            (wire::BBOX, Some(bbox(l))),
        ]
    }

    fn push(&self, out: &mut Vec<Value>, ty: &str, l: &Value, extra: Pairs) {
        out.push(change(ty, vec![self.location(), self.target(l), extra]));
    }

    /// The main component renamed.
    fn rename_master(&self, cc: &Value, root: &Value, out: &mut Vec<Value>) {
        if let Some(name) = js::truthy_value(cc.get(wire::NAME)) {
            if js::norm(Some(name)) != js::norm(self.c.get(wire::NAME)) {
                self.push(
                    out,
                    change_type::RENAME,
                    root,
                    vec![
                        (field::FROM, self.c.get(wire::NAME).cloned()),
                        (field::TO, Some(name.clone())),
                    ],
                );
            }
        }
    }

    /// The changes of one layer `l` the plugin sent back as `n`, in the order they are listed.
    fn layer_changes(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        self.text_change(l, n, out);
        self.fill_change(l, n, out);
        self.stroke_change(l, n, out);
        self.radius_change(l, n, out);
        self.size_change(l, n, out);
        self.spacing_change(l, n, out);
        if let Some(name) = js::truthy_value(n.get(wire::NAME)) {
            if !self.is_root(l) && !js::strict_eq(Some(name), l.get(wire::NAME)) {
                self.push(
                    out,
                    change_type::RENAME,
                    l,
                    vec![
                        (field::FROM, l.get(wire::NAME).cloned()),
                        (field::TO, Some(name.clone())),
                    ],
                );
            }
        }
    }

    fn text_change(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        if is_text(l)
            && n.get(wire::CHARACTERS).is_some()
            && js::norm(n.get(wire::CHARACTERS)) != js::norm(l.get(wire::CHARACTERS))
        {
            self.push(
                out,
                change_type::TEXT,
                l,
                vec![
                    (field::FROM, l.get(wire::CHARACTERS).cloned()),
                    (field::TO, n.get(wire::CHARACTERS).cloned()),
                ],
            );
        }
    }

    /// Fill (text colour for text layers): changed, added or removed.
    fn fill_change(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        let fill = l.get(wire::FILLS).and_then(|f| f.get(0));
        let before = js::truthy_value(if is_text(l) {
            l.get(wire::TEXT_STYLE).and_then(|t| t.get(wire::COLOR))
        } else {
            fill
        });
        let after = first_of(n.get(wire::FILLS));
        let after = after.as_ref();
        let before_color =
            before.and_then(|b| js::truthy_value(b.get(wire::HEX)).or(b.get(wire::COLOR)));
        let fill_changed = match (after, before) {
            (Some(_), None) => !is_text(l),
            (Some(after), Some(before)) => paint_differs(after, before_color, before),
            (None, _) => false,
        };
        if let (Some(after), true) = (after, fill_changed) {
            let from = before.map_or(Value::Null, |before| {
                let token = if is_text(l) {
                    l.get(wire::TEXT_STYLE)
                        .and_then(|t| t.get(wire::COLOR_TOKEN))
                } else {
                    before.get(field::TOKEN)
                };
                js::object(vec![
                    (wire::COLOR, before_color.cloned()),
                    (wire::OPACITY, Some(or_one(before.get(wire::OPACITY)))),
                    (field::TOKEN, token.cloned()),
                ])
            });
            self.push(
                out,
                change_type::FILL,
                l,
                vec![
                    (field::FROM, Some(from)),
                    (
                        field::TO,
                        Some(json!({
                            (wire::COLOR): lower(after.get(wire::COLOR)),
                            (wire::OPACITY): or_one(after.get(wire::OPACITY)),
                            (field::TOKEN): color_token(self.baseline, after),
                        })),
                    ),
                ],
            );
        } else if let (None, Some(before), false, true) =
            (after, before, is_text(l), n.get(wire::FILLS).is_some())
        {
            self.push(
                out,
                change_type::FILL,
                l,
                vec![
                    (
                        field::FROM,
                        Some(js::object(vec![
                            (wire::COLOR, before.get(wire::COLOR).cloned()),
                            (wire::OPACITY, Some(or_one(before.get(wire::OPACITY)))),
                            (field::TOKEN, before.get(field::TOKEN).cloned()),
                        ])),
                    ),
                    (field::TO, Some(Value::Null)),
                ],
            );
        }
    }

    /// Stroke (border): colour, width, added or removed.
    fn stroke_change(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        let sb = stroke_before(l);
        let sa = first_of(n.get(wire::STROKES));
        let sa = sa.as_ref();
        if !(n.get(wire::STROKES).is_some() && (sb.is_some() || sa.is_some())) {
            return;
        }
        let width = |s: &Value| js::to_number(Some(&or_one(s.get(wire::WIDTH))));
        let changed = match (sb, sa) {
            (Some(sb), Some(sa)) => {
                paint_differs(sa, sb.get(wire::COLOR), sb)
                    || (width(sa) - js::to_number(sb.get(wire::WIDTH))).abs() > 0.25
            }
            _ => true,
        };
        if !changed {
            return;
        }
        let number = |v: Option<&Value>| v.and_then(Value::as_f64);
        let from = sb.map_or(Value::Null, |sb| {
            js::object(vec![
                (wire::COLOR, sb.get(wire::COLOR).cloned()),
                (wire::OPACITY, Some(or_one(sb.get(wire::OPACITY)))),
                (wire::WIDTH, sb.get(wire::WIDTH).cloned()),
                (field::TOKEN, Some(js::or_null(sb.get(field::TOKEN)))),
                (
                    wire::WIDTH_TOKEN,
                    Some(find_dim(self.dims, "border", number(sb.get(wire::WIDTH)))),
                ),
            ])
        });
        let to = sa.map_or(Value::Null, |sa| {
            let w = or_one(sa.get(wire::WIDTH));
            json!({
                (wire::COLOR): lower(sa.get(wire::COLOR)),
                (wire::OPACITY): or_one(sa.get(wire::OPACITY)),
                (wire::WIDTH): w,
                (field::TOKEN): color_token(self.baseline, sa),
                (wire::WIDTH_TOKEN): find_dim(self.dims, "border", w.as_f64()),
            })
        });
        self.push(
            out,
            change_type::STROKE,
            l,
            vec![(field::FROM, Some(from)), (field::TO, Some(to))],
        );
    }

    /// Corner radius (not for text).
    fn radius_change(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        let (Some(radius), false) = (n.get(wire::RADIUS).and_then(Value::as_array), is_text(l))
        else {
            return;
        };
        let zeros = json!([0, 0, 0, 0]);
        let rb = js::truthy_value(l.get(wire::RADIUS)).unwrap_or(&zeros);
        let or_zero =
            |v: Option<&Value>| js::truthy_value(v).map_or(0.0, |v| js::to_number(Some(v)));
        let differs = radius
            .iter()
            .enumerate()
            .any(|(i, v)| (or_zero(Some(v)) - or_zero(rb.get(i))).abs() > 0.5);
        if !differs {
            return;
        }
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
            find_dim(self.dims, "radius", first_number)
        } else {
            Value::Null
        };
        self.push(
            out,
            change_type::RADIUS,
            l,
            vec![
                (
                    field::FROM,
                    Some(json!({ (wire::RADIUS): rb, (field::TOKEN): js::or_null(l.get(wire::RADIUS_TOKEN)) })),
                ),
                (
                    field::TO,
                    Some(json!({ (wire::RADIUS): corners, (field::TOKEN): token })),
                ),
            ],
        );
    }

    /// Size (not for text: Penpot's font metrics differ).
    fn size_change(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        let grew = |k: &str| (js::to_number(n.get(k)) - js::to_number(l.get(k))).abs() > TOL;
        if is_text(l) || !(grew(wire::W) || grew(wire::H)) {
            return;
        }
        let token = |k: &str| {
            if grew(k) {
                find_dim(self.dims, "size", n.get(k).and_then(Value::as_f64))
            } else {
                Value::Null
            }
        };
        self.push(
            out,
            change_type::SIZE,
            l,
            vec![
                (field::FROM, Some(pair(l.get(wire::W), l.get(wire::H)))),
                (field::TO, Some(pair(n.get(wire::W), n.get(wire::H)))),
                (
                    wire::SIZE_TOKENS,
                    Some(json!({ (wire::W): token(wire::W), (wire::H): token(wire::H) })),
                ),
            ],
        );
    }

    /// Spacing: the insets of the children inside this frame (padding), compared side by side.
    fn spacing_change(&self, l: &Value, n: &Value, out: &mut Vec<Value>) {
        let kids: Vec<&Value> = list(l.get(wire::CHILDREN))
            .iter()
            .filter(|k| self.sent_layer(k.get(wire::PATH)).is_some())
            .collect();
        if kids.is_empty() {
            return;
        }
        let now: Vec<&Value> = kids
            .iter()
            .filter_map(|k| self.sent_layer(k.get(wire::PATH)))
            .collect();
        let sb = stroke_before(l);
        let (ib, ia) = (insets(l, &kids), insets(n, &now));
        let sw = sb.map_or(json!(0), |sb| {
            sb.get(wire::WIDTH).cloned().unwrap_or(Value::Null)
        });
        let border = js::to_number(Some(&sw));
        let mut sides = Vec::new();
        for (i, side) in [
            wire::side::TOP,
            wire::side::RIGHT,
            wire::side::BOTTOM,
            wire::side::LEFT,
        ]
        .into_iter()
        .enumerate()
        {
            let (Some(from), Some(to)) = (ib[i], ia[i]) else {
                continue;
            };
            if (to - from).abs() > TOL {
                let token = match find_dim(self.dims, "space", Some(to - border)) {
                    Value::Null => find_dim(self.dims, "space", Some(to)),
                    token => token,
                };
                sides.push((
                    side,
                    Some(json!({ (field::FROM): js::num(from), (field::TO): js::num(to), (field::TOKEN): token })),
                ));
            }
        }
        if !sides.is_empty() {
            // The stroke of a layer that has one always carries a width; without it the
            // Node object held `undefined` and JSON dropped the key.
            let border = sb.map_or(Some(json!(0)), |sb| sb.get(wire::WIDTH).cloned());
            self.push(
                out,
                change_type::SPACING,
                l,
                vec![
                    (field::SIDES, Some(js::object(sides))),
                    (wire::BORDER, border),
                ],
            );
        }
    }

    /// Removed: topmost layers of the master the plugin did not send.
    fn removed_layers(&self, flat: &Layers, out: &mut Vec<Value>) {
        for l in &flat.order {
            if self.sent_layer(l.get(wire::PATH)).is_some() {
                continue;
            }
            let parent = l
                .get(wire::PATH)
                .and_then(Value::as_str)
                .and_then(parent_path)
                .map(Value::from);
            if let Some(parent) = &parent {
                if self.sent_layer(Some(parent)).is_none() && flat.has(Some(parent)) {
                    continue;
                }
            }
            self.push(
                out,
                change_type::REMOVED,
                l,
                vec![(wire::TEXT, l.get(wire::CHARACTERS).cloned())],
            );
        }
    }

    /// Added: each new layer, on the smallest non-text layer under its centre.
    fn added_layers(&self, root: &Value, flat: &Layers, added: &[Value], out: &mut Vec<Value>) {
        for a in added {
            let (cx, cy) = centre(a);
            let mut host = root;
            for l in &flat.order {
                if !is_text(l) && contains(l, cx, cy) && area(l) <= area(host) {
                    host = l;
                }
            }
            self.push(
                out,
                change_type::ADDED,
                host,
                vec![
                    (
                        wire::KIND,
                        js::truthy_value(a.get(wire::KIND))
                            .or(a.get(field::TYPE))
                            .cloned(),
                    ),
                    (wire::LAYER, a.get(wire::NAME).cloned()),
                    (wire::BBOX, Some(bbox(a))),
                    (wire::CHARACTERS, a.get(wire::CHARACTERS).cloned()),
                ],
            );
        }
    }
}

/// The first stroke of a layer as exported, when it has one.
fn stroke_before(l: &Value) -> Option<&Value> {
    js::truthy_value(l.get(wire::STROKES).and_then(|s| s.get(0)))
}
