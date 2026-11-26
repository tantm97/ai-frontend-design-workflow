//! Plain HTTP GET for dfw's downloads (the headless shell and its index, the dfw binary,
//! axe-core, the fonts a rendered page asks for, the Google Fonts lookup): one place that sets
//! the deadline and the size cap, so no download can hang a command or fill the memory. The
//! telemetry POST and the review proxy keep their own clients (see `telemetry/flush.rs` and
//! `studio/live_proxy.rs` for why).

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use ureq::ResponseExt;

/// A TCP connection that gives up after `deadline` instead of waiting for the system's own
/// timeout (minutes on an address that drops packets), with the same deadline set for reads and
/// writes. Every raw socket dfw opens goes through here: `TcpStream::connect` is a
/// `disallowed-methods` entry in `clippy.toml` (§0 row 21).
pub fn connect(host: &str, port: u16, deadline: Duration) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, format!("no address for {host}"));
    for addr in (host, port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&addr, deadline) {
            Ok(stream) => {
                stream.set_read_timeout(Some(deadline))?;
                stream.set_write_timeout(Some(deadline))?;
                return Ok(stream);
            }
            Err(err) => last = err,
        }
    }
    Err(last)
}

/// What a GET returned.
#[derive(Debug)]
pub struct Fetched {
    pub status: u16,
    /// The URL it ended at, after redirects.
    pub landed: String,
    pub content_type: Option<String>,
    pub body: Vec<u8>,
}

impl Fetched {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// GET `url` with `headers`, following redirects. The whole exchange must end within `timeout`
/// and the body may be at most `limit` bytes. A status that is not 2xx comes back as it is (the
/// callers word it); a refused connection, a timeout or a body over the cap is an error.
pub fn get(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: u64,
) -> Result<Fetched, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .build()
        .into();
    let mut request = agent.get(url);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let mut res = request.call()?;
    let content_type = res
        .headers()
        .get(ureq::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .map(str::to_string);
    let landed = res.get_uri().to_string();
    let status = res.status().as_u16();
    let body = res.body_mut().with_config().limit(limit).read_to_vec()?;
    Ok(Fetched {
        status,
        landed,
        content_type,
        body,
    })
}

/// The loopback address the local servers listen on and tests connect to.
pub const LOOPBACK: &str = "127.0.0.1";

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::serve_routes;
    use std::collections::HashMap;

    #[test]
    fn a_body_over_the_cap_is_an_error_and_a_status_is_returned() {
        /// A route whose body is over the smaller cap.
        const BIG: &str = "/big";
        let (base, _) = serve_routes(HashMap::from([
            (BIG.to_string(), (200, vec![b'x'; 2048])),
            ("/gone".to_string(), (404, b"no".to_vec())),
        ]));
        let t = Duration::from_secs(5);
        let big_url = format!("{base}{BIG}");
        assert!(get(&big_url, &[], t, 1024).is_err());
        let big = get(&big_url, &[], t, 4096).unwrap();
        assert!(big.ok() && big.body.len() == 2048 && big.landed.ends_with(BIG));
        let gone = get(&format!("{base}/gone"), &[], t, 1024).unwrap();
        assert_eq!((gone.status, gone.ok()), (404, false));
    }

    #[test]
    fn connect_gives_up_at_its_deadline() {
        // A port nothing listens on is refused at once; an address that drops packets
        // (TEST-NET-1, RFC 5737) is given up at the deadline, not after the system's minutes.
        let free = std::net::TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = free.local_addr().unwrap().port();
        drop(free);
        assert!(connect(LOOPBACK, port, Duration::from_secs(2)).is_err());
        let start = std::time::Instant::now();
        let dropped = connect("192.0.2.1", 9, Duration::from_millis(300));
        assert!(dropped.is_err());
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "took {:?}",
            start.elapsed()
        );
        // A listening port connects, with the deadline set for reads.
        let up = std::net::TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = up.local_addr().unwrap().port();
        let s = connect(LOOPBACK, port, Duration::from_secs(2)).unwrap();
        assert_eq!(s.read_timeout().unwrap(), Some(Duration::from_secs(2)));
    }

    #[test]
    fn a_server_that_never_answers_is_given_up() {
        let listener = std::net::TcpListener::bind((LOOPBACK, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let started = std::time::Instant::now();
        let r = get(
            &format!("http://127.0.0.1:{port}/"),
            &[],
            Duration::from_millis(300),
            1024,
        );
        assert!(r.is_err());
        assert!(started.elapsed() < Duration::from_secs(3));
        drop(listener);
    }
}
