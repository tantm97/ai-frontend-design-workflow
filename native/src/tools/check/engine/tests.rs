//! Tests of the engine lookup and the project env filter, ported from
//! tests/unit/impeccable-hardening.test.mjs of the branch fix/impeccable-hardening.

use super::*;
use crate::core::util::test_dir;
use std::time::{Duration, SystemTime};

const ENGINE: &str = "fake engine bytes";

fn write(file: &Path, text: &str) -> PathBuf {
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(file, fs::Permissions::from_mode(0o755)).unwrap();
    }
    file.to_path_buf()
}

struct Skill {
    skill_dir: PathBuf,
    home: PathBuf,
    stamp_file: PathBuf,
}

/// A skill folder pinned at 9.9.9 whose checksum file lists the digest of ENGINE for every
/// platform.
fn skill(root: &Path) -> Skill {
    let skill_dir = root.join("plugin").join("skills").join("impeccable");
    let scripts = skill_dir.join("scripts");
    write(&scripts.join("impeccable"), "#!/bin/sh\nexit 99\n");
    write(&scripts.join("impeccable.cmd"), "@echo off\r\n");
    write(&scripts.join("VERSION"), "9.9.9\n");
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
    write(
        &scripts.join("engine-checksums.txt"),
        &format!(
            "# comment\n9.9.8 impeccable-linux-x64 {}\n{}\n",
            sha256("old"),
            lines.join("\r\n")
        ),
    );
    Skill {
        skill_dir,
        home: root.join("home"),
        stamp_file: root.join("dfwhome").join("engine-verified.json"),
    }
}

fn resolve(
    s: &Skill,
    platform: &str,
    arch: &str,
    env: &[(&str, String)],
    allow_unverified: bool,
) -> Engine {
    resolve_engine(&EngineQuery {
        skill_dir: &s.skill_dir,
        platform,
        arch,
        env: &|name| env.iter().find(|(k, _)| *k == name).map(|(_, v)| v.clone()),
        home: &s.home,
        stamp_file: &s.stamp_file,
        allow_unverified,
    })
}

fn set_mtime(file: &Path, ms: u64) {
    let f = fs::File::options().write(true).open(file).unwrap();
    f.set_modified(SystemTime::UNIX_EPOCH + Duration::from_millis(ms))
        .unwrap();
}

/// engine: nothing runs until the version cache holds a binary with the pinned sha256
/// (tests/unit/impeccable-hardening.test.mjs on fix/impeccable-hardening)
#[test]
fn nothing_runs_until_the_cache_holds_the_pinned_binary() {
    let s = skill(&test_dir("engine-pinned"));
    let cached = s.home.join(".impeccable/bin/9.9.9/impeccable");
    let e = resolve(&s, "linux", "x64", &[], false);
    assert!(!e.ready);
    assert_eq!(e.cmd, None);
    assert!(e.detail.unwrap().contains("dfw check --setup"));

    write(&cached, "tampered");
    let e = resolve(&s, "linux", "x64", &[], false);
    assert!(!e.ready, "a cached file with another digest is not run");
    assert!(e
        .detail
        .unwrap()
        .contains("does not match the pinned checksum"));

    write(&cached, ENGINE);
    let e = resolve(&s, "linux", "x64", &[], false);
    assert_eq!(
        (e.ready, e.verified, e.cmd.as_deref()),
        (true, true, Some(cached.as_path())),
        "dfw runs the engine itself, not the sh launcher"
    );
    assert_eq!(
        e.env,
        vec![
            (
                "IMPECCABLE_SKILL_DIR".to_string(),
                s.skill_dir.to_string_lossy().into_owned()
            ),
            (
                "IMPECCABLE_SELF".to_string(),
                s.skill_dir
                    .join("scripts/impeccable")
                    .to_string_lossy()
                    .into_owned()
            ),
        ]
    );
}

