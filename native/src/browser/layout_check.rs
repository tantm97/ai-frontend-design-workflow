//! Ports `core/lib/layout.mjs`: the rendered layout check (`layout_check`, `find_voids`).
//! What a reviewer sees, not what the HTML says: every screen is rendered (composed, CSS
//! inlined) at each breakpoint and state in headless Chrome and measured for
//!   overlap, h-overflow, anchor (MUST); clipped, void, consistency, reflow, variant-cosmetic,
//!   touch-target (SHOULD).
//! The measuring itself is JavaScript that runs in the page: `layout_measure.js`, the same text
//! as `measure` in layout.mjs. Thresholds are applied here.
// Over 800 lines with its tests: the check and its message texts are one Node file.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

use regex::{Captures, Regex};
use serde::Deserialize;
use serde_json::{json, Value};

use super::cdp::lock;
use super::launch::{
    browser_status, fonts_ready, open_renderer_with, BrowserStatus, LaunchOptions, Renderer,
};
use crate::core::config::{load_config, studio_breakpoints, Breakpoint};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve, to_posix};
use crate::core::util::read_json;
use crate::design::compose::{
    build_composed, build_system, compose_html, load_components, ComposeOptions,
};
use crate::design::system_page::render_system_page;
use crate::design::tokens::relative_path;

/// `measure` of layout.mjs, byte for byte: it runs in the page and returns raw measurements.
const MEASURE: &str = include_str!("layout_measure.js");
const SET_STATE: &str = "(st) => document.documentElement.setAttribute('data-state', st)";
const ENLARGE_TEXT: &str = "() => (document.documentElement.style.fontSize = '125%')";

/// Pages measured at the same time.
const CONCURRENCY: usize = 6;

/// One finding in the dfw check format (`{ level, rule, file, line, message, source }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// `error` (MUST), `warn` (SHOULD) or `info`.
    pub level: &'static str,
    pub rule: String,
    /// The design file, relative to the project root (a URL for a live page).
    pub file: String,
    pub message: String,
    /// The check that found it: `layout` or `axe`.
    pub source: &'static str,
}

impl Finding {
    /// The finding as layout.mjs writes it (`line` is always 1).
    pub fn to_json(&self) -> Value {
        json!({ "level": self.level, "rule": self.rule, "file": self.file, "line": 1, "message": self.message, "source": self.source })
    }
}

/// What the page measured (the object `measure` returns).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Measure {
    pub vw: f64,
    pub vh: f64,
    pub overlaps: Vec<Overlap>,
    pub clipped: Vec<Clipped>,
    pub anchors: Vec<Anchor>,
    pub h_overflow: f64,
    pub blocks: Vec<Block>,
    pub components: Vec<ComponentSig>,
    pub content: Option<Content>,
    pub small: Vec<Small>,
}

/// Two elements painting over each other.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Overlap {
    pub a: String,
    pub b: String,
    pub area: f64,
}

/// Text cut off by its box.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Clipped {
    pub el: String,
    pub text: String,
}

/// An element marked `data-dfw-anchor`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Anchor {
    pub name: String,
    pub top: f64,
    pub bottom: f64,
    pub in_view: bool,
}

/// A painted content block, in document coordinates.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Block {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// A component instance and the computed styles that identify its look.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ComponentSig {
    pub c: String,
    pub v: String,
    pub s: String,
    pub sig: String,
}

/// The main content region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Content {
    pub x: f64,
    pub w: f64,
    pub top: f64,
}

/// A tappable control under 44x44.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Small {
    pub el: String,
    pub w: f64,
    pub h: f64,
}

/// An empty area [`find_voids`] found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Void {
    /// An empty band between content blocks.
    Gap { from: f64, height: f64 },
    /// Content ends early in the first viewport.
    Tail { height: f64, fill: f64 },
    /// The first viewport is mostly empty.
    Sparse { fill: f64 },
}

