//! Ports the export half of `core/lib/penpot.mjs` (`simplify`, `signature`, `buildExport`,
//! `exportPenpot`, `pruneExports`, `exportStructure`, `exportSummary`).
//!
//! Every screen x variant x breakpoint is rendered in headless Chrome and read back as a layer
//! tree by `snapshot_page.js`, which runs inside the page and stays JavaScript. The result
//! (`.dfw/penpot/export.json`) is what the dfw Penpot plugin turns into boards. dfw's HTML stays
//! the source of truth.
// Over 800 lines: the layer builder reads every style a snapshot can carry, and the render
// loop, the page list and the two text outputs (structure, summary) are the same job (export).

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{LazyLock, Mutex};
use std::thread;

use regex::Regex;
use serde_json::{json, Map, Value};

use super::tokens::{
    dfw_to_penpot, flatten_tokens, jv, parse_color, read_dfw_tokens, token_to_penpot_quiet,
    DfwTokens, Flat, Token,
};
use crate::browser::launch::{browser_status, fonts_ready, open_renderer, Renderer};
use crate::core::config::{js_spread, load_config};
use crate::core::error::DfwError;
use crate::core::paths::{project_paths, resolve, to_posix};
use crate::core::util::{get_path, now_iso, read_json, sha256, to_json_compact, write_json};
use crate::design::compose::{build_composed, build_system};
use crate::design::tokens::{dir_names, js, relative_path};

pub(super) const EXPORT_SCHEMA: &str = "dfw-penpot-export/1";
pub(super) const CHANGES_SCHEMA: &str = "dfw-penpot-changes/1";
const BOARD_GAP_X: f64 = 160.0;
const BOARD_GAP_Y: f64 = 240.0;
const KEEP_EXPORTS: usize = 20;

/// `snapshotPage` of penpot.mjs, byte for byte: it runs inside the rendered page
/// (self-contained, no closures) and returns a raw layer tree with page coordinates, colours
/// normalised to `{ hex, opacity }` and box shadows parsed.
const SNAPSHOT_PAGE: &str = include_str!("snapshot_page.js");

/// `.dfw/penpot` of a project.
pub(super) fn penpot_dir(root: &Path) -> PathBuf {
    project_paths(root).dfw.join("penpot")
}

/* ------------------------------------------------------------------ normalising snapshots */

fn style<'a>(n: &'a Value, key: &str) -> Option<&'a Value> {
    n.get("style").and_then(|s| s.get(key))
}

fn painted(n: &Value) -> bool {
    ["fill", "borders", "shadows"]
        .iter()
        .any(|k| jv::truthy(style(n, k)))
}

fn is_wrapper(n: &Value) -> bool {
    n.get("kind").and_then(Value::as_str) == Some("frame")
        && !jv::truthy(n.get("dfwId"))
        && !jv::truthy(n.get("component"))
        && !painted(n)
        && !jv::truthy(style(n, "clip"))
        && style(n, "opacity").is_none()
        && !jv::truthy(style(n, "backgroundImage"))
}

fn has_children(n: &Value) -> bool {
    n.get("children")
        .and_then(Value::as_array)
        .is_some_and(|c| !c.is_empty())
}

/// `simplify`: hoist plain layout wrappers (no id, no component, nothing painted) and drop
/// empty frames.
pub fn simplify(nodes: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for mut n in nodes {
        if let Some(children) = n.get_mut("children").filter(|c| jv::truthy(Some(c))) {
            let kids = match children.take() {
                Value::Array(kids) => kids,
                _ => Vec::new(),
            };
            *children = Value::Array(simplify(kids));
        }
        if is_wrapper(&n) {
            if let Some(Value::Array(kids)) = n.get_mut("children").map(Value::take) {
                out.extend(kids);
            }
        } else if n.get("kind").and_then(Value::as_str) == Some("frame")
            && !has_children(&n)
            && !jv::truthy(n.get("dfwId"))
            && !jv::truthy(n.get("component"))
            && !painted(&n)
        {
            continue;
        } else {
            out.push(n);
        }
    }
    out
}

static PLAIN_ALIAS: LazyLock<Regex> =
    // A constant pattern: known to compile.
    LazyLock::new(|| Regex::new(r"^\{([^{}]+)\}$").expect("constant pattern"));
static SCALE_STEP: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\.[0-9]+$").expect("constant pattern"));
static RADIUS_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(^|\.)radius(\.|$)").expect("constant pattern"));
static TEXTISH: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(^|[.-])(ink|text|on|fg|foreground|muted|subtle)([.-]|$)")
        .expect("constant pattern")
});
static EXPORT_FILE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^px_[0-9A-Za-z_]+\.json$").expect("constant pattern"));

/// `resolveAlias`: the token a name ends at, aliases followed (`"{a.b}"` to `a.b` and on);
/// `None` when it is missing or circular.
pub(super) fn resolve_alias<'a>(flat: &'a Flat, name: &str) -> Option<&'a Token> {
    let mut name = name.to_string();
    for _ in 0..=10 {
        let token = flat.get(&name)?;
        let target = token
            .value
            .as_str()
            .and_then(|s| PLAIN_ALIAS.captures(js::trim(s)));
        match target {
            Some(m) => name = m[1].to_string(),
            None => return Some(token),
        }
    }
    None
}

/// A colour token resolved to its hex and opacity.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct ColorToken {
    pub name: String,
    pub hex: String,
    pub opacity: f64,
    score: f64,
}