/// engine: the verification is cached by path + size + mtime
#[test]
fn the_verification_is_cached_by_path_size_and_mtime() {
    let s = skill(&test_dir("engine-stamp"));
    let cached = write(&s.home.join(".impeccable/bin/9.9.9/impeccable"), ENGINE);
    let digests = vec![sha256(ENGINE)];
    set_mtime(&cached, 1_700_000_000_000);
    assert!(verify_engine(&cached, &digests, &s.stamp_file));
    let stamps = read_json(&s.stamp_file, None).unwrap();
    assert_eq!(
        stamps[cached.to_string_lossy().as_ref()],
        json!({ "size": ENGINE.len(), "mtimeMs": 1_700_000_000_000_u64, "sha256": sha256(ENGINE) })
    );
    assert!(
        fs::read_to_string(&s.stamp_file)
            .unwrap()
            .contains("\"mtimeMs\": 1700000000000,"),
        "written as Node writes a whole number"
    );
    // Same size and mtime: the stamp answers and the file is not hashed again.
    fs::write(&cached, ENGINE.replace("fake", "FAKE")).unwrap();
    set_mtime(&cached, 1_700_000_000_000);
    assert!(verify_engine(&cached, &digests, &s.stamp_file));
    // A new mtime or size invalidates the stamp, and the changed file fails the hash.
    set_mtime(&cached, 1_700_000_005_000);
    assert!(!verify_engine(&cached, &digests, &s.stamp_file));
    fs::write(&cached, format!("{ENGINE}!")).unwrap();
    assert!(!verify_engine(&cached, &digests, &s.stamp_file));
    // A stamp for a digest that is no longer pinned (skill update) does not count.
    fs::write(&cached, ENGINE).unwrap();
    assert!(verify_engine(&cached, &digests, &s.stamp_file));
    assert!(!verify_engine(
        &cached,
        &[sha256("next release")],
        &s.stamp_file
    ));
    assert!(!verify_engine(&cached, &[], &s.stamp_file));
}

/// engine: unverified locations are used only with the global opt-in
#[test]
fn unverified_locations_need_the_global_opt_in() {
    let root = test_dir("engine-unverified");
    let s = skill(&root);
    let env_bin = write(&root.join("own/impeccable"), "own build");
    let sibling = write(
        &s.skill_dir.join("scripts/bin/linux-x64/impeccable"),
        "sibling",
    );
    let home_bin = write(&s.home.join(".impeccable/bin/impeccable"), "home");
    let path_bin = write(&root.join("pathdir/impeccable"), "path");
    let path = format!(
        "{}:{}",
        root.join("empty").display(),
        root.join("pathdir").display()
    );
    let env = [
        ("IMPECCABLE_BIN", env_bin.to_string_lossy().into_owned()),
        ("PATH", path.clone()),
    ];
    let path_only = [("PATH", path)];

    let e = resolve(&s, "linux", "x64", &env, false);
    assert!(!e.ready);
    assert_eq!(e.cmd, None);
    assert_eq!(
        e.unverified,
        vec![
            env_bin.clone(),
            sibling.clone(),
            home_bin.clone(),
            path_bin.clone()
        ],
        "reported so setup can refuse to start the launcher"
    );

    let cached = write(&s.home.join(".impeccable/bin/9.9.9/impeccable"), ENGINE);
    let e = resolve(&s, "linux", "x64", &env, false);
    assert_eq!(
        (e.cmd.clone(), e.verified),
        (Some(cached.clone()), true),
        "without the opt-in the verified cache wins over IMPECCABLE_BIN"
    );

    let e = resolve(&s, "linux", "x64", &env, true);
    assert_eq!((e.cmd, e.ready, e.verified), (Some(env_bin), true, false));
    let e = resolve(&s, "linux", "x64", &path_only, true);
    assert_eq!(
        e.cmd,
        Some(sibling.clone()),
        "launcher order: sibling build before ~/.impeccable/bin/impeccable"
    );
    for f in [&sibling, &home_bin, &cached] {
        fs::remove_file(f).unwrap();
    }
    let e = resolve(&s, "linux", "x64", &path_only, true);
    assert_eq!(
        (e.cmd, e.verified),
        (Some(path_bin), false),
        "PATH is the last resort"
    );
}

