//! Browser commands of `core/lib/cli.mjs`: shoot, layout, diff (`cmdShoot`). The a11y check and the token
//! extraction have no command of their own: they run inside `check` and `tokens extract`.

use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::{json, Value};

use super::{Ctx, Handler};
use crate::browser::a11y::{a11y_check, A11yOptions};
use crate::browser::layout_check::{
    changed_screens, layout_check, mark_checked, LayoutOptions, LayoutResult,
};
use crate::browser::pixeldiff::{decode_rgba, diff_images, diff_pairs, format_line, THRESHOLD};
use crate::browser::screenshot::{screenshot, Job, CONCURRENCY};
use crate::core::config::{load_config, studio_breakpoints, Breakpoint};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::{out, read_json, sha256, to_json_pretty, ArgValue, Args};

/// The commands of this group.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("shoot", Handler::Native(shoot)),
        ("layout", Handler::Native(layout)),
        ("diff", Handler::Native(diff)),
    ]
}

/// A flag as cli.mjs uses it in a string: its text, "true" when it stood alone, nothing when it
/// is missing or empty.
fn flag(a: &Args, key: &str) -> Option<String> {
    match a.get(key) {
        Some(ArgValue::Flag) => Some("true".to_string()),
        Some(ArgValue::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// `path.relative(from, to)` for two absolute, normalised paths.
fn relative(from: &Path, to: &Path) -> String {
    let (f, t): (Vec<Component>, Vec<Component>) =
        (from.components().collect(), to.components().collect());
    let common = f.iter().zip(&t).take_while(|(a, b)| a == b).count();
    let mut rel = PathBuf::new();
    for _ in common..f.len() {
        rel.push("..");
    }
    for part in &t[common..] {
        rel.push(part);
    }
    rel.display().to_string()
}

/// `isAlive(pid)`: whether a process with this id exists and may be signalled.
// TODO(merge): use the studio group's is_alive
fn is_alive(pid: Option<&Value>) -> bool {
    // Only a positive integer names one process: 0 and negative numbers address process groups.
    let Some(pid) = pid.and_then(super::workflow::positive_pid) else {
        return false;
    };
    let mut probe = if cfg!(windows) {
        let mut c = Command::new("tasklist");
        c.args(["/NH", "/FI", &format!("PID eq {pid}")]);
        c
    } else {
        let mut c = Command::new("kill");
        c.args(["-0", &pid.to_string()]);
        c
    };
    probe.stdin(Stdio::null()).stderr(Stdio::null());
    if cfg!(windows) {
        probe
            .output()
            .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!(" {pid} ")))
    } else {
        probe
            .stdout(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }
}

/// `new URL(url).pathname` for the default screenshot name. Not the whole WHATWG parser: dot
/// segments and percent-encoding are left as written.
fn url_pathname(url: &str) -> Result<String, DfwError> {
    let invalid = || DfwError::internal("Invalid URL");
    let colon = url.find(':').ok_or_else(invalid)?;
    let scheme = url[..colon].to_ascii_lowercase();
    let well_formed = scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !well_formed {
        return Err(invalid());
    }
    let rest = &url[colon + 1..];
    let path_end = |s: &str| s.find(['?', '#']).unwrap_or(s.len());
    let special = ["http", "https", "ws", "wss", "ftp", "file"].contains(&scheme.as_str());
    if !special && !rest.starts_with("//") {
        return Ok(rest[..path_end(rest)].to_string());
    }
    // The authority follows "//"; a special scheme written with fewer slashes still has one.
    let after = rest
        .strip_prefix("//")
        .unwrap_or_else(|| rest.trim_start_matches('/'));
    let authority_end = after.find(['/', '\\', '?', '#']).unwrap_or(after.len());
    if special && scheme != "file" && authority_end == 0 {
        return Err(invalid());
    }
    let path = &after[authority_end..];
    let path = &path[..path_end(path)];
    Ok(if path.is_empty() && special {
        "/".to_string()
    } else {
        path.to_string()
    })
}

/// `str.replace(/\W+/g, '_')` (or another class of kept characters).
fn squash(text: &str, keep: impl Fn(char) -> bool) -> String {
    let mut out = String::new();
    let mut gap = false;
    for c in text.chars() {
        if keep(c) {
            out.push(c);
            gap = false;
        } else if !gap {
            out.push('_');
            gap = true;
        }
    }
    out
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// `new URLSearchParams()` with `theme` and `state` set, as text.
fn query(theme: Option<&str>, state: Option<&str>) -> String {
    let encode = |v: &str| {
        let mut out = String::new();
        for b in v.bytes() {
            match b {
                b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                    out.push(char::from(b))
                }
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    };
    let pairs: Vec<String> = [("theme", theme), ("state", state)]
        .iter()
        .filter_map(|(k, v)| v.map(|v| format!("{k}={}", encode(v))))
        .collect();
    pairs.join("&")
}

/// The breakpoints to shoot: those of design/design.json, else of the config, kept when `--bp`
/// names them.
fn breakpoints(a: &Args, root: &Path) -> Result<Vec<Breakpoint>, DfwError> {
    let config = load_config(Some(root))?;
    let manifest = read_json(&project_paths(root).manifest, Some(json!({})))?;
    let all = match manifest.get("breakpoints").filter(|b| js::truthy(b)) {
        Some(list) => list
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Breakpoint::from_value)
            .collect(),
        None => studio_breakpoints(&config),
    };
    Ok(match flag(a, "bp") {
        Some(ids) => {
            let ids: Vec<&str> = ids.split(',').collect();
            all.into_iter()
                .filter(|b| ids.contains(&b.id.as_str()))
                .collect()
        }
        None => all,
    })
}

/// Screenshots of any URL: `--url <u> [--out <file>]`, one per breakpoint.
fn url_jobs(a: &Args, root: &Path, url: &str, bps: &[Breakpoint]) -> Result<Vec<Job>, DfwError> {
    let mut jobs = Vec::new();
    for bp in bps {
        let out_file = match a.text("out").filter(|o| !o.is_empty()) {
            Some(name) => {
                // Several breakpoints into one --out name would overwrite each other: suffix
                // the breakpoint.
                let name = if !name.contains("{bp}") && bps.len() > 1 {
                    // name.replace(/(\.\w+)?$/, (ext) => `__{bp}${ext || '.png'}`)
                    let ext_at = name
                        .rfind('.')
                        .filter(|at| name.len() > at + 1 && name[at + 1..].chars().all(word));
                    match ext_at {
                        Some(at) => format!("{}__{{bp}}{}", &name[..at], &name[at..]),
                        None => format!("{name}__{{bp}}.png"),
                    }
                } else {
                    name.to_string()
                };
                resolve(name.replacen("{bp}", &bp.id, 1))
            }
            None => {
                let page = squash(&url_pathname(url)?, word);
                let page = if page.is_empty() { "root" } else { &page };
                project_paths(root)
                    .dfw
                    .join("shots")
                    .join("url")
                    .join(format!("{page}__{}.png", bp.id))
            }
        };
        jobs.push(Job {
            url: url.to_string(),
            width: bp.width,
            height: bp.height,
            out: out_file,
            full_page: !a.truthy("viewport"),
            selector: flag(a, "selector"),
            scale: 1.0,
        });
    }
    Ok(jobs)
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(str::to_string))
        .collect()
}

fn text<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// Screenshots of the design, served by the running Studio: screens x variants x breakpoints
/// x states x themes, and the design system page with `--system`.
fn studio_jobs(a: &Args, root: &Path, bps: &[Breakpoint]) -> Result<Vec<Job>, DfwError> {
    let p = project_paths(root);
    let info = read_json(&p.studio, Some(Value::Null))?;
    if !js::truthy(&info) || !is_alive(info.get("pid")) {
        return Err(DfwError::new(
            "Start the studio first (dfw studio start) or pass --url.",
        ));
    }
    let m = read_json(&p.manifest, Some(Value::Null))?;
    if !js::truthy(&m) {
        return Err(DfwError::new("design/design.json not found."));
    }
    // TODO(merge): use workflow::state::load_state
    let state = read_json(&p.state, Some(Value::Null))?;
    let version = match state.get("design").and_then(|d| d.get("version")) {
        Some(Value::Number(n)) => format!("v{n}"),
        Some(Value::String(s)) => format!("v{s}"),
        _ => "v1".to_string(),
    };
    let screens_all: Vec<&Value> = m
        .get("screens")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .collect();
    let page_screens: Option<Vec<String>> = flag(a, "page").map(|page| {
        let listed = m
            .get("pages")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .find(|pg| text(pg, "id") == page)
            .map(|pg| strings(pg.get("screens")))
            .unwrap_or_default();
        let tagged = screens_all
            .iter()
            .filter(|s| text(s, "page") == page)
            .map(|s| text(s, "id").to_string());
        listed.into_iter().chain(tagged).collect()
    });
    let studio_url = text(&info, "url");
    let shots_dir = p.dfw.join("shots").join(version);
    let name = |parts: &[&str]| {
        let kept: Vec<&str> = parts.iter().copied().filter(|s| !s.is_empty()).collect();
        format!("{}.png", kept.join("__"))
    };
    let (system, screen, variant, selector) = (
        a.truthy("system"),
        flag(a, "screen"),
        flag(a, "variant"),
        flag(a, "selector"),
    );
    let mut jobs = Vec::new();
    if system {
        if let Some(bp) = bps.first() {
            jobs.push(Job {
                url: format!("{studio_url}__dfw/system.html"),
                width: bp.width,
                height: bp.height,
                out: shots_dir.join(name(&["_system", &bp.id])),
                full_page: true,
                selector: None,
                scale: 1.0,
            });
        }
    }
    let screens = if system && screen.is_none() && page_screens.is_none() {
        Vec::new()
    } else {
        screens_all
    };
    let element = selector.as_ref().map(|s| {
        let cleaned = squash(s, |c| word(c) || c == '.' || c == '-');
        let cleaned: String = cleaned.trim_matches('_').chars().take(60).collect();
        format!("el-{cleaned}")
    });
    for s in screens {
        let id = text(s, "id");
        if screen.as_deref().is_some_and(|only| only != id) {
            continue;
        }
        if page_screens
            .as_ref()
            .is_some_and(|ids| !ids.iter().any(|x| x == id))
        {
            continue;
        }
        let states: Vec<Option<String>> = if a.truthy("states") {
            let own = s
                .get("states")
                .filter(|v| js::truthy(v))
                .or(m.get("states"));
            std::iter::once(None)
                .chain(strings(own).into_iter().map(Some))
                .collect()
        } else {
            vec![flag(a, "state")]
        };
        let themes: Vec<Option<String>> = if a.truthy("themes") {
            let all = strings(m.get("themes"));
            if all.is_empty() {
                vec![None]
            } else {
                all.into_iter().map(Some).collect()
            }
        } else {
            vec![flag(a, "theme")]
        };
        for v in s
            .get("variants")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if variant.as_deref().is_some_and(|only| only != text(v, "id")) {
                continue;
            }
            for bp in bps {
                for st in &states {
                    for th in &themes {
                        let q = query(th.as_deref(), st.as_deref());
                        let q = if q.is_empty() { q } else { format!("?{q}") };
                        jobs.push(Job {
                            url: format!("{studio_url}design/{}{q}", text(v, "file")),
                            width: bp.width,
                            height: bp.height,
                            out: shots_dir.join(name(&[
                                &id.replace('/', "__"),
                                text(v, "id"),
                                &bp.id,
                                th.as_deref().unwrap_or(""),
                                st.as_deref().unwrap_or(""),
                                element.as_deref().unwrap_or(""),
                            ])),
                            full_page: !a.truthy("viewport"),
                            selector: selector.clone(),
                            scale: 1.0,
                        });
                    }
                }
            }
        }
    }
    Ok(jobs)
}

/// `dfw shoot`: screenshots in one browser run.
fn shoot(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let bps = breakpoints(a, &root)?;
    let jobs = match flag(a, "url") {
        Some(url) => url_jobs(a, &root, &url, &bps)?,
        None => studio_jobs(a, &root, &bps)?,
    };
    if jobs.is_empty() {
        return Err(DfwError::new(
            "Nothing to screenshot (check --screen/--variant/--bp).",
        ));
    }
    let results = screenshot(&jobs, CONCURRENCY)?;
    let lines: Vec<String> = results
        .iter()
        .map(|r| {
            let error = r
                .error
                .as_ref()
                .map(|e| format!(" ({e})"))
                .unwrap_or_default();
            format!(
                "{} {}{error}",
                if r.ok { "ok  " } else { "FAIL" },
                relative(&root, &r.out)
            )
        })
        .collect();
    out(&lines.join("\n"));
    Ok(if results.iter().all(|r| r.ok) { 0 } else { 2 })
}

/// `dfw layout check [--screen a,b] [--bp ids] [--states] [--no-stress] [--changed]
/// [--no-system] [--json]`: the rendered layout check.
fn layout(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    if a.pos(1).filter(|sub| !sub.is_empty()).unwrap_or("check") != "check" {
        return Err(DfwError::new("Usage: dfw layout check [--screen a,b] [--bp ids] [--states] [--no-stress] [--changed] [--no-system] [--json]"));
    }
    let root = ctx.root();
    let ids = |key: &str| flag(a, key).map(|v| v.split(',').map(str::to_string).collect());
    let mut screens: Option<Vec<String>> = ids("screen");
    let mut system = !a.truthy("no-system");
    if a.truthy("changed") {
        // TODO(merge): design::layout_cache (changed_screens, mark_checked)
        let ch = changed_screens(&root)?;
        if ch.is_empty() {
            out("Layout: nothing changed since the last check.");
            return Ok(0);
        }
        system = system && !ch.changed_comps.is_empty();
        screens = Some(ch.screens);
    }
    let opts = LayoutOptions {
        screens,
        bp: ids("bp"),
        states: a.truthy("states"),
        stress: !a.truthy("no-stress"),
        system,
    };
    let (findings, renders) = match layout_check(&root, &opts)? {
        LayoutResult::Unavailable { detail } => {
            out(&format!("Layout check unavailable: {detail}"));
            return Ok(0);
        }
        LayoutResult::Ran { findings, renders } => (findings, renders),
    };
    let errors = findings.iter().filter(|f| f.level == "error").count();
    if errors == 0 {
        mark_checked(&root)?;
    }
    let code = if errors > 0 { 2 } else { 0 };
    if a.truthy("json") {
        let findings: Vec<Value> = findings.iter().map(|f| f.to_json()).collect();
        out(&to_json_pretty(
            &json!({ "status": "ran", "findings": findings, "renders": renders }),
        ));
        return Ok(code);
    }
    let mut lines = vec![format!(
        "Layout: {renders} render(s), {errors} must, {} should.",
        findings.len() - errors
    )];
    for f in &findings {
        let label = match f.level {
            "error" => "MUST",
            "warn" => "SHOULD",
            _ => "NOTE",
        };
        lines.push(format!("- [{label}] ({}) {}", f.rule, f.message));
    }
    out(&lines.join("\n"));
    Ok(code)
}

/// `dfw diff <a.png|dirA> <b.png|dirB> [--out] [--threshold 0.1] [--json]`: pixel diff. Unlike
/// the Node CLI it needs no browser.
fn diff(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let (Some(first), Some(second)) = (a.pos(1), a.pos(2)) else {
        return Err(DfwError::new("Usage: dfw diff <a.png|dirA> <b.png|dirB> [--out <file|dir>] [--threshold 0.1] [--json]"));
    };
    let out_arg = flag(a, "out").map(resolve);
    let pairs = diff_pairs(&resolve(first), &resolve(second), out_arg.as_deref())?;
    // Number(a.threshold) || 0.1
    let threshold = match a.get("threshold") {
        Some(ArgValue::Flag) => 1.0,
        Some(ArgValue::Text(t)) => t
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|n| *n != 0.0 && !n.is_nan())
            .unwrap_or(THRESHOLD),
        None => THRESHOLD,
    };
    let mut results = diff_images(&pairs, threshold);
    if a.truthy("json") {
        out(&to_json_pretty(&Value::Array(
            results.iter().map(|r| r.to_value()).collect(),
        )));
        return Ok(0);
    }
    let ratio =
        |r: &crate::browser::pixeldiff::DiffResult| r.outcome.as_ref().map_or(0.0, |d| d.ratio);
    results.sort_by(|x, y| ratio(y).total_cmp(&ratio(x)));
    let cwd = resolve(".");
    let lines: Vec<String> = results
        .iter()
        .map(|r| {
            format_line(
                r,
                &r.pair
                    .out
                    .as_ref()
                    .map(|o| relative(&cwd, o))
                    .unwrap_or_default(),
            )
        })
        .collect();
    out(&lines.join("\n"));
    Ok(0)
}

