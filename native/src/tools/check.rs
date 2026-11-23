//! Ports `core/lib/check.mjs`: orchestrated checks (dfw lint, Impeccable detect, Hallmark
//! sloplint, optional Taste copy-lint) and the report.
//! Severity is normalized to: error (Must fix), warn (Should fix), info (Nice to have).
//!
//! The Impeccable part follows the hardening of 0.5.2 (the engine lookup is
//! `review::locate::resolve_engine`, the env filter is in `core::config::load_config`): only a
//! verified engine runs, directly; a tool that did not run is listed as `unverified`.
//!
//! A check result is the JSON object the Node code built: `{ tool, status, detail?, findings }`.
//
// Split by job: `run` (starting a tool), `report` (the text and the exit code), this file (the tools and the run
// over all of them).

mod report;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Map, Value};

pub use self::report::{check_exit_code, format_report};
use crate::core::config::env_map;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{canonicalize, project_paths, rel, resolve, to_posix};
use crate::core::process::{node_executable, plan_for, run_timed, Ran, Spawn};
use crate::core::util::read_json;
use crate::design::compose::{build_composed, build_system};
use crate::design::dsaudit::audit_design_system;

use crate::core::level::Level;
use crate::core::platform::node_platform;
use crate::design::lint::lint_design;
use crate::design::tokens::relative_path;
use crate::review::locate::{
    impeccable_engine, launcher_command, sloplint_script, taste_copy_lint_script, Engine,
};

/// `UI_EXT`: the files the checks look at.
pub const UI_EXT: [&str; 11] = [
    ".html", ".htm", ".css", ".scss", ".sass", ".less", ".jsx", ".tsx", ".vue", ".svelte", ".astro",
];

/// `path.extname`: from the last dot of the last segment, unless the name starts with it.
pub fn extname(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 && name != ".." => &name[i..],
        _ => "",
    }
}

fn is_url(target: &str) -> bool {
    target.starts_with("http:") || target.starts_with("https:")
}

/// `{ tool, status, detail, findings }`; `detail` is left out when there is none.
fn result(tool: &str, status: &str, detail: Option<String>, findings: Vec<Value>) -> Value {
    let mut map = Map::new();
    map.insert("tool".into(), json!(tool));
    map.insert("status".into(), json!(status));
    if let Some(detail) = detail {
        map.insert("detail".into(), json!(detail));
    }
    map.insert("findings".into(), Value::Array(findings));
    Value::Object(map)
}

/// Where the external tools are. The checks ask only for the tool they are about to run;
/// tests inject stand-ins so the real engine and scripts never start.
pub struct Lookup<'a> {
    pub engine: &'a dyn Fn() -> Result<Option<Engine>, DfwError>,
    pub sloplint: &'a dyn Fn() -> Result<Option<PathBuf>, DfwError>,
    pub taste: &'a dyn Fn() -> Result<Option<PathBuf>, DfwError>,
    /// The Node.js that runs the two scripts.
    pub node: &'a dyn Fn() -> Option<String>,
}

impl Lookup<'static> {
    /// The tools of the installed plugins.
    pub fn installed() -> Self {
        Lookup {
            engine: &impeccable_engine,
            sloplint: &sloplint_script,
            taste: &taste_copy_lint_script,
            node: &installed_node,
        }
    }
}

/// The Node.js on PATH (its absolute path), as the scripts' runner.
fn installed_node() -> Option<String> {
    node_executable().map(|p| p.display().to_string())
}

/// `parseJsonLoose`: the JSON that starts at the first `[` or `{` of a tool's output.
fn parse_json_loose(s: &str) -> Option<Value> {
    let start = s.find(['[', '{'])?;
    serde_json::from_str(&s[start..]).ok()
}

/// `(r.stderr || r.stdout).slice(0, 400)`.
fn error_detail(ran: &Ran) -> String {
    let text = if ran.stderr.is_empty() {
        &ran.stdout
    } else {
        &ran.stderr
    };
    js::slice16(text, 0, 400).to_string()
}

