//! dfw value -> Penpot string: one dfw token in its Penpot form (`tokenToPenpot`), the
//! Penpot set tree (`nest`) and the whole token file (`dfwToPenpot`).

use super::*;

/// Collects the notes of a lossy conversion.
pub(super) type Note<'a> = &'a mut dyn FnMut(String);

/// `x.toString(16).padStart(2, '0')` for a rounded channel.
pub(super) fn hex2(x: f64) -> String {
    if x.is_finite() {
        format!("{:0>2}", format!("{:x}", x.max(0.0) as u64))
    } else {
        js::number(x)
    }
}

/// `parseInt(text, 16)`: the leading hexadecimal digits, `NaN` without any.
pub(super) fn parse_int16(text: &str) -> f64 {
    let t = js::trim(text);
    let (sign, t) = match t.strip_prefix('-') {
        Some(rest) => (-1.0, rest),
        None => (1.0, t.strip_prefix('+').unwrap_or(t)),
    };
    let t = t
        .strip_prefix("0x")
        .or_else(|| t.strip_prefix("0X"))
        .unwrap_or(t);
    let digits: Vec<u32> = t.chars().map_while(|c| c.to_digit(16)).collect();
    if digits.is_empty() {
        return f64::NAN;
    }
    sign * digits.iter().fold(0.0, |n, d| n * 16.0 + f64::from(*d))
}

/// The three channels of `#rrggbb`, as `parseInt` of each pair.
fn hex_channels(hex: &str) -> Vec<f64> {
    [1, 3, 5]
        .iter()
        .map(|i| parse_int16(&js::slice16(hex, *i, i + 2)))
        .collect()
}

fn color_out(v: Option<&Value>, note: Note) -> Result<Value, String> {
    let v = match v {
        Some(Value::String(s)) => return Ok(Value::String(s.clone())),
        Some(v @ (Value::Object(_) | Value::Array(_))) => v,
        other => return Err(format!("not a colour: {}", js::stringify(other))),
    };
    let alpha = js::present(v.get("alpha"));
    let opaque = alpha.is_none_or(|a| a.as_f64() == Some(1.0));
    let space = v.get("colorSpace").and_then(Value::as_str);
    let components = v.get("components");
    let hex = js::truthy_value(v.get("hex"));
    let numbers = |list: &Vec<Value>| list.iter().all(Value::is_number);
    let rgb: Vec<f64> = if let Some(list) = components
        .and_then(Value::as_array)
        .filter(|list| space == Some("srgb") && numbers(list))
    {
        match hex {
            Some(Value::String(h)) if opaque => return Ok(Value::String(h.to_lowercase())),
            Some(_) if opaque => return Err("\"hex\" must be text like \"#0066ff\"".to_string()),
            _ => {}
        }
        list.iter()
            .map(|x| js::round(js::min(1.0, js::max(0.0, js::to_number(Some(x)))) * 255.0))
            .collect()
    } else if space == Some("oklch") && js::present(components).is_some() {
        let Some(list) = components.and_then(Value::as_array) else {
            return Err("oklch \"components\" must be a list of three numbers".to_string());
        };
        if !numbers(list) {
            return hex_fallback(v, hex, opaque, note);
        }
        let hex = oklch_to_hex(Oklch {
            l: js::to_number(list.first()),
            c: js::to_number(list.get(1)),
            h: js::to_number(list.get(2)),
        });
        note(format!("oklch converted to sRGB {hex}"));
        if opaque {
            return Ok(Value::String(hex));
        }
        hex_channels(&hex)
    } else {
        return hex_fallback(v, hex, opaque, note);
    };
    Ok(Value::String(rgb_text(&rgb, opaque, alpha)))
}

