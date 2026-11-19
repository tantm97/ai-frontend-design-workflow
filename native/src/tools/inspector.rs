//! Ports `core/lib/inspector.mjs`: source mapping for the running app. code-inspector-plugin
//! (MIT, github.com/zh-lx/code-inspector) adds data-insp-path="<file>:<line>:<column>:<tag>" to
//! every element in development builds only. dfw never adds it silently: it is a devDependency
//! the user approves at G3 like any other. "dfw inspector setup" prints the change; "--write"
//! applies it only after that approval.
//!
//! Over 800 lines with its tests: the cases of inspector.test.mjs and inspector-shapes.test.mjs
//! are the second half of the file.

use std::fs;
use std::path::Path;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::util::{ensure_inside, read_json, to_json_pretty, write_atomic_inside};
use crate::workflow::stack::detect_stack;
use crate::workflow::state::{gate_passed, load_state};

/// The npm package.
pub const PACKAGE: &str = "code-inspector-plugin";
/// Pinned; exact version, no range.
pub const VERSION: &str = "2.0.9";
pub const LICENSE: &str = "MIT";
const REPO: &str = "https://github.com/zh-lx/code-inspector";
const PURPOSE: &str = "dev-only source mapping (element -> file:line:col) for click-to-comment on the running app (dfw review)";

// Relative paths in the attribute; no hotkeys, switch, console tips or editor-launch server:
// dfw only reads the attribute. The plugin itself is inactive outside development
// (vite: apply on "serve" only; webpack: mode/NODE_ENV development; turbopack: NODE_ENV development).
const OPTIONS: &str =
    "pathType: 'relative', hotKeys: false, showSwitch: false, hideConsole: true, server: 'close'";

fn call(bundler: &str) -> String {
    format!("codeInspectorPlugin({{ bundler: '{bundler}', {OPTIONS} }})")
}

/// A regular expression that is part of the program. A pattern that does not compile would be
/// a typo the first test running it catches; callers then take their "no match" path.
fn re(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

fn is_match(pattern: &str, text: &str) -> bool {
    re(pattern).is_some_and(|r| r.is_match(text))
}

/* ---------------------------------------------------------------- data-insp-path */

/// A parsed `data-insp-path` value.
#[derive(Debug, Clone, PartialEq)]
pub struct InspPath {
    pub file: String,
    pub line: f64,
    pub column: Option<f64>,
    pub tag: Option<String>,
    /// "file:line:col" (or "file:line").
    pub source: String,
}

fn all_digits(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())
}

/// `parseInspPath(value, root)`: parse a data-insp-path value ("src/Foo.tsx:42:7:div",
/// "C:\\app\\src\\Foo.tsx:42:7:div"). Absolute paths inside `root` become project-relative
/// (POSIX separators). `None` when the value is not one.
pub fn parse_insp_path(value: &str, root: Option<&str>) -> Option<InspPath> {
    let mut parts: Vec<&str> = value.trim().split(':').collect();
    if value.trim().is_empty() {
        return None;
    }
    let mut tag = None;
    if parts.len() > 2 && !parts.last().is_some_and(|p| all_digits(p)) {
        tag = parts.pop().map(str::to_string);
    }
    let mut nums: Vec<f64> = Vec::new();
    while parts.len() > 1 && nums.len() < 2 && parts.last().is_some_and(|p| all_digits(p)) {
        nums.insert(0, parts.pop().and_then(|p| p.parse().ok())?);
    }
    let (line, column) = (*nums.first()?, nums.get(1).copied());
    let mut file = parts.join(":").replace('\\', "/");
    if file.is_empty() {
        return None;
    }
    if let Some(root) = root.filter(|r| !r.is_empty()) {
        let b = file.as_bytes();
        let is_win = b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/';
        if is_win || file.starts_with('/') {
            let rel = if is_win {
                relative(&root.replace('/', "\\"), &file.replace('/', "\\"), true)
            } else {
                relative(&crate::core::paths::to_posix(root), &file, false)
            };
            let absolute =
                rel.starts_with(['/', '\\']) || (is_win && rel.as_bytes().get(1) == Some(&b':'));
            if !rel.is_empty() && !rel.starts_with("..") && !absolute {
                file = rel.replace('\\', "/");
            }
        }
    }
    if let Some(rest) = file.strip_prefix("./") {
        file = rest.to_string();
    }
    let source = match column {
        Some(c) => format!("{file}:{}:{}", js::number(line), js::number(c)),
        None => format!("{file}:{}", js::number(line)),
    };
    Some(InspPath {
        file,
        line,
        column,
        tag,
        source,
    })
}

/// `path.posix.relative` / `path.win32.relative` for an absolute `to`. Windows paths compare
/// without case; a path on another drive is returned as it is.
fn relative(from: &str, to: &str, win: bool) -> String {
    let sep = if win { "\\" } else { "/" };
    let split = |p: &str| -> (String, Vec<String>) {
        let mut p = p.to_string();
        if !win && !p.starts_with('/') {
            p = resolve(&p).display().to_string();
        }
        let (root, rest) = match p.as_bytes() {
            [d, b':', ..] if win && d.is_ascii_alphabetic() => {
                (p[..2].to_ascii_lowercase(), &p[2..])
            }
            _ => (String::new(), &p[..]),
        };
        let mut parts: Vec<String> = Vec::new();
        for part in rest
            .split(['/', '\\'])
            .filter(|s| !s.is_empty() && *s != ".")
        {
            if part == ".." {
                parts.pop();
            } else {
                parts.push(part.to_string());
            }
        }
        (root, parts)
    };
    let (from_root, from_parts) = split(from);
    let (to_root, to_parts) = split(to);
    if from_root != to_root {
        return to.to_string();
    }
    let same = |a: &String, b: &String| {
        if win {
            a.eq_ignore_ascii_case(b)
        } else {
            a == b
        }
    };
    let common = from_parts
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| same(a, b))
        .count();
    let mut out: Vec<&str> = vec![".."; from_parts.len() - common];
    out.extend(to_parts[common..].iter().map(String::as_str));
    out.join(sep)
}

/* ---------------------------------------------------------------- bundler detection */

