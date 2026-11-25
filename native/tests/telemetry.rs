//! Telemetry through the built binary (the `telemetry` OpenSpec change): what reaches a
//! collector, what stops it, and that a command never waits for it. The collector is a
//! `TcpListener` in the test; the binary is pointed at it with `dfw telemetry endpoint`, which
//! stands in for an address compiled in (one decision covers both, see `telemetry::address`).

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

#[path = "../src/core/paths/layout.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the project layout"
)]
mod layout;
use layout::rel;

#[path = "../src/core/host/wire.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of what a host sends"
)]
mod wire;
use wire::{event, tool};

#[path = "../src/core/word.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the command line")]
mod word;
use word::{command, flag, hook_name, sub};

#[path = "../src/core/host/id.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the host ids")]
mod host_id;

#[path = "../src/workflow/names.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the workflow names"
)]
mod names;

#[path = "../src/telemetry/wire.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of what the telemetry sends"
)]
mod telemetry_wire;
use telemetry_wire::{attr, event as recorded, file as telemetry_file, otlp};

#[path = "../src/core/fixture.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the fixture names"
)]
mod fixture;
use fixture::{APP_MODULE, SCREEN_PAGE, SECRET_COMPONENT};

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");
/// What the build script passed on when these tests were compiled (the same build as the binary).
const BUILT_WITH_ENDPOINT: Option<&str> = option_env!("DFW_OTLP_ENDPOINT");
/// A local address nothing listens on: a flush to it fails at once and the spool stays.
const NOWHERE: &str = "http://127.0.0.1:1";
/// A phase no workflow has.
const NO_PHASE: &str = "nowhere-phase";
/// A first argument that is no command.
const NO_COMMAND: &str = "no-such-command";
/// A session id that must never leave the machine.
const SECRET_SESSION: &str = "session-secret-1";
/// File content that must never leave the machine.
const SECRET_CONTENT: &str = "secret file content";
/// What stands for the project's path in the output compared.
const PROJECT_MASK: &str = "<project>";
/// The value of `DFW_TELEMETRY` that turns telemetry off.
const SWITCHED_OFF: &str = "off";

fn address_compiled_in() -> bool {
    BUILT_WITH_ENDPOINT.is_some_and(|e| !e.trim().is_empty())
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A fresh folder (its real path) whose name is a marker no event may contain.
fn tmp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-telemetry-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

/// One machine of a test: a project folder and a dfw home.
struct Machine {
    project: PathBuf,
    home: PathBuf,
    env: Vec<(&'static str, String)>,
}

struct Ran {
    code: i32,
    stdout: String,
    stderr: String,
    took: Duration,
}

impl Machine {
    /// A copy of the fixture project (it has `design/`, so `dfw init` runs natively).
    fn new(label: &str) -> Machine {
        let base = tmp_dir(label);
        let project = base.join("acme-secret-project");
        copy_tree(&repo().join("tests/fixtures/penpot/project"), &project);
        Machine {
            project,
            home: base.join("home"),
            env: Vec::new(),
        }
    }

    fn with_env(mut self, name: &'static str, value: &str) -> Machine {
        self.env.push((name, value.to_string()));
        self
    }

    fn telemetry_dir(&self) -> PathBuf {
        self.home.join("telemetry")
    }

    fn spool(&self) -> PathBuf {
        self.telemetry_dir().join(telemetry_file::SPOOL)
    }

    /// `dfw <args>` in the project. Nothing of the session that runs the tests reaches the
    /// binary: not its project, not its telemetry switches, not its proxy.
    fn run_with(&self, args: &[&str], stdin: &str, extra: &[(&str, &str)]) -> Ran {
        let user_home = self.home.join("user");
        let mut cmd = home::command(DFW, &self.project, &user_home, &self.home);
        cmd.args(args);
        for name in [
            "DO_NOT_TRACK",
            "DFW_TELEMETRY",
            "CI",
            "ALL_PROXY",
            "all_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "HTTP_PROXY",
            "http_proxy",
            "NO_PROXY",
            "no_proxy",
        ] {
            cmd.env_remove(name);
        }
        cmd.env("DFW_PLUGIN_ROOT", repo().join("core"))
            .env("DFW_NOW", "2026-01-02T03:04:05.678Z");
        for (name, value) in &self.env {
            cmd.env(name, value);
        }
        for (name, value) in extra {
            cmd.env(name, value);
        }
        let started = Instant::now();
        let mut child = cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        let out = child.wait_with_output().unwrap();
        Ran {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
            took: started.elapsed(),
        }
    }

    fn run(&self, args: &[&str]) -> Ran {
        self.run_with(args, "", &[])
    }

    fn ok(&self, args: &[&str]) -> String {
        let ran = self.run(args);
        assert_eq!(ran.code, 0, "dfw {args:?}: {}", ran.stderr);
        ran.stdout
    }

    /// Point telemetry at `endpoint` and turn it on (no notice needed after the user's own "on").
    fn turn_on(&self, endpoint: &str) {
        self.ok(&[command::TELEMETRY, sub::ENDPOINT, endpoint]);
        let said = self.ok(&[command::TELEMETRY, sub::ON]);
        assert!(said.starts_with("on: sending to "), "{said}");
    }

    /// The log records `dfw telemetry show` prints.
    fn shown(&self) -> Vec<Value> {
        let text = self.ok(&[command::TELEMETRY, sub::SHOW]);
        if text.starts_with("No events") {
            return Vec::new();
        }
        records(&serde_json::from_str(&text).unwrap())
    }
}

fn records(body: &Value) -> Vec<Value> {
    body[otlp::RESOURCE_LOGS][0][otlp::SCOPE_LOGS][0][otlp::LOG_RECORDS]
        .as_array()
        .unwrap()
        .clone()
}

fn attribute<'a>(record: &'a Value, key: &str) -> Option<&'a Value> {
    record[otlp::ATTRIBUTES]
        .as_array()?
        .iter()
        .find(|a| a[otlp::KEY] == key)
        .map(|a| &a[otlp::VALUE])
}

