//! `lintManifest` of lint.mjs: design/design.json.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{finding, is_private, item, list, malformed};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::core::util::read_json;

/// `Set.prototype.has` for values read from JSON: text, numbers, booleans and null are equal
/// by value, an object or an array only to itself (so never to another entry).
fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Object(_) | Value::Array(_), _) => false,
        _ => a == b,
    }
}

/// `path.join(dir, file)`: an absolute `file` is still joined below `dir`.
fn join_below(dir: &Path, file: &str) -> PathBuf {
    dir.join(file.trim_start_matches(['/', '\\']))
}

/// `lintManifest(root)`: design/design.json. A value of a type the Node code died on (a
/// TypeError) is an error here too.
// Over 120 lines with its messages, one function as in lint.mjs: the rules run in the order
// their findings are printed.
pub fn lint_manifest(root: &Path) -> Result<Vec<Value>, DfwError> {
    const REL: &str = "design/design.json";
    let p = project_paths(root);
    let mut out = Vec::new();
    let mut add =
        |level: &str, rule: &str, message: String| out.push(finding(level, rule, REL, 1, message));
    let manifest = match read_json(&p.manifest, Some(Value::Null)) {
        Ok(manifest) => manifest,
        Err(err) => {
            add("error", "manifest-json", err.message().to_string());
            return Ok(out);
        }
    };
    if !js::truthy(Some(&manifest)) {
        add(
            "error",
            "manifest-missing",
            "design/design.json is missing.".into(),
        );
        return Ok(out);
    }
    let screens = list(manifest.get("screens"), "\"screens\"")?;
    for screen in screens {
        item(screen, "a screen")?;
    }
    if screens.is_empty() {
        add(
            "warn",
            "manifest-screens",
            "Manifest has no screens yet (fine during the system phase).".into(),
        );
    }
    let id = |v: &Value| js::to_string(v.get("id"));
    for screen in screens {
        if is_private(screen) {
            continue;
        }
        let Some(blueprint) = screen.get("blueprint").filter(|b| js::truthy(Some(b))) else {
            add("warn", "blueprint", format!("Screen \"{}\" has no blueprint (goal, layout, aboveFold, data, feedback). Agree it with the user before building: it is cheaper to fix a blueprint than a screen.", id(screen)));
            continue;
        };
        for key in ["goal", "layout", "aboveFold", "data", "feedback"] {
            let value = blueprint.get(key);
            if !js::truthy(value) || value.and_then(Value::as_array).is_some_and(Vec::is_empty) {
                add(
                    "warn",
                    "blueprint",
                    format!("Screen \"{}\" blueprint is missing \"{key}\".", id(screen)),
                );
            }
        }
    }
    // `for (const pg of m.pages || [])`: a text is iterated letter by letter (no letter has
    // screens), any other value that is not a list is a TypeError.
    let pages = match manifest.get("pages") {
        Some(Value::String(_)) => &[],
        other => list(other, "\"pages\"")?,
    };
    for page in pages {
        // Of a text, each character is taken for a screen id.
        let letters: Vec<Value>;
        let listed = match item(page, "a page")?.get("screens") {
            Some(Value::String(text)) => {
                letters = text.chars().map(|c| Value::String(c.into())).collect();
                letters.as_slice()
            }
            other => list(other, "\"screens\" of a page")?,
        };
        for sid in listed {
            let known = screens
                .iter()
                .any(|s| s.get("id").is_some_and(|have| same_value(have, sid)));
            if !known {
                add(
                    "error",
                    "manifest-page-screen",
                    format!(
                        "Page \"{}\" lists unknown screen \"{}\".",
                        id(page),
                        js::to_string(Some(sid))
                    ),
                );
            }
        }
    }
    let mut seen: Vec<Option<&Value>> = Vec::new();
    for screen in screens {
        let sid = screen.get("id");
        if !js::truthy(sid) {
            add("error", "manifest-screen-id", "A screen has no id.".into());
        }
        let again = seen.iter().any(|have| match (have, sid) {
            (None, None) => true,
            (Some(a), Some(b)) => same_value(a, b),
            _ => false,
        });
        if again {
            add(
                "error",
                "manifest-screen-dup",
                format!("Duplicate screen id \"{}\".", id(screen)),
            );
        }
        seen.push(sid);
        let variants = screen
            .get("variants")
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice);
        if variants.is_empty() {
            add(
                "error",
                "manifest-variants",
                format!("Screen \"{}\" has no variants.", id(screen)),
            );
            continue;
        }
        for variant in variants {
            let file = item(variant, "a variant")?.get("file");
            let Some(file) = file.filter(|f| js::truthy(Some(f))) else {
                add(
                    "error",
                    "manifest-variant-file",
                    format!("Variant \"{}/{}\" has no file.", id(screen), id(variant)),
                );
                continue;
            };
            let Some(file) = file.as_str() else {
                return Err(malformed("\"file\" of a variant must be text"));
            };
            if !join_below(&p.design, file).exists() {
                add(
                    "error",
                    "manifest-file-missing",
                    format!(
                        "Variant \"{}/{}\" points to missing file design/{file}.",
                        id(screen),
                        id(variant)
                    ),
                );
            }
        }
    }
    Ok(out)
}