const CONFIGS: [(&str, &[&str]); 9] = [
    (
        "vite",
        &[
            "vite.config.ts",
            "vite.config.mts",
            "vite.config.js",
            "vite.config.mjs",
            "vite.config.cjs",
            "vite.config.cts",
        ],
    ),
    (
        "webpack",
        &[
            "webpack.config.js",
            "webpack.config.cjs",
            "webpack.config.mjs",
            "webpack.config.ts",
            "webpack.dev.js",
            "webpack.config.dev.js",
        ],
    ),
    (
        "next",
        &[
            "next.config.ts",
            "next.config.mjs",
            "next.config.js",
            "next.config.cjs",
        ],
    ),
    ("nuxt", &["nuxt.config.ts", "nuxt.config.js"]),
    (
        "astro",
        &["astro.config.mjs", "astro.config.ts", "astro.config.js"],
    ),
    (
        "rsbuild",
        &[
            "rsbuild.config.ts",
            "rsbuild.config.js",
            "rsbuild.config.mjs",
        ],
    ),
    (
        "rspack",
        &["rspack.config.js", "rspack.config.ts", "rspack.config.mjs"],
    ),
    (
        "umi",
        &[
            ".umirc.ts",
            ".umirc.js",
            "config/config.ts",
            "config/config.js",
        ],
    ),
    ("vue-cli", &["vue.config.js"]),
];

/// `major(v)`: the first "major[.minor]" in a version range.
fn major(v: &str) -> Option<(u64, u64)> {
    let m = re(r"([0-9]+)(?:\.([0-9]+))?")?.captures(v)?;
    let n = |i: usize| m.get(i).and_then(|x| x.as_str().parse().ok());
    Some((n(1)?, n(2).unwrap_or(0)))
}

/// Result of [`detect_bundler`].
#[derive(Debug, Clone, PartialEq)]
pub struct Bundler {
    pub bundler: Option<&'static str>,
    pub config_file: Option<&'static str>,
    pub package_json: bool,
    /// The `next` dependency as written, for Next.js.
    pub next_version: Option<Value>,
    /// "webpack", "turbopack" or "turbo-experimental", for Next.js.
    pub next_mode: Option<&'static str>,
}

/// `detectBundler(root)`: which bundler serves the app in development, and its config file.
pub fn detect_bundler(root: &Path) -> Result<Bundler, DfwError> {
    let pkg = read_json(&root.join("package.json"), Some(json!({})))?;
    let mut deps = js::spread(pkg.get("dependencies"));
    deps.extend(js::spread(pkg.get("devDependencies")));
    let has = |d: &str| deps.get(d).is_some_and(js::truthy);
    let order = [
        ("next", has("next")),
        ("nuxt", has("nuxt")),
        ("astro", has("astro")),
        ("umi", has("umi") || has("@umijs/max")),
        ("rsbuild", has("@rsbuild/core")),
        ("rspack", has("@rspack/cli") || has("@rspack/core")),
        ("vue-cli", has("@vue/cli-service")),
        ("vite", has("vite") || has("@sveltejs/kit")),
        ("webpack", has("webpack") || has("webpack-cli")),
        ("esbuild", has("esbuild")),
    ];
    let exists = |f: &str| root.join(f).exists();
    let bundler = order
        .iter()
        .find(|(_, ok)| *ok)
        .map(|(b, _)| *b)
        .or_else(|| {
            // No dependency hint: fall back to config files present.
            CONFIGS
                .iter()
                .find(|(_, files)| files.iter().any(|f| exists(f)))
                .map(|(b, _)| *b)
        });
    let config_file = CONFIGS
        .iter()
        .find(|(b, _)| Some(*b) == bundler)
        .and_then(|(_, files)| files.iter().find(|f| exists(f)).copied());
    let mut out = Bundler {
        bundler,
        config_file,
        package_json: root.join("package.json").exists(),
        next_version: None,
        next_mode: None,
    };
    if bundler == Some("next") {
        let v = major(&js::or_empty(deps.get("next")));
        let dev = js::or_empty(pkg.get("scripts").and_then(|s| s.get("dev")));
        let turbo = is_match(r"--turbo(pack)?(?-u:\b)", &dev);
        out.next_version = deps.get("next").cloned();
        // Next 16 develops with Turbopack by default; 15.3+ with --turbopack; otherwise webpack.
        out.next_mode = Some(if is_match(r"--webpack(?-u:\b)", &dev) {
            "webpack"
        } else if v
            .is_some_and(|(maj, min)| maj >= 16 || (turbo && (maj > 15 || (maj == 15 && min >= 3))))
        {
            "turbopack"
        } else if turbo {
            "turbo-experimental"
        } else {
            "webpack"
        });
    }
    Ok(out)
}

/* ---------------------------------------------------------------- config patching */

fn is_esm(file: &str, text: &str) -> bool {
    [".mjs", ".mts", ".ts"]
        .iter()
        .any(|ext| file.ends_with(ext))
        || is_match(r"(?m)^\s*(import\s|export\s)", text)
}

