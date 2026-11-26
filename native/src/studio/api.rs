//! The data the Studio API serves, ported from `core/studio/server.mjs`: the manifest (declared
//! or inferred), the token list, the components, design files prepared for a frame (composed,
//! component CSS linked, overlay injected), the Design System page and Studio screenshots.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use serde_json::{json, Map, Value};

use crate::browser::screenshot::{screenshot, Job, CONCURRENCY};
use crate::core::js;
use crate::design::pages::{screen_flows, studio_pages};

use crate::core::error::DfwError;
use crate::core::js::regex::with_js_classes;
use crate::core::paths::{file, in_design, project_paths, rel, resolve_inside, to_posix};
use crate::core::util::{get_path, list_files, read_json};
use crate::design::compose::{compose_html, load_components, ComposeOptions};
use crate::design::edit::load_token_names;
use crate::design::html::escape_html;
use crate::design::lint::designed_states;
use crate::design::manifest::{self, key};
use crate::design::system_page::render_system_page;
use crate::design::tokens::relative_path;
use crate::studio::paths;

/// `--name:` in tokens.css, compiled once (the token list is read on every Studio request).
static TOKEN_DECL: LazyLock<Option<regex::Regex>> =
    LazyLock::new(|| with_js_classes(r"(--[\w-]+)\s*:").ok());

/// URL prefix under which `design/` is served.
pub use super::paths::DESIGN_BASE;
/// The script every design frame gets.
pub const OVERLAY_TAG: &str = r#"<script src="/__dfw/overlay.js" data-dfw-overlay></script>"#;

/// The keys of an entry of the token list (`/__dfw/api/tokens`): `{ name, var, type }`.
mod token_entry {
    /// The token's path (`color.brand`).
    pub const NAME: &str = "name";
    /// The CSS custom property (`--color-brand`).
    pub const VAR: &str = "var";
    /// The DTCG type, `null` for a variable only tokens.css declares.
    pub const TYPE: &str = "type";
}

/// The keys of the body of `/__dfw/api/shoot`: `{ frames: [{ src, name, w, h }], fullPage }`.
mod shoot {
    pub const FRAMES: &str = "frames";
    pub const FULL_PAGE: &str = "fullPage";
    /// The frame's address in the Studio (a design file or the Design System page).
    pub const SRC: &str = "src";
    pub const NAME: &str = "name";
    pub const W: &str = "w";
    pub const H: &str = "h";
}

/// The keys of an entry of the answer of `/__dfw/api/shoot`: `{ name, ok, error?, file, url }`.
mod shot {
    /// The PNG's file name.
    pub const NAME: &str = "name";
    pub const OK: &str = "ok";
    pub const ERROR: &str = "error";
    /// The PNG, relative to the project root.
    pub const FILE: &str = "file";
    /// Where the Studio serves it, `null` when it failed.
    pub const URL: &str = "url";
}

/// Content types by file extension (`MIME` of server.mjs).
pub fn mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        ".html" => "text/html; charset=utf-8",
        ".css" => "text/css; charset=utf-8",
        ".js" | ".mjs" => "text/javascript; charset=utf-8",
        ".json" => "application/json; charset=utf-8",
        ".svg" => "image/svg+xml",
        ".png" => "image/png",
        ".jpg" | ".jpeg" => "image/jpeg",
        ".webp" => "image/webp",
        ".avif" => "image/avif",
        ".gif" => "image/gif",
        ".ico" => "image/x-icon",
        ".woff" => "font/woff",
        ".woff2" => "font/woff2",
        ".ttf" => "font/ttf",
        ".otf" => "font/otf",
        _ => return None,
    })
}

/// `path.extname`: the last `.suffix` of the last segment (none for a dot file).
pub fn extname(rel: &str) -> &str {
    let name = rel.rsplit(['/', '\\']).next().unwrap_or_default();
    match name.rfind('.') {
        Some(0) | None => "",
        Some(i) => &name[i..],
    }
}

