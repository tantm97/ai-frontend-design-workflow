//! Usage telemetry (the `telemetry` OpenSpec change): content-free events about how dfw is
//! used, sent as OTLP/HTTP JSON logs to a collector whose address is compiled into release
//! binaries or set by the user. A binary without an address records nothing, writes no file and
//! opens no connection.
//!
//! A command never waits for the network: `record` appends to a spool file, and at most once a
//! minute starts `dfw telemetry flush` as a separate process that it does not wait for.
//! Nothing here may change what a command prints or returns: every error is swallowed.

pub mod events;
pub mod flush;
pub mod otlp;
pub mod spool;

use std::env;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;

use crate::cli::Ctx;
use crate::core::config::global_config_path;
use crate::core::host::current_host;
use crate::core::util::{get_path, read_json, ArgValue};
use crate::workflow::state::load_state;
use events::{
    CommandName, DurationBucket, Event, GateId, HookName, HookOutcome, HostId, ModeName, PhaseName,
    Spooled,
};

/// What happens when a collector address exists and the user has chosen nothing: `true` is
/// opt-out (on after the one-time notice), `false` is opt-in (off until `dfw telemetry on`).
/// The open question of the telemetry design; the maintainer decides it here.
pub const DEFAULT_ON_WITH_ADDRESS: bool = true;

/// Printed once on stderr, by the first command that runs with a collector address. Nothing is
/// recorded before it.
pub const NOTICE: &str = "\
dfw: usage telemetry is on for this install.
  Sent: a random install id, dfw version, OS and CPU type, host, command and hook names,
  a duration range, exit code, workflow mode and phase, approved gate ids, check names with
  finding counts.
  Never sent: project names, paths, file names or contents, comments, design tokens, URLs,
  usernames, environment variables.
  See what is waiting to be sent: dfw telemetry show
  Turn it off: dfw telemetry off   (or set DO_NOT_TRACK=1 or DFW_TELEMETRY=off)
  Nothing was recorded before this notice.";

/// Commands that change the workflow's mode or phase.
const WORKFLOW_COMMANDS: [&str; 7] = [
    "init", "mode", "phase", "approve", "reopen", "pause", "resume",
];

/// Where an address came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    /// Compiled in (`DFW_OTLP_ENDPOINT` at build time).
    Build,
    /// `dfw telemetry endpoint <url>`.
    User,
}

/// A collector to send to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Address {
    /// The full URL of the logs endpoint (`.../v1/logs`).
    pub url: String,
    /// Extra request headers; only ever the compiled ones, and only for the compiled address.
    pub headers: Vec<(String, String)>,
    pub source: Source,
}

/// Why telemetry is off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OffReason {
    DoNotTrack,
    EnvOff,
    Ci,
    UserOff,
    NoAddress,
    /// An address exists but `DEFAULT_ON_WITH_ADDRESS` is false and the user did not turn it on.
    NotOptedIn,
    /// The global config file cannot be read, so the user's choice is unknown.
    ConfigUnreadable,
}

/// The answer to "is it on".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Off(OffReason),
    /// It would be on, but the one-time notice was not shown yet: nothing is recorded.
    NoticeDue(Address),
    On(Address),
}

/// Everything the decision looks at.
#[derive(Debug, Clone, Default)]
pub struct Inputs {
    pub do_not_track: Option<String>,
    pub dfw_telemetry: Option<String>,
    pub ci: Option<String>,
    /// `telemetry.enabled` of the global config, when it is a boolean.
    pub enabled: Option<bool>,
    pub address: Option<Address>,
    pub notice_shown: bool,
    pub default_on: bool,
}

/// An environment switch that is set to something other than "no".
fn switched_on(value: Option<&str>) -> bool {
    value.is_some_and(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "0" | "false"))
}

/// The decision, in the order of telemetry design decision 5: the environment (`DO_NOT_TRACK`,
/// `DFW_TELEMETRY=off`, `CI`), then the user's setting, then whether an address exists, then
/// the one-time notice.
pub fn decide(inputs: &Inputs) -> State {
    if switched_on(inputs.do_not_track.as_deref()) {
        return State::Off(OffReason::DoNotTrack);
    }
    let env_off = inputs.dfw_telemetry.as_deref().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "off" | "0" | "false" | "no"
        )
    });
    if env_off {
        return State::Off(OffReason::EnvOff);
    }
    if switched_on(inputs.ci.as_deref()) {
        return State::Off(OffReason::Ci);
    }
    match (inputs.enabled, inputs.address.clone()) {
        (Some(false), _) => State::Off(OffReason::UserOff),
        (_, None) => State::Off(OffReason::NoAddress),
        // The user turned it on: that is the consent the notice stands in for.
        (Some(true), Some(address)) => State::On(address),
        (None, Some(_)) if !inputs.default_on => State::Off(OffReason::NotOptedIn),
        (None, Some(address)) if inputs.notice_shown => State::On(address),
        (None, Some(address)) => State::NoticeDue(address),
    }
}