/// Colour tokens resolved to hex and opacity (aliases followed), most semantic name first.
fn color_token_index(tree: Option<&Value>) -> Vec<ColorToken> {
    let empty = json!({});
    let flat = flatten_tokens(jv::truthy_value(tree).unwrap_or(&empty));
    let mut out = Vec::new();
    for (name, t) in flat.iter() {
        if t.type_name() != Some("color") {
            continue;
        }
        let Some(resolved) = resolve_alias(&flat, name) else {
            continue;
        };
        let penpot = token_to_penpot_quiet(name, resolved);
        let Some(c) = penpot.value().and_then(|v| parse_color(&jv::text(Some(v)))) else {
            continue;
        };
        let alias = t.value.is_string();
        // Prefer semantic names (aliases, no numeric scale step) over palette steps.
        let score = (if alias { 0.0 } else { 2.0 })
            + (if SCALE_STEP.is_match(name) { 4.0 } else { 0.0 })
            + name.split('.').count() as f64 * 0.1;
        out.push(ColorToken {
            name: name.to_string(),
            hex: jv::text(c.get("hex")),
            opacity: js::present(c.get("alpha")).map_or(1.0, |a| jv::num(Some(a))),
            score,
        });
    }
    out.sort_by(|a, b| a.score.total_cmp(&b.score));
    out
}

/// Radius tokens in px: name and value.
fn radius_token_index(tree: Option<&Value>) -> Vec<(String, Value)> {
    let empty = json!({});
    let flat = flatten_tokens(jv::truthy_value(tree).unwrap_or(&empty));
    flat.iter()
        .filter(|(name, t)| {
            t.type_name() == Some("dimension")
                && RADIUS_NAME.is_match(name)
                && t.value.get("unit").and_then(Value::as_str) == Some("px")
        })
        .filter_map(|(name, t)| Some((name.to_string(), t.value.get("value")?.clone())))
        .collect()
}

/// Token with exactly this colour; text colours prefer ink/text/on-* names, fills the others.
fn find_color_token(index: &[ColorToken], c: Option<&Value>, text_role: bool) -> Value {
    let Some(c) = jv::truthy_value(c) else {
        return Value::Null;
    };
    let (hex, opacity) = (
        c.get("hex").and_then(Value::as_str),
        jv::num(c.get("opacity")),
    );
    let hits: Vec<&ColorToken> = index
        .iter()
        .filter(|t| Some(t.hex.as_str()) == hex && (t.opacity - opacity).abs() < 0.01)
        .collect();
    hits.iter()
        .find(|t| text_role == TEXTISH.is_match(&t.name))
        .or(hits.first())
        .filter(|t| !t.name.is_empty())
        .map_or(Value::Null, |t| t.name.as_str().into())
}

struct Linker<'a> {
    colors: &'a [ColorToken],
    radii: &'a [(String, Value)],
    project_root: Option<&'a Path>,
}

/// `fileURLToPath` for the `file:` URLs Chrome reports.
fn file_url_to_path(url: &str) -> PathBuf {
    let rest = url.strip_prefix("file:").unwrap_or(url);
    // An authority (`//host`) comes before the path; a local file has an empty one.
    let rest = match rest.strip_prefix("//") {
        Some(after) => &after[after.find('/').unwrap_or(after.len())..],
        None => rest,
    };
    let rest = &rest[..rest.find(['?', '#']).unwrap_or(rest.len())];
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| rest.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(byte) => {
                out.push(byte);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    let path = String::from_utf8_lossy(&out).into_owned();
    // "/C:/x" is "C:\x" on Windows.
    if cfg!(windows) {
        PathBuf::from(path.trim_start_matches('/').replace('/', "\\"))
    } else {
        PathBuf::from(path)
    }
}

/// `pathToFileURL(path).href`.
fn path_to_file_url(path: &Path) -> String {
    let text = resolve(path).to_string_lossy().replace('\\', "/");
    let mut url = String::from("file://");
    if !text.starts_with('/') {
        url.push('/');
    }
    for byte in text.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(
                byte,
                b'/' | b'-'
                    | b'_'
                    | b'.'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b':'
                    | b'@'
            );
        if keep {
            url.push(byte as char);
        } else {
            url.push_str(&format!("%{byte:02X}"));
        }
    }
    url
}

