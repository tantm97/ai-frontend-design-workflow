//! The files of telemetry, all under `<dfw home>/telemetry/`: the spool of events waiting to be
//! sent (`spool.jsonl`, one event per line), the install id (`id`), the mark that the notice was
//! shown (`notice`) and the time of the last flush (`last-flush`, its modification time).
//! Every change of the spool runs under a lock folder, as the review files do; unlike them a
//! recorder never waits: when the lock is busy the event is dropped.

use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::BuildHasher;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::events::Spooled;
use crate::core::paths::global_dir;
use crate::core::util::sha256;

/// The spool never grows beyond this; the oldest events go first.
pub const SPOOL_CAP: u64 = 1024 * 1024;
/// A flush is started at most this often.
pub const FLUSH_EVERY: Duration = Duration::from_secs(60);

const SPOOL: &str = "spool.jsonl";
const ID: &str = "id";
const NOTICE: &str = "notice";
const STAMP: &str = "last-flush";
const LOCK: &str = ".lock";
const FLUSHING: &str = ".flushing";

/// A lock older than this was left by a process that died (a flush ends within 3 seconds).
const LOCK_STALE: Duration = Duration::from_secs(10);
const LOCK_RETRY: Duration = Duration::from_micros(250);
/// How long a command waits for the lock before it drops its event.
pub const RECORDER_PATIENCE: Duration = Duration::from_millis(2);
/// How long the flush and the telemetry commands wait.
pub const FLUSH_PATIENCE: Duration = Duration::from_secs(2);

/// The telemetry folder of this user.
pub fn dir() -> PathBuf {
    global_dir().join("telemetry")
}

/// Holds a lock folder until dropped.
pub struct Lock(PathBuf);

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

/// The right to send: one flush at a time, so two of them never send the same events.
/// `None` when another flush holds it.
pub fn flushing(dir: &Path) -> Option<Lock> {
    take(dir.join(FLUSHING), Duration::ZERO)
}

fn lock(dir: &Path, patience: Duration) -> Option<Lock> {
    take(dir.join(LOCK), patience)
}

fn take(path: PathBuf, patience: Duration) -> Option<Lock> {
    let start = Instant::now();
    loop {
        match fs::create_dir(&path) {
            Ok(()) => return Some(Lock(path)),
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {
                let age = fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|at| SystemTime::now().duration_since(at).ok());
                if age.is_some_and(|age| age > LOCK_STALE) {
                    let _ = fs::remove_dir(&path);
                    continue;
                }
                if start.elapsed() > patience {
                    return None;
                }
                thread::sleep(LOCK_RETRY);
            }
            Err(_) => return None,
        }
    }
}

/// Replace a file through a temporary one, so a reader never sees half of it.
fn replace(file: &Path, bytes: &[u8]) -> io::Result<()> {
    write(file, bytes)
}

/// [`crate::core::util::write_atomic`] with the `io::Error` the spool functions return.
fn write(file: &Path, bytes: impl AsRef<[u8]>) -> io::Result<()> {
    crate::core::util::write_atomic(file, bytes)
        .map_err(|e| io::Error::other(e.message().to_string()))
}

/// The start of the newest lines of `old` that fit in `room` bytes.
fn newest_from(old: &[u8], room: usize) -> usize {
    let cut = old.len().saturating_sub(room);
    if cut == 0 {
        return 0;
    }
    // Cut at a line start: the line the cut falls in goes too.
    old[cut - 1..]
        .iter()
        .position(|b| *b == b'\n')
        .map_or(old.len(), |i| cut + i)
}

/// Add events to the spool. Returns whether a flush is due (and marks it started, so only one
/// process starts it). Nothing is written when the lock is busy.
pub fn append(dir: &Path, events: &[Spooled]) -> io::Result<bool> {
    let mut lines = String::new();
    for event in events {
        lines.push_str(&serde_json::to_string(event)?);
        lines.push('\n');
    }
    fs::create_dir_all(dir)?;
    let Some(_lock) = lock(dir, RECORDER_PATIENCE) else {
        return Ok(false);
    };
    let file = dir.join(SPOOL);
    let size = fs::metadata(&file).map(|m| m.len()).unwrap_or(0);
    if size + lines.len() as u64 > SPOOL_CAP {
        // Half is dropped at once, so a full spool is not rewritten on every event.
        let old = fs::read(&file).unwrap_or_default();
        let from = newest_from(&old, (SPOOL_CAP / 2) as usize);
        replace(&file, &old[from..])?;
    }
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&file)?
        .write_all(lines.as_bytes())?;
    let stamp = dir.join(STAMP);
    let recent = fs::metadata(&stamp)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|at| SystemTime::now().duration_since(at).ok())
        .is_some_and(|age| age < FLUSH_EVERY);
    if !recent {
        write(&stamp, b"")?;
    }
    Ok(!recent)
}

