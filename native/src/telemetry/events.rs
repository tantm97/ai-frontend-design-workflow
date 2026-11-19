//! What telemetry may say: a closed list of events whose fields are closed lists too
//! (telemetry design decision 6). There is no free-form attribute and no field that takes text
//! from the command line, a file or the environment: a name is accepted only when it is one of
//! the names compiled into this binary, so a path or a comment cannot be attached by mistake.

use std::time::Duration;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::core::host::{HookResult, HOSTS};
use crate::workflow::state::{GATES, MODES, PHASES};

/// A name that stands in when the real one is not on its list.
const UNKNOWN: &str = "unknown";

const HOOK_NAMES: [&str; 5] = [
    "session-start",
    "pre-tool-use",
    "post-tool-use",
    "user-prompt-submit",
    "stop",
];

/// The `tool` names of check.mjs, lighthouse.mjs and a11y.mjs.
const CHECK_TOOLS: [&str; 7] = [
    "dfw-lint",
    "dfw-ds-audit",
    "impeccable",
    "sloplint",
    "taste-copy-lint",
    "axe",
    "lighthouse",
];

fn command_name(name: &str) -> Option<&'static str> {
    if name == UNKNOWN {
        return Some(UNKNOWN);
    }
    crate::cli::commands()
        .into_iter()
        .map(|(known, _)| known)
        .find(|known| *known == name)
}

fn host_id(id: &str) -> Option<&'static str> {
    if id == UNKNOWN {
        return Some(UNKNOWN);
    }
    HOSTS.iter().map(|h| h.id).find(|known| *known == id)
}

fn hook_name(name: &str) -> Option<&'static str> {
    HOOK_NAMES.into_iter().find(|known| *known == name)
}

fn gate_id(id: &str) -> Option<&'static str> {
    GATES
        .iter()
        .map(|(gate, _)| *gate)
        .find(|known| *known == id)
}

fn mode_name(name: &str) -> Option<&'static str> {
    MODES.iter().map(|m| m.name).find(|known| *known == name)
}

fn phase_name(name: &str) -> Option<&'static str> {
    PHASES.into_iter().find(|known| *known == name)
}

fn check_tool(name: &str) -> Option<&'static str> {
    CHECK_TOOLS.into_iter().find(|known| *known == name)
}

/// A string field of an event: one of the names on a list built into the binary. The value
/// held is the binary's own copy, never the text it was looked up with.
macro_rules! closed_name {
    ($(#[$doc:meta])* $name:ident, $lookup:path) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub struct $name(&'static str);

        impl $name {
            /// The name when it is on the list.
            pub fn new(text: &str) -> Option<Self> {
                $lookup(text).map(Self)
            }

            pub fn as_str(self) -> &'static str {
                self.0
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                Self::new(&text).ok_or_else(|| D::Error::custom("not a known name"))
            }
        }
    };
}

closed_name!(
    /// A command of the command table (`cli::commands`), or `unknown`.
    CommandName,
    command_name
);
closed_name!(
    /// A hook handler.
    HookName,
    hook_name
);
closed_name!(
    /// A host of `core::host::HOSTS`, or `unknown`.
    HostId,
    host_id
);
closed_name!(
    /// An approval gate.
    GateId,
    gate_id
);
closed_name!(
    /// A workflow mode.
    ModeName,
    mode_name
);
closed_name!(
    /// A workflow phase.
    PhaseName,
    phase_name
);
closed_name!(
    /// An external or built-in check.
    CheckTool,
    check_tool
);

impl CommandName {
    /// The command, or `unknown` for a first argument that is no command.
    pub fn or_unknown(text: &str) -> Self {
        Self::new(text).unwrap_or(Self(UNKNOWN))
    }
}

impl HostId {
    /// The host, or `unknown`.
    pub fn or_unknown(text: Option<&str>) -> Self {
        text.and_then(Self::new).unwrap_or(Self(UNKNOWN))
    }
}

/// How long something took, as a range: the exact time is not collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DurationBucket {
    #[serde(rename = "lt_10ms")]
    Lt10ms,
    #[serde(rename = "lt_100ms")]
    Lt100ms,
    #[serde(rename = "lt_1s")]
    Lt1s,
    #[serde(rename = "lt_10s")]
    Lt10s,
    #[serde(rename = "lt_1m")]
    Lt1m,
    #[serde(rename = "ge_1m")]
    Ge1m,
}

impl DurationBucket {
    pub fn of(elapsed: Duration) -> Self {
        match elapsed.as_millis() {
            0..10 => Self::Lt10ms,
            10..100 => Self::Lt100ms,
            100..1_000 => Self::Lt1s,
            1_000..10_000 => Self::Lt10s,
            10_000..60_000 => Self::Lt1m,
            _ => Self::Ge1m,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lt10ms => "lt_10ms",
            Self::Lt100ms => "lt_100ms",
            Self::Lt1s => "lt_1s",
            Self::Lt10s => "lt_10s",
            Self::Lt1m => "lt_1m",
            Self::Ge1m => "ge_1m",
        }
    }
}