impl Linker<'_> {
    /// Turn raw snapshot nodes into export layers: fills and strokes with token links, paths.
    fn layers(&self, nodes: &[Value]) -> Vec<Value> {
        nodes
            .iter()
            .enumerate()
            .map(|(i, n)| self.layer(n, &i.to_string()))
            .collect()
    }

    // Long: one step per property of a snapshot node, in the key order of the export file.
    fn layer(&self, n: &Value, path: &str) -> Value {
        let kind = n.get("kind").and_then(Value::as_str);
        let st = |key: &str| style(n, key);
        let mut l = Map::new();
        let mut put = |key: &str, value: Option<Value>| {
            if let Some(value) = value {
                l.insert(key.to_string(), value);
            }
        };
        put("kind", n.get("kind").cloned());
        put("name", n.get("name").cloned());
        put("path", Some(path.into()));
        put("dfwId", Some(jv::or_null(n.get("dfwId"))));
        put("anchor", Some(jv::or_null(n.get("anchor"))));
        for key in ["x", "y", "w", "h"] {
            put(key, n.get(key).cloned());
        }
        put("tag", jv::truthy_value(n.get("tag")).cloned());
        if let Some(fill) = jv::truthy_value(st("fill")) {
            put(
                "fills",
                Some(json!([jv::obj(vec![
                    ("color", fill.get("hex").cloned()),
                    ("opacity", fill.get("opacity").cloned()),
                    (
                        "token",
                        Some(find_color_token(self.colors, Some(fill), false))
                    ),
                ])])),
            );
        }
        if let Some(borders) = jv::truthy_value(st("borders")) {
            let sides: &[Value] = borders.as_array().map_or(&[], Vec::as_slice);
            let top = sides.first();
            fn hex(b: Option<&Value>) -> Option<&Value> {
                b.and_then(|b| b.get("color")).and_then(|c| c.get("hex"))
            }
            let uniform = !sides.is_empty()
                && sides.iter().all(|x| {
                    jv::truthy(Some(x))
                        && jv::strict_eq(x.get("w"), top.and_then(|t| t.get("w")))
                        && jv::strict_eq(hex(Some(x)), hex(top))
                });
            if uniform {
                let color = top.and_then(|t| t.get("color"));
                let line = top.and_then(|t| t.get("style")).and_then(Value::as_str);
                let line = line
                    .filter(|s| matches!(*s, "dashed" | "dotted"))
                    .unwrap_or("solid");
                put(
                    "strokes",
                    Some(json!([jv::obj(vec![
                        ("color", hex(top).cloned()),
                        ("opacity", color.and_then(|c| c.get("opacity")).cloned()),
                        ("width", top.and_then(|t| t.get("w")).cloned()),
                        ("style", Some(line.into())),
                        ("token", Some(find_color_token(self.colors, color, false))),
                    ])])),
                );
            } else {
                // Penpot strokes are uniform: these are drawn as thin rectangles.
                put(
                    "sideBorders",
                    Some(jv::obj(vec![
                        ("top", sides.first().cloned()),
                        ("right", sides.get(1).cloned()),
                        ("bottom", sides.get(2).cloned()),
                        ("left", sides.get(3).cloned()),
                    ])),
                );
            }
        }
        if let Some(radius) = jv::truthy_value(st("radius")) {
            put("radius", Some(radius.clone()));
            let corners: &[Value] = radius.as_array().map_or(&[], Vec::as_slice);
            let first = corners.first();
            if corners.iter().all(|x| jv::strict_eq(Some(x), first)) {
                let token = self
                    .radii
                    .iter()
                    .find(|(_, px)| jv::strict_eq(Some(px), first))
                    .filter(|(name, _)| !name.is_empty());
                put("radiusToken", token.map(|(name, _)| name.as_str().into()));
            }
        }
        if let Some(shadows) = jv::truthy_value(st("shadows")).and_then(Value::as_array) {
            let list: Vec<Value> = shadows
                .iter()
                .map(|s| {
                    let mut shadow = js_spread(Some(s));
                    let color = jv::truthy_value(s.get("color"))
                        .cloned()
                        .unwrap_or_else(|| json!({ "hex": "#000000", "opacity": 0.25 }));
                    shadow.insert("color".into(), color);
                    Value::Object(shadow)
                })
                .collect();
            put("shadows", Some(Value::Array(list)));
        }
        put("opacity", st("opacity").cloned());
        put("clip", jv::truthy(st("clip")).then_some(Value::Bool(true)));
        if let Some(image) = jv::truthy_value(st("backgroundImage")) {
            let shown = jv::slice16(&jv::text(Some(image)), 0, 60);
            put(
                "note",
                Some(format!("background-image not exported ({shown})").into()),
            );
        }
        if kind == Some("text") {
            put("characters", n.get("characters").cloned());
            let text = n.get("text");
            let color = text.and_then(|t| t.get("color"));
            let mut t = js_spread(text);
            match color {
                Some(c) => {
                    t.insert("color".into(), c.clone());
                }
                None => {
                    t.shift_remove("color");
                }
            }
            t.insert(
                "colorToken".into(),
                find_color_token(self.colors, color, true),
            );
            put("text", Some(Value::Object(t)));
            put(
                "placeholder",
                jv::truthy(n.get("placeholder")).then_some(Value::Bool(true)),
            );
        }
        if kind == Some("svg") {
            put("svg", n.get("svg").cloned());
            put("icon", jv::truthy_value(n.get("icon")).cloned());
        }
        if kind == Some("image") {
            put("alt", n.get("alt").cloned());
            put("src", Some(self.image_src(n.get("src"))));
        }
        if let Some(component) = jv::truthy_value(n.get("component")) {
            let mut c = js_spread(Some(component));
            c.insert("key".into(), component_key(component).into());
            put("component", Some(Value::Object(c)));
        }
        if let Some(children) = n.get("children").and_then(Value::as_array) {
            if !children.is_empty() {
                let kids = children
                    .iter()
                    .enumerate()
                    .map(|(i, c)| self.layer(c, &format!("{path}.{i}")))
                    .collect();
                put("children", Some(Value::Array(kids)));
            }
        }
        let mut l = Value::Object(l);
        if l.get("children").is_some() || kind == Some("frame") {
            l["sig"] = signature(&l).into();
        }
        l
    }

    /// Where the plugin loads an image from: a file under `design/` through the Studio, or a
    /// web address as it is.
    fn image_src(&self, src: Option<&Value>) -> Value {
        let Some(src) = jv::truthy_value(src).and_then(Value::as_str) else {
            return Value::Null;
        };
        match self.project_root {
            Some(root) if src.starts_with("file:") => {
                let design = project_paths(root).design;
                let rel = to_posix(&relative_path(&design, &file_url_to_path(src)));
                if rel.starts_with("..") {
                    Value::Null
                } else {
                    format!("/design/{rel}").into()
                }
            }
            _ if src.starts_with("http:") || src.starts_with("https:") => src.into(),
            _ => Value::Null,
        }
    }
}

