//! Ports the event stream of `core/studio/server.mjs`: the pages connected to `/__dfw/events`
//! and the broadcast of `design`, `comments`, `presence` and `ping` events.

use std::io::Write;
use std::net::TcpStream;
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::Value;

use super::http::write_head;
use crate::core::util::to_json_compact;

/// How long one event may take to reach a page; a page that does not read is dropped.
const EVENT_WRITE_TIMEOUT: Duration = Duration::from_secs(5);

/// Events waiting for one page; a page that falls this far behind is dropped.
const QUEUE: usize = 64;

/// The keys of the data of the events the Studio sends.
pub mod data {
    /// `design`: the design files that changed, relative to `design/`.
    pub const FILES: &str = "files";
    /// `comments`: the comment that was added or changed.
    pub const ID: &str = "id";
    /// `comments`: how many drafts were sent.
    pub const SENT: &str = "sent";
    /// `presence`: the file and the element the agent is about to edit, as the note names them.
    pub use super::presence::{DFW_ID, FILE};
}

/// The keys of `.dfw/presence.json`, the note the pre-tool-use hook writes:
/// `{ file, dfwId, at }`.
mod presence {
    /// The design file, relative to `design/`.
    pub const FILE: &str = "file";
    /// The `data-dfw-id` of the element.
    pub const DFW_ID: &str = "dfwId";
    /// When the note was written (ISO time).
    pub const AT: &str = "at";
}

/// The connected pages (`clients` in server.mjs): one queue per page. Each page's own
/// connection thread writes its events, so a page that never reads cannot hold up a broadcast
/// (the list lock is only held to put events in the queues).
#[derive(Default)]
pub struct Clients {
    queues: Mutex<Vec<SyncSender<Arc<str>>>>,
}

impl Clients {
    /// Answer `/__dfw/events`: the stream head, a `hello` event, then write this page's events
    /// until it is gone. Runs on the connection's thread and returns when the page left, fell
    /// behind or stopped reading.
    pub fn connect(&self, mut stream: TcpStream) {
        let _ = stream.set_read_timeout(None);
        let _ = stream.set_write_timeout(Some(EVENT_WRITE_TIMEOUT));
        let head = [
            ("Content-Type".to_string(), "text/event-stream".to_string()),
            ("Cache-Control".to_string(), "no-store".to_string()),
        ];
        let ok = write_head(&mut stream, super::http::status::OK, &head)
            .and_then(|_| stream.write_all(b"event: hello\ndata: {}\n\n"))
            .and_then(|_| stream.flush())
            .is_ok();
        if !ok {
            return;
        }
        let (tx, rx) = sync_channel(QUEUE);
        match self.queues.lock() {
            Ok(mut list) => list.push(tx),
            Err(_) => return,
        }
        write_events(&mut stream, &rx);
    }

    /// `broadcast(event, data)`: one event to every page; a page that is gone or too far
    /// behind is dropped. Never waits on a socket.
    pub fn broadcast(&self, event: &str, data: &Value) {
        let msg: Arc<str> = format!("event: {event}\ndata: {}\n\n", to_json_compact(data)).into();
        if let Ok(mut list) = self.queues.lock() {
            list.retain(|tx| match tx.try_send(Arc::clone(&msg)) {
                Ok(()) => true,
                Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
            });
        }
    }

    /// Pages connected now.
    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.queues.lock().map_or(0, |l| l.len())
    }
}

/// Write queued events to one page until the queue is dropped or a write fails (the page left
/// or did not read within [`EVENT_WRITE_TIMEOUT`]).
fn write_events(stream: &mut TcpStream, rx: &Receiver<Arc<str>>) {
    for msg in rx {
        if stream
            .write_all(msg.as_bytes())
            .and_then(|_| stream.flush())
            .is_err()
        {
            return;
        }
    }
}

/// The presence note the pre-tool-use hook wrote, as the `presence` event carries it, when it
/// is recent (under 10 s by the real clock, as `Date.now() - Date.parse(p.at)` in Node).
pub fn presence_event(text: &str, now_ms: i64) -> Option<Value> {
    let p: Value = serde_json::from_str(text).ok()?;
    let file = p.get(presence::FILE)?.as_str()?;
    let at = p
        .get(presence::AT)
        .and_then(Value::as_str)
        .and_then(parse_iso_ms)?;
    if now_ms - at >= 10_000 {
        return None;
    }
    let dfw_id = p.get(presence::DFW_ID).and_then(Value::as_str);
    Some(serde_json::json!({ (data::FILE): file, (data::DFW_ID): dfw_id }))
}

/// `Date.parse` of an ISO time `YYYY-MM-DDTHH:MM:SS[.sss](Z|±HH:MM)` in milliseconds.
pub fn parse_iso_ms(text: &str) -> Option<i64> {
    let b = text.as_bytes();
    let num = |from: usize, len: usize| -> Option<i64> {
        let s = text.get(from..from + len)?;
        s.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| s.parse().ok())
            .flatten()
    };
    if b.len() < 19
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (num(0, 4)?, num(5, 2)?, num(8, 2)?);
    let (h, mi, s) = (num(11, 2)?, num(14, 2)?, num(17, 2)?);
    let mut i = 19;
    let mut ms = 0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while b.get(i).is_some_and(u8::is_ascii_digit) {
            i += 1;
        }
        let frac = text.get(start..i)?;
        if frac.is_empty() {
            return None;
        }
        ms = format!("{frac:0<3}")[..3].parse().ok()?;
    }
    let offset_min = match text.get(i..)? {
        "Z" => 0,
        rest if rest.len() == 6
            && (rest.starts_with('+') || rest.starts_with('-'))
            && &rest[3..4] == ":" =>
        {
            let sign = if rest.starts_with('-') { -1 } else { 1 };
            sign * (num(i + 1, 2)? * 60 + num(i + 4, 2)?)
        }
        _ => return None,
    };
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 24 || mi > 59 || s > 59 {
        return None;
    }
    // Days from the civil date (Howard Hinnant's algorithm).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + h) * 60 + mi - offset_min) * 60_000 + s * 1000 + ms)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn iso_times_parse_like_date_parse() {
        assert_eq!(parse_iso_ms("1970-01-01T00:00:00.000Z"), Some(0));
        assert_eq!(
            parse_iso_ms("2026-01-02T03:04:05.678Z"),
            Some(1_767_323_045_678)
        );
        assert_eq!(
            parse_iso_ms("2026-01-02T05:04:05.678+02:00"),
            Some(1_767_323_045_678)
        );
        assert_eq!(
            parse_iso_ms("2026-01-02T03:04:05Z"),
            Some(1_767_323_045_000)
        );
        assert_eq!(parse_iso_ms("yesterday"), None);
    }

    #[test]
    fn presence_is_relayed_only_when_recent() {
        let now = parse_iso_ms("2026-01-02T03:04:10.000Z").unwrap();
        let note = r#"{"file":"screens/demo/a.html","dfwId":"demo.title","at":"2026-01-02T03:04:05.000Z"}"#;
        assert_eq!(
            presence_event(note, now),
            Some(json!({ "file": "screens/demo/a.html", "dfwId": "demo.title" }))
        );
        assert_eq!(presence_event(note, now + 10_000), None);
        assert_eq!(
            presence_event(r#"{"file":"x","dfwId":3,"at":"2026-01-02T03:04:09Z"}"#, now),
            Some(json!({ "file": "x", "dfwId": null }))
        );
        assert_eq!(
            presence_event(r#"{"file":1,"at":"2026-01-02T03:04:09Z"}"#, now),
            None
        );
        assert_eq!(presence_event("not json", now), None);
    }
}
