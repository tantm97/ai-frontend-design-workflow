//! Ports `core/lib/dsaudit.mjs`: the design system audit.
//!
//! Senior design-system review of the project's tokens and DESIGN.md: contrast, semantic colour
//! collisions, neutral temperature, hue budget, type rhythm and hierarchy, family/radius sprawl,
//! tinted elevation, whether every decision has a written reason, and whether every component
//! master is designed for the product (inventory, no verbatim kit copies).

use std::path::Path;

use serde_json::{json, Map, Value};

use super::color::{audit_colors, audit_type_scale, hex_to_oklch};

use super::inventory::inventory_findings;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::core::util::read_json;

const TOKENS_FILE: &str = "design/tokens/tokens.json";

const ROLES: [&str; 20] = [
    "canvas",
    "surface",
    "surface-sunken",
    "surface-hover",
    "ink",
    "muted",
    "subtle",
    "border",
    "border-strong",
    "rule",
    "accent",
    "accent-hover",
    "accent-soft",
    "accent-text",
    "on-accent",
    "focus",
    "focus-ring",
    "brand",
    "on-brand",
    "signature",
];

/// A token of the tree: its `$value`, its own or inherited `$type`, whether it has a usage rule.
struct Token<'a> {
    name: String,
    value: Option<&'a Value>,
    kind: Option<&'a Value>,
    described: bool,
}

/// `Object.entries(node)` for an object or an array (whose keys are its indexes).
fn entries(node: &Value) -> Vec<(String, &Value)> {
    match node {
        Value::Object(map) => js::entries(map)
            .into_iter()
            .map(|(k, v)| (k.clone(), v))
            .collect(),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, v)| (i.to_string(), v))
            .collect(),
        _ => Vec::new(),
    }
}

/// `collect(tree)`: every token by dotted name, in the order of the file.
fn collect(tree: &Value) -> Vec<Token<'_>> {
    fn walk<'a>(
        node: &'a Value,
        trail: &mut Vec<String>,
        inherited: Option<&'a Value>,
        out: &mut Vec<Token<'a>>,
    ) {
        let kind = node
            .get("$type")
            .filter(|t| js::truthy(Some(t)))
            .or(inherited);
        if let Some(map) = node.as_object().filter(|m| m.contains_key("$value")) {
            let token = Token {
                name: trail.join("."),
                value: map.get("$value"),
                kind,
                described: js::truthy(map.get("$description")),
            };
            // A Map: a name seen again keeps its place and takes the new token.
            match out.iter_mut().find(|t| t.name == token.name) {
                Some(slot) => *slot = token,
                None => out.push(token),
            }
            return;
        }
        for (k, v) in entries(node) {
            if !k.starts_with('$') {
                trail.push(k);
                walk(v, trail, kind, out);
                trail.pop();
            }
        }
    }
    let mut out = Vec::new();
    walk(tree, &mut Vec::new(), None, &mut out);
    out
}

/// `/^\{[^}]+\}$/`: a value that is one alias; the name inside the braces.
fn alias(value: &str) -> Option<&str> {
    let name = value.strip_prefix('{')?.strip_suffix('}')?;
    (!name.is_empty() && !name.contains('}')).then_some(name)
}

/// `resolver(tokens)`: follow aliases (at most ten) to the token that holds a value.
struct Resolver<'a> {
    tokens: &'a [Token<'a>],
}

impl<'a> Resolver<'a> {
    fn get(&self, name: &str) -> Option<&'a Token<'a>> {
        let mut name = name.to_string();
        for _ in 0..=10 {
            let token = self.tokens.iter().find(|t| t.name == name)?;
            match token.value.and_then(Value::as_str).and_then(alias) {
                Some(next) => name = next.to_string(),
                None => return Some(token),
            }
        }
        None
    }

    /// The colour a token resolves to: the first seven characters of a `#` text, or the `hex`
    /// of a colour object.
    fn hex(&self, name: &str) -> Option<Value> {
        match self.get(name)?.value? {
            Value::String(s) => s.starts_with('#').then(|| json!(js::slice16(s, 0, 7))),
            other => other.get("hex").filter(|h| js::truthy(Some(h))).cloned(),
        }
    }

    /// `r.get('font.size.<k>')?.value?.value ?? null`.
    fn size(&self, key: &str) -> Option<&'a Value> {
        js::present(
            self.get(&format!("font.size.{key}"))?
                .value
                .and_then(|v| v.get("value")),
        )
    }
}