/// `toPosix(path.relative(root, path.resolve(root, file || '')))`: a tool's file, project-relative.
fn finding_file(root: &Path, file: Option<&Value>) -> String {
    let file = file
        .filter(|f| js::truthy(f))
        .map(|f| js::to_string(Some(f)))
        .unwrap_or_default();
    to_posix(&relative_path(root, &resolve(root.join(file))))
}

/// `f.line || 1`.
fn finding_line(finding: &Value) -> Value {
    finding
        .get("line")
        .filter(|l| js::truthy(l))
        .cloned()
        .unwrap_or_else(|| json!(1))
}

/// `${value}${fix ? ` | fix: ${fix}` : ''}`.
fn with_fix(text: String, finding: &Value) -> String {
    match finding.get("fix").filter(|f| js::truthy(f)) {
        Some(fix) => format!("{text} | fix: {}", js::to_string(Some(fix))),
        None => text,
    }
}

fn has_suffix(target: &str, suffixes: &[&str]) -> bool {
    suffixes.iter().any(|s| target.ends_with(s))
}

/// The options every external tool runs with.
#[derive(Clone, Copy)]
pub struct ToolRun<'a> {
    /// `config.env`: variables for the tool, on top of this process's environment.
    pub env: &'a [(String, String)],
    pub timeout_ms: u64,
}

/// `sloplintScope`: Hallmark leads marketing pages only.
fn sloplint_scope(root: &Path) -> Result<&'static str, DfwError> {
    let manifest = read_json(&project_paths(root).manifest, Some(json!({})))?;
    if manifest.is_null() {
        // A design.json that holds `null` ends the run (as in Node), with what to fix.
        return Err(DfwError::new(
            "design/design.json holds null, not the design manifest (a JSON object). Restore it from version control, or fix it by hand, and run the command again.",
        ));
    }
    Ok(
        if manifest.get("surface").and_then(Value::as_str) == Some("marketing") {
            "page"
        } else {
            "app"
        },
    )
}

/// `runSloplint(root, targets, { env, timeoutMs })`.
pub fn run_sloplint(
    root: &Path,
    targets: &[String],
    opts: ToolRun,
    lookup: &Lookup,
) -> Result<Value, DfwError> {
    const TOOL: &str = "sloplint";
    let Some(script) = (lookup.sloplint)()? else {
        return Ok(result(TOOL, "unavailable", None, Vec::new()));
    };
    // It reads files, not URLs.
    let files: Vec<&String> = targets
        .iter()
        .filter(|t| {
            !is_url(t)
                && has_suffix(
                    t,
                    &[".html", ".css", ".jsx", ".tsx", ".vue", ".svelte", ".astro"],
                )
        })
        .collect();
    if files.is_empty() {
        return Ok(result(TOOL, "skipped", None, Vec::new()));
    }
    let scope = sloplint_scope(root)?;
    let Some(node) = (lookup.node)() else {
        return Ok(result(
            TOOL,
            "unavailable",
            Some("needs Node.js on PATH".into()),
            Vec::new(),
        ));
    };
    let mut args = vec![script.to_string_lossy().into_owned()];
    args.extend(files.iter().map(|f| f.to_string()));
    args.extend(["--json", "--scope", scope].map(String::from));
    let ran = run(
        &plan_for(&node, args, cfg!(windows)),
        Some(root),
        opts.env,
        opts.timeout_ms,
    );
    if ran.timed_out {
        return Ok(result(TOOL, "timeout", None, Vec::new()));
    }
    let Some(data) = parse_json_loose(&ran.stdout) else {
        return Ok(result(TOOL, "error", Some(error_detail(&ran)), Vec::new()));
    };
    // Hallmark leads marketing pages only. On product UI its Floor is a strong recommendation
    // (precedence level 5), so FAILs do not block approval there.
    let grade = |f: &Value| match f.get("grade").and_then(Value::as_str) {
        Some("FAIL") => Some(
            if scope == "page" {
                Level::Error
            } else {
                Level::Warn
            }
            .as_str(),
        ),
        Some("REFLEX" | "WARN") => Some(Level::Warn.as_str()),
        _ => None,
    };
    let findings = data
        .get("findings")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .filter_map(|f| {
            // Gate 20 is Hallmark's own stamp requirement; it only applies to pages Hallmark built.
            let gate = js::to_string(f.get("gate"));
            let level = grade(f).filter(|_| gate != "20")?;
            Some(json!({
                "level": level,
                "rule": format!("hallmark-{gate}"),
                "file": finding_file(root, f.get("file")),
                "line": finding_line(f),
                "message": with_fix(js::to_string(f.get("evidence")), f),
                "source": "sloplint",
            }))
        })
        .collect();
    Ok(result(TOOL, "ran", None, findings))
}

