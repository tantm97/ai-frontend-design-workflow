//! Child processes: how to start one the way Node's `spawn` does (on Windows through cmd.exe when
//! asked), how to run one with a deadline, whether a recorded pid is still alive, and whether
//! Node.js is on PATH.
//!
//! std has no kill(2) or OpenProcess, and this crate has no `unsafe`: the system tools (`kill`,
//! `tasklist`, `taskkill`) answer those questions instead.

#![allow(
    clippy::disallowed_methods,
    reason = "core::process is the one home of Command::new (clippy.toml disallowed-methods)"
)]

use std::io::Read;
use std::path::{Path, PathBuf};
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

/// An argument for a cmd.exe command line. `always`: wrapped in quotes as it is (the
/// executable's path in [`plan_for`]). Otherwise quoted, with inner
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

/// Whether a Windows path names its drive or share (`C:\\x`, `\\\\server\\x`): it does not depend
/// on the current folder or the current drive.
pub fn win32_fully_qualified(p: &str) -> bool {
    let b = p.as_bytes();
    let sep = |c: Option<&u8>| matches!(c, Some(b'/') | Some(b'\\'));
    (sep(b.first()) && sep(b.get(1)))
        || (b.len() > 2 && b[0].is_ascii_alphabetic() && b[1] == b':' && sep(b.get(2)))
}

/// Where a bare program name (`node`, `claude`, `npx`) is found on `path`, as an absolute path.
/// Only absolute PATH entries are searched, and never the current folder: cmd.exe looks there
/// first, so a cloned repository's `node.cmd` or `claude.bat` would run. On Windows each
/// extension of `pathext` is tried (`.COM;.EXE;.BAT;.CMD` when it is not set). `is_program`
/// answers whether a candidate is a runnable file (injected by the tests).
pub fn find_in_path(
    name: &str,
    path: &str,
    pathext: Option<&str>,
    windows: bool,
    is_program: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
    if name.is_empty() || name.contains(separators) || name == "." || name == ".." {
        return None;
    }
    let exts: Vec<String> = if windows {
        let list = pathext
            .filter(|e| !e.trim().is_empty())
            .unwrap_or(".COM;.EXE;.BAT;.CMD");
        let exts: Vec<String> = list
            .split(';')
            .map(str::trim)
            .filter(|e| e.starts_with('.') && e.len() > 1)
            .map(str::to_string)
            .collect();
        let lower = name.to_ascii_lowercase();
        if exts
            .iter()
            .any(|e| lower.ends_with(&e.to_ascii_lowercase()))
        {
            vec![String::new()]
        } else {
            exts
        }
    } else {
        vec![String::new()]
    };
    let list_separator = if windows { ';' } else { ':' };
    for dir in path.split(list_separator) {
        let dir = if windows {
            dir.trim().trim_matches('"')
        } else {
            dir
        };
        let absolute = if windows {
            win32_fully_qualified(dir)
        } else {
            dir.starts_with('/')
        };
        if !absolute {
            continue;
        }
        let dir = dir.trim_end_matches(separators);
        for ext in &exts {
            let candidate = if windows {
                format!("{dir}\\{name}{ext}")
            } else {
                format!("{dir}/{name}")
            };
            if is_program(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

/// Whether a path is a file this user may run.
fn is_program(path: &str) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    meta.is_file()
}

/// The environment variable that lists the folders searched for a program.
pub const PATH_VAR: &str = "PATH";

/// [`find_in_path`] on this process's `PATH` (and `PATHEXT` on Windows).
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = std::env::var(PATH_VAR).unwrap_or_default();
    let pathext = std::env::var("PATHEXT").ok();
    find_in_path(name, &path, pathext.as_deref(), cfg!(windows), &is_program).map(PathBuf::from)
}

/// How to start a program found by [`find_executable`] (or another absolute path). A Windows
/// `.cmd` or `.bat` cannot be started directly: it goes through cmd.exe with its absolute path
/// in quotes and every argument quoted for cmd.exe ([`quote_for_cmd`]). cmd.exe still expands
/// `%NAME%` inside quotes, so arguments from untrusted input do not belong here.
pub fn plan_for(exe: &str, args: Vec<String>, windows: bool) -> Spawn {
    let lower = exe.to_ascii_lowercase();
    if windows && (lower.ends_with(".cmd") || lower.ends_with(".bat")) {
        return Spawn {
            cmd: quote_for_cmd(exe, true),
            args: args.iter().map(|a| quote_for_cmd(a, false)).collect(),
            shell: true,
        };
    }
    Spawn::direct(exe, args)
}

/// `nodeExecutable()` of util.mjs inside the dfw binary: the absolute path of the Node.js on
/// PATH ([`find_executable`]), else nothing. It is not started to ask: a hook pays for every
/// process it starts.
pub fn node_executable() -> Option<PathBuf> {
    find_executable("node")
}

/* ---------------------------------------------------------------- detached processes */

/// Start `exe args` as a process of its own that outlives dfw and that dfw does not wait for
/// (the Studio and review servers, the telemetry flush), its stdout and stderr appended to
/// `log` (else discarded). Returns its pid.
///
/// On Unix: a process group of its own, so closing the terminal does not stop it.
///
/// On Windows every child inherits all of dfw's inheritable handles, among them the pipes of
/// whoever reads dfw's output (an agent's shell, a test): a server started directly kept them
/// open, and the caller waited until the server stopped. Clearing the inherit flag needs FFI,
/// which this crate does not use, so the child is started by PowerShell's `Start-Process`
/// through ShellExecute, which hands down no handles. With a log, cmd.exe (started the same way)
/// writes the output to it; the pid is the program's own, found as cmd.exe's child. PowerShell
/// exits right away; dfw waits only for it (`process-and-io.md` P15).
pub fn spawn_detached(
    exe: &Path,
    args: &[String],
    log: Option<&Path>,
) -> Result<u32, crate::core::error::DfwError> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let mut cmd = Command::new(exe);
        cmd.args(args).stdin(Stdio::null());
        match log {
            Some(log) => {
                let file = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(log)?;
                cmd.stdout(file.try_clone()?).stderr(file);
            }
            None => {
                cmd.stdout(Stdio::null()).stderr(Stdio::null());
            }
        }
        cmd.process_group(0);
        Ok(cmd.spawn()?.id())
    }
    #[cfg(not(unix))]
    {
        spawn_detached_windows(exe, args, log)
    }
}