fn add_import(text: &str, esm: bool) -> String {
    if text.contains("'code-inspector-plugin'") || text.contains("\"code-inspector-plugin\"") {
        return text.to_string();
    }
    if !esm {
        let line = "const { codeInspectorPlugin } = require('code-inspector-plugin');\n";
        let strict = re(r#"^(?:'use strict'|"use strict");?\s*\n"#).and_then(|r| r.find(text));
        return match strict {
            Some(strict) => format!("{}{line}{}", strict.as_str(), &text[strict.end()..]),
            None => format!("{line}{text}"),
        };
    }
    let line = "import { codeInspectorPlugin } from 'code-inspector-plugin';\n";
    // After the last top-level import statement (single- or multi-line, optional trailing // comment).
    // One statement per match: nothing before the module string may hold a quote or a semicolon.
    let imports = re(r#"(?m)^import(?-u:\b)[^'";]*['"][^'"\n]+['"];?[ \t]*(?://[^\n]*)?\r?\n"#);
    let end = imports
        .and_then(|r| r.find_iter(text).last())
        .map_or(0, |m| m.end());
    format!("{}{line}{}", &text[..end], &text[end..])
}

/// Brace depth at an index, or `None` when it sits inside a comment or a string (rough: good
/// enough for config files).
fn depth_at(text: &str, index: usize) -> Option<i32> {
    let t = text.as_bytes();
    let mut depth = 0;
    let mut quote: Option<u8> = None;
    let mut i = 0;
    while i < index {
        let c = t[i];
        if let Some(q) = quote {
            if c == b'\\' {
                i += 1;
            } else if c == q {
                quote = None;
            }
        } else if c == b'"' || c == b'\'' || c == b'`' {
            quote = Some(c);
        } else if c == b'/' && matches!(t.get(i + 1), Some(b'/') | Some(b'*')) {
            let line = t[i + 1] == b'/';
            let end = if line {
                text[i..].find('\n').map(|at| at + i)
            } else {
                text.get(i + 2..)
                    .and_then(|rest| rest.find("*/"))
                    .map(|at| at + i + 2)
            };
            match end {
                Some(end) if end < index => i = if line { end } else { end + 1 },
                _ => return None,
            }
        } else if c == b'{' {
            depth += 1;
        } else if c == b'}' {
            depth -= 1;
        }
        i += 1;
    }
    quote.is_none().then_some(depth)
}

/// Insert `item` as the first element of the config's own `plugins: [` array; `None` when
/// there is none. Lists in comments and nested lists (css.postcss.plugins, resolve.plugins,
/// rollupOptions.plugins) are not it.
fn add_to_plugins(text: &str, item: &str) -> Option<String> {
    let (mut best, mut best_depth) = (None, i32::MAX);
    for m in re(r"(?-u:\b)plugins\s*:\s*\[")?.find_iter(text) {
        // The shallowest list; the first one among equals.
        match depth_at(text, m.start()) {
            Some(d) if d <= 1 && d < best_depth => (best, best_depth) = (Some(m.end()), d),
            _ => {}
        }
    }
    let at = best?;
    let (head, rest) = text.split_at(at);
    let after = rest.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let ws = &rest[..rest.len() - after.len()];
    Some(if after.starts_with(']') {
        format!("{head}{item}{rest}") // empty list
    } else if ws.contains('\n') {
        format!("{head}{ws}{item},{rest}") // one plugin per line
    } else {
        format!("{head}{item}, {rest}")
    })
}

/// Insert `prop` as the first property of the exported config object; `None` when not found.
fn add_property(text: &str, prop: &str) -> Option<String> {
    let shapes = [
        // defineConfig({ / defineConfig(({ mode }) => ({ ; never a function body
        r"defineConfig\(\s*(?:(?:async\s*)?\([^)]*\)\s*=>\s*\(\s*)?\{",
        r"(?:const|let|var)\s+nextConfig\s*(?::\s*[A-Za-z0-9_.]+\s*)?=\s*\{",
        r"module\.exports\s*=\s*(?:\([^)]*\)\s*=>\s*\(\s*)?\{",
        r"export\s+default\s*\{",
    ];
    let at = shapes.iter().find_map(|s| re(s)?.find(text))?.end();
    Some(format!("{}\n  {prop},{}", &text[..at], &text[at..]))
}

/// Result of [`patch_config`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Patch {
    /// The new text; `changed` is false when the file already loads the plugin.
    Text { text: String, changed: bool },
    /// The file's shape is not one dfw edits safely: what to do by hand.
    Manual(String),
}

/// `patchConfig(text, { bundler, file, nextMode })`: patch a bundler config so it loads
/// code-inspector-plugin.
pub fn patch_config(
    text: &str,
    bundler: Option<&str>,
    file: &str,
    next_mode: Option<&str>,
) -> Patch {
    if text.contains("code-inspector-plugin") || text.contains("codeInspectorPlugin") {
        return Patch::Text {
            text: text.to_string(),
            changed: false,
        };
    }
    let manual = |why: &str| Patch::Manual(why.to_string());
    let list = |b: &str| {
        add_to_plugins(text, &call(b))
            .or_else(|| add_property(text, &format!("plugins: [{}]", call(b))))
    };
    let body = match (bundler, next_mode.unwrap_or("webpack")) {
        (Some(b @ ("vite" | "webpack")), _) => list(b),
        (Some("next"), "turbopack") => {
            if is_match(r"(?-u:\b)turbopack\s*:", text) {
                return manual("next.config already has a \"turbopack\" key: add rules: codeInspectorPlugin(...) to it by hand.");
            }
            add_property(text, &format!("turbopack: {{ rules: {} }}", call("turbopack")))
        }
        (Some("next"), "webpack") => {
            if is_match(r"(?-u:\b)webpack\s*[:(]", text) {
                return manual("next.config already has a \"webpack\" function: push the plugin inside it, only when dev is true.");
            }
            add_property(
                text,
                &format!(
                    "webpack: (config, {{ dev }}) => {{\n    if (dev) config.plugins.push({});\n    return config;\n  }}",
                    call("webpack")
                ),
            )
        }
        (Some("next"), _) => {
            return manual("Next.js 15.0-15.2 with Turbopack: use experimental.turbo.rules = codeInspectorPlugin({ bundler: \"turbopack\", ... }).")
        }
        (other, _) => {
            return Patch::Manual(format!(
                "dfw does not edit {} config files automatically.",
                other.filter(|b| !b.is_empty()).unwrap_or("this")
            ))
        }
    };
    match body {
        Some(body) => Patch::Text {
            text: add_import(&body, is_esm(file, text)),
            changed: true,
        },
        None => Patch::Manual(format!(
            "could not find the plugin list or the exported config object in {}.",
            if file.is_empty() { "the config" } else { file }
        )),
    }
}

fn manual_for(bundler: Option<&str>) -> String {
    match bundler {
        Some("nuxt") => format!("nuxt.config: vite: {{ plugins: [{}] }}", call("vite")),
        Some("astro") => format!("astro.config: vite: {{ plugins: [{}] }}", call("vite")),
        Some("rsbuild") => format!("rsbuild.config: tools: {{ rspack: {{ plugins: [{}] }} }}", call("rspack")),
        Some("rspack") => format!("rspack.config: plugins: [{}]", call("rspack")),
        Some("umi") => format!(
            ".umirc / config: chainWebpack(memo) {{ memo.plugin('code-inspector-plugin').use({}) }} (or mako: {{ plugins: [{}] }})",
            call("webpack"),
            call("mako")
        ),
        Some("vue-cli") => format!(
            "vue.config.js: chainWebpack: (config) => {{ config.plugin('code-inspector-plugin').use({}) }}",
            call("webpack")
        ),
        Some("esbuild") => format!(
            "your esbuild script: plugins: [codeInspectorPlugin({{ bundler: 'esbuild', dev: () => <true only for the dev build>, {OPTIONS} }})]"
        ),
        _ => format!("Unknown bundler: see https://github.com/zh-lx/code-inspector#-usage and pass the options {{ {OPTIONS} }}."),
    }
}

/* ---------------------------------------------------------------- plan and setup */

/// `inspectorPlan(root)`: what "dfw inspector setup" would do in this project, with the keys
/// in the order Node built them.
pub fn inspector_plan(root: &Path) -> Result<Value, DfwError> {
    let stack = detect_stack(root)?;
    let framework = js::or_null(stack.get("framework").and_then(|f| f.get("name")));
    let manager = js::or_empty(stack.get("packageManager"));
    let b = detect_bundler(root)?;
    let mut plan = Map::new();
    plan.insert("bundler".into(), json!(b.bundler));
    plan.insert("configFile".into(), json!(b.config_file));
    plan.insert("packageJson".into(), json!(b.package_json));
    if b.bundler == Some("next") {
        if let Some(v) = &b.next_version {
            plan.insert("nextVersion".into(), v.clone());
        }
        plan.insert("nextMode".into(), json!(b.next_mode));
    }
    plan.insert(
        "dependency".into(),
        json!({ "name": PACKAGE, "version": VERSION, "dev": true, "license": LICENSE, "purpose": PURPOSE }),
    );
    plan.insert("framework".into(), framework);
    let manager = if manager.is_empty() {
        "npm".into()
    } else {
        manager
    };
    plan.insert("packageManager".into(), json!(manager));
    // `INSTALL[plan.packageManager] || 'npm install'`.
    let install = match manager.as_str() {
        "npm" | "pnpm" | "yarn" | "bun" => format!("{manager} install"),
        _ => "npm install".into(),
    };
    plan.insert(
        "install".into(),
        json!(format!(
            "{install} (after adding \"{PACKAGE}\": \"{VERSION}\" to devDependencies)"
        )),
    );
    let unsupported = |plan: &mut Map<String, Value>, why: String| {
        plan.insert("supported".into(), json!(false));
        plan.insert("manual".into(), json!(why));
    };
    if !b.package_json {
        unsupported(&mut plan, "No package.json: code-inspector-plugin needs a JavaScript bundler (Vite, webpack, Rspack, esbuild, Turbopack...). Live review still works without it (comments carry selector + text, no file:line).".into());
        return Ok(Value::Object(plan));
    }
    let edited = matches!(b.bundler, Some("vite" | "webpack" | "next"));
    match (edited, b.config_file) {
        (true, Some(file)) => {
            let text = fs::read_to_string(root.join(file))?;
            match patch_config(&text, b.bundler, file, b.next_mode) {
                Patch::Manual(why) => unsupported(&mut plan, why),
                Patch::Text { text, changed } => {
                    plan.insert("supported".into(), json!(true));
                    plan.insert("patched".into(), json!(text));
                    plan.insert("alreadyConfigured".into(), json!(!changed));
                }
            }
        }
        (true, None) => {
            let name = b.bundler.unwrap_or_default();
            let (what, with) = if name == "next" {
                ("webpack/turbopack", "webpack")
            } else {
                ("plugins", name)
            };
            unsupported(
                &mut plan,
                format!(
                    "No {name} config file found: create one with {what}: [{}].",
                    call(with)
                ),
            );
        }
        (false, _) => unsupported(&mut plan, manual_for(b.bundler)),
    }
    plan.insert(
        "devDependency".into(),
        json!(format!("{PACKAGE}@{VERSION}")),
    );
    Ok(Value::Object(plan))
}

fn text_of<'a>(plan: &'a Value, key: &str) -> &'a str {
    plan.get(key).and_then(Value::as_str).unwrap_or_default()
}

