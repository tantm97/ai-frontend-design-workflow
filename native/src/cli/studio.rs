//! Server commands of `core/lib/cli.mjs`: studio, review (`cmdStudio`, `cmdReview`) and the internal
//! `__studio-server` and `__review-server` processes they spawn.

use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Ctx, Handler};
use crate::core::config::{load_config, studio_port};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, resolve};
use crate::core::process::{is_alive, kill};
use crate::core::util::{now_iso, out, read_json, write_json, ArgValue, Args};
use crate::studio::live_proxy::{
    assert_local_target, configured_shortcut, start_live_server, LiveOptions,
};
use crate::studio::server;

/// The commands of this group.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("studio", studio as Handler),
        ("review", review as Handler),
        ("__studio-server", studio_server as Handler),
        ("__review-server", review_server as Handler),
    ]
}

fn port_free(port: u16) -> bool {
    TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// A GET to the local server: status and body, `None` when nothing answers.
fn get(port: u16, path: &str) -> Option<(u16, String)> {
    let mut s = TcpStream::connect(("127.0.0.1", port)).ok()?;
    let _ = s.set_read_timeout(Some(Duration::from_secs(5)));
    s.write_all(
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n")
            .as_bytes(),
    )
    .ok()?;
    let mut raw = Vec::new();
    s.read_to_end(&mut raw).ok()?;
    let text = String::from_utf8_lossy(&raw).into_owned();
    let code = text.get(9..12)?.parse().ok()?;
    let body = text
        .split_once("\r\n\r\n")
        .map_or(String::new(), |(_, b)| b.to_string());
    Some((code, body))
}

/// `waitFor(url)`: until the server answers 2xx, at most 5 s.
fn wait_for(port: u16, path: &str) -> Option<String> {
    let end = Instant::now() + Duration::from_secs(5);
    while Instant::now() < end {
        if let Some((code, body)) = get(port, path) {
            if (200..300).contains(&code) {
                return Some(body);
            }
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    None
}

/// Start this binary again as a detached server process, its output appended to `log`.
fn spawn_server(args: &[String], log: &Path) -> Result<u32, DfwError> {
    let exe = std::env::current_exe()?;
    let file = OpenOptions::new().create(true).append(true).open(log)?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdin(Stdio::null())
        .stdout(file.try_clone()?)
        .stderr(file);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Its own process group: closing the terminal or stopping the agent does not stop it.
        cmd.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    Ok(cmd.spawn()?.id())
}

/// `Number(flag || fallback)` for a port flag.
fn port_arg(a: &Args, fallback: u16) -> Result<u16, DfwError> {
    match a.text("port").filter(|p| !p.is_empty()) {
        None => Ok(fallback),
        Some(p) => p
            .trim()
            .parse::<u16>()
            .map_err(|_| DfwError::new(format!("--port {p} is not a port number."))),
    }
}

fn free_from(mut port: u16) -> u16 {
    for _ in 0..20 {
        if port_free(port) {
            break;
        }
        port = port.wrapping_add(1);
    }
    port
}

/// `dfw studio start|stop|status [--port <n>]`.
fn studio(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let p = project_paths(&root);
    let sub = a.pos(1).filter(|s| !s.is_empty()).unwrap_or("status");
    let info = read_json(&p.studio, Some(Value::Null))?;
    let alive = js::truthy(&info) && is_alive(info.get("pid"));
    let text = |k: &str| crate::core::js::to_string(info.get(k));
    match sub {
        "status" => {
            out(&if alive {
                format!("studio running: {} (pid {})", text("url"), text("pid"))
            } else {
                "studio not running".into()
            });
            return Ok(0);
        }
        "stop" => {
            if alive {
                kill(info.get("pid"));
            }
            if p.studio.exists() {
                fs::remove_file(&p.studio)?;
            }
            out("studio stopped");
            return Ok(0);
        }
        "start" => {}
        _ => return Err(DfwError::new("Usage: dfw studio start|stop|status")),
    }
    if alive {
        out(&format!("studio already running: {}", text("url")));
        return Ok(0);
    }
    let config = load_config(Some(&root))?;
    let port = free_from(port_arg(a, studio_port(&config))?);
    fs::create_dir_all(&p.dfw)?;
    let log = p.dfw.join("studio.log");
    let args: Vec<String> = vec![
        "__studio-server".into(),
        "--root".into(),
        root.display().to_string(),
        "--port".into(),
        port.to_string(),
    ];
    let pid = spawn_server(&args, &log)?;
    let url = format!("http://127.0.0.1:{port}/");
    // The ping route answers even when design.json is broken (the manifest route does not).
    if wait_for(port, "/__dfw/api/ping").is_none_or(|b| !b.contains(r#""dfw":"studio""#)) {
        // Never leave a server running that no studio.json names: "studio stop" could not
        // reach it.
        kill(Some(&json!(pid)));
        return Err(DfwError::new(format!(
            "Studio did not start; see {}",
            log.display()
        )));
    }
    write_json(
        &p.studio,
        &json!({ "pid": pid, "port": port, "url": url, "startedAt": now_iso() }),
    )?;
    out(&format!("studio running: {url}"));
    Ok(0)
}

/// The `--shortcut` flag as Node reads it (`String(a.shortcut)`: alone it is "true").
fn shortcut_flag(a: &Args) -> Option<String> {
    match a.get("shortcut") {
        Some(ArgValue::Flag) => Some("true".into()),
        Some(ArgValue::Text(t)) => Some(t.clone()),
        None => None,
    }
}

/// `dfw review start --url <dev server> [--port <n>] [--shortcut <combo>] [--no-shots] | stop | status`.
fn review(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = ctx.root();
    let p = project_paths(&root);
    let file = p.dfw.join("review.json");
    let sub = a.pos(1).filter(|s| !s.is_empty()).unwrap_or("status");
    let info = read_json(&file, Some(Value::Null))?;
    let running = js::truthy(&info) && is_alive(info.get("pid"));
    let text = |k: &str| crate::core::js::to_string(info.get(k));
    match sub {
        "status" => {
            out(&if running {
                format!(
                    "review running: {} -> {} (pid {})",
                    text("url"),
                    text("target"),
                    text("pid")
                )
            } else {
                "review not running".into()
            });
            return Ok(0);
        }
        "stop" => {
            if running {
                kill(info.get("pid"));
            }
            if file.exists() {
                fs::remove_file(&file)?;
            }
            out("review stopped");
            return Ok(0);
        }
        "start" => {}
        _ => return Err(DfwError::new("Usage: dfw review start --url <dev server url> [--port <n>] [--shortcut <combo>] [--no-shots] | stop | status")),
    }
    let Some(raw_url) = a.text("url").filter(|u| !u.is_empty()) else {
        return Err(DfwError::new(
            "dfw review start needs --url <the running app, e.g. http://localhost:5173>.",
        ));
    };
    let t = assert_local_target(raw_url)?;
    let target = t.origin();
    // Comment-mode key: --shortcut, else "dfw config set review.shortcut <combo>", else
    // Alt/Option+C ("off": button only).
    let shortcut = configured_shortcut(&root, shortcut_flag(a).as_deref())?;
    if running {
        if info.get("target").and_then(Value::as_str) == Some(target.as_str()) {
            out(&format!(
                "review already running: {} -> {target}",
                text("url")
            ));
            return Ok(0);
        }
        kill(info.get("pid"));
    }
    let port = free_from(port_arg(a, 4700)?);
    fs::create_dir_all(&p.dfw)?;
    let log = p.dfw.join("review.log");
    let mut args: Vec<String> = [
        "__review-server",
        "--root",
        &root.display().to_string(),
        "--url",
        &target,
        "--port",
        &port.to_string(),
        "--shortcut",
        &shortcut,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    if a.truthy("no-shots") {
        args.push("--no-shots".into());
    }
    let pid = spawn_server(&args, &log)?;
    // Always bound on 127.0.0.1; served as http://localhost:<port>/ for a localhost app (its
    // cookies then reach the proxy).
    let Some(body) = wait_for(port, "/__dfw/api/live") else {
        kill(Some(&json!(pid)));
        return Err(DfwError::new(format!(
            "dfw review did not start; see {}",
            log.display()
        )));
    };
    let url = serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|v| v.get("url").and_then(Value::as_str).map(str::to_string))
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}/"));
    write_json(
        &file,
        &json!({ "pid": pid, "port": port, "url": url, "target": target, "shortcut": shortcut, "startedAt": now_iso() }),
    )?;
    let label: Vec<String> = shortcut
        .split('+')
        .map(|k| match k {
            "alt" => "Alt/Option".into(),
            "meta" => "Cmd/Meta".into(),
            "ctrl" => "Ctrl".into(),
            "shift" => "Shift".into(),
            other => other.to_uppercase(),
        })
        .collect();
    let key = if shortcut == "off" {
        String::new()
    } else {
        format!(" (or {})", label.join("+"))
    };
    let shown = match url.strip_suffix('/') {
        Some(base) => format!("{base}{}", t.pathname()),
        None => url.clone(),
    };
    out(&format!("review running: {shown}  (proxy of {target})\nOpen it, click \"Comment\"{key}, click an element, write, Cmd/Ctrl+Enter. Comments arrive like Studio comments (dfw comments wait)."));
    Ok(0)
}

fn root_flag(a: &Args) -> PathBuf {
    resolve(
        a.text("root")
            .filter(|r| !r.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default()),
    )
}

/// `dfw __studio-server --root <dir> --port <n>` (spawned by `dfw studio start`).
fn studio_server(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let port = a
        .text("port")
        .filter(|p| !p.is_empty())
        .map(|p| p.trim().parse::<u16>())
        .transpose()
        .map_err(|_| DfwError::new("--port is not a port number."))?;
    server::serve(&root_flag(a), port)
}

/// `dfw __review-server --root <dir> --url <app> --port <n> [--shortcut <combo>] [--no-shots]`
/// (spawned by `dfw review start`).
fn review_server(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let root = root_flag(a);
    let target = a.text("url").unwrap_or_default().to_string();
    let port = a
        .text("port")
        .and_then(|p| p.trim().parse::<u16>().ok())
        .filter(|p| *p > 0)
        .unwrap_or(4700);
    let opts = LiveOptions {
        root: root.clone(),
        target: target.clone(),
        shots: !a.truthy("no-shots"),
        shortcut: shortcut_flag(a),
    };
    let (server, mut listeners, url) = start_live_server(&opts, port)?;
    out(&format!(
        "dfw review: {url} -> {target}  (project: {})",
        root.display()
    ));
    let first = listeners.remove(0);
    for extra in listeners {
        let s = std::sync::Arc::clone(&server);
        std::thread::spawn(move || s.serve(extra));
    }
    server.serve(first);
    Ok(0)
}
