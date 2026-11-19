//! Ports `core/lib/live.mjs`: `dfw review`, click-to-comment on the running app. A local reverse
//! proxy (loopback only) in front of the app's dev server: it injects the Studio overlay helpers
//! (overlay.js) and the live review UI (live.js) into HTML responses, passes everything else
//! through (WebSocket upgrades too, so Vite/webpack/Next HMR keeps working) and writes comments to
//! the same feedback contract as the Studio (`.dfw/reviews/`). Source file:line comes from
//! code-inspector-plugin's `data-insp-path` when the project has it (`dfw inspector setup`).
//!
//! A strict Content-Security-Policy gets the proxy origin and a per-response nonce for the
//! injected scripts only; for a localhost app the proxy is served as `http://localhost:<port>/`
//! so the app's localhost cookies (host-scoped, not port-scoped) reach it.
//!
//! This file holds the pure parts; `live_proxy/proxy.rs` the server.

mod proxy;

use std::path::Path;
use std::sync::LazyLock;

use regex::{Captures, Regex};
use serde_json::Value;

pub use proxy::{start_live_server, LiveOptions};

use crate::core::config::{load_config, review_shortcut};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::with_js_classes;
use crate::core::paths::project_paths;
use crate::core::util::{get_path, read_json};
use crate::design::html::escape_attr;

// Compiled once: every proxied page and response goes through them. A pattern that did not
// compile would be a typo the tests catch; callers then leave the text as it is.
static META_CSP: LazyLock<Option<Regex>> = LazyLock::new(|| {
    with_js_classes(
        r#"(?i)<meta(?-u:\b)[^>]*http-equiv\s*=\s*["']?content-security-policy["']?[^>]*>"#,
    )
    .ok()
});
static META_CONTENT: LazyLock<Option<Regex>> =
    LazyLock::new(|| with_js_classes(r#"(?i)(?-u:\b)content\s*=\s*(?:"([^"]*)"|'([^']*)')"#).ok());
static COOKIE_DOMAIN: LazyLock<Option<Regex>> =
    LazyLock::new(|| with_js_classes(r"(?i);\s*domain\s*=\s*\.?([^;]*)").ok());
static BODY_OPEN: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?i)<body[^>]*>").ok());

const LOCAL_HOSTS: [&str; 5] = ["localhost", "127.0.0.1", "::1", "[::1]", "0.0.0.0"];

/// Comment-mode key when nothing else is set.
pub const DEFAULT_SHORTCUT: &str = "alt+c";

/// `normalizeShortcut(raw)`: a review shortcut ("Alt+C", "option+c", "ctrl+shift+k", "c",
/// "off") in `ctrl+alt+shift+meta+<key>` order. One letter or digit plus optional modifiers;
/// "off" disables it.
pub fn normalize_shortcut(raw: &str) -> Result<String, DfwError> {
    let s = js::trim(raw).to_lowercase();
    if s.is_empty() || s == "off" || s == "none" || s == "false" {
        return Ok("off".into());
    }
    let modifier = |part: &str| -> Option<&'static str> {
        Some(match part {
            "alt" | "option" | "opt" => "alt",
            "ctrl" | "control" => "ctrl",
            "meta" | "cmd" | "command" => "meta",
            "shift" => "shift",
            _ => return None,
        })
    };
    let mut mods: Vec<&str> = Vec::new();
    let mut key: Option<String> = None;
    let mut bad = false;
    for part in s.split('+').map(js::trim) {
        if let Some(m) = modifier(part) {
            mods.push(m);
        } else if part.len() == 1
            && part
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            && key.is_none()
        {
            key = Some(part.to_string());
        } else {
            bad = true;
        }
    }
    match key.filter(|_| !bad) {
        Some(key) => {
            let mut out: Vec<String> = ["ctrl", "alt", "shift", "meta"]
                .iter()
                .filter(|m| mods.contains(m))
                .map(|m| m.to_string())
                .collect();
            out.push(key);
            Ok(out.join("+"))
        }
        None => Err(DfwError::new(format!("Invalid review shortcut \"{raw}\": use one letter or digit with optional modifiers (alt, ctrl, shift, meta), e.g. alt+c, ctrl+shift+k, or off."))),
    }
}

