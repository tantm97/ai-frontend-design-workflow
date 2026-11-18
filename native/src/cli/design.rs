//! Design commands of `core/lib/cli.mjs`: build, tokens, lint, check, system, style, type, palette, icons,
//! fonts, blueprint.
//!
//! Still on the Node CLI: `tokens extract` (browser), `tokens export|import` (penpot) and a
//! `check` that renders pages for the accessibility check (whole design or `--url` without
//! `--no-a11y`: browser/a11y).
// Over 800 lines: one arm of the cli.mjs switch per function, plus the `__debug` helpers.

use std::path::Path;

use serde_json::{json, Map, Value};

use super::{forward, Ctx, Handler};
use crate::core::config::env_map;
use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::{out, to_json_compact, to_json_pretty, write_json, ArgValue, Args};
use crate::design::color::{self, Names, Oklch, Ramp, Seeds, Signature};
use crate::design::compose::{
    build_system, compose_html, load_components, parse_component_file, ComposeOptions,
};
use crate::design::dsaudit::audit_design_system;
use crate::design::fonts::font_subsets;
use crate::design::icons::{add_icons, icon_bundle, search_icons, slice_end};
use crate::design::inventory::{inventory_findings, kit_ref, kit_refs, load_inventory};
use crate::design::lint::{
    lint_component_file, lint_css, lint_html, lint_screen, lint_tokens, state_coverage,
};
use crate::design::style::{
    apply_style, format_style, get_style, list_styles, match_styles, search_styles, AXES,
};
use crate::design::system::{
    list_presets, system_choose, system_init, system_options, system_summary, InitOptions,
};
use crate::design::tokens::{build_tokens, dir_names, relative_path};
use crate::review::locate::impeccable_engine;
use crate::tools::check::{
    check_exit_code, format_report, launch, run_checks, setup_engine, CheckOptions,
};
use crate::tools::lighthouse::run_lighthouse;

/// The commands of this group.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("build", Handler::Native(build)),
        ("tokens", Handler::Native(build)),
        ("lint", Handler::Native(check)),
        ("check", Handler::Native(check)),
        ("system", Handler::Native(system)),
        ("style", Handler::Native(style)),
        ("type", Handler::Native(type_scale)),
        ("palette", Handler::Native(palette)),
        ("icons", Handler::Native(icons)),
        ("fonts", Handler::Native(fonts)),
        ("blueprint", Handler::Native(blueprint)),
    ]
}

// ---------------------------------------------------------------- arguments as cli.mjs read them

/// A flag as the string JavaScript made of it (`--x` alone is `true`); `None` when it is
/// missing or empty (`a.x || fallback`).
fn flag(args: &Args, key: &str) -> Option<String> {
    match args.get(key) {
        Some(ArgValue::Flag) => Some("true".to_string()),
        Some(ArgValue::Text(t)) if !t.is_empty() => Some(t.clone()),
        _ => None,
    }
}

/// `v == null ? undefined : Number(v)`.
fn num(args: &Args, key: &str) -> Option<f64> {
    match args.get(key) {
        Some(ArgValue::Flag) => Some(1.0),
        Some(ArgValue::Text(t)) => Some(js::str_to_number(t)),
        None => None,
    }
}

/// `Number(a.x) || fallback`.
fn num_or(args: &Args, key: &str, fallback: f64) -> f64 {
    num(args, key)
        .filter(|n| *n != 0.0 && !n.is_nan())
        .unwrap_or(fallback)
}

/// `a.fonts === 'latin' ? 'latin' : 'vi'`.
fn fonts_arg(args: &Args) -> &'static str {
    if args.text("fonts") == Some("latin") {
        "latin"
    } else {
        "vi"
    }
}

/// `a._[1] || fallback`.
fn sub(ctx: &Ctx, fallback: &'static str) -> String {
    ctx.args
        .pos(1)
        .filter(|s| !s.is_empty())
        .unwrap_or(fallback)
        .to_string()
}

/// `a._.slice(2)`.
fn rest(ctx: &Ctx) -> Vec<String> {
    ctx.args.positionals.iter().skip(2).cloned().collect()
}

fn usage(text: &str) -> Result<i32, DfwError> {
    Err(DfwError::new(text))
}

const LEVEL_LABELS: [(&str, &str); 3] = [("error", "MUST"), ("warn", "SHOULD"), ("info", "NOTE")];

fn level_label(finding: &Value) -> String {
    let level = finding.get("level").and_then(Value::as_str);
    LEVEL_LABELS
        .iter()
        .find(|(l, _)| Some(*l) == level)
        .map_or("undefined".to_string(), |(_, label)| label.to_string())
}

fn has_error(findings: &[Value]) -> bool {
    findings.iter().any(|f| f["level"] == "error")
}

// ---------------------------------------------------------------- build, tokens

