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
        json!({ (key::COLOR_SPACE): color_space::SRGB, (key::COMPONENTS): components, (key::HEX): hex })
    } else {
        json!({ (key::COLOR_SPACE): color_space::SRGB, (key::COMPONENTS): components, (key::ALPHA): js::num(round4(a)), (key::HEX): hex })
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
        return Some(json!({ (key::AMOUNT): n, (key::UNIT): unit::PX }));
    }
    let text = js::to_string(value);
    let m = DIMENSION.captures(&text)?;
    let unit = m
        .get(2)
        .map_or(unit::PX.to_string(), |u| u.as_str().to_lowercase());
    Some(json!({ (key::AMOUNT): js::num(js::str_to_number(&m[1])), (key::UNIT): unit }))
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
    penpot_type::DIMENSION,
    penpot_type::SPACING,
    penpot_type::SIZING,
    penpot_type::BORDER_RADIUS,
    penpot_type::BORDER_WIDTH,
    "strokeWidth",
    penpot_type::FONT_SIZES,
    "fontSize",
    penpot_type::LETTER_SPACING,
];

/// The dfw type of a plain alias, from the Penpot type.
fn alias_type(ty: Option<&str>) -> Option<&'static str> {
    Some(match ty? {
        penpot_type::COLOR => dtcg::COLOR,
        penpot_type::FONT_FAMILIES | dtcg::FONT_FAMILY => dtcg::FONT_FAMILY,
        penpot_type::FONT_WEIGHTS | dtcg::FONT_WEIGHT => dtcg::FONT_WEIGHT,
        penpot_type::NUMBER | "opacity" | "rotation" => dtcg::NUMBER,
        penpot_type::SHADOW | "boxShadow" => dtcg::SHADOW,
        penpot_type::TYPOGRAPHY => dtcg::TYPOGRAPHY,
        penpot_type::TEXT_CASE | penpot_type::TEXT_DECORATION => dtcg::STRING,
        other if PENPOT_DIMENSION_TYPES.contains(&other) => dtcg::DIMENSION,
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
    let duration = json!(dtcg::DURATION);
    let mut prefer = js::truthy_value(prefer);
    if prefer.is_none() && ty == Some(penpot_type::NUMBER) && DURATION_NAME.is_match(name) {
        prefer = Some(&duration);
    }
    let unsupported = || {
        Converted::Unsupported(format!(
            "Penpot type \"{}\" has no dfw equivalent",
            js::to_string(token.ty.as_ref())
        ))
    };
    let raw = |t: &str, why: &str, warn: Note| {
        if matches!(t, dtcg::DIMENSION | dtcg::NUMBER | dtcg::DURATION) && is_expression(value) {
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
    let wants_duration = prefer.and_then(Value::as_str) == Some(dtcg::DURATION);
    Ok(match ty {
        Some(penpot_type::COLOR) => match parse_color(&js::to_string(Some(value))) {
            Some(c) => res(dtcg::COLOR, c, description),
            None => raw(
                dtcg::COLOR,
                "not a plain hex/rgb/hsl colour (expression?)",
                warn,
            ),
        },
        Some(penpot_type::FONT_FAMILIES | dtcg::FONT_FAMILY) => {
            res(dtcg::FONT_FAMILY, split_families(value), description)
        }
        Some(penpot_type::FONT_WEIGHTS | dtcg::FONT_WEIGHT) => {
            match parse_weight(Some(value), warn) {
                Some(w) => res(dtcg::FONT_WEIGHT, w, description),
                None => raw(dtcg::FONT_WEIGHT, "not a numeric or named weight", warn),
            }
        }
        Some(t @ (penpot_type::NUMBER | "opacity" | "rotation")) => match parse_number(Some(value))
        {
            None => raw(
                if wants_duration {
                    dtcg::DURATION
                } else {
                    dtcg::NUMBER
                },
                "not a plain number",
                warn,
            ),
            Some(n) if wants_duration => res(
                dtcg::DURATION,
                json!({ (key::AMOUNT): n, (key::UNIT): unit::MS }),
                description,
            ),
            Some(n) => {
                if t != penpot_type::NUMBER {
                    warn(format!("Penpot {t} token stored as a dfw number"));
                }
                res(dtcg::NUMBER, n, description)
            }
        },
        Some(penpot_type::TEXT_CASE | penpot_type::TEXT_DECORATION) => {
            res(dtcg::STRING, js::to_string(Some(value)), description)
        }
        Some(penpot_type::SHADOW | "boxShadow") => match value {
            Value::String(_) => raw(dtcg::SHADOW, "shadow given as a string", warn),
            Value::Array(items) => {
                let list: Result<Vec<Value>, DfwError> = items.iter().map(shadow_in).collect();
                res(dtcg::SHADOW, Value::Array(list?), description)
            }
            other => res(dtcg::SHADOW, shadow_in(other)?, description),
        },
        Some(penpot_type::TYPOGRAPHY) => match value {
            Value::Object(_) | Value::Array(_) => {
                res(dtcg::TYPOGRAPHY, typography_in(value, warn), description)
            }
            _ => raw(dtcg::TYPOGRAPHY, "typography given as a string", warn),
        },
        Some(t) if PENPOT_DIMENSION_TYPES.contains(&t) => match parse_dimension(Some(value)) {
            Some(d) => res(dtcg::DIMENSION, d, description),
            None => raw(dtcg::DIMENSION, "not a plain number with px/rem/em", warn),
        },
        _ => unsupported(),
    })
}

fn shadow_in(s: &Value) -> Result<Value, DfwError> {
    match s {
        Value::String(_) => return Ok(s.clone()),
        Value::Null => {
            return Err(DfwError::new(
                "A shadow token in the Penpot file has an empty (null) layer. Fix the token in Penpot and export the tokens again.",
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
    let color = s.get(key::COLOR);
    let color = if is_alias(color) {
        color.cloned()
    } else {
        parse_color(&js::to_string(color)).or_else(|| color.cloned())
    };
    let either = |a: &str, b: &str| js::present(s.get(a)).or(s.get(b));
    let inset = js::truthy(s.get(key::INSET))
        || s.get(key::SHADOW_TYPE).and_then(Value::as_str) == Some("innerShadow");
    Ok(js::object(vec![
        (key::COLOR, color),
        (key::OFFSET_X, Some(d(either(key::OFFSET_X, key::X)))),
        (key::OFFSET_Y, Some(d(either(key::OFFSET_Y, key::Y)))),
        (key::BLUR, Some(d(s.get(key::BLUR)))),
        (key::SPREAD, Some(d(s.get(key::SPREAD)))),
        (key::INSET, inset.then_some(Value::Bool(true))),
    ]))
}

fn typography_in(value: &Value, warn: Note) -> Value {
    let pick = |keys: &[&str]| keys.iter().find_map(|k| value.get(*k));
    let or_self = |parsed: Option<Value>, raw: &Value| parsed.unwrap_or_else(|| raw.clone());
    let mut v = Map::new();
    if let Some(fam) = pick(&[key::FONT_FAMILIES, key::FONT_FAMILY, key::CSS_FONT_FAMILY]) {
        let out = if is_alias(Some(fam)) {
            fam.clone()
        } else {
            split_families(fam)
        };
        v.insert(key::FONT_FAMILY.into(), out);
    }
    if let Some(size) = pick(&[key::FONT_SIZES, key::FONT_SIZE, key::CSS_FONT_SIZE]) {
        let out = if is_alias(Some(size)) {
            size.clone()
        } else {
            or_self(parse_dimension(Some(size)), size)
        };
        v.insert(key::FONT_SIZE.into(), out);
    }
    if let Some(w) = pick(&[key::FONT_WEIGHTS, key::FONT_WEIGHT, key::CSS_FONT_WEIGHT]) {
        let out = if is_alias(Some(w)) {
            w.clone()
        } else {
            or_self(parse_weight(Some(w), warn), w)
        };
        v.insert(key::FONT_WEIGHT.into(), out);
    }
    if let Some(ls) = pick(&[key::LETTER_SPACING, key::CSS_LETTER_SPACING]) {
        let out = if is_alias(Some(ls)) {
            ls.clone()
        } else {
            or_self(parse_dimension(Some(ls)), ls)
        };
        v.insert(key::LETTER_SPACING.into(), out);
    }
    if let Some(lh) = pick(&[key::LINE_HEIGHTS, key::LINE_HEIGHT, key::CSS_LINE_HEIGHT]) {
        let out = if is_alias(Some(lh)) {
            lh.clone()
        } else {
            or_self(parse_number(Some(lh)), lh)
        };
        v.insert(key::LINE_HEIGHT.into(), out);
    }
    if let Some(tc) = pick(&[key::TEXT_CASE, key::CSS_TEXT_CASE]) {
        v.insert(key::TEXT_CASE.into(), tc.clone());
    }
    if let Some(td) = pick(&[key::TEXT_DECORATION, key::CSS_TEXT_DECORATION]) {
        v.insert(key::TEXT_DECORATION.into(), td.clone());
    }
    Value::Object(v)
}