/// `findVoids(m, { minBand })`: whitespace analysis on measured blocks (pure). Node's default
/// for `min_band` is 0.22.
pub fn find_voids(m: &Measure, min_band: f64) -> Vec<Void> {
    let mut res = Vec::new();
    let Some(Content { x, w, top }) = m.content else {
        return res;
    };
    let in_main: Vec<&Block> = m
        .blocks
        .iter()
        .filter(|b| b.x + b.w > x + 4.0 && b.x < x + w - 4.0 && b.y + b.h > top)
        .collect();
    if in_main.is_empty() {
        return res;
    }
    let bottom = in_main
        .iter()
        .map(|b| b.y + b.h)
        .fold(f64::NEG_INFINITY, f64::max);
    // Cells a block covers along one axis: from its start to its end, kept inside 0..count.
    let span = |from: f64, to: f64, cell: f64, count: i64| {
        let first = ((from / cell).floor() as i64).max(0);
        let last = ((to / cell).ceil() as i64).min(count);
        first..last
    };
    // Vertical coverage in 8px rows across the main region.
    let step = 8.0;
    let rows = ((bottom - top) / step).ceil() as i64;
    let mut covered = vec![false; rows.max(0) as usize];
    for b in &in_main {
        for r in span(b.y - top, b.y + b.h - top, step, rows) {
            covered[r as usize] = true;
        }
    }
    let mut run = 0i64;
    for r in 0..=rows {
        if r < rows && !covered[r as usize] {
            run += 1;
            continue;
        }
        let height = run as f64 * step;
        if height > m.vh * min_band {
            res.push(Void::Gap {
                from: js::round(top + (r - run) as f64 * step),
                height,
            });
        }
        run = 0;
    }
    // First-viewport fill: area of the main region covered by blocks.
    let cell = 24.0;
    let cols = (w / cell).ceil() as i64;
    let vrows = ((m.vh - top) / cell).ceil() as i64;
    if cols > 0 && vrows > 0 {
        let mut grid = vec![false; (cols * vrows) as usize];
        for b in &in_main {
            for r in span(b.y - top, b.y + b.h - top, cell, vrows) {
                for c in span(b.x - x, b.x + b.w - x, cell, cols) {
                    grid[(r * cols + c) as usize] = true;
                }
            }
        }
        let fill = grid.iter().filter(|v| **v).count() as f64 / grid.len() as f64;
        // Empty band at the bottom of the first viewport while content ends early.
        let tail = m.vh - m.vh.min(bottom);
        if bottom < m.vh && tail > m.vh * 0.3 {
            res.push(Void::Tail {
                height: js::round(tail),
                fill: js::round(fill * 100.0),
            });
        } else if fill < 0.35 {
            res.push(Void::Sparse {
                fill: js::round(fill * 100.0),
            });
        }
    }
    res
}

/// One render: a screen variant at a breakpoint, in a state or a mode.
#[derive(Debug, Clone, PartialEq, Eq)]
struct RenderJob {
    screen: String,
    variant: String,
    rel: String,
    bp: Breakpoint,
    state: Option<String>,
    stress: bool,
    reflow: bool,
    system: bool,
}

impl RenderJob {
    /// `j.state` when it is truthy.
    fn state(&self) -> Option<&str> {
        self.state.as_deref().filter(|s| !s.is_empty())
    }
}

fn reflow_bp() -> Breakpoint {
    Breakpoint {
        id: "reflow".into(),
        label: None,
        width: 320,
        height: 640,
    }
}

fn list<'a>(value: &'a Value, key: &str) -> impl Iterator<Item = &'a Value> {
    value
        .get(key)
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// `s.states || m.states || []`, as text.
pub(crate) fn states_of(screen: &Value, manifest: &Value) -> Vec<String> {
    let own = screen.get("states").filter(|v| js::truthy(v));
    let states = own.or_else(|| manifest.get("states").filter(|v| js::truthy(v)));
    states
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|s| js::to_string(Some(s)))
        .collect()
}

/// `m.breakpoints || config.studio.breakpoints`.
pub(crate) fn manifest_breakpoints(manifest: &Value, config: &Value) -> Vec<Breakpoint> {
    match manifest.get("breakpoints").filter(|b| js::truthy(b)) {
        Some(all) => all
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Breakpoint::from_value)
            .collect(),
        None => studio_breakpoints(config),
    }
}

fn screen_jobs(manifest: &Value, bps: &[Breakpoint], opts: &LayoutOptions) -> Vec<RenderJob> {
    let mut jobs = Vec::new();
    for s in list(manifest, "screens") {
        let id = js::to_string(s.get("id"));
        if opts
            .screens
            .as_ref()
            .is_some_and(|only| !only.contains(&id))
        {
            continue;
        }
        let mut states = vec![None];
        if opts.states {
            states.extend(states_of(s, manifest).into_iter().map(Some));
        }
        for v in list(s, "variants") {
            let job =
                |bp: &Breakpoint, state: Option<String>, stress: bool, reflow: bool| RenderJob {
                    screen: id.clone(),
                    variant: js::to_string(v.get("id")),
                    rel: format!("design/{}", js::to_string(v.get("file"))),
                    bp: bp.clone(),
                    state,
                    stress,
                    reflow,
                    system: false,
                };
            if opts.stress {
                jobs.push(job(&reflow_bp(), None, false, true));
            }
            for bp in bps {
                for st in &states {
                    jobs.push(job(bp, st.clone(), false, false));
                    if opts.stress && st.is_none() {
                        jobs.push(job(bp, None, true, false));
                    }
                }
            }
        }
    }
    jobs
}