/// `runImpeccable(root, targets, { env, timeoutMs, engine })`. Never downloads (a hook would
/// time out) and never runs an engine that failed the checksum.
pub fn run_impeccable(
    root: &Path,
    targets: &[String],
    opts: ToolRun,
    engine: Option<&Engine>,
) -> Value {
    const TOOL: &str = "impeccable";
    let Some(engine) = engine else {
        return result(TOOL, "unavailable", None, Vec::new());
    };
    let Some(cmd) = engine.cmd.as_ref().filter(|_| engine.ready) else {
        return result(TOOL, "not-installed", engine.detail.clone(), Vec::new());
    };
    let files: Vec<&String> = targets
        .iter()
        .filter(|t| UI_EXT.contains(&extname(t)) || is_url(t))
        .collect();
    if files.is_empty() {
        return result(TOOL, "skipped", None, Vec::new());
    }
    let mut args: Vec<String> = ["detect", "--json"].map(String::from).to_vec();
    args.extend(files.iter().map(|f| f.to_string()));
    let env: Vec<(String, String)> = opts.env.iter().chain(&engine.env).cloned().collect();
    let ran = run(
        &Spawn::direct(cmd.to_string_lossy(), args),
        Some(root),
        &env,
        opts.timeout_ms,
    );
    if ran.timed_out {
        return result(TOOL, "timeout", None, Vec::new());
    }
    let Some(Value::Array(data)) = parse_json_loose(&ran.stdout) else {
        return result(TOOL, "error", Some(error_detail(&ran)), Vec::new());
    };
    let findings = data
        .iter()
        .map(|f| {
            let severity = match f.get("severity").and_then(Value::as_str) {
                Some("error" | "critical") => Level::Error.as_str(),
                Some("info" | "advisory") => Level::Info.as_str(),
                _ => Level::Warn.as_str(),
            };
            let level = if f.get("advisory").is_some_and(js::truthy) {
                Level::Info.as_str()
            } else {
                severity
            };
            let snippet = f
                .get("snippet")
                .filter(|s| js::truthy(s))
                .map(|s| format!(" [{}]", js::slice16(&js::to_string(Some(s)), 0, 80)))
                .unwrap_or_default();
            json!({
                "level": level,
                "rule": format!("impeccable-{}", js::to_string(f.get("antipattern"))),
                "file": finding_file(root, f.get("file")),
                "line": finding_line(f),
                "message": format!("{}: {}{snippet}", js::to_string(f.get("name")), js::to_string(f.get("description"))),
                "source": "impeccable",
            })
        })
        .collect();
    result(TOOL, "ran", None, findings)
}

/// Variables for a tool, on top of this process's environment.
pub type Env = [(String, String)];

/// A check tool run with its timeout in milliseconds (`checks.timeout`).
fn run(command: &Spawn, cwd: Option<&Path>, env: &Env, timeout_ms: u64) -> Ran {
    run_timed(command, cwd, env, Duration::from_millis(timeout_ms))
}

/// What starting the upstream launcher gave (`dfw check --setup`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Launched {
    pub stderr: String,
    /// Why it did not start.
    pub error: Option<String>,
}

/// The result of [`setup_engine`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub ok: bool,
    pub text: String,
}

/// Start the launcher for [`setup_engine`]: up to three minutes, for the download.
pub fn launch(command: &Spawn, env: &Env) -> Launched {
    let ran = run(command, None, env, 180_000);
    Launched {
        stderr: ran.stderr,
        error: ran.error,
    }
}