/// The last two branches of `colorOut`: another colour space goes through its `hex`.
fn hex_fallback(v: &Value, hex: Option<&Value>, opaque: bool, note: Note) -> Result<Value, String> {
    let space = js::to_string(v.get("colorSpace"));
    let Some(hex) = hex else {
        return Err(format!(
            "colour space \"{space}\" has no sRGB form (add a \"hex\" fallback)"
        ));
    };
    note(format!("{space} colour exported through its hex fallback"));
    let Value::String(hex) = hex else {
        return Err("\"hex\" must be text like \"#0066ff\"".to_string());
    };
    if opaque {
        return Ok(Value::String(hex.to_lowercase()));
    }
    Ok(Value::String(rgb_text(
        &hex_channels(hex),
        opaque,
        js::present(v.get("alpha")),
    )))
}

fn rgb_text(rgb: &[f64], opaque: bool, alpha: Option<&Value>) -> String {
    if opaque {
        return format!("#{}", rgb.iter().map(|x| hex2(*x)).collect::<String>());
    }
    let channels: Vec<String> = rgb.iter().map(|x| js::number(*x)).collect();
    format!(
        "rgba({}, {})",
        channels.join(", "),
        fmt(js::to_number(alpha))
    )
}

/// dfw wraps Penpot math in `calc()` so the CSS stays valid; Penpot gets the bare expression.
fn uncalc(text: &str) -> String {
    match CALC.captures(text) {
        Some(c) => c[1].to_string(),
        None => text.to_string(),
    }
}

pub(super) fn is_expression(value: &Value) -> bool {
    let Value::String(s) = value else {
        return false;
    };
    let plain = ALIAS.replace_all(s, "x");
    MATH.is_match(plain.strip_prefix('-').unwrap_or(&plain))
}

fn dim_out(v: Option<&Value>, note: Note, bare: bool) -> Result<String, String> {
    match v {
        Some(Value::String(_) | Value::Number(_)) => Ok(uncalc(&js::to_string(v))),
        Some(Value::Object(map)) if map.get("value").is_some_and(Value::is_number) => {
            let value = fmt(js::to_number(map.get("value")));
            if map.get("unit").and_then(Value::as_str) == Some("px") {
                return Ok(if bare { value } else { format!("{value}px") });
            }
            let unit = js::to_string(map.get("unit"));
            note(format!(
                "unit \"{unit}\": Penpot works in px; check the value after import"
            ));
            Ok(format!("{value}{unit}"))
        }
        other => Err(format!("not a dimension: {}", js::stringify(other))),
    }
}

fn shadow_out(v: &Value, note: Note) -> Result<Value, String> {
    fn one(s: &Value, note: Note) -> Result<Value, String> {
        match s {
            Value::String(_) => return Ok(s.clone()),
            Value::Null => return Err("a shadow layer is empty (null)".to_string()),
            _ => {}
        }
        let zero = json!({ "value": 0, "unit": "px" });
        let color = color_out(s.get("color"), note)?;
        let offset_x = dim_out(s.get("offsetX"), note, true)?;
        let offset_y = dim_out(s.get("offsetY"), note, true)?;
        let blur = dim_out(s.get("blur"), note, true)?;
        let spread = dim_out(js::present(s.get("spread")).or(Some(&zero)), note, true)?;
        Ok(json!({
            "color": color,
            "offsetX": offset_x,
            "offsetY": offset_y,
            "blur": blur,
            "spread": spread,
            "inset": js::truthy(s.get("inset")),
        }))
    }
    match v {
        Value::String(_) => Ok(v.clone()),
        Value::Array(items) => {
            let mut list = Vec::new();
            for s in items {
                list.push(one(s, note)?);
            }
            Ok(Value::Array(list))
        }
        other => one(other, note),
    }
}

const TYPO_OUT: [(&str, &str); 7] = [
    ("fontFamily", "fontFamilies"),
    ("fontSize", "fontSizes"),
    ("fontWeight", "fontWeights"),
    ("letterSpacing", "letterSpacing"),
    ("lineHeight", "lineHeights"),
    ("textCase", "textCase"),
    ("textDecoration", "textDecoration"),
];
const TEXT_CASES: [&str; 4] = ["none", "uppercase", "lowercase", "capitalize"];
const TEXT_DECORATIONS: [&str; 3] = ["underline", "strike-through", "line-through"];

