//! `lintManifest` of lint.mjs: design/design.json.

use std::path::{Path, PathBuf};

use serde_json::Value;

use super::{finding, malformed};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::level::Level;
use crate::core::paths::project_paths;
use crate::core::util::read_json;
use crate::design::manifest::{items_checked as list, key, not_null as item, Screen};

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

/// The file every finding of this lint names.
const REL: &str = "design/design.json";

/// One finding on design/design.json, at line 1.
fn add(out: &mut Vec<Value>, level: Level, rule: &str, message: String) {
    out.push(finding(level.as_str(), rule, REL, 1, message));
}

/// The id of a screen, page or variant as text.
fn id(v: &Value) -> String {
    js::to_string(v.get(key::ID))
}

/// `lintManifest(root)`: design/design.json. A value of the wrong type ends the lint with an
/// error that names the field.
pub fn lint_manifest(root: &Path) -> Result<Vec<Value>, DfwError> {
    let p = project_paths(root);
    let mut out = Vec::new();
    let manifest = match read_json(&p.manifest, Some(Value::Null)) {
        Ok(manifest) => manifest,
        Err(err) => {
            add(
                &mut out,
                Level::Error,
                "manifest-json",
                err.message().to_string(),
            );
            return Ok(out);
        }
    };
    if !js::truthy(Some(&manifest)) {
        add(
            &mut out,
            Level::Error,
            "manifest-missing",
            "design/design.json is missing.".into(),
        );
        return Ok(out);
    }
    let screens = list(manifest.get(key::SCREENS), "\"screens\"")?;
    for screen in screens {
        item(screen, "a screen")?;
    }
    if screens.is_empty() {
        add(
            &mut out,
            Level::Warn,
            "manifest-screens",
            "Manifest has no screens yet (fine during the system phase).".into(),
        );
    }
    check_blueprints(screens, &mut out);
    check_page_screens(&manifest, screens, &mut out)?;
    check_screens(&p.design, screens, &mut out)?;
    Ok(out)
}

/// Every public screen has a blueprint with its five parts filled in.
fn check_blueprints(screens: &[Value], out: &mut Vec<Value>) {
    for screen in screens {
        if Screen(screen).is_private() {
            continue;
        }
        let Some(blueprint) = screen.get(key::BLUEPRINT).filter(|b| js::truthy(Some(b))) else {
            add(out, Level::Warn, "blueprint", format!("Screen \"{}\" has no blueprint (goal, layout, aboveFold, data, feedback). Agree it with the user before building: it is cheaper to fix a blueprint than a screen.", id(screen)));
            continue;
        };
        for key in ["goal", "layout", "aboveFold", "data", "feedback"] {
            let value = blueprint.get(key);
            if !js::truthy(value) || value.and_then(Value::as_array).is_some_and(Vec::is_empty) {
                add(
                    out,
                    Level::Warn,
                    "blueprint",
                    format!("Screen \"{}\" blueprint is missing \"{key}\".", id(screen)),
                );
            }
        }
    }
}

/// Every screen a page lists is a screen of the manifest.
fn check_page_screens(
    manifest: &Value,
    screens: &[Value],
    out: &mut Vec<Value>,
) -> Result<(), DfwError> {
    // `for (const pg of m.pages || [])`: a text is iterated letter by letter (no letter has
    // screens), any other value that is not a list is a TypeError.
    let pages = match manifest.get(key::PAGES) {
        Some(Value::String(_)) => &[],
        other => list(other, "\"pages\"")?,
    };
    for page in pages {
        // Of a text, each character is taken for a screen id.
        let letters: Vec<Value>;
        let listed = match item(page, "a page")?.get(key::SCREENS) {
            Some(Value::String(text)) => {
                letters = text.chars().map(|c| Value::String(c.into())).collect();
                letters.as_slice()
            }
            other => list(other, "\"screens\" of a page")?,
        };
        for sid in listed {
            let known = screens
                .iter()
                .any(|s| s.get(key::ID).is_some_and(|have| same_value(have, sid)));
            if !known {
                add(
                    out,
                    Level::Error,
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
    Ok(())
}

/// Every screen has a unique id and variants, and every variant a file that exists.
fn check_screens(design: &Path, screens: &[Value], out: &mut Vec<Value>) -> Result<(), DfwError> {
    let mut seen: Vec<Option<&Value>> = Vec::new();
    for screen in screens {
        let sid = screen.get(key::ID);
        if !js::truthy(sid) {
            add(
                out,
                Level::Error,
                "manifest-screen-id",
                "A screen has no id.".into(),
            );
        }
        let again = seen.iter().any(|have| match (have, sid) {
            (None, None) => true,
            (Some(a), Some(b)) => same_value(a, b),
            _ => false,
        });
        if again {
            add(
                out,
                Level::Error,
                "manifest-screen-dup",
                format!("Duplicate screen id \"{}\".", id(screen)),
            );
        }
        seen.push(sid);
        check_variants(design, screen, out)?;
    }
    Ok(())
}

/// A screen has variants, and each variant names a file that exists under design/.
fn check_variants(design: &Path, screen: &Value, out: &mut Vec<Value>) -> Result<(), DfwError> {
    let variants = screen
        .get(key::VARIANTS)
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    if variants.is_empty() {
        add(
            out,
            Level::Error,
            "manifest-variants",
            format!("Screen \"{}\" has no variants.", id(screen)),
        );
        return Ok(());
    }
    for variant in variants {
        let file = item(variant, "a variant")?.get(key::FILE);
        let Some(file) = file.filter(|f| js::truthy(Some(f))) else {
            add(
                out,
                Level::Error,
                "manifest-variant-file",
                format!("Variant \"{}/{}\" has no file.", id(screen), id(variant)),
            );
            continue;
        };
        let Some(file) = file.as_str() else {
            return Err(malformed("\"file\" of a variant must be text"));
        };
        if !join_below(design, file).exists() {
            add(
                out,
                Level::Error,
                "manifest-file-missing",
                format!(
                    "Variant \"{}/{}\" points to missing file design/{file}.",
                    id(screen),
                    id(variant)
                ),
            );
        }
    }
    Ok(())
}