/// `pathToFileURL(file).href`.
pub(crate) fn file_url(file: &Path) -> String {
    let mut url = String::from("file://");
    for b in to_posix(&resolve(file).display().to_string()).bytes() {
        if b.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@[]|".contains(&b) {
            url.push(b as char);
        } else {
            url.push_str(&format!("%{b:02X}"));
        }
    }
    url
}

/// Write the composed Design System page to `.dfw/build/design/system/index.html` with its
/// local CSS inlined, like `build_composed` does for screens.
fn build_system_page(root: &Path) -> Result<PathBuf, DfwError> {
    let regex = |source| Regex::new(source).map_err(|err| DfwError::internal(err.to_string()));
    let p = project_paths(root);
    let out_dir = p.dfw.join("build/design/system");
    let sys_out = out_dir.join("index.html");
    fs::create_dir_all(&out_dir)?;
    let page = render_system_page(root, "../")?;
    let opts = ComposeOptions {
        lib: None,
        design_base: Some("../"),
    };
    let html = compose_html(root, &page, opts)?.html;
    let src_dir = p.design.join("system");
    let href = regex(r#"href=["']([^"']+)["']"#)?;
    let mut failed = None;
    let inlined = regex(r#"(?i)<link\b[^>]*rel=["']stylesheet["'][^>]*>"#)?.replace_all(
        &html,
        |tag: &Captures| {
            let tag = &tag[0];
            let local = href
                .captures(tag)
                .map(|c| c[1].to_string())
                .filter(|h| {
                    !["//", "http://", "https://"]
                        .iter()
                        .any(|p| h.starts_with(p))
                })
                .map(|h| resolve(src_dir.join(h)))
                .filter(|f| f.exists());
            match local.map(|f| js::read_text(&f)) {
                Some(Ok(css)) => format!("<style>{css}</style>"),
                Some(Err(err)) => {
                    failed = Some(err);
                    tag.to_string()
                }
                None => tag.to_string(),
            }
        },
    );
    if let Some(err) = failed {
        return Err(err);
    }
    let base = to_posix(&relative_path(&out_dir, &src_dir));
    let with_base = regex(r"(?i)<head([^>]*)>")?.replacen(&inlined, 1, |head: &Captures| {
        format!("<head{}><base href=\"{base}/\">", &head[1])
    });
    fs::write(&sys_out, with_base.as_bytes())?;
    Ok(sys_out)
}

/// What to check (`opts` of `layoutCheck`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutOptions {
    /// Only these screen ids; `None` is every screen of the design.
    pub screens: Option<Vec<String>>,
    /// Only these breakpoint ids.
    pub bp: Option<Vec<String>>,
    /// Also every state of each screen.
    pub states: bool,
    /// Also with long lists and at 320px with text at 125% (the default in Node).
    pub stress: bool,
    /// Also the generated Design System page (the default in Node).
    pub system: bool,
}

impl Default for LayoutOptions {
    fn default() -> Self {
        LayoutOptions {
            screens: None,
            bp: None,
            states: false,
            stress: true,
            system: true,
        }
    }
}

/// The result of [`layout_check`] (`{ status, detail, findings, renders }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LayoutResult {
    /// No browser to render with: reported, never treated as a pass.
    Unavailable { detail: String },
    Ran {
        findings: Vec<Finding>,
        renders: usize,
    },
}

/// `layoutCheck(root, opts)`: run the rendered layout check.
pub fn layout_check(root: &Path, opts: &LayoutOptions) -> Result<LayoutResult, DfwError> {
    layout_check_with(root, opts, &LaunchOptions::default())
}