/// `penpotDimensionType`: the Penpot type for a dfw dimension token, from its name (dfw has one
/// `dimension` type, Penpot several).
pub fn penpot_dimension_type(name: &str, value: &Value) -> &'static str {
    let n = name.to_lowercase();
    let px = match value {
        Value::Object(map) => map.get("unit").and_then(Value::as_str) == Some("px"),
        Value::Array(_) => false,
        _ => true,
    };
    if FONT_SIZE.is_match(&n) {
        return if px { "fontSizes" } else { "dimension" };
    }
    if TRACKING.is_match(&n) {
        return if px { "letterSpacing" } else { "dimension" };
    }
    if RADIUS.is_match(&n) {
        "borderRadius"
    } else if BORDER_WIDTH.is_match(&n) {
        "borderWidth"
    } else if SPACING.is_match(&n) {
        "spacing"
    } else if SIZING.is_match(&n) {
        "sizing"
    } else {
        "dimension"
    }
}

/// The result of converting one token: its form on the other side, or why it has none.
#[derive(Debug, Clone, PartialEq)]
pub enum Converted {
    Token {
        ty: Value,
        value: Value,
        description: Option<Value>,
    },
    Unsupported(String),
}

impl Converted {
    /// The reason, when the token has no form on the other side.
    pub fn unsupported(&self) -> Option<&str> {
        match self {
            Converted::Unsupported(reason) => Some(reason),
            Converted::Token { .. } => None,
        }
    }

    /// The converted value.
    pub fn value(&self) -> Option<&Value> {
        match self {
            Converted::Token { value, .. } => Some(value),
            Converted::Unsupported(_) => None,
        }
    }

    /// The description that travels with the token (only a truthy one does).
    pub fn description(&self) -> Option<&Value> {
        match self {
            Converted::Token { description, .. } => description.as_ref(),
            Converted::Unsupported(_) => None,
        }
    }

    /// The token as the object the Node code returned.
    #[cfg(test)]
    pub fn to_json(&self) -> Value {
        match self {
            Converted::Token {
                ty,
                value,
                description,
            } => js::object(vec![
                ("type", Some(ty.clone())),
                ("value", Some(value.clone())),
                ("description", description.clone()),
            ]),
            Converted::Unsupported(reason) => json!({ "unsupported": reason }),
        }
    }
}

pub(super) fn res(
    ty: impl Into<Value>,
    value: impl Into<Value>,
    description: Option<&Value>,
) -> Converted {
    Converted::Token {
        ty: ty.into(),
        value: value.into(),
        description: js::truthy_value(description).cloned(),
    }
}

fn families_out(value: &Value) -> String {
    match value {
        Value::Array(items) => js::join(items, ", "),
        other => js::to_string(Some(other)),
    }
}

/// `tokenToPenpot`: one dfw token in its Penpot form. `note` collects lossy conversions.
pub fn token_to_penpot(name: &str, token: &Token, note: Note) -> Converted {
    to_penpot(name, token, note).unwrap_or_else(Converted::Unsupported)
}

/// [`token_to_penpot`] without collecting notes.
pub fn token_to_penpot_quiet(name: &str, token: &Token) -> Converted {
    token_to_penpot(name, token, &mut |_| {})
}

/// `typeof value !== 'object'`.
fn is_primitive(value: &Value) -> bool {
    !matches!(value, Value::Null | Value::Object(_) | Value::Array(_))
}

