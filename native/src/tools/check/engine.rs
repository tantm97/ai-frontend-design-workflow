//! Which Impeccable engine dfw may run, and which `env` a project may hand to external tools.
//!
//! This is the behaviour of the pull request "fix/impeccable-hardening" (`locate.mjs`:
//! `engineAssets`, `verifyEngine`, `resolveEngine`, `launcherCommand`, `impeccableEngine`;
//! `config.mjs`: `filterProjectEnv`, `allowUnverifiedEngine`), not of the Node code on the
//! branch this port started from: only a checksum-verified engine runs, dfw starts it directly
//! (not through the sh launcher), and a project config cannot set variables that choose what
//! runs or where it connects.
// TODO(merge): move the engine lookup to review::locate (it replaces `impeccable_launcher`)
// and the env filter into core::config::load_config, as in the Node modules.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use serde_json::{json, Map, Value};

use crate::core::config::{defaults, global_config_path, load_config};
use crate::core::error::DfwError;
use crate::core::paths::{global_dir, project_paths};
use crate::core::util::{deep_merge, read_json, sha256, write_json};
use crate::design::html::is_js_space;
use crate::design::tokens::js;
use crate::review::locate::plugin_root_of;

/// The Impeccable engine of an installed plugin: what to run, or why nothing may run.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Engine {
    /// The upstream launcher script (used only by `dfw check --setup`, to download).
    pub launcher: PathBuf,
    /// The engine version the plugin pins; empty when its VERSION file is missing.
    pub version: String,
    /// Where the version cache holds the engine.
    pub cached: PathBuf,
    /// Variables the engine needs because dfw starts it without the launcher.
    pub env: Vec<(String, String)>,
    /// The executable to run; `None` when not ready.
    pub cmd: Option<PathBuf>,
    pub ready: bool,
    /// The executable matches a pinned SHA-256.
    pub verified: bool,
    /// Engines the upstream launcher would run that dfw cannot verify (set when not ready).
    pub unverified: Vec<PathBuf>,
    /// Why nothing runs, with what to do next.
    pub detail: Option<String>,
}

/// Node's `process.platform` of this build.
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// Node's `process.arch` of this build.
pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        other => other,
    }
}

/// `engineAssets(platform, arch)`: release asset names for a platform. Windows on ARM also
/// runs the x64 build, as the upstream launcher does.
pub fn engine_assets(platform: &str, arch: &str) -> Vec<String> {
    let os = match platform {
        "darwin" => "darwin",
        "linux" => "linux",
        "win32" => "windows",
        _ => return Vec::new(),
    };
    if !matches!(arch, "arm64" | "x64") {
        return Vec::new();
    }
    let exe = if os == "windows" { ".exe" } else { "" };
    if os == "windows" && arch == "arm64" {
        return vec![
            format!("impeccable-windows-arm64{exe}"),
            format!("impeccable-windows-x64{exe}"),
        ];
    }
    vec![format!("impeccable-{os}-{arch}{exe}")]
}

/// A number as JavaScript writes it in JSON: no `.0` on a whole number.
fn json_number(v: f64) -> Value {
    if v.fract() == 0.0 && v.abs() < 9e15 {
        json!(v as i64)
    } else {
        js::num(v)
    }
}

/// `verifyEngine(file, digests, stampFile)`: true when `file` has one of the pinned SHA-256
/// digests. A passing result is remembered in `stamp_file` under path + size + mtime, so a hook
/// does not hash the 19 MB engine on every edit.
pub fn verify_engine(file: &Path, digests: &[String], stamp_file: &Path) -> bool {
    let Ok(meta) = fs::metadata(file) else {
        return false;
    };
    if !meta.is_file() || digests.is_empty() {
        return false;
    }
    // `mtimeMs` as Node computes it, so a stamp written by either implementation serves both.
    let mtime_ms = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0.0, |d| {
            d.as_secs() as f64 * 1000.0 + f64::from(d.subsec_nanos()) / 1e6
        });
    let size = meta.len();
    // An unreadable stamp file: hash again.
    let stamps = match read_json(stamp_file, Some(json!({}))) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let key = file.to_string_lossy().into_owned();
    if let Some(stamp) = stamps.get(&key) {
        let same_file = stamp.get("size").and_then(Value::as_f64) == Some(size as f64)
            && stamp.get("mtimeMs").and_then(Value::as_f64) == Some(mtime_ms);
        let pinned = stamp
            .get("sha256")
            .and_then(Value::as_str)
            .is_some_and(|sum| digests.iter().any(|d| d == sum));
        if same_file && pinned {
            return true;
        }
    }
    let Ok(bytes) = fs::read(file) else {
        return false;
    };
    let sum = sha256(bytes);
    if !digests.contains(&sum) {
        return false;
    }
    let mut stamps = stamps;
    stamps.insert(
        key,
        json!({ "size": size, "mtimeMs": json_number(mtime_ms), "sha256": sum }),
    );
    // A read-only dfw home: verified now, hashed again next time.
    let _ = write_json(stamp_file, &Value::Object(stamps));
    true
}