/// What a hook call ended as. The text of a context, refusal or error is not collected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HookOutcome {
    /// Nothing to say.
    None,
    /// Text was added to the model's context.
    Context,
    /// A tool call was refused.
    Deny,
    /// The agent was told not to stop yet.
    Block,
    /// An internal error; the hook failed open.
    Error,
}

impl HookOutcome {
    /// The kind of a handler's result.
    pub fn of(result: Option<&HookResult>) -> Self {
        match result {
            None => Self::None,
            Some(HookResult::Context(_)) => Self::Context,
            Some(HookResult::Deny(_)) => Self::Deny,
            Some(HookResult::Block(_)) => Self::Block,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Context => "context",
            Self::Deny => "deny",
            Self::Block => "block",
            Self::Error => "error",
        }
    }
}

/// One check and how many findings it reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckCount {
    pub tool: CheckTool,
    pub findings: u32,
}

/// Everything telemetry can record. Adding a fact means adding a typed field here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    /// A command ended.
    Command {
        name: CommandName,
        duration: DurationBucket,
        exit: i32,
    },
    /// A hook call ended.
    Hook {
        name: HookName,
        host: HostId,
        duration: DurationBucket,
        outcome: HookOutcome,
    },
    /// The user approved a gate.
    Gate { gate: GateId },
    /// A check run ended.
    Checks { tools: Vec<CheckCount> },
    /// Where the workflow stands after a command that changes it.
    Workflow { mode: ModeName, phase: PhaseName },
}

/// An event in the spool: when it happened (whole seconds since 1970) and what.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Spooled {
    pub at: u64,
    pub event: Event,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No string field accepts a path or free text (telemetry task 2.1).
    #[test]
    fn names_come_from_closed_lists_only() {
        for text in [
            "/Users/someone/project",
            "design/screens/home/a.html",
            "a.html",
            "status --root /tmp/x",
            "please fix the header",
            "",
            "STATUS",
            " status",
        ] {
            assert!(CommandName::new(text).is_none(), "{text}");
            assert!(HookName::new(text).is_none(), "{text}");
            assert!(HostId::new(text).is_none(), "{text}");
            assert!(GateId::new(text).is_none(), "{text}");
            assert!(ModeName::new(text).is_none(), "{text}");
            assert!(PhaseName::new(text).is_none(), "{text}");
            assert!(CheckTool::new(text).is_none(), "{text}");
        }
        assert_eq!(CommandName::or_unknown("/etc/passwd").as_str(), "unknown");
        assert_eq!(HostId::or_unknown(Some("/etc/passwd")).as_str(), "unknown");
        assert_eq!(
            CommandName::new("status").map(CommandName::as_str),
            Some("status")
        );
        assert_eq!(GateId::new("G2").map(GateId::as_str), Some("G2"));
        assert_eq!(HostId::new("copilot").map(HostId::as_str), Some("copilot"));
    }

    /// A spool line that names something off the lists, or carries an extra field, is not an event.
    #[test]
    fn a_spool_line_is_an_event_only_when_every_field_is_known() {
        let parse = |line: &str| serde_json::from_str::<Spooled>(line).ok();
        let good = r#"{"at":1700000000,"event":{"command":{"name":"status","duration":"lt_10ms","exit":0}}}"#;
        let event = parse(good).unwrap();
        assert_eq!(serde_json::to_string(&event).unwrap(), good);
        for bad in [
            r#"{"at":1,"event":{"command":{"name":"/home/me/app","duration":"lt_10ms","exit":0}}}"#,
            r#"{"at":1,"event":{"command":{"name":"status","duration":"lt_10ms","exit":0,"path":"/x"}}}"#,
            r#"{"at":1,"event":{"command":{"name":"status","duration":"12ms","exit":0}}}"#,
            r#"{"at":1,"event":{"note":{"text":"hello"}}}"#,
            r#"{"at":1,"event":{"gate":{"gate":"G9"}}}"#,
            r#"{"at":1,"event":{"workflow":{"mode":"new-product","phase":"my phase"}}}"#,
            r#"{"at":1,"event":{"checks":{"tools":[{"tool":"my-tool","findings":1}]}}}"#,
            r#"{"at":1,"event":{"gate":{"gate":"G2"}},"cwd":"/x"}"#,
        ] {
            assert!(parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn durations_fall_into_buckets() {
        let bucket = |ms: u64| DurationBucket::of(Duration::from_millis(ms)).as_str();
        assert_eq!(bucket(0), "lt_10ms");
        assert_eq!(bucket(9), "lt_10ms");
        assert_eq!(bucket(10), "lt_100ms");
        assert_eq!(bucket(999), "lt_1s");
        assert_eq!(bucket(9_999), "lt_10s");
        assert_eq!(bucket(59_999), "lt_1m");
        assert_eq!(bucket(60_000), "ge_1m");
        // The spool word and the word that is sent are the same.
        assert_eq!(
            serde_json::to_string(&DurationBucket::Lt10ms).unwrap(),
            "\"lt_10ms\""
        );
        assert_eq!(
            serde_json::to_string(&HookOutcome::Context).unwrap(),
            "\"context\""
        );
    }
}
