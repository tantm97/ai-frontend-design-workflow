//! Workflow commands: init, status, mode, phase, approve, reopen, pause, resume, decide,
//! decisions, config, lang, verify-snapshot.
//! Every branch runs here; `approve DS` and `approve G2` run their gate's checks first (G2 with
//! the rendered accessibility check).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::{Ctx, Handler};
use crate::browser::a11y::{a11y_check, A11yOptions};
use crate::core::config::{get_config_value, load_config, set_config_value, Scope};
use crate::core::error::exit;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::level::{self, Level};
use crate::core::paths::project_paths;
use crate::core::process::is_alive;
use crate::core::util::{
    append_jsonl_inside, now_iso, out, parse_value, read_json, to_json_compact, to_json_pretty,
    write_atomic_inside, write_json_inside, Args,
};
use crate::design::compose::{build_system, load_components};
use crate::design::dsaudit::audit_design_system;
use crate::design::inventory::DS_GATE_RULES;
use crate::design::tokens::{build_tokens, dir_names};
use crate::review::comments::{add_comment, all_comments};
use crate::review::status::{self, Status};
use crate::tools::check::{format_report, run_checks, CheckOptions};
use crate::workflow::decisions::{decide, format_decisions, read_decisions};
use crate::workflow::freeze::{freeze_design, reopen_design, verify_snapshot};
use crate::workflow::lang::{language_status, set_language};
use crate::workflow::stack::detect_stack;
use crate::workflow::state::{
    approve_gate, arg_raw, arg_truthy, current_approval, gate_title, load_state, mode_info,
    new_state, push_history, require_state, save_state, set_field, set_mode, set_phase,
    state_field_error, summarize, type_error,
};

/// The commands of this group.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("init", init as Handler),
        ("status", status as Handler),
        ("mode", mode as Handler),
        ("phase", phase as Handler),
        ("approve", approve as Handler),
        ("reopen", reopen as Handler),
        ("pause", pause_resume as Handler),
        ("resume", pause_resume as Handler),
        ("decide", decide_cmd as Handler),
        ("decisions", decisions as Handler),
        ("config", config as Handler),
        ("lang", lang as Handler),
        ("verify-snapshot", verify_snapshot_cmd as Handler),
    ]
}

fn print_json(value: &Value) -> Result<i32, DfwError> {
    out(&to_json_pretty(value));
    Ok(0)
}

fn init(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let p = project_paths(&root);
    if p.state.exists() {
        return Err(DfwError::new(format!(
            "dfw is already initialized here ({}). Use \"dfw status\".",
            p.state.display()
        )));
    }
    let mode = arg_truthy(a, "mode").map_or("new-product".to_string(), |m| js::to_string(Some(&m)));
    let product = arg_truthy(a, "product").unwrap_or_else(|| "".into());
    let mut state = new_state(&mode, product)?;
    let mode = js::to_string(state.get("mode"));
    if mode_info(&mode).is_some_and(|m| m.requires_approved_design) {
        return Err(DfwError::new(format!("Mode \"{mode}\" needs an existing approved design; initialize with another mode first.")));
    }
    save_state(&root, &mut state)?;
    fs::create_dir_all(&p.reviews)?;
    if !p.config.exists() {
        write_json_inside(&root, &p.config, &json!({ "schema": 1 }))?;
    }
    if !p.decisions.exists() {
        write_atomic_inside(&root, &p.decisions, "")?;
    }
    write_atomic_inside(
        &root,
        &p.dfw.join(".gitignore"),
        "config.local.json\nstudio.json\nstudio.log\nreview.json\nreview.log\npresence.json\nshots/\nchecks/\n",
    )?;
    let mut created = Vec::new();
    for doc in ["PRODUCT.md", "DESIGN.md"] {
        if !root.join(doc).exists() {
            write_atomic_inside(&root, &root.join(doc), crate::assets::template_text(doc)?)?;
            created.push(doc);
        }
    }
    if !p.design.exists() {
        crate::assets::copy_templates(&root, "design", &p.design)?;
        let mut m = read_json(&p.manifest, None)?;
        let product = js::truthy_value(state.get("product"))
            .cloned()
            .unwrap_or_else(|| {
                Value::String(
                    root.file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                )
            });
        if let Value::Object(map) = &mut m {
            map.insert("product".into(), product);
        }
        write_json_inside(&root, &p.manifest, &m)?;
        created.push("design/");
        // `try { buildTokens(root) } catch {}`: a starter that does not build is not an error here.
        let _ = build_tokens(&root);
    }
    let created = if created.is_empty() {
        String::new()
    } else {
        format!("Created: {}\n", created.join(", "))
    };
    out(&format!(
        "Initialized dfw ({mode}) in {}\n{created}{}",
        root.display(),
        summarize(Some(&state))?
    ));
    Ok(0)
}

