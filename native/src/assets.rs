//! Embedded files (design decision 6): every file of `core/` the binary reads, so it runs from
//! any folder without a plugin next to it.
//!
//! | Folder | What reads it |
//! |---|---|
//! | `core/studio` | the Studio and the review proxy (browser files, the Penpot plugin) |
//! | `core/templates` | `dfw init` (PRODUCT.md, DESIGN.md, the starter `design/`), the handoff |
//! | `core/kits` | presets, style guides and the starter kit's masters (`design::system`, `style`, `inventory`) |
//! | `core/assets` | the icon library (`design::icons`) |
//! | `core/workflow` | the guide the MCP server serves (`tools::mcp`), rendered for the host there |
//! | `core/manifest.json` | the plugin list `dfw install` installs when no plugin folder is found |
//!
//! When `DFW_PLUGIN_ROOT` names a folder that has the same sub-folder (`studio/`, `kits/`...),
//! the files there win, so a change needs no rebuild during development.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};
use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{env_nonempty, resolve};
use crate::core::util::{list_files, write_atomic};

// build.rs makes cargo rebuild when one of these folders changes.
static STUDIO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/studio");
static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/templates");
static KITS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/kits");
static ASSETS: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/assets");
static WORKFLOW: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/workflow");
static MANIFEST: &str = include_str!("../../core/manifest.json");

/// A folder of `core/` carried by the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Folder {
    Studio,
    Templates,
    Kits,
    Assets,
    Workflow,
}

impl Folder {
    /// The folder's name under `core/` and under a `DFW_PLUGIN_ROOT` override.
    fn name(self) -> &'static str {
        match self {
            Folder::Studio => "studio",
            Folder::Templates => "templates",
            Folder::Kits => "kits",
            Folder::Assets => "assets",
            Folder::Workflow => "workflow",
        }
    }

    fn embedded(self) -> &'static Dir<'static> {
        match self {
            Folder::Studio => &STUDIO,
            Folder::Templates => &TEMPLATES,
            Folder::Kits => &KITS,
            Folder::Assets => &ASSETS,
            Folder::Workflow => &WORKFLOW,
        }
    }

    /// `<DFW_PLUGIN_ROOT>/<name>` when that folder exists.
    fn override_dir(self) -> Option<PathBuf> {
        let dir = resolve(env_nonempty("DFW_PLUGIN_ROOT")?).join(self.name());
        dir.is_dir().then_some(dir)
    }
}

/// A clean relative path: `/`-separated normal segments only (the callers clamp `..` first; this
/// refuses anything else so an override folder is never left). `""` and `"."` are the folder.
fn clean(rel: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = rel
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let bad = parts
        .iter()
        .any(|p| *p == ".." || p.contains('\\') || p.contains(':') || p.contains('\0'));
    (!bad).then_some(parts)
}

/// A clean relative path that names something inside the folder (not the folder itself).
fn clean_file(rel: &str) -> Option<Vec<&str>> {
    clean(rel).filter(|parts| !parts.is_empty())
}

/// A file by its path relative to the folder (`app.html`, `base/presets/crisp.tokens.json`):
/// from the override folder when there is one, else embedded. `None` when it does not exist, is
/// a folder or the path leaves the folder.
pub fn file(folder: Folder, rel: &str) -> Option<Cow<'static, [u8]>> {
    let parts = clean_file(rel)?;
    if let Some(dir) = folder.override_dir() {
        let file = parts.iter().fold(dir, |acc, p| acc.join(p));
        return fs::read(&file).ok().map(Cow::Owned);
    }
    folder
        .embedded()
        .get_file(parts.join("/"))
        .map(|f| Cow::Borrowed(f.contents()))
}