/// The collector address compiled into this binary, if any.
pub fn compiled_endpoint() -> Option<&'static str> {
    option_env!("DFW_OTLP_ENDPOINT")
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// The request headers compiled into this binary, if any (`name=value,name=value`).
pub fn compiled_headers() -> Option<&'static str> {
    option_env!("DFW_OTLP_HEADERS")
        .map(str::trim)
        .filter(|v| !v.is_empty())
}

/// The logs URL of a collector address: `http(s)://host[/base]`, with `/v1/logs` added unless
/// it is there already. `None` for anything else.
pub fn logs_url(endpoint: &str) -> Option<String> {
    let endpoint = endpoint.trim().trim_end_matches('/');
    let rest = endpoint
        .strip_prefix("https://")
        .or_else(|| endpoint.strip_prefix("http://"))?;
    if rest.is_empty()
        || rest.starts_with('/')
        || endpoint
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    Some(if endpoint.ends_with("/v1/logs") {
        endpoint.to_string()
    } else {
        format!("{endpoint}/v1/logs")
    })
}

fn parse_headers(text: &str) -> Vec<(String, String)> {
    text.split(',')
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, value)| (name.trim().to_string(), value.trim().to_string()))
        .filter(|(name, _)| !name.is_empty())
        .collect()
}

/// The address events go to: the user's own collector when one is set (then only that one,
/// and the compiled headers stay home), else the compiled one.
fn address(user_endpoint: Option<&str>) -> Option<Address> {
    if let Some(own) = user_endpoint.filter(|e| !e.trim().is_empty()) {
        return logs_url(own).map(|url| Address {
            url,
            headers: Vec::new(),
            source: Source::User,
        });
    }
    let url = logs_url(compiled_endpoint()?)?;
    Some(Address {
        url,
        headers: compiled_headers().map(parse_headers).unwrap_or_default(),
        source: Source::Build,
    })
}

/// The state for this process: environment, global config and the telemetry folder.
pub fn current(dir: &Path) -> State {
    let Ok(config) = read_json(&global_config_path(), Some(Value::Null)) else {
        return State::Off(OffReason::ConfigUnreadable);
    };
    let user_endpoint = get_path(&config, "telemetry.endpoint").and_then(Value::as_str);
    decide(&Inputs {
        do_not_track: env::var("DO_NOT_TRACK").ok(),
        dfw_telemetry: env::var("DFW_TELEMETRY").ok(),
        ci: env::var("CI").ok(),
        enabled: get_path(&config, "telemetry.enabled").and_then(Value::as_bool),
        address: address(user_endpoint),
        notice_shown: spool::notice_shown(dir),
        default_on: DEFAULT_ON_WITH_ADDRESS,
    })
}

/// The state in words (`dfw telemetry status`, `dfw doctor`).
pub fn describe(state: &State) -> String {
    let target = |a: &Address| match a.source {
        Source::Build => format!("{} (the address built into this binary)", a.url),
        Source::User => format!("{} (set with \"dfw telemetry endpoint\")", a.url),
    };
    match state {
        State::Off(OffReason::DoNotTrack) => "off: DO_NOT_TRACK is set".into(),
        State::Off(OffReason::EnvOff) => "off: DFW_TELEMETRY=off".into(),
        State::Off(OffReason::Ci) => "off: CI is set".into(),
        State::Off(OffReason::UserOff) => "off: turned off with \"dfw telemetry off\"".into(),
        State::Off(OffReason::NoAddress) => "off: no collector address in this build".into(),
        State::Off(OffReason::NotOptedIn) => {
            "off: not turned on (\"dfw telemetry on\" turns it on)".into()
        }
        State::Off(OffReason::ConfigUnreadable) => {
            "off: the global dfw config file cannot be read".into()
        }
        State::NoticeDue(a) => format!(
            "off until the next command has printed the notice; then on, sending to {}",
            target(a)
        ),
        State::On(a) => format!("on: sending to {}", target(a)),
    }
}