fn component_key(c: &Value) -> String {
    let part = |key: &str| jv::truthy_value(c.get(key)).map(|v| jv::text(Some(v)));
    // The name is joined as it is (a missing one is empty), variant and size only when set.
    let name = match c.get("name") {
        None | Some(Value::Null) => String::new(),
        name => jv::text(name),
    };
    [
        name,
        part("variant").unwrap_or_default(),
        part("size").unwrap_or_default(),
    ]
    .join("|")
}

/// `signature`: the structure of a layer (kinds, names, child structure; not text, positions
/// or colours).
pub fn signature(l: &Value) -> String {
    let own = match jv::truthy_value(l.get("component")) {
        Some(c) => jv::text(c.get("key")),
        None => jv::text(l.get("name")),
    };
    let children: Vec<String> = l
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| match c.get("sig") {
            Some(Value::Null) => String::new(),
            Some(sig) => jv::text(Some(sig)),
            None => format!("{}:{}", jv::text(c.get("kind")), jv::text(c.get("name"))),
        })
        .collect();
    format!("{}:{own}[{}]", jv::text(l.get("kind")), children.join(","))
}

/// Rebase a subtree to its own origin (component main).
fn rebase(l: &Value, ox: f64, oy: f64) -> Value {
    let mut c = js_spread(Some(l));
    let moved =
        |v: Option<&Value>, origin: f64| js::num(js::round((jv::num(v) - origin) * 100.0) / 100.0);
    c.insert("x".into(), moved(l.get("x"), ox));
    c.insert("y".into(), moved(l.get("y"), oy));
    if let Some(children) = jv::truthy_value(l.get("children")).and_then(Value::as_array) {
        let kids = children.iter().map(|ch| rebase(ch, ox, oy)).collect();
        c.insert("children".into(), Value::Array(kids));
    }
    Value::Object(c)
}

/// What [`build_export`] builds from.
pub struct ExportInput<'a> {
    /// `[{ page: { id, title }, screen, variant, breakpoint: { id, width, height }, file, snapshot }]`
    pub renders: &'a [Value],
    pub tokens: Option<&'a DfwTokens>,
    pub product: Value,
    pub design_version: String,
    /// The project, so images under `design/` get a Studio path.
    pub project_root: Option<&'a Path>,
    pub generated_at: String,
}