/// The inputs of [`resolve_engine`]; [`EngineQuery::for_skill`] fills them from this process.
pub struct EngineQuery<'a> {
    pub skill_dir: &'a Path,
    /// `process.platform`: `darwin`, `linux` or `win32`.
    pub platform: &'a str,
    /// `process.arch`: `arm64` or `x64`.
    pub arch: &'a str,
    /// The environment to read `IMPECCABLE_HOME`, `IMPECCABLE_BIN` and `PATH` from.
    pub env: &'a dyn Fn(&str) -> Option<String>,
    pub home: &'a Path,
    pub stamp_file: &'a Path,
    /// Global config `impeccable.allowUnverifiedEngine`.
    pub allow_unverified: bool,
}

fn fields(line: &str) -> Vec<&str> {
    line.split(is_js_space).filter(|f| !f.is_empty()).collect()
}

/// `resolveEngine(...)`: find the Impeccable engine dfw may run for the skill in `skill_dir`.
///
/// Only the version cache (`~/.impeccable/bin/<version>/impeccable[.exe]`) is trusted, and only
/// after its SHA-256 matches the vendored `engine-checksums.txt`. The other places the upstream
/// launcher looks (`IMPECCABLE_BIN`, a sibling build, `~/.impeccable/bin/impeccable`, `PATH`)
/// are never hashed, so they are used only with `allow_unverified`. dfw runs `cmd` directly with
/// `env`; it does not go through the sh launcher.
// Over 120 lines, one function as in locate.mjs: the candidates are tried in the launcher's order.
pub fn resolve_engine(q: &EngineQuery) -> Engine {
    let scripts = q.skill_dir.join("scripts");
    let win = q.platform == "win32";
    let exe = if win { ".exe" } else { "" };
    let launcher = scripts.join(if win { "impeccable.cmd" } else { "impeccable" });
    let read = |name: &str| {
        fs::read(scripts.join(name))
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    let version = fields(&read("VERSION"))
        .first()
        .map(|v| v.to_string())
        .unwrap_or_default();
    let assets = engine_assets(q.platform, q.arch);
    let is_digest =
        |sum: &str| sum.len() == 64 && sum.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
    let digests: Vec<String> = read("engine-checksums.txt")
        .split('\n')
        .map(fields)
        .filter(|f| {
            !version.is_empty()
                && f.first() == Some(&version.as_str())
                && f.get(1)
                    .is_some_and(|asset| assets.iter().any(|a| a == asset))
                && f.get(2).is_some_and(|sum| is_digest(sum))
        })
        .map(|f| f[2].to_string())
        .collect();
    let env = |name: &str| (q.env)(name).filter(|v| !v.is_empty());
    let engine_name = format!("impeccable{exe}");
    let cached = env("IMPECCABLE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| q.home.join(".impeccable"))
        .join("bin")
        .join(if version.is_empty() {
            "unknown"
        } else {
            &version
        })
        .join(&engine_name);
    // The launcher's unverified candidates, in its order: two before the version cache, PATH after it.
    let sibling = assets.first().map(|asset| {
        let build = asset.strip_prefix("impeccable-").unwrap_or(asset);
        let build = build.strip_suffix(".exe").unwrap_or(build);
        scripts.join("bin").join(build).join(&engine_name)
    });
    let before: Vec<PathBuf> = [
        env("IMPECCABLE_BIN").map(PathBuf::from),
        sibling,
        Some(q.home.join(".impeccable").join("bin").join(&engine_name)),
    ]
    .into_iter()
    .flatten()
    .filter(|f| f.is_file())
    .collect();
    let path = (q.env)("PATH")
        .or_else(|| (q.env)("Path"))
        .unwrap_or_default();
    let extensions: &[&str] = if win {
        &[".exe", ".com", ".cmd", ".bat"]
    } else {
        &[""]
    };
    let on_path: Vec<PathBuf> = path
        .split(if win { ';' } else { ':' })
        .filter(|d| !d.is_empty())
        .flat_map(|d| {
            extensions
                .iter()
                .map(move |x| Path::new(d).join(format!("impeccable{x}")))
        })
        .filter(|f| f.is_file())
        .take(1)
        .collect();
    let base = Engine {
        env: vec![
            (
                "IMPECCABLE_SKILL_DIR".into(),
                q.skill_dir.to_string_lossy().into_owned(),
            ),
            (
                "IMPECCABLE_SELF".into(),
                launcher.to_string_lossy().into_owned(),
            ),
        ],
        launcher,
        version,
        cached: cached.clone(),
        ..Engine::default()
    };
    let usable = |cmd: &Path, verified: bool| Engine {
        cmd: Some(cmd.to_path_buf()),
        ready: true,
        verified,
        ..base.clone()
    };
    if q.allow_unverified {
        if let Some(first) = before.first() {
            return usable(first, false);
        }
    }
    if verify_engine(&cached, &digests, q.stamp_file) {
        return usable(&cached, true);
    }
    if q.allow_unverified {
        if let Some(first) = on_path.first() {
            return usable(first, false);
        }
    }
    let setup = "run \"dfw check --setup\" once";
    let detail = if digests.is_empty() {
        format!("No pinned Impeccable engine for {}-{}.", q.platform, q.arch)
    } else if cached.is_file() {
        format!(
            "Engine at {} does not match the pinned checksum; delete it, then {setup}.",
            cached.display()
        )
    } else {
        format!("Engine not downloaded yet; {setup}.")
    };
    Engine {
        unverified: before.into_iter().chain(on_path).collect(),
        detail: Some(detail),
        ..base
    }
}

/// How to start the upstream launcher (used only to download the engine).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherCommand {
    pub cmd: String,
    pub args: Vec<String>,
    /// Windows: the arguments are one command line for cmd.exe and must not be quoted again.
    pub verbatim: bool,
}

/// `launcherCommand(launcher, args, { platform, env })`: no sh on Windows. A .cmd file cannot be
/// spawned directly; cmd.exe needs the whole command line in one quoted string.
pub fn launcher_command(
    launcher: &str,
    args: &[String],
    platform: &str,
    comspec: Option<&str>,
) -> LauncherCommand {
    if platform != "win32" {
        return LauncherCommand {
            cmd: launcher.to_string(),
            args: args.to_vec(),
            verbatim: false,
        };
    }
    let line: Vec<String> = std::iter::once(launcher)
        .chain(args.iter().map(String::as_str))
        .map(|x| format!("\"{x}\""))
        .collect();
    LauncherCommand {
        cmd: comspec
            .filter(|c| !c.is_empty())
            .unwrap_or("cmd.exe")
            .to_string(),
        args: vec![
            "/d".into(),
            "/s".into(),
            "/c".into(),
            format!("\"{}\"", line.join(" ")),
        ],
        verbatim: true,
    }
}

/// `allowUnverifiedEngine(globalFile)`: the opt-in to run an Impeccable engine dfw cannot
/// checksum. Read from the global config only: a project must not grant it. Only the boolean
/// `true` opts in.
pub fn allow_unverified_engine(global_file: &Path) -> Result<bool, DfwError> {
    let config = read_json(global_file, Some(json!({})))?;
    Ok(config
        .get("impeccable")
        .and_then(|i| i.get("allowUnverifiedEngine"))
        == Some(&Value::Bool(true)))
}

/// `impeccableEngine()`: the engine for the installed Impeccable plugin, or `None` when the
/// plugin is not installed.
pub fn impeccable_engine() -> Result<Option<Engine>, DfwError> {
    let Some(root) = plugin_root_of("impeccable")? else {
        return Ok(None);
    };
    let skill_dir = root.join("skills").join("impeccable");
    let home = std::env::home_dir().unwrap_or_default();
    let engine = resolve_engine(&EngineQuery {
        skill_dir: &skill_dir,
        platform: node_platform(),
        arch: node_arch(),
        env: &|name| std::env::var(name).ok(),
        home: &home,
        stamp_file: &global_dir().join("engine-verified.json"),
        allow_unverified: allow_unverified_engine(&global_config_path())?,
    });
    Ok(engine.launcher.exists().then_some(engine))
}

/// Names a project config may set in `env`: none of them can choose what runs or where it
/// connects. An allowlist, not a denylist: loader, proxy and tool-specific variables cannot be
/// enumerated.
pub const PROJECT_ENV_ALLOW: [&str; 5] = ["NO_COLOR", "FORCE_COLOR", "TZ", "LANG", "LC_ALL"];
/// Privacy switches a project config may keep on (value `"1"`), never turn off.
pub const PRIVACY_ENV: [&str; 4] = [
    "DO_NOT_TRACK",
    "IMPECCABLE_NO_UPDATE_CHECK",
    "IMPECCABLE_HOOK_DISABLED",
    "IMPECCABLE_NO_TELEMETRY",
];

/// `filterProjectEnv(layer, file, warn)`: a project layer without the `env` entries it may not
/// set; one warning per dropped key.
pub fn filter_project_env(layer: Value, file: &str, warn: &mut dyn FnMut(String)) -> Value {
    let Value::Object(mut layer) = layer else {
        return layer;
    };
    let Some(given) = layer.get("env") else {
        return Value::Object(layer);
    };
    let mut env = Map::new();
    match given {
        Value::Object(given) => {
            for (k, v) in js::entries(given) {
                let kept = v.as_str().is_some_and(|v| {
                    PROJECT_ENV_ALLOW.contains(&k.as_str())
                        || (PRIVACY_ENV.contains(&k.as_str()) && v == "1")
                });
                if kept {
                    env.insert(k.clone(), v.clone());
                } else {
                    warn(format!("dfw: ignored env.{k} in {file} (a project config cannot set it; use the global config: dfw config set env.{k} <value> --global)"));
                }
            }
        }
        _ => warn(format!(
            "dfw: ignored env in {file} (it must be an object of strings)"
        )),
    }
    layer.insert("env".into(), Value::Object(env));
    Value::Object(layer)
}

/// `config.env` as the hardened `loadConfig` builds it: defaults, the global file (trusted: any
/// variable), then the project and project-local files through [`filter_project_env`].
pub fn merged_env(
    global_file: &Path,
    root: &Path,
    warn: &mut dyn FnMut(String),
) -> Result<Value, DfwError> {
    let p = project_paths(root);
    let read = |file: &Path| read_json(file, Some(json!({})));
    let project_layer = |file: &Path, warn: &mut dyn FnMut(String)| {
        Ok::<Value, DfwError>(filter_project_env(
            read(file)?,
            &file.to_string_lossy(),
            warn,
        ))
    };
    let layers = [
        defaults(),
        read(global_file)?,
        project_layer(&p.config, warn)?,
        project_layer(&p.config_local, warn)?,
    ];
    let mut merged = json!({});
    for layer in layers {
        if let Some(env) = layer.as_object().and_then(|l| l.get("env")) {
            // deepMerge assigns `out['__proto__']`, which sets a prototype instead of a key.
            let env = match env {
                Value::Object(map) => Value::Object(
                    map.iter()
                        .filter(|(k, _)| *k != "__proto__")
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect(),
                ),
                other => other.clone(),
            };
            merged = deep_merge(merged, json!({ "env": env }));
        }
    }
    Ok(merged.get("env").cloned().unwrap_or(Value::Null))
}

/// The configuration the checks run with: `load_config(root)` whose `env` is [`merged_env`].
/// A key a project may not set is reported on stderr and left out.
pub fn check_config(root: &Path) -> Result<Value, DfwError> {
    let mut config = load_config(Some(root))?;
    let env = merged_env(&global_config_path(), root, &mut |message| {
        eprintln!("{message}")
    })?;
    if let Value::Object(map) = &mut config {
        map.insert("env".into(), env);
    }
    Ok(config)
}

#[cfg(test)]
mod tests;
