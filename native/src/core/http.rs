//! Plain HTTP GET for dfw's downloads (the headless shell and its index, the dfw binary,
//! axe-core, the fonts a rendered page asks for, the Google Fonts lookup): one place that sets
//! the deadline and the size cap, so no download can hang a command or fill the memory.
//! [`download`] is the one way to fetch what dfw unpacks, installs, runs or reads from a fixed
//! host: https on the [`Source`]'s host only (also after redirects), verified before the body is
//! handed back (S11). [`get`] stays for the fonts a rendered page asks for (any host the page
//! names). The telemetry POST and the review proxy keep their own clients (see
//! `telemetry/flush.rs` and `studio/live_proxy.rs` for why).

use std::io;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

use sha2::Digest;
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
    fetch(url, headers, timeout, limit, false)
}

/// [`get`], with every redirect to plain http refused when `https_only`.
fn fetch(
    url: &str,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: u64,
    https_only: bool,
) -> Result<Fetched, ureq::Error> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .http_status_as_error(false)
        .https_only(https_only)
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

/// Where a download may come from and how it is checked before use. Each is a constant next to
/// its caller.
#[derive(Debug, Clone, Copy)]
pub struct Source {
    /// The one host the URL may name (over https).
    pub host: &'static str,
    /// The other hosts the download may end at after a redirect (GitHub serves release assets
    /// from its own storage hosts).
    pub redirects: &'static [&'static str],
    pub verify: Verify,
}

/// How a [`download`] is verified before its body is handed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// The body's SHA-256 (hex) must equal the expected one (a release's SHA256SUMS).
    Sha256,
    /// The body's Subresource Integrity (`sha512-<base64>`) must equal the expected one (npm).
    Sri,
    /// No checksum is published (Chrome for Testing): the caller records what it unpacked.
    RecordedOnly,
    /// A document dfw only reads and never runs (an index, a checksum list, a stylesheet).
    Read,
}

/// Why a [`download`] gave nothing back.
#[derive(Debug)]
pub enum Failed {
    /// The URL, or the one a redirect ended at, is not https on the source's hosts.
    Refused { url: String, host: &'static str },
    /// No connection, a timeout or a body over the cap.
    Transport(ureq::Error),
    /// The server answered with a status that is not 2xx.
    Status(u16),
    /// The body is not the one expected (`want` is empty when nothing was given to compare).
    Mismatch { want: String, got: String },
}

impl std::fmt::Display for Failed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Failed::Refused { url, host } => {
                write!(f, "refusing to download {url}: expected https://{host}/...")
            }
            Failed::Transport(err) => write!(f, "{err}"),
            Failed::Status(status) => write!(f, "HTTP {status}"),
            Failed::Mismatch { want, got } => write!(f, "expected {want}, got {got}"),
        }
    }
}

/// The scheme and the host of `url`, both in lower case, when it names no user
/// (`https://user@host/` is refused).
fn scheme_and_host(url: &str) -> Option<(String, String)> {
    let uri: ureq::http::Uri = url.parse().ok()?;
    let authority = uri.authority()?;
    if authority.as_str().contains('@') {
        return None;
    }
    Some((
        uri.scheme_str()?.to_ascii_lowercase(),
        authority.host().to_ascii_lowercase(),
    ))
}

/// Whether `url` may be fetched from `source`: https on its host or, once `landed` after
/// redirects, on one of its redirect hosts. Unit tests also serve fixtures over plain http on
/// the loopback address.
fn allowed(source: &Source, url: &str, landed: bool) -> bool {
    let Some((scheme, host)) = scheme_and_host(url) else {
        return false;
    };
    let on_host = host == source.host || (landed && source.redirects.contains(&host.as_str()));
    let secure = scheme == "https" || (cfg!(test) && scheme == "http" && host == LOOPBACK);
    on_host && secure
}

