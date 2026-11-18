//! Ports `changedScreens` and `markChecked` of `core/lib/layout.mjs`: which screens changed
//! since the last layout check. The check itself (a browser measures the page) is
//! `browser/layout_check`.

use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use super::tokens::dir_names;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve, ProjectPaths};
use crate::core::util::{read_json, sha256, write_json};

/// Files every screen loads besides the tokens.
const SHARED: [&str; 2] = ["styles/base.css", "system/head.html"];

/// A content hash, not a modification time: a checkout or touch that does not change a file
/// does not invalidate its check, and an edit always does. `None` when the file cannot be read.
fn hash_of(file: &Path) -> Option<String> {
    fs::read(file)
        .ok()
        .map(|bytes| sha256(bytes)[..16].to_string())
}

/// `hash !== cached` of the Node code: an unreadable file is `null`, a key the cache does not
/// have is `undefined`, and those two are different.
fn differs(hash: &Option<String>, cached: Option<&Value>) -> bool {
    match (hash, cached) {
        (Some(hash), Some(Value::String(cached))) => hash != cached,
        (None, Some(Value::Null)) => false,
        _ => true,
    }
}

fn cache_file(p: &ProjectPaths) -> std::path::PathBuf {
    p.dfw.join("layout-cache.json")
}

/// The `file` of every variant of every screen in the manifest, with the screen's id.
fn variants(manifest: &Value) -> Vec<(String, &str)> {
    fn list(v: Option<&Value>) -> impl Iterator<Item = &Value> {
        v.and_then(Value::as_array).into_iter().flatten()
    }
    let mut out = Vec::new();
    for screen in list(manifest.get("screens")) {
        for variant in list(screen.get("variants")) {
            if let Some(file) = variant.get("file").and_then(Value::as_str) {
                out.push((js::to_string(screen.get("id")), file));
            }
        }
    }
    out
}

/// `path.join(design, file)`: the name is relative even when it starts with `/`.
fn design_file(p: &ProjectPaths, file: &str) -> std::path::PathBuf {
    resolve(p.design.join(file.trim_start_matches('/')))
}

/// What [`changed_screens`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    /// Ids of the screens to check again, in manifest order (an id that is not a string is
    /// listed as its text).
    pub screens: Vec<String>,
    /// Names of the masters whose file changed.
    pub changed_comps: Vec<String>,
    /// The tokens or a file every screen loads (base styles, shared head) changed.
    pub tokens_changed: bool,
}

impl Changed {
    /// Nothing to check again.
    pub fn is_empty(&self) -> bool {
        self.screens.is_empty() && self.changed_comps.is_empty() && !self.tokens_changed
    }
}

/// `changedScreens`: screens whose files (or the masters and shared files they use) changed
/// since the last [`mark_checked`].
pub fn changed_screens(root: &Path) -> Result<Changed, DfwError> {
    let p = project_paths(root);
    let empty = || Some(Value::Object(Map::new()));
    let cache = read_json(&cache_file(&p), empty())?;
    let manifest = read_json(&p.manifest, empty())?;
    let comp_dir = p.design.join("system").join("components");
    let mut changed_comps: Vec<String> = Vec::new();
    for f in dir_names(&comp_dir)? {
        if differs(&hash_of(&comp_dir.join(&f)), cache.get(format!("c:{f}"))) {
            let name = f.replacen(".html", "", 1);
            if !changed_comps.contains(&name) {
                changed_comps.push(name);
            }
        }
    }
    // tokens_changed also covers the other files every screen loads (base styles, shared head).
    let tokens_changed = differs(
        &hash_of(&p.tokens.join("tokens.json")),
        cache.get("t:tokens"),
    ) || SHARED.iter().any(|f| {
        let cached = js::present(cache.get(format!("f:{f}"))).unwrap_or(&Value::Null);
        differs(&hash_of(&p.design.join(f)), Some(cached))
    });
    let mut screens: Vec<String> = Vec::new();
    for (id, file) in variants(&manifest) {
        let abs = design_file(&p, file);
        if !abs.exists() {
            continue;
        }
        // A changed master reaches a screen through other masters too (card -> button), so any
        // changed master re-checks every screen.
        let changed = tokens_changed
            || !changed_comps.is_empty()
            || differs(&hash_of(&abs), cache.get(format!("s:{file}")));
        if changed && !screens.contains(&id) {
            screens.push(id);
        }
    }
    Ok(Changed {
        screens,
        changed_comps,
        tokens_changed,
    })
}

