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
#[allow(clippy::too_many_lines, reason = "Long: one sentence per change type")]
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
