//! Inline styles and token names of `edit.mjs`: `parseStyle`, `setStyleProp`, `loadTokenNames`,
//! `tokenVar` and `layoutValue`.

use std::path::Path;

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::design::tokens::css_var_name;

/// `WIDTH_PERCENTS`: the widths a resize may snap to.
pub const WIDTH_PERCENTS: [&str; 6] = ["25%", "33.333%", "50%", "66.667%", "75%", "100%"];
const MAX_SPAN: f64 = 24.0;

/// `parseStyle`: an inline style as `(property, value)` pairs; `;` inside parentheses or quotes
/// does not split. Custom properties keep their case (`--colW` is not `--colw`), other names
/// are lower-cased.
pub fn parse_style(style: &str) -> Vec<(String, String)> {
    fn flush(cur: &mut String, out: &mut Vec<(String, String)>) {
        if let Some(i) = cur.find(':').filter(|i| *i > 0) {
            let prop = js::trim(&cur[..i]);
            let prop = if prop.starts_with("--") {
                prop.to_string()
            } else {
                prop.to_lowercase()
            };
            out.push((prop, js::trim(&cur[i + 1..]).to_string()));
        }
        cur.clear();
    }
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut cur = String::new();
    for ch in style.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                }
            }
            None => match ch {
                '"' | '\'' => quote = Some(ch),
                '(' => depth += 1,
                ')' => depth = depth.saturating_sub(1),
                ';' if depth == 0 => {
                    flush(&mut cur, &mut out);
                    continue;
                }
                _ => {}
            },
        }
        cur.push(ch);
    }
    flush(&mut cur, &mut out);
    out
}

/// `setStyleProp`: the style with `prop` set (`Some`) or removed (`None`). Setting `gap`,
/// `padding` or `margin` also drops the longhands it replaces.
pub fn set_style_prop(style: &str, prop: &str, value: Option<&str>) -> String {
    let in_family = |name: &str| match prop {
        "gap" => name == "row-gap" || name == "column-gap",
        "padding" | "margin" => name
            .strip_prefix(prop)
            .is_some_and(|rest| rest.starts_with('-')),
        _ => false,
    };
    let mut decls: Vec<(String, String)> = parse_style(style)
        .into_iter()
        .filter(|(name, _)| name != prop && !in_family(name))
        .collect();
    if let Some(value) = value {
        decls.push((prop.to_string(), value.to_string()));
    }
    let parts: Vec<String> = decls.iter().map(|(k, v)| format!("{k}: {v}")).collect();
    parts.join("; ")
}

/// One token of a DTCG tree, as the `walk` of edit.mjs and system-page.mjs sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenEntry {
    /// Dotted name (`space.4`).
    pub name: String,
    /// `$type` of the token or the nearest group that has one (`node.$type || type`).
    pub ty: Option<Value>,
    /// `$value`.
    pub value: Value,
    /// `$description` when it is truthy.
    pub description: Option<Value>,
}

impl TokenEntry {
    /// True when the type is the string `name` (`t.type === 'color'`).
    pub fn is_type(&self, name: &str) -> bool {
        self.ty.as_ref().and_then(Value::as_str) == Some(name)
    }

    /// The css variable of the token (`--space-4`).
    pub fn var(&self) -> String {
        css_var_name(&self.name)
    }
}