/// `dfw tokens extract --url <u> [--out <file>]`: a token seed from a rendered page.
fn tokens_extract(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let url = match a.get("url") {
        Some(ArgValue::Text(url)) if !url.is_empty() => url.as_str(),
        Some(ArgValue::Flag) => "true",
        _ => return usage("Usage: dfw tokens extract --url <reference site or running app> [--out design/system/extracted.tokens.json]"),
    };
    if !crate::browser::launch::browser_status().installed() {
        return Err(DfwError::new(
            "dfw tokens extract renders in Chrome/Edge/Chromium: none found (dfw setup --browser).",
        ));
    }
    let file = a.text("out").filter(|f| !f.is_empty());
    out(&crate::browser::extract::extract_tokens_to_file(
        &ctx.root(),
        url,
        file,
    )?);
    Ok(0)
}

/// `dfw build` and `dfw tokens build`: write tokens.css and components.css.
fn build(ctx: &Ctx) -> Result<i32, DfwError> {
    if ctx.args.pos(0) == Some("tokens") {
        match ctx.args.pos(1) {
            Some("extract") => return tokens_extract(ctx),
            // The Penpot bridge.
            Some("export") => return super::penpot::tokens_export(ctx),
            Some("import") => return super::penpot::tokens_import(ctx),
            Some("build") => {}
            _ => return usage("Usage: dfw tokens build | extract --url <u> | export --format penpot [--out <file>] | import <file> [--from penpot] [--dry-run]"),
        }
    }
    let root = ctx.root();
    let built = build_tokens(&root)?;
    let sys = build_system(&root)?;
    let themes = if built.themes.is_empty() {
        String::new()
    } else {
        format!(" (themes: {})", built.themes.join(", "))
    };
    let components = if sys.components > 0 {
        format!(
            "; {} component(s) -> design/system/components.css",
            sys.components
        )
    } else {
        String::new()
    };
    out(&format!(
        "Wrote {}{themes}{components}",
        relative_path(&root, &built.out)
    ));
    Ok(0)
}

/// `dfw system audit [--json]`: the senior review of tokens, DESIGN.md and the inventory.
fn system_audit(root: &Path, a: &Args) -> Result<i32, DfwError> {
    let findings = audit_design_system(root)?;
    if a.truthy("json") {
        out(&to_json_pretty(&Value::Array(findings)));
        return Ok(0);
    }
    if findings.is_empty() {
        out("Design system audit: no findings.");
        return Ok(0);
    }
    let lines: Vec<String> = findings
        .iter()
        .map(|f| {
            format!(
                "- [{}] ({}) {}",
                level_label(f),
                js::to_string(f.get("rule")),
                js::to_string(f.get("message"))
            )
        })
        .collect();
    out(&lines.join("\n"));
    Ok(if has_error(&findings) { 2 } else { 0 })
}

// ---------------------------------------------------------------- lint, check

/// The page `dfw check --setup` hands to the upstream launcher so it has something to detect.
const SETUP_PAGE: &str = "<!doctype html><html lang=\"en\"><title>x</title><p>x</p></html>";

/// `dfw lint [files...]` (dfw's own lint only) and `dfw check [files...|--url <u>]` (every tool).
fn check(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let is_check = a.pos(0) == Some("check");
    let root = ctx.root();
    let url = match a.get("url") {
        Some(ArgValue::Text(u)) if !u.is_empty() => Some(u.clone()),
        Some(ArgValue::Flag) => return usage("--url needs a value: dfw check --url <running app>"),
        _ => None,
    };
    let mut files: Vec<String> = a.positionals.iter().skip(1).cloned().collect();
    // "--strict <file>": the file is a target, not the flag's value.
    if let Some(ArgValue::Text(file)) = a.get("strict") {
        files.push(file.clone());
    }
    let targets: Option<Vec<String>> = match url.clone() {
        Some(u) => Some(vec![u]),
        None if !files.is_empty() => Some(files),
        None => None,
    };
    let setup = a.truthy("setup");
    // Rendered accessibility (axe) on the whole design or a URL needs the browser group: that
    // run goes to the Node CLI as a whole. Per-file runs stay fast and static, and run here.
    if is_check && !setup && !a.truthy("no-a11y") && (url.is_some() || targets.is_none()) {
        return forward(&ctx.argv);
    }
    let config = load_config(Some(&root))?;
    if setup {
        let page = project_paths(&root).dfw.join("checks").join("setup.html");
        if let Some(dir) = page.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(&page, SETUP_PAGE)?;
        let done = setup_engine(
            &env_map(&config),
            &page.to_string_lossy(),
            &impeccable_engine,
            &launch,
        )?;
        if !done.ok && done.text == "Impeccable plugin not found." {
            return Err(DfwError::new(done.text));
        }
        out(&done.text);
        return Ok(0);
    }
    let opts = CheckOptions {
        timeout_ms: Some(num_or(a, "timeout", 60000.0) as u64),
        tools: if is_check { None } else { Some(&["dfw"]) },
    };
    let mut results = run_checks(&root, targets.as_deref(), &config, opts)?;
    if is_check && a.truthy("lighthouse") {
        let Some(url) = &url else {
            return usage("--lighthouse needs --url <running app> (verify phase).");
        };
        results.push(run_lighthouse(url, 240_000));
    }
    let report = format_report(&results, num_or(a, "max", 60.0) as usize);
    if a.truthy("json") {
        out(&to_json_pretty(
            &json!({ "results": results, "tally": report.tally(), "unverified": report.unverified }),
        ));
    } else {
        out(&report.text);
    }
    Ok(check_exit_code(&report, a.truthy("strict")))
}