/// `safeJoin(base, rel)` of server.mjs: `rel` normalised as a path below `/`, so `..` stops at
/// `base`; `/` and `\` both separate. Returns the segments (empty: `base` itself).
pub fn clamp_rel(rel: &str) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for seg in rel.split(['/', '\\']) {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out
}

/// `base` joined with [`clamp_rel`] of `rel`, proved inside `base` by real path: a symlink
/// (file or folder) that leads out of `base` gives `None`, as does `base` itself.
pub fn safe_join(base: &Path, rel: &str) -> Option<PathBuf> {
    let clean = clamp_rel(rel).join("/");
    resolve_inside(base, &clean).ok()
}

/// `manifest()`: `design/design.json` with the config's breakpoints, screen flows and canvas
/// pages; without that file, screens inferred from `design/screens/**`.
pub fn manifest(root: &Path, config: &Value) -> Result<Value, DfwError> {
    let p = project_paths(root);
    let m = read_json(&p.manifest, Some(Value::Null))?;
    let mut base = Map::new();
    if let Some(bps) = get_path(config, "studio.breakpoints") {
        base.insert(key::BREAKPOINTS.into(), bps.clone());
    }
    base.insert(key::THEMES.into(), json!([]));
    base.insert(key::STATES.into(), json!([]));
    if js::truthy(&m) {
        let mut out = base.clone();
        for (k, v) in js::spread(Some(&m)) {
            out.insert(k, v);
        }
        match m.get(key::BREAKPOINTS).filter(|b| js::truthy(b)) {
            Some(b) => {
                out.insert(key::BREAKPOINTS.into(), b.clone());
            }
            None => match base.get(key::BREAKPOINTS) {
                Some(b) => {
                    out.insert(key::BREAKPOINTS.into(), b.clone());
                }
                None => {
                    out.shift_remove(key::BREAKPOINTS);
                }
            },
        }
        let mut files: HashMap<String, String> = HashMap::new();
        let screens = out
            .get(key::SCREENS)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        for s in &screens {
            for v in manifest::items(s.get(key::VARIANTS)).iter() {
                let Some(file) = v
                    .get(key::FILE)
                    .and_then(Value::as_str)
                    .filter(|f| !f.is_empty())
                else {
                    continue;
                };
                // design.json is user input: a file outside design/ is not read (rule 5).
                let Ok(abs) = resolve_inside(&p.design, file) else {
                    continue;
                };
                if abs.is_file() {
                    files.insert(file.to_string(), js::read_text(&abs)?);
                }
            }
        }
        mark_designed_states(root, &mut out, &files)?;
        let flows = screen_flows(&Value::Object(out.clone()), &files);
        out.insert(key::FLOWS.into(), Value::Array(flows));
        let pages = studio_pages(&Value::Object(out.clone()));
        out.insert(key::STUDIO_PAGES.into(), Value::Array(pages));
        return Ok(Value::Object(out));
    }
    let mut screens: Vec<(String, Vec<Value>)> = Vec::new();
    for rel in list_files(&p.screens, &[])? {
        let Some(stem) = rel.strip_suffix(".html") else {
            continue;
        };
        let parts: Vec<&str> = rel.split('/').collect();
        let (id, variant) = if parts.len() > 1 {
            let last = parts[parts.len() - 1];
            (
                parts[..parts.len() - 1].join("/"),
                last.strip_suffix(".html").unwrap_or(last).to_string(),
            )
        } else {
            (stem.to_string(), "a".to_string())
        };
        let entry = json!({ (key::ID): variant, (key::LABEL): variant, (key::FILE): format!("{}/{rel}", file::SCREENS) });
        match screens.iter_mut().find(|(sid, _)| *sid == id) {
            Some((_, variants)) => variants.push(entry),
            None => screens.push((id, vec![entry])),
        }
    }
    let mut out = base;
    let product = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    out.insert(key::PRODUCT.into(), json!(product));
    out.insert(key::INFERRED.into(), json!(true));
    let screens: Vec<Value> = screens
        .into_iter()
        .map(|(id, variants)| json!({ (key::ID): id, (key::TITLE): id, (key::VARIANTS): variants }))
        .collect();
    out.insert(key::SCREENS.into(), Value::Array(screens));
    let pages = studio_pages(&Value::Object(out.clone()));
    out.insert(key::STUDIO_PAGES.into(), Value::Array(pages));
    Ok(Value::Object(out))
}