/// `value?.value` of a dimension token.
fn inner_value<'a>(token: &Token<'a>) -> Option<&'a Value> {
    token.value.and_then(|v| v.get("value"))
}

/// Values a JavaScript `Set` holds once: text, numbers, booleans, null and "missing" by value;
/// every object or array is its own entry.
fn distinct<'a>(values: impl Iterator<Item = Option<&'a Value>>) -> Vec<Option<&'a Value>> {
    let mut out: Vec<Option<&Value>> = Vec::new();
    for value in values {
        let composite = value.is_some_and(|v| v.is_object() || v.is_array());
        let seen = !composite
            && out.iter().any(|have| match (have, value) {
                (Some(Value::Number(a)), Some(Value::Number(b))) => a.as_f64() == b.as_f64(),
                (a, b) => *a == b,
            });
        if !seen {
            out.push(value);
        }
    }
    out
}

/// `Math.max(...c)` and `Math.min(...c)` of a JSON array (`NaN` when an item is not numeric).
fn max_min(items: &[Value]) -> (f64, f64) {
    let numbers: Vec<f64> = items.iter().map(|v| js::to_number(Some(v))).collect();
    if numbers.iter().any(|n| n.is_nan()) {
        return (f64::NAN, f64::NAN);
    }
    let max = numbers.iter().fold(f64::NEG_INFINITY, |m, n| m.max(*n));
    let min = numbers.iter().fold(f64::INFINITY, |m, n| m.min(*n));
    (max, min)
}

/// A token value of the wrong type: the audit stops (a token file of the wrong shape never
/// passes a gate).
fn malformed(what: &str) -> DfwError {
    super::malformed("design/tokens/tokens.json", what)
}

/// A shadow whose every layer is a neutral near-black. Like `every`, it stops at the first
/// layer that is not, so only the layers before it have to be well-formed.
fn is_grey_shadow(name: &str, value: Option<&Value>) -> Result<bool, DfwError> {
    let layers = match value {
        Some(Value::Array(layers)) => layers.as_slice(),
        Some(layer) => std::slice::from_ref(layer),
        None => &[],
    };
    for layer in layers {
        if layer.is_null() {
            return Err(malformed(&format!("a layer of shadow {name} is null")));
        }
        let components = layer.get("color").and_then(|c| c.get("components"));
        let grey = match components.filter(|c| js::truthy(Some(c))) {
            None => false,
            Some(Value::Array(list)) => {
                let (max, min) = max_min(list);
                max - min < 0.02 && max < 0.05
            }
            // `Math.max(...text)` spreads the characters: not numbers unless they are digits.
            Some(Value::String(text)) => {
                let digits: Vec<Value> = text.chars().map(|c| json!(c.to_string())).collect();
                let (max, min) = max_min(&digits);
                max - min < 0.02 && max < 0.05
            }
            Some(_) => {
                return Err(malformed(&format!(
                    "the colour components of shadow {name} must be a list"
                )))
            }
        };
        if !grey {
            return Ok(false);
        }
    }
    Ok(true)
}

