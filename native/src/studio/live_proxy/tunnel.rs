//! The WebSocket (HMR) tunnel of the review proxy: the client and the app spliced both ways,
//! closed when either side closes or nothing passes for a while.

use std::io::{self, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a WebSocket tunnel may carry nothing in either direction before it is closed, so a
/// stuck peer cannot hold a connection slot for ever (`process-and-io.md` P13). An HMR socket
/// that idles longer reconnects by itself.
const TUNNEL_IDLE: Duration = Duration::from_secs(10 * 60);

/// Copy both ways until either side closes or the tunnel idles for [`TUNNEL_IDLE`], then close
/// both.
pub(super) fn splice(a: TcpStream, b: TcpStream) {
    splice_with(a, b, TUNNEL_IDLE);
}

/// [`splice`] with its idle deadline (short in the tests). Each direction reads with a short
/// timeout and checks the time of the last byte either way; a write that cannot finish within
/// `idle` (a peer that stopped reading) ends the tunnel too.
pub(super) fn splice_with(a: TcpStream, b: TcpStream, idle: Duration) {
    let (Ok(a2), Ok(b2)) = (a.try_clone(), b.try_clone()) else {
        return;
    };
    let poll = (idle / 4).clamp(Duration::from_millis(10), Duration::from_secs(1));
    for s in [&a, &b] {
        let _ = s.set_read_timeout(Some(poll));
        let _ = s.set_write_timeout(Some(idle));
    }
    let start = Instant::now();
    let last = Arc::new(AtomicU64::new(0));
    let last2 = Arc::clone(&last);
    // No thread for the other direction (the system is out of them): drop the connection.
    let Ok(t) = std::thread::Builder::new().spawn(move || {
        pump(a2, b2, start, &last2, idle);
    }) else {
        return;
    };
    pump(b, a, start, &last, idle);
    let _ = t.join();
}

/// One direction of [`splice_with`]: `from` to `to` until a side closes or both idle.
fn pump(mut from: TcpStream, mut to: TcpStream, start: Instant, last: &AtomicU64, idle: Duration) {
    let elapsed = || u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut buf = [0u8; 16 * 1024];
    loop {
        match from.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                last.store(elapsed(), Ordering::Relaxed);
                if to.write_all(&buf[..n]).is_err() {
                    break;
                }
                last.store(elapsed(), Ordering::Relaxed);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                let quiet = elapsed().saturating_sub(last.load(Ordering::Relaxed));
                if u128::from(quiet) >= idle.as_millis() {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    // Closing both ends wakes the other direction at once.
    let _ = from.shutdown(Shutdown::Both);
    let _ = to.shutdown(Shutdown::Both);
}