/// Add `designedStates` to each variant whose file was read: which of the screen's declared states
/// (its own, else the manifest's) the variant designs. The Studio draws only those state frames;
/// an undesigned state would repeat the default screen.
fn mark_designed_states(
    root: &Path,
    out: &mut Map<String, Value>,
    files: &HashMap<String, String>,
) -> Result<(), DfwError> {
    let top_states = out.get(key::STATES).cloned().unwrap_or(json!([]));
    if let Some(Value::Array(list)) = out.get_mut(key::SCREENS) {
        for s in list.iter_mut() {
            let declared: Vec<String> = match s.get(key::STATES) {
                Some(Value::Array(own)) if !own.is_empty() => own.clone(),
                _ => top_states.as_array().cloned().unwrap_or_default(),
            }
            .iter()
            .filter_map(|v| v.as_str().map(str::to_string))
            .collect();
            let Some(Value::Array(variants)) = s.get_mut(key::VARIANTS) else {
                continue;
            };
            for v in variants.iter_mut() {
                let text = v
                    .get(key::FILE)
                    .and_then(Value::as_str)
                    .and_then(|f| files.get(f));
                let (Some(text), Value::Object(map)) = (text, v) else {
                    continue;
                };
                let designed = designed_states(root, text, &declared)?;
                map.insert(key::DESIGNED_STATES.into(), json!(designed));
            }
        }
    }
    Ok(())
}

/// `tokenList()`: `[{name, var, type}]` for the inspector, from tokens.json (typography split
/// into its parts) plus any other variable declared in tokens.css.
pub fn token_list(root: &Path) -> Result<Value, DfwError> {
    const SUB: [(&str, &str); 5] = [
        ("fontFamily", "font-family"),
        ("fontSize", "font-size"),
        ("fontWeight", "font-weight"),
        ("letterSpacing", "letter-spacing"),
        ("lineHeight", "line-height"),
    ];
    let mut out: Vec<Value> = Vec::new();
    let mut seen: Vec<String> = Vec::new();
    for t in load_token_names(root)? {
        let var = t.var();
        let object = matches!(t.value, Value::Object(_) | Value::Array(_));
        if t.is_type("typography") && object {
            for (k, sub) in SUB {
                if t.value.get(k).is_some() {
                    out.push(json!({ (token_entry::NAME): format!("{}.{sub}", t.name), (token_entry::VAR): format!("{var}-{sub}"), (token_entry::TYPE): "typography" }));
                }
            }
        } else {
            let ty =
                t.ty.clone()
                    .filter(|v| js::truthy(v))
                    .unwrap_or(Value::Null);
            out.push(json!({ (token_entry::NAME): t.name, (token_entry::VAR): var, (token_entry::TYPE): ty }));
        }
    }
    for x in &out {
        seen.push(js::to_string(x.get(token_entry::VAR)));
    }
    let css = project_paths(root).tokens_css;
    if css.exists() {
        let text = js::read_text(&css)?;
        let decl = TOKEN_DECL
            .as_ref()
            .ok_or_else(|| DfwError::internal("the token declaration pattern does not compile"))?;
        for c in decl.captures_iter(&text) {
            let Some(name) = c.get(1).map(|m| m.as_str()) else {
                continue;
            };
            if seen.iter().any(|s| s == name) {
                continue;
            }
            seen.push(name.to_string());
            out.push(json!({ (token_entry::NAME): name[2..].replace('-', "."), (token_entry::VAR): name, (token_entry::TYPE): null }));
        }
    }
    Ok(Value::Array(out))
}