fn to_penpot(name: &str, token: &Token, note: Note) -> Result<Converted, String> {
    let value = &token.value;
    let description = token.description.as_ref();
    let done = |ty: &str, value: Value| Ok(res(ty, value, description));
    match token.type_name() {
        Some("color") => {
            if is_alias(Some(value)) {
                done("color", value.clone())
            } else {
                done("color", color_out(Some(value), note)?)
            }
        }
        Some("dimension") => {
            let ty = penpot_dimension_type(name, value);
            let text = match value {
                Value::String(s) => uncalc(s),
                other => dim_out(Some(other), note, false)?,
            };
            done(ty, text.into())
        }
        Some("fontFamily") => done("fontFamilies", families_out(value).into()),
        Some("fontWeight") => done("fontWeights", js::to_string(Some(value)).into()),
        Some("number") => done("number", uncalc(&js::to_string(Some(value))).into()),
        Some("duration") => {
            note(
                "duration exported as a Penpot \"number\" token in ms (Penpot has no duration type)"
                    .to_string(),
            );
            match value {
                Value::Null => Err(
                    "the value is empty (null); give it {\"value\": 200, \"unit\": \"ms\"}"
                        .to_string(),
                ),
                Value::Object(_) | Value::Array(_) => {
                    let n = js::to_number(value.get("value"));
                    let seconds = value.get("unit").and_then(Value::as_str) == Some("s");
                    done("number", fmt(if seconds { n * 1000.0 } else { n }).into())
                }
                other => done("number", uncalc(&js::to_string(Some(other))).into()),
            }
        }
        Some("string") => match value.as_str() {
            Some(v) if TEXT_CASES.contains(&v) => done("textCase", v.into()),
            Some("line-through") => done("textDecoration", "strike-through".into()),
            Some(v) if TEXT_DECORATIONS.contains(&v) => done("textDecoration", v.into()),
            _ => Ok(Converted::Unsupported(
                "Penpot has no free-text token type (only textCase/textDecoration values map)"
                    .to_string(),
            )),
        },
        Some("shadow") => done("shadow", shadow_out(value, note)?),
        Some("typography") => {
            if is_alias(Some(value)) {
                return done("typography", value.clone());
            }
            if value.is_null() {
                return Err(
                    "the value is empty (null); give it fontFamily, fontSize and the other fields"
                        .to_string(),
                );
            }
            let mut v = Map::new();
            for (key, penpot_key) in TYPO_OUT {
                let Some(sub) = value.get(key) else {
                    continue;
                };
                let out: Value = if is_alias(Some(sub)) {
                    sub.clone()
                } else if key == "fontFamily" {
                    families_out(sub).into()
                } else if key == "fontSize" || key == "letterSpacing" || !is_primitive(sub) {
                    dim_out(Some(sub), note, false)?.into()
                } else {
                    js::to_string(Some(sub)).into()
                };
                v.insert(penpot_key.to_string(), out);
            }
            done("typography", Value::Object(v))
        }
        _ => Ok(Converted::Unsupported(format!(
            "Penpot has no \"{}\" token type",
            js::to_string(token.ty.as_ref())
        ))),
    }
}

/// One token for [`nest`]: a field that is `None` is not written.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NestEntry {
    pub name: String,
    pub value: Option<Value>,
    pub ty: Option<Value>,
    pub description: Option<Value>,
}

/// `nest`: a nested Penpot set from flat entries; names with a reserved segment are left out.
pub fn nest(entries: &[NestEntry]) -> Value {
    let mut root = json!({});
    for entry in entries {
        let Some(parts) = token_path(&entry.name) else {
            continue;
        };
        let Some((leaf, groups)) = parts.split_last() else {
            continue;
        };
        // Node bug kept: `node[name]` of a new group with an inherited name found the built-in
        // function, and the token went onto it instead of into the set.
        let mut probe = Some(&root);
        let lost = groups.iter().any(|part| {
            let here = probe.and_then(Value::as_object);
            let own = here.is_some_and(|map| map.contains_key(*part));
            probe = here.and_then(|map| map.get(*part));
            !own && INHERITED.contains(part)
        });
        if lost {
            continue;
        }
        let mut node = &mut root;
        for part in groups {
            let Value::Object(map) = node else {
                break;
            };
            let slot = map.entry((*part).to_string()).or_insert(Value::Null);
            if !slot.is_object() {
                *slot = json!({});
            }
            node = slot;
        }
        if let Value::Object(map) = node {
            map.insert(
                (*leaf).to_string(),
                js::object(vec![
                    ("$value", entry.value.clone()),
                    ("$type", entry.ty.clone()),
                    (
                        "$description",
                        js::truthy_value(entry.description.as_ref()).cloned(),
                    ),
                ]),
            );
        }
    }
    js::key_order(root)
}