/// `buildExport`: the export document from rendered snapshots. Its id carries the render time
/// and a hash of the pages.
// Long: the document is built in the key order its hash (the export id) is taken over.
pub fn build_export(input: &ExportInput) -> Value {
    let base = input.tokens.map(|t| &t.base);
    let colors = color_token_index(base);
    let radii = radius_token_index(base);
    let linker = Linker {
        colors: &colors,
        radii: &radii,
        project_root: input.project_root,
    };
    let mut components: Vec<(String, Value)> = Vec::new();
    // Pages by id, in the order of their first render.
    let mut pages: Vec<(String, Map<String, Value>, Vec<Value>)> = Vec::new();
    for r in input.renders {
        let page = r.get("page");
        let page_id = page.and_then(|p| p.get("id"));
        let page_key = jv::stringify(page_id);
        if !pages.iter().any(|(k, _, _)| *k == page_key) {
            let title = jv::truthy_value(page.and_then(|p| p.get("title"))).or(page_id);
            let Value::Object(head) =
                jv::obj(vec![("id", page_id.cloned()), ("title", title.cloned())])
            else {
                continue;
            };
            pages.push((page_key.clone(), head, Vec::new()));
        }
        let snapshot = r.get("snapshot");
        let nodes = snapshot
            .and_then(|s| s.get("children"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let layers = linker.layers(&simplify(nodes));
        let breakpoint = r.get("breakpoint");
        let bp = |key: &str| breakpoint.and_then(|b| b.get(key));
        let background = snapshot.and_then(|s| s.get("background"));
        let key = format!(
            "{}|{}|{}",
            jv::text(r.get("screen")),
            jv::text(r.get("variant")),
            jv::text(bp("id"))
        );
        let height = js::max(
            jv::num(bp("height")),
            jv::num(snapshot.and_then(|s| s.get("height"))).ceil(),
        );
        for l in &layers {
            visit_components(l, &key, &mut components);
        }
        let board = jv::obj(vec![
            ("key", Some(key.as_str().into())),
            (
                "name",
                Some(
                    format!(
                        "{} / {} @ {}",
                        jv::text(r.get("screen")),
                        jv::text(r.get("variant")),
                        jv::text(bp("id"))
                    )
                    .into(),
                ),
            ),
            ("screen", r.get("screen").cloned()),
            ("variant", r.get("variant").cloned()),
            ("breakpoint", bp("id").cloned()),
            ("file", r.get("file").cloned()),
            ("width", bp("width").cloned()),
            ("height", Some(js::num(height))),
            ("viewportHeight", bp("height").cloned()),
            (
                "background",
                Some(jv::obj(vec![
                    ("color", background.and_then(|b| b.get("hex")).cloned()),
                    (
                        "opacity",
                        background.and_then(|b| b.get("opacity")).cloned(),
                    ),
                    ("token", Some(find_color_token(&colors, background, false))),
                ])),
            ),
            ("layers", Some(Value::Array(layers))),
        ]);
        if let Some(slot) = pages.iter_mut().find(|(k, _, _)| *k == page_key) {
            slot.2.push(board);
        }
    }
    // Canvas layout per page: one row per breakpoint, screens/variants left to right.
    let pages: Vec<Value> = pages
        .into_iter()
        .map(|(_, mut head, mut boards)| {
            lay_out(&mut boards);
            head.insert("boards".into(), Value::Array(boards));
            Value::Object(head)
        })
        .collect();
    let token_file = input
        .tokens
        .map(|t| dfw_to_penpot(Some(&t.base), &t.themes));
    let stamp: String = input
        .generated_at
        .chars()
        .filter(|c| !matches!(c, '-' | ':' | '.' | 'T' | 'Z'))
        .collect();
    let pages = Value::Array(pages);
    let id = format!(
        "px_{}_{}",
        jv::slice16(&stamp, 0, 14),
        &sha256(to_json_compact(&pages))[..8]
    );
    json!({
        "schema": EXPORT_SCHEMA,
        "id": id,
        "generatedAt": input.generated_at,
        "product": input.product,
        "designVersion": input.design_version,
        "tokens": token_file.as_ref().map_or(Value::Null, |(doc, _)| doc.clone()),
        "tokenReport": token_file.as_ref().map_or(Value::Null, |(_, report)| {
            json!({ "unsupported": report.unsupported, "lossy": report.lossy })
        }),
        "colors": colors
            .iter()
            .map(|c| json!({ "name": c.name, "hex": c.hex, "opacity": js::num(c.opacity) }))
            .collect::<Vec<_>>(),
        "components": components.into_iter().map(|(_, c)| c).collect::<Vec<_>>(),
        "pages": pages,
    })
}

/// The first instance of every component becomes its main, rebased to its own origin.
fn visit_components(l: &Value, board_key: &str, components: &mut Vec<(String, Value)>) {
    if let Some(component) = jv::truthy_value(l.get("component")) {
        let key = jv::stringify(component.get("key"));
        if !components.iter().any(|(k, _)| *k == key) {
            let name: Vec<String> = ["name", "variant", "size"]
                .iter()
                .filter_map(|k| jv::truthy_value(component.get(*k)))
                .map(|v| jv::text(Some(v)))
                .collect();
            let main = jv::obj(vec![
                ("key", component.get("key").cloned()),
                ("name", Some(name.join(" / ").into())),
                ("component", component.get("name").cloned()),
                ("variant", component.get("variant").cloned()),
                ("size", component.get("size").cloned()),
                ("sig", l.get("sig").cloned()),
                (
                    "from",
                    Some(format!("{board_key}#{}", jv::text(l.get("path"))).into()),
                ),
                (
                    "root",
                    Some(rebase(l, jv::num(l.get("x")), jv::num(l.get("y")))),
                ),
            ]);
            components.push((key, main));
        }
    }
    for c in l
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        visit_components(c, board_key, components);
    }
}

fn lay_out(boards: &mut [Value]) {
    let mut rows: Vec<String> = Vec::new();
    for b in boards.iter() {
        let bp = jv::stringify(b.get("breakpoint"));
        if !rows.contains(&bp) {
            rows.push(bp);
        }
    }
    let mut y = 0.0;
    for bp in rows {
        let (mut x, mut row_height) = (0.0, 0.0);
        for b in boards
            .iter_mut()
            .filter(|b| jv::stringify(b.get("breakpoint")) == bp)
        {
            let (width, height) = (jv::num(b.get("width")), jv::num(b.get("height")));
            b["x"] = js::num(x);
            b["y"] = js::num(y);
            x += width + BOARD_GAP_X;
            row_height = js::max(row_height, height);
        }
        y += row_height + BOARD_GAP_Y;
    }
}

/* ------------------------------------------------------------------ export (render + write) */

/// `studioPages` of core/studio/pages.mjs, for what an export needs: the canvas pages that hold
/// screens (not the Design System page, not the flow page) as id, title and screen ids.
// TODO(merge): use studio::pages::studio_pages
fn screen_pages(manifest: &Value) -> Vec<(String, Value, Vec<String>)> {
    let list = |v: Option<&Value>| -> Vec<Value> {
        jv::truthy_value(v)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let id_of = |s: &Value| s.get("id").and_then(Value::as_str).map(str::to_string);
    let screens: Vec<Value> = list(manifest.get("screens"))
        .into_iter()
        .filter(|s| !jv::truthy(s.get("archived")))
        .collect();
    let ids: Vec<String> = screens.iter().filter_map(id_of).collect();
    let mut pages: Vec<(String, Value, Vec<String>)> = Vec::new();
    let mut assigned: Vec<String> = Vec::new();
    fn ensure(
        pages: &mut Vec<(String, Value, Vec<String>)>,
        id: &str,
        title: Option<&Value>,
    ) -> usize {
        if let Some(i) = pages.iter().position(|(p, _, _)| p == id) {
            return i;
        }
        let title = jv::truthy_value(title)
            .cloned()
            .unwrap_or_else(|| id.into());
        pages.push((id.to_string(), title, Vec::new()));
        pages.len() - 1
    }
    // Returns true when the screen went onto the page (it is then assigned).
    let add = |pages: &mut Vec<(String, Value, Vec<String>)>, page: usize, sid: &str| -> bool {
        if !ids.iter().any(|i| i == sid) || pages[page].2.iter().any(|s| s == sid) {
            return false;
        }
        pages[page].2.push(sid.to_string());
        true
    };
    for p in list(manifest.get("pages")) {
        let Some(id) = jv::truthy_value(p.get("id")).and_then(Value::as_str) else {
            continue;
        };
        if id == "system" {
            continue;
        }
        let page = ensure(&mut pages, id, p.get("title"));
        for sid in list(p.get("screens")) {
            if let Some(sid) = sid.as_str() {
                if add(&mut pages, page, sid) {
                    assigned.push(sid.to_string());
                }
            }
        }
        // Screens referenced only from sections ("id" or "id/variant") belong to the page too.
        for section in list(p.get("sections")) {
            for reference in list(section.get("screens")) {
                let reference = jv::text(Some(&reference));
                let sid = reference.split('/').next().unwrap_or_default();
                if add(&mut pages, page, sid) {
                    assigned.push(sid.to_string());
                }
            }
        }
    }
    for s in &screens {
        let (Some(id), Some(page)) = (
            id_of(s),
            jv::truthy_value(s.get("page")).and_then(Value::as_str),
        ) else {
            continue;
        };
        if page != "system" {
            let page = ensure(&mut pages, page, None);
            if add(&mut pages, page, &id) {
                assigned.push(id);
            }
        }
    }
    let rest: Vec<String> = ids
        .iter()
        .filter(|id| !assigned.contains(id))
        .cloned()
        .collect();
    let (explore, main): (Vec<String>, Vec<String>) =
        rest.into_iter().partition(|id| id.starts_with('_'));
    let title = |t: &str| Value::String(t.to_string());
    if !main.is_empty() || pages.is_empty() {
        for id in &main {
            let page = ensure(&mut pages, "screens", Some(&title("Screens")));
            add(&mut pages, page, id);
        }
    }
    if pages.is_empty() && main.is_empty() && explore.is_empty() {
        ensure(&mut pages, "screens", Some(&title("Screens")));
    }
    for id in &explore {
        let page = ensure(&mut pages, "explorations", Some(&title("Explorations")));
        add(&mut pages, page, id);
    }
    pages
}

/// The renders an export needs: one job per screen variant and breakpoint, as
/// `{ page, screen, variant, breakpoint, file }`.
fn select_renders(
    root: &Path,
    page: Option<&str>,
    bp: Option<&str>,
) -> Result<(Value, Vec<Value>), DfwError> {
    let p = project_paths(root);
    let m = read_json(&p.manifest, Some(Value::Null))?;
    if !jv::truthy(Some(&m)) {
        return Err(DfwError::new("design/design.json not found."));
    }
    let list = |v: Option<&Value>| -> Vec<Value> {
        v.and_then(Value::as_array).cloned().unwrap_or_default()
    };
    let all_bps = match jv::truthy_value(m.get("breakpoints")) {
        Some(bps) => list(Some(bps)),
        None => list(get_path(&load_config(Some(root))?, "studio.breakpoints")),
    };
    let bps: Vec<Value> = match bp.filter(|b| !b.is_empty()) {
        Some(bp) => {
            let wanted: Vec<&str> = bp.split(',').collect();
            all_bps
                .iter()
                .filter(|b| {
                    b.get("id")
                        .and_then(Value::as_str)
                        .is_some_and(|id| wanted.contains(&id))
                })
                .cloned()
                .collect()
        }
        None => all_bps.clone(),
    };
    if bps.is_empty() {
        let have: Vec<Value> = all_bps
            .iter()
            .map(|b| b.get("id").cloned().unwrap_or(Value::Null))
            .collect();
        return Err(DfwError::new(format!(
            "No breakpoint matches --bp {} (have: {}).",
            bp.unwrap_or("null"),
            js::join(&have, ", ")
        )));
    }
    let page = page.filter(|p| !p.is_empty());
    let pages: Vec<_> = screen_pages(&m)
        .into_iter()
        .filter(|(id, _, _)| page.is_none_or(|p| p == id))
        .collect();
    if let (Some(page), true) = (page, pages.is_empty()) {
        return Err(DfwError::new(format!(
            "No page \"{page}\" in design/design.json."
        )));
    }
    let screens = list(m.get("screens"));
    let mut jobs = Vec::new();
    for (id, title, screen_ids) in &pages {
        for sid in screen_ids {
            // The last screen with an id wins, as in a JavaScript Map.
            let screen = screens
                .iter()
                .rev()
                .find(|s| s.get("id").and_then(Value::as_str) == Some(sid));
            for v in list(screen.and_then(|s| jv::truthy_value(s.get("variants")))) {
                for b in &bps {
                    jobs.push(jv::obj(vec![
                        ("page", Some(json!({ "id": id, "title": title }))),
                        ("screen", Some(sid.as_str().into())),
                        ("variant", v.get("id").cloned()),
                        ("breakpoint", Some(b.clone())),
                        (
                            "file",
                            Some(format!("design/{}", jv::text(v.get("file"))).into()),
                        ),
                    ]));
                }
            }
        }
    }
    Ok((m, jobs))
}

/// What `dfw penpot export` takes from the command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportOptions {
    /// Only this dfw page.
    pub page: Option<String>,
    /// Only these breakpoints (ids, comma separated).
    pub bp: Option<String>,
    /// Where the export goes, relative to the project (default `.dfw/penpot/export.json`).
    pub out: Option<String>,
}

fn snapshot(renderer: &Renderer, job: &Value, file: Option<&PathBuf>) -> Result<Value, DfwError> {
    let size = |key: &str| {
        let n = jv::num(job.get("breakpoint").and_then(|b| b.get(key)));
        if n.is_finite() && n >= 0.0 {
            n as u32
        } else {
            0
        }
    };
    let page = renderer.page(size("width"), size("height"), 1.0)?;
    let result = (|| {
        let file = file.ok_or_else(|| {
            DfwError::internal("The \"path\" argument must be of type string. Received undefined")
        })?;
        page.goto(&path_to_file_url(file), 30_000)?;
        fonts_ready(&page, 3000)?;
        page.wait_for_timeout(40);
        page.evaluate(SNAPSHOT_PAGE.trim_end(), None)
    })();
    page.close();
    result
}

/// `exportPenpot`: render and write `.dfw/penpot/export.json`, a copy under `exports/<id>.json`
/// and `tokens.penpot.json`. Returns the file and the document; a render that failed is in
/// the document's `errors`.
pub fn export_penpot(root: &Path, opts: &ExportOptions) -> Result<(PathBuf, Value), DfwError> {
    if !browser_status().installed() {
        return Err(DfwError::new(
            "dfw penpot export renders in Chrome/Edge/Chromium: none found (dfw setup --browser).",
        ));
    }
    let (manifest, jobs) = select_renders(root, opts.page.as_deref(), opts.bp.as_deref())?;
    if jobs.is_empty() {
        return Err(DfwError::new(
            "No screens to export (check --page/--bp and design/design.json).",
        ));
    }
    build_system(root)?;
    let mut rels: Vec<String> = Vec::new();
    for job in &jobs {
        let rel = jv::text(job.get("file"));
        if !rels.contains(&rel) {
            rels.push(rel);
        }
    }
    let files = build_composed(root, &rels)?;
    let file_of = |job: &Value| {
        let rel = jv::text(job.get("file"));
        files.iter().find(|f| f.rel == rel).map(|f| &f.out)
    };
    let renderer = open_renderer()?;
    let results: Mutex<Vec<Option<Result<Value, DfwError>>>> =
        Mutex::new(jobs.iter().map(|_| None).collect());
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..jobs.len().min(4) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(i) else {
                    break;
                };
                let result = snapshot(&renderer, job, file_of(job));
                if let Ok(mut slots) = results.lock() {
                    slots[i] = Some(result);
                }
            });
        }
    });
    renderer.close();
    let results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut renders = Vec::new();
    let mut errors = Vec::new();
    for (job, result) in jobs.iter().zip(results) {
        // A worker that panicked left its job without a result.
        let result = result.unwrap_or_else(|| Err(DfwError::internal("render worker stopped")));
        match result {
            Ok(snapshot) => {
                let mut render = js_spread(Some(job));
                render.insert("snapshot".into(), snapshot);
                renders.push(Value::Object(render));
            }
            Err(err) => errors.push(Value::String(format!(
                "{}/{}@{}: {}",
                jv::text(job.get("screen")),
                jv::text(job.get("variant")),
                jv::text(job.get("breakpoint").and_then(|b| b.get("id"))),
                err.message()
            ))),
        }
    }
    let tokens = read_dfw_tokens(root, false).ok();
    let state = read_json(&project_paths(root).state, Some(json!({})))?;
    let version = js::present(state.get("design").and_then(|d| d.get("version")));
    let mut doc = build_export(&ExportInput {
        renders: &renders,
        tokens: tokens.as_ref(),
        product: jv::truthy_value(manifest.get("product"))
            .cloned()
            .unwrap_or_else(|| "".into()),
        design_version: format!(
            "v{}",
            version.map_or("1".to_string(), |v| jv::text(Some(v)))
        ),
        project_root: Some(root),
        generated_at: now_iso(),
    });
    doc["errors"] = Value::Array(errors);
    let dir = penpot_dir(root);
    write_json(
        &dir.join("exports")
            .join(format!("{}.json", jv::text(doc.get("id")))),
        &doc,
    )?;
    prune_exports(&dir, KEEP_EXPORTS)?;
    let file = match opts.out.as_deref().filter(|o| !o.is_empty()) {
        Some(out) => resolve(root.join(out)),
        None => resolve(dir.join("export.json")),
    };
    write_json(&file, &doc)?;
    if let Some(tokens) = jv::truthy_value(doc.get("tokens")) {
        write_json(&dir.join("tokens.penpot.json"), tokens)?;
    }
    Ok((file, doc))
}