/// `__debug png-info <file>`: size and a hash of the decoded pixels of a PNG, so the conformance
/// tests can compare images whatever encoder wrote them.
pub(super) fn debug_png_info(ctx: &Ctx) -> Result<i32, DfwError> {
    let file = ctx.args.pos(2).unwrap_or_default();
    let image = decode_rgba(Path::new(file)).map_err(DfwError::new)?;
    out(&json!({ "width": image.w, "height": image.h, "sha256": sha256(&image.rgba) }).to_string());
    Ok(0)
}

/// `__debug a11y [--url <u>] [--screen a,b] [--states] [--themes]`:
/// `JSON.stringify(await a11yCheck(root, opts), null, 2)`. The check has no command of its own
/// until `dfw check` is ported.
pub(super) fn debug_a11y(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let opts = A11yOptions {
        url: flag(a, "url"),
        screens: flag(a, "screen").map(|v| v.split(',').map(str::to_string).collect()),
        states: a.truthy("states"),
        themes: a.truthy("themes"),
    };
    out(&to_json_pretty(&a11y_check(&ctx.root(), &opts)?.to_json()));
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::{parse_args, test_dir, write_json};

    fn bp(id: &str, width: u32, height: u32) -> Breakpoint {
        Breakpoint {
            id: id.into(),
            label: None,
            width,
            height,
        }
    }

    #[test]
    fn url_jobs_name_their_files_like_cmd_shoot() {
        let root = test_dir("shoot-url");
        let both = [bp("mobile", 390, 844), bp("desktop", 1440, 900)];
        let outs = |argv: &[&str], bps: &[Breakpoint]| -> Vec<String> {
            let a = parse_args(argv);
            let url = flag(&a, "url").unwrap();
            url_jobs(&a, &root, &url, bps)
                .unwrap()
                .iter()
                .map(|j| relative(&root, &j.out))
                .collect()
        };
        assert_eq!(
            outs(
                &["shoot", "--url", "http://localhost:3000/cart/items?x=1#top"],
                &both
            ),
            [
                ".dfw/shots/url/_cart_items__mobile.png",
                ".dfw/shots/url/_cart_items__desktop.png"
            ]
        );
        assert_eq!(
            outs(&["shoot", "--url", "http://localhost:3000"], &both[..1]),
            [".dfw/shots/url/___mobile.png"]
        );
        assert_eq!(
            outs(&["shoot", "--url", "data:,"], &both[..1]),
            [".dfw/shots/url/___mobile.png"]
        );
        assert_eq!(
            outs(&["shoot", "--url", "about:blank"], &both[..1]),
            [".dfw/shots/url/blank__mobile.png"]
        );
        let cwd = resolve(".");
        let out = |name: &str| relative(&root, &cwd.join(name));
        assert_eq!(
            outs(&["shoot", "--url", "http://x/", "--out", "shot.png"], &both),
            [out("shot__mobile.png"), out("shot__desktop.png")]
        );
        assert_eq!(
            outs(&["shoot", "--url", "http://x/", "--out", "a.b/shot"], &both),
            [out("a.b/shot__mobile.png"), out("a.b/shot__desktop.png")]
        );
        assert_eq!(
            outs(
                &["shoot", "--url", "http://x/", "--out", "s-{bp}.png"],
                &both
            ),
            [out("s-mobile.png"), out("s-desktop.png")]
        );
        assert_eq!(
            outs(
                &["shoot", "--url", "http://x/", "--out", "one.png"],
                &both[..1]
            ),
            [out("one.png")]
        );
        let a = parse_args(&[
            "shoot",
            "--url",
            "http://x/",
            "--viewport",
            "--selector",
            "#a",
        ]);
        let job = &url_jobs(&a, &root, "http://x/", &both[..1]).unwrap()[0];
        assert_eq!(
            (
                job.full_page,
                job.selector.as_deref(),
                job.width,
                job.height
            ),
            (false, Some("#a"), 390, 844)
        );
        assert!(url_jobs(&parse_args(&["shoot"]), &root, "no scheme", &both).is_err());
    }

    #[test]
    fn pathname_of_common_urls() {
        for (url, path) in [
            ("http://localhost:3000", "/"),
            ("http://localhost:3000/a/b.html?q#h", "/a/b.html"),
            ("HTTPS://example.test/x", "/x"),
            ("file:///tmp/a.html", "/tmp/a.html"),
            ("data:text/html,<p>hi</p>", "text/html,<p>hi</p>"),
            ("about:blank", "blank"),
        ] {
            assert_eq!(url_pathname(url).unwrap(), path, "{url}");
        }
        for bad in ["localhost:3000/x", "/just/a/path", "http://", ""] {
            // "localhost:3000/x" parses in Node too (scheme "localhost"); the others are invalid.
            if bad == "localhost:3000/x" {
                assert_eq!(url_pathname(bad).unwrap(), "3000/x");
            } else {
                assert!(url_pathname(bad).is_err(), "{bad}");
            }
        }
    }

    #[test]
    fn studio_jobs_cover_screens_variants_breakpoints_states_and_themes() {
        let root = test_dir("shoot-studio");
        let p = project_paths(&root);
        let both = [bp("mobile", 390, 844), bp("desktop", 1440, 900)];
        let jobs = |argv: &[&str]| studio_jobs(&parse_args(argv), &root, &both);
        assert_eq!(
            jobs(&["shoot"]).unwrap_err().message(),
            "Start the studio first (dfw studio start) or pass --url."
        );
        // This test process is alive: its pid stands in for the Studio's.
        write_json(
            &p.studio,
            &json!({ "pid": std::process::id(), "url": "http://127.0.0.1:4600/" }),
        )
        .unwrap();
        assert_eq!(
            jobs(&["shoot"]).unwrap_err().message(),
            "design/design.json not found."
        );
        write_json(
            &p.manifest,
            &json!({
                "states": ["empty", "error"],
                "themes": ["dark"],
                "pages": [{ "id": "shop", "screens": ["cart"] }],
                "screens": [
                    { "id": "cart", "variants": [{ "id": "a", "file": "screens/cart/a.html" }, { "id": "b", "file": "screens/cart/b.html" }] },
                    { "id": "admin/users", "page": "back", "states": ["loading state"], "variants": [{ "id": "a", "file": "screens/admin/users/a.html" }] },
                ],
            }),
        )
        .unwrap();
        write_json(&p.state, &json!({ "design": { "version": 3 } })).unwrap();
        let list = |argv: &[&str]| -> Vec<(String, String)> {
            jobs(argv)
                .unwrap()
                .iter()
                .map(|j| (relative(&root, &j.out), j.url.clone()))
                .collect()
        };
        let all = list(&["shoot"]);
        assert_eq!(all.len(), 6);
        assert_eq!(
            all[0],
            (
                ".dfw/shots/v3/cart__a__mobile.png".to_string(),
                "http://127.0.0.1:4600/design/screens/cart/a.html".to_string()
            )
        );
        assert_eq!(all[5].0, ".dfw/shots/v3/admin__users__a__desktop.png");
        assert_eq!(
            list(&["shoot", "--screen", "cart", "--variant", "b"]).len(),
            2
        );
        assert_eq!(list(&["shoot", "--page", "shop"]).len(), 4);
        assert_eq!(list(&["shoot", "--page", "back"]).len(), 2);
        let states = list(&["shoot", "--screen", "admin/users", "--states", "--themes"]);
        assert_eq!(
            states
                .iter()
                .map(|(o, u)| (o.as_str(), u.rsplit('/').next().unwrap()))
                .collect::<Vec<_>>()[..2],
            [
                (
                    ".dfw/shots/v3/admin__users__a__mobile__dark.png",
                    "a.html?theme=dark"
                ),
                (
                    ".dfw/shots/v3/admin__users__a__mobile__dark__loading state.png",
                    "a.html?theme=dark&state=loading+state"
                ),
            ]
        );
        assert_eq!(
            list(&["shoot", "--screen", "cart", "--states"]).len(),
            2 * 2 * 3
        );
        let one = list(&[
            "shoot",
            "--screen",
            "cart",
            "--variant",
            "a",
            "--state",
            "empty",
            "--theme",
            "dark",
            "--selector",
            "#main > .card[data-x]",
        ]);
        assert_eq!(
            one[0].0,
            ".dfw/shots/v3/cart__a__mobile__dark__empty__el-main_.card_data-x.png"
        );
        let system = list(&["shoot", "--system"]);
        assert_eq!(
            system,
            [(
                ".dfw/shots/v3/_system__mobile.png".to_string(),
                "http://127.0.0.1:4600/__dfw/system.html".to_string()
            )]
        );
        assert_eq!(
            list(&["shoot", "--system", "--screen", "cart"]).len(),
            1 + 4
        );
        assert!(jobs(&["shoot", "--screen", "nope"]).unwrap().is_empty());
        // A Studio that is gone: its pid file is stale.
        write_json(
            &p.studio,
            &json!({ "pid": 0, "url": "http://127.0.0.1:4600/" }),
        )
        .unwrap();
        assert!(jobs(&["shoot"]).is_err());
    }

    #[test]
    fn breakpoints_come_from_the_design_then_the_config() {
        let root = test_dir("shoot-bp");
        let ids = |argv: &[&str]| -> Vec<String> {
            breakpoints(&parse_args(argv), &root)
                .unwrap()
                .into_iter()
                .map(|b| b.id)
                .collect()
        };
        assert_eq!(ids(&["shoot"]).len(), 3, "the config defaults");
        write_json(&project_paths(&root).manifest, &json!({ "breakpoints": [{ "id": "wide", "width": 1920, "height": 1080 }, { "id": "tiny", "width": 320, "height": 480 }] })).unwrap();
        assert_eq!(ids(&["shoot"]), ["wide", "tiny"]);
        assert_eq!(ids(&["shoot", "--bp", "tiny,nope"]), ["tiny"]);
        assert_eq!(ids(&["shoot", "--bp", "nope"]), Vec::<String>::new());
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative(Path::new("/a/b"), Path::new("/a/b/c/d.png")),
            "c/d.png"
        );
        assert_eq!(
            relative(Path::new("/a/b"), Path::new("/a/x/d.png")),
            "../x/d.png"
        );
        assert_eq!(relative(Path::new("/a/b"), Path::new("/a/b")), "");
    }
}
