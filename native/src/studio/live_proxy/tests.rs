//! Ported from tests/unit/inspector.test.mjs (the live.mjs cases) and studio-guard.test.mjs.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;

use super::proxy::LiveServer;
use super::*;
use crate::core::util::test_dir;
use crate::review::comments::all_comments;

fn tags() -> String {
    live_tags(None, DEFAULT_SHORTCUT)
}

/// injectLive / assertLocalTarget
#[test]
fn inject_live_and_assert_local_target() {
    assert_eq!(
        inject_live(
            "<html><head><title>x</title></head><body></body></html>",
            None,
            DEFAULT_SHORTCUT
        ),
        format!(
            "<html><head><title>x</title>{}</head><body></body></html>",
            tags()
        )
    );
    assert_eq!(
        inject_live("<body class=\"a\"><p>x</p></body>", None, DEFAULT_SHORTCUT),
        format!("<body class=\"a\">{}<p>x</p></body>", tags())
    );
    let once = inject_live("<head></head>", None, DEFAULT_SHORTCUT);
    assert_eq!(inject_live(&once, None, DEFAULT_SHORTCUT), once, "once");
    assert_eq!(
        inject_live("<p>x</p>", None, "c"),
        format!("{}<p>x</p>", live_tags(None, "c"))
    );
    assert_eq!(
        assert_local_target("http://localhost:5173/cart")
            .unwrap()
            .origin(),
        "http://localhost:5173"
    );
    assert!(assert_local_target("http://127.0.0.1:3000").is_ok());
    assert!(assert_local_target("http://[::1]:3000").is_ok());
    assert!(assert_local_target("http://app.localhost").is_ok());
    let err = |raw: &str| assert_local_target(raw).unwrap_err().message().to_string();
    assert!(err("https://example.com").contains("only proxies a local dev server"));
    assert!(err("file:///etc/passwd").contains("http(s)"));
    assert_eq!(err("nothing"), "Not a URL: nothing");
    assert_eq!(
        assert_local_target("http://localhost:80/")
            .unwrap()
            .origin(),
        "http://localhost"
    );
    assert_eq!(
        assert_local_target("HTTP://LocalHost:5173/a?b")
            .unwrap()
            .pathname(),
        "/a"
    );
}

/// review shortcut: normalised combos, Alt+C default, invalid combos refused
#[test]
fn review_shortcuts() {
    assert_eq!(normalize_shortcut("Alt+C").unwrap(), "alt+c");
    assert_eq!(normalize_shortcut("option+c").unwrap(), "alt+c");
    assert_eq!(normalize_shortcut("shift+ctrl+k").unwrap(), "ctrl+shift+k");
    assert_eq!(normalize_shortcut("cmd+shift+1").unwrap(), "shift+meta+1");
    assert_eq!(
        normalize_shortcut("c").unwrap(),
        "c",
        "a bare key is allowed when the user asks for it"
    );
    for off in ["off", "none", "", "false"] {
        assert_eq!(normalize_shortcut(off).unwrap(), "off");
    }
    for bad in ["alt", "alt+cc", "alt+c+d", "hyper+c", "ctrl+enter"] {
        let e = normalize_shortcut(bad).unwrap_err();
        assert!(e.message().contains("Invalid review shortcut"), "{bad}");
    }
    assert!(
        tags().contains("data-shortcut=\"alt+c\""),
        "Alt/Option+C by default, not a bare C"
    );
    assert!(live_tags(Some("a\"b"), "off").contains("data-shortcut=\"off\" nonce=\"a&quot;b\""));
}

