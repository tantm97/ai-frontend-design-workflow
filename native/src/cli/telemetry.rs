//! `dfw telemetry status | on | off | endpoint <url> | show | flush`: see and control the usage
//! telemetry of `crate::telemetry`. New with the telemetry change: the Node CLI has no such command.

use std::path::Path;

use serde_json::json;

use super::word::{command, sub};
use super::{Ctx, Handler};
use crate::core::config::{set_config_value, Scope};
use crate::core::error::DfwError;
use crate::core::util::{out, to_json_compact, to_json_pretty};
use crate::telemetry::flush::{flush, resource, Flushed, BATCH};
use crate::telemetry::otlp::request_body;
use crate::telemetry::{compiled_endpoint, compiled_headers, current, describe, logs_url, spool};

const USAGE: &str =
    "Usage: dfw telemetry status | on | off | endpoint <url> | show [--raw] | flush";

/// The command of this file.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![(command::TELEMETRY, telemetry as Handler)]
}

/// Write one key of the global config (`<dfw home>/config.json`).
fn set(key: &str, value: serde_json::Value) -> Result<(), DfwError> {
    // The global scope needs no project.
    set_config_value(Scope::Global, Path::new(""), key, value).map(|_| ())
}

fn telemetry(ctx: &Ctx) -> Result<i32, DfwError> {
    let a = &ctx.args;
    let dir = spool::dir();
    match a.pos(1).unwrap_or(sub::STATUS) {
        sub::STATUS => {
            let mut lines = vec![describe(&current(&dir))];
            if let Some(id) = spool::install_id(&dir, false) {
                lines.push(format!("install id: {id}"));
            }
            let pending = spool::pending(&dir).len();
            if pending > 0 {
                lines.push(format!(
                    "{pending} events waiting to be sent (\"dfw telemetry show\" prints them)"
                ));
            }
            out(&lines.join("\n"));
        }
        sub::ON => {
            set("telemetry.enabled", json!(true))?;
            out(&describe(&current(&dir)));
        }
        sub::OFF => {
            set("telemetry.enabled", json!(false))?;
            spool::forget(&dir)?;
            out("Telemetry is off. The install id and the events waiting to be sent were deleted.");
        }
        sub::ENDPOINT => {
            let url = a.pos(2).unwrap_or_default();
            if logs_url(url).is_none() {
                return Err(DfwError::new(
                    "Give the address of an OTLP/HTTP collector, for example: dfw telemetry endpoint https://otel.example.org",
                ));
            }
            set("telemetry.endpoint", json!(url))?;
            out(&describe(&current(&dir)));
        }
        sub::SHOW => {
            let events = spool::pending(&dir);
            let Some(resource) = resource(&dir).filter(|_| !events.is_empty()) else {
                out("No events are waiting to be sent.");
                return Ok(0);
            };
            // One request body per batch, as the flush sends them (it sends them compact: --raw).
            for batch in events.chunks(BATCH) {
                let body = request_body(&resource, batch);
                out(&if a.truthy("raw") {
                    to_json_compact(&body)
                } else {
                    to_json_pretty(&body)
                });
            }
        }
        sub::FLUSH => match flush(&dir) {
            Flushed::Off(why) => out(&format!("Nothing sent: telemetry is {why}")),
            Flushed::Nothing => out("No events are waiting to be sent."),
            Flushed::Busy => out("Another flush is sending right now."),
            Flushed::Sent(n) => out(&format!("Sent {n} events.")),
            Flushed::Failed(why) => {
                return Err(DfwError::new(format!(
                    "The collector did not take the events ({why}). They stay in the spool."
                )))
            }
        },
        _ => return Err(DfwError::new(USAGE)),
    }
    Ok(0)
}

/// `__debug telemetry-build`: whether a collector address and headers were compiled into this
/// binary (never their values).
pub(super) fn debug_telemetry_build(_ctx: &Ctx) -> Result<i32, DfwError> {
    out(&to_json_compact(&json!({
        "endpoint": compiled_endpoint().is_some(),
        "headers": compiled_headers().is_some(),
    })));
    Ok(0)
}
