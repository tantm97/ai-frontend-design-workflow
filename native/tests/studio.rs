//! Tests of `dfw studio` that need the built binary: start, stop and what is left behind.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

#[path = "../src/core/paths/layout.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the project layout"
)]
mod layout;
use layout::{file, rel};

#[path = "../src/core/fixture.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the fixture names"
)]
mod fixture;
use fixture::{SCREEN, SCREEN_PAGE};

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn cli(root: &Path, args: &[&str]) -> (i32, String, String) {
    let mut cmd = Command::new(DFW);
    cmd.args(args).current_dir(root);
    for name in [
        "DFW_HOST",
        "DFW_PROJECT_DIR",
        "CLAUDE_PROJECT_DIR",
        "DFW_NOW",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("HOME", root.join(".home"))
        .env("USERPROFILE", root.join(".home"))
        .env("DFW_HOME", root.join(".home/dfw"))
        .env("DFW_TELEMETRY", "off")
        .env("DFW_PLUGIN_ROOT", repo().join("core"));
    let out = output_within(cmd, "", Duration::from_secs(60));
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// The output of `cmd` (stdin written first), or a failure after `limit` instead of a hang: a
/// process the command started that keeps its output pipes open (Windows hands inheritable
/// handles down to every child) would otherwise stall the whole test run.
fn output_within(mut cmd: Command, stdin: &str, limit: Duration) -> Output {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    // A command that does not read its stdin may have closed it already.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let (mut o, mut e) = (Vec::new(), Vec::new());
        let reader = std::thread::spawn(move || {
            let _ = err.read_to_end(&mut e);
            e
        });
        let _ = out.read_to_end(&mut o);
        let _ = tx.send((o, reader.join().unwrap_or_default()));
    });
    match rx.recv_timeout(limit) {
        Ok((stdout, stderr)) => Output {
            status: child.wait().unwrap(),
            stdout,
            stderr,
        },
        Err(_) => {
            let exited = child.try_wait().unwrap();
            let _ = child.kill();
            panic!(
                "{cmd:?}: no end of output after {limit:?} (the command {}): a process it started holds its stdout or stderr open",
                exited.map_or("is still running".to_string(), |s| format!("exited with {s}"))
            );
        }
    }
}

fn free_port() -> u16 {
    TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn status_of(port: u16, path: &str) -> Option<u16> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n").as_bytes())
        .ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    out.get(9..12)?.parse().ok()
}

/// dfw studio start with a broken design.json starts a Studio that "studio stop" can stop:
/// nothing keeps running that no studio.json names
#[test]
fn studio_start_with_a_broken_design_json_can_be_stopped() {
    let dir = std::env::temp_dir().join(format!("dfw-native-studio-{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let root = dir.canonicalize().unwrap();
    fs::create_dir_all(root.join(rel::screen(SCREEN))).unwrap();
    fs::write(root.join(rel::screen(SCREEN_PAGE)), "<p>a</p>").unwrap();
    fs::write(root.join(rel::MANIFEST), "{ broken").unwrap();
    let port = free_port();
    let (code, stdout, stderr) = cli(&root, &["studio", "start", "--port", &port.to_string()]);
    assert_eq!(code, 0, "{stdout}{stderr}");
    assert!(stdout.contains(&format!("studio running: http://127.0.0.1:{port}/")));
    assert!(root.join(rel::join(rel::DFW, file::STUDIO)).exists());
    assert_eq!(status_of(port, "/__dfw/api/ping"), Some(200));
    assert_eq!(
        status_of(port, "/__dfw/api/manifest"),
        Some(500),
        "the broken file is reported by its route"
    );
    let (code, stdout, _) = cli(&root, &["studio", "stop"]);
    assert_eq!((code, stdout.trim()), (0, "studio stopped"));
    let end = Instant::now() + Duration::from_secs(5);
    while status_of(port, "/__dfw/api/ping").is_some() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        status_of(port, "/__dfw/api/ping"),
        None,
        "the server is gone"
    );
    let _ = fs::remove_dir_all(&root);
}