/// `/__dfw/api/components`: the masters without their styles, template and examples.
pub fn components(root: &Path) -> Result<Value, DfwError> {
    let lib = load_components(root)?;
    let list = lib
        .iter()
        .map(|d| {
            let mut v = d.to_json();
            if let Some(map) = v.as_object_mut() {
                for k in ["styles", "template", "examples"] {
                    map.shift_remove(k);
                }
            }
            v
        })
        .collect();
    Ok(Value::Array(list))
}

/// `prepareHtml(html)`: compose `<dfw-use>`/`<dfw-icon>`/`<dfw-head>`, link the component CSS
/// when instances need it, and inject the overlay before the last `</body>`.
pub fn prepare_html(root: &Path, html: &str) -> Result<String, DfwError> {
    let r = compose_html(
        root,
        html,
        ComposeOptions {
            lib: None,
            design_base: Some(DESIGN_BASE),
            prop_values: true,
        },
    )?;
    let mut out = r.html;
    if !r.instances.is_empty() && !out.contains(in_design::COMPONENTS_CSS) {
        let link = format!(
            r#"<link rel="stylesheet" href="{DESIGN_BASE}{}">"#,
            in_design::COMPONENTS_CSS
        );
        out = match find_ci(&out, "</head>", false) {
            Some(i) => format!("{}{link}{}", &out[..i], &out[i..]),
            None => link + &out,
        };
    }
    Ok(match find_ci(&out, "</body>", true) {
        Some(i) => format!("{}{OVERLAY_TAG}</body>{}", &out[..i], &out[i + 7..]),
        None => out + OVERLAY_TAG,
    })
}

/// First (or last) position of an ASCII `needle` in `hay`, ignoring ASCII case.
pub fn find_ci(hay: &str, needle: &str, last: bool) -> Option<usize> {
    let lower = hay.to_ascii_lowercase();
    if last {
        lower.rfind(needle)
    } else {
        lower.find(needle)
    }
}

/// `fallbackSystemPage()`: the plain list of masters, used when the Design System page fails.
pub fn fallback_system_page(root: &Path) -> Result<String, DfwError> {
    let lib = load_components(root)?;
    let sections: Vec<String> = lib
        .iter()
        .map(|d| {
            let name = escape_html(d.display_name());
            format!(
                "<section id=\"c-{name}\" data-dfw-id=\"system.component.{name}\" style=\"padding:32px 0;border-top:1px solid #ddd\">\n  <h2 style=\"margin:0 0 4px\">{name}</h2><p style=\"margin:0 0 16px;color:#666\">{}</p>\n  <div style=\"display:flex;flex-wrap:wrap;gap:16px;align-items:center\">{}</div></section>",
                escape_html(&d.description),
                d.examples
            )
        })
        .collect();
    let sections = if sections.is_empty() {
        format!(
            "<p>No components yet. Add masters to <code>{}/</code>.</p>",
            rel::COMPONENTS
        )
    } else {
        sections.join("\n")
    };
    Ok(format!(
        "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Design System</title>\n<link rel=\"stylesheet\" href=\"{DESIGN_BASE}{tokens_css}\"><link rel=\"stylesheet\" href=\"{DESIGN_BASE}{components_css}\">\n</head><body style=\"margin:0;padding:48px;font-family:system-ui,sans-serif\">\n<h1 data-dfw-id=\"system.title\">Design System</h1>\n{sections}\n</body></html>",
        tokens_css = in_design::TOKENS_CSS,
        components_css = in_design::COMPONENTS_CSS,
    ))
}

