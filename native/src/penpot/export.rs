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
    dfw_to_penpot, flatten_tokens, parse_color, read_dfw_tokens, token_to_penpot_quiet, DfwTokens,
    Flat, Token,
};
use super::wire;
use crate::browser::launch::{browser_status, fonts_ready, open_renderer, Renderer};
use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::fixed_regex;
use crate::core::paths::{
    file, file_url, file_url_to_path, project_paths, rel, resolve, to_posix, SafeName,
};
use crate::core::util::{
    get_path, now_iso, read_json, sha256, to_json_compact, write_json, write_json_inside,
};
use crate::design::compose::{build_composed, build_system};
use crate::design::manifest::{self, key};
use crate::design::pages::studio_pages;
use crate::design::tokens::{dir_names, relative_path};

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
    project_paths(root).penpot
}

/* ------------------------------------------------------------------ normalising snapshots */

fn style<'a>(n: &'a Value, key: &str) -> Option<&'a Value> {
    n.get(wire::STYLE).and_then(|s| s.get(key))
}

fn painted(n: &Value) -> bool {
    [
        wire::style::FILL,
        wire::style::BORDERS,
        wire::style::SHADOWS,
    ]
    .iter()
    .any(|k| js::truthy(style(n, k)))
}

fn is_wrapper(n: &Value) -> bool {
    n.get(wire::KIND).and_then(Value::as_str) == Some(wire::kind::FRAME)
        && !js::truthy(n.get(wire::DFW_ID))
        && !js::truthy(n.get(wire::COMPONENT))
        && !painted(n)
        && !js::truthy(style(n, wire::style::CLIP))
        && style(n, wire::style::OPACITY).is_none()
        && !js::truthy(style(n, wire::style::BACKGROUND_IMAGE))
}

fn has_children(n: &Value) -> bool {
    n.get(wire::CHILDREN)
        .and_then(Value::as_array)
        .is_some_and(|c| !c.is_empty())
}

/// `simplify`: hoist plain layout wrappers (no id, no component, nothing painted) and drop
/// empty frames.
pub fn simplify(nodes: Vec<Value>) -> Vec<Value> {
    let mut out = Vec::new();
    for mut n in nodes {
        if let Some(children) = n.get_mut(wire::CHILDREN).filter(|c| js::truthy(Some(c))) {
            let kids = match children.take() {
                Value::Array(kids) => kids,
                _ => Vec::new(),
            };
            *children = Value::Array(simplify(kids));
        }
        if is_wrapper(&n) {
            if let Some(Value::Array(kids)) = n.get_mut(wire::CHILDREN).map(Value::take) {
                out.extend(kids);
            }
        } else if n.get(wire::KIND).and_then(Value::as_str) == Some(wire::kind::FRAME)
            && !has_children(&n)
            && !js::truthy(n.get(wire::DFW_ID))
            && !js::truthy(n.get(wire::COMPONENT))
            && !painted(&n)
        {
            continue;
        } else {
            out.push(n);
        }
    }
    out
}

static PLAIN_ALIAS: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^\{([^{}]+)\}$"));
static SCALE_STEP: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"\.[0-9]+$"));
static RADIUS_NAME: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"(^|\.)radius(\.|$)"));
static TEXTISH: LazyLock<Regex> =
    LazyLock::new(|| fixed_regex(r"(^|[.-])(ink|text|on|fg|foreground|muted|subtle)([.-]|$)"));
/// The id of an export (`px_<time>_<hex>`): the only form that names a file in `exports/`.
pub(super) static EXPORT_ID: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^px_[0-9A-Za-z_]+$"));

/// The file of the export `id` (`px_<time>_<hex>.json`), in `exports/`.
pub(super) fn export_file(id: &SafeName) -> String {
    format!("{id}.{}", file::JSON_EXT)
}

/// The export id a file of `exports/` is named after, if it is one.
fn export_of_file(name: &str) -> Option<SafeName> {
    let id = name.strip_suffix(file::JSON_EXT)?.strip_suffix('.')?;
    SafeName::matching(&EXPORT_ID, id)
}

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
    let flat = flatten_tokens(js::truthy_value(tree).unwrap_or(&empty));
    let mut out = Vec::new();
    for (name, t) in flat.iter() {
        if t.type_name() != Some("color") {
            continue;
        }
        let Some(resolved) = resolve_alias(&flat, name) else {
            continue;
        };
        let penpot = token_to_penpot_quiet(name, resolved);
        let Some(c) = penpot
            .value()
            .and_then(|v| parse_color(&js::to_string(Some(v))))
        else {
            continue;
        };
        let alias = t.value.is_string();
        // Prefer semantic names (aliases, no numeric scale step) over palette steps.
        let score = (if alias { 0.0 } else { 2.0 })
            + (if SCALE_STEP.is_match(name) { 4.0 } else { 0.0 })
            + name.split('.').count() as f64 * 0.1;
        out.push(ColorToken {
            name: name.to_string(),
            hex: js::to_string(c.get(super::tokens::key::HEX)),
            opacity: js::present(c.get(wire::dtcg::ALPHA)).map_or(1.0, |a| js::to_number(Some(a))),
            score,
        });
    }
    out.sort_by(|a, b| a.score.total_cmp(&b.score));
    out
}