/// patchCsp: adds the proxy origin + nonce for the injected scripts and the comment POST, nothing else
#[test]
fn patch_csp_adds_only_what_the_injected_scripts_need() {
    let o = "http://localhost:4700";
    let n = Some("N0nce");
    // strict-dynamic: host sources are ignored, so the nonce is what lets the scripts run.
    assert_eq!(
        patch_csp("default-src 'none'; script-src 'strict-dynamic' 'nonce-app' https:; style-src 'self'; img-src data:; frame-ancestors 'none'; report-uri /csp", o, n),
        "default-src 'none'; script-src 'strict-dynamic' 'nonce-app' https: http://localhost:4700 'nonce-N0nce'; style-src 'self'; img-src data:; frame-ancestors 'none'; report-uri /csp; connect-src http://localhost:4700"
    );
    // default-src only: script-src/connect-src are created from it, default-src itself untouched.
    assert_eq!(
        patch_csp("default-src 'self' https://cdn.example", o, n),
        "default-src 'self' https://cdn.example; script-src 'self' https://cdn.example 'nonce-N0nce'; connect-src 'self' https://cdn.example"
    );
    // 'unsafe-inline' without nonces: adding a nonce would disable the app's inline scripts.
    assert_eq!(
        patch_csp("script-src https://cdn.example 'unsafe-inline'", o, n),
        "script-src https://cdn.example 'unsafe-inline' http://localhost:4700"
    );
    // script-src-elem and script-src both patched; several policies patched one by one.
    assert_eq!(
        patch_csp("script-src-elem 'self'; script-src 'none', connect-src 'self'", o, n),
        "script-src-elem 'self' 'nonce-N0nce'; script-src http://localhost:4700 'nonce-N0nce', connect-src 'self'"
    );
    assert_eq!(
        patch_csp("img-src 'self'", o, n),
        "img-src 'self'",
        "scripts and fetch not restricted: unchanged"
    );
    assert_eq!(patch_csp("", o, n), "");
    let meta = patch_meta_csp(
        "<head><meta http-equiv=\"Content-Security-Policy\" content=\"script-src &#39;nonce-app&#39;\"><META HTTP-EQUIV='content-security-policy' CONTENT=\"default-src 'self'\"></head>",
        o,
        n,
    );
    assert_eq!(meta, "<head><meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'nonce-app' http://localhost:4700 'nonce-N0nce'\"><META HTTP-EQUIV='content-security-policy' content=\"default-src 'self'; script-src 'self' 'nonce-N0nce'; connect-src 'self'\"></head>");
}

/// fixSetCookie: only a Domain naming the other loopback host is dropped
#[test]
fn fix_set_cookie_drops_only_the_other_loopback_domain() {
    let cookies: Vec<String> = [
        "a=1; Domain=127.0.0.1; Path=/",
        "b=2; domain=localhost; HttpOnly",
        "c=3; Path=/",
        "d=4; Domain=example.test",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        fix_set_cookie(&cookies, "localhost"),
        [
            "a=1; Path=/",
            "b=2; domain=localhost; HttpOnly",
            "c=3; Path=/",
            "d=4; Domain=example.test"
        ]
    );
    assert_eq!(
        fix_set_cookie(&["a=1; Domain=.localhost".into()], "127.0.0.1"),
        ["a=1"]
    );
}

/* ---------------------------------------------------------------- the proxy on a socket */

/// One HTTP exchange: the raw answer.
fn call(port: u16, raw: &str) -> String {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(raw.as_bytes()).unwrap();
    let mut out = Vec::new();
    s.read_to_end(&mut out).unwrap();
    String::from_utf8_lossy(&out).into_owned()
}

fn get(port: u16, path: &str, host: &str) -> String {
    call(
        port,
        &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n"),
    )
}

fn body(answer: &str) -> &str {
    answer.split_once("\r\n\r\n").map_or("", |(_, b)| b)
}

fn header<'a>(answer: &'a str, name: &str) -> Vec<&'a str> {
    let head = answer.split_once("\r\n\r\n").map_or(answer, |(h, _)| h);
    head.lines()
        .filter_map(|l| l.split_once(": "))
        .filter(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v)
        .collect()
}

const APP_HTML: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>Shop</title></head>\n<body><main data-insp-path=\"src/pages/Cart.tsx:10:5:main\"><h1>Cart</h1>\n<section data-insp-path=\"ABS/src/components/Summary.tsx:42:7:section\"><p id=\"total\"><span class=\"amount\">$12.00</span></p></section>\n<button id=\"plain\">No source</button></main></body></html>";

/// A tiny "dev server": HTML with data-insp-path, a gzip route, a redirect, a login cookie, a
/// strict CSP page and a WebSocket-style upgrade echo.
fn fake_app(root: &str) -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let html = APP_HTML.replace("ABS", root);
    thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let html = html.clone();
            thread::spawn(move || app_answer(stream, port, &html));
        }
    });
    port
}