/// `pruneExports`: keep the newest exports in `.dfw/penpot/exports/` (every render writes a
/// full copy). Ids start with the render time, so name order is age order. Returns the file
/// names removed.
pub fn prune_exports(dir: &Path, keep: usize) -> Result<Vec<String>, DfwError> {
    let d = dir.join("exports");
    if !d.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<String> = dir_names(&d)?
        .into_iter()
        .filter(|f| EXPORT_FILE.is_match(f))
        .collect();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    files.truncate(files.len().saturating_sub(keep));
    for f in &files {
        match fs::remove_file(d.join(f)) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(err.into()),
        }
    }
    Ok(files)
}

/// `exportStructure`: a geometry-free outline of an export (stable across platforms and
/// fonts), for tests.
// Only the tests read it today; the Studio group's export route may print it.
#[allow(dead_code)]
pub fn export_structure(doc: &Value) -> Vec<String> {
    fn list(v: Option<&Value>) -> impl Iterator<Item = &Value> {
        v.and_then(Value::as_array).into_iter().flatten()
    }
    // An element of `bits.join(' ')`: null and undefined are empty.
    fn bit(v: Option<&Value>) -> String {
        match v {
            None | Some(Value::Null) => String::new(),
            v => jv::text(v),
        }
    }
    let token_or = |v: Option<&Value>, other: &str| {
        let token = jv::truthy_value(v.and_then(|v| v.get("token")));
        jv::text(token.or(v.and_then(|v| v.get(other))))
    };
    fn row(
        l: &Value,
        indent: &str,
        lines: &mut Vec<String>,
        token_or: &dyn Fn(Option<&Value>, &str) -> String,
    ) {
        let mut bits = vec![
            bit(l.get("kind")),
            bit(l.get("name")),
            format!("path={}", jv::text(l.get("path"))),
        ];
        if let Some(anchor) = jv::truthy_value(l.get("anchor")) {
            if !jv::strict_eq(Some(anchor), l.get("name")) {
                bits.push(format!("anchor={}", jv::text(Some(anchor))));
            }
        }
        if let Some(c) = jv::truthy_value(l.get("component")) {
            bits.push(format!("component={}", jv::text(c.get("key"))));
        }
        if let Some(characters) = jv::truthy_value(l.get("characters")) {
            bits.push(to_json_compact(characters));
        }
        let first = |key: &str| jv::truthy_value(l.get(key).and_then(|v| v.get(0)));
        if let Some(fill) = first("fills") {
            bits.push(format!("fill={}", token_or(Some(fill), "color")));
        }
        if let Some(stroke) = first("strokes") {
            bits.push(format!("stroke={}", token_or(Some(stroke), "color")));
        }
        if let Some(Value::Object(sides)) = jv::truthy_value(l.get("sideBorders")) {
            let present: Vec<&str> = js::entries(sides)
                .into_iter()
                .filter(|(_, v)| jv::truthy(Some(v)))
                .map(|(k, _)| k.as_str())
                .collect();
            bits.push(format!("borders={}", present.join("+")));
        }
        if let Some(token) = jv::truthy_value(l.get("radiusToken")) {
            bits.push(format!("radius={}", jv::text(Some(token))));
        }
        if let Some(text) = jv::truthy_value(l.get("text")) {
            let token = jv::truthy_value(text.get("colorToken"));
            let hex = text.get("color").and_then(|c| c.get("hex"));
            bits.push(format!("color={}", jv::text(token.or(hex))));
        }
        lines.push(format!("{indent}{}", bits.join(" ")));
        for c in list(l.get("children")) {
            row(c, &format!("{indent}  "), lines, token_or);
        }
    }
    let mut lines = Vec::new();
    for p in list(doc.get("pages")) {
        lines.push(format!(
            "page {} \"{}\"",
            jv::text(p.get("id")),
            jv::text(p.get("title"))
        ));
        for b in list(p.get("boards")) {
            lines.push(format!(
                "  board \"{}\" key={} {}w background={}",
                jv::text(b.get("name")),
                jv::text(b.get("key")),
                jv::text(b.get("width")),
                token_or(b.get("background"), "color")
            ));
            for l in list(b.get("layers")) {
                row(l, "    ", &mut lines, &token_or);
            }
        }
    }
    for c in list(doc.get("components")) {
        lines.push(format!(
            "component \"{}\" key={} sig={}",
            jv::text(c.get("name")),
            jv::text(c.get("key")),
            jv::text(c.get("sig"))
        ));
    }
    lines
}