/// [`layout_check`] with launch options for the browser.
pub fn layout_check_with(
    root: &Path,
    opts: &LayoutOptions,
    launch: &LaunchOptions,
) -> Result<LayoutResult, DfwError> {
    if let BrowserStatus::Missing { detail } = browser_status() {
        return Ok(LayoutResult::Unavailable {
            detail: detail.to_string(),
        });
    }
    let p = project_paths(root);
    let manifest = read_json(&p.manifest, Some(json!({})))?;
    let mut bps = manifest_breakpoints(&manifest, &load_config(Some(root))?);
    if let Some(only) = &opts.bp {
        bps.retain(|b| only.contains(&b.id));
    }
    build_system(root)?;
    let mut jobs = screen_jobs(&manifest, &bps, opts);
    let mut rels: Vec<String> = Vec::new();
    for job in &jobs {
        if !rels.contains(&job.rel) {
            rels.push(job.rel.clone());
        }
    }
    let mut files: Vec<(String, PathBuf)> = build_composed(root, &rels)?
        .into_iter()
        .map(|built| (built.rel, built.out))
        .collect();
    if opts.system && !load_components(root)?.is_empty() {
        let rel = "design/system/index.html".to_string();
        files.push((rel.clone(), build_system_page(root)?));
        let bp = bps.iter().find(|b| b.width >= 1280).or(bps.first());
        // Node renders nothing without a breakpoint and then fails on `r.bp.id`.
        let bp = bp.ok_or_else(|| {
            DfwError::internal("Cannot read properties of undefined (reading 'id')")
        })?;
        jobs.push(RenderJob {
            screen: "_system".into(),
            variant: "page".into(),
            rel,
            bp: bp.clone(),
            state: None,
            stress: false,
            reflow: false,
            system: true,
        });
    }
    let renderer = open_renderer_with(launch)?;
    let slots: Mutex<Vec<Option<Result<Measure, String>>>> = Mutex::new(vec![None; jobs.len()]);
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..CONCURRENCY.min(jobs.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(i) else {
                    break;
                };
                let file = files.iter().find(|(rel, _)| *rel == job.rel);
                let measured = measure(&renderer, job, file.map(|(_, out)| out.as_path()));
                lock(&slots)[i] = Some(measured.map_err(|e| e.message().to_string()));
            });
        }
    });
    renderer.close();
    let slots = slots
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let results: Vec<(RenderJob, Result<Measure, String>)> = jobs
        .into_iter()
        .zip(slots)
        // A worker that panicked left its job without a result.
        .map(|(job, slot)| {
            let measured = slot.unwrap_or_else(|| Err("render worker stopped".into()));
            (job, measured)
        })
        .collect();
    Ok(LayoutResult::Ran {
        findings: findings_of(&results),
        renders: results.len(),
    })
}

/// Render one job and measure the page.
fn measure(renderer: &Renderer, job: &RenderJob, file: Option<&Path>) -> Result<Measure, DfwError> {
    let page = renderer.page(job.bp.width, job.bp.height, 1.0)?;
    let measured = (|| {
        // A design file that does not exist was not composed.
        let file = file.ok_or_else(|| {
            DfwError::internal("The \"path\" argument must be of type string. Received undefined")
        })?;
        page.goto(&file_url(file), 30_000)?;
        if let Some(state) = job.state() {
            page.evaluate(SET_STATE, Some(&json!(state)))?;
        }
        if job.reflow {
            page.evaluate(ENLARGE_TEXT, None)?;
        }
        fonts_ready(&page, 3000)?;
        page.wait_for_timeout(40);
        let touch = !job.stress && !job.reflow && !job.system && job.bp.width < 1024;
        let arg = json!({ "stress": job.stress, "touch": touch });
        let raw = page.evaluate(MEASURE.trim_end(), Some(&arg))?;
        serde_json::from_value(raw).map_err(|e| DfwError::internal(e.to_string()))
    })();
    page.close();
    measured
}

/// `text.slice(0, n)`, by characters.
pub(crate) fn head(text: &str, n: usize) -> String {
    text.chars().take(n).collect()
}

/// Keys in the order of first use, like a JavaScript `Map`.
type Ordered<T> = Vec<(String, T)>;

/// The entry of `key` in an [`Ordered`] map, added when it is new.
fn entry<'a, T: Default>(map: &'a mut Ordered<T>, key: &str) -> &'a mut T {
    let at = match map.iter().position(|(k, _)| k == key) {
        Some(at) => at,
        None => {
            map.push((key.to_string(), T::default()));
            map.len() - 1
        }
    };
    &mut map[at].1
}

