//! `dfw telemetry flush`: send the first events of the spool to the collector and take out
//! what it accepted. This is the only code of the crate's telemetry that opens a connection,
//! and it runs only in a process started for it.

use std::path::Path;
use std::time::Duration;

use super::otlp::{request_body, Resource};
use super::{current, describe, spool, Address, State};
use crate::core::util::to_json_compact;

/// Events per request.
pub const BATCH: usize = 500;
/// The whole request must be done within this.
const TIMEOUT: Duration = Duration::from_secs(3);

/// How a flush ended.
#[derive(Debug, PartialEq, Eq)]
pub enum Flushed {
    /// Telemetry is off; the words say why.
    Off(String),
    Nothing,
    /// Another flush is sending right now.
    Busy,
    Sent(usize),
    /// The collector did not accept the batch; it stays in the spool.
    Failed(String),
}

/// Its own client: `core::http` sends GETs and follows redirects; this is a POST that must not.
fn post(address: &Address, body: &str) -> Result<(), String> {
    // No redirects: events go to the address that was configured and nowhere else.
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .max_redirects(0)
        .build()
        .into();
    let mut request = agent
        .post(&address.url)
        .header("Content-Type", "application/json");
    for (name, value) in &address.headers {
        request = request.header(name, value);
    }
    let response = request.send(body).map_err(|err| err.to_string())?;
    if response.status().is_success() {
        Ok(())
    } else {
        Err(format!("HTTP {}", response.status().as_u16()))
    }
}

/// The resource of this install, for requests and for `dfw telemetry show`. `None` without an
/// install id (it is made when the first event is recorded).
pub fn resource(dir: &Path) -> Option<Resource> {
    Some(Resource {
        version: crate::core::platform::dfw_version(),
        install_id: spool::install_id(dir, false)?,
    })
}

/// Send one batch.
pub fn flush(dir: &Path) -> Flushed {
    let address = match current(dir) {
        State::On(address) => address,
        other => return Flushed::Off(describe(&other)),
    };
    if !dir.exists() {
        return Flushed::Nothing;
    }
    let Some(_sending) = spool::flushing(dir) else {
        return Flushed::Busy;
    };
    let Some(batch) = spool::next_batch(dir, BATCH) else {
        return Flushed::Nothing;
    };
    if batch.events.is_empty() {
        // Only lines that are not events: they go, nothing is sent.
        let _ = spool::remove_sent(dir, &batch);
        return Flushed::Nothing;
    }
    // No id: `dfw telemetry off` ran since the events were recorded. Nothing leaves.
    let Some(resource) = resource(dir) else {
        return Flushed::Nothing;
    };
    let body = to_json_compact(&request_body(&resource, &batch.events));
    match post(&address, &body) {
        Ok(()) => {
            let _ = spool::remove_sent(dir, &batch);
            Flushed::Sent(batch.events.len())
        }
        Err(why) => Flushed::Failed(why),
    }
}
