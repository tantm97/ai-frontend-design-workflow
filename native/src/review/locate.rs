//! Ports `core/lib/locate.mjs`: locate sibling plugins (Impeccable, Hallmark, Taste reference)
//! and the external tools they ship.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::core::config::{allow_unverified_engine, global_config_path};
use crate::core::error::DfwError;
use crate::core::host::HOSTS;
use crate::core::js;
use crate::core::paths::{claude_dir, global_dir, manifest_path, plugin_root, resolve};
use crate::core::platform::{node_arch, node_platform};
use crate::core::process::{plan_for, Spawn};
use crate::core::util::{read_json, sha256, write_json, MARKETPLACE_NAME};

/// The most recently changed folder directly under `dir`.
fn newest_dir(dir: &Path) -> Option<PathBuf> {
    let mut subs: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    subs.sort_by_key(|sub| std::cmp::Reverse(sub.0));
    subs.into_iter().next().map(|(_, path)| path)
}

/// `pluginRoot(name)`: root directory of a plugin from the dfw marketplace, installed or in the
/// development repository.
pub fn plugin_root_of(name: &str) -> Result<Option<PathBuf>, DfwError> {
    let host_dirs: Vec<PathBuf> = HOSTS
        .iter()
        .flat_map(|h| h.installed_plugin_dirs())
        .collect();
    plugin_root_in(
        name,
        plugin_root().as_deref(),
        &claude_dir(),
        &host_dirs,
        &global_dir(),
    )
}

fn plugin_root_in(
    name: &str,
    own_root: Option<&Path>,
    claude: &Path,
    host_dirs: &[PathBuf],
    global: &Path,
) -> Result<Option<PathBuf>, DfwError> {
    let is_plugin = |dir: &Path| manifest_path(dir).is_some();
    if let Some(own) = own_root {
        // 1. Sibling in the same marketplace checkout (dev mode, or plugin cache layout).
        let sibling = resolve(own.join("..").join(name));
        if is_plugin(&sibling) {
            return Ok(Some(sibling));
        }
        // 1b. Dev checkout: core/ next to plugins/.
        let dev = resolve(own.join("..").join("plugins").join(name));
        if is_plugin(&dev) {
            return Ok(Some(dev));
        }
    }
    // 2. Plugin cache: <claudeDir>/plugins/cache/<marketplace>/<plugin>/<version>/
    let cache = claude
        .join("plugins")
        .join("cache")
        .join(MARKETPLACE_NAME)
        .join(name);
    if let Some(cached) = newest_dir(&cache).filter(|c| is_plugin(c)) {
        return Ok(Some(cached));
    }
    // 2b. Other hosts' install directories (e.g. ~/.copilot/installed-plugins/dfw-marketplace/<plugin>).
    if let Some(p) = host_dirs
        .iter()
        .map(|d| d.join(name))
        .find(|p| is_plugin(p))
    {
        return Ok(Some(p));
    }
    // 3. Marketplace repo recorded in global config.
    let config = read_json(&global.join("config.json"), Some(json!({})))?;
    if let Some(repo) = config
        .get("marketplaceRepo")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        let p = Path::new(repo).join("plugins").join(name);
        if is_plugin(&p) {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

/// The Impeccable engine dfw may run (`resolveEngine` of locate.mjs).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Engine {
    /// The upstream launcher (`impeccable`, `impeccable.cmd` on Windows): used only to download.
    pub launcher: PathBuf,
    /// The engine version the skill pins (first word of `scripts/VERSION`).
    pub version: String,
    /// The version cache: the only place dfw trusts after a checksum.
    pub cached: PathBuf,
    /// Variables the engine runs with, besides the config's `env`.
    pub env: Vec<(String, String)>,
    /// What dfw runs directly (not through the sh launcher); `None` when nothing may run.
    pub cmd: Option<PathBuf>,
    pub ready: bool,
    /// `cmd` matched a pinned SHA-256 (else it runs only through the global opt-in).
    pub verified: bool,
    /// Engines found that dfw cannot verify, in the launcher's order (when not ready).
    pub unverified: Vec<PathBuf>,
    /// Why nothing may run (when not ready).
    pub detail: Option<String>,
}

/// Where and how to look for the engine; [`EngineLookup::current`] fills it from this process.
#[derive(Debug, Clone)]
pub struct EngineLookup {
    pub skill_dir: PathBuf,
    /// Node's names: `darwin`, `linux`, `win32`.
    pub platform: String,
    /// Node's names: `arm64`, `x64`.
    pub arch: String,
    /// The environment the launcher would see (`IMPECCABLE_BIN`, `IMPECCABLE_HOME`, `PATH`).
    pub env: Vec<(String, String)>,
    pub home: PathBuf,
    /// Remembers verified engines by path, size and mtime.
    pub stamp_file: PathBuf,
    /// The global opt-in `impeccable.allowUnverifiedEngine`.
    pub allow_unverified: bool,
}

impl EngineLookup {
    /// This machine, this process's environment and the global opt-in.
    pub fn current(skill_dir: PathBuf) -> Result<EngineLookup, DfwError> {
        Ok(EngineLookup {
            skill_dir,
            platform: node_platform().into(),
            arch: node_arch().into(),
            env: std::env::vars().collect(),
            home: std::env::home_dir().unwrap_or_default(),
            stamp_file: global_dir().join("engine-verified.json"),
            allow_unverified: allow_unverified_engine(&global_config_path())?,
        })
    }

    fn var(&self, name: &str) -> Option<&str> {
        self.env
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }
}

/// `engineAssets(platform, arch)`: release asset names for a platform. Windows on ARM also runs
/// the x64 build, as the upstream launcher does.
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
    if os == "windows" && arch == "arm64" {
        return vec![
            "impeccable-windows-arm64.exe".into(),
            "impeccable-windows-x64.exe".into(),
        ];
    }
    let exe = if os == "windows" { ".exe" } else { "" };
    vec![format!("impeccable-{os}-{arch}{exe}")]
}

