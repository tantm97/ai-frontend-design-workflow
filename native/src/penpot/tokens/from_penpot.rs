//! Penpot string -> dfw value: colours, dimensions, weights, shadows and typography of a
//! Penpot token as a dfw token (`tokenFromPenpot`).

use super::*;

const NAMED_WEIGHTS: [(&str, u32); 17] = [
    ("thin", 100),
    ("hairline", 100),
    ("extralight", 200),
    ("ultralight", 200),
    ("light", 300),
    ("regular", 400),
    ("normal", 400),
    ("book", 400),
    ("medium", 500),
    ("semibold", 600),
    ("demibold", 600),
    ("bold", 700),
    ("extrabold", 800),
    ("ultrabold", 800),
    ("black", 900),
    ("heavy", 900),
    ("extrablack", 950),
];

fn srgb(r: f64, g: f64, b: f64, a: f64) -> Value {
    let components: Vec<Value> = [r, g, b]
        .iter()
        .map(|x| js::num(round4(x / 255.0)))
        .collect();
    let hex = format!(
        "#{}",
        [r, g, b]
            .iter()
            .map(|x| hex2(js::round(*x)))
            .collect::<String>()
    );
    if a == 1.0 {
        json!({ "colorSpace": "srgb", "components": components, "hex": hex })
    } else {
        json!({ "colorSpace": "srgb", "components": components, "alpha": js::num(round4(a)), "hex": hex })
    }
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> [f64; 3] {
    let (s, l) = (s / 100.0, l / 100.0);
    let k = |n: f64| (n + h / 30.0) % 12.0;
    let f = |n: f64| {
        l - s * js::min(l, 1.0 - l) * js::max(-1.0, js::min(k(n) - 3.0, js::min(9.0 - k(n), 1.0)))
    };
    [f(0.0) * 255.0, f(8.0) * 255.0, f(4.0) * 255.0]
}

/// The alpha of `rgb()`/`hsl()`: a number or a percentage; 1 when it is not given.
fn alpha_of(text: Option<&str>) -> f64 {
    match text {
        None => 1.0,
        Some(t) if t.ends_with('%') => js::parse_float(t) / 100.0,
        Some(t) => js::parse_float(t),
    }
}

/// `parseColor`: a Penpot colour string as a DTCG colour object, or `None` when it is not a
/// plain colour.
pub fn parse_color(text: &str) -> Option<Value> {
    let t = js::trim(text);
    if let Some(m) = HEX_COLOR.captures(t) {
        let mut h = m[1].to_string();
        if h.len() <= 4 {
            h = h.chars().flat_map(|c| [c, c]).collect();
        }
        let n = |i: usize| parse_int16(&h[i..i + 2]);
        let alpha = if h.len() > 6 { n(6) / 255.0 } else { 1.0 };
        return Some(srgb(n(0), n(2), n(4), alpha));
    }
    let number = |m: &regex::Captures, i: usize| js::str_to_number(&m[i]);
    if let Some(m) = RGB_COLOR.captures(t) {
        let a = alpha_of(m.get(4).map(|g| g.as_str()));
        return Some(srgb(number(&m, 1), number(&m, 2), number(&m, 3), a));
    }
    if let Some(m) = HSL_COLOR.captures(t) {
        let a = alpha_of(m.get(4).map(|g| g.as_str()));
        let [r, g, b] = hsl_to_rgb(number(&m, 1), number(&m, 2), number(&m, 3));
        return Some(srgb(js::round(r), js::round(g), js::round(b), a));
    }
    None
}

/// `parseDimension`: `"16"`, `"16px"`, `"-2.5"`, `"1.5rem"` as `{ value, unit }` (no unit is
/// px); `None` when it is an expression.
pub fn parse_dimension(value: Option<&Value>) -> Option<Value> {
    if let Some(n @ Value::Number(_)) = value {
        return Some(json!({ "value": n, "unit": "px" }));
    }
    let text = js::to_string(value);
    let m = DIMENSION.captures(&text)?;
    let unit = m
        .get(2)
        .map_or("px".to_string(), |u| u.as_str().to_lowercase());
    Some(json!({ "value": js::num(js::str_to_number(&m[1])), "unit": unit }))
}

fn parse_number(value: Option<&Value>) -> Option<Value> {
    if let Some(n @ Value::Number(_)) = value {
        return Some(n.clone());
    }
    let text = js::to_string(value);
    let m = NUMBER.captures(&text)?;
    let n = js::str_to_number(&m[1]);
    Some(js::num(if m.get(2).is_some() {
        round4(n / 100.0)
    } else {
        n
    }))
}

fn parse_weight(value: Option<&Value>, warn: Note) -> Option<Value> {
    if let Some(n @ Value::Number(_)) = value {
        return Some(n.clone());
    }
    let text = js::to_string(value);
    let t = js::trim(&text);
    if !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit()) {
        return Some(js::num(js::str_to_number(t)));
    }
    let key: String = t
        .to_lowercase()
        .chars()
        .filter(|c| !(crate::core::js::is_space(*c) || *c == '_' || *c == '-'))
        .collect();
    let italic = key.ends_with("italic");
    let base = key.strip_suffix("italic").unwrap_or(&key);
    let weight = NAMED_WEIGHTS
        .iter()
        .find(|(name, _)| *name == base)
        .map(|(_, w)| *w)
        .or((italic && key == "italic").then_some(400))?;
    if italic {
        warn(
            "italic is not part of a dfw fontWeight token: set font-style in the master"
                .to_string(),
        );
    }
    Some(json!(weight))
}