fn where_of(r: &RenderJob) -> String {
    let target = if r.screen == "_system" {
        "Design System page".to_string()
    } else {
        format!("{}/{}", r.screen, r.variant)
    };
    let at = if r.reflow {
        "320px, text 125%"
    } else {
        &r.bp.id
    };
    let state = r.state().map(|s| format!(" [{s}]")).unwrap_or_default();
    let stress = if r.stress {
        " [stress: long lists]"
    } else {
        ""
    };
    format!("{target} @ {at}{state}{stress}")
}

/// Apply the thresholds to what each render measured.
// Over 120 lines, as in layoutCheck: the per-render rules, then the two cross-screen
// comparisons, in the order Node reports them, each next to its message text.
#[rustfmt::skip]
fn findings_of(results: &[(RenderJob, Result<Measure, String>)]) -> Vec<Finding> {
    let n = js::number;
    let mut findings = Vec::new();
    let layout = |level, rule: &str, file: &str, message: String| Finding { level, rule: rule.to_string(), file: file.to_string(), message, source: "layout" };
    for (r, measured) in results {
        let mut add = |level, rule: &str, message: String| findings.push(layout(level, rule, &r.rel, format!("{}: {message}", where_of(r))));
        let mm = match measured {
            Ok(mm) => mm,
            Err(error) => {
                add("warn", "render", format!("could not render ({})", head(error, 120)));
                continue;
            }
        };
        if r.reflow {
            // Two-dimensional content (data tables, toolbars) is exempt from reflow in WCAG: a SHOULD, not a MUST.
            if mm.h_overflow > 1.0 {
                add("warn", "reflow", format!("page scrolls sideways by {}px. Content should reflow to one column at 320px (WCAG 1.4.10); scroll only inside data tables.", n(mm.h_overflow)));
            }
            for o in mm.overlaps.iter().take(3) {
                add("warn", "reflow", format!("\"{}\" overlaps \"{}\" when text is enlarged.", o.a, o.b));
            }
            for c in mm.clipped.iter().take(3) {
                add("warn", "reflow", format!("text clipped in \"{}\" when text is enlarged (\"{}…\"). Let boxes grow with their text.", c.el, c.text));
            }
            continue;
        }
        if mm.h_overflow > 1.0 {
            add("error", "h-overflow", format!("page is {}px wider than the viewport.", n(mm.h_overflow)));
        }
        for o in mm.overlaps.iter().take(5) {
            add("error", "overlap", format!("\"{}\" overlaps \"{}\" ({}px²).", o.a, o.b, n(o.area)));
        }
        for a in mm.anchors.iter().filter(|a| !a.in_view && r.screen != "_system") {
            let grow = if r.stress { " even when lists grow: scroll the list inside its container" } else { "" };
            add("error", "anchor", format!("anchored \"{}\" is not fully visible (top {}, bottom {}, viewport {}). Anchored feedback and primary actions must stay on screen{grow}.", a.name, n(a.top), n(a.bottom), n(mm.vh)));
        }
        for c in mm.clipped.iter().take(3) {
            add("warn", "clipped", format!("text clipped in \"{}\" (\"{}…\").", c.el, c.text));
        }
        if !mm.small.is_empty() {
            // `new Map(small.map((x) => [x.el, x]))`: the place of the first, the sizes of the last.
            let mut uniq: Ordered<Small> = Vec::new();
            for x in &mm.small {
                *entry(&mut uniq, &x.el) = x.clone();
            }
            let shown: Vec<String> = uniq.iter().take(5).map(|(_, x)| format!("\"{}\" {}x{}", x.el, n(x.w), n(x.h))).collect();
            let more = if uniq.len() > 5 { ", …" } else { "" };
            add("warn", "touch-target", format!("{} tappable element(s) smaller than 44x44px on a touch screen: {}{more}. Size controls with var(--size-target-touch) (44px); var(--size-target-wcag) (24px) is only the legal floor.", mm.small.len(), shown.join(", ")));
        }
        if !r.stress {
            for v in find_voids(mm, 0.22) {
                match v {
                    Void::Gap { from, height } => add("warn", "void", format!("empty band of {}px between content blocks (from y={}). Close it, or let the block above fill the space.", n(height), n(from))),
                    Void::Tail { height, fill } if r.screen != "_system" => add("warn", "void", format!("content ends early: {}px empty at the bottom of the first screen ({}% of the view filled). Fill the height (task layouts) or tighten the layout.", n(height), n(fill))),
                    Void::Tail { .. } => {}
                    Void::Sparse { fill } => add("warn", "void", format!("only {}% of the first screen holds content.", n(fill))),
                }
            }
        }
    }
    let plain = results.iter().filter_map(|(r, m)| match m {
        Ok(m) if !r.stress && !r.reflow && r.screen != "_system" => Some((r, m)),
        _ => None,
    });
    // Cross-screen consistency: component key -> computed signature -> screens.
    let mut sigs: Ordered<Ordered<Vec<String>>> = Vec::new();
    for (r, m) in plain.clone() {
        for c in &m.components {
            let key = format!("{}/{}/{}@{}", c.c, c.v, c.s, r.bp.id);
            let screens = entry(entry(&mut sigs, &key), &c.sig);
            if !screens.contains(&r.screen) {
                screens.push(r.screen.clone());
            }
        }
    }
    let before_at = |key: &str| key.split('@').next().unwrap_or_default().to_string();
    for (key, by_sig) in sigs.iter().filter(|(_, by_sig)| by_sig.len() > 1) {
        let variants: Vec<String> = by_sig.iter().map(|(sig, screens)| format!("{}: {sig}", screens.join(", "))).collect();
        findings.push(layout("warn", "consistency", "design/design.json", format!("Component {} renders differently across screens ({}). A screen is overriding the master; fix the master or the screen.", before_at(key), variants.join(" | "))));
    }
    // Variants that only restyle the same structure should be themes (compare at the same breakpoint, default state).
    struct Shape<'a> {
        variant: &'a str,
        sig: String,
        n: usize,
        rel: &'a str,
    }
    let mut structure: Ordered<Vec<Shape>> = Vec::new();
    for (r, m) in plain.filter(|(r, _)| r.state().is_none()) {
        let names: Vec<String> = m.components.iter().map(|c| format!("{}/{}", c.c, c.v)).collect();
        entry(&mut structure, &format!("{}@{}", r.screen, r.bp.id)).push(Shape { variant: &r.variant, sig: names.join("|"), n: names.len(), rel: &r.rel });
    }
    let mut seen_pairs: Vec<String> = Vec::new();
    for (key, shapes) in &structure {
        let screen = before_at(key);
        for (i, a) in shapes.iter().enumerate() {
            for b in &shapes[i + 1..] {
                let pair = format!("{screen}:{}:{}", a.variant, b.variant);
                if a.n < 3 || a.sig != b.sig || a.rel == b.rel || seen_pairs.contains(&pair) {
                    continue;
                }
                seen_pairs.push(pair);
                findings.push(layout("warn", "variant-cosmetic", b.rel, format!("{screen}: variants \"{}\" and \"{}\" have the same component structure ({} components in the same order). A variant should be a different layout or flow; a restyle belongs in a theme (design/tokens/themes/).", a.variant, b.variant, a.n)));
            }
        }
    }
    findings
}