fn is_file(f: &Path) -> bool {
    !f.as_os_str().is_empty() && f.is_file()
}

/// `fs.statSync(f).mtimeMs`.
fn mtime_ms(meta: &fs::Metadata) -> Option<f64> {
    let d = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some(d.as_secs() as f64 * 1000.0 + f64::from(d.subsec_nanos()) / 1e6)
}

/// A number as JSON: whole numbers without a fraction, as JavaScript writes them.
fn json_number(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 9e15 {
        Value::from(n as i64)
    } else {
        Value::from(n)
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
    let (size, mtime) = (meta.len(), mtime_ms(&meta));
    // An unreadable stamp file: hash again.
    let mut stamps = match read_json(stamp_file, Some(json!({}))) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    let key = file.to_string_lossy().into_owned();
    if let Some(s) = stamps.get(&key) {
        let same = s.get("size").and_then(Value::as_u64) == Some(size)
            && s.get("mtimeMs").and_then(Value::as_f64) == mtime;
        let pinned = s
            .get("sha256")
            .and_then(Value::as_str)
            .is_some_and(|d| digests.iter().any(|x| x == d));
        if same && pinned {
            return true;
        }
    }
    let Ok(bytes) = fs::read(file) else {
        return false;
    };
    let digest = sha256(&bytes);
    if !digests.contains(&digest) {
        return false;
    }
    stamps.insert(
        key,
        json!({ "size": size, "mtimeMs": mtime.map_or(Value::Null, json_number), "sha256": digest }),
    );
    // A read-only dfw home: verified now, hashed again next time.
    let _ = write_json(stamp_file, &Value::Object(stamps));
    true
}

/// `resolveEngine(opts)`: find the Impeccable engine dfw may run for the skill in `skill_dir`.
/// Only the version cache (`~/.impeccable/bin/<version>/impeccable[.exe]`) is trusted, and only
/// after its SHA-256 matches the vendored `engine-checksums.txt`. The other places the upstream
/// launcher looks (`IMPECCABLE_BIN`, a sibling build, `~/.impeccable/bin/impeccable`, PATH) are
/// never hashed, so they are used only with the global opt-in.
// Over 120 lines: the candidates are tried in one sequence, the upstream launcher's order.
#[allow(clippy::too_many_lines)]
pub fn resolve_engine(o: &EngineLookup) -> Engine {
    let scripts = o.skill_dir.join("scripts");
    let win = o.platform == "win32";
    let exe = if win { ".exe" } else { "" };
    let launcher = scripts.join(if win { "impeccable.cmd" } else { "impeccable" });
    let read = |name: &str| fs::read_to_string(scripts.join(name)).unwrap_or_default();
    // `.trim().split(/\s+/)[0]`: JavaScript's white space, not Rust's.
    let words = |line: &str| -> Vec<String> {
        line.split(js::is_space)
            .filter(|w| !w.is_empty())
            .map(str::to_string)
            .collect()
    };
    let version = words(&read("VERSION"))
        .into_iter()
        .next()
        .unwrap_or_default();
    let assets = engine_assets(&o.platform, &o.arch);
    let digests: Vec<String> = read("engine-checksums.txt")
        .lines()
        .filter_map(|line| {
            let parts = words(line);
            let (v, asset, sum) = (parts.first()?, parts.get(1)?, parts.get(2)?);
            let hex =
                sum.len() == 64 && sum.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
            (!version.is_empty() && *v == version && assets.iter().any(|a| a == asset) && hex)
                .then(|| sum.clone())
        })
        .collect();
    let impeccable_home = o
        .var("IMPECCABLE_HOME")
        .filter(|h| !h.is_empty())
        .map_or_else(|| o.home.join(".impeccable"), PathBuf::from);
    let cached = impeccable_home
        .join("bin")
        .join(if version.is_empty() {
            "unknown"
        } else {
            &version
        })
        .join(format!("impeccable{exe}"));
    // The launcher's unverified candidates, in its order: three before the version cache, PATH after it.
    let sibling = assets.first().map(|a| {
        let platform = a.trim_start_matches("impeccable-").trim_end_matches(".exe");
        scripts
            .join("bin")
            .join(platform)
            .join(format!("impeccable{exe}"))
    });
    let before: Vec<PathBuf> = [
        o.var("IMPECCABLE_BIN").map(PathBuf::from),
        sibling,
        Some(
            o.home
                .join(".impeccable")
                .join("bin")
                .join(format!("impeccable{exe}")),
        ),
    ]
    .into_iter()
    .flatten()
    .filter(|f| is_file(f))
    .collect();
    let search = o.var("PATH").or_else(|| o.var("Path")).unwrap_or_default();
    let exts: &[&str] = if win {
        &[".exe", ".com", ".cmd", ".bat"]
    } else {
        &[""]
    };
    let on_path: Vec<PathBuf> = search
        .split(if win { ';' } else { ':' })
        .filter(|d| !d.is_empty())
        .flat_map(|d| {
            exts.iter()
                .map(move |x| Path::new(d).join(format!("impeccable{x}")))
        })
        .filter(|f| is_file(f))
        .take(1)
        .collect();
    let skill_dir = o.skill_dir.to_string_lossy().into_owned();
    let mut engine = Engine {
        env: vec![
            ("IMPECCABLE_SKILL_DIR".into(), skill_dir),
            (
                "IMPECCABLE_SELF".into(),
                launcher.to_string_lossy().into_owned(),
            ),
        ],
        launcher,
        version,
        cached: cached.clone(),
        cmd: None,
        ready: false,
        verified: false,
        unverified: Vec::new(),
        detail: None,
    };
    let chosen = if o.allow_unverified && !before.is_empty() {
        Some((before[0].clone(), false))
    } else if verify_engine(&cached, &digests, &o.stamp_file) {
        Some((cached.clone(), true))
    } else if o.allow_unverified && !on_path.is_empty() {
        Some((on_path[0].clone(), false))
    } else {
        None
    };
    if let Some((cmd, verified)) = chosen {
        engine.cmd = Some(cmd);
        engine.ready = true;
        engine.verified = verified;
        return engine;
    }
    let setup = "run \"dfw check --setup\" once";
    engine.detail = Some(if digests.is_empty() {
        format!("No pinned Impeccable engine for {}-{}.", o.platform, o.arch)
    } else if is_file(&cached) {
        format!(
            "Engine at {} does not match the pinned checksum; delete it, then {setup}.",
            cached.display()
        )
    } else {
        format!("Engine not downloaded yet; {setup}.")
    });
    engine.unverified = before.into_iter().chain(on_path).collect();
    engine
}

/// `launcherCommand(launcher, args, { platform })`: how to start the upstream launcher (only to
/// download the engine). No sh on Windows: the `.cmd` launcher goes through cmd.exe with the
/// quoting of [`plan_for`], the one home of Windows spawning (`process-and-io.md` P2).
pub fn launcher_command(launcher: &str, args: &[String], platform: &str) -> Spawn {
    plan_for(launcher, args.to_vec(), platform == "win32")
}

/// `impeccableEngine()`: the engine for the installed Impeccable plugin, or `None` when the
/// plugin (its launcher) is not installed.
pub fn impeccable_engine() -> Result<Option<Engine>, DfwError> {
    let Some(root) = plugin_root_of("impeccable")? else {
        return Ok(None);
    };
    let skill_dir = root.join("skills").join("impeccable");
    let e = resolve_engine(&EngineLookup::current(skill_dir)?);
    Ok(e.launcher.exists().then_some(e))
}

fn script_of(plugin: &str, script: &str) -> Result<Option<PathBuf>, DfwError> {
    Ok(plugin_root_of(plugin)?
        .map(|root| {
            root.join("skills")
                .join(plugin)
                .join("scripts")
                .join(script)
        })
        .filter(|p| p.exists()))
}

/// `sloplintScript()`: Hallmark's sloplint checker, when Hallmark is installed.
pub fn sloplint_script() -> Result<Option<PathBuf>, DfwError> {
    script_of("hallmark", "sloplint.mjs")
}

/// `tasteCopyLintScript()`: the copy checker of the Taste reference, when it is installed.
pub fn taste_copy_lint_script() -> Result<Option<PathBuf>, DfwError> {
    script_of("taste-reference", "copy-lint.mjs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn plugin(dir: &Path) {
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(dir.join(".claude-plugin/plugin.json"), "{}").unwrap();
    }

    #[test]
    fn a_plugin_is_found_next_to_this_one_in_the_dev_checkout_in_the_cache_or_in_the_recorded_repo()
    {
        let tmp = test_dir("locate");
        let (claude, global) = (tmp.join("claude"), tmp.join("global"));
        let hosts = [tmp.join("copilot/installed-plugins")];
        let find =
            |own: &Path| plugin_root_in("impeccable", Some(own), &claude, &hosts, &global).unwrap();
        // Nothing installed.
        let own = tmp.join("market/plugins/dfw");
        assert_eq!(find(&own), None);
        assert_eq!(
            plugin_root_in("impeccable", None, &claude, &hosts, &global).unwrap(),
            None
        );
        // 3. The marketplace repo recorded in the global config.
        let repo = tmp.join("repo");
        plugin(&repo.join("plugins/impeccable"));
        fs::create_dir_all(&global).unwrap();
        fs::write(
            global.join("config.json"),
            json!({ "marketplaceRepo": repo }).to_string(),
        )
        .unwrap();
        assert_eq!(find(&own), Some(repo.join("plugins/impeccable")));
        // 2b. Another host's install folder wins over it.
        plugin(&hosts[0].join("impeccable"));
        assert_eq!(find(&own), Some(hosts[0].join("impeccable")));
        // 2. The plugin cache wins over both: the newest version folder.
        let cache = claude
            .join("plugins/cache")
            .join(MARKETPLACE_NAME)
            .join("impeccable");
        plugin(&cache.join("1.0.0"));
        assert_eq!(find(&own), Some(cache.join("1.0.0")));
        // 1b. A dev checkout (core/ next to plugins/) wins over the cache.
        let core = tmp.join("checkout/core");
        plugin(&tmp.join("checkout/plugins/impeccable"));
        assert_eq!(find(&core), Some(tmp.join("checkout/plugins/impeccable")));
        // 1. A sibling wins over everything.
        plugin(&tmp.join("market/plugins/impeccable"));
        assert_eq!(find(&own), Some(tmp.join("market/plugins/impeccable")));
    }

    const ENGINE: &str = "fake engine bytes";

    /// A skill folder pinned at 9.9.9 whose checksum file lists the digest of ENGINE for every
    /// platform (`skill()` of tests/unit/impeccable-hardening.test.mjs).
    fn skill(root: &Path) -> EngineLookup {
        let skill_dir = root.join("plugin/skills/impeccable");
        let scripts = skill_dir.join("scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("impeccable"), "#!/bin/sh\nexit 99\n").unwrap();
        fs::write(scripts.join("impeccable.cmd"), "@echo off\r\n").unwrap();
        fs::write(scripts.join("VERSION"), "9.9.9\n").unwrap();
        let assets = [
            "impeccable-darwin-arm64",
            "impeccable-darwin-x64",
            "impeccable-linux-x64",
            "impeccable-linux-arm64",
            "impeccable-windows-x64.exe",
        ];
        let lines: Vec<String> = assets
            .iter()
            .map(|a| format!("9.9.9 {a} {}", sha256(ENGINE)))
            .collect();
        fs::write(
            scripts.join("engine-checksums.txt"),
            format!(
                "# comment\n9.9.8 impeccable-linux-x64 {}\n{}\n",
                sha256("old"),
                lines.join("\r\n")
            ),
        )
        .unwrap();
        EngineLookup {
            skill_dir,
            platform: "linux".into(),
            arch: "x64".into(),
            env: Vec::new(),
            home: root.join("home"),
            stamp_file: root.join("dfwhome/engine-verified.json"),
            allow_unverified: false,
        }
    }

    fn put(file: &Path, text: &str) -> PathBuf {
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
        file.to_path_buf()
    }

    /// engine: nothing runs until the version cache holds a binary with the pinned sha256
    #[test]
    fn nothing_runs_until_the_version_cache_holds_a_binary_with_the_pinned_sha256() {
        let o = skill(&test_dir("engine-cache"));
        let cached = o.home.join(".impeccable/bin/9.9.9/impeccable");
        let e = resolve_engine(&o);
        assert!(!e.ready && e.cmd.is_none());
        assert!(e.detail.unwrap().contains("dfw check --setup"));
        put(&cached, "tampered");
        let e = resolve_engine(&o);
        assert!(!e.ready, "a cached file with another digest is not run");
        assert!(e
            .detail
            .unwrap()
            .contains("does not match the pinned checksum"));
        put(&cached, ENGINE);
        let e = resolve_engine(&o);
        assert_eq!(
            (e.ready, e.verified, e.cmd.as_deref()),
            (true, true, Some(cached.as_path())),
            "dfw runs the engine itself, not the sh launcher"
        );
        assert_eq!(
            e.env,
            [
                (
                    "IMPECCABLE_SKILL_DIR".to_string(),
                    o.skill_dir.to_string_lossy().into_owned()
                ),
                (
                    "IMPECCABLE_SELF".to_string(),
                    o.skill_dir
                        .join("scripts/impeccable")
                        .to_string_lossy()
                        .into_owned()
                )
            ]
        );
    }

    /// engine: the verification is cached by path + size + mtime
    #[test]
    fn the_verification_is_cached_by_path_size_and_mtime() {
        let o = skill(&test_dir("engine-stamp"));
        let cached = put(&o.home.join(".impeccable/bin/9.9.9/impeccable"), ENGINE);
        let digests = vec![sha256(ENGINE)];
        let at = |ms: u64| SystemTime::UNIX_EPOCH + std::time::Duration::from_millis(ms);
        let touch = |ms: u64| {
            fs::File::options()
                .write(true)
                .open(&cached)
                .unwrap()
                .set_modified(at(ms))
                .unwrap()
        };
        touch(1_700_000_000_000);
        assert!(verify_engine(&cached, &digests, &o.stamp_file));
        let stamps = read_json(&o.stamp_file, None).unwrap();
        let key = cached.to_string_lossy().into_owned();
        assert_eq!(
            stamps[&key],
            json!({ "size": ENGINE.len(), "mtimeMs": 1_700_000_000_000_i64, "sha256": sha256(ENGINE) })
        );
        // Same size and mtime: the stamp answers and the file is not hashed again.
        fs::write(&cached, ENGINE.replace("fake", "FAKE")).unwrap();
        touch(1_700_000_000_000);
        assert!(verify_engine(&cached, &digests, &o.stamp_file));
        // A new mtime or size invalidates the stamp, and the changed file fails the hash.
        touch(1_700_000_005_000);
        assert!(!verify_engine(&cached, &digests, &o.stamp_file));
        fs::write(&cached, format!("{ENGINE}!")).unwrap();
        assert!(!verify_engine(&cached, &digests, &o.stamp_file));
        // A stamp for a digest that is no longer pinned (skill update) does not count.
        fs::write(&cached, ENGINE).unwrap();
        assert!(verify_engine(&cached, &digests, &o.stamp_file));
        assert!(!verify_engine(
            &cached,
            &[sha256("next release")],
            &o.stamp_file
        ));
        assert!(!verify_engine(&cached, &[], &o.stamp_file));
    }

    /// engine: unverified locations are used only with the global opt-in
    #[test]
    fn unverified_locations_are_used_only_with_the_global_opt_in() {
        let root = test_dir("engine-unverified");
        let o = skill(&root);
        let env_bin = put(&root.join("own/impeccable"), "own build");
        let sibling = put(
            &o.skill_dir.join("scripts/bin/linux-x64/impeccable"),
            "sibling",
        );
        let home_bin = put(&o.home.join(".impeccable/bin/impeccable"), "home");
        let path_bin = put(&root.join("pathdir/impeccable"), "path");
        let path = format!(
            "{}:{}",
            root.join("empty").display(),
            root.join("pathdir").display()
        );
        let with = |env: Vec<(&str, String)>, allow: bool| EngineLookup {
            env: env.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
            allow_unverified: allow,
            ..o.clone()
        };
        let both = || {
            vec![
                ("IMPECCABLE_BIN", env_bin.display().to_string()),
                ("PATH", path.clone()),
            ]
        };
        let e = resolve_engine(&with(both(), false));
        assert!(!e.ready && e.cmd.is_none());
        assert_eq!(
            e.unverified,
            [
                env_bin.clone(),
                sibling.clone(),
                home_bin.clone(),
                path_bin.clone()
            ],
            "reported so setup can refuse to start the launcher"
        );
        let cached = put(&o.home.join(".impeccable/bin/9.9.9/impeccable"), ENGINE);
        let e = resolve_engine(&with(both(), false));
        assert_eq!(
            (e.cmd.as_deref(), e.verified),
            (Some(cached.as_path()), true),
            "without the opt-in the verified cache wins over IMPECCABLE_BIN"
        );
        let e = resolve_engine(&with(both(), true));
        assert_eq!(
            (e.cmd.as_deref(), e.ready, e.verified),
            (Some(env_bin.as_path()), true, false)
        );
        let only_path = || vec![("PATH", path.clone())];
        assert_eq!(
            resolve_engine(&with(only_path(), true)).cmd,
            Some(sibling.clone()),
            "launcher order: sibling build before ~/.impeccable/bin/impeccable"
        );
        for f in [&sibling, &home_bin, &cached] {
            fs::remove_file(f).unwrap();
        }
        let e = resolve_engine(&with(only_path(), true));
        assert_eq!(
            (e.cmd, e.verified),
            (Some(path_bin), false),
            "PATH is the last resort"
        );
    }

    /// engine on Windows: .exe in the cache, the .cmd launcher, and no sh (the lookup part;
    /// launcherCommand belongs to the check runner)
    #[test]
    fn engine_on_windows_exe_in_the_cache_and_the_cmd_launcher() {
        let o = EngineLookup {
            platform: "win32".into(),
            ..skill(&test_dir("engine-win"))
        };
        let dir = o.home.join(".impeccable/bin/9.9.9");
        put(&dir.join("impeccable"), ENGINE);
        let e = resolve_engine(&o);
        assert!(!e.ready, "the POSIX file name is not the Windows engine");
        assert_eq!(e.cached.file_name().unwrap(), "impeccable.exe");
        assert_eq!(e.launcher.file_name().unwrap(), "impeccable.cmd");
        put(&dir.join("impeccable.exe"), ENGINE);
        let e = resolve_engine(&o);
        assert_eq!((e.ready, e.verified), (true, true), "{:?}", e.detail);
        assert_eq!(e.cmd.unwrap().file_name().unwrap(), "impeccable.exe");
        let arm = EngineLookup {
            arch: "arm64".into(),
            ..o.clone()
        };
        assert!(
            resolve_engine(&arm).ready,
            "Windows on ARM accepts the x64 build, as the upstream launcher does"
        );
        let shim = put(&o.home.join("b/impeccable.cmd"), "");
        fs::remove_file(dir.join("impeccable.exe")).unwrap();
        let path = format!(
            "{};{}",
            o.home.join("a").display(),
            o.home.join("b").display()
        );
        let e = resolve_engine(&EngineLookup {
            env: vec![("PATH".into(), path)],
            ..o.clone()
        });
        assert_eq!(
            e.unverified,
            [shim],
            "PATH is split on \";\" and runnable extensions are tried"
        );
        assert_eq!(
            engine_assets("win32", "x64"),
            ["impeccable-windows-x64.exe"]
        );
        assert_eq!(
            engine_assets("darwin", "arm64"),
            ["impeccable-darwin-arm64"]
        );
        assert!(engine_assets("freebsd", "x64").is_empty());
        assert!(engine_assets("linux", "riscv64").is_empty());

        let launcher = e.launcher.to_string_lossy().into_owned();
        let args: Vec<String> = ["detect", "--json", "C:\\p\\setup.html"]
            .map(String::from)
            .to_vec();
        let c = launcher_command(&launcher, &args, "win32");
        assert!(c.shell, "through cmd.exe, the line as written");
        assert_eq!(
            format!("{} {}", c.cmd, c.args.join(" ")),
            format!("\"{launcher}\" detect --json C:\\p\\setup.html")
        );
        assert!(!std::iter::once(&c.cmd)
            .chain(&c.args)
            .any(|x| x == "sh" || x.ends_with("/sh") || x.ends_with("\\sh.exe")));
        assert_eq!(
            launcher_command("/x/impeccable", &["a".to_string()], "linux"),
            Spawn::direct("/x/impeccable", vec!["a".into()])
        );
    }

    #[test]
    fn newest_folder_wins() {
        let dir = test_dir("doctor-newest");
        assert_eq!(newest_dir(&dir.join("none")), None);
        fs::create_dir(dir.join("0.4.0")).unwrap();
        fs::write(dir.join("file"), "").unwrap();
        assert_eq!(newest_dir(&dir), Some(dir.join("0.4.0")));
    }
}