/// What an export left out or changed (`report` of `dfwToPenpot`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExportReport {
    pub exported: usize,
    pub sets: Vec<String>,
    /// `{ set, token, type, reason }`
    pub unsupported: Vec<Value>,
    /// `{ set, token, note }`
    pub lossy: Vec<Value>,
    pub skipped_themes: Vec<String>,
}

impl ExportReport {
    /// The report as the object the Node code printed.
    pub fn to_json(&self) -> Value {
        json!({
            "exported": self.exported,
            "sets": self.sets,
            "unsupported": self.unsupported,
            "lossy": self.lossy,
            "skippedThemes": self.skipped_themes,
        })
    }
}

/// `dfwToPenpot`: dfw tokens (base tree and themes by name) as a Penpot token file.
pub fn dfw_to_penpot(base: Option<&Value>, themes: &Map<String, Value>) -> (Value, ExportReport) {
    let mut report = ExportReport::default();
    let mut doc = Map::new();
    let empty = json!({});
    let theme_names: Vec<&String> = js::entries(themes).into_iter().map(|(k, _)| k).collect();
    let mut sets: Vec<(String, &Value)> = vec![(
        BASE_SET.to_string(),
        js::truthy_value(base).unwrap_or(&empty),
    )];
    for name in &theme_names {
        let tree = js::truthy_value(themes.get(*name)).unwrap_or(&empty);
        sets.push((format!("{THEME_PREFIX}{name}"), tree));
    }
    for (set, tree) in &sets {
        let mut entries = Vec::new();
        let flat = flatten_tokens(tree);
        for (name, token) in flat.iter() {
            let mut notes = Vec::new();
            let converted = token_to_penpot(name, token, &mut |n| notes.push(n));
            for note in notes {
                report
                    .lossy
                    .push(json!({ "set": set, "token": name, "note": note }));
            }
            match converted {
                Converted::Unsupported(reason) => report.unsupported.push(js::object(vec![
                    ("set", Some(set.as_str().into())),
                    ("token", Some(name.into())),
                    ("type", token.ty.clone()),
                    ("reason", Some(reason.into())),
                ])),
                Converted::Token {
                    ty,
                    value,
                    description,
                } => {
                    entries.push(NestEntry {
                        name: name.to_string(),
                        value: Some(value),
                        ty: Some(ty),
                        description,
                    });
                    report.exported += 1;
                }
            }
        }
        doc.insert(set.clone(), nest(&entries));
    }
    report.sets = sets.iter().map(|(s, _)| s.clone()).collect();
    let mut penpot_themes = vec![json!({
        "name": "default",
        "group": THEME_GROUP,
        "description": "dfw base tokens (design/tokens/tokens.json)",
        "isSource": false,
        "selectedTokenSets": { BASE_SET: "enabled" },
    })];
    for name in &theme_names {
        penpot_themes.push(json!({
            "name": name,
            "group": THEME_GROUP,
            "description": format!("dfw theme design/tokens/themes/{name}.json over the base"),
            "isSource": false,
            "selectedTokenSets": { BASE_SET: "enabled", format!("{THEME_PREFIX}{name}"): "enabled" },
        }));
    }
    doc.insert("$themes".into(), Value::Array(penpot_themes));
    doc.insert(
        "$metadata".into(),
        json!({
            "tokenSetOrder": report.sets,
            "activeThemes": [format!("{THEME_GROUP}/default")],
            "activeSets": [BASE_SET],
        }),
    );
    (Value::Object(doc), report)
}