fn status(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let p = project_paths(&root);
    let state = load_state(&root)?;
    let as_json = a.truthy("json");
    let comments = match &state {
        Some(_) => all_comments(&root)?,
        None => Vec::new(),
    };
    let count = |wanted: fn(Status) -> bool| {
        comments
            .iter()
            .filter(|c| status::of(c).is_some_and(wanted))
            .count()
    };
    let (active, fixed, resolved) = (
        count(Status::is_active),
        count(|s| s == Status::Fixed),
        count(|s| s == Status::Resolved),
    );
    let studio = read_json(&p.studio, Some(Value::Null))?;
    let studio = if js::truthy(&studio) {
        if is_alive(studio.get("pid")) {
            studio
        } else {
            Value::Null
        }
    } else {
        Value::Null
    };
    let stack = read_json(&p.stack, Some(Value::Null))?;
    let approved = match &state {
        Some(s) => current_approval(s)?,
        None => None,
    };
    if as_json {
        return print_json(&json!({
            "root": root.to_string_lossy(),
            "state": state,
            "approved": approved,
            "comments": { "active": active, "fixed": fixed, "resolved": resolved },
            "studio": studio,
            "stack": stack,
        }));
    }
    let mut lines = vec![summarize(state.as_ref())?];
    if let Some(state) = &state {
        let gates = state
            .get("gates")
            .ok_or_else(|| state_field_error("gates", "an object"))?;
        let gate = |g: &str| js::truthy_value(gates.get(g)).is_some();
        lines.push(format!(
            "comments: {active} active, {fixed} awaiting user verification, {resolved} resolved"
        ));
        lines.push(format!(
            "studio: {}",
            if studio.is_null() {
                "not running".to_string()
            } else {
                js::to_string(studio.get("url"))
            }
        ));
        lines.push(format!(
            "stack: {}",
            if js::truthy(&stack) {
                let name = js::truthy_value(stack.get("framework").and_then(|f| f.get("name")));
                format!(
                    "{}{}",
                    name.map_or("?".to_string(), |n| js::to_string(Some(n))),
                    if gate("G3") {
                        " (approved)"
                    } else {
                        " (proposed)"
                    }
                )
            } else {
                "not set".to_string()
            }
        ));
        let next = next_step(
            state,
            Gates {
                g1: gate("G1"),
                ds: gate("DS"),
                g3: gate("G3"),
            },
            load_components(&root)?.len(),
            (active, fixed),
            approved.is_some(),
        );
        if let Some(next) = next {
            lines.push(format!("next: {next}"));
        }
    }
    out(&lines.join("\n"));
    Ok(0)
}

/// The gates [`next_step`] looks at.
struct Gates {
    g1: bool,
    ds: bool,
    g3: bool,
}

/// `nextStep(state, data)`: what the agent does next in this phase. `components` is the number
/// of component masters; `comments` the active and the fixed ones.
fn next_step(
    state: &Value,
    gates: Gates,
    components: usize,
    comments: (usize, usize),
    approved: bool,
) -> Option<String> {
    let Gates { g1, ds, g3 } = gates;
    let (active, fixed) = comments;
    let step = match state.get("phase").and_then(Value::as_str)? {
        "discovery" => "finish discovery (PRODUCT.md), then \"dfw phase direction\"",
        "direction" if g1 => "\"dfw phase system\"",
        "direction" => "present directions; record the choice with \"dfw approve G1 --note ...\"",
        "system" if ds => "\"dfw phase concepts\"",
        "system" if components > 0 => "review the Design System page in the studio; when the user approves: \"dfw approve DS --note ...\"",
        "system" => "build the design system: \"dfw system init --style <id>\", list the components the product needs in design/system/inventory.json, then design each master",
        "concepts" if active > 0 => return Some(format!("apply {active} open review comment(s)")),
        "concepts" if fixed > 0 => "ask the user to verify fixed comments in the studio",
        "concepts" if approved => "design approved; \"dfw phase stack\"",
        "concepts" => "review in the studio; when the user approves: \"dfw approve G2 --note ...\"",
        "stack" if g3 => "\"dfw phase implement\"",
        "stack" => "propose/confirm the stack; \"dfw approve G3 --note ...\"",
        "implement" => "implement from design/handoff; then \"dfw phase verify\"",
        "verify" => {
            "compare against the approved design; when accepted: \"dfw approve G4 --note ...\""
        }
        "audit" => "review-only: report findings, change nothing",
        _ => return None,
    };
    Some(step.to_string())
}

