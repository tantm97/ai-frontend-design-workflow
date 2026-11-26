//! Ports the file watching of `core/studio/server.mjs`: live reload of `design/` and
//! `.dfw/reviews/`, and the presence note.
//!
//! Node used `fs.watch` backed by a one-second mtime poll (fs.watch misses events on some
//! filesystems). The standard library has no change notification and no crate for it is
//! approved, so here the poll is the watch: every 250 ms, or every second with
//! `DFW_STUDIO_WATCH=poll` (the Node setting for "polling only"). A file counts as changed when
//! its mtime or size changed, appeared or disappeared; changes are flushed 150 ms after the last
//! one, as in Node.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf, MAIN_SEPARATOR};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::sse::{data, presence_event, Clients};
use crate::core::paths::{env_nonempty, file};
use crate::design::compose::build_system;

/// Files the Studio does not react to (`ignored` in server.mjs).
pub fn ignored(rel: &str) -> bool {
    rel.is_empty() || rel.starts_with(file::APPROVED) || rel.ends_with(".tmp") || rel == ".lock"
}

/// What identifies a version of a file: modification time and size.
type Stamp = (u128, u64);

fn stamp(file: &Path) -> Option<Stamp> {
    let meta = fs::metadata(file).ok()?;
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_nanos());
    Some((mtime, meta.len()))
}