/// The first events of the spool and the bytes they came from.
pub struct Batch {
    pub events: Vec<Spooled>,
    raw: Vec<u8>,
}

/// Up to `max` lines from the start of the spool. A line that is not an event (see
/// `events.rs`) is never sent; it leaves the spool with the batch.
pub fn next_batch(dir: &Path, max: usize) -> Option<Batch> {
    let _lock = lock(dir, FLUSH_PATIENCE)?;
    let bytes = fs::read(dir.join(SPOOL)).ok()?;
    let mut end = 0;
    let mut events = Vec::new();
    for line in bytes.split_inclusive(|b| *b == b'\n').take(max) {
        if line.last() != Some(&b'\n') {
            break; // half a line: a writer died; it stays
        }
        end += line.len();
        if let Ok(event) = serde_json::from_slice::<Spooled>(line) {
            events.push(event);
        }
    }
    (end > 0).then(|| Batch {
        events,
        raw: bytes[..end].to_vec(),
    })
}

/// Every event waiting in the spool.
pub fn pending(dir: &Path) -> Vec<Spooled> {
    next_batch(dir, usize::MAX).map_or(Vec::new(), |batch| batch.events)
}

/// Take a batch out of the spool after the collector accepted it. When the spool no longer
/// starts with it (the cap dropped old lines meanwhile) nothing is removed.
pub fn remove_sent(dir: &Path, batch: &Batch) -> io::Result<()> {
    let Some(_lock) = lock(dir, FLUSH_PATIENCE) else {
        return Ok(());
    };
    let file = dir.join(SPOOL);
    let bytes = fs::read(&file)?;
    match bytes.strip_prefix(batch.raw.as_slice()) {
        Some([]) => fs::remove_file(&file),
        Some(rest) => replace(&file, rest),
        None => Ok(()),
    }
}

/// 128 bits as hex. The system's random source where there is one; always the randomly keyed
/// hasher of the standard library, the clock and the process id.
fn random_id() -> String {
    let mut seed = Vec::new();
    for salt in 0u8..4 {
        seed.extend(RandomState::new().hash_one(salt).to_le_bytes());
    }
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    seed.extend(nanos.to_le_bytes());
    seed.extend(std::process::id().to_le_bytes());
    let mut random = [0u8; 16];
    if fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut random))
        .is_ok()
    {
        seed.extend(random);
    }
    sha256(seed)[..32].to_string()
}

/// The install id; `create` makes one when there is none (the first enabled event).
pub fn install_id(dir: &Path, create: bool) -> Option<String> {
    let file = dir.join(ID);
    let known = fs::read_to_string(&file)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|id| id.len() == 32 && id.bytes().all(|b| b.is_ascii_hexdigit()));
    if known.is_some() || !create {
        return known;
    }
    let id = random_id();
    fs::create_dir_all(dir).ok()?;
    write(&file, format!("{id}\n")).ok()?;
    Some(id)
}

/// Whether the one-time notice was printed on this machine.
pub fn notice_shown(dir: &Path) -> bool {
    dir.join(NOTICE).exists()
}

/// Remember that the notice was printed.
pub fn mark_notice_shown(dir: &Path) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    write(&dir.join(NOTICE), b"shown\n")
}