/// `__debug lint <file> [--as html|screen|component|css|tokens|states] [--rel <path>]
/// [--root <project>] [--base <tokens.json>]`: the findings of one lint function as compact
/// JSON (`--as tokens` on text that is not JSON prints `{"error":true}`).
pub(super) fn debug_lint(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let Some(file) = a.pos(2) else {
        return usage("Usage: dfw __debug lint <file> [--as html|screen|component|css|tokens|states] [--rel <path>] [--root <project>] [--base <tokens.json>]");
    };
    let text = js::read_text(Path::new(file))?;
    let rel = a.text("rel").unwrap_or(file);
    let findings = match a.text("as").unwrap_or("html") {
        "html" => lint_html(rel, &text),
        "component" => lint_component_file(rel, &text),
        "css" => lint_css(rel, &text),
        "screen" => {
            let root = ctx.root();
            lint_screen(&root, rel, &text, &load_components(&root)?)?
        }
        "states" => state_coverage(&ctx.root(), rel, &text)?,
        "tokens" => {
            let Ok(tree) = serde_json::from_str::<Value>(&text) else {
                out("{\"error\":true}");
                return Ok(0);
            };
            let base = match a.text("base") {
                Some(base) => Some(js::read_json(Path::new(base), None)?),
                None => None,
            };
            lint_tokens(rel, &tree, base.as_ref())
        }
        other => return usage(&format!("Unknown lint \"{other}\"")),
    };
    out(&to_json_compact(&Value::Array(findings)));
    Ok(0)
}

// ---------------------------------------------------------------- system

// One arm per subcommand, as the `system` case of cli.mjs.
fn system(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let a = &ctx.args;
    match sub(ctx, "list").as_str() {
        "presets" => {
            let lines: Vec<String> = list_presets()?
                .iter()
                .map(|(name, description)| {
                    format!(
                        "{} {}",
                        js::pad_end(name, 12),
                        short_description(description)
                    )
                })
                .collect();
            out(&lines.join("\n"));
        }
        "init" => {
            if a.truthy("only") {
                return usage("--only was removed: components are no longer copied from the kit. List what the product needs in design/system/inventory.json and design each master; read a kit master for its states with \"dfw system ref <name>\".");
            }
            let r = system_init(
                &root,
                &InitOptions {
                    preset: flag(a, "preset").unwrap_or_else(|| "crisp".to_string()),
                    style: flag(a, "style"),
                    fonts: fonts_arg(a).to_string(),
                    force: a.truthy("force"),
                },
            )?;
            out(&format!("Design system from \"{}\": tokens {}; {}. No components were copied. Next: list every component the product needs (name, purpose, usedBy, states) in design/system/inventory.json, then design each master for this product (\"dfw system ref <name>\" shows a kit master as a states checklist).",
                r.preset,
                if r.tokens_written { "written" } else { "kept (pass --force to replace)" },
                if r.inventory_created { "design/system/inventory.json created" } else { "inventory kept" }));
        }
        "ref" => {
            if let Some(name) = a.pos(2).filter(|n| !n.is_empty()) {
                out(&kit_ref(name)?);
                return Ok(0);
            }
            let refs = kit_refs()?;
            if a.truthy("json") {
                out(&to_json_pretty(&Value::Array(
                    refs.iter().map(|r| r.to_json()).collect(),
                )));
                return Ok(0);
            }
            let lines: Vec<String> = refs
                .iter()
                .map(|r| {
                    let mut parts = Vec::new();
                    if !r.variants.is_empty() {
                        parts.push(format!("variants: {}", r.variants));
                    }
                    if !r.sizes.is_empty() {
                        parts.push(format!("sizes: {}", r.sizes));
                    }
                    if !r.states.is_empty() {
                        parts.push(format!("states: {}", r.states.join(", ")));
                    }
                    format!("{} {}", js::pad_end(&r.name, 14), parts.join("  "))
                })
                .collect();
            out(&format!("Starter-kit references (read-only; design your own master, never copy):\n{}\nRead one: dfw system ref <name>", lines.join("\n")));
        }
        "inventory" => return system_inventory(&root, a),
        "options" => {
            let themes = system_options(&root, &rest(ctx), fonts_arg(a))?;
            out(&format!("Option themes: {}. Compare them in the studio (theme menu / \"Themes\" toggle), then: dfw system choose <preset>.", themes.join(", ")));
        }
        "choose" => {
            let chosen = system_choose(&root, a.pos(2).unwrap_or("undefined"), fonts_arg(a))?;
            out(&format!("Tokens now come from \"{chosen}\". Adapt design/tokens/tokens.json to the direction, then review the Design System page."));
        }
        "audit" => return system_audit(&root, a),
        "list" => {
            let summary = system_summary(&root)?;
            if a.truthy("json") {
                out(&to_json_pretty(&summary));
                return Ok(0);
            }
            let strings = |c: &Value, key: &str| -> Vec<String> {
                c[key]
                    .as_array()
                    .map(|l| l.iter().map(|v| js::to_string(Some(v))).collect())
                    .unwrap_or_default()
            };
            let lines: Vec<String> = summary["components"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|c| {
                    let mut parts = Vec::new();
                    for (key, label, sep) in [
                        ("variants", "variants", "|"),
                        ("sizes", "sizes", "|"),
                        ("props", "props", ", "),
                    ] {
                        let values = strings(c, key);
                        if !values.is_empty() {
                            parts.push(format!("{label}: {}", values.join(sep)));
                        }
                    }
                    format!(
                        "{} {}",
                        js::pad_end(&js::to_string(c.get("name")), 14),
                        parts.join("  ")
                    )
                })
                .collect();
            let text = lines.join("\n");
            out(if text.is_empty() {
                "No components yet (dfw system init)."
            } else {
                &text
            });
        }
        _ => {
            return usage("Usage: dfw system init|inventory|ref|presets|options|choose|audit|list")
        }
    }
    Ok(0)
}