/// `systemPage()`: the Design System page prepared like a design file; when it cannot be built
/// the error goes to the log and the fallback page is served.
pub fn system_page(root: &Path) -> Result<String, DfwError> {
    let html = match render_system_page(root, DESIGN_BASE) {
        Ok(html) => html,
        Err(err) => {
            eprintln!("dfw studio: system page: {}", err.message());
            fallback_system_page(root)?
        }
    };
    prepare_html(root, &html)
}

/// What a request for a design file answers.
#[derive(Debug, PartialEq)]
pub enum DesignFile {
    /// 200 with this body and content type.
    Found(Vec<u8>, &'static str),
    /// 404.
    Missing,
}

/// `serveDesignFile(rel)`: HTML prepared for a frame, anything else as it is.
pub fn design_file(root: &Path, rel: &str) -> Result<DesignFile, DfwError> {
    let design = project_paths(root).design;
    // A path through a symlink out of design/ is not served (it could be any file of the user).
    let Some(abs) = safe_join(&design, rel) else {
        return Ok(DesignFile::Missing);
    };
    // Pages link the generated component CSS even before any component exists.
    if rel == in_design::COMPONENTS_CSS && !abs.exists() {
        return Ok(DesignFile::Found(
            b"/* no components yet */".to_vec(),
            "text/css; charset=utf-8",
        ));
    }
    if !abs.exists() || abs.is_dir() {
        return Ok(DesignFile::Missing);
    }
    let ext = extname(&abs.to_string_lossy()).to_lowercase();
    if ext == ".html" {
        let text = String::from_utf8_lossy(&fs::read(&abs)?).into_owned();
        let html = prepare_html(root, &text)?;
        return Ok(DesignFile::Found(
            html.into_bytes(),
            "text/html; charset=utf-8",
        ));
    }
    Ok(DesignFile::Found(
        fs::read(&abs)?,
        mime(&ext).unwrap_or("application/octet-stream"),
    ))
}

/// `shootFrames({ frames, fullPage })`: screenshot Studio frames to
/// `.dfw/shots/studio/<local time>/<name>.png`. The caller holds the "one at a time" flag.
pub fn shoot_frames(root: &Path, port: u16, body: &Value) -> Result<Value, DfwError> {
    let p = project_paths(root);
    let frames = body
        .get(shoot::FRAMES)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    // `fullPage = true` as a default parameter: only a missing value becomes true.
    let full_page = body.get(shoot::FULL_PAGE).is_none_or(js::truthy);
    let list: Vec<&Value> = frames
        .iter()
        .filter(|f| {
            f.get(shoot::SRC)
                .and_then(Value::as_str)
                .is_some_and(|s| s.starts_with(DESIGN_BASE) || s == paths::SYSTEM_PAGE)
        })
        .take(200)
        .collect();
    if list.is_empty() {
        return Err(DfwError::new("Nothing to screenshot."));
    }
    let dir = new_run_dir(&p.shots.join(file::SHOTS_STUDIO), &local_stamp())?;
    let size = |v: Option<&Value>, fallback: f64| -> u32 {
        let n = js::to_number(v);
        let n = if n.is_nan() || n == 0.0 { fallback } else { n };
        n.clamp(200.0, 4000.0) as u32
    };
    let jobs: Vec<Job> = list
        .iter()
        .map(|f| {
            let name = js::to_string(
                f.get(shoot::NAME)
                    .filter(|n| js::truthy(n))
                    .or(Some(&json!("frame"))),
            );
            Job {
                url: format!(
                    "http://127.0.0.1:{port}{}",
                    js::to_string(f.get(shoot::SRC))
                ),
                width: size(f.get(shoot::W), 1440.0),
                height: size(f.get(shoot::H), 900.0),
                out: dir.join(format!("{}.png", frame_name(&name))),
                full_page,
                selector: None,
                scale: 1.0,
                inside: Some(root.to_path_buf()),
            }
        })
        .collect();
    let results = screenshot(&jobs, CONCURRENCY)?;
    let out = results
        .into_iter()
        .map(|r| {
            let rel = to_posix(&relative_path(&p.dfw, &r.out));
            let mut m = Map::new();
            m.insert(
                shot::NAME.into(),
                json!(r.out.file_name().map(|n| n.to_string_lossy().into_owned())),
            );
            m.insert(shot::OK.into(), json!(r.ok));
            if let Some(e) = &r.error {
                m.insert(shot::ERROR.into(), json!(e));
            }
            m.insert(shot::FILE.into(), json!(relative_path(root, &r.out)));
            m.insert(
                shot::URL.into(),
                if r.ok {
                    json!(format!("{}{rel}", paths::PREFIX))
                } else {
                    Value::Null
                },
            );
            Value::Object(m)
        })
        .collect();
    Ok(Value::Array(out))
}

/// A new folder `<base>/<stamp>` for one screenshot run; `<stamp>-2`, `-3`... when a run in
/// the same second already has one, so two runs never overwrite each other's files.
fn new_run_dir(base: &Path, stamp: &str) -> Result<PathBuf, DfwError> {
    fs::create_dir_all(base)?;
    for n in 1..1000 {
        let name = if n == 1 {
            stamp.to_string()
        } else {
            format!("{stamp}-{n}")
        };
        let dir = base.join(name);
        // create_dir fails when the folder exists: the claim is atomic between two runs.
        match fs::create_dir(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
    }
    Err(DfwError::new(format!(
        "Too many screenshot runs named {stamp} in {}; remove old ones.",
        base.display()
    )))
}

/// `String(name).replace(/[^\w.-]+/g, '_').slice(0, 120)`.
pub fn frame_name(name: &str) -> String {
    let mut out = String::new();
    let mut run = false;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-' {
            out.push(c);
            run = false;
        } else if !run {
            out.push('_');
            run = true;
        }
    }
    out.chars().take(120).collect()
}

