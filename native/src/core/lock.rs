//! A lock shared by threads and processes: a folder that holds an `owner` file with a token
//! only its holder knows (`process-and-io.md` P7). The reviews lock (`.dfw/reviews/.lock`) and
//! the edit lock (`.dfw/edits/.lock`) are this lock.
//!
//! The protocol, the same in every dfw process:
//! - **take**: make `<lock>.<token>.new.tmp` with the owner file, then rename it to `<lock>`. The
//!   rename fails while `<lock>` exists, so the lock never exists without its owner.
//! - **wait**: retry every `retry`. A lock older than `stale`, or held by the same owner for
//!   `wait`, was left by a process that died. The one waiter that makes the folder
//!   `<lock>/takeover` (atomic, one winner) renames the lock to `<lock>.<token>.stale.tmp` and
//!   removes it; every waiter starts its wait again for the next holder.
//! - **release**: remove the lock only when its owner file still holds your token, so a holder
//!   that was taken over never removes the lock of the next one.
//!
//! A lock folder without an owner file (left by dfw 0.6.0 or earlier) is taken over the same way.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use super::error::DfwError;
use super::util::unique_suffix;

/// The timings of a lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockTimes {
    /// A holder seen this long by one waiter is taken over.
    pub wait: Duration,
    /// A lock older than this is taken over at once.
    pub stale: Duration,
    /// Pause between two tries.
    pub retry: Duration,
}

/// The timings of the reviews and edit locks: retry every 15 ms, take over after 5 s of waiting
/// for one holder or when the lock is older than 10 s.
pub const STANDARD: LockTimes = LockTimes {
    wait: Duration::from_millis(5000),
    stale: Duration::from_millis(10_000),
    retry: Duration::from_millis(15),
};

const OWNER: &str = "owner";
const TAKEOVER: &str = "takeover";

/// A held lock; dropping it releases the lock when it is still this holder's.
#[derive(Debug)]
pub struct Lock {
    path: PathBuf,
    token: String,
}

impl Drop for Lock {
    fn drop(&mut self) {
        if owner(&self.path).as_deref() != Some(self.token.as_str()) {
            return; // taken over: the lock there now is someone else's
        }
        let aside = sibling(&self.path, &self.token, "old");
        if fs::rename(&self.path, &aside).is_ok() {
            // Cleanup: the lock is released either way.
            let _ = fs::remove_dir_all(&aside);
        }
    }
}

/// Take the lock at `path`, waiting (and taking over a dead holder's lock) as long as it takes.
pub fn acquire(path: &Path, times: &LockTimes) -> Result<Lock, DfwError> {
    acquire_within(path, times, None)?
        .ok_or_else(|| DfwError::internal(format!("Could not take the lock {}.", path.display())))
}

/// Take the lock at `path`, or give up after `give_up` and return `None` (a hook on a hot path,
/// `process-and-io.md` P8). `None` for `give_up` waits as [`acquire`] does.
pub fn acquire_within(
    path: &Path,
    times: &LockTimes,
    give_up: Option<Duration>,
) -> Result<Option<Lock>, DfwError> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let token = unique_suffix();
    let start = Instant::now();
    // The holder this waiter is waiting for and since when; a new holder starts a new wait.
    let mut waiting: Option<(Option<String>, Instant)> = None;
    loop {
        if try_take(path, &token)? {
            return Ok(Some(Lock {
                path: path.to_path_buf(),
                token,
            }));
        }
        let Ok(modified) = fs::metadata(path).and_then(|m| m.modified()) else {
            continue; // released meanwhile
        };
        let holder = owner(path);
        match &waiting {
            Some((seen, _)) if *seen == holder => {}
            _ => waiting = Some((holder.clone(), Instant::now())),
        }
        let waited = waiting
            .as_ref()
            .map_or(Duration::ZERO, |(_, at)| at.elapsed());
        let age = SystemTime::now()
            .duration_since(modified)
            .unwrap_or_default();
        if age > times.stale || waited > times.wait {
            take_over(path, &holder, &token, times);
            waiting = None;
            continue;
        }
        if give_up.is_some_and(|limit| start.elapsed() >= limit) {
            return Ok(None);
        }
        thread::sleep(times.retry);
    }
}

/// One try: true when this caller now holds the lock.
fn try_take(path: &Path, token: &str) -> Result<bool, DfwError> {
    if fs::symlink_metadata(path).is_ok() {
        return Ok(false);
    }
    let new = sibling(path, token, "new");
    fs::create_dir(&new)?;
    let made = fs::write(new.join(OWNER), token).and_then(|()| {
        // Fails while the lock exists (Unix: not empty; Windows: access denied); on Unix a
        // rename onto an empty folder succeeds, which only an owner-less lock of an older dfw is.
        match fs::rename(&new, path) {
            Err(err)
                if path.exists()
                    || matches!(
                        err.kind(),
                        io::ErrorKind::AlreadyExists
                            | io::ErrorKind::DirectoryNotEmpty
                            | io::ErrorKind::PermissionDenied
                    ) =>
            {
                Err(io::Error::new(io::ErrorKind::AlreadyExists, err))
            }
            other => other,
        }
    });
    match made {
        Ok(()) => Ok(true),
        Err(err) => {
            // Cleanup: our own temporary folder.
            let _ = fs::remove_dir_all(&new);
            if err.kind() == io::ErrorKind::AlreadyExists {
                Ok(false)
            } else {
                Err(err.into())
            }
        }
    }
}

