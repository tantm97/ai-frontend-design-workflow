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