/// `liveTags({ nonce, shortcut })`: the script tags injected into the app's HTML.
pub fn live_tags(nonce: Option<&str>, shortcut: &str) -> String {
    let n = nonce.map_or(String::new(), |n| format!(" nonce=\"{}\"", escape_attr(n)));
    format!(
        "<script defer src=\"/__dfw/overlay.js\" data-dfw-live{n}></script><script defer src=\"/__dfw/live.js\" data-shortcut=\"{}\"{n}></script>",
        escape_attr(shortcut)
    )
}

/// A parsed app URL (the parts of `new URL()` this module uses).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `http:` or `https:`.
    pub protocol: String,
    /// Lower case; IPv6 in brackets.
    pub hostname: String,
    /// `None` for the scheme's default port.
    pub port: Option<u16>,
    /// Path with query, `/` at least.
    pub path: String,
}

impl Target {
    /// `url.host`.
    pub fn host(&self) -> String {
        match self.port {
            Some(p) => format!("{}:{p}", self.hostname),
            None => self.hostname.clone(),
        }
    }

    /// `url.origin`.
    pub fn origin(&self) -> String {
        format!("{}//{}", self.protocol, self.host())
    }

    /// The port to connect to.
    pub fn connect_port(&self) -> u16 {
        self.port
            .unwrap_or(if self.protocol == "https:" { 443 } else { 80 })
    }

    /// `pathname` (without the query).
    pub fn pathname(&self) -> &str {
        self.path.split(['?', '#']).next().unwrap_or("/")
    }
}

