//! The OTLP/HTTP JSON encoding of the events, written by hand (telemetry design decision 3):
//! one `LogRecord` per event in one `resourceLogs` entry. 64-bit integers are strings, as the
//! OTLP JSON mapping asks.

use serde_json::{json, Value};

use super::events::{Event, Spooled};

/// Who sends: the resource attributes of every request.
pub struct Resource {
    /// `service.version`.
    pub version: String,
    /// `dfw.install_id`: random, per machine.
    pub install_id: String,
}

fn text(key: &str, value: &str) -> Value {
    json!({ "key": key, "value": { "stringValue": value } })
}

fn int(key: &str, value: i64) -> Value {
    json!({ "key": key, "value": { "intValue": value.to_string() } })
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
            "dfw.command",
            vec![
                text("dfw.command", name.as_str()),
                text("dfw.duration_bucket", duration.as_str()),
                int("dfw.exit_code", i64::from(*exit)),
            ],
        ),
        Event::Hook {
            name,
            host,
            duration,
            outcome,
        } => (
            "dfw.hook",
            vec![
                text("dfw.hook", name.as_str()),
                text("dfw.host", host.as_str()),
                text("dfw.duration_bucket", duration.as_str()),
                text("dfw.hook_outcome", outcome.as_str()),
            ],
        ),
        Event::Gate { gate } => ("dfw.gate_approved", vec![text("dfw.gate", gate.as_str())]),
        Event::Checks { tools } => (
            "dfw.checks",
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
            "dfw.workflow",
            vec![
                text("dfw.mode", mode.as_str()),
                text("dfw.phase", phase.as_str()),
            ],
        ),
    }
}

fn log_record(spooled: &Spooled) -> Value {
    let (name, attributes) = describe(&spooled.event);
    json!({
        "timeUnixNano": format!("{}000000000", spooled.at),
        "severityNumber": 9,
        "severityText": "INFO",
        "eventName": name,
        "body": { "stringValue": name },
        "attributes": attributes,
    })
}

/// The body of one `POST <address>/v1/logs`.
pub fn request_body(resource: &Resource, events: &[Spooled]) -> Value {
    let records: Vec<Value> = events.iter().map(log_record).collect();
    json!({
        "resourceLogs": [{
            "resource": { "attributes": [
                text("service.name", "dfw"),
                text("service.version", &resource.version),
                text("os.type", os_type()),
                text("host.arch", host_arch()),
                text("dfw.install_id", &resource.install_id),
            ] },
            "scopeLogs": [{
                "scope": { "name": "dfw.telemetry" },
                "logRecords": records,
            }],
        }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::events::*;

    fn at(event: Event) -> Spooled {
        Spooled {
            at: 1_700_000_000,
            event,
        }
    }

    #[test]
    fn a_request_is_one_resource_with_one_log_record_per_event() {
        let resource = Resource {
            version: "9.9.9".into(),
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
        let resource_logs = body["resourceLogs"].as_array().unwrap();
        assert_eq!(resource_logs.len(), 1);
        let attributes = resource_logs[0]["resource"]["attributes"]
            .as_array()
            .unwrap();
        let keys: Vec<&str> = attributes
            .iter()
            .map(|a| a["key"].as_str().unwrap())
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
        assert_eq!(attributes[0]["value"]["stringValue"], "dfw");
        assert_eq!(attributes[1]["value"]["stringValue"], "9.9.9");
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
            records[3]["attributes"][0]["key"],
            "dfw.check.dfw-lint.findings"
        );
        assert_eq!(records[3]["attributes"][0]["value"]["intValue"], "3");
    }
}