/// engine on Windows: .exe in the cache, the .cmd launcher, and no sh
#[test]
fn windows_uses_exe_the_cmd_launcher_and_no_sh() {
    let s = skill(&test_dir("engine-windows"));
    let dir = s.home.join(".impeccable/bin/9.9.9");
    write(&dir.join("impeccable"), ENGINE);
    let e = resolve(&s, "win32", "x64", &[], false);
    assert!(!e.ready, "the POSIX file name is not the Windows engine");
    assert_eq!(e.cached.file_name().unwrap(), "impeccable.exe");
    assert_eq!(e.launcher.file_name().unwrap(), "impeccable.cmd");

    write(&dir.join("impeccable.exe"), ENGINE);
    let e = resolve(&s, "win32", "x64", &[], false);
    assert_eq!(
        (
            e.ready,
            e.verified,
            e.cmd.as_ref().unwrap().file_name().unwrap().to_str()
        ),
        (true, true, Some("impeccable.exe"))
    );
    assert!(
        resolve(&s, "win32", "arm64", &[], false).ready,
        "Windows on ARM accepts the x64 build, as the upstream launcher does"
    );

    let path = format!(
        "{};{}",
        s.home.join("a").display(),
        s.home.join("b").display()
    );
    let shim = write(&s.home.join("b/impeccable.cmd"), "");
    fs::remove_file(dir.join("impeccable.exe")).unwrap();
    assert_eq!(
        resolve(&s, "win32", "x64", &[("PATH", path)], false).unverified,
        vec![shim],
        "PATH is split on \";\" and runnable extensions are tried"
    );

    let launcher = e.launcher.to_string_lossy().into_owned();
    let args: Vec<String> = ["detect", "--json", "C:\\p\\setup.html"]
        .map(String::from)
        .to_vec();
    let c = launcher_command(
        &launcher,
        &args,
        "win32",
        Some("C:\\Windows\\system32\\cmd.exe"),
    );
    assert_eq!(c.cmd, "C:\\Windows\\system32\\cmd.exe");
    assert_eq!(c.args[..3], ["/d", "/s", "/c"]);
    assert_eq!(
        c.args[3],
        format!("\"\"{launcher}\" \"detect\" \"--json\" \"C:\\p\\setup.html\"\"")
    );
    assert!(c.verbatim);
    assert!(!std::iter::once(&c.cmd)
        .chain(&c.args)
        .any(|x| x == "sh" || x.ends_with("/sh") || x.ends_with("\\sh.exe")));
    assert_eq!(
        launcher_command(&launcher, &args, "win32", None).cmd,
        "cmd.exe"
    );
    assert_eq!(
        launcher_command("/x/impeccable", &["a".to_string()], "linux", None),
        LauncherCommand {
            cmd: "/x/impeccable".into(),
            args: vec!["a".into()],
            verbatim: false
        }
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
}

/// config: project layers cannot set engine or loader variables, or loosen the privacy defaults
#[test]
fn project_layers_cannot_set_engine_or_loader_variables() {
    let root = test_dir("engine-env");
    let global = write(&root.join("home/config.json"), "{}");
    let project = write(
        &root.join(".dfw/config.json"),
        &json!({ "env": {
            "IMPECCABLE_BIN": "/repo/evil",
            "IMPECCABLE_API_URL": "http://evil.example",
            "IMPECCABLE_UPDATE_HOST": "evil.example",
            "IMPECCABLE_HOME": "/repo/cache",
            "DO_NOT_TRACK": "0",
            "IMPECCABLE_NO_UPDATE_CHECK": null,
            "IMPECCABLE_NO_TELEMETRY": "0",
            "PATH": "/repo/bin",
            "NODE_OPTIONS": "--require /repo/x.js",
            "LD_PRELOAD": "/repo/x.so",
            "DYLD_INSERT_LIBRARIES": "/repo/x.dylib",
            "NO_COLOR": "1",
        } })
        .to_string(),
    );
    let local = write(
        &root.join(".dfw/config.local.json"),
        &json!({ "env": { "IMPECCABLE_HOOK_DISABLED": "0", "IMPECCABLE_NO_TELEMETRY": "1" } })
            .to_string(),
    );
    let mut warnings = Vec::new();
    let env = merged_env(&global, &root, &mut |m| warnings.push(m)).unwrap();
    assert_eq!(
        env,
        json!({ "DO_NOT_TRACK": "1", "IMPECCABLE_NO_UPDATE_CHECK": "1", "IMPECCABLE_HOOK_DISABLED": "1", "NO_COLOR": "1", "IMPECCABLE_NO_TELEMETRY": "1" })
    );
    let dropped = [
        "IMPECCABLE_BIN",
        "IMPECCABLE_API_URL",
        "IMPECCABLE_UPDATE_HOST",
        "IMPECCABLE_HOME",
        "DO_NOT_TRACK",
        "IMPECCABLE_NO_UPDATE_CHECK",
        "IMPECCABLE_NO_TELEMETRY",
        "PATH",
        "NODE_OPTIONS",
        "LD_PRELOAD",
        "DYLD_INSERT_LIBRARIES",
    ];
    let count = |key: &str, file: &Path| {
        warnings
            .iter()
            .filter(|w| w.contains(&format!("env.{key} ")) && w.contains(file.to_str().unwrap()))
            .count()
    };
    for k in dropped {
        assert_eq!(count(k, &project), 1, "one warning for {k} naming the file");
    }
    assert_eq!(count("IMPECCABLE_HOOK_DISABLED", &local), 1);
    assert_eq!(
        warnings.len(),
        dropped.len() + 1,
        "allowed keys do not warn"
    );
}

/// config: a project "env": null cannot erase the defaults; the global config may set anything
#[test]
fn a_project_cannot_erase_the_defaults_and_the_global_config_may_set_anything() {
    let root = test_dir("engine-env-null");
    write(&root.join(".dfw/config.json"), "{\"env\":null}");
    let global = write(
        &root.join("home/config.json"),
        &json!({ "env": { "DO_NOT_TRACK": "0", "IMPECCABLE_API_URL": "http://mirror.internal" } })
            .to_string(),
    );
    let mut warnings = Vec::new();
    let env = merged_env(&global, &root, &mut |m| warnings.push(m)).unwrap();
    assert_eq!(env["IMPECCABLE_NO_UPDATE_CHECK"], "1");
    assert_eq!(
        env["DO_NOT_TRACK"], "0",
        "the user may change a default in the global config"
    );
    assert_eq!(env["IMPECCABLE_API_URL"], "http://mirror.internal");
    assert_eq!(warnings.len(), 1);
}

/// config: the unverified-engine opt-in is read from the global file only
#[test]
fn the_unverified_engine_opt_in_is_global_only() {
    let root = test_dir("engine-opt-in");
    let global = write(&root.join("home/config.json"), "{}");
    write(
        &root.join(".dfw/config.json"),
        r#"{"impeccable":{"allowUnverifiedEngine":true}}"#,
    );
    assert!(!allow_unverified_engine(&global).unwrap());
    fs::write(&global, r#"{"impeccable":{"allowUnverifiedEngine":true}}"#).unwrap();
    assert!(allow_unverified_engine(&global).unwrap());
    fs::write(
        &global,
        r#"{"impeccable":{"allowUnverifiedEngine":"true"}}"#,
    )
    .unwrap();
    assert!(
        !allow_unverified_engine(&global).unwrap(),
        "only the boolean true opts in"
    );
}
