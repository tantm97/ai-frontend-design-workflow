//! Telemetry through the built binary (the `telemetry` OpenSpec change): what reaches a
//! collector, what stops it, and that a command never waits for it. The collector is a
//! `TcpListener` in the test; the binary is pointed at it with `dfw telemetry endpoint`, which
//! stands in for an address compiled in (one decision covers both, see `telemetry::address`).

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

const DFW: &str = env!("CARGO_BIN_EXE_dfw");
/// What the build script passed on when these tests were compiled (the same build as the binary).
const BUILT_WITH_ENDPOINT: Option<&str> = option_env!("DFW_OTLP_ENDPOINT");
/// A local address nothing listens on: a flush to it fails at once and the spool stays.
const NOWHERE: &str = "http://127.0.0.1:1";

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
        self.telemetry_dir().join("spool.jsonl")
    }

    /// `dfw <args>` in the project. Nothing of the session that runs the tests reaches the
    /// binary: not its project, not its telemetry switches, not its proxy.
    fn run_with(&self, args: &[&str], stdin: &str, extra: &[(&str, &str)]) -> Ran {
        let mut cmd = Command::new(DFW);
        cmd.args(args).current_dir(&self.project);
        for name in [
            "DFW_NATIVE",
            "DFW_HOST",
            "DFW_PROJECT_DIR",
            "CLAUDE_PROJECT_DIR",
            "DFW_SESSION_ID",
            "CLAUDE_SESSION_ID",
            "CLAUDE_ENV_FILE",
            "XDG_CONFIG_HOME",
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
        cmd.env("DFW_HOME", &self.home)
            .env("DFW_PLUGIN_ROOT", repo().join("core"))
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
        self.ok(&["telemetry", "endpoint", endpoint]);
        let said = self.ok(&["telemetry", "on"]);
        assert!(said.starts_with("on: sending to "), "{said}");
    }

    /// The log records `dfw telemetry show` prints.
    fn shown(&self) -> Vec<Value> {
        let text = self.ok(&["telemetry", "show"]);
        if text.starts_with("No events") {
            return Vec::new();
        }
        records(&serde_json::from_str(&text).unwrap())
    }
}

fn records(body: &Value) -> Vec<Value> {
    body["resourceLogs"][0]["scopeLogs"][0]["logRecords"]
        .as_array()
        .unwrap()
        .clone()
}

fn attribute<'a>(record: &'a Value, key: &str) -> Option<&'a Value> {
    record["attributes"]
        .as_array()?
        .iter()
        .find(|a| a["key"] == key)
        .map(|a| &a["value"])
}