/// [`file`] as text (invalid UTF-8 replaced, as `readFileSync(f, 'utf8')`).
pub fn text(folder: Folder, rel: &str) -> Option<String> {
    file(folder, rel).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// Whether [`file`] finds the file.
pub fn exists(folder: Folder, rel: &str) -> bool {
    let Some(parts) = clean_file(rel) else {
        return false;
    };
    match folder.override_dir() {
        Some(dir) => parts.iter().fold(dir, |acc, p| acc.join(p)).is_file(),
        None => folder.embedded().get_file(parts.join("/")).is_some(),
    }
}

/// A JSON file of the folder with its keys in JavaScript order (`readJson(file)`); `None` when
/// the file does not exist, an internal error when it is not JSON.
pub fn json(folder: Folder, rel: &str) -> Result<Option<Value>, DfwError> {
    let Some(text) = text(folder, rel) else {
        return Ok(None);
    };
    let value: Value = serde_json::from_str(&text)
        .map_err(|err| DfwError::internal(format!("Invalid JSON in {rel}: {err}")))?;
    Ok(Some(js::key_order(value)))
}

/// The names of the files directly inside a sub-folder (`""` for the folder itself), sorted by
/// bytes as `dir_names` sorts; empty when the sub-folder does not exist.
pub fn file_names(folder: Folder, rel_dir: &str) -> Vec<String> {
    let Some(parts) = clean(rel_dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = match folder.override_dir() {
        Some(dir) => {
            let dir = parts.iter().fold(dir, |acc, p| acc.join(p));
            fs::read_dir(dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
                        .map(|e| e.file_name().to_string_lossy().into_owned())
                        .collect()
                })
                .unwrap_or_default()
        }
        None => {
            let root = folder.embedded();
            let sub = if parts.is_empty() {
                Some(root)
            } else {
                root.get_dir(parts.join("/"))
            };
            sub.map(|d| {
                d.files()
                    .filter_map(|f| f.path().file_name())
                    .map(|n| n.to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default()
        }
    };
    names.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
    names
}

/// A file of `core/studio` (`app.html`, `penpot/plugin.js`).
pub fn studio_file(rel: &str) -> Option<Cow<'static, [u8]>> {
    file(Folder::Studio, rel)
}

/// [`studio_file`] as text.
pub fn studio_text(rel: &str) -> Option<String> {
    text(Folder::Studio, rel)
}

/// A template by its path under `core/templates` (`PRODUCT.md`, `handoff.md`), as text.
pub fn template_text(rel: &str) -> Result<String, DfwError> {
    text(Folder::Templates, rel)
        .ok_or_else(|| DfwError::internal(format!("template {rel} is missing from this build")))
}

/// `copyDir(path.join(templatesDir(), rel), dest)`: every file of a template folder, each written
/// atomically.
pub fn copy_templates(rel: &str, dest: &Path) -> Result<(), DfwError> {
    let missing =
        || DfwError::internal(format!("template folder {rel} is missing from this build"));
    let parts = clean_file(rel).ok_or_else(missing)?;
    if let Some(dir) = Folder::Templates.override_dir() {
        let src = parts.iter().fold(dir, |acc, p| acc.join(p));
        for file in list_files(&src, &[])? {
            write_atomic(&dest.join(&file), fs::read(src.join(&file))?)?;
        }
        return Ok(());
    }
    let dir = TEMPLATES.get_dir(parts.join("/")).ok_or_else(missing)?;
    fn copy(dir: &Dir<'_>, base: &Path, dest: &Path) -> Result<(), DfwError> {
        for file in dir.files() {
            let rel = file.path().strip_prefix(base).unwrap_or(file.path());
            write_atomic(&dest.join(rel), file.contents())?;
        }
        for sub in dir.dirs() {
            copy(sub, base, dest)?;
        }
        Ok(())
    }
    copy(dir, dir.path(), dest)
}

/// `core/manifest.json` as it was at build time (name, version, the design skills it depends on).
pub fn manifest() -> Value {
    serde_json::from_str(MANIFEST).unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_browser_files_are_embedded() {
        for rel in [
            "app.html",
            "app.js",
            "app.css",
            "overlay.js",
            "live.js",
            "penpot/plugin.js",
            "penpot/icon.svg",
            "penpot/ui.html",
        ] {
            assert!(STUDIO.get_file(rel).is_some(), "{rel}");
        }
    }

    #[test]
    fn the_templates_are_embedded() {
        assert!(template_text("PRODUCT.md").unwrap().len() > 10);
        let dest = crate::core::util::test_dir("templates");
        copy_templates("design", &dest).unwrap();
        assert!(dest.join("design.json").is_file());
        assert!(dest.join("tokens/tokens.json").is_file());
        assert!(dest.join("screens/.gitkeep").is_file());
    }

    #[test]
    fn kits_assets_workflow_and_the_manifest_are_embedded() {
        assert!(KITS.get_file("base/presets/crisp.tokens.json").is_some());
        assert!(KITS.get_file("base/components/button.html").is_some());
        assert!(KITS.get_file("styles/swiss-grid.json").is_some());
        assert!(ASSETS.get_file("icons/lucide.json").is_some());
        assert!(WORKFLOW.get_file("SKILL.md").is_some());
        assert!(WORKFLOW.get_file("references/phase-concepts.md").is_some());
        assert_eq!(manifest()["name"], "dfw");
        assert!(manifest()["dependencies"]
            .as_array()
            .is_some_and(|d| !d.is_empty()));
    }

    #[test]
    fn file_names_lists_the_files_of_one_folder_in_byte_order() {
        let names = file_names(Folder::Kits, "base/presets");
        assert!(names.contains(&"crisp.tokens.json".to_string()));
        assert!(names.windows(2).all(|w| w[0].as_bytes() < w[1].as_bytes()));
        assert!(file_names(Folder::Kits, "no-such-folder").is_empty());
        assert!(file_names(Folder::Kits, "../core").is_empty());
        // Only files, not the sub-folders.
        assert!(!file_names(Folder::Kits, "base").contains(&"presets".to_string()));
    }

    #[test]
    fn json_reads_in_javascript_key_order_and_none_when_missing() {
        let t = json(Folder::Kits, "base/presets/crisp.tokens.json")
            .unwrap()
            .unwrap();
        assert!(t.is_object());
        assert!(json(Folder::Kits, "base/presets/none.json")
            .unwrap()
            .is_none());
        assert!(exists(Folder::Assets, "icons/lucide.json"));
        assert!(!exists(Folder::Assets, "icons"));
        assert!(!exists(Folder::Assets, "../manifest.json"));
    }

    #[test]
    fn paths_that_leave_the_folder_are_refused() {
        for rel in ["../x", "a/../../x", "a\\b", "c:x"] {
            assert!(clean(rel).is_none(), "{rel}");
            assert!(file(Folder::Studio, rel).is_none(), "{rel}");
        }
        for rel in ["", "/"] {
            assert!(file(Folder::Studio, rel).is_none(), "{rel}");
        }
        assert_eq!(clean("./penpot//ui.html"), Some(vec!["penpot", "ui.html"]));
    }
}
