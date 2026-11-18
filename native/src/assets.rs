//! Embedded files (new; no Node file, design decision 6). Today: the browser files of
//! `core/studio` (app, overlay, live review, Penpot plugin), which the Studio and the review proxy
//! serve. When `DFW_PLUGIN_ROOT` names a folder that has `studio/`, the files there win, so a CSS
//! or JavaScript change needs no rebuild during development.
//!
//! `core/kits/`, `core/templates/`, `core/schemas/`, `core/assets/` and `core/workflow/` join this
//! file when their groups embed them.

use std::borrow::Cow;
use std::fs;
use std::path::PathBuf;

use include_dir::{include_dir, Dir};

use crate::core::paths::{env_nonempty, resolve};

/// `core/studio` as it was at build time. Cargo does not see a change to these files: after
/// editing one, `touch src/assets.rs` (or set `DFW_PLUGIN_ROOT`) before `cargo build`.
static STUDIO: Dir<'static> = include_dir!("$CARGO_MANIFEST_DIR/../core/studio");

/// `<DFW_PLUGIN_ROOT>/studio` when that folder exists.
fn studio_override() -> Option<PathBuf> {
    let dir = resolve(env_nonempty("DFW_PLUGIN_ROOT")?).join("studio");
    dir.is_dir().then_some(dir)
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
    fn paths_that_leave_the_folder_are_refused() {
        for rel in ["", "/", "../x", "a/../../x", "a\\b", "c:x"] {
            assert!(clean(rel).is_none(), "{rel}");
        }
        assert_eq!(clean("./penpot//ui.html"), Some(vec!["penpot", "ui.html"]));
    }
}