#[cfg(test)]
#[rustfmt::skip]
mod tests {
    use super::*;

    fn job(screen: &str, variant: &str, bp: &str, width: u32) -> RenderJob {
        RenderJob {
            screen: screen.into(),
            variant: variant.into(),
            rel: format!("design/screens/{screen}/{variant}.html"),
            bp: Breakpoint { id: bp.into(), label: None, width, height: 700 },
            state: None,
            stress: false,
            reflow: false,
            system: false,
        }
    }

    fn measured(value: Value) -> Result<Measure, String> {
        Ok(serde_json::from_value(value).unwrap())
    }

    fn lines(results: &[(RenderJob, Result<Measure, String>)]) -> Vec<String> {
        findings_of(results).iter().map(|f| format!("{} {} {} {}", f.level, f.rule, f.file, f.message)).collect()
    }

    /// The JavaScript that runs in the page is the Node source, byte for byte.
    #[test]
    fn page_script_is_the_function_of_layout_mjs() {
        let node = concat!(env!("CARGO_MANIFEST_DIR"), "/../core/lib/layout.mjs");
        let source = fs::read_to_string(node).unwrap();
        assert!(MEASURE.starts_with("function measure(opts) {\n"));
        assert!(source.contains(&format!("\n{MEASURE}\n/** Whitespace analysis")));
        for arrow in [SET_STATE, ENLARGE_TEXT] {
            assert!(source.contains(&format!("page.evaluate({arrow}")), "{arrow}");
        }
    }