fn is_line_end(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// The text under the `## Rationale` (or `## Decisions and rationale`) headings of DESIGN.md.
///
/// `text.split(/^(?=## )/m)`, then of each part whose first line is
/// `/^## (?:Rationale|Decisions and rationale)\s*$/m` everything after that line.
fn rationale_body(text: &str) -> String {
    // A part starts at the start of the text and wherever "## " follows a line end.
    let mut starts = vec![0];
    let mut after_line_end = false;
    for (at, c) in text.char_indices() {
        if after_line_end && at > 0 && text[at..].starts_with("## ") {
            starts.push(at);
        }
        after_line_end = is_line_end(c);
    }
    let mut body = String::new();
    for (i, start) in starts.iter().enumerate() {
        let part = &text[*start..starts.get(i + 1).copied().unwrap_or(text.len())];
        let (first, rest) = part.split_once('\n').unwrap_or((part, ""));
        let after_title = ["## Rationale", "## Decisions and rationale"]
            .iter()
            .filter_map(|title| first.strip_prefix(title))
            .find(|tail| {
                // `\s*$` with the m flag: only white space up to the end, or up to a line end
                // (a lone "\r" is one, and is white space itself).
                let space = tail.len() - tail.trim_start_matches(js::is_space).len();
                space == tail.len() || tail[..space].contains(is_line_end)
            });
        if after_title.is_some() {
            body.push_str(rest);
        }
    }
    body
}

/// `(body.match(/^\s*[-*] .{20,}/gm) || []).length`: bullets of at least 20 characters (UTF-16
/// units, as JavaScript counts them) that start a line, blank lines before them allowed.
fn count_decisions(body: &str) -> usize {
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let mut count = 0;
    // True while only white space was seen since the start of the text or a line end.
    let mut at_line_start = true;
    let mut i = 0;
    while i < chars.len() {
        let (at, c) = chars[i];
        if at_line_start && matches!(c, '-' | '*') && body[at + 1..].starts_with(' ') {
            let line = body[at + 2..].split(is_line_end).next().unwrap_or_default();
            if js::len16(line) >= 20 {
                count += 1;
                // The match runs to the end of the line.
                let end = at + 2 + line.len();
                while i < chars.len() && chars[i].0 < end {
                    i += 1;
                }
                at_line_start = false;
                continue;
            }
        }
        if is_line_end(c) {
            at_line_start = true;
        } else if !js::is_space(c) {
            at_line_start = false;
        }
        i += 1;
    }
    count
}

/// `auditDesignSystem(root)`: findings `{ level, rule, file, line, message, source: "dfw-ds" }`.
/// Nothing when the project has no `design/tokens/tokens.json`.
// Over 120 lines: the checks run in the order their findings are printed and share the
// resolved tokens.
#[allow(clippy::too_many_lines, reason = "A10 debt")]
pub fn audit_design_system(root: &Path) -> Result<Vec<Value>, DfwError> {
    let p = project_paths(root);
    let file = p.tokens.join("tokens.json");
    let mut findings = Vec::new();
    if !file.exists() {
        return Ok(findings);
    }
    let mut add = |level: &str, rule: &str, message: String| {
        findings.push(json!({ "level": level, "rule": rule, "file": TOKENS_FILE, "line": 1, "message": message, "source": "dfw-ds" }));
    };
    let tree = read_json(&file, Some(json!({})))?;
    let tokens = collect(&tree);
    let r = Resolver { tokens: &tokens };

    let mut roles = Map::new();
    for role in ROLES {
        if let Some(hex) = r.hex(&format!("color.{role}")) {
            roles.insert(role.to_string(), hex);
        }
    }
    let mut status = Map::new();
    for s in ["ok", "info", "warn", "err"] {
        for suffix in ["", "-tint", "-text"] {
            if let Some(hex) = r.hex(&format!("color.status.{s}{suffix}")) {
                status.insert(format!("{s}{suffix}"), hex);
            }
        }
    }
    let exceptions: Vec<String> = tree
        .get("$extensions")
        .and_then(|e| e.get("dfw"))
        .and_then(|d| d.get("exceptions"))
        .and_then(Value::as_array)
        .map(|list| list.iter().map(|x| js::to_string(Some(x))).collect())
        .unwrap_or_default();
    let on_fill = r.hex("color.on-fill").unwrap_or_else(|| json!("#ffffff"));
    if ["accent", "surface", "ink"]
        .iter()
        .all(|k| roles.contains_key(*k))
    {
        let input = json!({ "roles": roles, "status": status, "onFill": on_fill });
        for x in audit_colors(&input, &exceptions) {
            add(
                x["level"].as_str().unwrap_or_default(),
                x["rule"].as_str().unwrap_or_default(),
                js::to_string(x.get("message")),
            );
        }
    }

    // Usage rules: every palette entry and role should say what it is for.
    let color_tokens: Vec<&Token> = tokens
        .iter()
        .filter(|t| {
            t.kind.and_then(Value::as_str) == Some("color") && !t.name.starts_with("color.status.")
        })
        .collect();
    let undocumented: Vec<&str> = color_tokens
        .iter()
        .filter(|t| !t.described)
        .map(|t| t.name.as_str())
        .collect();
    if undocumented.len() * 3 > color_tokens.len() {
        add("warn", "usage-rules", format!("{} of {} colour tokens have no $description (usage rule), e.g. {}. A senior palette says what each colour is for and what it must never be used for.", undocumented.len(), color_tokens.len(), undocumented.iter().take(4).copied().collect::<Vec<_>>().join(", ")));
    }

    // Type.
    let sizes = |keys: &[&str]| -> Vec<Value> {
        keys.iter()
            .filter_map(|k| r.size(k))
            .filter(|v| js::truthy(Some(v)))
            .cloned()
            .collect()
    };
    let ui = sizes(&["2xs", "xs", "sm", "md", "lg", "xl", "2xl"]);
    let display = sizes(&["display-sm", "display-md", "display-lg", "display-xl"]);
    // A malformed design.json is reported by lint (manifest-json).
    let manifest = read_json(&p.manifest, Some(json!({}))).unwrap_or_else(|_| json!({}));
    if manifest.is_null() {
        // A design.json that holds `null` (lint reports it as manifest-missing).
        return Err(DfwError::new(
            "design/design.json holds null instead of the design manifest. Write the manifest there (\"dfw lint\" says what it needs) and run the command again.",
        ));
    }
    let body = r
        .size("md")
        .filter(|v| js::truthy(Some(v)))
        .map_or(16.0, |v| js::to_number(Some(v)));
    let surface = manifest
        .get("surface")
        .filter(|s| js::truthy(Some(s)))
        .map_or("product".to_string(), |s| js::to_string(Some(s)));
    for x in audit_type_scale(&json!({ "ui": ui, "display": display }), body, &surface) {
        add(
            x["level"].as_str().unwrap_or_default(),
            x["rule"].as_str().unwrap_or_default(),
            js::to_string(x.get("message")),
        );
    }
    if display.is_empty() {
        add("warn", "type-display", "No display sizes (font.size.display-*): titles and key numbers will sit at body scale, the most common \"AI look\".".into());
    }
    let families = distinct(
        tokens
            .iter()
            .filter(|t| t.kind.and_then(Value::as_str) == Some("fontFamily"))
            .map(|t| match t.value {
                Some(Value::Array(list)) => list.first(),
                other => other,
            }),
    );
    if families.len() > 3 {
        let names: Vec<String> = families
            .iter()
            .map(|f| match f {
                None | Some(Value::Null) => String::new(),
                some => js::to_string(*some),
            })
            .collect();
        add(
            "warn",
            "type-families",
            format!(
                "{} font families ({}); 2 (+ mono) is the ceiling.",
                families.len(),
                names.join(", ")
            ),
        );
    }

    // Shape and elevation.
    let radii = distinct(
        tokens
            .iter()
            .filter(|t| t.name.starts_with("radius.") && !t.name.ends_with("pill"))
            .map(inner_value),
    );
    if radii.len() > 5 {
        add(
            "warn",
            "radius-sprawl",
            format!(
                "{} radius values; 3-4 steps keep shapes consistent.",
                radii.len()
            ),
        );
    }
    // `roles.canvas ? hexToOklch(roles.canvas).c : 0`: a colour object whose hex is not text
    // is a TypeError there.
    let canvas_chroma = match roles.get("canvas") {
        Some(Value::String(hex)) => hex_to_oklch(hex).c,
        Some(_) => return Err(malformed("the hex of color.canvas must be text")),
        None => 0.0,
    };
    for shadow in tokens
        .iter()
        .filter(|t| t.kind.and_then(Value::as_str) == Some("shadow"))
    {
        if is_grey_shadow(&shadow.name, shadow.value)? && canvas_chroma > 0.008 {
            add("warn", "shadow-tint", format!("{} uses a neutral black shadow on a tinted canvas; tint shadows with the darkest neutral so elevation belongs to the palette.", shadow.name));
            break;
        }
    }
    // Spacing grid.
    let off: Vec<String> = tokens
        .iter()
        .filter(|t| {
            t.name.starts_with("space.")
                && t.value.and_then(|v| v.get("unit")).and_then(Value::as_str) == Some("px")
        })
        .filter_map(|t| {
            let value = inner_value(t)?;
            let n = js::to_number(Some(value));
            let six = value.as_f64() == Some(6.0);
            (n > 2.0 && n % 4.0 != 0.0 && !six)
                .then(|| format!("{}={}", t.name, js::to_string(Some(value))))
        })
        .collect();
    if !off.is_empty() {
        add(
            "warn",
            "space-grid",
            format!("Spacing off the 4px grid: {}.", off.join(", ")),
        );
    }

    // Rationale in DESIGN.md.
    let design = root.join("DESIGN.md");
    let text = if design.exists() {
        js::read_text(&design)?
    } else {
        String::new()
    };
    let bullets = count_decisions(&rationale_body(&text));
    if bullets < 5 {
        findings.push(json!({ "level": "warn", "rule": "rationale", "file": "DESIGN.md", "line": 1, "message": format!("DESIGN.md \"## Rationale\" has {bullets} decision(s); write at least 5 \"decision - because <context fact or principle>\" lines (accent choice, neutrals, type scale, density, motif)."), "source": "dfw-ds" }));
    }
    findings.extend(inventory_findings(root)?);
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::fs;

    fn write(root: &Path, rel: &str, text: &str) {
        let file = root.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn rules(findings: &[Value]) -> Vec<&str> {
        findings
            .iter()
            .map(|f| f["rule"].as_str().unwrap())
            .collect()
    }

    /// 5: 3-digit hex tokens are audited and do not crash the Design System page
    /// (tests/unit/audit-fixes.test.mjs; the Design System page half belongs to design/system_page)
    #[test]
    fn three_digit_hex_tokens_are_audited() {
        let count = |ink: &str, bg: &str, accent: &str, muted: &str| {
            let root = test_dir("dsaudit-hex");
            let tokens = json!({ "color": { "$type": "color", "ink": { "$value": ink }, "surface": { "$value": bg }, "canvas": { "$value": bg }, "accent": { "$value": accent }, "muted": { "$value": muted } } });
            write(&root, "design/tokens/tokens.json", &tokens.to_string());
            let findings = audit_design_system(&root).unwrap();
            rules(&findings)
                .iter()
                .filter(|r| **r == "contrast")
                .count()
        };
        let long = count("#cccccc", "#ffffff", "#dddddd", "#eeeeee");
        assert!(long > 0);
        assert_eq!(count("#ccc", "#fff", "#ddd", "#eee"), long);
    }

    /// lint: a malformed design.json is a finding, not a crash (tests/unit/audit-fixes.test.mjs,
    /// the audit half)
    #[test]
    fn a_malformed_manifest_does_not_stop_the_audit() {
        let root = test_dir("dsaudit-manifest");
        write(&root, "design/design.json", "{\"screens\": [");
        write(
            &root,
            "design/tokens/tokens.json",
            r##"{"color":{"$type":"color","ink":{"$value":"#111111"}}}"##,
        );
        assert!(audit_design_system(&root).is_ok());
    }

    #[test]
    fn no_tokens_file_means_no_findings() {
        let root = test_dir("dsaudit-empty");
        assert_eq!(audit_design_system(&root).unwrap(), Vec::<Value>::new());
    }

    #[test]
    fn aliases_resolve_and_loops_end() {
        let tree = json!({ "color": { "$type": "color", "a": { "$value": "{color.b}" }, "b": { "$value": "#1234567890" }, "x": { "$value": "{color.y}" }, "y": { "$value": "{color.x}" }, "o": { "$value": { "hex": "#abcdef" } } } });
        let tokens = collect(&tree);
        let r = Resolver { tokens: &tokens };
        assert_eq!(r.hex("color.a"), Some(json!("#123456")));
        assert_eq!(r.hex("color.x"), None);
        assert_eq!(r.hex("color.o"), Some(json!("#abcdef")));
        assert_eq!(r.hex("color.nope"), None);
        assert_eq!(alias("{a}"), Some("a"));
        assert_eq!(alias("{}"), None);
        assert_eq!(alias("{a}b}"), None);
    }

    #[test]
    fn rationale_is_read_as_the_node_expressions_read_it() {
        let text = "# Design\n## Rationale\n- Accent is signal blue because of the floor.\n\n  * Neutrals are cool steel under fluorescent light.\n- short\ntext - not a bullet at all, even when long enough\n## Other\n- Not counted: this bullet is under another heading.\n## Decisions and rationale  \r\n- Counted again: a second heading adds to the first.\n";
        let body = rationale_body(text);
        assert!(body.starts_with("- Accent"));
        assert!(!body.contains("Not counted"));
        assert_eq!(count_decisions(&body), 3);
        // A title with more text is another heading; 20 is counted in UTF-16 units.
        assert_eq!(rationale_body("## Rationale and more\n- x\n"), "");
        assert_eq!(count_decisions("- 12345678901234567890"), 1);
        assert_eq!(count_decisions("- 1234567890123456789"), 0);
        assert_eq!(count_decisions("- 🚀🚀🚀🚀🚀🚀🚀🚀🚀🚀"), 1);
        assert_eq!(count_decisions("x - 12345678901234567890"), 0);
        assert_eq!(
            count_decisions("\n\n \t- 12345678901234567890\r* 12345678901234567890"),
            2
        );
    }

    #[test]
    fn spacing_radius_families_shadow_and_usage_rules() {
        let root = test_dir("dsaudit-rules");
        let dim = |v: f64| json!({ "$value": { "value": v, "unit": "px" } });
        let tokens = json!({
            "color": { "$type": "color", "canvas": { "$value": "#f4ede0" }, "a": { "$value": "#111111" }, "b": { "$value": "#222222", "$description": "ink" } },
            "space": { "$type": "dimension", "1": dim(4.0), "2": dim(6.0), "3": dim(10.0), "4": dim(2.0) },
            "radius": { "$type": "dimension", "a": dim(1.0), "b": dim(2.0), "c": dim(3.0), "d": dim(4.0), "e": dim(5.0), "f": dim(6.0), "pill": dim(999.0) },
            "font": { "$type": "fontFamily", "a": { "$value": ["A", "x"] }, "b": { "$value": "B" }, "c": { "$value": "C" }, "d": { "$value": "D" }, "e": { "$value": "A" } },
            "shadow": { "$type": "shadow", "sm": { "$value": { "color": { "components": [0, 0, 0] } } } }
        });
        write(&root, "design/tokens/tokens.json", &tokens.to_string());
        let findings = audit_design_system(&root).unwrap();
        let message = |rule: &str| {
            findings
                .iter()
                .find(|f| f["rule"] == rule)
                .map(|f| f["message"].as_str().unwrap().to_string())
                .unwrap_or_else(|| panic!("no {rule} in {:?}", rules(&findings)))
        };
        assert_eq!(
            message("space-grid"),
            "Spacing off the 4px grid: space.3=10."
        );
        assert!(message("radius-sprawl").starts_with("6 radius values"));
        assert!(message("type-families").starts_with("4 font families (A, B, C, D)"));
        assert!(message("shadow-tint").starts_with("shadow.sm uses"));
        assert!(message("usage-rules").starts_with(
            "2 of 3 colour tokens have no $description (usage rule), e.g. color.canvas, color.a."
        ));
        assert!(message("rationale").contains("has 0 decision(s)"));
        assert!(findings
            .iter()
            .all(|f| f["source"] == "dfw-ds" && f["line"] == 1));
    }
    /// A token value of the wrong type ends the audit with an error that names the file.
    #[test]
    fn token_values_of_the_wrong_type_are_an_error_where_node_dies() {
        let audit = |tree: Value| {
            let root = test_dir("dsaudit-shapes");
            write(&root, "design/tokens/tokens.json", &tree.to_string());
            audit_design_system(&root)
        };
        let shadow = |value: Value| json!({ "color": { "$type": "color", "canvas": { "$value": "#f4ede0" } }, "shadow": { "$type": "shadow", "a": { "$value": value } } });
        let tinted = |value: Value| rules(&audit(shadow(value)).unwrap()).contains(&"shadow-tint");
        assert!(tinted(json!([])));
        assert!(tinted(json!({ "color": { "components": [] } })));
        assert!(tinted(json!({ "color": { "components": "000" } })));
        assert!(tinted(
            json!({ "color": { "components": [null, false, "0.01"] } })
        ));
        assert!(!tinted(json!({ "color": { "components": "abc" } })));
        assert!(!tinted(json!({ "color": { "components": "" } })));
        assert!(!tinted(json!({ "color": null })));
        assert!(!tinted(json!(7)));
        // A layer that is not grey ends the look at this shadow: the null after it is not read.
        assert!(!tinted(
            json!([{ "color": { "components": [1, 0, 0] } }, null])
        ));
        for dies in [
            json!(null),
            json!([{ "color": { "components": [0, 0, 0] } }, null]),
            json!({ "color": { "components": 5 } }),
            json!({ "color": { "components": {} } }),
        ] {
            assert!(audit(shadow(dies.clone())).is_err(), "{dies}");
        }
        let canvas = |value: Value| {
            audit(json!({ "color": { "$type": "color", "canvas": { "$value": value } } }))
        };
        assert!(canvas(json!({ "hex": 5 })).is_err());
        assert!(canvas(json!({ "hex": "" })).is_ok());
        assert!(canvas(json!(5)).is_ok());
        // design.json that holds null.
        let root = test_dir("dsaudit-null-manifest");
        write(&root, "design/tokens/tokens.json", "{}");
        write(&root, "design/design.json", "null");
        assert_eq!(
            audit_design_system(&root).unwrap_err().message(),
            "design/design.json holds null instead of the design manifest. Write the manifest there (\"dfw lint\" says what it needs) and run the command again."
        );
    }
}
