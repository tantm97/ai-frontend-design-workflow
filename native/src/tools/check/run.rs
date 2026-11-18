//! Running an external tool for the checks: `run` of check.mjs (`spawnSync` with a timeout).

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use super::engine::LauncherCommand;
use super::Env;

/// What a finished (or killed) tool left behind.
#[derive(Debug, Default)]
pub(super) struct Ran {
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    /// Why the tool did not start.
    pub error: Option<String>,
}

/// `spawnSync(cmd, args, { cwd, env: { ...process.env, ...env }, timeout })` with the output
/// as text. A tool that runs past `timeout_ms` is killed.
pub(super) fn run(
    command: &LauncherCommand,
    cwd: Option<&Path>,
    env: &Env,
    timeout_ms: u64,
) -> Ran {
    let mut child = Command::new(&command.cmd);
    #[cfg(windows)]
    if command.verbatim {
        use std::os::windows::process::CommandExt;
        for arg in &command.args {
            child.raw_arg(arg);
        }
    }
    if !(cfg!(windows) && command.verbatim) {
        child.args(&command.args);
    }
    if let Some(cwd) = cwd {
        child.current_dir(cwd);
    }
    child
        .envs(env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = match child.spawn() {
        Ok(child) => child,
        Err(err) => {
            return Ran {
                error: Some(format!("spawnSync {} {err}", command.cmd)),
                ..Ran::default()
            }
        }
    };
    // A grandchild can keep a pipe open after the tool is gone: the readers fill shared
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
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                timed_out = true;
                break;
            }
            _ => break,
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
        stdout: text(&out),
        stderr: text(&err),
        timed_out,
        error: None,
    }
}

pub(super) fn plain(cmd: impl Into<String>, args: Vec<String>) -> LauncherCommand {
    LauncherCommand {
        cmd: cmd.into(),
        args,
        verbatim: false,
    }
}

/// `nodeExecutable()` of util.mjs inside the dfw binary: "node" when it runs, else nothing.
// TODO(merge): one helper with tools::lighthouse::node_executable.
pub(super) fn node_executable() -> Option<String> {
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
        .then(|| "node".to_string())
}
