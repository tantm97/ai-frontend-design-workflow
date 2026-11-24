//! Ports `core/lib/icons.mjs`: icon search and copying into `design/system/icons/`.
//!
//! The library is Lucide (ISC), vendored at a pinned tag in `assets/icons/lucide.json`. Projects
//! copy only the icons they use, so designs stay self-contained.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::assets::{self, Folder};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, project_paths};
use crate::core::util::write_atomic_inside;

/// `iconBundle`: the parsed icon library `{ svgRoot, icons: { name: [paths, ...tags] } }`.
pub fn icon_bundle() -> Result<Value, DfwError> {
    assets::json(Folder::Assets, "icons/lucide.json")?.ok_or_else(|| {
        DfwError::new("Icon library missing (assets/icons/lucide.json). Reinstall dfw.")
    })
}

/// `iconSvg`: the SVG file text of one icon, or `None` when the library has no such icon.
pub fn icon_svg(bundle: &Value, name: &str) -> Option<String> {
    let entry = bundle.get("icons")?.as_object()?.get(name)?;
    Some(format!(
        "{}{}</svg>\n",
        js::to_string(bundle.get("svgRoot")),
        js::to_string(entry.get(0))
    ))
}

/// One result of [`search_icons`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconHit {
    pub name: String,
    pub score: u32,
    /// The first six tags.
    pub tags: Vec<String>,
}

/// `array.slice(0, n)` for a length: a negative `n` counts from the end, a fraction is cut.
pub fn slice_end(len: usize, n: f64) -> usize {
    if n.is_nan() {
        0
    } else if n < 0.0 {
        len.saturating_sub((-n.trunc()) as usize)
    } else {
        len.min(n.trunc() as usize)
    }
}

/// `searchIcons`: icons ranked by name and tag match, at most `limit`.
pub fn search_icons(bundle: &Value, query: &str, limit: f64) -> Vec<IconHit> {
    let words = js::words(&query.to_lowercase());
    let joined = words.join("-");
    let mut scored = Vec::new();
    let Some(icons) = bundle.get("icons").and_then(Value::as_object) else {
        return scored;
    };
    for (name, entry) in js::entries(icons) {
        let tags: Vec<String> = entry
            .as_array()
            .map(|e| e.iter().skip(1).map(|t| js::to_string(Some(t))).collect())
            .unwrap_or_default();
        let mut score = if *name == joined { 20 } else { 0 };
        for w in &words {
            if name == w {
                score += 10;
            } else if name.split('-').any(|part| part == w) {
                score += 6;
            } else if name.contains(w.as_str()) {
                score += 3;
            }
            if tags.contains(w) {
                score += 4;
            } else if tags.iter().any(|t| t.contains(w.as_str())) {
                score += 1;
            }
        }
        if score > 0 {
            scored.push(IconHit {
                name: name.clone(),
                score,
                tags: tags.into_iter().take(6).collect(),
            });
        }
    }
    // `a.name.localeCompare(b.name)`: icon names are lower-case ASCII words joined by "-", which
    // collate as their bytes do.
    scored.sort_by(|a, b| b.score.cmp(&a.score).then_with(|| a.name.cmp(&b.name)));
    scored.truncate(slice_end(scored.len(), limit));
    scored
}

/// What [`add_icons`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedIcons {
    pub added: Vec<String>,
    pub missing: Vec<String>,
    /// For each missing name, up to five icons with a similar name.
    pub suggestions: Vec<(String, Vec<String>)>,
}

/// `addIcons`: copy icons from the library into `design/system/icons/`.
pub fn add_icons(root: &Path, names: &[String]) -> Result<AddedIcons, DfwError> {
    let dir = project_paths(root).icons;
    fs::create_dir_all(&dir)?;
    let bundle = icon_bundle()?;
    let mut added = Vec::new();
    let mut missing = Vec::new();
    for n in names {
        match icon_svg(&bundle, n) {
            Some(svg) => {
                write_atomic_inside(root, &dir.join(file::icon(n)), svg)?;
                added.push(n.clone());
            }
            None => missing.push(n.clone()),
        }
    }
    let mut suggestions: Vec<(String, Vec<String>)> = Vec::new();
    for m in &missing {
        let names = search_icons(&bundle, m, 5.0)
            .into_iter()
            .map(|hit| hit.name)
            .collect();
        match suggestions.iter_mut().find(|(k, _)| k == m) {
            Some(slot) => slot.1 = names,
            None => suggestions.push((m.clone(), names)),
        }
    }
    Ok(AddedIcons {
        added,
        missing,
        suggestions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::paths::rel;
    use crate::core::util::test_dir;

    /// icons: search ranks exact names, add copies svg and suggests on miss
    #[test]
    fn search_ranks_exact_names_and_add_copies_svg() {
        let root = test_dir("icons");
        let bundle = icon_bundle().unwrap();
        assert_eq!(
            search_icons(&bundle, "scan barcode", 1.0)[0].name,
            "scan-barcode"
        );
        /// An icon of the bundle, and a misspelling of it.
        const ICON: &str = "search";
        const TYPO: &str = "serch";
        let r = add_icons(&root, &[ICON.to_string(), TYPO.to_string()]).unwrap();
        assert_eq!(r.added, [ICON]);
        assert_eq!(r.missing, [TYPO]);
        assert_eq!(r.suggestions[0].0, TYPO);
        let svg = fs::read_to_string(root.join(rel::join(rel::ICONS, &file::icon(ICON)))).unwrap();
        assert!(
            svg.starts_with("<svg")
                && svg[..svg.find('>').unwrap()].contains("viewBox=\"0 0 24 24\""),
            "{svg}"
        );
    }

    /// j: names that are prototype keys are not ids (tests/unit/node-stable.test.mjs; icons)
    #[test]
    fn prototype_names_are_not_icons() {
        let root = test_dir("icons-proto");
        let bundle = icon_bundle().unwrap();
        /// A prototype key of a JavaScript object.
        const PROTO: &str = "constructor";
        assert_eq!(icon_svg(&bundle, PROTO), None);
        let r = add_icons(&root, &[PROTO.to_string(), "toString".to_string()]).unwrap();
        assert!(r.added.is_empty(), "not icons");
        assert!(!root
            .join(rel::join(rel::ICONS, &file::icon(PROTO)))
            .exists());
    }

    #[test]
    fn limits_follow_array_slice() {
        assert_eq!(slice_end(10, 3.9), 3);
        assert_eq!(slice_end(10, -2.0), 8);
        assert_eq!(slice_end(2, 20.0), 2);
        assert_eq!(slice_end(2, f64::NAN), 0);
    }
}