/// `markChecked`: record the content hashes of the design in `.dfw/layout-cache.json` (the
/// receipt of a check is tied to the exact content that was measured).
pub fn mark_checked(root: &Path) -> Result<(), DfwError> {
    let p = project_paths(root);
    let value = |hash: Option<String>| hash.map_or(Value::Null, Value::String);
    let mut cache = Map::new();
    let comp_dir = p.design.join("system").join("components");
    for f in dir_names(&comp_dir)? {
        cache.insert(format!("c:{f}"), value(hash_of(&comp_dir.join(&f))));
    }
    cache.insert(
        "t:tokens".into(),
        value(hash_of(&p.tokens.join("tokens.json"))),
    );
    for f in SHARED {
        cache.insert(format!("f:{f}"), value(hash_of(&p.design.join(f))));
    }
    let manifest = read_json(&p.manifest, Some(Value::Object(Map::new())))?;
    for (_, file) in variants(&manifest) {
        cache.insert(format!("s:{file}"), value(hash_of(&design_file(&p, file))));
    }
    write_json(&cache_file(&p), &Value::Object(cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn write(root: &Path, rel: &str, text: &str) {
        let file = root.join(rel);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    /// 4: a master used through another master, or the shared base.css, marks its screens as changed
    #[test]
    fn master_through_master_and_base_css() {
        let button = |pad: u32| {
            format!("<dfw-component name=\"button\"><style>.btn{{padding:{pad}px}}</style><template><button class=\"btn\"><dfw-slot></dfw-slot></button></template></dfw-component>")
        };
        let root = test_dir("layout-cache");
        write(
            &root,
            "design/design.json",
            r#"{"screens":[{"id":"home","variants":[{"id":"a","file":"screens/home/index.html"}]}]}"#,
        );
        write(&root, "design/tokens/tokens.json", "{}");
        write(
            &root,
            "design/system/components/card.html",
            r#"<dfw-component name="card"><template><div class="card"><dfw-use c="button">Go</dfw-use></div></template></dfw-component>"#,
        );
        write(&root, "design/system/components/button.html", &button(4));
        write(&root, "design/styles/base.css", "body{margin:0}");
        write(
            &root,
            "design/screens/home/index.html",
            r#"<html><body><dfw-use c="card"></dfw-use></body></html>"#,
        );
        let screens = || changed_screens(&root).unwrap().screens;
        let none: Vec<String> = Vec::new();
        mark_checked(&root).unwrap();
        assert_eq!(screens(), none);
        write(&root, "design/system/components/button.html", &button(400));
        assert_eq!(screens(), ["home"], "home uses button through card");
        mark_checked(&root).unwrap();
        assert_eq!(screens(), none);
        write(
            &root,
            "design/styles/base.css",
            "body{margin:0;width:3000px}",
        );
        assert_eq!(screens(), ["home"], "base.css is loaded by every screen");
    }

    /// The cache file: one hash per master, the tokens, the shared files and each screen file;
    /// a file that cannot be read is `null`.
    #[test]
    fn cache_file() {
        let root = test_dir("layout-cache");
        write(
            &root,
            "design/design.json",
            r#"{"screens":[{"id":"home","variants":[{"id":"a","file":"screens/home/index.html"},{"id":"b","file":"screens/home/missing.html"}]}]}"#,
        );
        write(&root, "design/screens/home/index.html", "abc");
        write(&root, "design/system/components/b.html", "abc");
        let first = changed_screens(&root).unwrap();
        assert_eq!(first.changed_comps, ["b"]);
        assert!(first.tokens_changed);
        mark_checked(&root).unwrap();
        let text = fs::read_to_string(root.join(".dfw/layout-cache.json")).unwrap();
        // sha256("abc") starts with ba7816bf8f01cfea.
        assert_eq!(
            text,
            "{\n  \"c:b.html\": \"ba7816bf8f01cfea\",\n  \"t:tokens\": null,\n  \"f:styles/base.css\": null,\n  \"f:system/head.html\": null,\n  \"s:screens/home/index.html\": \"ba7816bf8f01cfea\",\n  \"s:screens/home/missing.html\": null\n}\n"
        );
        let after = changed_screens(&root).unwrap();
        assert!(after.is_empty());
    }

    #[rustfmt::skip]
    /// A check is tied to the content it measured (the browser group's test of its stand-in).
    #[test]
    fn changed_screens_follow_content_hashes() {
        let root = test_dir("layout-cache");
        let write = |rel: &str, text: &str| {
            let file = root.join(rel);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, text).unwrap();
        };
        write("design/design.json", r#"{"screens":[{"id":"home","variants":[{"id":"a","file":"screens/home/a.html"},{"id":"gone","file":"screens/home/gone.html"}]}]}"#);
        write("design/screens/home/a.html", "<p>a</p>");
        write("design/system/components/button.html", "<b></b>");
        write("design/tokens/tokens.json", "{}");
        let first = changed_screens(&root).unwrap();
        assert_eq!(first, Changed { screens: vec!["home".into()], changed_comps: vec!["button".into()], tokens_changed: true });
        mark_checked(&root).unwrap();
        assert!(changed_screens(&root).unwrap().is_empty());
        let cache = fs::read_to_string(root.join(".dfw/layout-cache.json")).unwrap();
        let keys: Vec<String> = serde_json::from_str::<Value>(&cache).unwrap().as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["c:button.html", "t:tokens", "f:styles/base.css", "f:system/head.html", "s:screens/home/a.html", "s:screens/home/gone.html"]);
        assert!(cache.contains("\"f:styles/base.css\": null") && cache.ends_with("}\n"));
        write("design/styles/base.css", "body{}");
        assert_eq!(changed_screens(&root).unwrap(), Changed { screens: vec!["home".into()], changed_comps: vec![], tokens_changed: true });
        mark_checked(&root).unwrap();
        write("design/system/components/button.html", "<i></i>");
        assert_eq!(changed_screens(&root).unwrap(), Changed { screens: vec!["home".into()], changed_comps: vec!["button".into()], tokens_changed: false });
    }
}