/// GET `url` from `source` (as [`get`], with `headers`, `timeout` and `limit`) and verify the
/// body. Refused unless the URL, and the one it ended at after redirects, are https on the
/// source's hosts; `expect` is what a [`Verify::Sha256`] or [`Verify::Sri`] source's body is
/// compared with. The body comes back only when it passed.
pub fn download(
    source: &Source,
    url: &str,
    expect: Option<&str>,
    headers: &[(&str, &str)],
    timeout: Duration,
    limit: u64,
) -> Result<Fetched, Failed> {
    let refused = |url: &str| Failed::Refused {
        url: url.to_string(),
        host: source.host,
    };
    if !allowed(source, url, false) {
        return Err(refused(url));
    }
    // Plain http reaches this point only for a unit test's loopback fixture.
    let https_only = !cfg!(test) || url.starts_with("https:");
    let fetched = fetch(url, headers, timeout, limit, https_only).map_err(Failed::Transport)?;
    if !allowed(source, &fetched.landed, true) {
        return Err(refused(&fetched.landed));
    }
    if !fetched.ok() {
        return Err(Failed::Status(fetched.status));
    }
    let got = match source.verify {
        Verify::Sha256 => crate::core::util::sha256(&fetched.body),
        Verify::Sri => format!(
            "sha512-{}",
            crate::core::encoding::base64_encode(&sha2::Sha512::digest(&fetched.body))
        ),
        Verify::RecordedOnly | Verify::Read => return Ok(fetched),
    };
    match expect {
        Some(want) if want == got => Ok(fetched),
        want => Err(Failed::Mismatch {
            want: want.unwrap_or_default().to_string(),
            got,
        }),
    }
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

    /// A fixture source on the loopback server, verified as `verify`.
    const fn local(verify: Verify) -> Source {
        Source {
            host: LOOPBACK,
            redirects: &[],
            verify,
        }
    }

    #[test]
    fn a_download_is_https_on_the_source_host_only() {
        const STORE: Source = Source {
            host: "storage.example",
            redirects: &["cdn.example"],
            verify: Verify::Read,
        };
        /// A file on the redirect host.
        const ON_CDN: &str = "https://cdn.example/a.zip";
        for (url, landed, ok) in [
            ("https://storage.example/a.zip", false, true),
            ("HTTPS://Storage.Example:8443/a.zip", false, true),
            ("http://storage.example/a.zip", false, false),
            ("https://storage.example.evil.example/a.zip", false, false),
            ("https://evil.example/storage.example/a.zip", false, false),
            ("https://user:pw@storage.example/a.zip", false, false),
            ("https://storage.example@evil.example/a.zip", false, false),
            ("file:///tmp/a.zip", false, false),
            ("not a url", false, false),
            ("", false, false),
            // A redirect host is accepted where a download ends, never where it starts.
            (ON_CDN, false, false),
            (ON_CDN, true, true),
            ("http://cdn.example/a.zip", true, false),
            // The fixture exception is plain http on the loopback address only.
            ("http://127.0.0.1:9/a.zip", false, false),
        ] {
            assert_eq!(allowed(&STORE, url, landed), ok, "{url} landed={landed}");
        }
        assert!(allowed(&local(Verify::Read), "http://127.0.0.1:9/", false));
        assert!(!allowed(&local(Verify::Read), "http://localhost:9/", false));
    }

    #[test]
    fn a_download_comes_back_only_verified() {
        const BODY: &[u8] = b"payload";
        let (base, hits) = serve_routes(HashMap::from([
            ("/a".to_string(), (200, BODY.to_vec())),
            ("/gone".to_string(), (404, b"no".to_vec())),
        ]));
        let t = Duration::from_secs(5);
        let url = format!("{base}/a");
        let get = |source: &Source, url: &str, expect: Option<&str>| {
            download(source, url, expect, &[], t, 1024)
        };
        // Another host is refused before anything is asked.
        let other = Source {
            host: "storage.example",
            ..local(Verify::Read)
        };
        let refused = get(&other, &url, None).unwrap_err();
        assert!(matches!(refused, Failed::Refused { .. }), "{refused}");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(matches!(
            get(&local(Verify::Read), &format!("{base}/gone"), None),
            Err(Failed::Status(404))
        ));
        let sha = crate::core::util::sha256(BODY);
        let sri = format!(
            "sha512-{}",
            crate::core::encoding::base64_encode(&sha2::Sha512::digest(BODY))
        );
        // Each kind refuses no checksum, a wrong one and the other kind's right one.
        for (verify, right, other) in [(Verify::Sha256, &sha, &sri), (Verify::Sri, &sri, &sha)] {
            let source = local(verify);
            assert_eq!(get(&source, &url, Some(right)).unwrap().body, BODY);
            for wrong in [None, Some("0"), Some(other.as_str())] {
                let err = get(&source, &url, wrong).unwrap_err();
                assert!(
                    matches!(&err, Failed::Mismatch { got, .. } if got == right),
                    "{verify:?} {wrong:?}: {err}"
                );
            }
        }
        for verify in [Verify::RecordedOnly, Verify::Read] {
            assert_eq!(get(&local(verify), &url, None).unwrap().body, BODY);
        }
    }

    #[test]
    fn a_redirect_off_the_source_host_is_refused() {
        // The fixture answers every request with a redirect to the same server under another
        // name (localhost), which is not the source's host.
        let server = tiny_http::Server::http((LOOPBACK, 0)).unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        std::thread::spawn(move || {
            for request in server.incoming_requests() {
                let elsewhere = request.url() == "/away";
                let response = if elsewhere {
                    tiny_http::Response::from_data(b"other host".to_vec())
                        .with_status_code(302)
                        .with_header(
                            tiny_http::Header::from_bytes(
                                "location",
                                format!("http://localhost:{port}/landed"),
                            )
                            .unwrap(),
                        )
                } else {
                    tiny_http::Response::from_data(b"landed".to_vec())
                };
                let _ = request.respond(response);
            }
        });
        let err = download(
            &local(Verify::Read),
            &format!("http://{LOOPBACK}:{port}/away"),
            None,
            &[],
            Duration::from_secs(5),
            1024,
        )
        .unwrap_err();
        assert!(
            matches!(&err, Failed::Refused { url, .. } if url.starts_with("http://localhost:")),
            "{err}"
        );
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
