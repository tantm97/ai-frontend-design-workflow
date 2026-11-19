//! Child processes: how to start one the way Node's `spawn` does (on Windows through cmd.exe when
//! asked), how to run one with a deadline, whether a recorded pid is still alive, and whether
//! Node.js is on PATH.
//!
//! std has no kill(2) or OpenProcess, and this crate has no `unsafe`: the system tools (`kill`,
//! `tasklist`, `taskkill`) answer those questions instead.

use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

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

/// What a finished (or killed) program left behind.
#[derive(Debug, Default)]
pub struct Ran {
    /// The exit code; `None` when a signal ended it, it was killed or it did not start.
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// It ran past its deadline and was killed, with everything it started.
    pub timed_out: bool,
    /// Why it did not start: `spawnSync <cmd> <error>`.
    pub error: Option<String>,
}

/// `spawnSync(cmd, args, { cwd, env: { ...process.env, ...env }, timeout })` with the output as
/// text: the one way dfw runs an external program and waits for it. A program that runs past
/// `timeout` is killed together with its children (its process group on Unix, its process tree
/// on Windows), so a browser that `npx` started does not outlive it.
pub fn run_timed(
    plan: &Spawn,
    cwd: Option<&Path>,
    env: &[(String, String)],
    timeout: Duration,
) -> Ran {
    let mut child = command(plan);
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        child.process_group(0);
    }
    child
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(err) => {
            let why = if err.kind() == std::io::ErrorKind::NotFound {
                "ENOENT".to_string()
            } else {
                err.to_string()
            };
            return Ran {
                error: Some(format!("spawnSync {} {why}", plan.cmd)),
                ..Ran::default()
            };
        }
    };
    // A grandchild can keep a pipe open after the program is gone: the readers fill shared
    // buffers and are given a moment, not joined for ever.
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        let text = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&text);
        let reader = thread::spawn(move || {
            let Some(mut pipe) = pipe else { return };
            let mut chunk = [0u8; 8192];
            while let Ok(n) = pipe.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                if let Ok(mut buf) = shared.lock() {
                    buf.extend_from_slice(&chunk[..n]);
                }
            }
        });
        (text, reader)
    };
    let (out, out_reader) = drain(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let (err, err_reader) = drain(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    let mut timed_out = false;
    let mut status = None;
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                kill_tree(&mut child);
                timed_out = true;
                break;
            }
            Ok(Some(s)) => {
                status = s.code();
                break;
            }
            Err(_) => break,
        }
    }
    let settle = Instant::now() + Duration::from_secs(if timed_out { 1 } else { 5 });
    while !(out_reader.is_finished() && err_reader.is_finished()) && Instant::now() < settle {
        thread::sleep(Duration::from_millis(2));
    }
    let text = |buf: &Mutex<Vec<u8>>| {
        buf.lock()
            .map(|b| String::from_utf8_lossy(&b).into_owned())
            .unwrap_or_default()
    };
    Ran {
        status,
        stdout: text(&out),
        stderr: text(&err),
        timed_out,
        error: None,
    }
}

/// End a child that ran past its deadline and everything it started. On Unix the child leads
/// its own process group ([`run_timed`] starts it so), and the group is signalled: the one place
/// dfw sends a signal to a negative pid, a group it made itself (P10 is about pids read from
/// files). On Windows `taskkill /T` ends the process tree.
fn kill_tree(child: &mut Child) {
    let pid = child.id();
    let mut c = if cfg!(windows) {
        let mut c = Command::new("taskkill");
        c.args(["/T", "/F", "/PID", &pid.to_string()]);
        c
    } else {
        let mut c = Command::new("kill");
        c.args(["-KILL", "--", &format!("-{pid}")]);
        c
    };
    let _ = c
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = child.kill();
    let _ = child.wait();
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

    /// A program past its deadline is killed with what it started: the orphaned `sleep` holds
    /// stdout open and must neither delay the result nor survive it.
    #[test]
    #[cfg(unix)]
    fn a_timed_out_program_is_killed_with_its_children() {
        let plan = Spawn::direct("sh", vec!["-c".into(), "sleep 30 & echo $!; wait".into()]);
        let started = Instant::now();
        let ran = run_timed(&plan, None, &[], Duration::from_millis(300));
        assert!(ran.timed_out && ran.status.is_none(), "{ran:?}");
        assert!(started.elapsed() < Duration::from_secs(3));
        let grandchild: u64 = ran.stdout.trim().parse().unwrap();
        let gone = Instant::now() + Duration::from_secs(2);
        while is_alive(Some(&json!(grandchild))) && Instant::now() < gone {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            !is_alive(Some(&json!(grandchild))),
            "sleep {grandchild} outlived the timeout"
        );
    }

    #[test]
    fn a_finished_program_gives_its_code_and_output() {
        let ran = run_timed(
            &Spawn::direct("dfw-no-such-program", Vec::new()),
            None,
            &[],
            Duration::from_secs(5),
        );
        assert_eq!(
            ran.error.as_deref(),
            Some("spawnSync dfw-no-such-program ENOENT")
        );
        if cfg!(unix) {
            let plan = Spawn::direct(
                "sh",
                vec!["-c".into(), "echo out; echo $X >&2; exit 3".into()],
            );
            let env = [("X".to_string(), "err".to_string())];
            let ran = run_timed(&plan, None, &env, Duration::from_secs(5));
            assert_eq!(
                (
                    ran.status,
                    ran.stdout.as_str(),
                    ran.stderr.as_str(),
                    ran.timed_out
                ),
                (Some(3), "out\n", "err\n", false)
            );
        }
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