/// `setupEngine`, "dfw check --setup": let the upstream launcher download the engine, then
/// accept it only when dfw verifies it too. The launcher is not started when it would run a
/// binary dfw cannot verify.
pub fn setup_engine(
    env: &[(String, String)],
    target: &str,
    resolve: &dyn Fn() -> Result<Option<Engine>, DfwError>,
    launch: &dyn Fn(&Spawn, &Env) -> Launched,
) -> Result<Setup, DfwError> {
    let ready = || Setup {
        ok: true,
        text: "Impeccable engine ready.".into(),
    };
    let Some(engine) = resolve()? else {
        return Ok(Setup {
            ok: false,
            text: "Impeccable plugin not found.".into(),
        });
    };
    if engine.ready {
        return Ok(ready());
    }
    if let Some(first) = engine.unverified.first() {
        return Ok(Setup { ok: false, text: format!("Impeccable setup stopped: the launcher would run an engine dfw cannot verify ({}). Remove it or unset IMPECCABLE_BIN, or allow it: dfw config set impeccable.allowUnverifiedEngine true --global", first.display()) });
    }
    let args: Vec<String> = ["detect", "--json", target].map(String::from).to_vec();
    let command = launcher_command(&engine.launcher.to_string_lossy(), &args, node_platform());
    let launched = launch(&command, env);
    let after = resolve()?;
    if after.as_ref().is_some_and(|e| e.ready) {
        return Ok(ready());
    }
    let why = [
        Some(launched.stderr),
        launched.error,
        after.and_then(|e| e.detail),
    ]
    .into_iter()
    .flatten()
    .find(|text| !text.is_empty())
    .unwrap_or_default();
    Ok(Setup {
        ok: false,
        text: format!("Impeccable setup failed: {}", js::slice16(&why, 0, 500)),
    })
}

/// `runTasteCopyLint(root, targets, { env, timeoutMs })`.
pub fn run_taste_copy_lint(
    root: &Path,
    targets: &[String],
    opts: ToolRun,
    lookup: &Lookup,
) -> Result<Value, DfwError> {
    const TOOL: &str = "taste-copy-lint";
    let Some(script) = (lookup.taste)()? else {
        return Ok(result(TOOL, "unavailable", None, Vec::new()));
    };
    let files: Vec<&String> = targets
        .iter()
        .filter(|t| has_suffix(t, &[".html", ".md", ".txt"]))
        .collect();
    if files.is_empty() {
        return Ok(result(TOOL, "skipped", None, Vec::new()));
    }
    let Some(node) = (lookup.node)() else {
        return Ok(result(
            TOOL,
            "unavailable",
            Some("needs Node.js on PATH".into()),
            Vec::new(),
        ));
    };
    let mut args = vec![script.to_string_lossy().into_owned()];
    args.extend(files.iter().map(|f| f.to_string()));
    args.push("--json".into());
    let ran = run(
        &plan_for(&node, args, cfg!(windows)),
        Some(root),
        opts.env,
        opts.timeout_ms,
    );
    let Some(data) = parse_json_loose(&ran.stdout) else {
        return Ok(result(TOOL, "error", Some(error_detail(&ran)), Vec::new()));
    };
    let mut findings = Vec::new();
    let reports = data.get("files").and_then(Value::as_array);
    for (i, report) in reports
        .map(Vec::as_slice)
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let list = report.get("findings").and_then(Value::as_array);
        for f in list.map(Vec::as_slice).unwrap_or_default() {
            let sev = f.get("sev").and_then(Value::as_str);
            if sev == Some("INFO") {
                continue;
            }
            let text = f.get("msg").filter(|m| js::truthy(m)).or(f.get("id"));
            findings.push(json!({
                // Reference-mode skill: never a hard blocker.
                "level": if sev == Some("FAIL") { Level::Warn } else { Level::Info }.as_str(),
                "rule": format!("taste-{}", js::to_string(f.get("id"))),
                "file": files.get(i).map(|f| f.as_str()).unwrap_or_default(),
                "line": 1,
                "message": with_fix(js::to_string(text), f),
                "source": "taste-copy-lint",
            }));
        }
    }
    Ok(result(TOOL, "ran", None, findings))
}

// ---------------------------------------------------------------- all checks