    /// layout: void detection on measured blocks
    #[test]
    fn layout_void_detection_on_measured_blocks() {
        let base = |blocks: Value| -> Measure {
            serde_json::from_value(json!({ "vw": 1366, "vh": 768, "content": { "x": 0, "w": 1366, "top": 56, "docH": 768 }, "blocks": blocks })).unwrap()
        };
        // Content ends at y=300 on a 768px screen: a tail void.
        let tail = find_voids(&base(json!([{ "x": 100, "y": 80, "w": 900, "h": 220 }])), 0.22);
        assert!(matches!(tail[0], Void::Tail { .. }), "{tail:?}");
        // Two blocks with a 400px empty band between them.
        let gap = find_voids(&base(json!([{ "x": 100, "y": 80, "w": 900, "h": 200 }, { "x": 100, "y": 700, "w": 900, "h": 500 }])), 0.22);
        assert!(gap.iter().any(|v| matches!(v, Void::Gap { height, .. } if *height >= 400.0)), "{gap:?}");
        // A filled screen: no voids.
        assert_eq!(find_voids(&base(json!([{ "x": 0, "y": 56, "w": 1366, "h": 712 }])), 0.22), []);
    }

    #[test]
    fn voids_without_content_or_blocks_are_none() {
        assert_eq!(find_voids(&Measure::default(), 0.22), []);
        let no_blocks = Measure { content: Some(Content::default()), ..Measure::default() };
        assert_eq!(find_voids(&no_blocks, 0.22), []);
    }

    #[test]
    fn jobs_cover_variants_breakpoints_states_stress_and_reflow() {
        let manifest = json!({ "states": ["empty"], "screens": [
            { "id": "a", "states": ["error", "loading"], "variants": [{ "id": "v1", "file": "screens/a/v1.html" }] },
            { "id": "b", "variants": [{ "id": "v1", "file": "screens/b/v1.html" }, { "id": "v2", "file": "screens/b/v2.html" }] },
        ] });
        let bps = [job("", "", "desktop", 1280).bp, job("", "", "mobile", 390).bp];
        let name = |j: &RenderJob| {
            let mode = if j.reflow { " reflow" } else if j.stress { " stress" } else { "" };
            format!("{}/{}@{}{}{mode}", j.screen, j.variant, j.bp.id, j.state.as_ref().map(|s| format!("[{s}]")).unwrap_or_default())
        };
        let names = |opts: &LayoutOptions| screen_jobs(&manifest, &bps, opts).iter().map(name).collect::<Vec<_>>();
        let only_a = LayoutOptions { screens: Some(vec!["a".into()]), states: true, ..LayoutOptions::default() };
        assert_eq!(names(&only_a), [
            "a/v1@reflow reflow", "a/v1@desktop", "a/v1@desktop stress", "a/v1@desktop[error]", "a/v1@desktop[loading]",
            "a/v1@mobile", "a/v1@mobile stress", "a/v1@mobile[error]", "a/v1@mobile[loading]",
        ]);
        let plain = LayoutOptions { stress: false, ..LayoutOptions::default() };
        assert_eq!(names(&plain), ["a/v1@desktop", "a/v1@mobile", "b/v1@desktop", "b/v1@mobile", "b/v2@desktop", "b/v2@mobile"]);
        let b_states = LayoutOptions { screens: Some(vec!["b".into()]), states: true, stress: false, ..LayoutOptions::default() };
        assert_eq!(names(&b_states)[..2], ["b/v1@desktop", "b/v1@desktop[empty]"]);
        let none = LayoutOptions { screens: Some(vec![]), ..LayoutOptions::default() };
        assert_eq!(names(&none), Vec::<String>::new());
        assert_eq!(screen_jobs(&manifest, &bps, &plain)[0].rel, "design/screens/a/v1.html");
    }