fn app_answer(mut s: TcpStream, port: u16, html: &str) {
    let Ok(Some(head)) = crate::studio::http::read_head(&mut s) else {
        return;
    };
    if head.is_upgrade() {
        let host = head.header("host").unwrap_or_default().to_string();
        let _ = s.write_all(format!("HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nX-Seen-Host: {host}\r\n\r\n").as_bytes());
        let mut buf = [0u8; 64];
        while let Ok(n) = s.read(&mut buf) {
            if n == 0
                || s.write_all(format!("echo:{}", String::from_utf8_lossy(&buf[..n])).as_bytes())
                    .is_err()
            {
                break;
            }
        }
        return;
    }
    let reply = |s: &mut TcpStream, head: &str, body: &[u8]| {
        let _ = s.write_all(
            format!("HTTP/1.1 {head}\r\nContent-Length: {}\r\n\r\n", body.len()).as_bytes(),
        );
        let _ = s.write_all(body);
    };
    match head.target() {
        "/gz" => {
            let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
            gz.write_all(b"<html><head></head><body>gz</body></html>").unwrap();
            reply(&mut s, "200 OK\r\nContent-Type: text/html\r\nContent-Encoding: gzip", &gz.finish().unwrap());
        }
        "/old" => reply(&mut s, &format!("302 Found\r\nLocation: http://127.0.0.1:{port}/cart"), b""),
        "/app.js" => reply(&mut s, "200 OK\r\nContent-Type: text/javascript", b"console.log(1)"),
        "/login" => reply(&mut s, "200 OK\r\nContent-Type: text/plain\r\nSet-Cookie: sid=s3cret; Path=/; HttpOnly; SameSite=Lax\r\nSet-Cookie: pref=1; Domain=127.0.0.1; Path=/", b"ok"),
        "/strict" => reply(&mut s, "200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'; script-src 'nonce-app' 'strict-dynamic'; style-src 'self'; img-src 'self'; frame-ancestors 'none'\r\nContent-Security-Policy-Report-Only: script-src 'nonce-app'", format!("<html><head><meta http-equiv=\"Content-Security-Policy\" content=\"script-src 'nonce-app' 'strict-dynamic'\"></head><body><script nonce=\"app\"></script><p>{}</p></body></html>", head.header("cookie").unwrap_or("anon")).as_bytes()),
        _ => reply(&mut s, "200 OK\r\nContent-Type: text/html; charset=utf-8\r\nETag: \"x\"", html.as_bytes()),
    }
}

fn proxy_for(root: &std::path::Path, target: &str) -> (u16, Arc<LiveServer>) {
    let opts = LiveOptions {
        root: root.to_path_buf(),
        target: target.into(),
        shots: false,
        shortcut: None,
    };
    let (server, mut listeners, _) = start_live_server(&opts, 0).unwrap();
    let first = listeners.remove(0);
    let port = first.local_addr().unwrap().port();
    for l in std::iter::once(first).chain(listeners) {
        let s = Arc::clone(&server);
        thread::spawn(move || s.serve(l));
    }
    (port, server)
}

fn post_comment(port: u16, extra: &str, json: &str) -> String {
    call(port, &format!("POST /__dfw/api/comments HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{json}", json.len()))
}