/// Options of [`run_checks`].
#[derive(Clone, Copy, Default)]
pub struct CheckOptions<'a> {
    /// Budget of each external tool; `checks.timeoutMs`, then 8 s, when `None`.
    pub timeout_ms: Option<u64>,
    /// Only these tools (`dfw`, `impeccable`, `sloplint`, `taste`); every tool when `None`.
    pub tools: Option<&'a [&'a str]>,
}

/// `projectRel`: a target typed as `./design/x.html`, an absolute path or relative to the
/// current directory, made project-relative.
fn project_rel(root: &Path, target: &str) -> String {
    let abs = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        let from_cwd = resolve(target);
        if from_cwd.exists() {
            from_cwd
        } else {
            resolve(root.join(target))
        }
    };
    let mut rel = relative_path(root, &abs);
    if rel.starts_with("..") {
        // The same file reached through a symlinked directory (/tmp vs /private/tmp).
        if let (Ok(real_root), Ok(real_abs)) = (canonicalize(root), canonicalize(&abs)) {
            rel = relative_path(&real_root, &real_abs);
        }
    }
    to_posix(&rel)
}

/// `listDesignUi`: every UI file under design/, without the approved snapshots, the handoff
/// and the component masters.
fn list_design_ui(root: &Path) -> Result<Vec<String>, DfwError> {
    fn walk(abs: &Path, rel: &str, out: &mut Vec<String>) -> Result<(), DfwError> {
        // Node lists a directory sorted by bytes.
        let mut entries: Vec<(String, bool)> = Vec::new();
        for entry in fs::read_dir(abs)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            entries.push((name, entry.file_type()?.is_dir()));
        }
        entries.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
        for (name, is_dir) in entries {
            let r = format!("{rel}/{name}");
            if is_dir {
                let skipped = name == "approved"
                    || name == "handoff"
                    || (name == "components" && rel.ends_with("/system"));
                if !skipped {
                    walk(&abs.join(&name), &r, out)?;
                }
            } else if UI_EXT.contains(&extname(&name)) {
                out.push(r);
            }
        }
        Ok(())
    }
    let dir = project_paths(root).design;
    let mut out = Vec::new();
    if dir.exists() {
        walk(&dir, "design", &mut out)?;
    }
    Ok(out)
}

/// Composed copies handed to the tools: the copy (`.dfw/build/...`) and the design file it was
/// made from.
type Composed = Vec<(String, String)>;

/// The files to hand to the external tools, and which of them are composed copies.
fn tool_targets(root: &Path, external: Vec<String>) -> Result<(Vec<String>, Composed), DfwError> {
    let mut expanded = Vec::new();
    for t in external {
        if t == "design" {
            expanded.extend(list_design_ui(root)?);
        } else {
            expanded.push(t);
        }
    }
    expanded.retain(|t| !rel::under(rel::COMPONENTS, t));
    let mut composed = Composed::new();
    let mut targets = Vec::new();
    for t in expanded {
        if is_url(&t) || !t.ends_with(".html") {
            targets.push(t);
            continue;
        }
        let built = build_composed(root, std::slice::from_ref(&t))?;
        match built.first().filter(|b| b.composed) {
            Some(b) => {
                let rel = to_posix(&relative_path(root, &b.out));
                match composed.iter_mut().find(|(k, _)| *k == rel) {
                    Some(slot) => slot.1 = t,
                    None => composed.push((rel.clone(), t)),
                }
                targets.push(rel);
            }
            None => targets.push(t),
        }
    }
    Ok((targets, composed))
}

/// Point the findings a tool reported on a composed copy at the design file it was made from.
fn remap(mut res: Value, composed: &[(String, String)]) -> Value {
    let source_of = |key: &str| composed.iter().find(|(k, _)| k == key).map(|(_, v)| v);
    let list = res.get_mut("findings").and_then(Value::as_array_mut);
    for f in list.into_iter().flatten() {
        let k = to_posix(
            &f.get("file")
                .filter(|v| js::truthy(v))
                .map(|v| js::to_string(Some(v)))
                .unwrap_or_default(),
        );
        // `k.replace(/^.*?(\.dfw\/build\/)/, '$1')`: an absolute path of a composed copy.
        let tail = k
            .find(".dfw/build/")
            .filter(|i| !k[..*i].contains(['\n', '\r', '\u{2028}', '\u{2029}']));
        let hit = source_of(&k).or_else(|| tail.and_then(|i| source_of(&k[i..])));
        if let (Some(hit), Value::Object(map)) = (hit, f) {
            map.insert("file".into(), json!(hit));
        }
    }
    res
}