fn split_families(value: &Value) -> Value {
    let items: Vec<String> = match value {
        Value::Array(items) => items.iter().map(|x| js::to_string(Some(x))).collect(),
        other => js::to_string(Some(other))
            .split(',')
            .map(str::to_string)
            .collect(),
    };
    let quote = |c: char| c == '"' || c == '\'';
    Value::Array(
        items
            .iter()
            .map(|x| {
                let t = js::trim(x);
                let t = t.strip_prefix(quote).unwrap_or(t);
                t.strip_suffix(quote).unwrap_or(t)
            })
            .filter(|x| !x.is_empty())
            .map(Value::from)
            .collect(),
    )
}

const PENPOT_DIMENSION_TYPES: [&str; 9] = [
    "dimension",
    "spacing",
    "sizing",
    "borderRadius",
    "borderWidth",
    "strokeWidth",
    "fontSizes",
    "fontSize",
    "letterSpacing",
];

/// The dfw type of a plain alias, from the Penpot type.
fn alias_type(ty: Option<&str>) -> Option<&'static str> {
    Some(match ty? {
        "color" => "color",
        "fontFamilies" | "fontFamily" => "fontFamily",
        "fontWeights" | "fontWeight" => "fontWeight",
        "number" | "opacity" | "rotation" => "number",
        "shadow" | "boxShadow" => "shadow",
        "typography" => "typography",
        "textCase" | "textDecoration" => "string",
        other if PENPOT_DIMENSION_TYPES.contains(&other) => "dimension",
        _ => return None,
    })
}

/// `tokenFromPenpot`: one Penpot token as a dfw token. `prefer` is the current dfw type of the
/// name (keeps a duration a duration); `warn` collects notes. An error is a value the Node code
/// could not read at all (a `null` shadow).
pub fn token_from_penpot(
    name: &str,
    token: &Token,
    prefer: Option<&Value>,
    warn: Note,
) -> Result<Converted, DfwError> {
    let value = &token.value;
    let description = token.description.as_ref();
    let ty = token.type_name();
    // A number token named like a duration is a dfw duration (Penpot has no duration type).
    let duration = json!("duration");
    let mut prefer = js::truthy_value(prefer);
    if prefer.is_none() && ty == Some("number") && DURATION_NAME.is_match(name) {
        prefer = Some(&duration);
    }
    let unsupported = || {
        Converted::Unsupported(format!(
            "Penpot type \"{}\" has no dfw equivalent",
            js::to_string(token.ty.as_ref())
        ))
    };
    let raw = |t: &str, why: &str, warn: Note| {
        if matches!(t, "dimension" | "number" | "duration") && is_expression(value) {
            let text = js::to_string(Some(value));
            warn(format!(
                "math expression kept as calc({text}) (check that the CSS is what you expect)"
            ));
            return res(t, format!("calc({text})"), description);
        }
        let shown = match value {
            Value::String(s) => s.clone(),
            other => to_json_compact(other),
        };
        warn(format!("{why}; kept as the raw string \"{shown}\""));
        res(t, value.clone(), description)
    };
    if let Value::String(s) = value {
        let trimmed = js::trim(s);
        if PLAIN_ALIAS.is_match(trimmed) {
            // A plain alias: keep it; the dfw type follows the current token or the Penpot type.
            let t = match prefer {
                Some(p) => p.clone(),
                None => match alias_type(ty) {
                    Some(t) => t.into(),
                    None => return Ok(unsupported()),
                },
            };
            return Ok(res(t, trimmed, description));
        }
    }
    let wants_duration = prefer.and_then(Value::as_str) == Some("duration");
    Ok(match ty {
        Some("color") => match parse_color(&js::to_string(Some(value))) {
            Some(c) => res("color", c, description),
            None => raw(
                "color",
                "not a plain hex/rgb/hsl colour (expression?)",
                warn,
            ),
        },
        Some("fontFamilies" | "fontFamily") => {
            res("fontFamily", split_families(value), description)
        }
        Some("fontWeights" | "fontWeight") => match parse_weight(Some(value), warn) {
            Some(w) => res("fontWeight", w, description),
            None => raw("fontWeight", "not a numeric or named weight", warn),
        },
        Some(t @ ("number" | "opacity" | "rotation")) => match parse_number(Some(value)) {
            None => raw(
                if wants_duration { "duration" } else { "number" },
                "not a plain number",
                warn,
            ),
            Some(n) if wants_duration => {
                res("duration", json!({ "value": n, "unit": "ms" }), description)
            }
            Some(n) => {
                if t != "number" {
                    warn(format!("Penpot {t} token stored as a dfw number"));
                }
                res("number", n, description)
            }
        },
        Some("textCase" | "textDecoration") => {
            res("string", js::to_string(Some(value)), description)
        }
        Some("shadow" | "boxShadow") => match value {
            Value::String(_) => raw("shadow", "shadow given as a string", warn),
            Value::Array(items) => {
                let list: Result<Vec<Value>, DfwError> = items.iter().map(shadow_in).collect();
                res("shadow", Value::Array(list?), description)
            }
            other => res("shadow", shadow_in(other)?, description),
        },
        Some("typography") => match value {
            Value::Object(_) | Value::Array(_) => {
                res("typography", typography_in(value, warn), description)
            }
            _ => raw("typography", "typography given as a string", warn),
        },
        Some(t) if PENPOT_DIMENSION_TYPES.contains(&t) => match parse_dimension(Some(value)) {
            Some(d) => res("dimension", d, description),
            None => raw("dimension", "not a plain number with px/rem/em", warn),
        },
        _ => unsupported(),
    })
}