/// `(event name, the value of its first attribute)` of each record.
fn summary(records: &[Value]) -> Vec<(String, String)> {
    records
        .iter()
        .map(|r| {
            let first = &r["attributes"][0]["value"];
            let value = first["stringValue"]
                .as_str()
                .or(first["intValue"].as_str())
                .unwrap();
            (r["eventName"].as_str().unwrap().to_string(), value.into())
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
fn events_arrive_as_otlp_json_and_carry_no_content() {
    let sink = Sink::start(Duration::ZERO);
    let m = Machine::new("otlp");
    m.turn_on(&sink.url);
    let project = m.project.to_string_lossy().into_owned();

    // The first recorded command starts the detached flush: wait for it, so the rest of the
    // events leave in one request of this test's own flush.
    m.ok(&["init", "--product", "Acme Secret Shop"]);
    let first = sink
        .next(Duration::from_secs(10))
        .expect("the detached flush");
    wait_until("the first batch leaves the spool", || !m.spool().exists());

    m.ok(&["status", "--root", &project]);
    m.ok(&[
        "approve",
        "G1",
        "--note",
        "warm palette as the client asked",
    ]);
    m.ok(&[
        "stack",
        "set",
        "--json",
        "{\"framework\":{\"name\":\"SecretFramework\"}}",
    ]);
    assert_eq!(m.run(&["phase", "nowhere-phase"]).code, 1);
    assert_eq!(
        m.run(&["no-such-command", "design/screens/home/a.html"])
            .code,
        1
    );
    let hook_input = json!({
        "hook_event_name": "PreToolUse", "session_id": "session-secret-1", "cwd": project,
        "tool_name": "Write", "tool_input": { "file_path": format!("{project}/src/SecretComponent.tsx"), "content": "secret file content" },
    });
    let hook = m.run_with(
        &["hook", "pre-tool-use"],
        &hook_input.to_string(),
        &[("CLAUDE_PROJECT_DIR", &project)],
    );
    assert_eq!(hook.code, 0);
    assert!(hook.stdout.contains("deny"), "{}", hook.stdout);
    m.run_with(
        &["hook", "session-start", "--host", "copilot"],
        &json!({ "cwd": project }).to_string(),
        &[],
    );

    // `show` prints what the flush then sends.
    let shown = m.shown();
    assert_eq!(
        m.ok(&["telemetry", "flush"]),
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
    let id = fs::read_to_string(m.telemetry_dir().join("id")).unwrap();
    for body in [&first_body, &sent] {
        assert_eq!(body["resourceLogs"].as_array().unwrap().len(), 1);
        let resource = body["resourceLogs"][0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        let pairs: Vec<(&str, &str)> = resource
            .iter()
            .map(|a| {
                (
                    a["key"].as_str().unwrap(),
                    a["value"]["stringValue"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(pairs[0], ("service.name", "dfw"));
        assert_eq!(pairs[1].0, "service.version");
        assert_eq!(pairs[2].0, "os.type");
        assert_eq!(pairs[3].0, "host.arch");
        assert_eq!(pairs[4], ("dfw.install_id", id.trim()));
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
                    "timeUnixNano",
                    "severityNumber",
                    "severityText",
                    "eventName",
                    "body",
                    "attributes"
                ]
            );
            let nanos = record["timeUnixNano"].as_str().unwrap();
            assert!(
                nanos.len() == 19 && nanos.bytes().all(|b| b.is_ascii_digit()),
                "{nanos}"
            );
            assert_eq!(record["body"]["stringValue"], record["eventName"]);
            for a in record["attributes"].as_array().unwrap() {
                assert!(a["key"].as_str().unwrap().starts_with("dfw."));
                let value = a["value"].as_object().unwrap();
                assert_eq!(value.len(), 1);
                let (kind, v) = value.iter().next().unwrap();
                assert!(kind == "stringValue" || kind == "intValue", "{kind}");
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
        said(&[("dfw.command", "init"), ("dfw.workflow", "new-product")])
    );
    assert_eq!(
        summary(&shown),
        said(&[
            ("dfw.command", "status"),
            ("dfw.command", "approve"),
            ("dfw.gate_approved", "G1"),
            ("dfw.workflow", "new-product"),
            ("dfw.command", "stack"),
            ("dfw.command", "phase"),
            ("dfw.command", "unknown"),
            ("dfw.hook", "pre-tool-use"),
            ("dfw.hook", "session-start"),
        ])
    );
    let string = |r: &Value, key: &str| {
        attribute(r, key).unwrap()["stringValue"]
            .as_str()
            .unwrap()
            .to_string()
    };
    let int = |r: &Value, key: &str| {
        attribute(r, key).unwrap()["intValue"]
            .as_str()
            .unwrap()
            .to_string()
    };
    assert_eq!(int(&shown[0], "dfw.exit_code"), "0");
    assert!(string(&shown[0], "dfw.duration_bucket").starts_with("lt_"));
    assert_eq!(string(&shown[3], "dfw.phase"), "discovery");
    assert_eq!(int(&shown[5], "dfw.exit_code"), "1");
    assert_eq!(string(&shown[7], "dfw.host"), "claude");
    assert_eq!(string(&shown[7], "dfw.hook_outcome"), "deny");
    assert_eq!(string(&shown[8], "dfw.host"), "copilot");

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
        "nowhere-phase",
        "no-such-command",
        "design/",
        "screens",
        "a.html",
        ".html",
        ".tsx",
        "SecretComponent",
        "secret file content",
        "session-secret-1",
        "--note",
        "--root",
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
        ("DFW_TELEMETRY", "off"),
        ("CI", "true"),
    ] {
        let m = Machine::new("switch");
        m.turn_on(&sink.url);
        let m = m.with_env(name, value);
        m.ok(&["init"]);
        m.ok(&["status"]);
        m.run_with(&["hook", "session-start"], "{}", &[]);
        assert!(
            !m.telemetry_dir().exists(),
            "{name}={value}: no spool, no install id, no flush"
        );
        let status = m.ok(&["telemetry", "status"]);
        assert!(status.starts_with("off: "), "{status}");
        assert!(status.contains(name), "{status}");
        assert!(m
            .ok(&["telemetry", "flush"])
            .starts_with("Nothing sent: telemetry is off"));
    }

    // `dfw telemetry off` after events were recorded: the install id and the spool go.
    let m = Machine::new("off");
    m.turn_on(NOWHERE);
    m.ok(&["init"]);
    m.ok(&["status"]);
    assert_eq!(
        m.shown().len(),
        3,
        "recorded while on (the collector is unreachable)"
    );
    assert!(m.telemetry_dir().join("id").exists());
    m.ok(&["telemetry", "off"]);
    assert!(
        !m.telemetry_dir().exists(),
        "the install id and the spool are deleted"
    );
    m.ok(&["status"]);
    m.run_with(&["hook", "session-start"], "{}", &[]);
    assert!(
        !m.telemetry_dir().exists(),
        "no later command records anything"
    );
    assert_eq!(
        m.ok(&["telemetry", "status"]),
        "off: turned off with \"dfw telemetry off\"\n"
    );
    // The address is still set: only the switch keeps it off.
    assert!(m
        .ok(&["telemetry", "on"])
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
        .with_env("DFW_TELEMETRY", "off")
        .run(&["version"]);
    m.ok(&["telemetry", "endpoint", NOWHERE]);
    assert!(m
        .ok(&["telemetry", "status"])
        .starts_with("off until the next command has printed the notice"));

    // A hook's stderr is not shown to the user: no notice, nothing recorded, nothing marked.
    let hook = m.run_with(&["hook", "session-start"], "{}", &[]);
    assert_eq!(hook.stderr, "");
    assert!(!m.telemetry_dir().exists());

    let first = m.run(&["version"]);
    assert_eq!(first.code, 0);
    assert_eq!(first.stdout, quiet.stdout, "stdout is the command's own");
    assert!(first
        .stderr
        .starts_with("dfw: usage telemetry is on for this install."));
    assert!(first.stderr.contains("dfw telemetry off"));
    assert!(first
        .stderr
        .contains("Nothing was recorded before this notice."));
    assert!(m.telemetry_dir().join("notice").exists());
    assert!(
        !m.spool().exists(),
        "the run that prints the notice records nothing"
    );
    assert!(!m.telemetry_dir().join("id").exists());

    let second = m.run(&["version"]);
    assert_eq!(second.stderr, "", "the notice is printed once");
    assert_eq!(second.stdout, quiet.stdout);
    assert_eq!(
        summary(&m.shown()),
        [("dfw.command".to_string(), "version".to_string())]
    );
}

/// Whether the build default (`DEFAULT_ON_WITH_ADDRESS`) is opt-out: with an address set and
/// no choice, the state waits for the notice instead of asking for `dfw telemetry on`.
fn dfw_default_is_on() -> bool {
    let m = Machine::new("default");
    m.ok(&["telemetry", "endpoint", NOWHERE]);
    !m.ok(&["telemetry", "status"]).contains("not turned on")
}

/// The command returns before the POST completes: the collector answers after two seconds.
#[test]
fn a_command_does_not_wait_for_the_collector() {
    let sink = Sink::start(Duration::from_secs(2));
    let m = Machine::new("slow");
    m.turn_on(&sink.url);
    let ran = m.run(&["version"]);
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
    assert!(request.body.contains("\"dfw.command\""));
    assert!(
        m.spool().exists(),
        "not accepted yet: the event is still in the spool"
    );
    let hook = m.run_with(&["hook", "session-start"], "{}", &[]);
    assert!(hook.took < Duration::from_millis(1500), "{:?}", hook.took);
    wait_until("the accepted event leaves the spool", || {
        fs::read_to_string(m.spool()).is_ok_and(|left| !left.contains("\"version\""))
    });
    assert_eq!(
        summary(&m.shown()),
        [("dfw.hook".to_string(), "session-start".to_string())]
    );
}

/// Telemetry changes neither stdout nor the exit code of a command, on or failing to send.
#[test]
fn output_and_exit_code_are_the_same_with_telemetry_on() {
    let off = Machine::new("same-off").with_env("DFW_TELEMETRY", "off");
    let on = Machine::new("same-on");
    on.turn_on(NOWHERE);
    // A telemetry folder that cannot be written: recording fails, the command does not.
    let broken = Machine::new("same-broken");
    broken.turn_on(NOWHERE);
    fs::write(broken.telemetry_dir(), "a file where the folder should be").unwrap();
    let hook_input =
        json!({ "tool_name": "Write", "tool_input": { "file_path": "src/app.ts" } }).to_string();
    let commands = [
        (vec!["init", "--product", "Fixture"], ""),
        (vec!["status", "--json"], ""),
        (vec!["status"], ""),
        (vec!["phase", "nowhere"], ""),
        (vec!["approve", "G1"], ""),
        (vec!["approve", "G1", "--note", "ok"], ""),
        (vec!["decisions"], ""),
        (vec!["nope"], ""),
        (vec!["hook", "pre-tool-use"], hook_input.as_str()),
    ];
    let all = |m: &Machine| -> Vec<(i32, String, String)> {
        let mask = |text: String| text.replace(&*m.project.to_string_lossy(), "<project>");
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
    let built: Value = serde_json::from_str(&m.ok(&["__debug", "telemetry-build"])).unwrap();
    assert_eq!(built["endpoint"], json!(address_compiled_in()));
    if !address_compiled_in() {
        // An unset variable yields "no address" (telemetry task 1.1).
        assert_eq!(built, json!({ "endpoint": false, "headers": false }));
        assert_eq!(
            m.ok(&["telemetry", "status"]),
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
        json!({ "tool_name": "Write", "tool_input": { "file_path": "src/app.ts" } }).to_string();
    for (args, stdin) in [
        (vec!["version"], ""),
        (vec!["help"], ""),
        (vec!["init", "--product", "Fixture"], ""),
        (vec!["status"], ""),
        (vec!["approve", "G1", "--note", "ok"], ""),
        (vec!["phase", "direction"], ""),
        (vec!["stack", "detect", "--write"], ""),
        (vec!["doctor"], ""),
        (vec!["nope"], ""),
        (vec!["hook", "session-start"], "{}"),
        (vec!["hook", "pre-tool-use"], hook_input.as_str()),
        (vec!["hook", "stop"], "{}"),
        (vec!["telemetry"], ""),
        (vec!["telemetry", "status"], ""),
        (vec!["telemetry", "show"], ""),
        (vec!["telemetry", "flush"], ""),
        (vec!["telemetry", "on"], ""),
        (vec!["status", "--root", project.as_str()], ""),
        (vec!["telemetry", "flush"], ""),
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
        m.ok(&["telemetry", "status"]),
        "off: no collector address in this build\n"
    );
    trap.assert_silent();

    // The trap does catch the HTTP client: with a collector set, the flush lands in it.
    m.ok(&["telemetry", "endpoint", "http://collector.invalid:4318"]);
    m.ok(&["status"]);
    let caught = trap
        .next(Duration::from_secs(10))
        .expect("the flush goes through the proxy");
    // The HTTP client asks the proxy for a tunnel to the collector.
    assert_eq!(caught.line, "CONNECT collector.invalid:4318 HTTP/1.1");
}
