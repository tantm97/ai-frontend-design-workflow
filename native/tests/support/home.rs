//! The one way a binary test starts `dfw` (§0 row 43, `testing.md` T9): every home a user-global
//! file could be read from or written to points into the test's own folder, and nothing of the
//! session that runs the tests (its project, host or session id) reaches the binary.
//! Each test file includes it with `#[path = "support/home.rs"] mod home;`.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

/// The variables that name a user home or a host's config folder, each with its folder under the
/// test's user home (`""`: the user home itself). `DFW_HOME` is set apart: tests choose where it is.
/// `tests/golden/harness.mjs` moves the same ones (checked below).
pub const HOMES: [(&str, &str); 6] = [
    ("HOME", ""),
    ("USERPROFILE", ""),
    ("XDG_CONFIG_HOME", ".config"),
    ("CLAUDE_CONFIG_DIR", ".claude"),
    ("COPILOT_HOME", ".copilot"),
    ("CODEX_HOME", ".codex"),
];

/// dfw's own home.
pub const DFW_HOME: &str = "DFW_HOME";

/// Variables of the session that runs the tests; a test that needs one sets it again.
pub const SESSION: [&str; 7] = [
    "DFW_HOST",
    "DFW_PROJECT_DIR",
    "CLAUDE_PROJECT_DIR",
    "DFW_NOW",
    "DFW_SESSION_ID",
    "CLAUDE_SESSION_ID",
    "CLAUDE_ENV_FILE",
];

/// `<exe>` run in `cwd`, with every home under `user_home` and dfw's home at `dfw_home`.
pub fn command(exe: impl AsRef<OsStr>, cwd: &Path, user_home: &Path, dfw_home: &Path) -> Command {
    let mut cmd = Command::new(exe);
    cmd.current_dir(cwd);
    for name in SESSION {
        cmd.env_remove(name);
    }
    // The folders exist, as a user's do: on Windows a process started with a USERPROFILE that is
    // missing (PowerShell, which starts the detached telemetry flush) fails to start.
    for (name, sub) in HOMES {
        let dir = user_home.join(sub);
        let _ = std::fs::create_dir_all(&dir);
        cmd.env(name, dir);
    }
    cmd.env(DFW_HOME, dfw_home);
    cmd
}

#[test]
fn the_homes_are_the_ones_the_golden_harness_moves() {
    let harness = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/golden/harness.mjs"),
    )
    .unwrap();
    let start = harness.find("Object.assign(env, {").unwrap();
    let block = &harness[start..start + harness[start..].find("});").unwrap()];
    let moved: Vec<&str> = block
        .lines()
        .filter_map(|l| l.trim().split_once(':').map(|(k, _)| k))
        .filter(|k| k.ends_with("HOME") || k.ends_with("PROFILE") || k.ends_with("CONFIG_DIR"))
        .collect();
    let mut ours: Vec<&str> = HOMES.iter().map(|(n, _)| *n).collect();
    ours.push(DFW_HOME);
    let (mut a, mut b) = (moved.clone(), ours.clone());
    a.sort_unstable();
    b.sort_unstable();
    assert_eq!(
        a, b,
        "tests/golden/harness.mjs and native/tests/support/home.rs move different homes"
    );
}