fn mode(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let mut state = require_state(&root)?;
    set_mode(&root, &mut state, ctx.args.pos(1))?;
    out(&summarize(Some(&state))?);
    Ok(0)
}

fn phase(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let mut state = require_state(&root)?;
    set_phase(
        &root,
        &mut state,
        a.pos(1),
        a.truthy("force"),
        arg_raw(a, "reason"),
    )?;
    out(&summarize(Some(&state))?);
    Ok(0)
}

/// The checks of gate DS: the design system exists, lints clean and passes the audit, with the
/// rationale and the inventory rules as MUST; no preset option is left undecided.
fn approve_ds(root: &Path, a: &Args) -> Result<(), DfwError> {
    if load_components(root)?.is_empty() {
        return Err(DfwError::new("DS needs a design system: component masters in design/system/components/, designed from design/system/inventory.json (start with \"dfw system init\")."));
    }
    build_system(root)?;
    let config = load_config(Some(root))?;
    let p = project_paths(root);
    let mut targets = vec!["design/tokens/tokens.json".to_string()];
    let masters = dir_names(&p.design.join("system").join("components"))?;
    targets.extend(
        masters
            .iter()
            .map(|f| format!("design/system/components/{f}")),
    );
    let opts = CheckOptions {
        timeout_ms: Some(30000),
        tools: Some(&["dfw"]),
    };
    let mut results = run_checks(root, Some(&targets), &config, opts)?;
    let mut audit = audit_design_system(root)?;
    for finding in &mut audit {
        let rule = finding.get("rule").and_then(Value::as_str);
        let must = rule.is_some_and(|r| r == "rationale" || DS_GATE_RULES.contains(&r));
        if let (true, Value::Object(map)) = (must, finding) {
            map.insert("level".into(), Level::Error.as_str().into());
        }
    }
    results.push(json!({ "tool": "dfw-ds-audit", "status": "ran", "findings": audit }));
    let report = format_report(&results, 30);
    if report.errors > 0 && !a.truthy("accept-findings") {
        return Err(DfwError::new(format!(
            "DS blocked: {} MUST finding(s) in the design system.\n{}",
            report.errors, report.text
        )));
    }
    let options = dir_names(&p.tokens.join("themes"))?
        .iter()
        .any(|f| f.starts_with("opt-"));
    if options && !a.truthy("force") {
        return Err(DfwError::new("Preset options are still active (opt-* themes). Promote the chosen one first: dfw system choose <preset>."));
    }
    Ok(())
}

/// Gate G2: every check, the rendered accessibility check with states included, must pass (or
/// the user accepts the findings, which is recorded); no review comment may be left open; then
/// the design is frozen.
fn approve_g2(root: &Path, a: &Args, state: &mut Value, note: Value) -> Result<i32, DfwError> {
    let config = load_config(Some(root))?;
    let opts = CheckOptions {
        timeout_ms: Some(60000),
        tools: None,
    };
    let mut results = run_checks(root, None, &config, opts)?;
    let a11y = A11yOptions {
        url: None,
        screens: None,
        states: true,
        themes: false,
    };
    results.push(a11y_check(root, &a11y)?.to_json());
    let report = format_report(&results, 30);
    if report.errors > 0 && !a.truthy("accept-findings") {
        return Err(DfwError::new(format!("G2 blocked: {} MUST finding(s) in the design.\n{}\nFix them, or if the user explicitly accepts them, re-run with --accept-findings (it is recorded).", report.errors, report.text)));
    }
    // A configured check that did not run is not a pass: freezing without it is the user's call.
    let not_verified: Vec<&str> = report
        .text
        .lines()
        .filter(|l| l.starts_with("NOT VERIFIED"))
        .collect();
    if !report.unverified.is_empty() && !a.truthy("accept-unverified") {
        return Err(DfwError::new(format!("G2 blocked: {} configured check(s) did not run ({}), so the design is not verified.\n{}\nInstall what is missing (\"dfw doctor\" lists it; \"dfw setup --a11y\" downloads axe) or turn the check off in the config. If the user explicitly accepts freezing without these checks, re-run with --accept-unverified (it is recorded).", report.unverified.len(), report.unverified.join(", "), not_verified.join("\n"))));
    }
    let open: Vec<String> = all_comments(root)?
        .iter()
        .filter(|c| status::of(c).is_some_and(|s| s.is_active() || s == Status::Fixed))
        .map(|c| js::to_string(c.get("id")))
        .collect();
    if !open.is_empty() && !a.truthy("accept-open-comments") {
        return Err(DfwError::new(format!("G2 blocked: {} review comment(s) are not resolved or dismissed ({}). Resolve them with the user, or pass --accept-open-comments if the user says so.", open.len(), open.join(", "))));
    }
    freeze_design(root, state, Some(note))?;
    let last = state
        .get("design")
        .and_then(|d| d.get("approved"))
        .and_then(Value::as_array)
        .and_then(|list| list.last())
        .ok_or_else(|| type_error("version"))?;
    let hash = js::to_string(last.get("hash"));
    let version = js::to_string(last.get("version"));
    // Recorded only once the design is frozen: a refusal after the checks records nothing.
    if report.errors > 0 {
        let musts: Vec<Value> = report
            .findings
            .iter()
            .filter(|f| level::of(f) == Some(Level::Error))
            .take(50)
            .cloned()
            .collect();
        append_jsonl_inside(
            root,
            &project_paths(root).decisions,
            &json!({
                "at": now_iso(),
                "level": 1,
                "topic": "gate:G2:accepted-findings",
                "decision": format!("User accepted {} MUST finding(s) at freeze.", report.errors),
                "findings": musts,
                "by": "user",
            }),
        )?;
    }
    if !report.unverified.is_empty() {
        append_jsonl_inside(
            root,
            &project_paths(root).decisions,
            &json!({
                "at": now_iso(),
                "level": 1,
                "topic": "gate:G2:accepted-unverified",
                "decision": format!("User accepted freezing {version} with {} check(s) not run: {}.", report.unverified.len(), report.unverified.join(", ")),
                "unverified": report.unverified,
                "by": "user",
            }),
        )?;
    }
    let mut lines = vec![format!(
        "Design frozen as {version} (sha256 {}).",
        js::slice16(&hash, 0, 12)
    )];
    lines.extend(not_verified.iter().map(|l| l.to_string()));
    lines.push(summarize(Some(state))?);
    out(&lines.join("\n"));
    Ok(0)
}