/// `(event name, the value of its first attribute)` of each record.
fn summary(records: &[Value]) -> Vec<(String, String)> {
    records
        .iter()
        .map(|r| {
            let first = &r[otlp::ATTRIBUTES][0][otlp::VALUE];
            let value = first[otlp::STRING_VALUE]
                .as_str()
                .or(first[otlp::INT_VALUE].as_str())
                .unwrap();
            (
                r[otlp::EVENT_NAME].as_str().unwrap().to_string(),
                value.into(),
            )
        })
        .collect()
}

/// One request a sink received.
struct Request {
    line: String,
    headers: Vec<String>,
    body: String,
}

/// A collector for one test: answers every request with 200 after `delay`, and hands the
/// requests to the test.
struct Sink {
    url: String,
    requests: Receiver<Request>,
}

fn read_request(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut headers = Vec::new();
    let mut length = 0;
    loop {
        let mut header = String::new();
        reader.read_line(&mut header).ok()?;
        let header = header.trim_end().to_string();
        if header.is_empty() {
            break;
        }
        if let Some(value) = header.to_ascii_lowercase().strip_prefix("content-length:") {
            length = value.trim().parse().ok()?;
        }
        headers.push(header);
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        line: line.trim_end().to_string(),
        headers,
        body: String::from_utf8(body).ok()?,
    })
}

impl Sink {
    fn start(delay: Duration) -> Sink {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (send, requests) = channel();
        thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let Some(request) = read_request(&stream) else {
                    continue;
                };
                let _ = send.send(request);
                thread::sleep(delay);
                let _ = stream.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                );
            }
        });
        Sink { url, requests }
    }

    fn next(&self, within: Duration) -> Option<Request> {
        self.requests.recv_timeout(within).ok()
    }

    /// Nothing arrives for a while.
    fn assert_silent(&self) {
        let got = self.next(Duration::from_millis(400));
        assert!(
            got.is_none(),
            "unexpected request: {:?}",
            got.map(|r| r.line)
        );
    }
}

fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "timed out: {what}"
        );
        thread::sleep(Duration::from_millis(20));
    }
}