    #[test]
    fn findings_per_render_carry_the_messages_of_layout_mjs() {
        let m = json!({ "vw": 390, "vh": 700, "hOverflow": 24, "blocks": [], "content": null,
            "overlaps": [{ "a": "title", "b": "badge", "area": 1200 }],
            "clipped": [{ "el": "card", "text": "Long text" }],
            "anchors": [{ "name": "save", "top": 720, "bottom": 764, "left": 0, "right": 90, "inView": false }, { "name": "ok", "top": 1, "bottom": 2, "inView": true }],
            "small": [{ "el": "tiny", "w": 120, "h": 30 }, { "el": "x", "w": 20, "h": 20 }, { "el": "tiny", "w": 121, "h": 31 }],
            "components": [] });
        let mut stressed = job("s", "a", "mobile", 390);
        stressed.stress = true;
        let mut state = job("s", "a", "mobile", 390);
        state.state = Some("empty".into());
        let mut reflow = job("s", "a", "reflow", 320);
        reflow.reflow = true;
        let mut system = job("_system", "page", "desktop", 1280);
        system.system = true;
        let results = vec![
            (job("s", "a", "mobile", 390), measured(m.clone())),
            (stressed, measured(json!({ "vh": 700, "anchors": m["anchors"] }))),
            (state, Err("x".repeat(130))),
            (reflow, measured(m.clone())),
            (system, measured(json!({ "vh": 900, "anchors": m["anchors"], "content": { "x": 0, "w": 1280, "top": 0 }, "blocks": [{ "x": 0, "y": 0, "w": 1280, "h": 300 }] }))),
        ];
        let f = "design/screens/s/a.html";
        assert_eq!(lines(&results), [
            format!("error h-overflow {f} s/a @ mobile: page is 24px wider than the viewport."),
            format!("error overlap {f} s/a @ mobile: \"title\" overlaps \"badge\" (1200px²)."),
            format!("error anchor {f} s/a @ mobile: anchored \"save\" is not fully visible (top 720, bottom 764, viewport 700). Anchored feedback and primary actions must stay on screen."),
            format!("warn clipped {f} s/a @ mobile: text clipped in \"card\" (\"Long text…\")."),
            format!("warn touch-target {f} s/a @ mobile: 3 tappable element(s) smaller than 44x44px on a touch screen: \"tiny\" 121x31, \"x\" 20x20. Size controls with var(--size-target-touch) (44px); var(--size-target-wcag) (24px) is only the legal floor."),
            format!("error anchor {f} s/a @ mobile [stress: long lists]: anchored \"save\" is not fully visible (top 720, bottom 764, viewport 700). Anchored feedback and primary actions must stay on screen even when lists grow: scroll the list inside its container."),
            format!("warn render {f} s/a @ mobile [empty]: could not render ({})", "x".repeat(120)),
            format!("warn reflow {f} s/a @ 320px, text 125%: page scrolls sideways by 24px. Content should reflow to one column at 320px (WCAG 1.4.10); scroll only inside data tables."),
            format!("warn reflow {f} s/a @ 320px, text 125%: \"title\" overlaps \"badge\" when text is enlarged."),
            format!("warn reflow {f} s/a @ 320px, text 125%: text clipped in \"card\" when text is enlarged (\"Long text…\"). Let boxes grow with their text."),
        ]);
    }

    #[test]
    fn findings_across_screens_consistency_and_cosmetic_variants() {
        let comps = |pad: &str| json!({ "vh": 700, "components": [
            { "c": "button", "v": "primary", "s": "", "sig": format!("14px 600 normal {pad} 16px 6px") },
            { "c": "card", "v": "", "s": "", "sig": "sig" },
            { "c": "stat", "v": "", "s": "", "sig": "sig" },
        ] });
        let results = vec![
            (job("home", "a", "desktop", 1280), measured(comps("8px"))),
            (job("home", "b", "desktop", 1280), measured(comps("8px"))),
            (job("list", "a", "desktop", 1280), measured(comps("12px"))),
            (job("list", "a", "mobile", 390), measured(comps("12px"))),
        ];
        assert_eq!(lines(&results), [
            "warn consistency design/design.json Component button/primary/ renders differently across screens (home: 14px 600 normal 8px 16px 6px | list: 14px 600 normal 12px 16px 6px). A screen is overriding the master; fix the master or the screen.",
            "warn variant-cosmetic design/screens/home/b.html home: variants \"a\" and \"b\" have the same component structure (3 components in the same order). A variant should be a different layout or flow; a restyle belongs in a theme (design/tokens/themes/).",
        ]);
        assert_eq!(
            crate::core::util::to_json_compact(&findings_of(&results)[1].to_json()).split("\"message\"").next().unwrap(),
            "{\"level\":\"warn\",\"rule\":\"variant-cosmetic\",\"file\":\"design/screens/home/b.html\",\"line\":1,"
        );
    }

    #[test]
    fn file_urls_are_percent_encoded() {
        assert_eq!(file_url(Path::new("/tmp/a b/é#1?.html")), "file:///tmp/a%20b/%C3%A9%231%3F.html");
        assert_eq!(file_url(Path::new("/x/../y/100%.html")), "file:///y/100%25.html");
    }
}
