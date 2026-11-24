//! The OTLP/HTTP JSON encoding of the events, written by hand (telemetry design decision 3):
//! one `LogRecord` per event in one `resourceLogs` entry. 64-bit integers are strings, as the
//! OTLP JSON mapping asks.

use serde_json::{json, Value};

use super::events::{Event, Spooled};
use super::wire::{attr, event, otlp};

/// Who sends: the resource attributes of every request.
pub struct Resource {
    /// `service.version`.
    pub version: String,
    /// `dfw.install_id`: random, per machine.
    pub install_id: String,
}

fn text(key: &str, value: &str) -> Value {
    json!({ (otlp::KEY): key, (otlp::VALUE): { (otlp::STRING_VALUE): value } })
}

fn int(key: &str, value: i64) -> Value {
    json!({ (otlp::KEY): key, (otlp::VALUE): { (otlp::INT_VALUE): value.to_string() } })
}

/// `os.type` in OpenTelemetry's words.
fn os_type() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    }
}

/// `host.arch` in OpenTelemetry's words.
fn host_arch() -> &'static str {
    match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    }
}

/// The event's name and attributes.
fn describe(event: &Event) -> (&'static str, Vec<Value>) {
    match event {
        Event::Command {
            name,
            duration,
            exit,
        } => (
            event::COMMAND,
            vec![
                text(attr::COMMAND, name.as_str()),
                text(attr::DURATION_BUCKET, duration.as_str()),
                int(attr::EXIT_CODE, i64::from(*exit)),
            ],
        ),
        Event::Hook {
            name,
            host,
            duration,
            outcome,
        } => (
            event::HOOK,
            vec![
                text(attr::HOOK, name.as_str()),
                text(attr::HOST, host.as_str()),
                text(attr::DURATION_BUCKET, duration.as_str()),
                text(attr::HOOK_OUTCOME, outcome.as_str()),
            ],
        ),
        Event::Gate { gate } => (event::GATE_APPROVED, vec![text(attr::GATE, gate.as_str())]),
        Event::Checks { tools } => (
            event::CHECKS,
            tools
                .iter()
                .map(|c| {
                    int(
                        &format!("dfw.check.{}.findings", c.tool.as_str()),
                        i64::from(c.findings),
                    )
                })
                .collect(),
        ),
        Event::Workflow { mode, phase } => (
            event::WORKFLOW,
            vec![
                text(attr::MODE, mode.as_str()),
                text(attr::PHASE, phase.as_str()),
            ],
        ),
    }
}

fn log_record(spooled: &Spooled) -> Value {
    let (name, attributes) = describe(&spooled.event);
    json!({
        (otlp::TIME_UNIX_NANO): format!("{}000000000", spooled.at),
        (otlp::SEVERITY_NUMBER): 9,
        (otlp::SEVERITY_TEXT): "INFO",
        (otlp::EVENT_NAME): name,
        (otlp::BODY): { (otlp::STRING_VALUE): name },
        (otlp::ATTRIBUTES): attributes,
    })
}

/// The body of one `POST <address>/v1/logs`.
pub fn request_body(resource: &Resource, events: &[Spooled]) -> Value {
    let records: Vec<Value> = events.iter().map(log_record).collect();
    json!({
        (otlp::RESOURCE_LOGS): [{
            (otlp::RESOURCE): { (otlp::ATTRIBUTES): [
                text(attr::SERVICE_NAME, "dfw"),
                text(attr::SERVICE_VERSION, &resource.version),
                text(attr::OS_TYPE, os_type()),
                text(attr::HOST_ARCH, host_arch()),
                text(attr::INSTALL_ID, &resource.install_id),
            ] },
            (otlp::SCOPE_LOGS): [{
                (otlp::SCOPE): { (otlp::NAME): "dfw.telemetry" },
                (otlp::LOG_RECORDS): records,
            }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::events::*;
    use crate::telemetry::wire::otlp::{
        ATTRIBUTES, KEY, RESOURCE, RESOURCE_LOGS, STRING_VALUE, VALUE,
    };

    fn at(event: Event) -> Spooled {
        Spooled {
            at: 1_700_000_000,
            event,
        }
    }

    #[test]
    fn a_request_is_one_resource_with_one_log_record_per_event() {
        const VERSION: &str = "9.9.9";
        let resource = Resource {
            version: VERSION.into(),
            install_id: "0123456789abcdef0123456789abcdef".into(),
        };
        let events = [
            at(Event::Command {
                name: CommandName::or_unknown("status"),
                duration: DurationBucket::Lt10ms,
                exit: 2,
            }),
            at(Event::Hook {
                name: HookName::new("pre-tool-use").unwrap(),
                host: HostId::or_unknown(Some("claude")),
                duration: DurationBucket::Lt100ms,
                outcome: HookOutcome::Deny,
            }),
            at(Event::Gate {
                gate: GateId::new("G2").unwrap(),
            }),
            at(Event::Checks {
                tools: vec![
                    CheckCount {
                        tool: CheckTool::new("dfw-lint").unwrap(),
                        findings: 3,
                    },
                    CheckCount {
                        tool: CheckTool::new("axe").unwrap(),
                        findings: 0,
                    },
                ],
            }),
            at(Event::Workflow {
                mode: ModeName::new("new-product").unwrap(),
                phase: PhaseName::new("concepts").unwrap(),
            }),
        ];
        let body = request_body(&resource, &events);
        let resource_logs = body[RESOURCE_LOGS].as_array().unwrap();
        assert_eq!(resource_logs.len(), 1);
        let attributes = resource_logs[0][RESOURCE][ATTRIBUTES].as_array().unwrap();
        let keys: Vec<&str> = attributes
            .iter()
            .map(|a| a[KEY].as_str().unwrap())
            .collect();
        assert_eq!(
            keys,
            [
                "service.name",
                "service.version",
                "os.type",
                "host.arch",
                "dfw.install_id"
            ]
        );
        assert_eq!(attributes[0][VALUE][STRING_VALUE], "dfw");
        assert_eq!(attributes[1][VALUE][STRING_VALUE], VERSION);
        let records = resource_logs[0]["scopeLogs"][0]["logRecords"]
            .as_array()
            .unwrap();
        assert_eq!(records.len(), 5);
        assert_eq!(
            serde_json::to_string(&records[0]).unwrap(),
            r#"{"timeUnixNano":"1700000000000000000","severityNumber":9,"severityText":"INFO","eventName":"dfw.command","body":{"stringValue":"dfw.command"},"attributes":[{"key":"dfw.command","value":{"stringValue":"status"}},{"key":"dfw.duration_bucket","value":{"stringValue":"lt_10ms"}},{"key":"dfw.exit_code","value":{"intValue":"2"}}]}"#
        );
        let names: Vec<&str> = records
            .iter()
            .map(|r| r["eventName"].as_str().unwrap())
            .collect();
        assert_eq!(
            names,
            [
                "dfw.command",
                "dfw.hook",
                "dfw.gate_approved",
                "dfw.checks",
                "dfw.workflow"
            ]
        );
        assert_eq!(
            records[3][ATTRIBUTES][0][KEY],
            "dfw.check.dfw-lint.findings"
        );
        assert_eq!(records[3][ATTRIBUTES][0][VALUE]["intValue"], "3");
    }
}