/// `snapshot(dir)`: every file under `dir` (paths with the platform separator, as `fs.watch`
/// reports them), ignored ones left out.
pub fn snapshot(dir: &Path) -> BTreeMap<String, Stamp> {
    fn walk(dir: &Path, rel: &str, out: &mut BTreeMap<String, Stamp>) {
        let Ok(entries) = fs::read_dir(if rel.is_empty() {
            dir.to_path_buf()
        } else {
            dir.join(rel)
        }) else {
            return;
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let r = if rel.is_empty() {
                name
            } else {
                format!("{rel}{MAIN_SEPARATOR}{name}")
            };
            if ignored(&r) {
                continue;
            }
            match entry.file_type() {
                Ok(t) if t.is_dir() => walk(dir, &r, out),
                Ok(t) if t.is_file() => {
                    if let Some(s) = stamp(&dir.join(&r)) {
                        out.insert(r, s);
                    }
                }
                _ => {}
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, "", &mut out);
    out
}

/// One watched folder (`design` or `reviews`) and what it held at the last poll.
struct Watched {
    kind: &'static str,
    dir: PathBuf,
    seen: BTreeMap<String, Stamp>,
}

impl Watched {
    fn new(kind: &'static str, dir: PathBuf) -> Self {
        if !dir.exists() {
            let _ = fs::create_dir_all(&dir);
        }
        let seen = snapshot(&dir);
        Watched { kind, dir, seen }
    }

    /// `poll(dir, kind)`: `kind:file` for every file that changed, appeared or went away.
    fn poll(&mut self) -> Vec<String> {
        let now = snapshot(&self.dir);
        let mut changed = Vec::new();
        for (file, s) in &now {
            if self.seen.get(file) != Some(s) {
                changed.push(format!("{}:{file}", self.kind));
            }
        }
        for file in self.seen.keys() {
            if !now.contains_key(file) {
                changed.push(format!("{}:{file}", self.kind));
            }
        }
        self.seen = now;
        changed
    }
}

/// What a flush of pending changes does: rebuild the component CSS when a master changed, then
/// the `design` event (files with `/`) and the `comments` event.
pub fn flush(root: &Path, clients: &Clients, files: &[String]) {
    let master = |f: &&String| {
        f.strip_prefix("design:system")
            .and_then(|r| r.strip_prefix(['/', '\\']))
            .and_then(|r| r.strip_prefix("components"))
            .is_some_and(|r| r.starts_with(['/', '\\']))
    };
    if files.iter().any(|f| master(&f)) {
        if let Err(err) = build_system(root) {
            eprintln!("dfw studio: buildSystem: {}", err.message());
        }
    }
    let mut design: Vec<String> = Vec::new();
    for f in files {
        if let Some(rel) = f.strip_prefix("design:") {
            let rel = rel.split(MAIN_SEPARATOR).collect::<Vec<_>>().join("/");
            if !design.contains(&rel) {
                design.push(rel);
            }
        }
    }
    if !design.is_empty() {
        clients.broadcast("design", &json!({ (data::FILES): design }));
    }
    if files.iter().any(|f| f.starts_with("reviews:")) {
        clients.broadcast("comments", &json!({}));
    }
}

/// Watch `design/`, `.dfw/reviews/` and `.dfw/presence.json` until the process ends, and send
/// a `ping` every 25 s so proxies and browsers keep the stream open.
pub fn run(root: PathBuf, clients: &Clients) {
    let paths = crate::core::paths::project_paths(&root);
    let mut folders = [
        Watched::new("design", paths.design.clone()),
        Watched::new("reviews", paths.reviews.clone()),
    ];
    let every = if env_nonempty("DFW_STUDIO_WATCH").as_deref() == Some("poll") {
        Duration::from_millis(1000)
    } else {
        Duration::from_millis(250)
    };
    let presence = paths.presence.clone();
    let mut presence_seen = stamp(&presence);
    let mut pending: Vec<String> = Vec::new();
    let mut deadline: Option<Instant> = None;
    let mut next_poll = Instant::now() + every;
    let mut next_presence = Instant::now();
    let mut next_ping = Instant::now() + Duration::from_secs(25);
    loop {
        std::thread::sleep(Duration::from_millis(40));
        let now = Instant::now();
        if now >= next_poll {
            next_poll = now + every;
            for w in folders.iter_mut() {
                let changed = w.poll();
                if !changed.is_empty() {
                    for c in changed {
                        if !pending.contains(&c) {
                            pending.push(c);
                        }
                    }
                    deadline = Some(now + Duration::from_millis(150));
                }
            }
        }
        if deadline.is_some_and(|d| now >= d) {
            deadline = None;
            flush(&root, clients, &std::mem::take(&mut pending));
        }
        // fs.watchFile(presence.json, { interval: 120 }) in Node.
        if now >= next_presence {
            next_presence = now + Duration::from_millis(120);
            let current = stamp(&presence);
            if current != presence_seen {
                presence_seen = current;
                if current.is_some() {
                    if let Some(event) = fs::read_to_string(&presence)
                        .ok()
                        .and_then(|t| presence_event(&t, now_ms()))
                    {
                        clients.broadcast("presence", &event);
                    }
                }
            }
        }
        if now >= next_ping {
            next_ping = now + Duration::from_secs(25);
            clients.broadcast("ping", &Value::Object(Default::default()));
        }
    }
}

/// The real clock in milliseconds (not `DFW_NOW`: presence compares with the hook's real time).
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::HTML_FILE;
    use crate::core::util::test_dir;

    #[test]
    fn ignored_files_follow_server_mjs() {
        for f in [
            "",
            "approved",
            "approved/v1/a.html",
            "x.json.123.tmp",
            ".lock",
        ] {
            assert!(ignored(f), "{f}");
        }
        for f in ["screens/a.html", "rv_1.json", "sub/.lock"] {
            assert!(!ignored(f), "{f}");
        }
    }

    #[test]
    fn a_poll_reports_new_changed_and_removed_files() {
        /// A file the poll sees removed.
        const GONE: &str = "gone.css";
        let dir = test_dir("studio-watch-poll");
        fs::create_dir_all(dir.join(file::SCREENS)).unwrap();
        fs::write(dir.join(file::SCREENS).join(HTML_FILE), "a").unwrap();
        fs::write(dir.join(GONE), "x").unwrap();
        let mut w = Watched::new("design", dir.clone());
        assert!(w.poll().is_empty());
        fs::write(dir.join(file::SCREENS).join(HTML_FILE), "ab").unwrap();
        fs::write(dir.join("new.css"), "x").unwrap();
        fs::write(dir.join("skip.json.1.tmp"), "x").unwrap();
        fs::remove_file(dir.join(GONE)).unwrap();
        let mut got = w.poll();
        got.sort();
        let a = format!("design:screens{MAIN_SEPARATOR}a.html");
        let mut want = vec![a, format!("design:{GONE}"), "design:new.css".to_string()];
        want.sort();
        assert_eq!(got, want);
        fs::remove_dir_all(&dir).unwrap();
    }
}