/// Move a dead holder's lock aside and remove it. Of many waiters that judged the same lock,
/// only the one that makes the `takeover` folder inside it goes on; it checks that the owner
/// is still the judged one, then renames the lock away (atomic). A waiter whose claim landed
/// in a newer lock sees the new owner and removes its claim again.
fn take_over(path: &Path, judged: &Option<String>, token: &str, times: &LockTimes) {
    let claim = path.join(TAKEOVER);
    if fs::create_dir(&claim).is_err() {
        // Another waiter is taking it over, or the lock is gone. A claim left by a waiter that
        // died half way is removed once it is stale.
        let old = fs::metadata(&claim)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|m| SystemTime::now().duration_since(m).ok());
        if old.is_some_and(|age| age > times.stale) {
            // Cleanup: a dead waiter's claim.
            let _ = fs::remove_dir(&claim);
        }
        return;
    }
    if owner(path) != *judged {
        // Cleanup: our claim in a lock that is not the one we judged.
        let _ = fs::remove_dir(&claim);
        return;
    }
    let aside = sibling(path, token, "stale");
    if fs::rename(path, &aside).is_err() {
        let _ = fs::remove_dir(&claim); // cleanup: the lock is gone already
        return;
    }
    if owner(&aside) != *judged {
        // The judged holder released and a new one took the lock between the check and the
        // rename: give it back.
        let _ = fs::remove_dir(aside.join(TAKEOVER));
        let _ = fs::rename(&aside, path);
        return;
    }
    // Cleanup: the dead holder's folder.
    let _ = fs::remove_dir_all(&aside);
}

fn owner(path: &Path) -> Option<String> {
    fs::read_to_string(path.join(OWNER)).ok()
}

/// `<lock>.<token>.<what>.tmp`, next to the lock (the Studio's watcher skips `.tmp` names).
fn sibling(path: &Path, token: &str, what: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(format!(".{token}.{what}.tmp"));
    PathBuf::from(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const FAST: LockTimes = LockTimes {
        wait: Duration::from_millis(300),
        stale: Duration::from_millis(10_000),
        retry: Duration::from_millis(2),
    };

    /// Many threads under one lock: the counter they increment by read-sleep-write loses nothing.
    #[test]
    fn one_holder_at_a_time() {
        let dir = test_dir("lock-threads");
        let lock = dir.join(".lock");
        let file = dir.join("n");
        fs::write(&file, "0").unwrap();
        thread::scope(|s| {
            for _ in 0..12 {
                s.spawn(|| {
                    for _ in 0..10 {
                        let _held = acquire(&lock, &FAST).unwrap();
                        let n: u32 = fs::read_to_string(&file).unwrap().parse().unwrap();
                        thread::sleep(Duration::from_micros(200));
                        fs::write(&file, (n + 1).to_string()).unwrap();
                    }
                });
            }
        });
        assert_eq!(fs::read_to_string(&file).unwrap(), "120");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no lock folder is left"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A holder that never releases (a crashed process) is taken over by one waiter, and the
    /// late release of the old holder does not remove the new holder's lock.
    #[test]
    fn a_dead_holder_is_taken_over_once_and_never_releases_the_next() {
        let dir = test_dir("lock-takeover");
        let lock = dir.join(".lock");
        let dead = acquire(&lock, &FAST).unwrap();
        let inside = AtomicUsize::new(0);
        let most = AtomicUsize::new(0);
        thread::scope(|s| {
            for _ in 0..20 {
                s.spawn(|| {
                    let held = acquire(&lock, &FAST).unwrap();
                    assert_eq!(owner(&lock).as_deref(), Some(held.token.as_str()));
                    let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    thread::sleep(Duration::from_millis(5));
                    inside.fetch_sub(1, Ordering::SeqCst);
                });
            }
            // The old holder wakes up while the others work: its release is a no-op.
            thread::sleep(Duration::from_millis(400));
            drop(dead);
        });
        assert_eq!(most.load(Ordering::SeqCst), 1, "never two holders at once");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "nothing is left");
        fs::remove_dir_all(&dir).unwrap();
    }

    /// An owner-less lock folder (dfw 0.6.0) older than `stale` is taken over; a fresh one is
    /// waited for, and `acquire_within` gives up in time.
    #[test]
    fn an_old_lock_folder_is_taken_over_and_a_busy_lock_can_be_given_up() {
        let dir = test_dir("lock-legacy");
        let lock = dir.join(".lock");
        fs::create_dir(&lock).unwrap();
        let quick = LockTimes {
            wait: Duration::from_secs(60),
            stale: Duration::from_secs(60),
            retry: Duration::from_millis(2),
        };
        let started = Instant::now();
        assert!(
            acquire_within(&lock, &quick, Some(Duration::from_millis(30)))
                .unwrap()
                .is_none()
        );
        assert!(started.elapsed() < Duration::from_secs(1));
        let old = LockTimes {
            stale: Duration::ZERO,
            ..quick
        };
        let held = acquire_within(&lock, &old, Some(Duration::from_millis(30)))
            .unwrap()
            .expect("an old lock is taken over");
        assert_eq!(owner(&lock).as_deref(), Some(held.token.as_str()));
        drop(held);
        assert!(!lock.exists());
        fs::remove_dir_all(&dir).unwrap();
    }
}
