//! Ports `core/lib/lighthouse.mjs`: the optional Lighthouse run for the verify phase (G4):
//! performance, best practices and SEO of the running app. Opt-in per run ("dfw check --url <u>
//! --lighthouse"): it runs a pinned Lighthouse through npx (a one-time ~20 MB download into the
//! npm cache) against the Chrome dfw already uses. Accessibility is left to axe (dfw check).
//! Error reporting stays off.

// `dfw check` (tools/check) is the only caller and is not ported yet.
#![allow(dead_code)]

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::browser::cdp::lock;
use crate::browser::launch::find_browser;
use crate::core::js;
use crate::core::process::node_executable;

const LIGHTHOUSE_VERSION: &str = "13.6.0";

fn unverifiable(detail: &str) -> Value {
    json!({ "tool": "lighthouse", "status": "unverifiable", "detail": detail, "findings": [] })
}

/// `runLighthouse(url, { timeoutMs })`: a dfw check result (`{ tool, status, detail, findings }`).
/// Node's default timeout is 240 s.
pub fn run_lighthouse(url: &str, timeout_ms: u64) -> Value {
    let (node, browser) = (node_executable(), find_browser());
    let (Some(node), Some(browser)) = (node, browser) else {
        return unverifiable(if node.is_none() {
            "needs Node.js (npx)"
        } else {
            "no Chrome/Edge/Chromium"
        });
    };
    // Kept from Node: inside the binary `node` is a bare command name, so "next to it" is the
    // working directory and npx is normally not found there.
    let npx = if cfg!(windows) { "npx.cmd" } else { "npx" };
    if !Path::new(npx).exists() {
        return unverifiable(&format!("npx not found next to {node}"));
    }
    let args = [
        "-y".to_string(),
        format!("lighthouse@{LIGHTHOUSE_VERSION}"),
        url.to_string(),
        "--output=json".into(),
        "--output-path=stdout".into(),
        "--quiet".into(),
        "--no-enable-error-reporting".into(),
        "--only-categories=performance,best-practices,seo".into(),
        "--chrome-flags=--headless=new --no-first-run --no-default-browser-check --disable-features=HttpsUpgrades,HttpsFirstBalancedModeAutoEnable,HttpsFirstModeV2ForTypicallySecureUsers".into(),
    ];
    let mut command = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/d", "/s", "/c", npx]);
        c
    } else {
        Command::new(npx)
    };
    command
        .args(args)
        .env("CHROME_PATH", &browser.path)
        .env("NO_UPDATE_NOTIFIER", "1");
    let (stdout, stderr, error) = run_with_timeout(command, timeout_ms);
    let detail = if stderr.is_empty() { error } else { stderr };
    lighthouse_report(stdout.as_deref(), url, &detail)
}

/// `spawnSync(..., { timeout })`: stdout (none when the start failed), stderr, and the error of
/// a failed start or a timeout.
fn run_with_timeout(mut command: Command, timeout_ms: u64) -> (Option<String>, String, String) {
    let spawned = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return (None, String::new(), format!("spawnSync npx {e}")),
    };
    // A grandchild (the browser Lighthouse started) can keep a pipe open after npx is gone:
    // the readers fill shared buffers, and are given a moment, not joined for ever.
    let drain = |mut pipe: Option<Box<dyn Read + Send>>| {
        let text = Arc::new(Mutex::new(Vec::new()));
        let shared = Arc::clone(&text);
        let reader = thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            while let Some(n) = pipe
                .as_mut()
                .and_then(|p| p.read(&mut chunk).ok())
                .filter(|n| *n > 0)
            {
                lock(&shared).extend_from_slice(&chunk[..n]);
            }
        });
        (text, reader)
    };
    let mut child_out = child
        .stdout
        .take()
        .map(|p| Box::new(p) as Box<dyn Read + Send>);
    let mut child_err = child
        .stderr
        .take()
        .map(|p| Box::new(p) as Box<dyn Read + Send>);
    let (out, out_reader) = drain(child_out.take());
    let (err, err_reader) = drain(child_err.take());
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let mut error = String::new();
    loop {
        match child.try_wait() {
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(50)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                error = "spawnSync npx ETIMEDOUT".to_string();
                break;
            }
            _ => break,
        }
    }
    let settle = Instant::now() + Duration::from_secs(2);
    while !(out_reader.is_finished() && err_reader.is_finished()) && Instant::now() < settle {
        thread::sleep(Duration::from_millis(10));
    }
    let text = |buf: &Mutex<Vec<u8>>| String::from_utf8_lossy(&lock(buf)).into_owned();
    (Some(text(&out)), text(&err), error)
}