fn approve(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let gate = a.pos(1).unwrap_or_default().to_uppercase();
    let mut state = require_state(&root)?;
    let Some(note) = arg_truthy(a, "note") else {
        return Err(DfwError::new("Approvals record what the user approved: pass --note \"<summary of what was approved>\"."));
    };
    if gate == "DS" {
        approve_ds(&root, a)?;
    }
    if gate == "G2" {
        return approve_g2(&root, a, &mut state, note);
    }
    let file = project_paths(&root).stack;
    let mut stack = Value::Null;
    if gate == "G3" {
        stack = read_json(&file, Some(Value::Null))?;
        if file.exists() && !stack.is_object() {
            return Err(DfwError::new("G3 needs a stack proposal: .dfw/stack.json must hold a JSON object (run: dfw stack set ... or dfw stack detect --write)."));
        }
    }
    approve_gate(&root, &mut state, &gate, Some(note), Map::new())?;
    // The stack is marked approved only once the gate is recorded.
    if let Value::Object(map) = &mut stack {
        map.insert("status".into(), "approved".into());
        map.insert("approvedAt".into(), now_iso().into());
        write_json_inside(&root, &file, &stack)?;
    }
    out(&format!(
        "{gate} approved: {}.\n{}",
        gate_title(&gate).unwrap_or_default(),
        summarize(Some(&state))?
    ));
    Ok(0)
}

fn reopen(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let mut state = require_state(&root)?;
    reopen_design(&root, &mut state, arg_raw(&ctx.args, "reason"))?;
    out(&summarize(Some(&state))?);
    Ok(0)
}

fn pause_resume(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let cmd = ctx.args.pos(0).unwrap_or_default();
    let resume = cmd == "resume";
    let mut state = require_state(&root)?;
    set_field(&mut state, "active", Some(resume.into()))?;
    push_history(
        &mut state,
        js::object(vec![
            ("at", Some(now_iso().into())),
            ("event", Some(cmd.into())),
            ("reason", arg_raw(&ctx.args, "reason")),
        ]),
    )?;
    save_state(&root, &mut state)?;
    out(&format!(
        "dfw {} for {}",
        if resume {
            "resumed"
        } else {
            "paused (gates and hooks off)"
        },
        root.display()
    ));
    Ok(0)
}

fn decide_cmd(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    // `Number(a.level)`; `--level` alone never gets here (`require_flag_values`).
    let level = a.text("level").map_or(f64::NAN, js::str_to_number);
    let words = a.positionals.get(1..).unwrap_or_default().join(" ");
    let text = if words.is_empty() {
        arg_truthy(a, "text")
    } else {
        Some(words.into())
    };
    let entry = decide(
        &ctx.root(),
        level,
        arg_truthy(a, "topic"),
        text,
        arg_truthy(a, "overrides"),
    )?;
    print_json(&entry)
}