/// Delete the install id, the spool and everything else of telemetry.
pub fn forget(dir: &Path) -> io::Result<()> {
    match fs::remove_dir_all(dir) {
        Err(err) if err.kind() != io::ErrorKind::NotFound => Err(err),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use crate::telemetry::events::{CommandName, DurationBucket, Event};

    fn event(exit: i32) -> Spooled {
        Spooled {
            at: 1_700_000_000,
            event: Event::Command {
                name: CommandName::or_unknown("status"),
                duration: DurationBucket::Lt10ms,
                exit,
            },
        }
    }

    fn exits(events: &[Spooled]) -> Vec<i32> {
        events
            .iter()
            .map(|e| match e.event {
                Event::Command { exit, .. } => exit,
                _ => -1,
            })
            .collect()
    }

    #[test]
    fn events_are_appended_batched_and_removed_when_sent() {
        let dir = test_dir("spool").join("telemetry");
        assert!(pending(&dir).is_empty());
        assert!(
            append(&dir, &[event(1), event(2)]).unwrap(),
            "first: a flush is due"
        );
        assert!(
            !append(&dir, &[event(3)]).unwrap(),
            "a flush was just started"
        );
        assert!(!dir.join(LOCK).exists(), "the lock is released");
        assert_eq!(exits(&pending(&dir)), [1, 2, 3]);

        let batch = next_batch(&dir, 2).unwrap();
        assert_eq!(exits(&batch.events), [1, 2]);
        append(&dir, &[event(4)]).unwrap();
        remove_sent(&dir, &batch).unwrap();
        assert_eq!(
            exits(&pending(&dir)),
            [3, 4],
            "what arrived during the send stays"
        );

        let rest = next_batch(&dir, 500).unwrap();
        remove_sent(&dir, &rest).unwrap();
        assert!(!dir.join(SPOOL).exists());
        assert!(next_batch(&dir, 500).is_none());
    }

    #[test]
    fn a_line_that_is_not_an_event_is_never_in_a_batch() {
        let dir = test_dir("spool-junk");
        fs::write(
            dir.join(SPOOL),
            "{\"at\":1,\"event\":{\"command\":{\"name\":\"/secret/path\",\"duration\":\"lt_10ms\",\"exit\":0}}}\nnot json\n",
        )
        .unwrap();
        append(&dir, &[event(7)]).unwrap();
        let batch = next_batch(&dir, 500).unwrap();
        assert_eq!(exits(&batch.events), [7]);
        remove_sent(&dir, &batch).unwrap();
        assert!(!dir.join(SPOOL).exists(), "the junk left with the batch");
    }

    /// The spool stops growing at 1 MB: the oldest events are dropped.
    #[test]
    fn the_spool_is_capped() {
        let dir = test_dir("spool-cap");
        let line_len = serde_json::to_string(&event(1000)).unwrap().len() as u64 + 1;
        let many: Vec<Spooled> = (1000..2000).map(event).collect();
        let rounds = SPOOL_CAP / (line_len * 1000) + 3;
        let mut largest = 0;
        for _ in 0..rounds {
            append(&dir, &many).unwrap();
            largest = largest.max(fs::metadata(dir.join(SPOOL)).unwrap().len());
        }
        assert!(largest <= SPOOL_CAP, "{largest}");
        assert!(largest > SPOOL_CAP / 2);
        let left = pending(&dir);
        assert_eq!(
            left.len() as u64 * line_len,
            fs::metadata(dir.join(SPOOL)).unwrap().len(),
            "only whole lines are left"
        );
        assert_eq!(exits(&left).last(), Some(&1999), "the newest are kept");
        assert_eq!(newest_from(b"aa\nbb\ncc\n", 4), 6);
        assert_eq!(newest_from(b"aa\nbb\ncc\n", 6), 3);
        assert_eq!(newest_from(b"aa\nbb\ncc\n", 100), 0);
    }

    #[test]
    fn a_busy_lock_drops_the_event() {
        let dir = test_dir("spool-lock");
        fs::create_dir(dir.join(LOCK)).unwrap();
        let start = Instant::now();
        assert!(!append(&dir, &[event(1)]).unwrap());
        assert!(start.elapsed() < Duration::from_millis(200));
        assert!(
            !dir.join(SPOOL).exists(),
            "nothing recorded while the lock is held"
        );
        fs::remove_dir(dir.join(LOCK)).unwrap();
        append(&dir, &[event(2)]).unwrap();
        assert_eq!(exits(&pending(&dir)), [2]);
    }

    #[test]
    fn the_install_id_is_made_once_and_forgotten_with_the_folder() {
        let dir = test_dir("spool-id").join("telemetry");
        assert_eq!(install_id(&dir, false), None);
        assert!(!dir.exists(), "asking does not create anything");
        let id = install_id(&dir, true).unwrap();
        assert_eq!(id.len(), 32);
        assert!(id.bytes().all(|b| b.is_ascii_hexdigit()));
        assert_eq!(install_id(&dir, true), Some(id.clone()));
        assert_ne!(random_id(), id);
        assert!(!notice_shown(&dir));
        mark_notice_shown(&dir).unwrap();
        assert!(notice_shown(&dir));
        forget(&dir).unwrap();
        assert!(!dir.exists());
        forget(&dir).unwrap();
    }
}