/// A preset description without its fixed opening and its "Generated by" tail.
fn short_description(description: &str) -> &str {
    let mut text = description;
    if let Some(after) = text.strip_prefix("dfw starter preset \"") {
        if let Some(end) = after.find('"').filter(|end| *end > 0) {
            if let Some(tail) = after[end..].strip_prefix("\". ") {
                text = tail;
            }
        }
    }
    match text.find(" Generated by") {
        Some(cut) => &text[..cut],
        None => text,
    }
}

fn system_inventory(root: &Path, a: &Args) -> Result<i32, DfwError> {
    let inv = load_inventory(root)?;
    let findings = inventory_findings(root)?;
    if a.truthy("json") {
        out(&to_json_pretty(
            &json!({ "inventory": inv, "findings": findings }),
        ));
        return Ok(0);
    }
    let components = inv
        .as_ref()
        .and_then(|i| i.get("components"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let rows: Vec<String> = components
        .iter()
        .map(|c| {
            let used_by = c
                .get("usedBy")
                .and_then(Value::as_array)
                .map_or(String::new(), |u| js::join(u, ", "));
            let purpose = c
                .get("purpose")
                .filter(|p| js::truthy(Some(p)))
                .map_or(String::new(), |p| js::to_string(Some(p)));
            format!(
                "{} {} {purpose}",
                js::pad_end(&js::to_string(c.get("name")), 16),
                js::pad_end(&used_by, 28)
            )
        })
        .collect();
    let table = if rows.is_empty() {
        "Inventory is empty: design/system/inventory.json".to_string()
    } else {
        rows.join("\n")
    };
    let notes = if findings.is_empty() {
        "\n\nInventory and masters agree.".to_string()
    } else {
        let lines: Vec<String> = findings
            .iter()
            .map(|x| {
                format!(
                    "- [{}] ({}) {}: {}",
                    level_label(x),
                    js::to_string(x.get("rule")),
                    js::to_string(x.get("file")),
                    js::to_string(x.get("message"))
                )
            })
            .collect();
        format!("\n\n{}", lines.join("\n"))
    };
    out(&format!("{table}{notes}"));
    Ok(if has_error(&findings) { 2 } else { 0 })
}

// ---------------------------------------------------------------- style

fn style(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let a = &ctx.args;
    let text = |v: &Value, key: &str| js::to_string(v.get(key));
    let limit = |list: Vec<Value>| -> Vec<Value> {
        let end = slice_end(list.len(), num_or(a, "limit", 5.0));
        list.into_iter().take(end).collect()
    };
    match sub(ctx, "list").as_str() {
        "list" => {
            let lines: Vec<String> = list_styles()?
                .iter()
                .map(|g| format!("{} {}", js::pad_end(&text(g, "id"), 20), text(g, "concept")))
                .collect();
            out(&lines.join("\n"));
        }
        "search" => {
            let lines: Vec<String> = limit(search_styles(&rest(ctx).join(" "))?)
                .iter()
                .map(|g| {
                    format!(
                        "{} score {}  {}",
                        js::pad_end(&text(g, "id"), 20),
                        text(g, "score"),
                        text(g, "concept")
                    )
                })
                .collect();
            let found = lines.join("\n");
            out(if found.is_empty() { "No match." } else { &found });
        }
        "show" => {
            let g = get_style(a.pos(2).unwrap_or("undefined"))?;
            out(&if a.truthy("json") {
                to_json_pretty(&g)
            } else {
                format_style(&g)
            });
        }
        "match" => {
            let brief_arg = flag(a, "brief").unwrap_or_else(|| "design/system/brief.json".into());
            let file = resolve(root.join(brief_arg));
            let brief = js::read_json(&file, Some(Value::Null))?;
            if !js::truthy(Some(&brief)) {
                return usage(&format!("Brief not found: {} (template: dfw init creates design/system/brief.json).", relative_path(&root, &file)));
            }
            let res = limit(match_styles(&brief)?);
            if a.truthy("json") {
                out(&to_json_pretty(&Value::Array(res)));
                return Ok(0);
            }
            let wanted: Vec<String> = brief
                .get("personality")
                .and_then(Value::as_object)
                .map(js::entries)
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| {
                    let ends = AXES.iter().find(|(axis, _)| axis == k).map(|(_, e)| *e);
                    let end = |i: usize| ends.map_or("undefined", |e| e[i]);
                    format!("{k} {} ({}→{})", js::to_string(Some(v)), end(0), end(1))
                })
                .collect();
            let mut lines = vec![format!("Personality wanted: {}", wanted.join(", "))];
            for r in &res {
                let list = |key: &str| r[key].as_array().cloned().unwrap_or_default();
                let (mismatches, matches, notes) =
                    (list("mismatches"), list("matches"), list("notes"));
                let mut line = format!(
                    "{}%  {} {}",
                    js::pad_start(&text(r, "fit"), 3),
                    js::pad_end(&text(r, "id"), 20),
                    if mismatches.is_empty() {
                        "close on every axis".to_string()
                    } else {
                        format!("differs: {}", js::join(&mismatches, "; "))
                    }
                );
                if !matches.is_empty() {
                    line.push_str(&format!(" · keywords: {}", js::join(&matches, ", ")));
                }
                if !notes.is_empty() {
                    line.push_str(&format!(" · {}", js::join(&notes, ", ")));
                }
                lines.push(line);
            }
            out(&lines.join("\n"));
        }
        "apply" => {
            let r = apply_style(&root, a.pos(2).unwrap_or("undefined"), fonts_arg(a))?;
            build_tokens(&root)?;
            build_system(&root)?;
            let has_motifs = get_style(&r.id)?
                .get("motifs")
                .and_then(Value::as_array)
                .is_some_and(|m| !m.is_empty());
            out(&format!("Applied style \"{}\" (fonts: {} / {} / {}). Wrote design/tokens/tokens.json, design/system/head.html, design/system/style.json{}.",
                r.id, text(&r.fonts, "display"), text(&r.fonts, "body"), text(&r.fonts, "mono"),
                if has_motifs { ", design/system/components/motif.html" } else { "" }));
        }
        _ => return usage("Usage: dfw style list | search <words> | match [--brief f] | show <id> [--json] | apply <id|file.json> [--fonts latin]"),
    }
    Ok(0)
}

// ---------------------------------------------------------------- type, palette

fn type_scale(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    if a.pos(1) != Some("scale") {
        return usage("Usage: dfw type scale --base 16 --ratio 1.2 --display-ratio 1.333 [--from 28] [--write]");
    }
    let root = ctx.root();
    let from = if a.truthy("from") {
        num(a, "from")
    } else {
        None
    };
    let ts = color::type_scale(
        num_or(a, "base", 16.0),
        num_or(a, "ratio", 1.2),
        num_or(a, "display-ratio", 1.333),
        from,
    );
    let write = a.truthy("write");
    if write {
        let file = project_paths(&root).tokens.join("tokens.json");
        let mut t = js::read_json(&file, None)?;
        let old_font = t.get("font").filter(|f| js::truthy(Some(f))).cloned();
        let mut font = old_font
            .and_then(|f| f.as_object().cloned())
            .unwrap_or_default();
        let mut size = Map::new();
        size.insert("$type".into(), json!("dimension"));
        if let Some(old) = font.get("size").and_then(Value::as_object) {
            for (k, v) in js::entries(old) {
                size.insert(k.clone(), v.clone());
            }
        }
        for (k, v) in ts.ui.iter().chain(&ts.display) {
            size.insert(
                (*k).into(),
                json!({ "$value": { "value": js::num(*v), "unit": "px" } }),
            );
        }
        size["md"]["$description"] = json!(format!(
            "Body. Scale: base {}, UI ratio {}, display ratio {}.",
            js::number(ts.base),
            js::number(ts.ratio),
            js::number(ts.display_ratio)
        ));
        font.insert("size".into(), Value::Object(size));
        match t.as_object_mut() {
            Some(tree) => tree.insert("font".into(), Value::Object(font)),
            None => {
                return Err(DfwError::internal(
                    "design/tokens/tokens.json is not a JSON object.",
                ))
            }
        };
        write_json(&file, &js::key_order(t))?;
        build_tokens(&root)?;
    }
    let row = |steps: &[(&str, f64)]| -> String {
        let parts: Vec<String> = steps
            .iter()
            .map(|(k, v)| format!("{k} {}", js::number(*v)))
            .collect();
        parts.join(" · ")
    };
    out(&format!(
        "UI: {}\nDisplay: {}{}",
        row(&ts.ui),
        row(&ts.display),
        if write {
            "\nWritten to design/tokens/tokens.json."
        } else {
            ""
        }
    ));
    Ok(0)
}

fn palette(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    if a.pos(1) != Some("build") {
        return usage("Usage: dfw palette build --brand <hex|hue> [--chroma 0.14] --neutral-hue <h> --tint 0.01 [--signature <hue>|--harmony analogous|complementary|split-complementary|triadic] [--accent brand|signature|neutral] [--names brand,neutral,signature] [--dark] [--json]");
    }
    let brand = flag(a, "brand").unwrap_or_default();
    let mut seeds = Seeds {
        neutral_hue: num(a, "neutral-hue"),
        neutral_tint: Some(num(a, "tint").unwrap_or(0.01)),
        accent_from: a
            .get("accent")
            .map(|_| flag(a, "accent").unwrap_or_default()),
        dark: a.truthy("dark"),
        warm_surface: a.truthy("warm-surface"),
        ..Seeds::default()
    };
    if brand.starts_with('#') {
        seeds.brand_hex = Some(brand);
        seeds.brand_chroma = num(a, "chroma");
    } else {
        seeds.brand_hue = num(a, "brand");
        seeds.brand_chroma = Some(num(a, "chroma").unwrap_or(0.14));
        if seeds.brand_hue.is_none_or(f64::is_nan) {
            return usage("--brand needs a hex colour or an OKLCH hue (0-360).");
        }
    }
    let chroma = num(a, "signature-chroma");
    if a.get("signature").is_some() {
        seeds.signature = Some(Signature {
            hue: num(a, "signature"),
            chroma,
            ..Signature::default()
        });
    } else if let Some(harmony) = flag(a, "harmony") {
        seeds.signature = Some(Signature {
            harmony: Some(harmony),
            chroma,
            ..Signature::default()
        });
    }
    let names_arg = flag(a, "names").unwrap_or_else(|| "brand,neutral,signature".to_string());
    let mut names = names_arg.split(',').map(str::to_string);
    let names = Names {
        brand: names.next(),
        neutral: names.next(),
        signature: names.next(),
    };
    let pal = color::build_palette(&seeds, &names);
    if a.truthy("json") {
        out(&to_json_pretty(&pal));
        return Ok(0);
    }
    let pairs = |v: &Value, sep: &str| -> String {
        let parts: Vec<String> = v
            .as_object()
            .into_iter()
            .flatten()
            .map(|(k, v)| format!("{k} {}", js::to_string(Some(v))))
            .collect();
        parts.join(sep)
    };
    let mut lines: Vec<String> = Vec::new();
    for (name, ramp) in pal["ramps"].as_object().into_iter().flatten() {
        lines.push(format!("{} {}", js::pad_end(name, 14), pairs(ramp, "  ")));
    }
    lines.push("Roles:".into());
    for (k, v) in pal["roles"].as_object().into_iter().flatten() {
        lines.push(format!(
            "  {} {}",
            js::pad_end(k, 14),
            js::to_string(Some(v))
        ));
    }
    lines.push("Status:".into());
    lines.push(format!("  {}", pairs(&pal["status"], "  ")));
    for n in pal["notes"].as_array().into_iter().flatten() {
        lines.push(format!("Note: {}", js::to_string(Some(n))));
    }
    lines.push("Next: put the ramps in color.palette.* with usage rules, roles as aliases, then \"dfw system audit\".".into());
    out(&lines.join("\n"));
    Ok(0)
}

// ---------------------------------------------------------------- blueprint, icons, fonts

fn blueprint(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let a = &ctx.args;
    let m = js::read_json(&project_paths(&root).manifest, Some(json!({})))?;
    let wanted = a.pos(1).filter(|w| !w.is_empty() && *w != "list");
    let screens: Vec<&Value> = m
        .get("screens")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|x| match wanted {
            Some(id) => x.get("id").and_then(Value::as_str) == Some(id),
            None => !js::to_string(x.get("id")).starts_with('_'),
        })
        .collect();
    if a.truthy("json") {
        let list: Vec<Value> = screens
            .iter()
            .map(|x| {
                let mut item = Map::new();
                for key in ["id", "title"] {
                    if let Some(v) = x.get(key) {
                        item.insert(key.into(), v.clone());
                    }
                }
                let blueprint = x.get("blueprint").filter(|b| js::truthy(Some(b)));
                item.insert(
                    "blueprint".into(),
                    blueprint.cloned().unwrap_or(Value::Null),
                );
                Value::Object(item)
            })
            .collect();
        out(&to_json_pretty(&Value::Array(list)));
        return Ok(0);
    }
    let blocks: Vec<String> = screens.iter().map(|x| blueprint_text(x)).collect();
    let text = blocks.join("\n\n");
    out(if text.is_empty() {
        "No screens."
    } else {
        &text
    });
    Ok(0)
}