/// One line for `dfw doctor`.
pub fn status_line() -> String {
    describe(&current(&spool::dir()))
}

/// Start `dfw telemetry flush` as a process of its own and do not wait for it.
fn spawn_flush() {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let mut flush = Command::new(exe);
    flush
        .args(["telemetry", "flush"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    // A group of its own: the flush outlives this command and is not stopped with it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        flush.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
        flush.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }
    let _ = flush.spawn();
}

/// Record the events `build` returns, when telemetry is on. `may_show_notice` is false where
/// the user would not see stderr (hooks, the MCP server): such a run never counts as the notice.
fn record_with(may_show_notice: bool, build: impl FnOnce() -> Vec<Event>) {
    let dir = spool::dir();
    match current(&dir) {
        State::Off(_) => {}
        State::NoticeDue(_) => {
            if may_show_notice && writeln!(std::io::stderr(), "{NOTICE}").is_ok() {
                let _ = spool::mark_notice_shown(&dir);
            }
        }
        State::On(_) => {
            let at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            let events: Vec<Spooled> = build()
                .into_iter()
                .map(|event| Spooled { at, event })
                .collect();
            if events.is_empty() || spool::install_id(&dir, true).is_none() {
                return;
            }
            if spool::append(&dir, &events).unwrap_or(false) {
                spawn_flush();
            }
        }
    }
}

/// Record one event (for the code that knows it: a check run reports its counts with this).
pub fn record(event: Event) {
    record_with(false, || vec![event]);
}

/// Record point of `main`: a command ended. `name` is looked up in the command table; the
/// arguments are read only for a gate id and the project whose mode and phase are reported.
pub fn command_finished(name: &str, ctx: &Ctx, started: Instant, code: i32) {
    // Hooks record themselves; the telemetry and debug commands are not usage.
    if name == "hook" || name == "telemetry" || name.starts_with("__") {
        return;
    }
    record_with(name != "mcp", || {
        let mut events = vec![Event::Command {
            name: CommandName::or_unknown(name),
            duration: DurationBucket::of(started.elapsed()),
            exit: code,
        }];
        if code == 0 && name == "approve" {
            let gate = ctx.args.pos(1).and_then(GateId::new);
            events.extend(gate.map(|gate| Event::Gate { gate }));
        }
        if code == 0 && WORKFLOW_COMMANDS.contains(&name) {
            events.extend(workflow_event(&ctx.root()));
        }
        events
    });
}

/// The mode and phase of a project, when both are names this binary knows.
fn workflow_event(root: &Path) -> Option<Event> {
    let state = load_state(root).ok()??;
    let name = |key: &str| state.get(key).and_then(Value::as_str);
    Some(Event::Workflow {
        mode: ModeName::new(name("mode")?)?,
        phase: PhaseName::new(name("phase")?)?,
    })
}

/// Record point of the hook dispatcher: a hook call ended.
pub fn hook_finished(name: &str, host: Option<&ArgValue>, started: Instant, outcome: HookOutcome) {
    record_with(false, || {
        let Some(name) = HookName::new(name) else {
            return Vec::new();
        };
        let host = current_host(host).ok().map(|h| h.id);
        vec![Event::Hook {
            name,
            host: HostId::or_unknown(host),
            duration: DurationBucket::of(started.elapsed()),
            outcome,
        }]
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn somewhere() -> Address {
        Address {
            url: "https://collector.example/v1/logs".into(),
            headers: Vec::new(),
            source: Source::Build,
        }
    }

    /// The decision, row by row, in the order of the design (telemetry task 2.2).
    #[test]
    fn the_decision_follows_the_order_of_the_design() {
        let on_by_all_else = || Inputs {
            enabled: Some(true),
            address: Some(somewhere()),
            notice_shown: true,
            default_on: true,
            ..Inputs::default()
        };
        // 1. The environment wins over everything, the user's "on" included.
        let with = |change: fn(&mut Inputs)| {
            let mut inputs = on_by_all_else();
            change(&mut inputs);
            decide(&inputs)
        };
        assert_eq!(with(|_| {}), State::On(somewhere()));
        assert_eq!(
            with(|i| i.do_not_track = Some("1".into())),
            State::Off(OffReason::DoNotTrack)
        );
        assert_eq!(
            with(|i| i.do_not_track = Some("true".into())),
            State::Off(OffReason::DoNotTrack)
        );
        assert_eq!(
            with(|i| i.do_not_track = Some("0".into())),
            State::On(somewhere())
        );
        assert_eq!(
            with(|i| i.do_not_track = Some(String::new())),
            State::On(somewhere())
        );
        assert_eq!(
            with(|i| i.dfw_telemetry = Some("off".into())),
            State::Off(OffReason::EnvOff)
        );
        assert_eq!(
            with(|i| i.dfw_telemetry = Some("OFF".into())),
            State::Off(OffReason::EnvOff)
        );
        assert_eq!(
            with(|i| i.dfw_telemetry = Some("on".into())),
            State::On(somewhere())
        );
        assert_eq!(
            with(|i| i.ci = Some("true".into())),
            State::Off(OffReason::Ci)
        );
        assert_eq!(with(|i| i.ci = Some("1".into())), State::Off(OffReason::Ci));
        assert_eq!(
            with(|i| i.ci = Some("false".into())),
            State::On(somewhere())
        );
        // DFW_TELEMETRY=on does not turn on what the user or the build left off.
        assert_eq!(
            with(|i| {
                i.dfw_telemetry = Some("on".into());
                i.enabled = Some(false);
            }),
            State::Off(OffReason::UserOff)
        );

        // 2. The user's setting.
        assert_eq!(
            with(|i| i.enabled = Some(false)),
            State::Off(OffReason::UserOff)
        );
        assert_eq!(
            with(|i| {
                i.enabled = Some(true);
                i.notice_shown = false;
            }),
            State::On(somewhere()),
            "the user's own \"on\" needs no notice"
        );
        assert_eq!(
            with(|i| {
                i.enabled = Some(true);
                i.default_on = false;
            }),
            State::On(somewhere())
        );
        assert_eq!(
            with(|i| i.address = None),
            State::Off(OffReason::NoAddress),
            "on, but nowhere to send"
        );

        // 3. No setting: an address decides, 4. after the notice.
        let unset = |change: fn(&mut Inputs)| {
            let mut inputs = on_by_all_else();
            inputs.enabled = None;
            change(&mut inputs);
            decide(&inputs)
        };
        assert_eq!(unset(|_| {}), State::On(somewhere()));
        assert_eq!(
            unset(|i| i.notice_shown = false),
            State::NoticeDue(somewhere())
        );
        assert_eq!(
            unset(|i| i.address = None),
            State::Off(OffReason::NoAddress)
        );
        assert_eq!(
            unset(|i| i.default_on = false),
            State::Off(OffReason::NotOptedIn),
            "the opt-in variant of the open question"
        );
        assert_eq!(decide(&Inputs::default()), State::Off(OffReason::NoAddress));
    }

    #[test]
    fn a_collector_address_becomes_its_logs_url() {
        assert_eq!(
            logs_url("https://otel.example.org").as_deref(),
            Some("https://otel.example.org/v1/logs")
        );
        assert_eq!(
            logs_url(" http://127.0.0.1:4318/ ").as_deref(),
            Some("http://127.0.0.1:4318/v1/logs")
        );
        assert_eq!(
            logs_url("https://h/otlp/v1/logs").as_deref(),
            Some("https://h/otlp/v1/logs")
        );
        for bad in [
            "",
            "otel.example.org",
            "ftp://h",
            "https://",
            "https:///x",
            "https://h/a b",
        ] {
            assert_eq!(logs_url(bad), None, "{bad}");
        }
    }

    #[test]
    fn the_users_collector_never_gets_the_compiled_headers() {
        let own = address(Some("http://127.0.0.1:4318")).unwrap();
        assert_eq!(own.source, Source::User);
        assert!(own.headers.is_empty());
        assert_eq!(
            address(Some("not a url")),
            None,
            "a broken own address is no address: never a fallback to the compiled one"
        );
        assert_eq!(
            parse_headers("Authorization=Bearer a=b, x-team = dfw ,broken,=v"),
            [
                ("Authorization".to_string(), "Bearer a=b".to_string()),
                ("x-team".to_string(), "dfw".to_string())
            ]
        );
    }

    #[test]
    fn the_spec_wording_for_a_build_without_an_address() {
        assert_eq!(
            describe(&State::Off(OffReason::NoAddress)),
            "off: no collector address in this build"
        );
    }
}