#[cfg(not(unix))]
fn spawn_detached_windows(
    exe: &Path,
    args: &[String],
    log: Option<&Path>,
) -> Result<u32, crate::core::error::DfwError> {
    use crate::core::error::DfwError;
    let root = ["SystemRoot", "windir"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .find(|dir| win32_fully_qualified(dir))
        .ok_or_else(|| {
            DfwError::new(
                "SystemRoot is not set to an absolute folder, so dfw cannot find PowerShell to start a background process.",
            )
        })?;
    let system = Path::new(&root).join("System32");
    let powershell = system.join(r"WindowsPowerShell\v1.0\powershell.exe");
    if !powershell.is_file() {
        return Err(DfwError::new(format!(
            "PowerShell was not found at {}; dfw starts background processes with it on Windows.",
            powershell.display()
        )));
    }
    let cwd = std::env::current_dir()?;
    let script = detached_script(exe, args, &cwd, log, &system.join("cmd.exe"));
    let utf16: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let plan = Spawn::direct(
        powershell.display().to_string(),
        [
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-EncodedCommand",
        ]
        .iter()
        .map(|a| (*a).to_string())
        .chain([crate::core::encoding::base64_encode(&utf16)])
        .collect(),
    );
    let ran = run_timed(&plan, None, &[], Duration::from_secs(30));
    ran.stdout.trim().parse::<u32>().map_err(|_| {
        DfwError::new(format!(
            "PowerShell could not start {}: {}",
            exe.display(),
            ran.error.unwrap_or_else(|| ran.stderr.trim().to_string())
        ))
    })
}

/// A PowerShell string literal: single quotes, each single quote doubled (PowerShell also reads
/// the typographic single quotes as quotes).
// Used on Windows only; its tests run everywhere.
#[cfg_attr(
    unix,
    allow(dead_code, reason = "Used on Windows only; its tests run everywhere")
)]
pub fn quote_for_powershell(text: &str) -> String {
    let mut out = String::from("'");
    for c in text.chars() {
        if matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}') {
            out.push(c);
        }
        out.push(c);
    }
    out.push('\'');
    out
}