fn decisions(ctx: &Ctx) -> Result<i32, DfwError> {
    let list = read_decisions(&ctx.root())?;
    if ctx.args.truthy("json") {
        return print_json(&Value::Array(list));
    }
    out(&format_decisions(&list)?);
    Ok(0)
}

fn config(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    match a.pos(1) {
        Some("get") => {
            let value = match a.pos(2).filter(|k| !k.is_empty()) {
                Some(key) => get_config_value(Some(&root), key)?.ok_or_else(|| {
                    DfwError::new(format!("\"{key}\" is not set in the config; \"dfw config get\" prints every setting."))
                })?,
                None => load_config(Some(&root))?,
            };
            out(&to_json_pretty(&value));
            Ok(0)
        }
        Some("set") => {
            let scope = if a.truthy("global") {
                Scope::Global
            } else if a.truthy("local") {
                Scope::Local
            } else {
                Scope::Project
            };
            let (Some(key), Some(value)) = (a.pos(2), a.pos(3)) else {
                return Err(DfwError::new(
                    "Usage: dfw config set <key> <value> [--global|--project|--local]",
                ));
            };
            let file = set_config_value(scope, &root, key, parse_value(value))?;
            out(&format!("Set {key} in {}", file.display()));
            Ok(0)
        }
        _ => Err(DfwError::new(
            "Usage: dfw config get [key] | set <key> <value> [--global|--project|--local]",
        )),
    }
}

fn lang(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    // `--session` alone never gets here (`require_flag_values`).
    let session = a.text("session");
    match a.pos(1).filter(|s| !s.is_empty()).unwrap_or("status") {
        "status" => print_json(&language_status(session)?),
        "set" => print_json(&set_language(a.pos(2), session)?),
        _ => Err(DfwError::new(
            "Usage: dfw lang status|set <code> [--session <id>]",
        )),
    }
}

fn verify_snapshot_cmd(ctx: &Ctx) -> Result<i32, DfwError> {
    let root = ctx.root();
    let state = require_state(&root)?;
    let version_of = |v: Option<&Value>| {
        js::truthy_value(v.and_then(|v| v.get("version"))).map(|v| js::to_string(Some(v)))
    };
    let named = ctx
        .args
        .pos(1)
        .filter(|v| !v.is_empty())
        .map(str::to_string);
    let version = match named.or(version_of(current_approval(&state)?.as_ref())) {
        Some(v) => Some(v),
        None => {
            let approved = state
                .get("design")
                .and_then(|d| d.get("approved"))
                .and_then(Value::as_array)
                .ok_or_else(|| state_field_error("design.approved", "a list"))?;
            version_of(approved.last())
        }
    };
    let Some(version) = version else {
        return Err(DfwError::new("No approved snapshot."));
    };
    let problems = verify_snapshot(&root, &version)?;
    if problems.is_empty() {
        out(&format!("Snapshot {version} intact."));
        return Ok(0);
    }
    out(&format!(
        "Snapshot {version} was modified:\n{}",
        problems.join("\n")
    ));
    Ok(exit::PROBLEMS)
}

/// `__debug hot <what> ...`: what the modules without a native command of their own compute,
/// for the golden tests and the tests that need several processes.
/// - `add-comments <root> <tag> <count>`: `addComment(root, { comment: tag + i })` in a loop;
/// - `stack-detect <root>`: `JSON.stringify(detectStack(root))`;
/// - `freeze <root> [note]`: `freezeDesign(root, loadState(root), { note })`, prints the state.
pub(super) fn debug_hot(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = PathBuf::from(a.pos(3).unwrap_or_default());
    match a.pos(2) {
        Some("add-comments") => {
            let tag = a.pos(4).unwrap_or_default();
            let count: usize = a.pos(5).and_then(|n| n.parse().ok()).unwrap_or(1);
            for i in 0..count {
                add_comment(&root, &json!({ "comment": format!("{tag}{i}") }))?;
            }
            Ok(0)
        }
        Some("stack-detect") => {
            out(&to_json_compact(&detect_stack(&root)?));
            Ok(0)
        }
        Some("freeze") => {
            let mut state = require_state(&root)?;
            freeze_design(&root, &mut state, a.pos(4).map(Value::from))?;
            out(&to_json_compact(&state));
            Ok(0)
        }
        other => Err(DfwError::new(format!(
            "Unknown __debug hot subcommand \"{}\". Known: add-comments, stack-detect, freeze",
            other.unwrap_or_default()
        ))),
    }
}
