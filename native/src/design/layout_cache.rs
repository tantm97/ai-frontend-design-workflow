//! Ports `changedScreens` and `markChecked` of `core/lib/layout.mjs`: which screens changed
//! since the last layout check. The check itself (a browser measures the page) is
//! `browser/layout_check`.
//!
//! The key covers every input of a check (`performance.md` PF5): the masters, the tokens, the
//! shared files, each screen file, every other file under `design/` a page can link (themes,
//! stylesheets, images), the breakpoints (the manifest's, else the config's) and the states.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::tokens::dir_names;
use crate::core::config::{key as config_key, load_config_quietly};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{file, in_design, project_paths, resolve, ProjectPaths};
use crate::core::util::{get_path, list_files, read_json, sha256, write_json_inside};
use crate::design::manifest::{key as manifest_key, Manifest};

/// The keys of `.dfw/layout-cache.json`: one constant, or a prefix and a name.
mod key {
    /// The hash of `design/tokens/tokens.json`.
    pub const TOKENS: &str = "t:tokens";
    /// The hash of what every render of every screen depends on: the breakpoints and the
    /// manifest's states.
    pub const LAYOUT: &str = "l:layout";
    /// `c:<master file>`.
    pub const MASTER: &str = "c:";
    /// `f:<shared file>`, a [`super::SHARED`] file.
    pub const SHARED: &str = "f:";
    /// `s:<screen file>`, as the manifest names it.
    pub const SCREEN: &str = "s:";
    /// `m:<screen id>`: the hash of the screen's own states.
    pub const STATES: &str = "m:";
    /// `d:<file>`: any other file under `design/` (a theme, a stylesheet, an image).
    pub const OTHER: &str = "d:";
}

/// Files every screen loads besides the tokens.
const SHARED: [&str; 2] = [in_design::BASE_CSS, in_design::HEAD];

/// The folders of `design/` that are not a layout input: the frozen snapshots and the handoff
/// (no screen loads them). Hidden files (`.DS_Store`) are not one either.
pub(crate) const NOT_LAYOUT_INPUT: [&str; 2] = [file::APPROVED, file::HANDOFF];

/// A content hash, not a modification time: a checkout or touch that does not change a file
/// does not invalidate its check, and an edit always does. `None` when the file cannot be read.
fn hash_of(file: &Path) -> Option<String> {
    fs::read(file).ok().map(hash_bytes)
}