/// `new URL(raw)` for the cases `dfw review` meets: `Err(None)` when it is not a URL,
/// `Err(Some(protocol))` for a scheme other than http(s).
fn parse_url(raw: &str) -> Result<Target, Option<String>> {
    let raw = raw.trim_matches(|c: char| c <= ' ');
    let (scheme, rest) = raw.split_once(':').ok_or(None)?;
    let valid = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !valid {
        return Err(None);
    }
    let protocol = format!("{}:", scheme.to_ascii_lowercase());
    if protocol != "http:" && protocol != "https:" {
        return Err(Some(protocol));
    }
    let rest = rest.trim_start_matches(['/', '\\']);
    let end = rest.find(['/', '\\', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let mut path = rest[end..].replace('\\', "/");
    if !path.starts_with('/') {
        path.insert(0, '/');
    }
    let hostport = authority.rsplit('@').next().unwrap_or_default();
    let (hostname, port) = if hostport.starts_with('[') {
        let close = hostport.find(']').ok_or(None)?;
        let after = &hostport[close + 1..];
        let port = match after {
            "" => None,
            p => Some(p.strip_prefix(':').ok_or(None)?),
        };
        (hostport[..=close].to_ascii_lowercase(), port)
    } else {
        match hostport.rsplit_once(':') {
            Some((h, p)) => (h.to_ascii_lowercase(), Some(p)),
            None => (hostport.to_ascii_lowercase(), None),
        }
    };
    if hostname.is_empty() {
        return Err(None);
    }
    let default = if protocol == "https:" { 443 } else { 80 };
    let port = match port.filter(|p| !p.is_empty()) {
        None => None,
        Some(p) => Some(p.parse::<u16>().map_err(|_| None)?).filter(|p| *p != default),
    };
    Ok(Target {
        protocol,
        hostname,
        port,
        path,
    })
}

/// `assertLocalTarget(raw)`: the app URL must be a local http(s) server; dfw review never
/// proxies to the network.
pub fn assert_local_target(raw: &str) -> Result<Target, DfwError> {
    let t = parse_url(raw).map_err(|e| match e {
        None => DfwError::new(format!("Not a URL: {raw}")),
        Some(p) => DfwError::new(format!("dfw review needs an http(s) URL, got {p}")),
    })?;
    let h = &t.hostname;
    if !LOCAL_HOSTS.contains(&h.as_str()) && !h.ends_with(".localhost") && !h.starts_with("127.") {
        return Err(DfwError::new(format!(
            "dfw review only proxies a local dev server (localhost / 127.0.0.1), not {}.",
            t.hostname
        )));
    }
    Ok(t)
}

/// `proxyHost(target)`: the host the review URL is served with: localhost for a localhost app
/// (cookies are host-scoped, not port-scoped).
pub fn proxy_host(t: &Target) -> &'static str {
    if t.hostname == "localhost" {
        "localhost"
    } else {
        "127.0.0.1"
    }
}

/// `patchCsp(value, { origin, nonce })`: let the injected scripts through a CSP value (header
/// or meta; possibly several comma-separated policies) without weakening anything else: the
/// proxy origin, and a nonce where host sources are not enough ('strict-dynamic', nonce/hash
/// lists), are added to the directive that governs `<script src>` and to the one that governs
/// fetch() (copied from default-src when absent). Nothing is removed, except a lone 'none'.
pub fn patch_csp(value: &str, origin: &str, nonce: Option<&str>) -> String {
    if value.is_empty() {
        return String::new();
    }
    let origin_low = origin.to_lowercase();
    value
        .split(',')
        .map(|policy| {
            let mut dirs: Vec<(String, Vec<String>)> = policy
                .split(';')
                .map(js::trim)
                .filter(|d| !d.is_empty())
                .map(|d| {
                    let mut words = d.split(crate::core::js::is_space).filter(|w| !w.is_empty());
                    let name = words.next().unwrap_or_default().to_lowercase();
                    (name, words.map(str::to_string).collect())
                })
                .collect();
            if dirs.is_empty() {
                return policy.to_string();
            }
            let find = |dirs: &[(String, Vec<String>)], n: &str| dirs.iter().position(|d| d.0 == n);
            let ensure = |dirs: &mut Vec<(String, Vec<String>)>, n: &str| -> Option<usize> {
                if let Some(i) = find(dirs, n) {
                    return Some(i);
                }
                // No directive and no default-src: not restricted.
                let def = find(dirs, "default-src")?;
                let list = dirs[def].1.clone();
                dirs.push((n.to_string(), list));
                Some(dirs.len() - 1)
            };
            let grow = |list: &mut Vec<String>, with_nonce: bool| {
                let low: Vec<String> = list.iter().map(|x| x.to_lowercase()).collect();
                if low.len() == 1 && low[0] == "'none'" {
                    list.clear();
                }
                let nonce_or_hash = low.iter().any(|x| {
                    ["'nonce-", "'sha256-", "'sha384-", "'sha512-"]
                        .iter()
                        .any(|p| x.starts_with(p))
                });
                if !low.iter().any(|x| x == "'self'" || *x == origin_low) {
                    list.push(origin.to_string());
                }
                // A nonce makes browsers ignore 'unsafe-inline': only add one where that
                // changes nothing for the app.
                if let Some(n) = nonce.filter(|n| !n.is_empty()) {
                    if with_nonce && (nonce_or_hash || !low.iter().any(|x| x == "'unsafe-inline'"))
                    {
                        list.push(format!("'nonce-{n}'"));
                    }
                }
            };
            let scripts: Vec<usize> = ["script-src-elem", "script-src"]
                .iter()
                .filter_map(|n| find(&dirs, n))
                .collect();
            let scripts = if scripts.is_empty() {
                ensure(&mut dirs, "script-src").into_iter().collect()
            } else {
                scripts
            };
            for i in scripts {
                grow(&mut dirs[i].1, true);
            }
            if let Some(i) = ensure(&mut dirs, "connect-src") {
                grow(&mut dirs[i].1, false);
            }
            dirs.iter()
                .map(|(name, list)| {
                    std::iter::once(name.clone())
                        .chain(list.iter().cloned())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// `patchMetaCsp(html, opts)`: the same for `<meta http-equiv="Content-Security-Policy">`.
pub fn patch_meta_csp(html: &str, origin: &str, nonce: Option<&str>) -> String {
    let (Some(meta), Some(content)) = (META_CSP.as_ref(), META_CONTENT.as_ref()) else {
        return html.to_string();
    };
    meta.replace_all(html, |tag: &Captures| {
        let tag = &tag[0];
        content
            .replacen(tag, 1, |c: &Captures| {
                let v = c.get(1).or_else(|| c.get(2)).map_or("", |m| m.as_str());
                let v = v
                    .replace("&#39;", "'")
                    .replace("&apos;", "'")
                    .replace("&quot;", "\"")
                    .replace("&amp;", "&");
                format!("content=\"{}\"", escape_attr(&patch_csp(&v, origin, nonce)))
            })
            .into_owned()
    })
    .into_owned()
}

/// `fixSetCookie(cookies, host)`: drop a Set-Cookie Domain that names a different local host
/// than the one the browser uses (127.0.0.1 vs localhost).
pub fn fix_set_cookie(cookies: &[String], host: &str) -> Vec<String> {
    let h = host.to_lowercase();
    let Some(domain) = COOKIE_DOMAIN.as_ref() else {
        return cookies.to_vec();
    };
    cookies
        .iter()
        .map(|c| {
            domain
                .replace_all(c, |m: &Captures| {
                    let dom = js::trim(&m[1]).to_lowercase();
                    if LOCAL_HOSTS.contains(&dom.as_str()) && dom != h {
                        String::new()
                    } else {
                        m[0].to_string()
                    }
                })
                .into_owned()
        })
        .collect()
}

/// `injectLive(html, opts)`: add the overlay and live review scripts (head first: React 19
/// tolerates extra head/body tags on hydration). Once only.
pub fn inject_live(html: &str, nonce: Option<&str>, shortcut: &str) -> String {
    if html.contains("data-dfw-live") {
        return html.to_string();
    }
    let tags = live_tags(nonce, shortcut);
    if let Some(i) = html.to_ascii_lowercase().find("</head>") {
        return format!("{}{tags}{}", &html[..i], &html[i..]);
    }
    if let Some(m) = BODY_OPEN.as_ref().and_then(|r| r.find(html)) {
        return format!("{}{tags}{}", &html[..m.end()], &html[m.end()..]);
    }
    tags + html
}

/// `nearestBreakpoint(root, width)`: the id of the breakpoint closest to a viewport width
/// (design.json's, else the config's); `None` without breakpoints.
pub fn nearest_breakpoint(root: &Path, width: f64) -> Result<Option<Value>, DfwError> {
    let manifest = read_json(&project_paths(root).manifest, Some(serde_json::json!({})))?;
    let mut bps = manifest
        .get("breakpoints")
        .filter(|b| crate::core::js::truthy(b))
        .cloned();
    if bps.is_none() {
        let config = load_config(Some(root))?;
        bps = get_path(&config, "studio.breakpoints").cloned();
    }
    let list = bps
        .as_ref()
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if list.is_empty() || width == 0.0 || width.is_nan() {
        return Ok(None);
    }
    let dist = |b: &Value| (crate::core::js::to_number(b.get("width")) - width).abs();
    let mut best = &list[0];
    for b in &list[1..] {
        // NaN compares false, as in the reduce of live.mjs.
        if dist(b) < dist(best) {
            best = b;
        }
    }
    Ok(best.get("id").cloned())
}

/// The shortcut a proxy uses: `--shortcut`, else `review.shortcut` in the config, else Alt+C.
pub fn configured_shortcut(root: &Path, flag: Option<&str>) -> Result<String, DfwError> {
    match flag {
        Some(f) => normalize_shortcut(f),
        None => {
            let config = load_config(Some(root))?;
            normalize_shortcut(
                review_shortcut(&config)
                    .as_deref()
                    .unwrap_or(DEFAULT_SHORTCUT),
            )
        }
    }
}

#[cfg(test)]
mod tests;
