//! The component masters of the "dfw components" page (`diffComponents`) and the dimension
//! tokens a changed size or radius is matched to (`dimensionTokenIndex`).

use super::*;

// A constant pattern: it compiles, and every test that reaches it proves so.
#[allow(clippy::expect_used)]
static RADIUS_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|\.)radius(\.|$)").expect("constant pattern"));
// A constant pattern: it compiles, and every test that reaches it proves so.
#[allow(clippy::expect_used)]
static BORDER_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(border|stroke|focus)[.-]?width").expect("constant pattern"));
// A constant pattern: it compiles, and every test that reaches it proves so.
#[allow(clippy::expect_used)]
static SPACE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|\.)(space|spacing|gap|inset|padding)(\.|$)").expect("constant pattern")
});
// A constant pattern: it compiles, and every test that reaches it proves so.
#[allow(clippy::expect_used)]
static SIZE_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|\.)(size|sizing|width|height)(\.|$)").expect("constant pattern")
});
// A constant pattern: it compiles, and every test that reaches it proves so.
#[allow(clippy::expect_used)]
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

pub(super) fn components_of(baseline: &Value) -> HashMap<String, &Value> {
    list(baseline.get("components"))
        .iter()
        .map(|c| (js::stringify(c.get("key")), c))
        .collect()
}

pub(super) fn comp_info(c: &Value) -> Value {
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
pub(super) fn diff_components(
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
#[allow(clippy::too_many_lines)]
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
