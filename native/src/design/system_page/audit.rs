//! A private copy of `auditDesignSystem` (`core/lib/dsaudit.mjs`) for the "Senior review"
//! section, until `design::dsaudit` is ported by its group.

use std::collections::HashMap;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::text::{alias, bullets, has_heading, hex_field, md_parts, part_body, slice_utf16};
use crate::core::error::DfwError;
use crate::core::paths::project_paths;
use crate::design::color::{audit_colors, audit_type_scale, hex_to_oklch};
use crate::design::edit::{collect_tokens, TokenEntry};
use crate::design::inventory::inventory_findings;
use crate::design::tokens::js;

/// `auditDesignSystem` of `core/lib/dsaudit.mjs`, for the "Senior review" section: the
/// findings as `{level, message}` in the order the Node code reports them.
// TODO(merge): use design::dsaudit::audit_design_system (another group ports that module).
// Over 120 lines: one function as in dsaudit.mjs, the checks in the order their findings print.
pub(super) fn audit_design_system(root: &Path) -> Result<Vec<Value>, DfwError> {
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
    let p = project_paths(root);
    let file = p.tokens.join("tokens.json");
    let mut findings: Vec<Value> = Vec::new();
    if !file.exists() {
        return Ok(findings);
    }
    let mut add = |level: &str, message: String| {
        findings.push(json!({ "level": level, "message": message }));
    };
    let tree = js::read_json(&file, Some(json!({})))?;
    // A `Map` in the Node code: a repeated name keeps its first place and takes the later token.
    let mut list: Vec<TokenEntry> = Vec::new();
    for token in collect_tokens(&tree) {
        match list.iter_mut().find(|t| t.name == token.name) {
            Some(slot) => *slot = token,
            None => list.push(token),
        }
    }
    let by_name: HashMap<&str, &TokenEntry> = list.iter().map(|t| (t.name.as_str(), t)).collect();
    pub(super) fn get<'a>(
        by_name: &HashMap<&str, &'a TokenEntry>,
        name: &str,
        depth: usize,
    ) -> Option<&'a TokenEntry> {
        let t = by_name.get(name).copied().filter(|_| depth <= 10)?;
        match alias(&t.value) {
            Some(inner) => get(by_name, inner, depth + 1),
            None => Some(t),
        }
    }
    let hex = |name: &str| -> Option<String> {
        let t = get(&by_name, name, 0)?;
        match &t.value {
            Value::String(s) => s.starts_with('#').then(|| slice_utf16(s, 7)),
            other => hex_field(other),
        }
    };
    let mut roles = Map::new();
    for k in ROLES {
        if let Some(h) = hex(&format!("color.{k}")).filter(|h| !h.is_empty()) {
            roles.insert(k.to_string(), json!(h));
        }
    }
    let mut status = Map::new();
    for s in ["ok", "info", "warn", "err"] {
        for suffix in ["", "-tint", "-text"] {
            if let Some(h) = hex(&format!("color.status.{s}{suffix}")).filter(|h| !h.is_empty()) {
                status.insert(format!("{s}{suffix}"), json!(h));
            }
        }
    }
    let exceptions: Vec<String> = tree
        .get("$extensions")
        .and_then(|e| e.get("dfw"))
        .and_then(|d| d.get("exceptions"))
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let on_fill = hex("color.on-fill")
        .filter(|h| !h.is_empty())
        .unwrap_or_else(|| "#ffffff".to_string());
    let canvas = roles
        .get("canvas")
        .and_then(Value::as_str)
        .map(str::to_string);
    if ["accent", "surface", "ink"]
        .iter()
        .all(|k| roles.contains_key(*k))
    {
        let input = json!({ "roles": roles, "status": status, "onFill": on_fill });
        for x in audit_colors(&input, &exceptions) {
            add(
                &js::to_string(x.get("level")),
                js::to_string(x.get("message")),
            );
        }
    }

    // Usage rules: every palette entry and role should say what it is for.
    let color_tokens: Vec<&TokenEntry> = list
        .iter()
        .filter(|t| t.is_type("color") && !t.name.starts_with("color.status."))
        .collect();
    let undocumented: Vec<&str> = color_tokens
        .iter()
        .filter(|t| t.description.is_none())
        .map(|t| t.name.as_str())
        .collect();
    if undocumented.len() as f64 > color_tokens.len() as f64 / 3.0 {
        let shown: Vec<&str> = undocumented.iter().take(4).copied().collect();
        add("warn", format!("{} of {} colour tokens have no $description (usage rule), e.g. {}. A senior palette says what each colour is for and what it must never be used for.", undocumented.len(), color_tokens.len(), shown.join(", ")));
    }

    // Type.
    let size = |k: &str| -> Option<&Value> {
        js::present(get(&by_name, &format!("font.size.{k}"), 0).and_then(|t| t.value.get("value")))
    };
    let sizes = |keys: &[&str]| -> Value {
        Value::Array(
            keys.iter()
                .filter_map(|k| size(k).filter(|v| js::truthy(Some(v))).cloned())
                .collect(),
        )
    };
    let ui = sizes(&["2xs", "xs", "sm", "md", "lg", "xl", "2xl"]);
    let display = sizes(&["display-sm", "display-md", "display-lg", "display-xl"]);
    let has_display = display.as_array().is_some_and(|d| !d.is_empty());
    // A malformed design.json is reported by lint (manifest-json).
    let manifest = js::read_json(&p.manifest, Some(json!({}))).unwrap_or_else(|_| json!({}));
    let body = size("md")
        .filter(|v| js::truthy(Some(v)))
        .map_or(16.0, |v| js::to_number(Some(v)));
    let surface = manifest.get("surface").filter(|v| js::truthy(Some(v)));
    let surface = surface.map_or("product".to_string(), |v| js::to_string(Some(v)));
    for x in audit_type_scale(&json!({ "ui": ui, "display": display }), body, &surface) {
        add(
            &js::to_string(x.get("level")),
            js::to_string(x.get("message")),
        );
    }
    if !has_display {
        add("warn", "No display sizes (font.size.display-*): titles and key numbers will sit at body scale, the most common \"AI look\".".to_string());
    }
    // A `Set` in the Node code: numbers are the same whatever way they were written.
    let same = |a: &Option<Value>, b: &Option<Value>| match (a, b) {
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        _ => a == b,
    };
    let unique = |values: Vec<Option<Value>>| {
        let mut out: Vec<Option<Value>> = Vec::new();
        for v in values {
            if !out.iter().any(|o| same(o, &v)) {
                out.push(v);
            }
        }
        out
    };
    let families = unique(
        list.iter()
            .filter(|t| t.is_type("fontFamily"))
            .map(|t| match &t.value {
                Value::Array(items) => items.first().cloned(),
                other => Some(other.clone()),
            })
            .collect(),
    );
    if families.len() > 3 {
        let names: Vec<Value> = families
            .iter()
            .map(|f| f.clone().unwrap_or(Value::Null))
            .collect();
        add(
            "warn",
            format!(
                "{} font families ({}); 2 (+ mono) is the ceiling.",
                families.len(),
                js::join(&names, ", ")
            ),
        );
    }

    // Shape and elevation.
    let radii = unique(
        list.iter()
            .filter(|t| t.name.starts_with("radius.") && !t.name.ends_with("pill"))
            .map(|t| t.value.get("value").cloned())
            .collect(),
    );
    if radii.len() > 5 {
        add(
            "warn",
            format!(
                "{} radius values; 3-4 steps keep shapes consistent.",
                radii.len()
            ),
        );
    }
    let canvas_c = canvas.map_or(0.0, |c| hex_to_oklch(&c).c);
    for t in list.iter().filter(|t| t.is_type("shadow")) {
        let layers: Vec<&Value> = match &t.value {
            Value::Array(items) => items.iter().collect(),
            other => vec![other],
        };
        let grey = layers.iter().all(|l| {
            let Some(c) = l
                .get("color")
                .and_then(|c| c.get("components"))
                .and_then(Value::as_array)
            else {
                return false;
            };
            let nums: Vec<f64> = c.iter().map(|v| js::to_number(Some(v))).collect();
            let max = nums.iter().copied().fold(f64::NEG_INFINITY, js::max);
            let min = nums.iter().copied().fold(f64::INFINITY, js::min);
            max - min < 0.02 && max < 0.05
        });
        if grey && canvas_c > 0.008 {
            add("warn", format!("{} uses a neutral black shadow on a tinted canvas; tint shadows with the darkest neutral so elevation belongs to the palette.", t.name));
            break;
        }
    }
    // Spacing grid.
    let off: Vec<String> = list
        .iter()
        .filter(|t| t.name.starts_with("space."))
        .filter_map(|t| {
            let amount = t.value.get("value");
            let n = js::to_number(amount);
            let is_six = amount.and_then(Value::as_f64) == Some(6.0);
            let px = t.value.get("unit").and_then(Value::as_str) == Some("px");
            (px && n > 2.0 && n % 4.0 != 0.0 && !is_six)
                .then(|| format!("{}={}", t.name, js::to_string(amount)))
        })
        .collect();
    if !off.is_empty() {
        add(
            "warn",
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
    let mut body = String::new();
    for part in md_parts(&text) {
        if has_heading(part, &["Rationale", "Decisions and rationale"]) {
            body.push_str(part_body(part));
        }
    }
    let count = bullets(&body, 20).len();
    if count < 5 {
        add("warn", format!("DESIGN.md \"## Rationale\" has {count} decision(s); write at least 5 \"decision - because <context fact or principle>\" lines (accent choice, neutrals, type scale, density, motif)."));
    }
    findings.extend(inventory_findings(root)?);
    Ok(findings)
}