/// dfw review proxy: injects the overlay into HTML, passes assets, redirects and upgrades
/// through, stores comments with source
#[test]
fn the_proxy_injects_passes_through_and_stores_comments() {
    let root = test_dir("live-proxy");
    let abs_root = root.to_string_lossy().replace('\\', "/");
    let app = fake_app(&abs_root);
    let (port, _server) = proxy_for(&root, &format!("http://127.0.0.1:{app}"));
    let host = format!("127.0.0.1:{port}");
    let page = get(port, "/cart", &host);
    let html = body(&page);
    let re = regex::Regex::new(r#"<script defer src="/__dfw/overlay\.js" data-dfw-live nonce="[A-Za-z0-9+/=]+"></script><script defer src="/__dfw/live\.js" data-shortcut="alt\+c" nonce="[A-Za-z0-9+/=]+"></script></head>"#).unwrap();
    assert!(re.is_match(html), "overlay injected: {html}");
    assert!(
        header(&page, "etag").is_empty(),
        "injected HTML is not cached under the upstream etag"
    );
    assert!(
        body(&get(port, "/gz", &host)).contains("data-dfw-live"),
        "gzip HTML decoded and injected"
    );
    assert_eq!(
        body(&get(port, "/app.js", &host)),
        "console.log(1)",
        "assets untouched"
    );
    assert_eq!(
        header(&get(port, "/old", &host), "location"),
        [format!("http://{host}/cart")],
        "redirects stay on the proxy"
    );
    for f in ["overlay.js", "live.js"] {
        assert!(get(port, &format!("/__dfw/{f}"), &host).starts_with("HTTP/1.1 200"));
    }

    // WebSocket upgrade (HMR) passthrough.
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(format!("GET /hmr HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").as_bytes()).unwrap();
    let mut buf = String::new();
    let mut chunk = [0u8; 256];
    let mut pinged = false;
    while !buf.contains("echo:ping") {
        let n = s.read(&mut chunk).unwrap();
        assert!(n > 0, "closed early: {buf}");
        buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
        if buf.contains("\r\n\r\n") && !pinged {
            s.write_all(b"ping").unwrap();
            pinged = true;
        }
    }
    assert!(buf.starts_with("HTTP/1.1 101"));
    assert!(
        buf.contains(&format!("X-Seen-Host: 127.0.0.1:{app}")),
        "Host rewritten to the app"
    );

    // A comment as live.js posts it: absolute data-insp-path -> project-relative source.
    let abs = format!("{abs_root}/src/components/Summary.tsx:42:7:section");
    let c = post_comment(port, "", &serde_json::json!({ "comment": "Total should be bold", "screen": "/cart", "url": "/cart?x=1", "viewport": [390, 844], "target": { "selector": "#total > span", "tag": "span", "text": "$12.00", "insp": abs, "source": "evil/should-not-pass.ts:1" } }).to_string());
    assert!(c.starts_with("HTTP/1.1 201"), "{c}");
    let c: Value = serde_json::from_str(body(&c)).unwrap();
    assert_eq!(c["target"]["source"], "src/components/Summary.tsx:42:7");
    assert_eq!(c["target"]["inspPath"], abs);
    assert_eq!(
        serde_json::json!([
            c["screen"],
            c["variant"],
            c["url"],
            c["breakpoint"],
            c["provider"],
            c["file"]
        ]),
        serde_json::json!(["/cart", "live", "/cart?x=1", "mobile", "dfw-live", null])
    );
    // Without the inspector attribute: selector + text only.
    let c2 = post_comment(
        port,
        "",
        r##"{"comment":"Make it primary","url":"/cart","target":{"selector":"#plain","text":"No source"}}"##,
    );
    let c2: Value = serde_json::from_str(body(&c2)).unwrap();
    assert_eq!(c2["target"]["source"], Value::Null);
    assert_eq!(c2["target"]["selector"], "#plain");
    assert!(post_comment(port, "", r#"{"comment":"  "}"#).starts_with("HTTP/1.1 400"));
    assert_eq!(all_comments(&root).unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}

/// dfw review refuses comments from another origin and requests for another host
#[test]
fn the_proxy_refuses_foreign_origins_and_hosts() {
    let root = test_dir("live-guard");
    let (port, _server) = proxy_for(&root, "http://127.0.0.1:9");
    let comment = r#"{"comment":"from another site","screen":"home"}"#;
    assert!(
        post_comment(port, "Origin: https://evil.example\r\n", comment).starts_with("HTTP/1.1 403")
    );
    assert!(get(port, "/", "evil.example").starts_with("HTTP/1.1 403"));
    assert_eq!(all_comments(&root).unwrap().len(), 0);
    assert!(post_comment(
        port,
        &format!("Origin: http://127.0.0.1:{port}\r\n"),
        comment
    )
    .starts_with("HTTP/1.1 201"));
    let _ = std::fs::remove_dir_all(&root);
}

/// dfw review accepts writes and WebSocket upgrades only from its own origin: another
/// loopback port is another site
#[test]
fn the_proxy_refuses_other_loopback_origins_and_foreign_upgrades() {
    let root = test_dir("live-origins");
    let app = fake_app("");
    let (port, _server) = proxy_for(&root, &format!("http://127.0.0.1:{app}"));
    let comment = r#"{"comment":"from another local site"}"#;
    for origin in [
        "http://localhost:8888",
        "http://127.0.0.1:1",
        "https://localhost:{port}",
    ] {
        let origin = origin.replace("{port}", &port.to_string());
        let r = post_comment(port, &format!("Origin: {origin}\r\n"), comment);
        assert!(r.starts_with("HTTP/1.1 403"), "{origin}: {r}");
    }
    let pass = call(port, &format!("POST /echo HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://localhost:8888\r\nContent-Length: 2\r\n\r\nhi"));
    assert!(pass.starts_with("HTTP/1.1 403"), "{pass}");
    assert_eq!(all_comments(&root).unwrap().len(), 0);
    for origin in [
        format!("http://localhost:{port}"),
        format!("http://[::1]:{port}"),
    ] {
        let r = post_comment(port, &format!("Origin: {origin}\r\n"), comment);
        assert!(r.starts_with("HTTP/1.1 201"), "{origin}: {r}");
    }
    let upgrade = |extra: &str| {
        call(port, &format!("GET /hmr HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{extra}Upgrade: websocket\r\nConnection: Upgrade\r\n\r\n"))
    };
    let r = upgrade("Origin: https://evil.example\r\n");
    assert!(r.starts_with("HTTP/1.1 403"), "{r}");
    let r = call(port, "GET /hmr HTTP/1.1\r\nHost: evil.example\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n");
    assert!(r.starts_with("HTTP/1.1 403"), "{r}");
    // The page's own HMR socket still upgrades.
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(format!("GET /hmr HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n").as_bytes()).unwrap();
    let mut chunk = [0u8; 256];
    let n = s.read(&mut chunk).unwrap();
    assert!(String::from_utf8_lossy(&chunk[..n]).starts_with("HTTP/1.1 101"));
    let _ = std::fs::remove_dir_all(&root);
}

/// dfw review answers every limit: a comment over MAX_COMMENT (413), a chunk size that does
/// not fit (413, no crash), a body that is not JSON (400)
#[test]
fn the_proxy_answers_requests_over_its_limits() {
    let root = test_dir("live-limits");
    let (port, _server) = proxy_for(&root, "http://127.0.0.1:9");
    let big = format!(r#"{{"comment":"{}"}}"#, "x".repeat(1_000_001));
    let r = post_comment(port, "", &big);
    assert!(
        r.starts_with("HTTP/1.1 413") && body(&r) == r#"{"error":"Request too large."}"#,
        "{r}"
    );
    let r = call(port, &format!("POST /__dfw/api/comments HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\nabc"));
    assert!(r.starts_with("HTTP/1.1 413"), "{r}");
    let r = call(port, &format!("POST /app HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nTransfer-Encoding: chunked\r\n\r\nffffffffffffffff\r\nabc"));
    assert!(r.starts_with("HTTP/1.1 413"), "{r}");
    let r = post_comment(port, "", "{not json");
    assert!(
        r.starts_with("HTTP/1.1 400")
            && body(&r) == r#"{"error":"The request body is not valid JSON."}"#,
        "{r}"
    );
    let r = call(port, &format!("POST /__dfw/api/comments HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Length: -1\r\n\r\n"));
    assert!(r.starts_with("HTTP/1.1 400"), "{r}");
    assert!(get(port, "/__dfw/api/live", &format!("127.0.0.1:{port}")).starts_with("HTTP/1.1 200"));
    let _ = std::fs::remove_dir_all(&root);
}

/// a compressed page that decodes past the cap is not decoded (a gzip bomb), it passes as sent
#[test]
fn decoding_stops_at_the_cap() {
    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    gz.write_all(&vec![b'a'; 1_000_000]).unwrap();
    let packed = gz.finish().unwrap();
    assert!(packed.len() < 10_000);
    assert_eq!(super::proxy::decode(&packed, Some("gzip"), 999_999), None);
    assert_eq!(
        super::proxy::decode(&packed, Some("gzip"), 1_000_000).map(|d| d.len()),
        Some(1_000_000)
    );
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
    z.write_all(&vec![b'a'; 100_000]).unwrap();
    assert_eq!(
        super::proxy::decode(&z.finish().unwrap(), Some("deflate"), 1000),
        None
    );
}

/// dfw review proxy: app down -> readable 502
#[test]
fn the_proxy_answers_502_when_the_app_is_down() {
    let root = test_dir("live-down");
    let free = TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let (port, _server) = proxy_for(&root, &format!("http://127.0.0.1:{free}"));
    let r = get(port, "/", &format!("127.0.0.1:{port}"));
    assert!(r.starts_with("HTTP/1.1 502"), "{r}");
    assert!(body(&r).contains("not reachable"));
    let _ = std::fs::remove_dir_all(&root);
}

/// dfw review proxy: strict CSP is extended for the injected scripts only; cookies pass both ways
#[test]
fn strict_csp_is_extended_and_cookies_pass() {
    let root = test_dir("live-csp");
    let app = fake_app("");
    let opts = LiveOptions {
        root: root.clone(),
        target: format!("http://localhost:{app}"),
        shots: false,
        shortcut: Some("Option+C".into()),
    };
    let (server, listeners, url) = start_live_server(&opts, 0).unwrap();
    let port = listeners[0].local_addr().unwrap().port();
    assert_eq!(
        url,
        format!("http://localhost:{port}/"),
        "localhost app -> localhost proxy URL (same cookie jar)"
    );
    assert_eq!(
        listeners[0].local_addr().unwrap().ip().to_string(),
        "127.0.0.1",
        "loopback only, never 0.0.0.0"
    );
    for l in listeners {
        let s = Arc::clone(&server);
        thread::spawn(move || s.serve(l));
    }
    let host = format!("localhost:{port}");
    let res = call(
        port,
        &format!("GET /strict HTTP/1.1\r\nHost: {host}\r\nCookie: sid=s3cret\r\n\r\n"),
    );
    let html = body(&res);
    let nonce = regex::Regex::new(r#"data-dfw-live nonce="([^"]+)""#)
        .unwrap()
        .captures(html)
        .unwrap()[1]
        .to_string();
    assert!(
        html.contains("data-shortcut=\"alt+c\""),
        "--shortcut normalised into the page"
    );
    assert_eq!(header(&res, "content-security-policy"), [format!("default-src 'none'; script-src 'nonce-app' 'strict-dynamic' http://localhost:{port} 'nonce-{nonce}'; style-src 'self'; img-src 'self'; frame-ancestors 'none'; connect-src http://localhost:{port}")]);
    assert_eq!(
        header(&res, "content-security-policy-report-only"),
        [format!(
            "script-src 'nonce-app' http://localhost:{port} 'nonce-{nonce}'"
        )]
    );
    assert!(html.contains(&format!("content=\"script-src 'nonce-app' 'strict-dynamic' http://localhost:{port} 'nonce-{nonce}'\"")), "meta CSP patched too");
    assert!(
        html.contains("<script nonce=\"app\">"),
        "the app's own nonce untouched"
    );
    assert!(
        html.contains("<p>sid=s3cret</p>"),
        "Cookie header forwarded to the app"
    );
    let second = call(
        port,
        &format!("GET /strict HTTP/1.1\r\nHost: {host}\r\n\r\n"),
    );
    assert!(
        !body(&second).contains(&nonce),
        "a fresh nonce per response"
    );
    let login = get(port, "/login", &host);
    assert_eq!(
        header(&login, "set-cookie"),
        [
            "sid=s3cret; Path=/; HttpOnly; SameSite=Lax",
            "pref=1; Path=/"
        ],
        "Domain=127.0.0.1 dropped for a localhost page, the rest untouched"
    );
    let live: Value = serde_json::from_str(body(&get(
        port,
        "/__dfw/api/live",
        &format!("127.0.0.1:{port}"),
    )))
    .unwrap();
    assert_eq!(live["url"], url);
    let _ = std::fs::remove_dir_all(&root);
}

/// A headless browser page of `width` x `height`, for the tests that need Chrome.
fn browser_page(
    dir: &std::path::Path,
    width: u32,
    height: u32,
) -> (
    crate::browser::launch::Browser,
    crate::browser::cdp::Context,
    crate::browser::cdp::Page,
) {
    let tmp = dir.join("browser-tmp");
    std::fs::create_dir_all(&tmp).unwrap();
    let browser = crate::browser::launch::launch_with(&crate::browser::launch::LaunchOptions {
        temp_dir: Some(tmp),
        ..Default::default()
    })
    .unwrap();
    let context = browser.new_context(Default::default()).unwrap();
    let mut page = context.new_page().unwrap();
    page.set_viewport_size(width, height).unwrap();
    (browser, context, page)
}

/// inspector: dfw review in a browser: click-to-comment carries the nearest data-insp-path (needs Chrome)
#[test]
#[ignore = "needs Chrome"]
fn review_in_a_browser_click_to_comment_carries_the_nearest_insp_path() {
    let root = test_dir("live-browser");
    std::fs::create_dir_all(root.join(".dfw")).unwrap();
    std::fs::write(
        root.join(".dfw/state.json"),
        "{\"schema\":1,\"mode\":\"existing-project\",\"phase\":\"verify\",\"active\":true}\n",
    )
    .unwrap();
    let abs_root = root.to_string_lossy().replace('\\', "/");
    let app = fake_app(&abs_root);
    let (port, _server) = proxy_for(&root, &format!("http://127.0.0.1:{app}"));
    let (browser, _context, page) = browser_page(&root, 1440, 900);
    page.goto(&format!("http://127.0.0.1:{port}/cart"), 30_000)
        .unwrap();
    let out = page
        .evaluate(
            r#"async () => {
      for (let i = 0; i < 50 && !window.__dfwLive; i++) await new Promise((res) => setTimeout(res, 50));
      const ui = document.querySelector('dfw-live').shadowRoot;
      // Option+C on macOS types "ç": matched by event.code.
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'ç', code: 'KeyC', altKey: true, bubbles: true }));
      if (ui.querySelector('.toggle').getAttribute('aria-pressed') !== 'true') return 'comment mode not on';
      document.querySelector('#total .amount').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      if (ui.querySelector('.pop').hidden) return 'composer not open';
      const src = ui.querySelector('.src').textContent;
      ui.querySelector('textarea').value = 'Total should be bold';
      ui.querySelector('.pop').requestSubmit();
      for (let i = 0; i < 50 && !/Sent to Claude/.test(ui.querySelector('.toast').textContent); i++) await new Promise((res) => setTimeout(res, 50));
      return `${src}|${ui.querySelector('.toast').textContent}`;
    }"#,
            None,
        )
        .unwrap();
    browser.close();
    let out = out.as_str().unwrap_or_default().to_string();
    assert!(
        regex::Regex::new(r"Summary\.tsx:42:7\|Sent to Claude · comment #001")
            .unwrap()
            .is_match(&out),
        "{out}"
    );
    let comments = all_comments(&root).unwrap();
    let c = &comments[0];
    assert_eq!(
        c["target"]["source"], "src/components/Summary.tsx:42:7",
        "nearest ancestor with the attribute"
    );
    assert_eq!(c["target"]["tag"], "span");
    assert_eq!(c["target"]["text"], "$12.00");
    assert!(c["target"]["selector"]
        .as_str()
        .unwrap()
        .contains("#total > span"));
    assert_eq!(c["screen"], "/cart");
    assert_eq!(c["breakpoint"], "desktop");
}

/// A strict-CSP app on `localhost`: a login route that sets a cookie, and a page whose own nonce
/// script records the keys it receives. Returns the port and the cookies the app saw.
fn strict_app() -> (u16, Arc<std::sync::Mutex<Vec<String>>>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let log = Arc::clone(&seen);
    thread::spawn(move || {
        for mut s in listener.incoming().flatten() {
            let Ok(Some(head)) = crate::studio::http::read_head(&mut s) else {
                continue;
            };
            let cookie = head.header("cookie").unwrap_or_default().to_string();
            log.lock().unwrap().push(cookie);
            let (headers, body) = if head.target() == "/login" {
                ("Content-Type: text/plain\r\nSet-Cookie: sid=s3cret; Path=/; HttpOnly; SameSite=Lax".to_string(), "ok".to_string())
            } else {
                (
                    "Content-Type: text/html; charset=utf-8\r\nContent-Security-Policy: default-src 'none'; script-src 'nonce-app' 'strict-dynamic'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'".to_string(),
                    "<!doctype html><html><head><title>App</title></head><body><h1 id=\"t\">Title</h1><input id=\"q\"><script nonce=\"app\">window.appKeys = []; document.addEventListener('keydown', (e) => window.appKeys.push(e.key));</script></body></html>".to_string(),
                )
            };
            let _ = s.write_all(format!("HTTP/1.1 200 OK\r\n{headers}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes());
        }
    });
    (port, seen)
}

/// inspector: dfw review in a browser: strict CSP app, localhost cookies, Alt/Option+C not C (needs Chrome)
#[test]
#[ignore = "needs Chrome"]
fn review_in_a_browser_strict_csp_localhost_cookies_and_the_shortcut() {
    let root = test_dir("live-browser-csp");
    std::fs::create_dir_all(root.join(".dfw")).unwrap();
    std::fs::write(
        root.join(".dfw/state.json"),
        "{\"schema\":1,\"mode\":\"existing-project\",\"phase\":\"verify\",\"active\":true}\n",
    )
    .unwrap();
    let (app, seen) = strict_app();
    let app_url = format!("http://localhost:{app}");
    let (port, _server) = proxy_for(&root, &app_url);
    let (browser, _context, page) = browser_page(&root, 1280, 800);
    // The user signs in on the app itself, then opens the review proxy on localhost.
    page.goto(&format!("{app_url}/login"), 30_000).unwrap();
    page.goto(&format!("http://localhost:{port}/"), 30_000)
        .unwrap();
    assert_eq!(
        seen.lock().unwrap().last().map(String::as_str),
        Some("sid=s3cret"),
        "the app's localhost cookie reaches it through the proxy"
    );
    let out = page
        .evaluate(
            r#"async () => {
      for (let i = 0; i < 50 && !window.__dfwLive; i++) await new Promise((res) => setTimeout(res, 50));
      if (!window.__dfwLive) return 'live.js blocked by CSP';
      if (typeof window.__dfwTargetInfo !== 'function') return 'overlay.js blocked by CSP';
      if (!Array.isArray(window.appKeys)) return "the app's own nonce script did not run";
      const ui = document.querySelector('dfw-live').shadowRoot;
      const pressed = () => ui.querySelector('.toggle').getAttribute('aria-pressed');
      const key = (init, el = document.body) => el.dispatchEvent(new KeyboardEvent('keydown', { bubbles: true, cancelable: true, composed: true, ...init }));
      if (getComputedStyle(ui.querySelector('.bar')).position !== 'fixed') return 'review UI styles blocked by style-src';
      key({ key: 'c', code: 'KeyC' });
      if (pressed() !== 'false') return 'plain C was stolen from the app';
      if (!window.appKeys.includes('c')) return 'the app did not get C';
      const q = document.querySelector('#q');
      q.focus();
      key({ key: 'ç', code: 'KeyC', altKey: true }, q);
      if (pressed() !== 'false') return 'shortcut fired while typing in an input';
      q.blur();
      key({ key: 'ç', code: 'KeyC', altKey: true });
      if (pressed() !== 'true') return 'Option+C did not toggle comment mode';
      document.querySelector('#t').dispatchEvent(new MouseEvent('click', { bubbles: true, cancelable: true }));
      ui.querySelector('textarea').value = 'Bigger title';
      ui.querySelector('.pop').requestSubmit();
      for (let i = 0; i < 50 && !/Sent to Claude|Not sent/.test(ui.querySelector('.toast').textContent); i++) await new Promise((res) => setTimeout(res, 50));
      return ui.querySelector('.toast').textContent;
    }"#,
            None,
        )
        .unwrap();
    browser.close();
    let out = out.as_str().unwrap_or_default().to_string();
    assert!(
        out.contains("Sent to Claude · comment #001"),
        "comment POST allowed by connect-src: {out}"
    );
    assert_eq!(all_comments(&root).unwrap()[0]["comment"], "Bigger title");
}

/// A tunnel that carries nothing for its idle deadline is closed both ways, so a stuck peer
/// cannot hold the connection (and its slot) for ever; traffic keeps it open (review P13).
#[test]
fn an_idle_websocket_tunnel_is_closed() {
    use std::time::{Duration, Instant};
    // Two socket pairs: (client, proxy side) and (proxy side, app).
    let pair = || {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let outer = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (inner, _) = l.accept().unwrap();
        (outer, inner)
    };
    let (mut client, from_client) = pair();
    let (mut app, to_app) = pair();
    let idle = Duration::from_millis(300);
    let tunnel = thread::spawn(move || super::tunnel::splice_with(from_client, to_app, idle));
    // Traffic within the deadline passes and keeps the tunnel open.
    for _ in 0..3 {
        thread::sleep(Duration::from_millis(150));
        client.write_all(b"ping").unwrap();
        let mut got = [0u8; 4];
        app.read_exact(&mut got).unwrap();
        assert_eq!(&got, b"ping");
    }
    let quiet = Instant::now();
    app.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut rest = Vec::new();
    // Both peers see the end once the tunnel idled.
    assert_eq!(app.read_to_end(&mut rest).unwrap(), 0);
    client
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    assert_eq!(client.read_to_end(&mut rest).unwrap(), 0);
    tunnel.join().unwrap();
    let waited = quiet.elapsed();
    assert!(
        waited >= Duration::from_millis(250) && waited < Duration::from_secs(3),
        "{waited:?}"
    );
}