/// `Object.values(value || {})`.
fn values(value: Option<&Value>) -> Vec<&Value> {
    match value {
        Some(Value::Object(map)) => map.values().collect(),
        Some(Value::Array(items)) => items.iter().collect(),
        _ => Vec::new(),
    }
}

/// `lighthouseReport(stdout, url, errorDetail)`: Lighthouse's JSON output -> a dfw check result
/// (status "error" when there is no usable report).
pub fn lighthouse_report(stdout: Option<&str>, url: &str, error_detail: &str) -> Value {
    // No stdout at all (the start failed) and "null" are no report; a number is not one either.
    let lhr = stdout
        .and_then(|text| serde_json::from_str::<Value>(text).ok())
        .filter(|v| v.is_object() || v.is_array());
    let Some(lhr) = lhr else {
        let detail = if error_detail.is_empty() {
            "no output"
        } else {
            error_detail
        };
        let detail: String = detail.chars().take(300).collect();
        return json!({ "tool": "lighthouse", "status": "error", "detail": detail, "findings": [] });
    };
    let categories = values(lhr.get("categories"));
    // A category Lighthouse could not score has score null: that is not a 0.
    let scores: Vec<String> = categories
        .iter()
        .map(|c| {
            let score = c.get("score").and_then(Value::as_f64).map_or_else(
                || "n/a".to_string(),
                // Math.round: halves go up.
                |s| js::number((s * 100.0 + 0.5).floor()),
            );
            format!("{} {score}", js::to_string(c.get("title")))
        })
        .collect();
    let mut findings = Vec::new();
    for cat in &categories {
        for audit_ref in values(cat.get("auditRefs")) {
            let id = audit_ref
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let Some(a) = lhr
                .get("audits")
                .and_then(|audits| audits.get(id))
                .filter(|a| js::truthy(a))
            else {
                continue;
            };
            let mode = a
                .get("scoreDisplayMode")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let weighted = audit_ref.get("weight").is_some_and(js::truthy);
            let score = match a.get("score").and_then(Value::as_f64) {
                Some(score)
                    if score < 0.9
                        && weighted
                        && ["binary", "numeric", "metricSavings"].contains(&mode) =>
                {
                    score
                }
                _ => continue,
            };
            let display = a.get("displayValue").filter(|v| js::truthy(v));
            findings.push(json!({
                "level": if score < 0.5 { "warn" } else { "info" },
                "rule": format!("lighthouse-{}", js::to_string(a.get("id"))),
                "file": url,
                "line": 1,
                "source": "lighthouse",
                "message": format!(
                    "{}: {}{}",
                    js::to_string(cat.get("title")),
                    js::to_string(a.get("title")),
                    display.map(|d| format!(" ({})", js::to_string(Some(d)))).unwrap_or_default()
                ),
            }));
        }
    }
    json!({ "tool": "lighthouse", "status": "ran", "detail": format!("scores: {}", scores.join(", ")), "findings": findings })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// lighthouse: no stdout (spawn failed) or "null" is an error result, not a crash
    #[test]
    fn no_usable_report_is_an_error_result() {
        for stdout in [None, Some(""), Some("null"), Some("42"), Some("not json")] {
            let r = lighthouse_report(stdout, "http://localhost:3000", "spawnSync npx ENOENT");
            assert_eq!(
                (&r["status"], &r["detail"], &r["findings"]),
                (&json!("error"), &json!("spawnSync npx ENOENT"), &json!([])),
                "{stdout:?}"
            );
        }
        assert_eq!(
            lighthouse_report(None, "http://localhost:3000", "")["detail"],
            json!("no output")
        );
        let long = "x".repeat(400);
        assert_eq!(
            lighthouse_report(None, "u", &long)["detail"]
                .as_str()
                .unwrap()
                .len(),
            300
        );
    }

    /// lighthouse: a category without a score reads "n/a", not 0; low audits become findings
    #[test]
    fn scores_and_findings() {
        let lhr = json!({
            "categories": {
                "performance": { "title": "Performance", "score": null, "auditRefs": [{ "id": "lcp", "weight": 25 }] },
                "seo": { "title": "SEO", "score": 0.92, "auditRefs": [{ "id": "meta", "weight": 1 }, { "id": "gone", "weight": 1 }] },
            },
            "audits": {
                "lcp": { "id": "lcp", "title": "Largest Contentful Paint", "score": 0.3, "scoreDisplayMode": "numeric", "displayValue": "6.1 s" },
                "meta": { "id": "meta", "title": "Has a meta description", "score": 1, "scoreDisplayMode": "binary" },
            },
        });
        let r = lighthouse_report(Some(&lhr.to_string()), "http://localhost:3000", "");
        assert_eq!(r["status"], json!("ran"));
        assert_eq!(r["detail"], json!("scores: Performance n/a, SEO 92"));
        assert_eq!(
            r["findings"],
            json!([{ "level": "warn", "rule": "lighthouse-lcp", "file": "http://localhost:3000", "line": 1, "source": "lighthouse", "message": "Performance: Largest Contentful Paint (6.1 s)" }])
        );
        let no_audits = r#"{"categories":{"seo":{"title":"SEO","score":1,"auditRefs":[{"id":"x","weight":1}]}}}"#;
        assert_eq!(
            lighthouse_report(Some(no_audits), "u", "")["status"],
            json!("ran"),
            "no audits map"
        );
    }

    #[test]
    fn audits_that_do_not_count() {
        let lhr = json!({
            "categories": { "p": { "title": "P", "score": 0.555, "auditRefs": [
                { "id": "unweighted", "weight": 0 }, { "id": "informative", "weight": 1 }, { "id": "fine", "weight": 1 },
                { "id": "unscored", "weight": 1 }, { "id": "mid", "weight": 1 },
            ] } },
            "audits": {
                "unweighted": { "id": "unweighted", "title": "U", "score": 0, "scoreDisplayMode": "binary" },
                "informative": { "id": "informative", "title": "I", "score": 0, "scoreDisplayMode": "informative" },
                "fine": { "id": "fine", "title": "F", "score": 0.9, "scoreDisplayMode": "numeric" },
                "unscored": { "id": "unscored", "title": "N", "score": null, "scoreDisplayMode": "numeric" },
                "mid": { "id": "mid", "title": "M", "score": 0.5, "scoreDisplayMode": "metricSavings" },
            },
        });
        let r = lighthouse_report(Some(&lhr.to_string()), "u", "");
        assert_eq!(r["detail"], json!("scores: P 56"));
        assert_eq!(r["findings"].as_array().unwrap().len(), 1);
        assert_eq!(r["findings"][0]["level"], json!("info"));
        assert_eq!(r["findings"][0]["message"], json!("P: M"));
    }

    #[test]
    fn a_run_that_cannot_start_or_takes_too_long_reports_why() {
        let (stdout, _, error) = run_with_timeout(Command::new("/no/such/npx"), 1000);
        assert!(
            stdout.is_none() && error.starts_with("spawnSync npx "),
            "{error}"
        );
        if cfg!(unix) {
            // The orphaned `sleep` keeps stdout open for 4 s: the result must not wait for it.
            let mut slow = Command::new("sh");
            slow.args(["-c", "echo partial; sleep 4"]);
            let started = Instant::now();
            let (stdout, _, error) = run_with_timeout(slow, 200);
            assert_eq!(
                (stdout.as_deref(), error.as_str()),
                (Some("partial\n"), "spawnSync npx ETIMEDOUT")
            );
            assert!(started.elapsed() < Duration::from_millis(3500));
        }
    }
}