/// Events arrive as valid OTLP JSON, and nothing of the project, the files or the arguments
/// is in them.
#[test]
// One collector run: every event kind is sent and checked against the same capture.
#[allow(
    clippy::too_many_lines,
    reason = "long function kept as ported; split it when it next changes (A10)"
)]
fn events_arrive_as_otlp_json_and_carry_no_content() {
    let sink = Sink::start(Duration::ZERO);
    let m = Machine::new("otlp");
    m.turn_on(&sink.url);
    let project = m.project.to_string_lossy().into_owned();

    // The first recorded command starts the detached flush: wait for it, so the rest of the
    // events leave in one request of this test's own flush.
    m.ok(&[command::INIT, flag::PRODUCT, "Acme Secret Shop"]);
    let first = sink
        .next(Duration::from_secs(10))
        .expect("the detached flush");
    wait_until("the first batch leaves the spool", || !m.spool().exists());

    m.ok(&[command::STATUS, flag::ROOT, &project]);
    m.ok(&[
        command::APPROVE,
        "G1",
        flag::NOTE,
        "warm palette as the client asked",
    ]);
    m.ok(&[
        command::STACK,
        sub::SET,
        flag::JSON,
        "{\"framework\":{\"name\":\"SecretFramework\"}}",
    ]);
    assert_eq!(m.run(&[command::PHASE, NO_PHASE]).code, 1);
    assert_eq!(m.run(&[NO_COMMAND, &rel::screen(SCREEN_PAGE)]).code, 1);
    let hook_input = json!({
        "hook_event_name": event::PRE_TOOL_USE, "session_id": SECRET_SESSION, "cwd": project,
        "tool_name": tool::WRITE, "tool_input": { "file_path": format!("{project}/{SECRET_COMPONENT}"), "content": SECRET_CONTENT },
    });
    let hook = m.run_with(
        &[command::HOOK, hook_name::PRE_TOOL_USE],
        &hook_input.to_string(),
        &[("CLAUDE_PROJECT_DIR", &project)],
    );
    assert_eq!(hook.code, 0);
    assert!(hook.stdout.contains(wire::reply::DENY), "{}", hook.stdout);
    m.run_with(
        &[
            command::HOOK,
            hook_name::SESSION_START,
            flag::HOST,
            host_id::COPILOT,
        ],
        &json!({ "cwd": project }).to_string(),
        &[],
    );

    // `show` prints what the flush then sends.
    let shown = m.shown();
    assert_eq!(
        m.ok(&[command::TELEMETRY, sub::FLUSH]),
        format!("Sent {} events.\n", shown.len())
    );
    let second = sink
        .next(Duration::from_secs(10))
        .expect("this test's flush");
    let sent: Value = serde_json::from_str(&second.body).unwrap();
    assert_eq!(records(&sent), shown);
    assert!(!m.spool().exists(), "what was accepted left the spool");
    assert!(m.shown().is_empty());

    // The request.
    for request in [&first, &second] {
        assert_eq!(request.line, "POST /v1/logs HTTP/1.1");
        assert!(request
            .headers
            .iter()
            .any(|h| h.eq_ignore_ascii_case("content-type: application/json")));
    }

    // Valid OTLP logs: one resource with the five attributes, typed values only.
    let first_body: Value = serde_json::from_str(&first.body).unwrap();
    let id = fs::read_to_string(m.telemetry_dir().join(telemetry_file::ID)).unwrap();
    for body in [&first_body, &sent] {
        assert_eq!(body[otlp::RESOURCE_LOGS].as_array().unwrap().len(), 1);
        let resource = body[otlp::RESOURCE_LOGS][0][otlp::RESOURCE][otlp::ATTRIBUTES]
            .as_array()
            .unwrap();
        let pairs: Vec<(&str, &str)> = resource
            .iter()
            .map(|a| {
                (
                    a[otlp::KEY].as_str().unwrap(),
                    a[otlp::VALUE][otlp::STRING_VALUE].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(pairs[0], (attr::SERVICE_NAME, "dfw"));
        assert_eq!(pairs[1].0, attr::SERVICE_VERSION);
        assert_eq!(pairs[2].0, attr::OS_TYPE);
        assert_eq!(pairs[3].0, attr::HOST_ARCH);
        assert_eq!(pairs[4], (attr::INSTALL_ID, id.trim()));
        assert_eq!(pairs.len(), 5);
        for record in records(body) {
            let keys: Vec<&str> = record
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect();
            assert_eq!(
                keys,
                [
                    otlp::TIME_UNIX_NANO,
                    otlp::SEVERITY_NUMBER,
                    otlp::SEVERITY_TEXT,
                    otlp::EVENT_NAME,
                    otlp::BODY,
                    otlp::ATTRIBUTES
                ]
            );
            let nanos = record[otlp::TIME_UNIX_NANO].as_str().unwrap();
            assert!(
                nanos.len() == 19 && nanos.bytes().all(|b| b.is_ascii_digit()),
                "{nanos}"
            );
            assert_eq!(
                record[otlp::BODY][otlp::STRING_VALUE],
                record[otlp::EVENT_NAME]
            );
            for a in record[otlp::ATTRIBUTES].as_array().unwrap() {
                assert!(a[otlp::KEY].as_str().unwrap().starts_with("dfw."));
                let value = a[otlp::VALUE].as_object().unwrap();
                assert_eq!(value.len(), 1);
                let (kind, v) = value.iter().next().unwrap();
                assert!(
                    kind == otlp::STRING_VALUE || kind == otlp::INT_VALUE,
                    "{kind}"
                );
                assert!(v.is_string());
            }
        }
    }

    // What was said: names from the command table, buckets, codes, ids; an unknown first
    // argument is "unknown".
    let said = |text: &[(&str, &str)]| -> Vec<(String, String)> {
        text.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    };
    assert_eq!(
        summary(&records(&first_body)),
        said(&[
            (recorded::COMMAND, "init"),
            (recorded::WORKFLOW, names::mode::NEW_PRODUCT)
        ])
    );
    assert_eq!(
        summary(&shown),
        said(&[
            (recorded::COMMAND, "status"),
            (recorded::COMMAND, "approve"),
            (recorded::GATE_APPROVED, "G1"),
            (recorded::WORKFLOW, names::mode::NEW_PRODUCT),
            (recorded::COMMAND, "stack"),
            (recorded::COMMAND, "phase"),
            (recorded::COMMAND, "unknown"),
            (recorded::HOOK, "pre-tool-use"),
            (recorded::HOOK, "session-start"),
        ])
    );
    let string = |r: &Value, key: &str| {
        attribute(r, key).unwrap()[otlp::STRING_VALUE]
            .as_str()
            .unwrap()
            .to_string()
    };
    let int = |r: &Value, key: &str| {
        attribute(r, key).unwrap()[otlp::INT_VALUE]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(int(&shown[0], attr::EXIT_CODE), "0");
    assert!(string(&shown[0], attr::DURATION_BUCKET).starts_with("lt_"));
    assert_eq!(string(&shown[3], attr::PHASE), "discovery");
    assert_eq!(int(&shown[5], attr::EXIT_CODE), "1");
    assert_eq!(string(&shown[7], attr::HOST), host_id::CLAUDE);
    assert_eq!(string(&shown[7], attr::HOOK_OUTCOME), wire::reply::DENY);
    assert_eq!(string(&shown[8], attr::HOST), host_id::COPILOT);

    // Nothing of the project: not its path or folder name, no file name, no argument value.
    let base_name = m
        .project
        .parent()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let home = m.home.to_string_lossy().into_owned();
    let design_folder = format!("{}/", rel::DESIGN);
    let secrets = [
        // the folder and its name, a file name, argument values, anything that looks like a path or a URL
        project.as_str(),
        home.as_str(),
        base_name.as_str(),
        "acme-secret-project",
        "Acme",
        "Secret",
        "warm palette",
        "client",
        "SecretFramework",
        "framework",
        NO_PHASE,
        NO_COMMAND,
        design_folder.as_str(),
        "screens",
        "a.html",
        ".html",
        ".tsx",
        "SecretComponent",
        SECRET_CONTENT,
        SECRET_SESSION,
        flag::NOTE,
        flag::ROOT,
        "/Users/",
        "/home/",
        "/tmp",
        "/var/",
        "127.0.0.1",
        "http",
    ];
    for body in [&first.body, &second.body] {
        for secret in &secrets {
            assert!(!body.contains(secret), "{secret:?} is in an event: {body}");
        }
    }
    sink.assert_silent();
}

/// `dfw telemetry off`, `DO_NOT_TRACK=1`, `DFW_TELEMETRY=off` and `CI=true` each stop recording
/// and sending at once.
#[test]
fn each_switch_stops_recording() {
    let sink = Sink::start(Duration::ZERO);
    for (name, value) in [
        ("DO_NOT_TRACK", "1"),
        ("DFW_TELEMETRY", SWITCHED_OFF),
        ("CI", "true"),
    ] {
        let m = Machine::new("switch");
        m.turn_on(&sink.url);
        let m = m.with_env(name, value);
        m.ok(&[command::INIT]);
        m.ok(&[command::STATUS]);
        m.run_with(&[command::HOOK, hook_name::SESSION_START], "{}", &[]);
        assert!(
            !m.telemetry_dir().exists(),
            "{name}={value}: no spool, no install id, no flush"
        );
        let status = m.ok(&[command::TELEMETRY, sub::STATUS]);
        assert!(status.starts_with("off: "), "{status}");
        assert!(status.contains(name), "{status}");
        assert!(m
            .ok(&[command::TELEMETRY, sub::FLUSH])
            .starts_with("Nothing sent: telemetry is off"));
    }

    // `dfw telemetry off` after events were recorded: the install id and the spool go.
    let m = Machine::new("off");
    m.turn_on(NOWHERE);
    m.ok(&[command::INIT]);
    m.ok(&[command::STATUS]);
    assert_eq!(
        m.shown().len(),
        3,
        "recorded while on (the collector is unreachable)"
    );
    assert!(m.telemetry_dir().join(telemetry_file::ID).exists());
    m.ok(&[command::TELEMETRY, sub::OFF]);
    assert!(
        !m.telemetry_dir().exists(),
        "the install id and the spool are deleted"
    );
    m.ok(&[command::STATUS]);
    m.run_with(&[command::HOOK, hook_name::SESSION_START], "{}", &[]);
    assert!(
        !m.telemetry_dir().exists(),
        "no later command records anything"
    );
    assert_eq!(
        m.ok(&[command::TELEMETRY, sub::STATUS]),
        "off: turned off with \"dfw telemetry off\"\n"
    );
    // The address is still set: only the switch keeps it off.
    assert!(m
        .ok(&[command::TELEMETRY, sub::ON])
        .starts_with("on: sending to http://127.0.0.1:1/v1/logs"));
    sink.assert_silent();
}

/// With an address and no choice by the user: the first command prints the notice and records
/// nothing; a hook never counts as the notice.
#[test]
fn nothing_is_recorded_before_the_notice() {
    if !dfw_default_is_on() {
        return;
    }
    let m = Machine::new("notice");
    let quiet = Machine::new("quiet")
        .with_env("DFW_TELEMETRY", SWITCHED_OFF)
        .run(&[command::VERSION]);
    m.ok(&[command::TELEMETRY, sub::ENDPOINT, NOWHERE]);
    assert!(m
        .ok(&[command::TELEMETRY, sub::STATUS])
        .starts_with("off until the next command has printed the notice"));

    // A hook's stderr is not shown to the user: no notice, nothing recorded, nothing marked.
    let hook = m.run_with(&[command::HOOK, hook_name::SESSION_START], "{}", &[]);
    assert_eq!(hook.stderr, "");
    assert!(!m.telemetry_dir().exists());

    let first = m.run(&[command::VERSION]);
    assert_eq!(first.code, 0);
    assert_eq!(first.stdout, quiet.stdout, "stdout is the command's own");
    assert!(first
        .stderr
        .starts_with("dfw: usage telemetry is on for this install."));
    assert!(first.stderr.contains("dfw telemetry off"));
    assert!(first
        .stderr
        .contains("Nothing was recorded before this notice."));
    assert!(m.telemetry_dir().join(telemetry_file::NOTICE).exists());
    assert!(
        !m.spool().exists(),
        "the run that prints the notice records nothing"
    );
    assert!(!m.telemetry_dir().join(telemetry_file::ID).exists());

    let second = m.run(&[command::VERSION]);
    assert_eq!(second.stderr, "", "the notice is printed once");
    assert_eq!(second.stdout, quiet.stdout);
    assert_eq!(
        summary(&m.shown()),
        [(recorded::COMMAND.to_string(), "version".to_string())]
    );
}

/// Whether the build default (`DEFAULT_ON_WITH_ADDRESS`) is opt-out: with an address set and
/// no choice, the state waits for the notice instead of asking for `dfw telemetry on`.
fn dfw_default_is_on() -> bool {
    let m = Machine::new("default");
    m.ok(&[command::TELEMETRY, sub::ENDPOINT, NOWHERE]);
    !m.ok(&[command::TELEMETRY, sub::STATUS])
        .contains("not turned on")
}

/// The command returns before the POST completes: the collector answers after two seconds.
#[test]
fn a_command_does_not_wait_for_the_collector() {
    let sink = Sink::start(Duration::from_secs(2));
    let m = Machine::new("slow");
    m.turn_on(&sink.url);
    let ran = m.run(&[command::VERSION]);
    assert_eq!(ran.code, 0);
    assert!(
        ran.took < Duration::from_millis(1500),
        "the command took {:?}",
        ran.took
    );
    // The request is on its way (or already waiting for its answer) while the command is done.
    let request = sink
        .next(Duration::from_secs(10))
        .expect("the detached flush");
    assert!(request.body.contains(&format!("\"{}\"", recorded::COMMAND)));
    assert!(
        m.spool().exists(),
        "not accepted yet: the event is still in the spool"
    );
    let hook = m.run_with(&[command::HOOK, hook_name::SESSION_START], "{}", &[]);
    assert!(hook.took < Duration::from_millis(1500), "{:?}", hook.took);
    wait_until("the accepted event leaves the spool", || {
        fs::read_to_string(m.spool()).is_ok_and(|left| !left.contains("\"version\""))
    });
    assert_eq!(
        summary(&m.shown()),
        [(recorded::HOOK.to_string(), "session-start".to_string())]
    );
}

/// Telemetry changes neither stdout nor the exit code of a command, on or failing to send.
#[test]
fn output_and_exit_code_are_the_same_with_telemetry_on() {
    let off = Machine::new("same-off").with_env("DFW_TELEMETRY", SWITCHED_OFF);
    let on = Machine::new("same-on");
    on.turn_on(NOWHERE);
    // A telemetry folder that cannot be written: recording fails, the command does not.
    let broken = Machine::new("same-broken");
    broken.turn_on(NOWHERE);
    fs::write(broken.telemetry_dir(), "a file where the folder should be").unwrap();
    let hook_input =
        json!({ "tool_name": tool::WRITE, "tool_input": { "file_path": APP_MODULE } }).to_string();
    let commands = [
        (vec![command::INIT, flag::PRODUCT, "Fixture"], ""),
        (vec![command::STATUS, flag::JSON], ""),
        (vec![command::STATUS], ""),
        (vec![command::PHASE, "nowhere"], ""),
        (vec![command::APPROVE, "G1"], ""),
        (vec![command::APPROVE, "G1", flag::NOTE, "ok"], ""),
        (vec![command::DECISIONS], ""),
        (vec!["nope"], ""),
        (
            vec![command::HOOK, hook_name::PRE_TOOL_USE],
            hook_input.as_str(),
        ),
    ];
    let all = |m: &Machine| -> Vec<(i32, String, String)> {
        // The project as dfw prints it: without the Windows `\\?\` prefix of the test's real
        // path, and JSON-escaped inside `--json` output.
        let shown = m.project.to_string_lossy();
        let shown = shown.strip_prefix(r"\\?\").unwrap_or(&shown).to_string();
        let escaped = serde_json::to_string(&shown).unwrap();
        let escaped = escaped.trim_matches('"').to_string();
        let mask = |text: String| {
            text.replace(&escaped, PROJECT_MASK)
                .replace(&shown, PROJECT_MASK)
        };
        commands
            .iter()
            .map(|(args, stdin)| m.run_with(args, stdin, &[]))
            .map(|ran| (ran.code, mask(ran.stdout), mask(ran.stderr)))
            .collect()
    };
    let expected = all(&off);
    assert_eq!(all(&on), expected);
    assert_eq!(all(&broken), expected);
    assert!(!on.shown().is_empty());
    assert!(broken.telemetry_dir().is_file());
}

/// The address comes from the build: `option_env!` in the crate sees what `build.rs` passed
/// on. This test builds nothing; it compares the binary with the build it came from.
#[test]
fn the_binary_knows_whether_an_address_was_compiled_in() {
    let m = Machine::new("build");
    let built: Value = serde_json::from_str(&m.ok(&[command::DEBUG, "telemetry-build"])).unwrap();
    assert_eq!(built["endpoint"], json!(address_compiled_in()));
    if !address_compiled_in() {
        // An unset variable yields "no address" (telemetry task 1.1).
        assert_eq!(built, json!({ "endpoint": false, "headers": false }));
        assert_eq!(
            m.ok(&[command::TELEMETRY, sub::STATUS]),
            "off: no collector address in this build\n"
        );
    }
}

/// A build without an address: no file under the telemetry folder, and no connection through
/// the HTTP client. Every proxy variable points at a listener of this test, so a request the
/// binary made with its HTTP client, to any host, would arrive there; the last step makes one on
/// purpose to show the trap works.
#[test]
fn a_build_without_an_address_writes_nothing_and_connects_nowhere() {
    if address_compiled_in() {
        return; // this build has an address: the test is about builds that do not
    }
    let trap = Sink::start(Duration::ZERO);
    let m = Machine::new("no-address")
        .with_env("ALL_PROXY", &trap.url)
        .with_env("HTTPS_PROXY", &trap.url)
        .with_env("HTTP_PROXY", &trap.url);
    let project = m.project.to_string_lossy().into_owned();
    let hook_input =
        json!({ "tool_name": tool::WRITE, "tool_input": { "file_path": APP_MODULE } }).to_string();
    for (args, stdin) in [
        (vec![command::VERSION], ""),
        (vec![command::HELP], ""),
        (vec![command::INIT, flag::PRODUCT, "Fixture"], ""),
        (vec![command::STATUS], ""),
        (vec![command::APPROVE, "G1", flag::NOTE, "ok"], ""),
        (vec![command::PHASE, "direction"], ""),
        (vec![command::STACK, "detect", "--write"], ""),
        (vec![command::DOCTOR], ""),
        (vec!["nope"], ""),
        (vec![command::HOOK, hook_name::SESSION_START], "{}"),
        (
            vec![command::HOOK, hook_name::PRE_TOOL_USE],
            hook_input.as_str(),
        ),
        (vec![command::HOOK, hook_name::STOP], "{}"),
        (vec![command::TELEMETRY], ""),
        (vec![command::TELEMETRY, sub::STATUS], ""),
        (vec![command::TELEMETRY, sub::SHOW], ""),
        (vec![command::TELEMETRY, sub::FLUSH], ""),
        (vec![command::TELEMETRY, sub::ON], ""),
        (vec![command::STATUS, flag::ROOT, project.as_str()], ""),
        (vec![command::TELEMETRY, sub::FLUSH], ""),
    ] {
        m.run_with(&args, stdin, &[]);
        assert!(
            !m.telemetry_dir().exists(),
            "dfw {args:?} created {:?}",
            fs::read_dir(m.telemetry_dir())
                .map(|d| d.flatten().map(|e| e.file_name()).collect::<Vec<_>>())
        );
    }
    assert_eq!(
        m.ok(&[command::TELEMETRY, sub::STATUS]),
        "off: no collector address in this build\n"
    );
    trap.assert_silent();

    // The trap does catch the HTTP client: with a collector set, the flush lands in it.
    m.ok(&[
        command::TELEMETRY,
        sub::ENDPOINT,
        "http://collector.invalid:4318",
    ]);
    m.ok(&[command::STATUS]);
    let caught = trap
        .next(Duration::from_secs(10))
        .expect("the flush goes through the proxy");
    // The HTTP client asks the proxy for a tunnel to the collector.
    assert_eq!(caught.line, "CONNECT collector.invalid:4318 HTTP/1.1");
}