/// Radius tokens in px: name and value.
fn radius_token_index(tree: Option<&Value>) -> Vec<(String, Value)> {
    let empty = json!({});
    let flat = flatten_tokens(js::truthy_value(tree).unwrap_or(&empty));
    flat.iter()
        .filter(|(name, t)| {
            t.type_name() == Some("dimension")
                && RADIUS_NAME.is_match(name)
                && t.value.get(wire::dtcg::UNIT).and_then(Value::as_str) == Some("px")
        })
        .filter_map(|(name, t)| Some((name.to_string(), t.value.get(wire::dtcg::VALUE)?.clone())))
        .collect()
}

/// Token with exactly this colour; text colours prefer ink/text/on-* names, fills the others.
fn find_color_token(index: &[ColorToken], c: Option<&Value>, text_role: bool) -> Value {
    let Some(c) = js::truthy_value(c) else {
        return Value::Null;
    };
    let (hex, opacity) = (
        c.get(wire::HEX).and_then(Value::as_str),
        js::to_number(c.get(wire::OPACITY)),
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

impl Linker<'_> {
    /// Turn raw snapshot nodes into export layers: fills and strokes with token links, paths.
    fn layers(&self, nodes: &[Value]) -> Vec<Value> {
        nodes
            .iter()
            .enumerate()
            .map(|(i, n)| self.layer(n, &i.to_string()))
            .collect()
    }

    /// One export layer of a snapshot node, its keys in the order of the export file: place,
    /// paint, content, children, then the structure signature.
    fn layer(&self, n: &Value, path: &str) -> Value {
        let kind = n.get(wire::KIND).and_then(Value::as_str);
        let mut l = Map::new();
        put(&mut l, wire::KIND, n.get(wire::KIND).cloned());
        put(&mut l, wire::NAME, n.get(wire::NAME).cloned());
        put(&mut l, wire::PATH, Some(path.into()));
        put(&mut l, wire::DFW_ID, Some(js::or_null(n.get(wire::DFW_ID))));
        put(&mut l, wire::ANCHOR, Some(js::or_null(n.get(wire::ANCHOR))));
        for key in [wire::X, wire::Y, wire::W, wire::H] {
            put(&mut l, key, n.get(key).cloned());
        }
        put(
            &mut l,
            wire::TAG,
            js::truthy_value(n.get(wire::TAG)).cloned(),
        );
        self.put_paint(&mut l, n);
        self.put_content(&mut l, n, kind);
        if let Some(children) = n.get(wire::CHILDREN).and_then(Value::as_array) {
            if !children.is_empty() {
                let kids = children
                    .iter()
                    .enumerate()
                    .map(|(i, c)| self.layer(c, &format!("{path}.{i}")))
                    .collect();
                put(&mut l, wire::CHILDREN, Some(Value::Array(kids)));
            }
        }
        let mut l = Value::Object(l);
        if l.get(wire::CHILDREN).is_some() || kind == Some(wire::kind::FRAME) {
            l[wire::SIG] = signature(&l).into();
        }
        l
    }

    /// What a node paints: fills, strokes (or side borders), radius, shadows, opacity, clip and
    /// a note on a background image.
    fn put_paint(&self, l: &mut Map<String, Value>, n: &Value) {
        let st = |key: &str| style(n, key);
        if let Some(fill) = js::truthy_value(st(wire::style::FILL)) {
            put(
                l,
                wire::FILLS,
                Some(json!([js::object(vec![
                    (wire::COLOR, fill.get(wire::HEX).cloned()),
                    (wire::OPACITY, fill.get(wire::OPACITY).cloned()),
                    (
                        wire::TOKEN,
                        Some(find_color_token(self.colors, Some(fill), false))
                    ),
                ])])),
            );
        }
        if let Some(borders) = js::truthy_value(st(wire::style::BORDERS)) {
            self.put_borders(l, borders);
        }
        if let Some(radius) = js::truthy_value(st(wire::style::RADIUS)) {
            put(l, wire::RADIUS, Some(radius.clone()));
            let corners: &[Value] = radius.as_array().map_or(&[], Vec::as_slice);
            let first = corners.first();
            if corners.iter().all(|x| js::strict_eq(Some(x), first)) {
                let token = self
                    .radii
                    .iter()
                    .find(|(_, px)| js::strict_eq(Some(px), first))
                    .filter(|(name, _)| !name.is_empty());
                put(
                    l,
                    wire::RADIUS_TOKEN,
                    token.map(|(name, _)| name.as_str().into()),
                );
            }
        }
        if let Some(shadows) = js::truthy_value(st(wire::style::SHADOWS)).and_then(Value::as_array)
        {
            let list: Vec<Value> = shadows
                .iter()
                .map(|s| {
                    let mut shadow = js::spread(Some(s));
                    let color = js::truthy_value(s.get(wire::COLOR))
                        .cloned()
                        .unwrap_or_else(
                            || json!({ (wire::HEX): "#000000", (wire::OPACITY): 0.25 }),
                        );
                    shadow.insert(wire::COLOR.into(), color);
                    Value::Object(shadow)
                })
                .collect();
            put(l, wire::SHADOWS, Some(Value::Array(list)));
        }
        put(l, wire::OPACITY, st(wire::style::OPACITY).cloned());
        put(
            l,
            wire::CLIP,
            js::truthy(st(wire::style::CLIP)).then_some(Value::Bool(true)),
        );
        if let Some(image) = js::truthy_value(st(wire::style::BACKGROUND_IMAGE)) {
            let shown = js::slice16(&js::to_string(Some(image)), 0, 60);
            put(
                l,
                wire::NOTE,
                Some(format!("background-image not exported ({shown})").into()),
            );
        }
    }

    /// Borders: one stroke when every side is the same, else the sides as they are.
    fn put_borders(&self, l: &mut Map<String, Value>, borders: &Value) {
        let sides: &[Value] = borders.as_array().map_or(&[], Vec::as_slice);
        let top = sides.first();
        fn hex(b: Option<&Value>) -> Option<&Value> {
            b.and_then(|b| b.get(wire::COLOR))
                .and_then(|c| c.get(wire::HEX))
        }
        let uniform = !sides.is_empty()
            && sides.iter().all(|x| {
                js::truthy(Some(x))
                    && js::strict_eq(
                        x.get(wire::border::W),
                        top.and_then(|t| t.get(wire::border::W)),
                    )
                    && js::strict_eq(hex(Some(x)), hex(top))
            });
        if uniform {
            let color = top.and_then(|t| t.get(wire::COLOR));
            let line = top
                .and_then(|t| t.get(wire::border::STYLE))
                .and_then(Value::as_str);
            let line = line
                .filter(|s| matches!(*s, "dashed" | "dotted"))
                .unwrap_or("solid");
            put(
                l,
                wire::STROKES,
                Some(json!([js::object(vec![
                    (wire::COLOR, hex(top).cloned()),
                    (
                        wire::OPACITY,
                        color.and_then(|c| c.get(wire::OPACITY)).cloned()
                    ),
                    (
                        wire::WIDTH,
                        top.and_then(|t| t.get(wire::border::W)).cloned()
                    ),
                    (wire::border::STYLE, Some(line.into())),
                    (
                        wire::TOKEN,
                        Some(find_color_token(self.colors, color, false))
                    ),
                ])])),
            );
        } else {
            // Penpot strokes are uniform: these are drawn as thin rectangles.
            put(
                l,
                wire::SIDE_BORDERS,
                Some(js::object(vec![
                    (wire::side::TOP, sides.first().cloned()),
                    (wire::side::RIGHT, sides.get(1).cloned()),
                    (wire::side::BOTTOM, sides.get(2).cloned()),
                    (wire::side::LEFT, sides.get(3).cloned()),
                ])),
            );
        }
    }

    /// What a node holds by kind (text, svg, image) and the component it is an instance of.
    fn put_content(&self, l: &mut Map<String, Value>, n: &Value, kind: Option<&str>) {
        if kind == Some(wire::kind::TEXT) {
            put(l, wire::CHARACTERS, n.get(wire::CHARACTERS).cloned());
            let text = n.get(wire::TEXT_STYLE);
            let color = text.and_then(|t| t.get(wire::COLOR));
            let mut t = js::spread(text);
            match color {
                Some(c) => {
                    t.insert(wire::COLOR.into(), c.clone());
                }
                None => {
                    t.shift_remove(wire::COLOR);
                }
            }
            t.insert(
                wire::COLOR_TOKEN.into(),
                find_color_token(self.colors, color, true),
            );
            put(l, wire::TEXT_STYLE, Some(Value::Object(t)));
            put(
                l,
                wire::PLACEHOLDER,
                js::truthy(n.get(wire::PLACEHOLDER)).then_some(Value::Bool(true)),
            );
        }
        if kind == Some("svg") {
            put(l, wire::SVG, n.get(wire::SVG).cloned());
            put(l, wire::ICON, js::truthy_value(n.get(wire::ICON)).cloned());
        }
        if kind == Some("image") {
            put(l, wire::ALT, n.get(wire::ALT).cloned());
            put(l, wire::SRC, Some(self.image_src(n.get(wire::SRC))));
        }
        if let Some(component) = js::truthy_value(n.get(wire::COMPONENT)) {
            let mut c = js::spread(Some(component));
            c.insert(wire::KEY.into(), component_key(component).into());
            put(l, wire::COMPONENT, Some(Value::Object(c)));
        }
    }

    /// Where the plugin loads an image from: a file under `design/` through the Studio, or a
    /// web address as it is.
    fn image_src(&self, src: Option<&Value>) -> Value {
        let Some(src) = js::truthy_value(src).and_then(Value::as_str) else {
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

/// Set a key of a layer when there is a value (a missing one is left out, as JSON drops
/// `undefined`).
fn put(l: &mut Map<String, Value>, key: &str, value: Option<Value>) {
    if let Some(value) = value {
        l.insert(key.to_string(), value);
    }
}

fn component_key(c: &Value) -> String {
    let part = |key: &str| js::truthy_value(c.get(key)).map(|v| js::to_string(Some(v)));
    // The name is joined as it is (a missing one is empty), variant and size only when set.
    let name = match c.get(wire::NAME) {
        None | Some(Value::Null) => String::new(),
        name => js::to_string(name),
    };
    [
        name,
        part(wire::VARIANT).unwrap_or_default(),
        part(wire::SIZE).unwrap_or_default(),
    ]
    .join("|")
}

/// `signature`: the structure of a layer (kinds, names, child structure; not text, positions
/// or colours).
pub fn signature(l: &Value) -> String {
    let own = match js::truthy_value(l.get(wire::COMPONENT)) {
        Some(c) => js::to_string(c.get(wire::KEY)),
        None => js::to_string(l.get(wire::NAME)),
    };
    let children: Vec<String> = l
        .get(wire::CHILDREN)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|c| match c.get(wire::SIG) {
            Some(Value::Null) => String::new(),
            Some(sig) => js::to_string(Some(sig)),
            None => format!(
                "{}:{}",
                js::to_string(c.get(wire::KIND)),
                js::to_string(c.get(wire::NAME))
            ),
        })
        .collect();
    format!(
        "{}:{own}[{}]",
        js::to_string(l.get(wire::KIND)),
        children.join(",")
    )
}

/// Rebase a subtree to its own origin (component main).
fn rebase(l: &Value, ox: f64, oy: f64) -> Value {
    let mut c = js::spread(Some(l));
    let moved = |v: Option<&Value>, origin: f64| {
        js::num(js::round((js::to_number(v) - origin) * 100.0) / 100.0)
    };
    c.insert(wire::X.into(), moved(l.get(wire::X), ox));
    c.insert(wire::Y.into(), moved(l.get(wire::Y), oy));
    if let Some(children) = js::truthy_value(l.get(wire::CHILDREN)).and_then(Value::as_array) {
        let kids = children.iter().map(|ch| rebase(ch, ox, oy)).collect();
        c.insert(wire::CHILDREN.into(), Value::Array(kids));
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
/// and a hash of the pages. The document is built in the key order its hash (the export id)
/// is taken over.
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
    let pages = pages_of_renders(&linker, input.renders, &mut components);
    let token_file = input
        .tokens
        .map(|t| dfw_to_penpot(Some(&t.base), &t.themes));
    let pages = Value::Array(pages);
    let id = export_id(&input.generated_at, &pages);
    json!({
        (wire::SCHEMA): EXPORT_SCHEMA,
        (wire::ID): id,
        (wire::GENERATED_AT): input.generated_at,
        (wire::PRODUCT): input.product,
        (wire::DESIGN_VERSION): input.design_version,
        (wire::TOKENS): token_file.as_ref().map_or(Value::Null, |(doc, _)| doc.clone()),
        (wire::TOKEN_REPORT): token_file.as_ref().map_or(Value::Null, |(_, report)| {
            json!({ (wire::UNSUPPORTED): report.unsupported, (wire::LOSSY): report.lossy })
        }),
        (wire::COLORS): colors
            .iter()
            .map(|c| json!({ (wire::NAME): c.name, (wire::HEX): c.hex, (wire::OPACITY): js::num(c.opacity) }))
            .collect::<Vec<_>>(),
        (wire::COMPONENTS): components.into_iter().map(|(_, c)| c).collect::<Vec<_>>(),
        (wire::PAGES): pages,
    })
}

/// The canvas pages of the renders, by page id in the order of their first render, each with
/// its boards laid out: one row per breakpoint, screens/variants left to right. Collects the
/// component mains on the way.
fn pages_of_renders(
    linker: &Linker,
    renders: &[Value],
    components: &mut Vec<(String, Value)>,
) -> Vec<Value> {
    let mut pages: Vec<(String, Map<String, Value>, Vec<Value>)> = Vec::new();
    for r in renders {
        let page = r.get(wire::PAGE);
        let page_id = page.and_then(|p| p.get(wire::ID));
        let page_key = js::stringify(page_id);
        if !pages.iter().any(|(k, _, _)| *k == page_key) {
            let title = js::truthy_value(page.and_then(|p| p.get(wire::TITLE))).or(page_id);
            let Value::Object(head) = js::object(vec![
                (wire::ID, page_id.cloned()),
                (wire::TITLE, title.cloned()),
            ]) else {
                continue;
            };
            pages.push((page_key.clone(), head, Vec::new()));
        }
        let board = board_of_render(linker, r, components);
        if let Some(slot) = pages.iter_mut().find(|(k, _, _)| *k == page_key) {
            slot.2.push(board);
        }
    }
    pages
        .into_iter()
        .map(|(_, mut head, mut boards)| {
            lay_out(&mut boards);
            head.insert(wire::BOARDS.into(), Value::Array(boards));
            Value::Object(head)
        })
        .collect()
}

/// The board of one render: its layers with token links, size and background.
fn board_of_render(linker: &Linker, r: &Value, components: &mut Vec<(String, Value)>) -> Value {
    let snapshot = r.get(wire::SNAPSHOT);
    let nodes = snapshot
        .and_then(|s| s.get(wire::CHILDREN))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let layers = linker.layers(&simplify(nodes));
    let breakpoint = r.get(wire::BREAKPOINT);
    let bp = |key: &str| breakpoint.and_then(|b| b.get(key));
    let background = snapshot.and_then(|s| s.get(wire::BACKGROUND));
    let key = format!(
        "{}|{}|{}",
        js::to_string(r.get(wire::SCREEN)),
        js::to_string(r.get(wire::VARIANT)),
        js::to_string(bp(key::ID))
    );
    let height = js::max(
        js::to_number(bp(wire::HEIGHT)),
        js::to_number(snapshot.and_then(|s| s.get(wire::HEIGHT))).ceil(),
    );
    for l in &layers {
        visit_components(l, &key, components);
    }
    js::object(vec![
        (wire::KEY, Some(key.as_str().into())),
        (
            wire::NAME,
            Some(
                format!(
                    "{} / {} @ {}",
                    js::to_string(r.get(wire::SCREEN)),
                    js::to_string(r.get(wire::VARIANT)),
                    js::to_string(bp(key::ID))
                )
                .into(),
            ),
        ),
        (wire::SCREEN, r.get(wire::SCREEN).cloned()),
        (wire::VARIANT, r.get(wire::VARIANT).cloned()),
        (wire::BREAKPOINT, bp(key::ID).cloned()),
        (wire::FILE, r.get(wire::FILE).cloned()),
        (wire::WIDTH, bp(wire::WIDTH).cloned()),
        (wire::HEIGHT, Some(js::num(height))),
        (wire::VIEWPORT_HEIGHT, bp(wire::HEIGHT).cloned()),
        (
            wire::BACKGROUND,
            Some(js::object(vec![
                (
                    wire::COLOR,
                    background.and_then(|b| b.get(wire::HEX)).cloned(),
                ),
                (
                    wire::OPACITY,
                    background.and_then(|b| b.get(wire::OPACITY)).cloned(),
                ),
                (
                    wire::TOKEN,
                    Some(find_color_token(linker.colors, background, false)),
                ),
            ])),
        ),
        (wire::LAYERS, Some(Value::Array(layers))),
    ])
}

/// `px_<render time, digits only>_<hash of the pages>`.
fn export_id(generated_at: &str, pages: &Value) -> String {
    let stamp: String = generated_at
        .chars()
        .filter(|c| !matches!(c, '-' | ':' | '.' | 'T' | 'Z'))
        .collect();
    format!(
        "px_{}_{}",
        js::slice16(&stamp, 0, 14),
        &sha256(to_json_compact(pages))[..8]
    )
}

/// The first instance of every component becomes its main, rebased to its own origin.
fn visit_components(l: &Value, board_key: &str, components: &mut Vec<(String, Value)>) {
    if let Some(component) = js::truthy_value(l.get(wire::COMPONENT)) {
        let key = js::stringify(component.get(wire::KEY));
        if !components.iter().any(|(k, _)| *k == key) {
            let name: Vec<String> = [wire::NAME, wire::VARIANT, wire::SIZE]
                .iter()
                .filter_map(|k| js::truthy_value(component.get(*k)))
                .map(|v| js::to_string(Some(v)))
                .collect();
            let main = js::object(vec![
                (wire::KEY, component.get(wire::KEY).cloned()),
                (wire::NAME, Some(name.join(" / ").into())),
                (wire::COMPONENT, component.get(wire::NAME).cloned()),
                (wire::VARIANT, component.get(wire::VARIANT).cloned()),
                (wire::SIZE, component.get(wire::SIZE).cloned()),
                (wire::SIG, l.get(wire::SIG).cloned()),
                (
                    wire::FROM,
                    Some(format!("{board_key}#{}", js::to_string(l.get(wire::PATH))).into()),
                ),
                (
                    wire::ROOT,
                    Some(rebase(
                        l,
                        js::to_number(l.get(wire::X)),
                        js::to_number(l.get(wire::Y)),
                    )),
                ),
            ]);
            components.push((key, main));
        }
    }
    for c in l
        .get(wire::CHILDREN)
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
        let bp = js::stringify(b.get(wire::BREAKPOINT));
        if !rows.contains(&bp) {
            rows.push(bp);
        }
    }
    let mut y = 0.0;
    for bp in rows {
        let (mut x, mut row_height) = (0.0, 0.0);
        for b in boards
            .iter_mut()
            .filter(|b| js::stringify(b.get(wire::BREAKPOINT)) == bp)
        {
            let (width, height) = (
                js::to_number(b.get(wire::WIDTH)),
                js::to_number(b.get(wire::HEIGHT)),
            );
            b[wire::X] = js::num(x);
            b[wire::Y] = js::num(y);
            x += width + BOARD_GAP_X;
            row_height = js::max(row_height, height);
        }
        y += row_height + BOARD_GAP_Y;
    }
}

/* ------------------------------------------------------------------ export (render + write) */

/// The canvas pages that hold screens (`studioPages(m)` without the Design System page and
/// the flow page) as id, title and screen ids.
fn screen_pages(manifest: &Value) -> Vec<(String, Value, Vec<String>)> {
    studio_pages(manifest)
        .into_iter()
        .filter(|p| !js::truthy(p.get(wire::SYSTEM)) && !js::truthy(p.get(wire::FLOW)))
        .map(|p| {
            let screens = manifest::items(p.get(key::SCREENS))
                .iter()
                .map(js::to_string)
                .collect();
            (
                js::to_string(p.get(key::ID)),
                p.get(key::TITLE).cloned().unwrap_or(Value::Null),
                screens,
            )
        })
        .collect()
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
    if !js::truthy(Some(&m)) {
        return Err(DfwError::new(format!("{} not found.", rel::MANIFEST)));
    }
    let list = |v: Option<&Value>| -> Vec<Value> {
        v.and_then(Value::as_array).cloned().unwrap_or_default()
    };
    let all_bps = match js::truthy_value(m.get(key::BREAKPOINTS)) {
        Some(bps) => list(Some(bps)),
        None => list(get_path(&load_config(Some(root))?, "studio.breakpoints")),
    };
    let bps: Vec<Value> = match bp.filter(|b| !b.is_empty()) {
        Some(bp) => {
            let wanted: Vec<&str> = bp.split(',').collect();
            all_bps
                .iter()
                .filter(|b| {
                    b.get(key::ID)
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
            .map(|b| b.get(key::ID).cloned().unwrap_or(Value::Null))
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
            "No page \"{page}\" in {}.",
            rel::MANIFEST
        )));
    }
    let screens = list(m.get(key::SCREENS));
    let mut jobs = Vec::new();
    for (id, title, screen_ids) in &pages {
        for sid in screen_ids {
            // The last screen with an id wins, as in a JavaScript Map.
            let screen = screens
                .iter()
                .rev()
                .find(|s| s.get(key::ID).and_then(Value::as_str) == Some(sid));
            for v in list(screen.and_then(|s| js::truthy_value(s.get(key::VARIANTS)))) {
                for b in &bps {
                    jobs.push(js::object(vec![
                        (
                            wire::PAGE,
                            Some(json!({ (wire::ID): id, (wire::TITLE): title })),
                        ),
                        (wire::SCREEN, Some(sid.as_str().into())),
                        (wire::VARIANT, v.get(key::ID).cloned()),
                        (wire::BREAKPOINT, Some(b.clone())),
                        (
                            wire::FILE,
                            Some(rel::design(&js::to_string(v.get(key::FILE))).into()),
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
        let n = js::to_number(job.get(wire::BREAKPOINT).and_then(|b| b.get(key)));
        if n.is_finite() && n >= 0.0 {
            n as u32
        } else {
            0
        }
    };
    let page = renderer.page(size(wire::WIDTH), size(wire::HEIGHT), 1.0)?;
    let result = (|| {
        let file = file.ok_or_else(|| {
            DfwError::new(format!(
                "Could not prepare {} for rendering. Check that the variant's \"file\" in {} names an HTML file under {}/.",
                js::to_string(job.get(wire::FILE)),
                rel::MANIFEST,
                rel::DESIGN
            ))
        })?;
        page.goto(&file_url(file), 30_000)?;
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
        return Err(DfwError::new(format!(
            "No screens to export (check --page/--bp and {}).",
            rel::MANIFEST
        )));
    }
    build_system(root)?;
    let mut rels: Vec<String> = Vec::new();
    for job in &jobs {
        let rel = js::to_string(job.get(wire::FILE));
        if !rels.contains(&rel) {
            rels.push(rel);
        }
    }
    let files = build_composed(root, &rels)?;
    let file_of = |job: &Value| {
        let rel = js::to_string(job.get(wire::FILE));
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
                let mut render = js::spread(Some(job));
                render.insert(wire::SNAPSHOT.into(), snapshot);
                renders.push(Value::Object(render));
            }
            Err(err) => errors.push(Value::String(format!(
                "{}/{}@{}: {}",
                js::to_string(job.get(wire::SCREEN)),
                js::to_string(job.get(wire::VARIANT)),
                js::to_string(job.get(wire::BREAKPOINT).and_then(|b| b.get(wire::ID))),
                err.message()
            ))),
        }
    }
    let tokens = read_dfw_tokens(root, false).ok();
    let state = read_json(&project_paths(root).state, Some(json!({})))?;
    let version = js::present(
        state
            .get(wire::state::DESIGN)
            .and_then(|d| d.get(wire::state::VERSION)),
    );
    let mut doc = build_export(&ExportInput {
        renders: &renders,
        tokens: tokens.as_ref(),
        product: js::truthy_value(manifest.get(wire::PRODUCT))
            .cloned()
            .unwrap_or_else(|| "".into()),
        design_version: format!(
            "v{}",
            version.map_or("1".to_string(), |v| js::to_string(Some(v)))
        ),
        project_root: Some(root),
        generated_at: now_iso(),
    });
    doc[wire::ERRORS] = Value::Array(errors);
    let dir = penpot_dir(root);
    let id = js::to_string(doc.get(wire::ID));
    let id = SafeName::matching(&EXPORT_ID, &id).ok_or_else(|| {
        DfwError::internal(format!(
            "Export id \"{id}\" is not of the form px_<time>_<hash>."
        ))
    })?;
    write_json_inside(
        root,
        &dir.join(file::PENPOT_EXPORTS).join(export_file(&id)),
        &doc,
    )?;
    prune_exports(&dir, KEEP_EXPORTS)?;
    let file = match opts.out.as_deref().filter(|o| !o.is_empty()) {
        // A file the user named may be outside the project on purpose.
        Some(out) => {
            let file = resolve(root.join(out));
            write_json(&file, &doc)?;
            file
        }
        None => {
            let file = resolve(dir.join(file::PENPOT_EXPORT));
            write_json_inside(root, &file, &doc)?;
            file
        }
    };
    if let Some(tokens) = js::truthy_value(doc.get(wire::TOKENS)) {
        write_json_inside(root, &dir.join(file::PENPOT_TOKENS), tokens)?;
    }
    Ok((file, doc))
}

/// `pruneExports`: keep the newest exports in `.dfw/penpot/exports/` (every render writes a
/// full copy). Ids start with the render time, so name order is age order. Returns the file
/// names removed.
pub fn prune_exports(dir: &Path, keep: usize) -> Result<Vec<String>, DfwError> {
    let d = dir.join(file::PENPOT_EXPORTS);
    if !d.exists() {
        return Ok(Vec::new());
    }
    let mut files: Vec<String> = dir_names(&d)?
        .into_iter()
        .filter(|f| export_of_file(f).is_some())
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
#[cfg(test)]
pub fn export_structure(doc: &Value) -> Vec<String> {
    fn list(v: Option<&Value>) -> impl Iterator<Item = &Value> {
        v.and_then(Value::as_array).into_iter().flatten()
    }
    // An element of `bits.join(' ')`: null and undefined are empty.
    fn bit(v: Option<&Value>) -> String {
        match v {
            None | Some(Value::Null) => String::new(),
            v => js::to_string(v),
        }
    }
    let token_or = |v: Option<&Value>, other: &str| {
        let token = js::truthy_value(v.and_then(|v| v.get(wire::TOKEN)));
        js::to_string(token.or(v.and_then(|v| v.get(other))))
    };
    fn row(
        l: &Value,
        indent: &str,
        lines: &mut Vec<String>,
        token_or: &dyn Fn(Option<&Value>, &str) -> String,
    ) {
        let mut bits = vec![
            bit(l.get(wire::KIND)),
            bit(l.get(wire::NAME)),
            format!("path={}", js::to_string(l.get(wire::PATH))),
        ];
        if let Some(anchor) = js::truthy_value(l.get(wire::ANCHOR)) {
            if !js::strict_eq(Some(anchor), l.get(wire::NAME)) {
                bits.push(format!("anchor={}", js::to_string(Some(anchor))));
            }
        }
        if let Some(c) = js::truthy_value(l.get(wire::COMPONENT)) {
            bits.push(format!("component={}", js::to_string(c.get(wire::KEY))));
        }
        if let Some(characters) = js::truthy_value(l.get(wire::CHARACTERS)) {
            bits.push(to_json_compact(characters));
        }
        let first = |key: &str| js::truthy_value(l.get(key).and_then(|v| v.get(0)));
        if let Some(fill) = first(wire::FILLS) {
            bits.push(format!("fill={}", token_or(Some(fill), wire::COLOR)));
        }
        if let Some(stroke) = first(wire::STROKES) {
            bits.push(format!("stroke={}", token_or(Some(stroke), wire::COLOR)));
        }
        if let Some(Value::Object(sides)) = js::truthy_value(l.get(wire::SIDE_BORDERS)) {
            let present: Vec<&str> = js::entries(sides)
                .into_iter()
                .filter(|(_, v)| js::truthy(Some(v)))
                .map(|(k, _)| k.as_str())
                .collect();
            bits.push(format!("borders={}", present.join("+")));
        }
        if let Some(token) = js::truthy_value(l.get(wire::RADIUS_TOKEN)) {
            bits.push(format!("radius={}", js::to_string(Some(token))));
        }
        if let Some(text) = js::truthy_value(l.get(wire::TEXT_STYLE)) {
            let token = js::truthy_value(text.get(wire::COLOR_TOKEN));
            let hex = text.get(wire::COLOR).and_then(|c| c.get(wire::HEX));
            bits.push(format!("color={}", js::to_string(token.or(hex))));
        }
        lines.push(format!("{indent}{}", bits.join(" ")));
        for c in list(l.get(wire::CHILDREN)) {
            row(c, &format!("{indent}  "), lines, token_or);
        }
    }
    let mut lines = Vec::new();
    for p in list(doc.get(wire::PAGES)) {
        lines.push(format!(
            "page {} \"{}\"",
            js::to_string(p.get(wire::ID)),
            js::to_string(p.get(wire::TITLE))
        ));
        for b in list(p.get(wire::BOARDS)) {
            lines.push(format!(
                "  board \"{}\" key={} {}w background={}",
                js::to_string(b.get(wire::NAME)),
                js::to_string(b.get(wire::KEY)),
                js::to_string(b.get(wire::WIDTH)),
                token_or(b.get(wire::BACKGROUND), wire::COLOR)
            ));
            for l in list(b.get(wire::LAYERS)) {
                row(l, "    ", &mut lines, &token_or);
            }
        }
    }
    for c in list(doc.get(wire::COMPONENTS)) {
        lines.push(format!(
            "component \"{}\" key={} sig={}",
            js::to_string(c.get(wire::NAME)),
            js::to_string(c.get(wire::KEY)),
            js::to_string(c.get(wire::SIG))
        ));
    }
    lines
}

/// `exportSummary`: what `dfw penpot export` prints.
pub fn export_summary(doc: &Value, file: &Path, root: &Path) -> String {
    fn count(l: &Value) -> usize {
        1 + l
            .get(wire::CHILDREN)
            .and_then(Value::as_array)
            .map_or(0, |c| c.iter().map(count).sum())
    }
    let list = |v: Option<&Value>| v.and_then(Value::as_array).cloned().unwrap_or_default();
    let pages = list(doc.get(wire::PAGES));
    let (mut boards, mut layers) = (0, 0);
    for p in &pages {
        for b in list(p.get(wire::BOARDS)) {
            boards += 1;
            layers += list(b.get(wire::LAYERS)).iter().map(count).sum::<usize>();
        }
    }
    let mut lines = vec![
        format!(
            "Wrote {} ({}): {} page(s), {boards} board(s), {layers} layer(s), {} component(s).",
            relative_path(root, file),
            js::to_string(doc.get(wire::ID)),
            pages.len(),
            list(doc.get(wire::COMPONENTS)).len()
        ),
        if js::truthy(doc.get(wire::TOKENS)) {
            format!("Tokens for Penpot: {} (Tokens panel > Tools > Import, before building the boards, so colours link to tokens).", rel::PENPOT_TOKENS)
        } else {
            format!("No {}: colours are not linked to tokens.", rel::TOKENS_JSON)
        },
    ];
    for e in list(doc.get(wire::ERRORS)) {
        lines.push(format!("Render failed: {}", js::to_string(Some(&e))));
    }
    lines.push("In Penpot: start the Studio (dfw studio start), open Plugins (Ctrl+Alt+P), install the manifest from \"dfw penpot plugin\", run \"dfw bridge\" > Build boards.".to_string());
    lines.join("\n")
}

#[cfg(test)]
#[path = "export_tests.rs"]
pub(super) mod tests;
