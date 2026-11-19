//! The binary alone: copied into an empty folder with no plugin folder, no `core/` and no
//! `DFW_PLUGIN_ROOT`, every command that reads a file of `core/` still works, because those files
//! are embedded (`src/assets.rs`). This is how `install.sh` installs dfw.

use std::fs;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use serde_json::Value;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

/// A fresh folder outside the repository (its real path: macOS reaches the temporary directory
/// through a symlink).
fn tmp_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dfw-native-standalone-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// Where the copied binary runs: `<tmp>/bin/dfw`, a home of its own and a project folder.
struct Standalone {
    tmp: PathBuf,
    exe: PathBuf,
    project: PathBuf,
}

impl Standalone {
    fn new() -> Self {
        let tmp = tmp_dir();
        let exe = tmp.join("bin").join(Path::new(DFW).file_name().unwrap());
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::copy(DFW, &exe).unwrap();
        let project = tmp.join("project");
        fs::create_dir_all(&project).unwrap();
        Standalone { tmp, exe, project }
    }

    fn run(&self, args: &[&str], stdin: &str) -> Output {
        let mut cmd = Command::new(&self.exe);
        cmd.args(args).current_dir(&self.project);
        for name in [
            "DFW_PLUGIN_ROOT",
            "DFW_BIN",
            "DFW_HOST",
            "DFW_PROJECT_DIR",
            "CLAUDE_PROJECT_DIR",
            "DFW_NOW",
            "DFW_VERSION",
            "XDG_CONFIG_HOME",
        ] {
            cmd.env_remove(name);
        }
        let home = self.tmp.join("home");
        cmd.env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("DFW_HOME", home.join(".config").join("dfw"))
            .env("DFW_TELEMETRY", "off");
        // No Node.js either: only the system folders on PATH.
        if cfg!(unix) {
            cmd.env("PATH", "/usr/bin:/bin");
        }
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }

    /// Run a command that must succeed; its stdout.
    fn ok(&self, args: &[&str]) -> String {
        let out = self.run(args, "");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            out.status.success(),
            "dfw {args:?} failed ({:?}): {stderr}",
            out.status.code()
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }
}

impl Drop for Standalone {
    fn drop(&mut self) {
        let _ = self.run(&["studio", "stop"], "");
        let _ = fs::remove_dir_all(&self.tmp);
    }
}

/// `GET <path>` on the Studio: status code and body.
fn get(port: u64, path: &str) -> Option<(u16, String)> {
    let mut stream = TcpStream::connect(("127.0.0.1", u16::try_from(port).ok()?)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok()?;
    let request =
        format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut response = String::new();
    stream.read_to_string(&mut response).ok()?;
    let code = response.split(' ').nth(1)?.parse().ok()?;
    let body = response.split_once("\r\n\r\n")?.1.to_string();
    Some((code, body))
}

#[test]
fn the_binary_alone_runs_every_command_that_reads_embedded_files() {
    let s = Standalone::new();
    assert!(s.ok(&["version"]).starts_with("dfw "));

    // Templates.
    s.ok(&["init", "--product", "Standalone"]);
    assert!(s.project.join(".dfw/state.json").is_file());
    assert!(s.project.join("PRODUCT.md").is_file());
    assert!(s.project.join("design/design.json").is_file());

    // Kits: presets, style guides, the starter kit's masters.
    assert!(s.ok(&["system", "presets"]).contains("crisp"));
    assert!(s.ok(&["style", "list"]).contains("swiss-grid"));
    assert!(s
        .ok(&["system", "ref", "button"])
        .contains("<dfw-component"));
    s.ok(&["system", "init", "--preset", "soft"]);
    let tokens = fs::read_to_string(s.project.join("design/tokens/tokens.json")).unwrap();
    assert!(!tokens.contains("Placeholder tokens created by dfw init"));
    s.ok(&["style", "apply", "swiss-grid", "--fonts", "latin"]);

    // Assets: the icon library.
    assert!(s.ok(&["icons", "search", "arrow"]).contains("arrow"));
    s.ok(&["icons", "add", "check"]);
    assert!(s.project.join("design/system/icons/check.svg").is_file());

    s.ok(&["build"]);
    assert!(s.project.join("design/tokens/tokens.css").is_file());
    fs::write(
        s.project.join("design/screens/home.html"),
        "<!doctype html>\n<html lang=\"en\"><head><title>Home</title></head><body><main><h1>Home</h1></main></body></html>\n",
    )
    .unwrap();
    let lint = s.run(&["lint"], "");
    assert!(
        matches!(lint.status.code(), Some(0 | 2)),
        "lint: {}",
        String::from_utf8_lossy(&lint.stderr)
    );

    // The Studio serves its embedded browser files.
    assert!(s.ok(&["studio", "start"]).contains("studio"));
    let info: Value =
        serde_json::from_str(&fs::read_to_string(s.project.join(".dfw/studio.json")).unwrap())
            .unwrap();
    let port = info["port"].as_u64().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    let mut page = None;
    while Instant::now() < end && page.is_none() {
        page = get(port, "/").filter(|(code, _)| *code == 200);
        if page.is_none() {
            std::thread::sleep(Duration::from_millis(100));
        }
    }
    let (_, html) = page.expect("the Studio answers on its port");
    assert!(html.contains("<html"), "{html}");
    let (code, js) = get(port, "/__dfw/static/app.js").unwrap();
    assert_eq!(code, 200);
    assert!(!js.is_empty());
    assert!(s.ok(&["studio", "stop"]).contains("studio stopped"));

    // The MCP guide comes from the embedded workflow text.
    let request = r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"dfw_guide","arguments":{"topic":"references/phase-concepts.md"}}}"#;
    let out = s.run(&["mcp"], &format!("{request}\n"));
    let reply: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(reply["result"]["isError"], false, "{reply}");
    assert!(reply["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("Phase: concepts"));
}