/// One argument of a Windows command line, always in quotes, read back as written by the C
/// runtime's rules (and Rust's): backslashes before a quote are doubled and a quote is written
/// `""`, which also keeps cmd.exe's own quote state balanced, so `&`, `|`, `<`, `>` and `^`
/// stay inside the quotes for it. cmd.exe still expands a `%NAME%` that names a variable.
// Used on Windows only; its tests run everywhere.
#[cfg_attr(
    unix,
    allow(dead_code, reason = "Used on Windows only; its tests run everywhere")
)]
pub fn quote_windows_arg(arg: &str) -> String {
    let mut out = String::from("\"");
    let mut backslashes = 0;
    for c in arg.chars() {
        match c {
            '\\' => backslashes += 1,
            '"' => {
                out.push_str(&"\\".repeat(backslashes * 2));
                out.push_str("\"\"");
                backslashes = 0;
            }
            _ => {
                out.push_str(&"\\".repeat(backslashes));
                out.push(c);
                backslashes = 0;
            }
        }
    }
    out.push_str(&"\\".repeat(backslashes * 2));
    out.push('"');
    out
}

/// The PowerShell script of [`spawn_detached`] on Windows: `Start-Process` (ShellExecute: no
/// handle inheritance, the environment of this process), hidden, and the pid it prints.
// Used on Windows only; its tests run everywhere.
#[cfg_attr(
    unix,
    allow(dead_code, reason = "Used on Windows only; its tests run everywhere")
)]
fn detached_script(
    exe: &Path,
    args: &[String],
    cwd: &Path,
    log: Option<&Path>,
    cmd_exe: &Path,
) -> String {
    let arg_line = args
        .iter()
        .map(|a| quote_windows_arg(a))
        .collect::<Vec<_>>()
        .join(" ");
    let program = format!(
        "{} {arg_line}",
        quote_windows_arg(&exe.display().to_string())
    );
    let q = |p: &Path| quote_for_powershell(&p.display().to_string());
    let mut script = String::from("$ErrorActionPreference = 'Stop'\n");
    match log {
        None => {
            script.push_str(&format!(
                "$p = Start-Process -FilePath {} -ArgumentList {} -WorkingDirectory {} -WindowStyle Hidden -PassThru\n[Console]::Out.Write($p.Id)\n",
                q(exe),
                // Start-Process passes the arguments on as written.
                quote_for_powershell(&arg_line),
                q(cwd),
            ));
            if args.is_empty() {
                script = script.replace(" -ArgumentList ''", "");
            }
        }
        Some(log) => {
            let log = quote_windows_arg(&log.display().to_string());
            let line = format!("/d /s /c \"{program} >>{log} 2>&1\"");
            let name = exe
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            // A WQL string: backslash escapes.
            let wql = name.replace('\\', "\\\\").replace('\'', "\\'");
            script.push_str(&format!(
                "$p = Start-Process -FilePath {} -ArgumentList {} -WorkingDirectory {} -WindowStyle Hidden -PassThru\n\
                 $name = {}\n\
                 $id = $p.Id\n\
                 for ($i = 0; $i -lt 400; $i++) {{\n\
                 \x20 $c = Get-CimInstance Win32_Process -Filter \"ParentProcessId = $($p.Id) AND Name = '$name'\" | Select-Object -First 1\n\
                 \x20 if ($c) {{ $id = $c.ProcessId; break }}\n\
                 \x20 if ($p.HasExited) {{ break }}\n\
                 \x20 Start-Sleep -Milliseconds 25\n\
                 }}\n\
                 [Console]::Out.Write($id)\n",
                q(cmd_exe),
                quote_for_powershell(&line),
                q(cwd),
                quote_for_powershell(&wql),
            ));
        }
    }
    script
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

    /// A Windows lookup never takes a program from the current folder or a relative PATH entry,
    /// tries PATHEXT in order, and runs a .cmd through cmd.exe with every part quoted.
    #[test]
    fn windows_lookup_skips_the_current_folder_and_quotes_cmd_shims() {
        /// A program in a folder with a space.
        const NODE_EXE: &str = "C:\\Program Files\\nodejs\\node.exe";
        /// A .cmd shim, run through cmd.exe.
        const CLAUDE_CMD: &str = "C:\\Users\\me\\AppData\\Roaming\\npm\\claude.cmd";
        const CLAUDE: &str = "claude";
        /// An argument cmd.exe would read as two commands.
        const AMPERSAND: &str = "a&b";
        /// An argument that needs no quotes.
        const PLAIN: &str = "plain";
        let files = [
            NODE_EXE,
            CLAUDE_CMD,
            "node.cmd",
            ".\\claude.bat",
            "repo\\bin\\npx.cmd",
            "\\\\server\\tools\\git.exe",
        ];
        // Windows file names do not care about case.
        let exists = |p: &str| files.iter().any(|f| f.eq_ignore_ascii_case(p));
        let path = ".;repo\\bin;;\\rooted;\"C:\\Program Files\\nodejs\\\";C:\\Users\\me\\AppData\\Roaming\\npm;\\\\server\\tools";
        let find = |name: &str| find_in_path(name, path, None, true, &exists);
        assert_eq!(
            find("node").as_deref(),
            Some("C:\\Program Files\\nodejs\\node.EXE")
        );
        assert_eq!(
            find(CLAUDE).as_deref(),
            Some("C:\\Users\\me\\AppData\\Roaming\\npm\\claude.CMD")
        );
        assert_eq!(find("npx"), None, "a relative PATH entry is not searched");
        assert_eq!(find("git").as_deref(), Some("\\\\server\\tools\\git.EXE"));
        assert_eq!(find("..\\node"), None);
        // PATHEXT decides which extensions count, in its order.
        assert_eq!(
            find_in_path(CLAUDE, path, Some(".EXE"), true, &exists),
            None
        );
        assert_eq!(
            find_in_path("claude.cmd", path, Some(".EXE;.CMD"), true, &exists).as_deref(),
            Some(CLAUDE_CMD)
        );

        let shim = plan_for(
            CLAUDE_CMD,
            vec![
                "a&calc".into(),
                "C:\\Users\\John Doe\\repo".into(),
                PLAIN.into(),
            ],
            true,
        );
        assert_eq!(
            shim,
            Spawn {
                cmd: format!("\"{CLAUDE_CMD}\""),
                args: vec![
                    "\"a&calc\"".into(),
                    "\"C:\\Users\\John Doe\\repo\"".into(),
                    PLAIN.into()
                ],
                shell: true
            }
        );
        let exe = plan_for(NODE_EXE, vec![AMPERSAND.into()], true);
        assert_eq!(exe, Spawn::direct(NODE_EXE, vec![AMPERSAND.into()]));
        assert!(!plan_for("/usr/bin/claude.cmd", Vec::new(), false).shell);
    }

    #[test]
    fn posix_lookup_skips_relative_entries() {
        const NODE: &str = "node";
        /// The one [`NODE`] in an absolute folder.
        const ABSOLUTE: &str = "/opt/node/bin/node";
        let files = [NODE, "./node", "bin/node", ABSOLUTE];
        let exists = |p: &str| files.contains(&p);
        let find = |path: &str| find_in_path(NODE, path, None, false, &exists);
        assert_eq!(find(":.:bin:/opt/node/bin/").as_deref(), Some(ABSOLUTE));
        assert_eq!(find("::.:bin"), None);
        assert_eq!(find_in_path("a/node", "/opt", None, false, &exists), None);
        // The real lookup: a program in the current folder is not found through a "." entry.
        let dir = crate::core::util::test_dir("lookup");
        const PROBE: &str = "dfw-lookup-probe";
        let bin = dir.join(PROBE);
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
            let abs = dir.display().to_string();
            assert_eq!(
                find_in_path(PROBE, &abs, None, false, &is_program),
                Some(bin.display().to_string())
            );
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();
            assert_eq!(find_in_path(PROBE, &abs, None, false, &is_program), None);
        }
    }

    #[test]
    fn cmd_quoting() {
        const PLAIN: &str = "plain";
        assert_eq!(quote_for_cmd(PLAIN, false), PLAIN);
        assert_eq!(quote_for_cmd("a&calc", false), "\"a&calc\"");
        assert_eq!(quote_for_cmd("", false), "\"\"");
        assert_eq!(quote_for_cmd("say \"hi\"", false), "\"say \"\"hi\"\"\"");
        assert_eq!(quote_for_cmd(PLAIN, true), format!("\"{PLAIN}\""));
        assert_eq!(quote_for_cmd("a\"b", true), "\"a\"b\"");
    }

    /// The C runtime's (and Rust's) reading of a Windows command line, for the round trips.
    fn crt_args(line: &str) -> Vec<String> {
        let chars: Vec<char> = line.chars().collect();
        let (mut args, mut cur, mut quoted, mut any) = (Vec::new(), String::new(), false, false);
        let mut i = 0;
        while i < chars.len() {
            let c = chars[i];
            if c == '\\' {
                let n = chars[i..].iter().take_while(|&&x| x == '\\').count();
                i += n;
                if chars.get(i) == Some(&'"') {
                    cur.push_str(&"\\".repeat(n / 2));
                    if n % 2 == 1 {
                        cur.push('"');
                        i += 1;
                    }
                } else {
                    cur.push_str(&"\\".repeat(n));
                }
                any = true;
                continue;
            }
            if c == '"' {
                if quoted && chars.get(i + 1) == Some(&'"') {
                    cur.push('"');
                    i += 2;
                    continue;
                }
                quoted = !quoted;
                any = true;
            } else if (c == ' ' || c == '\t') && !quoted {
                if any {
                    args.push(std::mem::take(&mut cur));
                }
                any = false;
            } else {
                cur.push(c);
                any = true;
            }
            i += 1;
        }
        if any {
            args.push(cur);
        }
        args
    }

    /// A PowerShell single-quoted literal read back (the quote at `start`): its text and the
    /// index after it.
    fn ps_literal(text: &str, start: usize) -> (String, usize) {
        let chars: Vec<char> = text.chars().collect();
        let quote = |c: char| matches!(c, '\'' | '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}');
        assert!(quote(chars[start]));
        let (mut out, mut i) = (String::new(), start + 1);
        loop {
            if quote(chars[i]) {
                if chars.get(i + 1).is_some_and(|&c| quote(c)) {
                    out.push(chars[i]);
                    i += 2;
                    continue;
                }
                return (out, i + 1);
            }
            out.push(chars[i]);
            i += 1;
        }
    }

    const TRICKY: [&str; 12] = [
        "plain",
        "with space",
        "it's",
        "say \"hi\"",
        "a & b | c < d > e ^ f",
        "$env:PATH $(Get-Date) `n",
        "trailing\\",
        "C:\\dir with space\\",
        "a\\\"b",
        "",
        "Tiếng Việt ‘quoted’ ✓",
        "http://localhost:5173/a b?x=1&y='2'",
    ];

    /// Every argument comes back from a Windows command line as written, and cmd.exe sees each
    /// `& | < > ^` inside quotes.
    #[test]
    fn windows_arguments_survive_the_command_line_and_cmd() {
        for arg in TRICKY {
            let quoted = quote_windows_arg(arg);
            assert_eq!(
                crt_args(&format!("prog {quoted}")),
                ["prog", arg],
                "{quoted}"
            );
            let mut inside = false;
            for c in quoted.chars() {
                if c == '"' {
                    inside = !inside;
                } else if "&|<>^".contains(c) {
                    assert!(inside, "{c} outside quotes for cmd.exe in {quoted}");
                }
            }
            assert!(!inside, "unbalanced quotes for cmd.exe: {quoted}");
        }
        let line = TRICKY.map(quote_windows_arg).join(" ");
        assert_eq!(crt_args(&line), TRICKY);
    }

    #[test]
    fn powershell_literals_keep_every_character() {
        for text in TRICKY {
            let literal = quote_for_powershell(text);
            assert_eq!(
                ps_literal(&literal, 0),
                (text.to_string(), literal.chars().count())
            );
        }
        assert_eq!(quote_for_powershell("it's"), "'it''s'");
        assert_eq!(quote_for_powershell("‘x’"), "'‘‘x’’'");
    }

    /// The script `Start-Process` runs: the program, its arguments and the folder as literals,
    /// the arguments and the log path back as written; the pid of the program (cmd.exe's
    /// child) is what it prints.
    #[test]
    fn the_detached_script_carries_every_value_as_written() {
        let exe = Path::new("C:\\Program Files\\dfw's\\dfw.exe");
        let cwd = Path::new("C:\\work\\dự án $x");
        let cmd = Path::new("C:\\Windows\\System32\\cmd.exe");
        let args: Vec<String> = TRICKY.iter().map(|a| (*a).to_string()).collect();
        // Every literal of a script, in order.
        let literals = |script: &str| {
            let mut out = Vec::new();
            let mut i = 0;
            let chars: Vec<char> = script.chars().collect();
            while i < chars.len() {
                if chars[i] == '\'' {
                    let (text, next) = ps_literal(script, i);
                    out.push(text);
                    i = next;
                } else {
                    i += 1;
                }
            }
            out
        };

        let plain = detached_script(exe, &args, cwd, None, cmd);
        assert!(plain.contains("Start-Process -FilePath "));
        assert!(plain.contains("-WindowStyle Hidden -PassThru"));
        assert!(
            !plain.contains("-Redirect"),
            "a redirect makes Start-Process hand down handles"
        );
        let l = literals(&plain);
        // 'Stop', the program, the arguments, the folder.
        assert_eq!(l[1], exe.display().to_string());
        assert_eq!(crt_args(&format!("x {}", l[2]))[1..], TRICKY);
        assert_eq!(l[3], cwd.display().to_string());
        assert!(plain.ends_with("[Console]::Out.Write($p.Id)\n"));
        assert!(!detached_script(exe, &[], cwd, None, cmd).contains("-ArgumentList"));

        let log = Path::new("C:\\work\\dự án\\.dfw\\studio.log");
        let logged = detached_script(exe, &args, cwd, Some(log), cmd);
        let l = literals(&logged);
        assert_eq!(l[1], cmd.display().to_string());
        let line = &l[2];
        let inner = line
            .strip_prefix("/d /s /c \"")
            .and_then(|r| r.strip_suffix('"'))
            .unwrap();
        // cmd.exe /s drops the outer quotes and runs the rest: the program, its arguments,
        // then the redirection.
        let mut words = crt_args(inner);
        assert_eq!(words.pop().as_deref(), Some("2>&1"));
        assert_eq!(words.pop(), Some(format!(">>{}", log.display())));
        assert_eq!(words[0], exe.display().to_string());
        assert_eq!(words[1..], TRICKY);
        assert_eq!(l[3], cwd.display().to_string());
        // The program's file name as a WQL string (backslash escapes; on Unix the whole path is
        // one name, so its quote and backslashes are escaped too).
        let name = exe.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(l[4], name.replace('\\', "\\\\").replace('\'', "\\'"));
        assert!(logged.contains("Get-CimInstance Win32_Process -Filter \"ParentProcessId = $($p.Id) AND Name = '$name'\""));
        assert!(logged.ends_with("[Console]::Out.Write($id)\n"));
    }
}