/// Every token of a tree in the order JavaScript walks it (integer-like keys first). A name
/// that appears twice is listed twice: callers that built a `Map` keep the last one.
// TODO(merge): design::tokens has the same walk as a private `collect` without descriptions.
pub fn collect_tokens(tree: &Value) -> Vec<TokenEntry> {
    fn walk(node: &Value, trail: &mut Vec<String>, ty: Option<&Value>, out: &mut Vec<TokenEntry>) {
        let children: Vec<(String, &Value)> = match node {
            Value::Object(map) => {
                let ty = map.get("$type").filter(|t| js::truthy(Some(t))).or(ty);
                if let Some(value) = map.get("$value") {
                    out.push(TokenEntry {
                        name: trail.join("."),
                        ty: ty.cloned(),
                        value: value.clone(),
                        description: map
                            .get("$description")
                            .filter(|d| js::truthy(Some(d)))
                            .cloned(),
                    });
                    return;
                }
                for (key, child) in js::entries(map) {
                    if !key.starts_with('$') {
                        trail.push(key.clone());
                        walk(child, trail, ty, out);
                        trail.pop();
                    }
                }
                return;
            }
            // An array is an object without `$type` and `$value`: its indexes are the keys.
            Value::Array(items) => items
                .iter()
                .enumerate()
                .map(|(i, v)| (i.to_string(), v))
                .collect(),
            _ => return,
        };
        for (key, child) in children {
            trail.push(key);
            walk(child, trail, ty, out);
            trail.pop();
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), None, &mut out);
    out
}

/// `loadTokenNames`: the tokens of `design/tokens/tokens.json` by name, in file order (a `Map`
/// in the Node code: a repeated name keeps its first place and takes the later token).
pub fn load_token_names(root: &Path) -> Result<Vec<TokenEntry>, DfwError> {
    let file = project_paths(root).tokens.join("tokens.json");
    let tree = js::read_json(&file, Some(Value::Null))?;
    let mut names: Vec<TokenEntry> = Vec::new();
    for token in collect_tokens(&tree) {
        match names.iter_mut().find(|t| t.name == token.name) {
            Some(slot) => *slot = token,
            None => names.push(token),
        }
    }
    Ok(names)
}

/// `tokenVar`: a token name (`space.4`) or css variable (`--space-4`) that starts with one of
/// `prefixes`, as `var(--space-4)`.
pub fn token_var(root: &Path, value: &str, prefixes: &[&str]) -> Result<String, DfwError> {
    let tokens = load_token_names(root)?;
    let name = if value.starts_with("--") {
        tokens
            .iter()
            .find(|t| t.var() == value)
            .map(|t| t.name.as_str())
    } else {
        Some(value)
    };
    let token = name
        .filter(|n| !n.is_empty() && prefixes.iter().any(|p| n.starts_with(p)))
        .and_then(|n| tokens.iter().find(|t| t.name == n));
    match token {
        Some(token) => Ok(format!("var({})", token.var())),
        None => {
            let kinds: Vec<String> = prefixes.iter().map(|p| format!("{p}*")).collect();
            Err(DfwError::new(format!(
                "\"{value}\" is not a {} token.",
                kinds.join(" or ")
            )))
        }
    }
}

/// `layoutValue`: check a resize value and return the CSS to write; `None` removes the
/// declaration (an empty value or `auto`: back to the screen's own layout).
pub fn layout_value(
    root: &Path,
    prop: &str,
    value: Option<&Value>,
) -> Result<Option<String>, DfwError> {
    let text = js::present(value).map(|v| js::to_string(Some(v)));
    let v = js::trim(text.as_deref().unwrap_or_default());
    if v.is_empty() || v == "auto" {
        return Ok(None);
    }
    if prop == "grid-column" {
        let span = crate::core::js::regex::with_js_classes(r"^(?:span\s+)?(\d+)$")?;
        let n = span
            .captures(v)
            .map(|m| js::str_to_number(&m[1]))
            .filter(|n| (1.0..=MAX_SPAN).contains(n));
        return match n {
            Some(n) => Ok(Some(format!("span {}", js::number(n)))),
            None => Err(DfwError::new(format!(
                "grid-column must be \"span N\" with N from 1 to {MAX_SPAN}."
            ))),
        };
    }
    if prop == "width" && v.ends_with('%') {
        if !WIDTH_PERCENTS.contains(&v) {
            return Err(DfwError::new(format!(
                "Width percentages snap to {}.",
                WIDTH_PERCENTS.join(", ")
            )));
        }
        return Ok(Some(v.to_string()));
    }
    token_var(root, v, &["size.", "space."]).map(Some)
}