/// `runChecks(root, targets, config, { timeoutMs, tools })` with the installed tools.
pub fn run_checks(
    root: &Path,
    targets: Option<&[String]>,
    config: &Value,
    opts: CheckOptions,
) -> Result<Vec<Value>, DfwError> {
    run_checks_with(root, targets, config, opts, &Lookup::installed())
}

/// Run all enabled checks on `targets` (project-relative paths or URLs). When `targets` is
/// `None`, checks the whole design workspace.
pub fn run_checks_with(
    root: &Path,
    targets: Option<&[String]>,
    config: &Value,
    opts: CheckOptions,
    lookup: &Lookup,
) -> Result<Vec<Value>, DfwError> {
    let p = project_paths(root);
    let checks = config.get("checks");
    let setting = |key: &str| checks.and_then(|c| c.get(key));
    let env = env_map(config);
    let configured = setting("timeoutMs")
        .and_then(Value::as_f64)
        .filter(|n| *n >= 1.0)
        .map(|n| n as u64);
    let budget = opts
        .timeout_ms
        .filter(|n| *n > 0)
        .or(configured)
        .unwrap_or(8000);
    // Targets may be typed as ./design/x.html, an absolute path or relative to the current
    // directory: make them project-relative.
    let targets: Option<Vec<String>> = targets.map(|list| {
        list.iter()
            .map(|t| {
                if is_url(t) {
                    t.clone()
                } else {
                    project_rel(root, t)
                }
            })
            .collect()
    });
    let design_targets: Vec<String> = match &targets {
        Some(list) => list.iter().filter(|t| !is_url(t)).cloned().collect(),
        None if p.design.exists() => vec!["design".to_string()],
        None => Vec::new(),
    };
    let mut results = Vec::new();
    let want = |name: &str| opts.tools.is_none_or(|tools| tools.contains(&name));

    if want("dfw") {
        let lint_targets: Option<Vec<String>> = targets.as_ref().map(|_| {
            design_targets
                .iter()
                .filter(|t| t.starts_with("design/"))
                .cloned()
                .collect()
        });
        let findings = match &lint_targets {
            Some(list) if list.is_empty() => Vec::new(),
            other => lint_design(root, other.as_deref())?,
        };
        results.push(result("dfw-lint", "ran", None, findings));
        let system_touched = lint_targets.as_ref().is_none_or(|list| {
            list.iter()
                .any(|t| rel::under(rel::TOKENS, t) || rel::under(rel::SYSTEM, t))
        });
        if system_touched {
            results.push(result(
                "dfw-ds-audit",
                "ran",
                None,
                audit_design_system(root)?,
            ));
        }
    }
    if !["impeccable", "sloplint", "taste"].iter().any(|t| want(t)) {
        return Ok(results);
    }
    // External tools read plain HTML: give them composed copies (components and icons expanded)
    // and map findings back to the source files.
    build_system(root)?;
    let external = targets.unwrap_or(design_targets);
    let (tool_targets, composed) = tool_targets(root, external)?;
    let remap = |res: Value| remap(res, &composed);
    let run_opts = ToolRun {
        env: &env,
        timeout_ms: budget,
    };
    if want("impeccable") && setting("impeccable") != Some(&Value::Bool(false)) {
        let engine = (lookup.engine)()?;
        results.push(remap(run_impeccable(
            root,
            &tool_targets,
            run_opts,
            engine.as_ref(),
        )));
    }
    if want("sloplint") && setting("sloplint") != Some(&Value::Bool(false)) {
        results.push(remap(run_sloplint(root, &tool_targets, run_opts, lookup)?));
    }
    if want("taste") && setting("tasteCopyLint").is_some_and(js::truthy) {
        results.push(remap(run_taste_copy_lint(
            root,
            &tool_targets,
            run_opts,
            lookup,
        )?));
    }
    Ok(results)
}

#[cfg(test)]
mod tests;
