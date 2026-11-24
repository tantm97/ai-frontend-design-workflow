//! Ports `core/lib/lighthouse.mjs`: the optional Lighthouse run for the verify phase (G4):
//! performance, best practices and SEO of the running app. Opt-in per run ("dfw check --url <u>
//! --lighthouse"): it runs a pinned Lighthouse through npx (a one-time ~20 MB download into the
//! npm cache) against the Chrome dfw already uses. Accessibility is left to axe (dfw check).
//! Error reporting stays off.

use std::path::Path;
use std::time::Duration;

use serde_json::{json, Value};

use crate::browser::launch::find_browser;
use crate::core::js;
use crate::core::level::Level;
use crate::core::process::{node_executable, run_timed, Spawn};
use crate::tools::check::outcome;

const LIGHTHOUSE_VERSION: &str = "13.6.0";

/// The tool and the source its results and findings name.
const TOOL: &str = "lighthouse";

fn unverifiable(detail: &str) -> Value {
    json!({ "tool": TOOL, "status": outcome::UNVERIFIABLE, "detail": detail, "findings": [] })
}

/// `runLighthouse(url, { timeoutMs })`: a dfw check result (`{ tool, status, detail, findings }`).
/// Node's default timeout is 240 s.
pub fn run_lighthouse(url: &str, timeout_ms: u64) -> Value {
    let Some(node) = node_executable() else {
        return unverifiable("needs Node.js (npx)");
    };
    let Some(browser) = find_browser() else {
        return unverifiable("no Chrome/Edge/Chromium");
    };
    lighthouse_with(&node, &browser.path, url, timeout_ms)
}

/// How npx of the Node.js at `node` (an absolute path) runs: Node itself with npx's script, so
/// neither cmd.exe nor a lookup by name is involved. That is `<dir>/npx` elsewhere (a link to
/// `npx-cli.js`) and `<dir>\node_modules\npm\bin\npx-cli.js` on Windows, which is what
/// `npx.cmd` runs. `None` when that script is not there (or `node` is not a `.exe` on Windows).
fn npx_plan(
    node: &str,
    args: Vec<String>,
    windows: bool,
    exists: &dyn Fn(&str) -> bool,
) -> Option<Spawn> {
    let separators: &[char] = if windows { &['/', '\\'] } else { &['/'] };
    let dir = &node[..node.rfind(separators)?];
    let script = if windows {
        if !node.to_ascii_lowercase().ends_with(".exe") {
            return None;
        }
        format!("{dir}\\node_modules\\npm\\bin\\npx-cli.js")
    } else {
        format!("{dir}/npx")
    };
    exists(&script).then(|| Spawn::direct(node, std::iter::once(script).chain(args).collect()))
}

