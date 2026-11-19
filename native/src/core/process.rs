//! Child processes: how to start one the way Node's `spawn` does (on Windows through cmd.exe when
//! asked), whether a recorded pid is still alive, and whether Node.js is on PATH.
//!
//! std has no kill(2) or OpenProcess, and this crate has no `unsafe`: the system tools (`kill`,
//! `tasklist`, `taskkill`) answer those questions instead.

use std::process::{Command, Stdio};

use serde_json::Value;

/// How to start a program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spawn {
    pub cmd: String,
    pub args: Vec<String>,
    /// On Windows, run `cmd.exe /d /s /c "<cmd> <args...>"` with `cmd` and `args` already quoted
    /// for cmd.exe (Node's `shell: true`). Ignored elsewhere.
    pub shell: bool,
}

impl Spawn {
    /// A program run directly with these arguments.
    pub fn direct(cmd: impl Into<String>, args: Vec<String>) -> Spawn {
        Spawn {
            cmd: cmd.into(),
            args,
            shell: false,
        }
    }
}

/// A `Command` for the plan. Through cmd.exe the command line goes out as written: Rust's own
/// quoting would quote the already quoted arguments again.
pub fn command(plan: &Spawn) -> Command {
    #[cfg(windows)]
    if plan.shell {
        use std::os::windows::process::CommandExt;
        let comspec = std::env::var("ComSpec")
            .ok()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "cmd.exe".into());
        let line = std::iter::once(plan.cmd.as_str())
            .chain(plan.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ");
        let mut c = Command::new(comspec);
        c.raw_arg("/d /s /c").raw_arg(format!("\"{line}\""));
        return c;
    }
    let mut c = Command::new(&plan.cmd);
    c.args(&plan.args);
    c
}

/// An argument for a cmd.exe command line. `always`: wrapped in quotes as it is
/// (`launcherCommand` of locate.mjs, for paths and fixed words). Otherwise quoted, with inner
/// quotes doubled, only when cmd.exe would split or interpret it (`spawnPlan` of install.mjs).
pub fn quote_for_cmd(arg: &str, always: bool) -> String {
    if always {
        return format!("\"{arg}\"");
    }
    let special = |c: char| c.is_whitespace() || "\"&|<>^()%!,;=".contains(c);
    if arg.is_empty() || arg.chars().any(special) {
        format!("\"{}\"", arg.replace('"', "\"\""))
    } else {
        arg.to_string()
    }
}

/// `Number.isInteger(pid) && pid > 0`: a pid that names one process (`1.0` is an integer too).
/// 0 and negative numbers address process groups, so they never count. Nor does a number above
/// `i32::MAX`: `kill` reads it as a negative pid_t, which is a process group again.
pub fn positive_pid(pid: &Value) -> Option<u64> {
    const MAX: u64 = i32::MAX as u64;
    pid.as_u64().filter(|p| (1..=MAX).contains(p)).or_else(|| {
        pid.as_f64()
            .filter(|f| *f >= 1.0 && f.fract() == 0.0 && *f <= MAX as f64)
            .map(|f| f as u64)
    })
}

/// `isAlive(pid)`: whether a process with this id exists and may be signalled.
pub fn is_alive(pid: Option<&Value>) -> bool {
    let Some(pid) = pid.and_then(positive_pid) else {
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

/// `process.kill(pid)`: SIGTERM (a forced end on Windows). Nothing for a pid that is not one.
pub fn kill(pid: Option<&Value>) {
    let Some(pid) = pid.and_then(positive_pid) else {
        return;
    };
    let mut c = if cfg!(windows) {
        let mut c = Command::new("taskkill");
        c.args(["/PID", &pid.to_string(), "/F"]);
        c
    } else {
        let mut c = Command::new("kill");
        c.arg(pid.to_string());
        c
    };
    let _ = c
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

/// `nodeExecutable()` of util.mjs inside the dfw binary: "node" when it runs, else nothing.
pub fn node_executable() -> Option<&'static str> {
    let mut probe = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/d", "/s", "/c", "node", "--version"]);
        c
    } else {
        let mut c = Command::new("node");
        c.arg("--version");
        c
    };
    probe
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
        .then_some("node")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_a_positive_integer_is_a_pid() {
        assert_eq!(positive_pid(&json!(42)), Some(42));
        assert_eq!(positive_pid(&json!(42.0)), Some(42));
        assert_eq!(positive_pid(&json!(2147483647)), Some(2147483647));
        // Above i32::MAX a pid_t wraps to a negative number: a process group.
        for not in [
            json!(0),
            json!(-1),
            json!(1.5),
            json!("42"),
            json!(null),
            json!(2147483648u64),
            json!(4294967295u64),
            json!(u64::MAX),
            json!(4294967295.0),
        ] {
            assert_eq!(positive_pid(&not), None, "{not}");
        }
        assert!(!is_alive(Some(&json!(0))));
        assert!(!is_alive(None));
    }

    #[test]
    fn this_process_is_alive() {
        assert!(is_alive(Some(&json!(std::process::id()))));
    }

    #[test]
    fn cmd_quoting() {
        assert_eq!(quote_for_cmd("plain", false), "plain");
        assert_eq!(quote_for_cmd("a&calc", false), "\"a&calc\"");
        assert_eq!(quote_for_cmd("", false), "\"\"");
        assert_eq!(quote_for_cmd("say \"hi\"", false), "\"say \"\"hi\"\"\"");
        assert_eq!(quote_for_cmd("plain", true), "\"plain\"");
        assert_eq!(quote_for_cmd("a\"b", true), "\"a\"b\"");
    }
}
