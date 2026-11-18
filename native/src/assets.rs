//! Embedded files (new; no Node file, design decision 6): the browser files of `core/studio`
//! (app, overlay, live review, Penpot plugin), which the Studio and the review proxy serve, and
//! `core/templates` (PRODUCT.md, DESIGN.md, the handoff and the starter `design/` folder). When
//! `DFW_PLUGIN_ROOT` names a folder that has `studio/` or `templates/`, the files there win, so a
//! change needs no rebuild during development.
//!
//! `core/kits/` and `core/assets/` are still read from the plugin folder (`design::icons::
//! plugin_dir`): the design modules walk them as folders.

use std::borrow::Cow;
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};

use crate::core::error::DfwError;
use crate::core::paths::{env_nonempty, resolve};
use crate::core::util::list_files;

/// `core/studio` as it was at build time (build.rs makes cargo rebuild when it changes).
static STUDIO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/studio");

/// `core/templates` as it was at build time.
static TEMPLATES: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/templates");

/// `<DFW_PLUGIN_ROOT>/<name>` when that folder exists.
fn override_dir(name: &str) -> Option<PathBuf> {
    let dir = resolve(env_nonempty("DFW_PLUGIN_ROOT")?).join(name);
    dir.is_dir().then_some(dir)
}

fn studio_override() -> Option<PathBuf> {
    override_dir("studio")
}

/// A clean relative path: `/`-separated normal segments only (the callers clamp `..` first; this
/// refuses anything else so an override folder is never left).
fn clean(rel: &str) -> Option<Vec<&str>> {
    let parts: Vec<&str> = rel
        .split('/')
        .filter(|p| !p.is_empty() && *p != ".")
        .collect();
    let bad = parts.is_empty()
        || parts
            .iter()
            .any(|p| *p == ".." || p.contains('\\') || p.contains(':') || p.contains('\0'));
    (!bad).then_some(parts)
}

/// A file of `core/studio` by its path relative to that folder (`app.html`, `penpot/plugin.js`):
/// from the override folder when there is one, else embedded. `None` when it does not exist or
/// is a folder.
pub fn studio_file(rel: &str) -> Option<Cow<'static, [u8]>> {
    let parts = clean(rel)?;
    if let Some(dir) = studio_override() {
        let file = parts.iter().fold(dir, |acc, p| acc.join(p));
        return fs::read(&file).ok().map(Cow::Owned);
    }
    STUDIO
        .get_file(parts.join("/"))
        .map(|f| Cow::Borrowed(f.contents()))
}

/// [`studio_file`] as text (invalid UTF-8 replaced, as `readFileSync(f, 'utf8')`).
pub fn studio_text(rel: &str) -> Option<String> {
    studio_file(rel).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

/// A template by its path under `core/templates` (`PRODUCT.md`, `handoff.md`), as text.
pub fn template_text(rel: &str) -> Result<String, DfwError> {
    let missing = || DfwError::internal(format!("template {rel} is missing from this build"));
    let parts = clean(rel).ok_or_else(missing)?;
    if let Some(dir) = override_dir("templates") {
        let file = parts.iter().fold(dir, |acc, p| acc.join(p));
        return Ok(String::from_utf8_lossy(&fs::read(&file)?).into_owned());
    }
    TEMPLATES
        .get_file(parts.join("/"))
        .map(|f| String::from_utf8_lossy(f.contents()).into_owned())
        .ok_or_else(missing)
}

/// `copyDir(path.join(templatesDir(), rel), dest)`: every file of a template folder.
pub fn copy_templates(rel: &str, dest: &Path) -> Result<(), DfwError> {
    let missing =
        || DfwError::internal(format!("template folder {rel} is missing from this build"));
    let parts = clean(rel).ok_or_else(missing)?;
    if let Some(dir) = override_dir("templates") {
        let src = parts.iter().fold(dir, |acc, p| acc.join(p));
        for file in list_files(&src, &[])? {
            let to = dest.join(&file);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(src.join(&file), &to)?;
        }
        return Ok(());
    }
    let dir = TEMPLATES.get_dir(parts.join("/")).ok_or_else(missing)?;
    fn copy(dir: &Dir<'_>, base: &Path, dest: &Path) -> Result<(), DfwError> {
        for file in dir.files() {
            let rel = file.path().strip_prefix(base).unwrap_or(file.path());
            let to = dest.join(rel);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&to, file.contents())?;
        }
        for sub in dir.dirs() {
            copy(sub, base, dest)?;
        }
        Ok(())
    }
    copy(dir, dir.path(), dest)
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
    fn paths_that_leave_the_folder_are_refused() {
        for rel in ["", "/", "../x", "a/../../x", "a\\b", "c:x"] {
            assert!(clean(rel).is_none(), "{rel}");
        }
        assert_eq!(clean("./penpot//ui.html"), Some(vec!["penpot", "ui.html"]));
    }
}