/// `exportSummary`: what `dfw penpot export` prints.
pub fn export_summary(doc: &Value, file: &Path, root: &Path) -> String {
    fn count(l: &Value) -> usize {
        1 + l
            .get("children")
            .and_then(Value::as_array)
            .map_or(0, |c| c.iter().map(count).sum())
    }
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let pages = list(doc.get("pages"));
    let (mut boards, mut layers) = (0, 0);
    for p in &pages {
        for b in list(p.get("boards")) {
            boards += 1;
            layers += list(b.get("layers")).iter().map(count).sum::<usize>();
        }
    }
    let mut lines = vec![
        format!(
            "Wrote {} ({}): {} page(s), {boards} board(s), {layers} layer(s), {} component(s).",
            relative_path(root, file),
            jv::text(doc.get("id")),
            pages.len(),
            list(doc.get("components")).len()
        ),
        if jv::truthy(doc.get("tokens")) {
            "Tokens for Penpot: .dfw/penpot/tokens.penpot.json (Tokens panel > Tools > Import, before building the boards, so colours link to tokens).".to_string()
        } else {
            "No design/tokens/tokens.json: colours are not linked to tokens.".to_string()
        },
    ];
    for e in list(doc.get("errors")) {
        lines.push(format!("Render failed: {}", jv::text(Some(&e))));
    }
    lines.push("In Penpot: start the Studio (dfw studio start), open Plugins (Ctrl+Alt+P), install the manifest from \"dfw penpot plugin\", run \"dfw bridge\" > Build boards.".to_string());
    lines.join("\n")
}

#[cfg(test)]
#[path = "export_tests.rs"]
pub(super) mod tests;