/// The local time as `2026-11-19T10-20-30` (the folder name of a Studio screenshot run).
fn local_stamp() -> String {
    // std knows no time zone; `date` does. Without it (Windows) the time is UTC.
    let plan = crate::core::process::Spawn::direct("date", vec!["+%Y-%m-%dT%H-%M-%S".into()]);
    let ran = crate::core::process::run_timed(&plan, None, &[], std::time::Duration::from_secs(2));
    let local =
        Some(ran.stdout.trim().to_string()).filter(|s| ran.status == Some(0) && s.len() == 19);
    local.unwrap_or_else(|| {
        let iso = crate::core::util::iso_from_millis(super::watch::now_ms());
        iso.replace([':', '.'], "-").chars().take(19).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// a design file or a screenshot reached through a symlink (file or folder) that leads
    /// out of its root is not served
    #[cfg(unix)]
    #[test]
    fn symlinks_out_of_the_base_are_refused() {
        use crate::core::fixture::OUTSIDE_DIR;
        use crate::core::paths::file;
        /// A folder outside the project, and a file in it a link points at.
        const FOLDER: &str = "folder";
        const SECRET_FILE: &str = "secret.txt";
        /// What the files outside hold.
        const SECRET: &str = "secret";
        /// A link in `design/` to [`SECRET_FILE`], and one to [`FOLDER`].
        const LEAK: &str = "leak.txt";
        const LINKED: &str = "linked";
        /// A file of [`FOLDER`] through [`LINKED`].
        const LINKED_SECRET: &str = "linked/secret.png";
        /// A page through a link that stays inside `design/`.
        const ALIAS_PAGE: &str = "alias/a.html";
        let dir = crate::core::util::test_dir("studio-symlink");
        let outside = dir.join(OUTSIDE_DIR);
        fs::create_dir_all(outside.join(FOLDER)).unwrap();
        fs::write(outside.join(SECRET_FILE), SECRET).unwrap();
        fs::write(outside.join("folder/secret.png"), SECRET).unwrap();
        let design = dir.join("project/design");
        fs::create_dir_all(design.join(file::SCREENS)).unwrap();
        fs::write(design.join("screens/a.html"), "<p>a</p>").unwrap();
        std::os::unix::fs::symlink(outside.join(SECRET_FILE), design.join(LEAK)).unwrap();
        std::os::unix::fs::symlink(outside.join(FOLDER), design.join(LINKED)).unwrap();
        // A link that stays inside is fine.
        std::os::unix::fs::symlink(design.join(file::SCREENS), design.join("alias")).unwrap();
        assert_eq!(safe_join(&design, LEAK), None);
        assert_eq!(safe_join(&design, LINKED_SECRET), None);
        assert_eq!(safe_join(&design, LINKED), None);
        assert_eq!(safe_join(&design, ""), None);
        assert!(safe_join(&design, ALIAS_PAGE).is_some());
        assert!(
            safe_join(&design, "../../outside/secret.txt").is_none_or(|p| p.starts_with(&design))
        );
        let root = dir.join("project");
        assert_eq!(design_file(&root, LEAK).unwrap(), DesignFile::Missing);
        assert_eq!(
            design_file(&root, LINKED_SECRET).unwrap(),
            DesignFile::Missing
        );
        assert!(matches!(
            design_file(&root, ALIAS_PAGE).unwrap(),
            DesignFile::Found(..)
        ));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn safe_join_stays_inside_the_base() {
        assert_eq!(clamp_rel("../../etc/passwd"), ["etc", "passwd"]);
        assert_eq!(clamp_rel("a/./b/../c"), ["a", "c"]);
        assert!(clamp_rel("..").is_empty());
        assert_eq!(clamp_rel("a\\..\\..\\b"), ["b"]);
    }

    #[test]
    fn extname_and_mime_follow_node() {
        assert_eq!(extname("a/b.HTML"), ".HTML");
        assert_eq!(extname("a/.env"), "");
        assert_eq!(extname("a.b/c"), "");
        assert_eq!(mime(".woff2"), Some("font/woff2"));
        assert_eq!(mime(".txt"), None);
    }

    /// two screenshot runs in the same second get two folders
    #[test]
    fn screenshot_runs_in_one_second_do_not_share_a_folder() {
        /// The second the three runs start in.
        const SECOND: &str = "2026-11-19T10-20-30";
        let dir = crate::core::util::test_dir("studio-shot-runs");
        let a = new_run_dir(&dir, SECOND).unwrap();
        let b = new_run_dir(&dir, SECOND).unwrap();
        let c = new_run_dir(&dir, SECOND).unwrap();
        assert_eq!(a, dir.join(SECOND));
        assert_eq!(b, dir.join(format!("{SECOND}-2")));
        assert_eq!(c, dir.join(format!("{SECOND}-3")));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn frame_names_are_safe_file_names() {
        assert_eq!(frame_name("home / a @ 1440"), "home_a_1440");
        let safe = "x.y-z_1";
        assert_eq!(frame_name(safe), safe);
        assert_eq!(frame_name(&"é".repeat(5)), "_");
        assert_eq!(frame_name(&"a".repeat(200)).len(), 120);
    }

    #[test]
    fn the_overlay_goes_before_the_last_body_end() {
        let dir = crate::core::util::test_dir("studio-prepare");
        let html = prepare_html(
            &dir,
            "<html><body><p>a</p></BODY><!-- </body> --></body></html>",
        )
        .unwrap();
        assert_eq!(
            html,
            format!("<html><body><p>a</p></BODY><!-- </body> -->{OVERLAY_TAG}</body></html>")
        );
        assert_eq!(
            prepare_html(&dir, "<p>x</p>").unwrap(),
            format!("<p>x</p>{OVERLAY_TAG}")
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
