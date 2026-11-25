//! The sentences shown for every change (`describeChange`, `describeComponentChange`).

use super::*;

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
    let opacity = match c.get(wire::OPACITY) {
        Some(o) if js::to_number(Some(o)) < 1.0 => {
            format!(
                " @{}%",
                js::number(js::round(js::to_number(Some(o)) * 100.0))
            )
        }
        _ => String::new(),
    };
    let token = js::truthy_value(c.get(field::TOKEN))
        .map_or(String::new(), |t| format!(" ({})", js::to_string(Some(t))));
    format!("{}{opacity}{token}", js::to_string(c.get(wire::COLOR)))
}

fn has_token(c: Option<&Value>) -> bool {
    js::truthy(c.and_then(|c| c.get(field::TOKEN)))
}

/// `("<text>")` for a removed or added text, cut at `max` characters.
fn quoted(v: Option<&Value>, max: usize) -> String {
    js::slice16(&js::norm(v), 0, max)
}

fn moved_beyond(c: &Value) -> bool {
    let d = |k: &str| js::to_number(c.get(wire::DELTA).and_then(|d| d.get(k))).abs();
    d(wire::X) > TOL || d(wire::Y) > TOL
}

/// `describeChange`: one line for a change on a screen board.
pub fn describe_change(c: &Value) -> String {
    let at = |key: &str, i: usize| rounded(c.get(key).and_then(|v| v.get(i)));
    // A delta is computed: `null` in it is what JSON keeps of NaN.
    let delta = |k: &str| match c.get(wire::DELTA).and_then(|d| d.get(k)) {
        Some(Value::Null) => "NaN".to_string(),
        other => rounded(other),
    };
    let s = |key: &str| js::to_string(c.get(key));
    match c.get(field::TYPE).and_then(Value::as_str) {
        Some(change_type::TEXT) => format!(
            "{} changed: \"{}\" -> \"{}\"",
            if js::truthy(c.get(wire::PLACEHOLDER)) {
                "Placeholder"
            } else {
                "Text"
            },
            js::norm(c.get(field::FROM)),
            js::norm(c.get(field::TO))
        ),
        Some(change_type::MOVE) => format!(
            "Moved by {}, {} px (now x {}, y {})",
            delta(wire::X),
            delta(wire::Y),
            at(field::TO, 0),
            at(field::TO, 1)
        ),
        Some(change_type::RESIZE) => format!(
            "Resized {}x{} -> {}x{} px{}",
            at(field::FROM, 2),
            at(field::FROM, 3),
            at(field::TO, 2),
            at(field::TO, 3),
            if moved_beyond(c) {
                format!(", moved by {}, {}", delta(wire::X), delta(wire::Y))
            } else {
                String::new()
            }
        ),
        Some(change_type::FILL) => format!(
            "Colour changed: {} -> {}{}",
            fmt_color(c.get(field::FROM)),
            fmt_color(c.get(field::TO)),
            if js::truthy(c.get(field::TO)) && !has_token(c.get(field::TO)) {
                " (no token has this value: use a token or propose one)"
            } else {
                ""
            }
        ),
        Some(change_type::RENAME) => format!(
            "Layer renamed \"{}\" -> \"{}\" (data-dfw-id stays; rename only if intended)",
            s(field::FROM),
            s(field::TO)
        ),
        Some(change_type::REMOVED) => removed_line(c),
        Some(change_type::ADDED) => format!(
            "Added in Penpot: {} \"{}\" at {}{}",
            s(wire::KIND),
            s(wire::NAME),
            rounded_list(c.get(wire::BBOX)),
            added_text(c)
        ),
        Some(change_type::BOARD_REMOVED) => "Board deleted in Penpot".to_string(),
        Some(change_type::COMMENT) => list(c.get(wire::MESSAGES))
            .iter()
            .map(|m| {
                format!(
                    "{}: {}",
                    js::to_string(m.get(wire::AUTHOR)),
                    js::to_string(m.get(wire::CONTENT))
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => s(field::TYPE),
    }
}

fn removed_line(c: &Value) -> String {
    let text = if js::truthy(c.get(wire::TEXT)) {
        format!(" (\"{}\")", quoted(c.get(wire::TEXT), 60))
    } else {
        String::new()
    };
    format!(
        "Removed in Penpot: {} \"{}\"{text}",
        js::to_string(c.get(wire::KIND)),
        js::to_string(c.get(wire::NAME))
    )
}

fn added_text(c: &Value) -> String {
    if js::truthy(c.get(wire::CHARACTERS)) {
        format!(" with text \"{}\"", quoted(c.get(wire::CHARACTERS), 80))
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
pub fn describe_component_change(c: &Value) -> String {
    let s = |key: &str| js::to_string(c.get(key));
    let (from, to) = (c.get(field::FROM), c.get(field::TO));
    match c.get(field::TYPE).and_then(Value::as_str) {
        Some(change_type::TEXT) => format!(
            "Example text changed: \"{}\" -> \"{}\" (slot/default content; screens pass their own text)",
            js::norm(from),
            js::norm(to)
        ),
        Some(change_type::FILL) => master_fill_line(c, from, to),
        Some(change_type::STROKE) => master_stroke_line(from, to),
        Some(change_type::RADIUS) => master_radius_line(from, to),
        Some(change_type::SIZE) => master_size_line(c, from, to),
        Some(change_type::SPACING) => master_spacing_line(c),
        Some(change_type::RENAME) => master_rename_line(c),
        Some(change_type::REMOVED) => removed_line(c),
        Some(change_type::ADDED) => format!(
            "Added in Penpot inside \"{}\": {} \"{}\" at {}{}",
            s(wire::NAME),
            s(wire::KIND),
            s(wire::LAYER),
            rounded_list(c.get(wire::BBOX)),
            added_text(c)
        ),
        Some(change_type::COMPONENT_REMOVED) => {
            "Main component deleted in Penpot (remove the variant only if intended: screens may use it)"
                .to_string()
        }
        _ => describe_change(c),
    }
}

fn master_fill_line(c: &Value, from: Option<&Value>, to: Option<&Value>) -> String {
    format!(
        "{} changed: {} -> {}{}",
        if kind(c) == Some(wire::kind::TEXT) {
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
    )
}

fn master_stroke_line(from: Option<&Value>, to: Option<&Value>) -> String {
    fn width(v: Option<&Value>) -> Option<&Value> {
        v.and_then(|v| v.get(wire::WIDTH))
    }
    let sub = |v: Option<&Value>| js::to_string(width(v));
    let no_colour_token = |v: Option<&Value>| {
        if has_token(v) {
            ""
        } else {
            " (no colour token has this value)"
        }
    };
    if !js::truthy(from) {
        return format!(
            "Border added: {}px {}{}",
            sub(to),
            fmt_color(to),
            no_colour_token(to)
        );
    }
    if !js::truthy(to) {
        return format!("Border removed (was {}px {})", sub(from), fmt_color(from));
    }
    let width_token = js::truthy_value(to.and_then(|t| t.get(wire::WIDTH_TOKEN)));
    let width_note = match width_token {
        Some(token) if !js::strict_eq(width(to), width(from)) => {
            format!(" [width {}]", js::to_string(Some(token)))
        }
        _ => String::new(),
    };
    format!(
        "Border changed: {}px {} -> {}px {}{}{width_note}",
        sub(from),
        fmt_color(from),
        sub(to),
        fmt_color(to),
        no_colour_token(to)
    )
}

fn master_radius_line(from: Option<&Value>, to: Option<&Value>) -> String {
    let corners = |v: Option<&Value>| {
        list(v.and_then(|v| v.get(wire::RADIUS)))
            .iter()
            .map(|x| fmt_px(Some(x)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let from_token = js::truthy_value(from.and_then(|f| f.get(field::TOKEN)))
        .map_or(String::new(), |t| format!(" ({})", js::to_string(Some(t))));
    let now = list(to.and_then(|t| t.get(wire::RADIUS)));
    let uniform = now.iter().all(|v| js::strict_eq(Some(v), now.first()));
    format!(
        "Corner radius changed: {}{from_token} -> {}{}",
        corners(from),
        corners(to),
        if uniform {
            token_or(to.and_then(|t| t.get(field::TOKEN)), "radius")
        } else {
            String::new()
        }
    )
}

fn master_size_line(c: &Value, from: Option<&Value>, to: Option<&Value>) -> String {
    let at = |v: Option<&Value>, i: usize| rounded(v.and_then(|v| v.get(i)));
    let token = |k: &str| js::truthy_value(c.get(wire::SIZE_TOKENS).and_then(|t| t.get(k)));
    let tokens: Vec<String> = [(wire::W, "width"), (wire::H, "height")]
        .iter()
        .filter_map(|(k, word)| token(k).map(|t| format!("{word} {}", js::to_string(Some(t)))))
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

fn master_spacing_line(c: &Value) -> String {
    let sides: Vec<String> = c
        .get(field::SIDES)
        .and_then(Value::as_object)
        .map(js::entries)
        .unwrap_or_default()
        .into_iter()
        .map(|(side, v)| {
            format!(
                "{side} {} -> {}{}",
                fmt_px(v.get(field::FROM)),
                fmt_px(v.get(field::TO)),
                token_or(v.get(field::TOKEN), "space")
            )
        })
        .collect();
    format!(
        "Spacing (padding) changed: {}{}",
        sides.join("; "),
        if js::truthy(c.get(wire::BORDER)) {
            format!(
                " (insets include the {}px border)",
                js::to_string(c.get(wire::BORDER))
            )
        } else {
            String::new()
        }
    )
}

fn master_rename_line(c: &Value) -> String {
    let s = |key: &str| js::to_string(c.get(key));
    if js::truthy(c.get(wire::IS_ROOT)) {
        format!(
            "Component renamed \"{}\" -> \"{}\" (the master's name/variant ids stay; rename only if intended)",
            s(field::FROM),
            s(field::TO)
        )
    } else {
        format!(
            "Layer renamed \"{}\" -> \"{}\"",
            s(field::FROM),
            s(field::TO)
        )
    }
}