/// One screen's blueprint as text.
fn blueprint_text(x: &Value) -> String {
    let s = |v: Option<&Value>| js::to_string(v);
    let or = |v: Option<&Value>, fallback: &str| {
        v.filter(|v| js::truthy(Some(v)))
            .map_or(fallback.to_string(), |v| js::to_string(Some(v)))
    };
    let id = s(x.get("id"));
    let Some(b) = x.get("blueprint").filter(|b| js::truthy(Some(b))) else {
        return format!("{id}: NO BLUEPRINT");
    };
    let list = |key: &str| {
        b.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let data = |key: &str| or(b.get("data").and_then(|d| d.get(key)), "?");
    let mut lines = vec![
        format!(
            "{id} · {} · layout {}",
            or(x.get("title"), ""),
            s(b.get("layout"))
        ),
        format!("  goal: {}", s(b.get("goal"))),
    ];
    if js::truthy(b.get("primaryAction")) {
        lines.push(format!("  primary action: {}", s(b.get("primaryAction"))));
    }
    lines.push(format!(
        "  above the fold: {}",
        js::join(&list("aboveFold"), "; ")
    ));
    for r in list("regions") {
        let priority = js::present(r.get("priority")).map_or(String::new(), |p| s(Some(p)));
        lines.push(format!(
            "  region {priority} {}: {}",
            s(r.get("name")),
            s(r.get("content"))
        ));
    }
    lines.push(format!(
        "  data: empty → {} | typical → {} | max → {}",
        data("empty"),
        data("typical"),
        data("max")
    ));
    lines.push(format!("  feedback: {}", s(b.get("feedback"))));
    lines.join("\n")
}

fn icons(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let a = &ctx.args;
    match sub(ctx, "list").as_str() {
        "search" => {
            let hits = search_icons(
                &icon_bundle()?,
                &rest(ctx).join(" "),
                num_or(a, "limit", 20.0),
            );
            let lines: Vec<String> = hits
                .iter()
                .map(|i| format!("{} {}", js::pad_end(&i.name, 24), i.tags.join(", ")))
                .collect();
            let text = lines.join("\n");
            out(if text.is_empty() { "No match." } else { &text });
        }
        "add" => {
            let r = add_icons(&root, &rest(ctx))?;
            let mut lines = vec![format!(
                "Added {} icon(s) to design/system/icons/.",
                r.added.len()
            )];
            for m in &r.missing {
                let similar = r
                    .suggestions
                    .iter()
                    .find(|(name, _)| name == m)
                    .map(|(_, names)| names.join(", "))
                    .filter(|names| !names.is_empty());
                lines.push(format!(
                    "  {m}: not found{}",
                    similar.map_or(String::new(), |names| format!(" (try: {names})"))
                ));
            }
            out(&lines.join("\n"));
            if !r.missing.is_empty() {
                return Ok(2);
            }
        }
        "list" => {
            let dir = project_paths(&root).design.join("system").join("icons");
            let names: Vec<String> = dir_names(&dir)?
                .iter()
                .filter_map(|f| f.strip_suffix(".svg").map(str::to_string))
                .collect();
            let text = names.join(" ");
            out(if text.is_empty() { "(none)" } else { &text });
        }
        _ => return usage("Usage: dfw icons search <words> | add <names...> | list"),
    }
    Ok(0)
}

fn fonts(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    if a.pos(1) != Some("check") || a.positionals.len() < 3 {
        return usage("Usage: dfw fonts check <family...>  (quote names with spaces)");
    }
    let mut res = Vec::new();
    for family in rest(ctx) {
        res.push(font_subsets(&family)?);
    }
    if a.truthy("json") {
        out(&to_json_pretty(&Value::Array(
            res.iter().map(|r| r.to_json()).collect(),
        )));
        return Ok(0);
    }
    let lines: Vec<String> = res
        .iter()
        .map(|r| {
            if r.ok {
                let vietnamese = r.subsets.iter().any(|s| s == "vietnamese");
                format!(
                    "{}: {}{}",
                    r.family,
                    r.subsets.join(", "),
                    if vietnamese {
                        ""
                    } else {
                        "  (NO vietnamese subset)"
                    }
                )
            } else {
                let why = r.error.as_ref().filter(|e| !e.is_empty());
                format!(
                    "{}: not found on Google Fonts{}",
                    r.family,
                    why.map_or(String::new(), |e| format!(" ({e})"))
                )
            }
        })
        .collect();
    out(&lines.join("\n"));
    Ok(0)
}

// ---------------------------------------------------------------- __debug helpers

/// `__debug compose <file> [--base <path to design/>] [--root <project>]`:
/// `JSON.stringify(composeHtml(root, text, { designBase }))`. With `--parse`:
/// `JSON.stringify(parseComponentFile(text, file))`.
pub(super) fn debug_compose(ctx: &Ctx) -> Result<i32, DfwError> {
    let file = ctx.args.pos(2).ok_or_else(|| {
        DfwError::new("Usage: dfw __debug compose <file> [--base <path>] [--parse]")
    })?;
    let text = js::read_text(&resolve(file))?;
    if ctx.args.truthy("parse") {
        let defs: Vec<Value> = parse_component_file(&text, file)
            .iter()
            .map(|d| d.to_json())
            .collect();
        out(&to_json_compact(&Value::Array(defs)));
        return Ok(0);
    }
    let composed = compose_html(
        &ctx.root(),
        &text,
        ComposeOptions {
            lib: None,
            design_base: ctx.args.text("base"),
        },
    )?;
    out(&to_json_compact(&composed.to_json()));
    Ok(0)
}

/// Numbers rounded to nine decimals, so the last bits of a float (which differ between math
/// libraries) do not show; the conformance test rounds the Node result the same way.
fn rounded(value: Value) -> Value {
    match value {
        Value::Number(n) => {
            let fixed = js::to_fixed(n.as_f64().unwrap_or(f64::NAN), 9);
            js::num(fixed.parse().unwrap_or(f64::NAN))
        }
        Value::Array(items) => Value::Array(items.into_iter().map(rounded).collect()),
        Value::Object(map) => {
            Value::Object(map.into_iter().map(|(k, v)| (k, rounded(v))).collect())
        }
        other => other,
    }
}

fn ramp_from(value: &Value) -> Ramp {
    let mut steps: [String; 11] = Default::default();
    for (slot, step) in steps.iter_mut().zip(color::STEPS) {
        *slot = js::to_string(value.get(step.to_string()));
    }
    Ramp(steps)
}

/// `__debug color <function> <args...>`: call a function of design/color with the arguments
/// (each JSON, or a plain string) and print the result as JSON.
pub(super) fn debug_color(ctx: &Ctx) -> Result<i32, DfwError> {
    let name = ctx.args.pos(2).unwrap_or_default();
    let args: Vec<Value> = ctx
        .args
        .positionals
        .iter()
        .skip(3)
        .map(|a| serde_json::from_str(a).unwrap_or_else(|_| Value::String(a.clone())))
        .collect();
    let arg = |i: usize| args.get(i);
    let text = |i: usize| js::to_string(arg(i));
    let number = |v: Option<&Value>| js::to_number(v);
    let field = |i: usize, key: &str| arg(i).and_then(|v| v.get(key));
    let field_or = |i: usize, key: &str, fallback: f64| {
        js::present(field(i, key)).map_or(fallback, |v| number(Some(v)))
    };
    let floats = |values: &[f64]| Value::Array(values.iter().map(|v| js::num(*v)).collect());
    let steps = |list: &[(&str, f64)]| -> Value {
        Value::Object(
            list.iter()
                .map(|(k, v)| (k.to_string(), js::num(*v)))
                .collect(),
        )
    };
    let result = match name {
        "hexToRgb" => floats(&color::hex_to_rgb(&text(0))),
        "rgbToHex" => {
            let c = |i: usize| number(arg(0).and_then(|v| v.get(i)));
            json!(color::rgb_to_hex([c(0), c(1), c(2)]))
        }
        "hexToOklch" => {
            let o = color::hex_to_oklch(&text(0));
            json!({ "l": js::num(o.l), "c": js::num(o.c), "h": js::num(o.h) })
        }
        "oklchToHex" => json!(color::oklch_to_hex(Oklch {
            l: number(field(0, "l")),
            c: number(field(0, "c")),
            h: number(field(0, "h")),
        })),
        "luminance" => js::num(color::luminance(&text(0))),
        "contrast" => js::num(color::contrast(&text(0), &text(1))),
        "hueDistance" => js::num(color::hue_distance(number(arg(0)), number(arg(1)))),
        "ramp" => color::ramp(
            number(field(0, "hue")),
            number(field(0, "chroma")),
            field_or(0, "hueShift", 0.0),
        )
        .to_json(),
        "neutralRamp" => {
            color::neutral_ramp(number(field(0, "hue")), field_or(0, "tint", 0.01)).to_json()
        }
        "stepFor" => {
            let ramp = ramp_from(arg(0).unwrap_or(&Value::Null));
            let from_light = field(3, "from").and_then(Value::as_str) != Some("dark");
            js::num(color::step_for(&ramp, &text(1), number(arg(2)), from_light))
        }
        "buildPalette" => {
            let seeds = Seeds::from_value(arg(0).unwrap_or(&Value::Null));
            let name = |key: &str| field(1, key).map(|v| js::to_string(Some(v)));
            color::build_palette(
                &seeds,
                &Names {
                    brand: name("brand"),
                    neutral: name("neutral"),
                    signature: name("signature"),
                },
            )
        }
        "auditColors" => {
            let exceptions: Vec<String> = field(1, "exceptions")
                .and_then(Value::as_array)
                .map(|l| l.iter().map(|v| js::to_string(Some(v))).collect())
                .unwrap_or_default();
            Value::Array(color::audit_colors(
                arg(0).unwrap_or(&Value::Null),
                &exceptions,
            ))
        }
        "typeScale" => {
            let ts = color::type_scale(
                field_or(0, "base", 16.0),
                field_or(0, "ratio", 1.2),
                field_or(0, "displayRatio", 1.333),
                field(0, "displayFrom").map(|v| number(Some(v))),
            );
            json!({ "ui": steps(&ts.ui), "display": steps(&ts.display), "ratio": js::num(ts.ratio), "displayRatio": js::num(ts.display_ratio), "base": js::num(ts.base) })
        }
        "auditTypeScale" => {
            let surface = field(1, "surface").map_or("product".to_string(), |v| js::to_string(Some(v)));
            Value::Array(color::audit_type_scale(
                arg(0).unwrap_or(&Value::Null),
                field_or(1, "body", 16.0),
                &surface,
            ))
        }
        _ => return usage("Usage: dfw __debug color <hexToRgb|rgbToHex|hexToOklch|oklchToHex|luminance|contrast|hueDistance|ramp|neutralRamp|stepFor|buildPalette|auditColors|typeScale|auditTypeScale> <args...>"),
    };
    out(&to_json_compact(&rounded(result)));
    Ok(0)
}