fn shadow_in(s: &Value) -> Result<Value, DfwError> {
    match s {
        Value::String(_) => return Ok(s.clone()),
        Value::Null => {
            return Err(DfwError::internal(
                "Cannot read properties of null (reading 'color')",
            ))
        }
        _ => {}
    }
    // `x ?? 0`, then the alias as it is, the dimension, or the value untouched.
    let zero = json!(0);
    let d = |x: Option<&Value>| -> Value {
        let x = js::present(x).unwrap_or(&zero);
        if is_alias(Some(x)) {
            x.clone()
        } else {
            parse_dimension(Some(x)).unwrap_or_else(|| x.clone())
        }
    };
    let color = s.get("color");
    let color = if is_alias(color) {
        color.cloned()
    } else {
        parse_color(&js::to_string(color)).or_else(|| color.cloned())
    };
    let either = |a: &str, b: &str| js::present(s.get(a)).or(s.get(b));
    let inset =
        js::truthy(s.get("inset")) || s.get("type").and_then(Value::as_str) == Some("innerShadow");
    Ok(js::object(vec![
        ("color", color),
        ("offsetX", Some(d(either("offsetX", "x")))),
        ("offsetY", Some(d(either("offsetY", "y")))),
        ("blur", Some(d(s.get("blur")))),
        ("spread", Some(d(s.get("spread")))),
        ("inset", inset.then_some(Value::Bool(true))),
    ]))
}

fn typography_in(value: &Value, warn: Note) -> Value {
    let pick = |keys: &[&str]| keys.iter().find_map(|k| value.get(*k));
    let or_self = |parsed: Option<Value>, raw: &Value| parsed.unwrap_or_else(|| raw.clone());
    let mut v = Map::new();
    if let Some(fam) = pick(&["fontFamilies", "fontFamily", "font-family"]) {
        let out = if is_alias(Some(fam)) {
            fam.clone()
        } else {
            split_families(fam)
        };
        v.insert("fontFamily".into(), out);
    }
    if let Some(size) = pick(&["fontSizes", "fontSize", "font-size"]) {
        let out = if is_alias(Some(size)) {
            size.clone()
        } else {
            or_self(parse_dimension(Some(size)), size)
        };
        v.insert("fontSize".into(), out);
    }
    if let Some(w) = pick(&["fontWeights", "fontWeight", "font-weight"]) {
        let out = if is_alias(Some(w)) {
            w.clone()
        } else {
            or_self(parse_weight(Some(w), warn), w)
        };
        v.insert("fontWeight".into(), out);
    }
    if let Some(ls) = pick(&["letterSpacing", "letter-spacing"]) {
        let out = if is_alias(Some(ls)) {
            ls.clone()
        } else {
            or_self(parse_dimension(Some(ls)), ls)
        };
        v.insert("letterSpacing".into(), out);
    }
    if let Some(lh) = pick(&["lineHeights", "lineHeight", "line-height"]) {
        let out = if is_alias(Some(lh)) {
            lh.clone()
        } else {
            or_self(parse_number(Some(lh)), lh)
        };
        v.insert("lineHeight".into(), out);
    }
    if let Some(tc) = pick(&["textCase", "text-case"]) {
        v.insert("textCase".into(), tc.clone());
    }
    if let Some(td) = pick(&["textDecoration", "text-decoration"]) {
        v.insert("textDecoration".into(), td.clone());
    }
    Value::Object(v)
}