/// [`run_lighthouse`] with the Node.js and the browser given.
fn lighthouse_with(node: &Path, browser: &Path, url: &str, timeout_ms: u64) -> Value {
    let args = vec![
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
    let node_text = node.display().to_string();
    let Some(plan) = npx_plan(&node_text, args, cfg!(windows), &|p| Path::new(p).is_file()) else {
        return unverifiable(&format!("npx not found next to {node_text}"));
    };
    let env = [
        ("CHROME_PATH".to_string(), browser.display().to_string()),
        ("NO_UPDATE_NOTIFIER".to_string(), "1".to_string()),
    ];
    let (stdout, stderr, error) = run_with_timeout(&plan, &env, timeout_ms);
    let detail = if stderr.is_empty() { error } else { stderr };
    lighthouse_report(stdout.as_deref(), url, &detail)
}

/// `spawnSync(..., { timeout })`: stdout (none when the start failed), stderr, and the error of
/// a failed start or a timeout. A timeout ends npx and everything it started.
fn run_with_timeout(
    plan: &Spawn,
    env: &[(String, String)],
    timeout_ms: u64,
) -> (Option<String>, String, String) {
    let ran = run_timed(plan, None, env, Duration::from_millis(timeout_ms));
    if let Some(error) = ran.error {
        return (None, String::new(), error);
    }
    let error = if ran.timed_out {
        "spawnSync npx ETIMEDOUT".to_string()
    } else {
        String::new()
    };
    (Some(ran.stdout), ran.stderr, error)
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
        return json!({ "tool": TOOL, "status": outcome::ERROR, "detail": detail, "findings": [] });
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
                "level": if score < 0.5 { Level::Warn } else { Level::Info }.as_str(),
                "rule": format!("lighthouse-{}", js::to_string(a.get("id"))),
                "file": url,
                "line": 1,
                "source": TOOL,
                "message": format!(
                    "{}: {}{}",
                    js::to_string(cat.get("title")),
                    js::to_string(a.get("title")),
                    display.map(|d| format!(" ({})", js::to_string(Some(d)))).unwrap_or_default()
                ),
            }));
        }
    }
    json!({ "tool": TOOL, "status": outcome::RAN, "detail": format!("scores: {}", scores.join(", ")), "findings": findings })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::check::field::{DETAIL, FINDINGS, STATUS};

    /// The app Lighthouse is run against.
    const URL: &str = "http://localhost:3000";

    /// lighthouse: no stdout (spawn failed) or "null" is an error result, not a crash
    #[test]
    fn no_usable_report_is_an_error_result() {
        /// Why npx did not start.
        const NOT_STARTED: &str = "spawnSync npx ENOENT";
        for stdout in [None, Some(""), Some("null"), Some("42"), Some("not json")] {
            let r = lighthouse_report(stdout, URL, NOT_STARTED);
            assert_eq!(
                (&r[STATUS], &r[DETAIL], &r[FINDINGS]),
                (&json!(outcome::ERROR), &json!(NOT_STARTED), &json!([])),
                "{stdout:?}"
            );
        }
        assert_eq!(lighthouse_report(None, URL, "")[DETAIL], json!("no output"));
        let long = "x".repeat(400);
        assert_eq!(
            lighthouse_report(None, "u", &long)[DETAIL]
                .as_str()
                .unwrap()
                .len(),
            300
        );
    }

    /// lighthouse: a category without a score reads "n/a", not 0; low audits become findings
    #[test]
    fn scores_and_findings() {
        /// A low audit, and one that passes.
        const LCP: &str = "lcp";
        const META: &str = "meta";
        let lhr = json!({
            "categories": {
                "performance": { "title": "Performance", "score": null, "auditRefs": [{ "id": LCP, "weight": 25 }] },
                "seo": { "title": "SEO", "score": 0.92, "auditRefs": [{ "id": META, "weight": 1 }, { "id": "gone", "weight": 1 }] },
            },
            "audits": {
                "lcp": { "id": LCP, "title": "Largest Contentful Paint", "score": 0.3, "scoreDisplayMode": "numeric", "displayValue": "6.1 s" },
                "meta": { "id": META, "title": "Has a meta description", "score": 1, "scoreDisplayMode": "binary" },
            },
        });
        let r = lighthouse_report(Some(&lhr.to_string()), URL, "");
        assert_eq!(r[STATUS], json!(outcome::RAN));
        assert_eq!(r[DETAIL], json!("scores: Performance n/a, SEO 92"));
        assert_eq!(
            r[FINDINGS],
            json!([{ "level": "warn", "rule": "lighthouse-lcp", "file": URL, "line": 1, "source": TOOL, "message": "Performance: Largest Contentful Paint (6.1 s)" }])
        );
        let no_audits = r#"{"categories":{"seo":{"title":"SEO","score":1,"auditRefs":[{"id":"x","weight":1}]}}}"#;
        assert_eq!(
            lighthouse_report(Some(no_audits), "u", "")[STATUS],
            json!(outcome::RAN),
            "no audits map"
        );
    }

    #[test]
    fn audits_that_do_not_count() {
        /// A display mode that has no score to judge.
        const INFORMATIVE: &str = "informative";
        /// A display mode with a score.
        const NUMERIC: &str = "numeric";
        // (id, weight, title, score, display mode): only the last one counts; the informative
        // one is named after its mode.
        let audits = [
            ("unweighted", 0, "U", json!(0), "binary"),
            (INFORMATIVE, 1, "I", json!(0), INFORMATIVE),
            ("fine", 1, "F", json!(0.9), NUMERIC),
            ("unscored", 1, "N", Value::Null, NUMERIC),
            ("mid", 1, "M", json!(0.5), "metricSavings"),
        ];
        let refs: Vec<Value> = audits
            .iter()
            .map(|(id, weight, ..)| json!({ "id": id, "weight": weight }))
            .collect();
        let by_id: serde_json::Map<String, Value> = audits
            .iter()
            .map(|(id, _, title, score, mode)| {
                let audit =
                    json!({ "id": id, "title": title, "score": score, "scoreDisplayMode": mode });
                (id.to_string(), audit)
            })
            .collect();
        let lhr = json!({
            "categories": { "p": { "title": "P", "score": 0.555, "auditRefs": refs } },
            "audits": by_id,
        });
        let r = lighthouse_report(Some(&lhr.to_string()), "u", "");
        assert_eq!(r[DETAIL], json!("scores: P 56"));
        assert_eq!(r[FINDINGS].as_array().unwrap().len(), 1);
        assert_eq!(r[FINDINGS][0]["level"], json!("info"));
        assert_eq!(r[FINDINGS][0]["message"], json!("P: M"));
    }

    /// lighthouse: npx is the one next to the Node.js found on PATH (it was looked up in the
    /// current folder, so --lighthouse never ran), started through Node, never through cmd.exe
    #[test]
    fn npx_comes_from_the_node_on_path() {
        /// A Node.js and its npx, elsewhere and on Windows.
        const NODE: &str = "/opt/node/bin/node";
        const NPX: &str = "/opt/node/bin/npx";
        const WIN_NODE: &str = "C:\\Program Files\\nodejs\\node.exe";
        const WIN_NPX: &str = "C:\\Program Files\\nodejs\\node_modules\\npm\\bin\\npx-cli.js";
        let files = [NPX, WIN_NPX];
        let exists = |p: &str| files.contains(&p);
        let url = vec!["http://localhost:3000/?a=1&calc".to_string()];
        assert_eq!(
            npx_plan(NODE, url.clone(), false, &exists),
            Some(Spawn::direct(NODE, vec![NPX.into(), url[0].clone()]))
        );
        assert_eq!(npx_plan("/usr/bin/node", url.clone(), false, &exists), None);
        let win = npx_plan(WIN_NODE, url.clone(), true, &exists).unwrap();
        assert_eq!(
            win,
            Spawn::direct(WIN_NODE, vec![WIN_NPX.into(), url[0].clone()]),
            "the URL is an argument of node.exe, not text for cmd.exe"
        );
        assert_eq!(
            npx_plan("C:\\Program Files\\nodejs\\node.cmd", url, true, &exists),
            None
        );
    }

    /// The whole run with a stand-in Node.js: it gets npx's script, the URL and CHROME_PATH.
    #[test]
    #[cfg(unix)]
    fn lighthouse_runs_with_the_npx_next_to_node() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::core::util::test_dir("lighthouse-npx");
        let node = dir.join("node");
        let report = r#"{"categories":{"seo":{"title":"SEO","score":1,"auditRefs":[]}}}"#;
        std::fs::write(
            &node,
            format!(
                "#!/bin/sh\ncase \"$1\" in */npx) ;; *) echo \"not npx: $1\" >&2; exit 1 ;; esac\n[ \"$CHROME_PATH\" = /x/chrome ] || exit 2\necho '{report}'\n"
            ),
        )
        .unwrap();
        let npx = dir.join("npx");
        let chrome = Path::new("/x/chrome");
        std::fs::write(&npx, "// npx-cli.js\n").unwrap();
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        let r = lighthouse_with(&node, chrome, URL, 5000);
        assert_eq!(
            (&r[STATUS], &r[DETAIL]),
            (&json!(outcome::RAN), &json!("scores: SEO 100")),
            "{r}"
        );
        std::fs::remove_file(&npx).unwrap();
        let r = lighthouse_with(&node, chrome, URL, 5000);
        assert_eq!(r[STATUS], json!(outcome::UNVERIFIABLE));
        assert_eq!(
            r[DETAIL],
            json!(format!("npx not found next to {}", node.display()))
        );
    }

    #[test]
    fn a_run_that_cannot_start_or_takes_too_long_reports_why() {
        let missing = Spawn::direct("/no/such/npx", Vec::new());
        let (stdout, _, error) = run_with_timeout(&missing, &[], 1000);
        assert!(
            stdout.is_none() && error == "spawnSync /no/such/npx ENOENT",
            "{error}"
        );
        if cfg!(unix) {
            // The orphaned `sleep` keeps stdout open for 4 s: the result must not wait for it.
            let slow = Spawn::direct("sh", vec!["-c".into(), "echo partial; sleep 4".into()]);
            let started = std::time::Instant::now();
            let (stdout, _, error) = run_with_timeout(&slow, &[], 200);
            assert_eq!(
                (stdout.as_deref(), error.as_str()),
                (Some("partial\n"), "spawnSync npx ETIMEDOUT")
            );
            assert!(started.elapsed() < Duration::from_millis(3500));
        }
    }
}
