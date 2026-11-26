//! Browser commands of `core/lib/cli.mjs`: shoot, layout, diff (`cmdShoot`). The a11y check and the token
//! extraction have no command of their own: they run inside `check` and `tokens extract`.

use std::path::Path;

use serde_json::{json, Value};

use super::word::{command, sub};
use super::{Ctx, Handler};
use crate::browser::a11y::{a11y_check, A11yOptions};
use crate::browser::layout_check::{layout_check, LayoutOptions, LayoutResult};
use crate::browser::pixeldiff::{decode_rgba, diff_images, diff_pairs, format_line, THRESHOLD};
use crate::browser::screenshot::{screenshot, CONCURRENCY};
use crate::browser::shoot::{shoot_breakpoints, studio_jobs, url_jobs, ShootOptions};
use crate::core::error::exit;
use crate::core::error::DfwError;
use crate::core::level::Level;
use crate::core::paths::resolve;
use crate::core::util::{out, sha256, to_json_pretty, Args};
use crate::design::layout_cache::{changed_screens, mark_checked};
use crate::design::tokens::relative_path as relative;
use crate::tools::check::field as result_key;

/// The keys of what `dfw layout --json` prints, besides the check result's.
mod layout_key {
    /// The number of pages rendered.
    pub const RENDERS: &str = "renders";
}

/// The keys of what `__debug png-info` prints.
mod png_key {
    pub const WIDTH: &str = "width";
    pub const HEIGHT: &str = "height";
    /// The SHA-256 of the decoded RGBA pixels.
    pub const SHA256: &str = "sha256";
}

/// The commands of this group.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        (command::SHOOT, shoot as Handler),
        (command::LAYOUT, layout as Handler),
        (command::DIFF, diff as Handler),
    ]
}

/// A value flag's text, nothing when it is missing or empty. Given alone it never gets here
/// (`require_flag_values`).
fn flag(a: &Args, key: &str) -> Option<String> {
    a.text(key).filter(|t| !t.is_empty()).map(str::to_string)
}

/// The flags of `dfw shoot` as [`ShootOptions`].
fn shoot_options(a: &Args) -> ShootOptions {
    ShootOptions {
        out: a.text("out").filter(|o| !o.is_empty()).map(str::to_string),
        viewport: a.truthy("viewport"),
        selector: flag(a, "selector"),
        system: a.truthy("system"),
        screen: flag(a, "screen"),
        variant: flag(a, "variant"),
        page: flag(a, "page"),
        states: a.truthy("states"),
        state: flag(a, "state"),
        themes: a.truthy("themes"),
        theme: flag(a, "theme"),
    }
}

/// `dfw shoot`: screenshots in one browser run.
fn shoot(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let bps = shoot_breakpoints(&root, flag(a, "bp").as_deref())?;
    let opts = shoot_options(a);
    let jobs = match flag(a, "url") {
        Some(url) => url_jobs(&root, &url, &bps, &opts)?,
        None => studio_jobs(&root, &bps, &opts)?,
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
    Ok(if results.iter().all(|r| r.ok) {
        exit::OK
    } else {
        exit::PROBLEMS
    })
}

/// `dfw layout check [--screen a,b] [--bp ids] [--states] [--no-stress] [--changed]
/// [--no-system] [--json]`: the rendered layout check.
fn layout(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    if a.pos(1).filter(|sub| !sub.is_empty()).unwrap_or(sub::CHECK) != sub::CHECK {
        return Err(DfwError::new("Usage: dfw layout check [--screen a,b] [--bp ids] [--states] [--no-stress] [--changed] [--no-system] [--json]"));
    }
    let root = ctx.root();
    let ids = |key: &str| flag(a, key).map(|v| v.split(',').map(str::to_string).collect());
    let mut screens: Option<Vec<String>> = ids("screen");
    let mut system = !a.truthy("no-system");
    if a.truthy("changed") {
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
    let errors = findings
        .iter()
        .filter(|f| f.level == Level::Error.as_str())
        .count();
    if errors == 0 {
        mark_checked(&root)?;
    }
    let code = if errors > 0 { exit::PROBLEMS } else { exit::OK };
    if a.truthy("json") {
        let findings: Vec<Value> = findings.iter().map(|f| f.to_json()).collect();
        out(&to_json_pretty(
            &json!({ (result_key::STATUS): "ran", (result_key::FINDINGS): findings, (layout_key::RENDERS): renders }),
        ));
        return Ok(code);
    }
    let mut lines = vec![format!(
        "Layout: {renders} render(s), {errors} must, {} should.",
        findings.len() - errors
    )];
    for f in &findings {
        let label = Level::parse(f.level).unwrap_or(Level::Info).label();
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
    let threshold = match a.text("threshold") {
        Some(t) => t
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

/// `__debug png-info <file>`: size and a hash of the decoded pixels of a PNG, so the golden
/// tests can compare images whatever encoder wrote them.
pub(super) fn debug_png_info(ctx: &Ctx) -> Result<i32, DfwError> {
    let file = ctx.args.pos(2).unwrap_or_default();
    let image = decode_rgba(Path::new(file)).map_err(DfwError::new)?;
    out(&json!({ (png_key::WIDTH): image.w, (png_key::HEIGHT): image.h, (png_key::SHA256): sha256(&image.rgba) }).to_string());
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
    use crate::cli::word::flag;
    use crate::core::util::parse_args;

    /// The file `--out` names.
    const OUT_FILE: &str = "a.png";
    /// The screen `--screen` names.
    const SCREEN: &str = "cart";
    /// The theme `--theme` names.
    const THEME: &str = "dark";
    /// The folder paths are made relative to.
    const BASE: &str = "/a/b";

    #[test]
    fn shoot_flags_become_options() {
        let opts = shoot_options(&parse_args(&[
            command::SHOOT,
            flag::OUT,
            OUT_FILE,
            "--viewport",
            "--selector",
            "#a",
            flag::SCREEN,
            SCREEN,
            "--page",
            "--states",
            "--theme",
            THEME,
        ]));
        assert_eq!(
            opts,
            ShootOptions {
                out: Some(OUT_FILE.into()),
                viewport: true,
                selector: Some("#a".into()),
                screen: Some(SCREEN.into()),
                // `--page` alone is refused before (`require_flag_values`); here it reads as none.
                page: None,
                states: true,
                theme: Some(THEME.into()),
                ..ShootOptions::default()
            }
        );
        assert_eq!(
            shoot_options(&parse_args(&[
                command::SHOOT,
                flag::OUT,
                "",
                flag::SCREEN,
                ""
            ])),
            ShootOptions::default()
        );
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative(Path::new(BASE), Path::new("/a/b/c/d.png")),
            "c/d.png"
        );
        assert_eq!(
            relative(Path::new(BASE), Path::new("/a/x/d.png")),
            "../x/d.png"
        );
        assert_eq!(relative(Path::new(BASE), Path::new(BASE)), "");
    }
}