/// `formatPlan(plan)`: the plan as "dfw inspector setup" prints it.
pub fn format_plan(plan: &Value) -> String {
    let bundler = Some(text_of(plan, "bundler"))
        .filter(|b| !b.is_empty())
        .unwrap_or("unknown");
    let mode = Some(text_of(plan, "nextMode"))
        .filter(|m| !m.is_empty())
        .map(|m| format!(" ({m})"))
        .unwrap_or_default();
    let config = text_of(plan, "configFile");
    let config_note = if config.is_empty() {
        String::new()
    } else {
        format!(", config: {config}")
    };
    let install = text_of(plan, "install")
        .split(" (")
        .next()
        .unwrap_or_default();
    let step = if plan.get("supported") == Some(&json!(true)) {
        if plan.get("alreadyConfigured") == Some(&json!(true)) {
            format!("2. {config} already loads the plugin.")
        } else {
            format!("2. {config} becomes:\n\n{}", text_of(plan, "patched"))
        }
    } else {
        format!("2. by hand: {}", text_of(plan, "manual"))
    };
    [
        format!("code-inspector-plugin {VERSION} ({LICENSE}, {REPO})"),
        format!("purpose: {PURPOSE}; development builds only (it does nothing in production builds)."),
        format!("bundler: {bundler}{mode}{config_note}"),
        format!("1. devDependency: \"{PACKAGE}\": \"{VERSION}\"  then {install}"),
        step,
        String::new(),
        "Needs the user's approval at G3: list it in .dfw/stack.json dependencies, e.g.".to_string(),
        format!("  {{\"name\":\"{PACKAGE}\",\"version\":\"{VERSION}\",\"dev\":true,\"license\":\"{LICENSE}\",\"purpose\":\"live review source mapping\"}}"),
        "then \"dfw approve G3 ...\" and \"dfw inspector setup --write\".".to_string(),
    ]
    .join("\n")
}

/// `stackApproves(stackJson)`: true when .dfw/stack.json lists code-inspector-plugin
/// (dependencies or devDependencies).
pub fn stack_approves(stack: &Value) -> bool {
    ["dependencies", "devDependencies"]
        .iter()
        .any(|key| match stack.get(key) {
            Some(Value::Array(list)) => list.iter().any(|d| match d {
                Value::String(s) => {
                    s.split('@').next().filter(|n| !n.is_empty()).unwrap_or(s) == PACKAGE
                }
                other => other.get("name").and_then(Value::as_str) == Some(PACKAGE),
            }),
            Some(Value::Object(map)) => map.contains_key(PACKAGE),
            _ => false,
        })
}