fn hash_bytes(bytes: impl AsRef<[u8]>) -> String {
    sha256(bytes)[..16].to_string()
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

fn cache_file(p: &ProjectPaths) -> PathBuf {
    p.layout_cache.clone()
}

/// `path.join(design, file)`: the name is relative even when it starts with `/`.
fn design_file(p: &ProjectPaths, file: &str) -> PathBuf {
    resolve(p.design.join(file.trim_start_matches('/')))
}

/// One screen of the manifest as the cache sees it.
struct ScreenInputs {
    id: String,
    /// The hash of the screen's own states.
    states: String,
    /// `(file as the manifest names it, it exists, its hash)` of every variant.
    files: Vec<(String, bool, Option<String>)>,
}

/// The hashes of every input of a layout check, read once.
struct Inputs {
    /// `(file name, hash)` of every entry of the masters' folder.
    masters: Vec<(String, Option<String>)>,
    tokens: Option<String>,
    shared: Vec<(&'static str, Option<String>)>,
    layout: String,
    screens: Vec<ScreenInputs>,
    /// `(path in design/, hash)` of every other file under `design/`.
    others: Vec<(String, Option<String>)>,
}

impl Inputs {
    fn read(root: &Path, p: &ProjectPaths) -> Result<Inputs, DfwError> {
        let manifest = read_json(&p.manifest, Some(Value::Object(Map::new())))?;
        let config = load_config_quietly(root)?;
        let masters = dir_names(&p.components)?
            .into_iter()
            .map(|f| {
                let hash = hash_of(&p.components.join(&f));
                (f, hash)
            })
            .collect();
        let shared = SHARED
            .iter()
            .map(|f| (*f, hash_of(&p.design.join(f))))
            .collect();
        let breakpoints = manifest
            .get(manifest_key::BREAKPOINTS)
            .filter(|b| js::truthy(b))
            .or_else(|| get_path(&config, config_key::STUDIO_BREAKPOINTS));
        let layout =
            hash_bytes(json!([breakpoints, manifest.get(manifest_key::STATES)]).to_string());
        let mut screens = Vec::new();
        for screen in Manifest(&manifest).screens() {
            let states = json!(screen.value().get(manifest_key::STATES)).to_string();
            let files = screen
                .variants()
                .filter_map(|v| v.file())
                .map(|f| {
                    let abs = design_file(p, f);
                    (f.to_string(), abs.exists(), hash_of(&abs))
                })
                .collect();
            screens.push(ScreenInputs {
                id: screen.id_text(),
                states: hash_bytes(states),
                files,
            });
        }
        let keyed = |f: &str| {
            let master = f
                .strip_prefix(&format!("{}/{}/", file::SYSTEM, file::COMPONENTS))
                .is_some_and(|name| !name.contains('/'));
            f == file::MANIFEST
                || f == in_design::TOKENS_JSON
                || SHARED.contains(&f)
                || master
                || screens
                    .iter()
                    .flat_map(|s| &s.files)
                    .any(|(v, _, _)| v.trim_start_matches('/') == f)
        };
        let others = list_files(&p.design, &NOT_LAYOUT_INPUT)?
            .into_iter()
            .filter(|f| !f.split('/').any(|part| part.starts_with('.')) && !keyed(f))
            .map(|f| {
                let hash = hash_of(&p.design.join(&f));
                (f, hash)
            })
            .collect();
        Ok(Inputs {
            masters,
            tokens: hash_of(&p.tokens_json),
            shared,
            layout,
            screens,
            others,
        })
    }
}

/// What [`changed_screens`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changed {
    /// Ids of the screens to check again, in manifest order (an id that is not a string is
    /// listed as its text).
    pub screens: Vec<String>,
    /// Names of the masters whose file changed.
    pub changed_comps: Vec<String>,
    /// The tokens, a file every screen can load (base styles, shared head, a theme, another
    /// stylesheet or image under `design/`) or the breakpoints and states changed.
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
    let cache = read_json(&cache_file(&p), Some(Value::Object(Map::new())))?;
    let now = Inputs::read(root, &p)?;
    let cached = |prefix: &str, name: &str| cache.get(format!("{prefix}{name}"));
    let mut changed_comps: Vec<String> = Vec::new();
    for (f, hash) in &now.masters {
        if differs(hash, cached(key::MASTER, f)) {
            let name = f.replacen(".html", "", 1);
            if !changed_comps.contains(&name) {
                changed_comps.push(name);
            }
        }
    }
    // A file under design/ that was there at the last check and is gone now.
    let removed = cache.as_object().is_some_and(|c| {
        c.keys().any(|k| {
            k.strip_prefix(key::OTHER)
                .is_some_and(|f| !now.others.iter().any(|(o, _)| o == f))
        })
    });
    // tokens_changed also covers the other files every screen can load and the breakpoints.
    let tokens_changed = differs(&now.tokens, cache.get(key::TOKENS))
        || now.shared.iter().any(|(f, hash)| {
            let cached = js::present(cached(key::SHARED, f)).unwrap_or(&Value::Null);
            differs(hash, Some(cached))
        })
        || differs(&Some(now.layout.clone()), cache.get(key::LAYOUT))
        || now
            .others
            .iter()
            .any(|(f, hash)| differs(hash, cached(key::OTHER, f)))
        || removed;
    let mut screens: Vec<String> = Vec::new();
    for screen in &now.screens {
        let states_changed = differs(
            &Some(screen.states.clone()),
            cached(key::STATES, &screen.id),
        );
        for (file, exists, hash) in &screen.files {
            if !exists {
                continue;
            }
            // A changed master reaches a screen through other masters too (card -> button), so
            // any changed master re-checks every screen.
            let changed = tokens_changed
                || !changed_comps.is_empty()
                || states_changed
                || differs(hash, cached(key::SCREEN, file));
            if changed && !screens.contains(&screen.id) {
                screens.push(screen.id.clone());
            }
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
    let now = Inputs::read(root, &p)?;
    let value = |hash: &Option<String>| hash.clone().map_or(Value::Null, Value::String);
    let mut cache = Map::new();
    for (f, hash) in &now.masters {
        cache.insert(format!("{}{f}", key::MASTER), value(hash));
    }
    cache.insert(key::TOKENS.into(), value(&now.tokens));
    for (f, hash) in &now.shared {
        cache.insert(format!("{}{f}", key::SHARED), value(hash));
    }
    for screen in &now.screens {
        for (file, _, hash) in &screen.files {
            cache.insert(format!("{}{file}", key::SCREEN), value(hash));
        }
    }
    cache.insert(key::LAYOUT.into(), Value::String(now.layout));
    for screen in &now.screens {
        cache.insert(
            format!("{}{}", key::STATES, screen.id),
            Value::String(screen.states.clone()),
        );
    }
    for (f, hash) in &now.others {
        cache.insert(format!("{}{f}", key::OTHER), value(hash));
    }
    write_json_inside(root, &cache_file(&p), &Value::Object(cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{BUTTON_MASTER, HTML_FILE_B, SCREEN, SCREEN_INDEX, SCREEN_PAGE};
    use crate::core::paths::{file, rel};
    use crate::core::util::test_dir;

    /// The component of [`BUTTON_MASTER`].
    const BUTTON: &str = "button";

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
            rel::MANIFEST,
            r#"{"screens":[{"id":"home","variants":[{"id":"a","file":"screens/home/index.html"}]}]}"#,
        );
        write(&root, rel::TOKENS_JSON, "{}");
        write(
            &root,
            &rel::master("card"),
            r#"<dfw-component name="card"><template><div class="card"><dfw-use c="button">Go</dfw-use></div></template></dfw-component>"#,
        );
        write(&root, &rel::component(BUTTON_MASTER), &button(4));
        write(&root, rel::BASE_CSS, "body{margin:0}");
        write(
            &root,
            &rel::screen(SCREEN_INDEX),
            r#"<html><body><dfw-use c="card"></dfw-use></body></html>"#,
        );
        let screens = || changed_screens(&root).unwrap().screens;
        let none: Vec<String> = Vec::new();
        mark_checked(&root).unwrap();
        assert_eq!(screens(), none);
        write(&root, &rel::component(BUTTON_MASTER), &button(400));
        assert_eq!(screens(), [SCREEN], "home uses button through card");
        mark_checked(&root).unwrap();
        assert_eq!(screens(), none);
        write(&root, rel::BASE_CSS, "body{margin:0;width:3000px}");
        assert_eq!(screens(), [SCREEN], "base.css is loaded by every screen");
    }

    /// The cache file: one hash per master, the tokens, the shared files and each screen file;
    /// a file that cannot be read is `null`.
    #[test]
    fn cache_file() {
        let root = test_dir("layout-cache");
        write(
            &root,
            rel::MANIFEST,
            r#"{"breakpoints":[{"id":"d","width":1,"height":1}],"screens":[{"id":"home","variants":[{"id":"a","file":"screens/home/index.html"},{"id":"b","file":"screens/home/missing.html"}]}]}"#,
        );
        /// The content of both files: its sha256 starts with ba7816bf8f01cfea.
        const CONTENT: &str = "abc";
        write(&root, &rel::screen(SCREEN_INDEX), CONTENT);
        write(&root, &rel::component(HTML_FILE_B), CONTENT);
        let first = changed_screens(&root).unwrap();
        assert_eq!(first.changed_comps, ["b"]);
        assert!(first.tokens_changed);
        mark_checked(&root).unwrap();
        let text = fs::read_to_string(root.join(rel::join(rel::DFW, file::LAYOUT_CACHE))).unwrap();
        assert_eq!(
            text,
            "{\n  \"c:b.html\": \"ba7816bf8f01cfea\",\n  \"t:tokens\": null,\n  \"f:styles/base.css\": null,\n  \"f:system/head.html\": null,\n  \"s:screens/home/index.html\": \"ba7816bf8f01cfea\",\n  \"s:screens/home/missing.html\": null,\n  \"l:layout\": \"9e43849f9c2ba915\",\n  \"m:home\": \"74234e98afe7498f\"\n}\n"
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
        write(rel::MANIFEST, r#"{"screens":[{"id":"home","variants":[{"id":"a","file":"screens/home/a.html"},{"id":"gone","file":"screens/home/gone.html"}]}]}"#);
        write(&rel::screen(SCREEN_PAGE), "<p>a</p>");
        write(&rel::component(BUTTON_MASTER), "<b></b>");
        write(rel::TOKENS_JSON, "{}");
        let first = changed_screens(&root).unwrap();
        assert_eq!(first, Changed { screens: vec![SCREEN.into()], changed_comps: vec![BUTTON.into()], tokens_changed: true });
        mark_checked(&root).unwrap();
        assert!(changed_screens(&root).unwrap().is_empty());
        let cache = fs::read_to_string(root.join(rel::join(rel::DFW, file::LAYOUT_CACHE))).unwrap();
        let keys: Vec<String> = serde_json::from_str::<Value>(&cache).unwrap().as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["c:button.html", "t:tokens", "f:styles/base.css", "f:system/head.html", "s:screens/home/a.html", "s:screens/home/gone.html", "l:layout", "m:home"]);
        assert!(cache.contains("\"f:styles/base.css\": null") && cache.ends_with("}\n"));
        write(rel::BASE_CSS, "body{}");
        assert_eq!(changed_screens(&root).unwrap(), Changed { screens: vec![SCREEN.into()], changed_comps: vec![], tokens_changed: true });
        mark_checked(&root).unwrap();
        write(&rel::component(BUTTON_MASTER), "<i></i>");
        assert_eq!(changed_screens(&root).unwrap(), Changed { screens: vec![SCREEN.into()], changed_comps: vec![BUTTON.into()], tokens_changed: false });
    }

    /// The key covers every input of a layout check (`performance.md` PF5): a change to any file
    /// under `design/` but the declared [`NOT_LAYOUT_INPUT`], to the breakpoints of the manifest
    /// or the config, or to the states, re-checks screens.
    #[test]
    fn every_layout_input_is_in_the_key() {
        use crate::core::fixture::{CSS_FILE, DARK_THEME, HANDOFF_NOTE, OTHER_CSS_FILE};
        use serde_json::json;
        /// The states the manifest and the screen declare, before and after a change.
        const EMPTY: &str = "empty";
        const FULL: &str = "full";
        const BUSY: &str = "busy";
        const IDLE: &str = "idle";
        let root = test_dir("layout-cache-inputs");
        let page = rel::join(file::SCREENS, SCREEN_PAGE);
        let manifest = |breakpoints: Value, states: &str, own: &str| {
            json!({ manifest_key::BREAKPOINTS: breakpoints, manifest_key::STATES: [states], manifest_key::SCREENS: [{ manifest_key::ID: SCREEN, manifest_key::STATES: [own], manifest_key::VARIANTS: [{ manifest_key::ID: "a", manifest_key::FILE: page }] }] }).to_string()
        };
        let bp = |width: u32| json!([{ "id": "d", "width": width, "height": 800 }]);
        let screen_css = rel::screen(&rel::join(SCREEN, CSS_FILE));
        let image = rel::join(rel::ASSETS, &file::icon("logo"));
        let new_css = rel::join(rel::STYLES, OTHER_CSS_FILE);
        write(&root, rel::MANIFEST, &manifest(bp(1000), EMPTY, BUSY));
        write(&root, rel::TOKENS_JSON, "{}");
        write(&root, &rel::join(rel::TOKENS, file::TOKENS_CSS), ":root{}");
        write(&root, &rel::join(rel::THEMES, DARK_THEME), "{}");
        write(&root, rel::HEAD, "<link rel=\"stylesheet\" href=\"x.css\">");
        write(&root, rel::COMPONENTS_CSS, ".btn{}");
        write(&root, &rel::component(BUTTON_MASTER), "<b></b>");
        write(&root, rel::BASE_CSS, "body{}");
        write(
            &root,
            &rel::screen(SCREEN_PAGE),
            &format!(
                "<link rel=\"stylesheet\" href=\"{CSS_FILE}\"><img src=\"../../assets/logo.svg\">"
            ),
        );
        write(&root, &screen_css, ".hero{}");
        write(&root, &image, "<svg></svg>");
        write(&root, &rel::approved(file::MANIFEST), "{}");
        write(&root, &rel::join(rel::HANDOFF, HANDOFF_NOTE), "x");
        write(&root, &rel::screen(file::GITIGNORE), "x");
        mark_checked(&root).unwrap();
        assert!(changed_screens(&root).unwrap().is_empty());
        let screens = || changed_screens(&root).unwrap().screens;
        let not_input = |f: &str| {
            NOT_LAYOUT_INPUT.iter().any(|d| rel::under(d, f))
                || f.split('/').any(|part| part.starts_with('.'))
        };
        let design = root.join(rel::DESIGN);
        for f in list_files(&design, &[]).unwrap() {
            if f == file::MANIFEST {
                continue; // its layout fields are changed one by one below
            }
            let path = design.join(&f);
            let mut bytes = fs::read(&path).unwrap();
            bytes.push(b' ');
            fs::write(&path, bytes).unwrap();
            if not_input(&f) {
                assert!(screens().is_empty(), "{f} is declared not a layout input");
            } else {
                assert_eq!(screens(), [SCREEN], "a change to {f} re-checks");
            }
            mark_checked(&root).unwrap();
        }
        write(&root, &new_css, ".new{}");
        assert_eq!(screens(), [SCREEN], "a new stylesheet re-checks");
        mark_checked(&root).unwrap();
        fs::remove_file(root.join(&new_css)).unwrap();
        assert_eq!(screens(), [SCREEN], "a removed stylesheet re-checks");
        mark_checked(&root).unwrap();
        for (what, text) in [
            ("a manifest breakpoint", manifest(bp(1200), EMPTY, BUSY)),
            ("the manifest states", manifest(bp(1200), FULL, BUSY)),
            ("a screen's states", manifest(bp(1200), FULL, IDLE)),
            ("no manifest breakpoints", manifest(Value::Null, FULL, IDLE)),
        ] {
            write(&root, rel::MANIFEST, &text);
            assert_eq!(screens(), [SCREEN], "{what} re-checks");
            mark_checked(&root).unwrap();
        }
        let config = |width: u32| json!({ "studio": { "breakpoints": bp(width) } }).to_string();
        write(&root, rel::CONFIG, &config(500));
        assert_eq!(
            screens(),
            [SCREEN],
            "the config breakpoints are used without manifest ones"
        );
        mark_checked(&root).unwrap();
        write(&root, rel::CONFIG, &config(600));
        assert_eq!(screens(), [SCREEN], "a config breakpoint re-checks");
    }
}