/// `JSON.stringify(value, null, indent)`: the two-space layout re-indented, which is safe
/// because a JSON string never holds a raw line break.
fn to_json_indented(value: &Value, indent: &str) -> String {
    let indent: String = indent.chars().take(10).collect();
    to_json_pretty(value)
        .split('\n')
        .map(|line| {
            let body = line.trim_start_matches(' ');
            format!("{}{body}", indent.repeat((line.len() - body.len()) / 2))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `inspectorSetup(root, { write })`: the plan, and with `write` the files changed. Writing is
/// refused unless G3 is approved and the dependency is in the approved stack.
pub fn inspector_setup(root: &Path, write: bool) -> Result<(Value, Vec<String>), DfwError> {
    let plan = inspector_plan(root)?;
    if !write {
        return Ok((plan, Vec::new()));
    }
    let stack = read_json(&project_paths(root).stack, Some(Value::Null))?;
    let mut missing = Vec::new();
    if !load_state(root)?.is_some_and(|state| gate_passed(&state, "G3")) {
        missing.push("G3 (technology stack) is not approved".to_string());
    }
    if !stack_approves(&stack) {
        missing.push(format!(
            ".dfw/stack.json does not list \"{PACKAGE}\" in its dependencies"
        ));
    }
    if !missing.is_empty() {
        return Err(DfwError::new(format!(
            "dfw inspector setup --write refused: {}. New dependencies need the user's approval: propose {PACKAGE}@{VERSION} (dev-only, {LICENSE}) with \"dfw stack set\" (dependencies: [{{\"name\":\"{PACKAGE}\",\"version\":\"{VERSION}\",\"dev\":true}}]), get the user's OK, \"dfw approve G3 --note ...\", then run this again.",
            missing.join("; ")
        )));
    }
    if plan.get("supported") != Some(&json!(true)) {
        return Err(DfwError::new(format!(
            "dfw cannot edit this project's bundler config automatically: {}",
            text_of(&plan, "manual")
        )));
    }
    let mut written = Vec::new();
    if plan.get("alreadyConfigured") != Some(&json!(true)) {
        let config = text_of(&plan, "configFile");
        let file = root.join(config);
        let mut backup = file.clone().into_os_string();
        backup.push(".dfw-backup");
        ensure_inside(root, Path::new(&backup))?;
        ensure_inside(root, &file)?;
        if !Path::new(&backup).exists() {
            fs::copy(&file, &backup)?; // the user's original, kept once
        }
        write_atomic_inside(root, &file, text_of(&plan, "patched"))?;
        written.push(config.to_string());
    }
    let pkg_file = root.join("package.json");
    let raw = fs::read_to_string(&pkg_file)?;
    let mut pkg: Value = serde_json::from_str(&raw)?;
    let pinned = pkg
        .get("devDependencies")
        .and_then(|d| d.get(PACKAGE))
        .and_then(Value::as_str)
        == Some(VERSION);
    if let (false, Some(top)) = (pinned, pkg.as_object_mut()) {
        let mut dev = js::spread(top.get("devDependencies"));
        dev.insert(PACKAGE.into(), json!(VERSION));
        top.insert("devDependencies".into(), Value::Object(dev));
        // The file's own indentation: the first indented line that starts a key.
        let indent = re(r#"(?m)^([ \t]+)""#)
            .and_then(|r| r.captures(&raw))
            .and_then(|m| m.get(1))
            .map_or("  ", |m| m.as_str());
        let newline = if raw.ends_with('\n') { "\n" } else { "" };
        write_atomic_inside(root, &pkg_file, to_json_indented(&pkg, indent) + newline)?;
        written.push("package.json".to_string());
    }
    Ok((plan, written))
}

#[cfg(test)]
mod tests {
    //! tests/unit/inspector.test.mjs (the inspector cases; the review proxy cases belong to
    //! studio/live_proxy) and tests/unit/inspector-shapes.test.mjs.
    use super::*;
    use crate::core::util::{copy_dir, test_dir, write_json};
    use std::path::PathBuf;

    const IMPORT: &str = "import { codeInspectorPlugin } from 'code-inspector-plugin';\n";

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../tests/fixtures/inspector")
            .join(name)
    }

    fn copy_fixture(name: &str) -> PathBuf {
        let dir = test_dir(&format!("insp-{name}"));
        copy_dir(&fixture(name), &dir, &[]).unwrap();
        dir
    }

    fn read(dir: &Path, file: &str) -> String {
        fs::read_to_string(dir.join(file)).unwrap()
    }

    fn patched(text: &str, bundler: &str, file: &str, mode: Option<&str>) -> (String, bool) {
        match patch_config(text, Some(bundler), file, mode) {
            Patch::Text { text, changed } => (text, changed),
            Patch::Manual(why) => panic!("manual: {why}"),
        }
    }

    fn vite(src: &str) -> String {
        patched(src, "vite", "vite.config.ts", None).0
    }

    fn is_manual(text: &str, bundler: &str, file: &str, mode: Option<&str>) -> bool {
        matches!(
            patch_config(text, Some(bundler), file, mode),
            Patch::Manual(_)
        )
    }

    fn matches(pattern: &str, text: &str) -> bool {
        Regex::new(pattern).unwrap().is_match(text)
    }

    /// G3 approved and the given stack, as `saveState` + `approveGate` + `writeJson` leave them.
    fn approve_g3(root: &Path, stack: Value) {
        write_json(
            &root.join(".dfw/state.json"),
            &json!({ "schema": 1, "mode": "existing-project", "phase": "implement", "gates": { "G1": null, "DS": null, "G2": null, "G3": { "at": "2026-01-02T03:04:05.678Z", "note": "ok" }, "G4": null }, "waived": {} }),
        )
        .unwrap();
        write_json(&root.join(".dfw/stack.json"), &stack).unwrap();
    }

    /// parseInspPath: file:line:col:tag, Windows paths, absolute paths, missing attribute
    #[test]
    fn parse_insp_path_cases() {
        let p = parse_insp_path("src/components/Foo.tsx:42:7:div", None).unwrap();
        assert_eq!(
            p,
            InspPath {
                file: "src/components/Foo.tsx".into(),
                line: 42.0,
                column: Some(7.0),
                tag: Some("div".into()),
                source: "src/components/Foo.tsx:42:7".into()
            }
        );
        let source = |v: &str, root: Option<&str>| parse_insp_path(v, root).unwrap().source;
        assert_eq!(source("src/App.vue:3:5:el-button", None), "src/App.vue:3:5");
        assert_eq!(source("src/App.tsx:3:5", None), "src/App.tsx:3:5", "no tag");
        assert_eq!(
            source("src/App.tsx:3:Header", None),
            "src/App.tsx:3",
            "no column"
        );
        assert_eq!(
            parse_insp_path("./src/a.jsx:1:1:p", None).unwrap().file,
            "src/a.jsx"
        );
        let win =
            parse_insp_path("C:\\work\\shop\\src\\components\\Foo.tsx:42:7:div", None).unwrap();
        assert_eq!(win.file, "C:/work/shop/src/components/Foo.tsx");
        assert_eq!(win.line, 42.0);
        assert_eq!(
            source(
                "C:\\work\\shop\\src\\components\\Foo.tsx:42:7:div",
                Some("C:\\work\\shop")
            ),
            "src/components/Foo.tsx:42:7",
            "Windows absolute -> project-relative"
        );
        assert_eq!(
            source("C:/work/shop/src/Foo.tsx:9:1:span", Some("C:\\work\\shop")),
            "src/Foo.tsx:9:1"
        );
        assert_eq!(
            source("c:/WORK/shop/src/Foo.tsx:9:1:span", Some("C:\\work\\shop")),
            "src/Foo.tsx:9:1"
        );
        assert_eq!(
            source("D:/work/shop/src/Foo.tsx:9:1:span", Some("C:\\work\\shop")),
            "D:/work/shop/src/Foo.tsx:9:1"
        );
        assert_eq!(
            source("/home/me/shop/src/Foo.tsx:42:7:div", Some("/home/me/shop")),
            "src/Foo.tsx:42:7",
            "POSIX absolute -> relative"
        );
        assert_eq!(
            source("/elsewhere/lib/x.tsx:1:2:a", Some("/home/me/shop")),
            "/elsewhere/lib/x.tsx:1:2",
            "outside the project stays absolute"
        );
        // null, undefined and 42 of the JS test have no counterpart: the argument is text here.
        for bad in ["", "   ", "src/Foo.tsx", "div", ":1:2"] {
            assert_eq!(parse_insp_path(bad, None), None, "{bad:?}");
        }
    }

    /// detectBundler: vite, webpack, next (webpack vs turbopack), nuxt
    #[test]
    fn detect_bundler_cases() {
        let b = detect_bundler(&fixture("vite")).unwrap();
        assert_eq!(
            (b.bundler, b.config_file),
            (Some("vite"), Some("vite.config.ts"))
        );
        assert_eq!(
            detect_bundler(&fixture("webpack")).unwrap().bundler,
            Some("webpack")
        );
        let n14 = detect_bundler(&fixture("next-webpack")).unwrap();
        assert_eq!(
            (n14.bundler, n14.next_mode, n14.config_file),
            (Some("next"), Some("webpack"), Some("next.config.js"))
        );
        assert_eq!(
            detect_bundler(&fixture("next16")).unwrap().next_mode,
            Some("turbopack"),
            "Next 16 dev uses Turbopack"
        );
        assert_eq!(
            detect_bundler(&fixture("nuxt")).unwrap().bundler,
            Some("nuxt")
        );
    }

    #[test]
    fn next_mode_follows_the_version_and_the_dev_script() {
        let mode = |next: &str, dev: &str| {
            let dir = test_dir("insp-next");
            write_json(
                &dir.join("package.json"),
                &json!({ "scripts": { "dev": dev }, "dependencies": { "next": next } }),
            )
            .unwrap();
            detect_bundler(&dir).unwrap().next_mode.unwrap()
        };
        assert_eq!(mode("^15.3.0", "next dev --turbopack"), "turbopack");
        assert_eq!(mode("15.2.4", "next dev --turbo"), "turbo-experimental");
        assert_eq!(mode("15.4.0", "next dev"), "webpack");
        assert_eq!(mode("16.0.0", "next dev --webpack"), "webpack");
        assert_eq!(mode("latest", "next dev --turbopack"), "turbo-experimental");
        // No dependency hint: the config file present decides.
        let dir = test_dir("insp-files");
        fs::write(dir.join("rspack.config.ts"), "").unwrap();
        let b = detect_bundler(&dir).unwrap();
        assert_eq!(
            (b.bundler, b.config_file, b.package_json),
            (Some("rspack"), Some("rspack.config.ts"), false)
        );
    }

    /// patchConfig: vite adds the import and the plugin to the top-level plugins list only
    #[test]
    fn patch_vite() {
        let src = read(&fixture("vite"), "vite.config.ts");
        let (text, changed) = patched(&src, "vite", "vite.config.ts", None);
        assert!(changed);
        assert!(text.contains("import react from '@vitejs/plugin-react';\nimport { codeInspectorPlugin } from 'code-inspector-plugin';"));
        assert!(text.contains("\n  plugins: [codeInspectorPlugin({ bundler: 'vite', pathType: 'relative', hotKeys: false, showSwitch: false, hideConsole: true, server: 'close' }), react()],\n"));
        assert!(
            text.contains("rollupOptions: { plugins: [] }"),
            "nested plugin lists untouched"
        );
        assert_eq!(
            patch_config(&text, Some("vite"), "vite.config.ts", None),
            Patch::Text {
                text: text.clone(),
                changed: false
            },
            "idempotent"
        );
        let (func, _) = patched(
            &read(&fixture("vite-noplugins"), "vite.config.js"),
            "vite",
            "vite.config.js",
            None,
        );
        assert!(
            func.contains(
                "defineConfig(({ mode }) => ({\n  plugins: [codeInspectorPlugin({ bundler: 'vite'"
            ),
            "function config without plugins gets a plugins list"
        );
    }

    /// patchConfig: webpack (CommonJS) and Next.js (webpack dev-only hook, Turbopack rules)
    #[test]
    fn patch_webpack_and_next() {
        let (wp, _) = patched(
            &read(&fixture("webpack"), "webpack.config.js"),
            "webpack",
            "webpack.config.js",
            None,
        );
        assert!(wp.starts_with(
            "'use strict';\nconst { codeInspectorPlugin } = require('code-inspector-plugin');"
        ));
        assert!(matches(
            r"plugins: \[codeInspectorPlugin\(\{ bundler: 'webpack'.*\}\), new HtmlWebpackPlugin\(\)\]",
            &wp
        ));

        let (n14, _) = patched(
            &read(&fixture("next-webpack"), "next.config.js"),
            "next",
            "next.config.js",
            Some("webpack"),
        );
        assert!(n14.contains("require('code-inspector-plugin')"));
        assert!(
            n14.contains("webpack: (config, { dev }) => {\n    if (dev) config.plugins.push(codeInspectorPlugin({ bundler: 'webpack'"),
            "only in dev"
        );
        assert!(n14.contains("reactStrictMode: true"));

        let (n16, _) = patched(
            &read(&fixture("next16"), "next.config.ts"),
            "next",
            "next.config.ts",
            Some("turbopack"),
        );
        assert!(n16.contains("import type { NextConfig } from 'next';\nimport { codeInspectorPlugin } from 'code-inspector-plugin';"));
        assert!(n16.contains("const nextConfig: NextConfig = {\n  turbopack: { rules: codeInspectorPlugin({ bundler: 'turbopack'"));

        assert!(
            is_manual(
                "module.exports = { webpack: (c) => c };",
                "next",
                "",
                Some("webpack")
            ),
            "existing webpack hook: manual"
        );
        assert!(is_manual(
            "export default defineNuxtConfig({})",
            "nuxt",
            "",
            None
        ));
        assert!(
            is_manual("const x = 1;", "vite", "vite.config.js", None),
            "unknown shape: manual, no guess"
        );
        assert!(is_manual(
            "export default { turbopack: {} }",
            "next",
            "",
            Some("turbopack")
        ));
        assert!(is_manual(
            "export default {}",
            "next",
            "",
            Some("turbo-experimental")
        ));
        assert_eq!(
            patch_config("x", None, "", None),
            Patch::Manual("dfw does not edit this config files automatically.".into())
        );
    }

    /// inspectorPlan: pinned dev dependency; unsupported bundlers get manual instructions
    #[test]
    fn plan_cases() {
        let p = inspector_plan(&fixture("vite")).unwrap();
        assert_eq!(p["supported"], json!(true));
        assert_eq!(p["dependency"]["name"], json!("code-inspector-plugin"));
        assert!(matches(r"^\d+\.\d+\.\d+$", VERSION), "exact version");
        assert_eq!(p["dependency"]["dev"], json!(true));
        assert_eq!(p["framework"], json!("React"));
        let keys: Vec<&String> = p.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "bundler",
                "configFile",
                "packageJson",
                "dependency",
                "framework",
                "packageManager",
                "install",
                "supported",
                "patched",
                "alreadyConfigured",
                "devDependency"
            ]
        );
        let nuxt = inspector_plan(&fixture("nuxt")).unwrap();
        assert_eq!(nuxt["supported"], json!(false));
        assert!(text_of(&nuxt, "manual").contains("vite: { plugins: [codeInspectorPlugin"));
        let empty = inspector_plan(&test_dir("insp-empty")).unwrap();
        assert_eq!(empty["supported"], json!(false));
        assert_eq!(empty.get("devDependency"), None);
        assert!(format_plan(&empty).contains("bundler: unknown\n1. devDependency: \"code-inspector-plugin\": \"2.0.9\"  then npm install\n2. by hand: No package.json"));
        let next = inspector_plan(&fixture("next16")).unwrap();
        assert_eq!(
            (&next["nextVersion"], &next["nextMode"]),
            (&json!("16.0.1"), &json!("turbopack"))
        );
        assert!(format_plan(&next).contains("bundler: next (turbopack), config: next.config.ts\n"));
    }

    /// inspector setup --write: refused without G3 or without the dependency in the stack, applied after approval
    #[test]
    fn setup_write_needs_g3_and_the_dependency() {
        let root = copy_fixture("vite");
        let before = read(&root, "vite.config.ts");
        let err = inspector_setup(&root, true).unwrap_err();
        assert!(
            matches(r"refused: G3 .*not approved", err.message()),
            "no workflow state: {err}"
        );
        approve_g3(
            &root,
            json!({ "kind": "javascript", "framework": { "name": "React" }, "dependencies": [] }),
        );
        let err = inspector_setup(&root, true).unwrap_err();
        assert!(
            err.message()
                .contains("does not list \"code-inspector-plugin\"")
                && !err.message().contains("G3 (technology"),
            "G3 approved but the dependency was not: {err}"
        );
        assert_eq!(
            read(&root, "vite.config.ts"),
            before,
            "nothing written when refused"
        );
        assert_eq!(
            inspector_setup(&root, false).unwrap().1.len(),
            0,
            "dry run never writes"
        );

        approve_g3(
            &root,
            json!({ "kind": "javascript", "dependencies": [{ "name": "code-inspector-plugin", "version": VERSION, "dev": true }] }),
        );
        let (_, mut written) = inspector_setup(&root, true).unwrap();
        written.sort();
        assert_eq!(written, ["package.json", "vite.config.ts"]);
        assert!(read(&root, "vite.config.ts").contains("codeInspectorPlugin({ bundler: 'vite'"));
        let raw = read(&root, "package.json");
        let pkg: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(
            pkg["devDependencies"]["code-inspector-plugin"],
            json!(VERSION),
            "pinned, devDependencies"
        );
        assert_eq!(
            pkg["dependencies"].get("code-inspector-plugin"),
            None,
            "never a runtime dependency"
        );
        assert!(
            raw.ends_with("    \"code-inspector-plugin\": \"2.0.9\"\n  }\n}\n"),
            "{raw}"
        );
        assert_eq!(
            inspector_setup(&root, true).unwrap().1,
            Vec::<String>::new(),
            "second run: nothing to do"
        );

        assert!(stack_approves(
            &json!({ "devDependencies": { "code-inspector-plugin": "2.0.9" } })
        ));
        assert!(stack_approves(
            &json!({ "dependencies": ["code-inspector-plugin@2.0.9"] })
        ));
        assert!(!stack_approves(
            &json!({ "dependencies": [{ "name": "react" }] })
        ));
        assert!(!stack_approves(&Value::Null));
    }

    #[test]
    fn package_json_keeps_its_indentation_and_final_newline() {
        let root = copy_fixture("vite");
        fs::write(
            root.join("package.json"),
            "{\n\t\"name\": \"x\",\n\t\"devDependencies\": {\n\t\t\"vite\": \"7.1.0\"\n\t}\n}",
        )
        .unwrap();
        approve_g3(
            &root,
            json!({ "devDependencies": { "code-inspector-plugin": "2.0.9" } }),
        );
        inspector_setup(&root, true).unwrap();
        assert_eq!(
            read(&root, "package.json"),
            "{\n\t\"name\": \"x\",\n\t\"devDependencies\": {\n\t\t\"vite\": \"7.1.0\",\n\t\t\"code-inspector-plugin\": \"2.0.9\"\n\t}\n}"
        );
    }

    /// patchConfig: an import with a trailing comment keeps the new import at the top of the file
    #[test]
    fn import_after_a_trailing_comment() {
        let src = "import { defineConfig } from 'vite'\nimport react from '@vitejs/plugin-react' // fast refresh\n\nexport default defineConfig({\n  plugins: [react()],\n  build: {\n    outDir: 'dist'\n  },\n})\n";
        let text = vite(src);
        assert!(text.contains(&format!("import react from '@vitejs/plugin-react' // fast refresh\n{IMPORT}\nexport default")), "{text}");
        assert!(
            text.contains("    outDir: 'dist'\n  },\n"),
            "nothing inserted inside the config object"
        );
        // Multi-line import, then a side-effect import: after the last one.
        let multi = vite("import {\n  defineConfig,\n  loadEnv,\n} from 'vite';\nimport './env';\nexport default defineConfig({ plugins: [] });\n");
        assert!(
            multi.contains(&format!("import './env';\n{IMPORT}export default")),
            "{multi}"
        );
    }

    /// patchConfig: a commented-out plugins list is not the plugin list
    #[test]
    fn commented_out_plugins_list() {
        let text = vite("import { defineConfig } from 'vite';\nexport default defineConfig({\n  // plugins: [legacy()],\n  /* plugins: [old()], */\n  plugins: [react()],\n});\n");
        assert!(matches(
            r"(?m)^ {2}plugins: \[codeInspectorPlugin\(\{ bundler: 'vite'.*\}\), react\(\)\],$",
            &text
        ));
        assert!(
            text.contains("  // plugins: [legacy()],\n  /* plugins: [old()], */\n"),
            "comments untouched"
        );
    }

    /// patchConfig: nested plugin lists (postcss, resolve) are left alone; the config gets its own list
    #[test]
    fn nested_plugin_lists() {
        let css = vite("import { defineConfig } from 'vite';\nexport default defineConfig({\n  css: { postcss: { plugins: [autoprefixer()] } },\n});\n");
        assert!(
            css.contains("css: { postcss: { plugins: [autoprefixer()] } },"),
            "{css}"
        );
        assert!(css.contains("defineConfig({\n  plugins: [codeInspectorPlugin({ bundler: 'vite'"));
        let (wp, _) = patched("module.exports = {\n  entry: './src/index.ts',\n  resolve: { plugins: [new TsPaths()] },\n};\n", "webpack", "webpack.config.js", None);
        assert!(
            wp.contains("resolve: { plugins: [new TsPaths()] },"),
            "{wp}"
        );
        assert!(
            wp.contains("module.exports = {\n  plugins: [codeInspectorPlugin({ bundler: 'webpack'")
        );
    }

    /// patchConfig: a config function with a body is not edited (manual), never a statement in the body
    #[test]
    fn config_function_with_a_body_is_manual() {
        let src = "import { defineConfig, loadEnv } from 'vite';\nexport default defineConfig(({ mode }) => {\n  const env = loadEnv(mode, process.cwd());\n  return { server: { port: Number(env.PORT) } };\n});\n";
        assert!(
            is_manual(src, "vite", "vite.config.ts", None),
            "manual instructions"
        );
    }

    /// inspector setup --write keeps the original config as <file>.dfw-backup, once
    #[test]
    fn backup_is_kept_once() {
        let root = copy_fixture("vite");
        let original = read(&root, "vite.config.ts");
        approve_g3(
            &root,
            json!({ "kind": "javascript", "dependencies": [{ "name": PACKAGE, "version": VERSION, "dev": true }] }),
        );
        inspector_setup(&root, true).unwrap();
        assert_eq!(read(&root, "vite.config.ts.dfw-backup"), original);
        // The user reverts by hand and runs setup again: the first backup is not replaced.
        fs::write(
            root.join("vite.config.ts"),
            original.replace("react()", "react(), other()"),
        )
        .unwrap();
        inspector_setup(&root, true).unwrap();
        assert_eq!(read(&root, "vite.config.ts.dfw-backup"), original);
    }

    #[test]
    fn plugin_lists_one_per_line_and_in_strings() {
        let text = vite("export default defineConfig({\n  plugins: [\n    react(),\n  ],\n});\n");
        assert!(text.contains("  plugins: [\n    codeInspectorPlugin({ bundler: 'vite', pathType: 'relative', hotKeys: false, showSwitch: false, hideConsole: true, server: 'close' }),\n    react(),\n  ],"), "{text}");
        // "plugins: [" inside a string is not a list.
        let text =
            vite("export default defineConfig({\n  define: { note: 'plugins: [x]' },\n});\n");
        assert!(
            text.contains("defineConfig({\n  plugins: [codeInspectorPlugin(")
                && text.contains("note: 'plugins: [x]'"),
            "{text}"
        );
        assert_eq!(depth_at("a /* { */ b", 10), Some(0));
        assert_eq!(depth_at("a /* { b", 7), None);
        assert_eq!(depth_at("{ x: '}' , y", 11), Some(1));
    }
}
