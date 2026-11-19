//! The report of the checks: `flatten`, `formatReport` and `checkExitCode` of check.mjs.

use crate::core::js;
use std::cmp::Ordering;

use serde_json::{json, Value};

/// `flatten(results)`: every finding of every tool.
pub fn flatten(results: &[Value]) -> Vec<Value> {
    results
        .iter()
        .filter_map(|r| r.get("findings").and_then(Value::as_array))
        .flatten()
        .cloned()
        .collect()
}

/// Sort keys of `String.prototype.localeCompare` for the paths findings carry.
///
/// ASCII follows the CLDR root collation Node uses: control characters are ignored, white space
/// and punctuation sort before digits, digits before letters, and case only breaks a tie (lower
/// first). Other characters sort after the letters by code point, which is not what CLDR does
/// for accented letters: an approximation, since no crate here carries the collation tables.
fn collation_keys(s: &str) -> (Vec<u32>, Vec<bool>) {
    const ORDER: &str =
        "\t\n\u{b}\u{c}\r _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789abcdefghijklmnopqrstuvwxyz";
    let mut primary = Vec::new();
    let mut upper = Vec::new();
    for c in s.chars() {
        if let Some(i) = ORDER.find(c.to_ascii_lowercase()).filter(|_| c.is_ascii()) {
            primary.push(i as u32);
            upper.push(c.is_ascii_uppercase());
        } else if !c.is_ascii() {
            primary.push(c as u32 + 0x100);
            upper.push(false);
        }
    }
    (primary, upper)
}

/// `a.localeCompare(b)` (see [`collation_keys`]).
pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let (pa, ua) = collation_keys(a);
    let (pb, ub) = collation_keys(b);
    pa.cmp(&pb).then_with(|| ua.cmp(&ub))
}

const LEVELS: [(&str, &str); 3] = [("error", "MUST"), ("warn", "SHOULD"), ("info", "NICE")];

fn level_rank(finding: &Value) -> usize {
    let level = finding.get("level").and_then(Value::as_str);
    LEVELS
        .iter()
        .position(|(l, _)| Some(*l) == level)
        .unwrap_or(LEVELS.len())
}

/// The result of [`format_report`].
#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub text: String,
    /// Findings by level.
    pub errors: usize,
    pub warnings: usize,
    pub infos: usize,
    /// All findings, MUST first, then by file and line.
    pub findings: Vec<Value>,
    /// Tools that could not run.
    pub unverified: Vec<String>,
}

impl Report {
    /// `{ error, warn, info }`.
    pub fn tally(&self) -> Value {
        json!({ "error": self.errors, "warn": self.warnings, "info": self.infos })
    }
}

/// `formatReport(results, { max })`. Node's default for `max` is 50.
pub fn format_report(results: &[Value], max: usize) -> Report {
    let mut all = flatten(results);
    let text_of = |f: &Value, key: &str| js::to_string(f.get(key));
    // A line that is not a number sorts last (Node's comparison gives no order for it).
    let line_key = |f: &Value| {
        let n = js::to_number(f.get("line"));
        (n.is_nan(), if n.is_nan() { 0.0 } else { n })
    };
    all.sort_by(|a, b| {
        level_rank(a)
            .cmp(&level_rank(b))
            .then_with(|| locale_compare(&text_of(a, "file"), &text_of(b, "file")))
            .then_with(|| {
                let (ka, kb) = (line_key(a), line_key(b));
                ka.0.cmp(&kb.0)
                    .then(ka.1.partial_cmp(&kb.1).unwrap_or(Ordering::Equal))
            })
    });
    let count = |rank: usize| all.iter().filter(|f| level_rank(f) == rank).count();
    let (errors, warnings, infos) = (count(0), count(1), count(2));
    let status = |r: &Value| text_of(r, "status");
    let tools: Vec<String> = results
        .iter()
        .map(|r| format!("{}={}", text_of(r, "tool"), status(r)))
        .collect();
    let mut lines = vec![format!(
        "Checks: {errors} must, {warnings} should, {infos} nice. Tools: {}",
        tools.join(", ")
    )];
    for f in all.iter().take(max) {
        let label = LEVELS.get(level_rank(f)).map_or("undefined", |(_, l)| *l);
        lines.push(format!(
            "- [{label}] {}:{} ({}/{}) {}",
            text_of(f, "file"),
            text_of(f, "line"),
            text_of(f, "source"),
            text_of(f, "rule"),
            text_of(f, "message")
        ));
    }
    if all.len() > max {
        lines.push(format!(
            "- ... {} more (run \"dfw check --json\" for all).",
            all.len() - max
        ));
    }
    // Fail closed: a check that could not run is reported as NOT VERIFIED, never read as a pass.
    let did_not_run = |r: &&Value| {
        [
            "unverifiable",
            "unavailable",
            "not-installed",
            "timeout",
            "error",
        ]
        .contains(&status(r).as_str())
    };
    let detail = |r: &Value| {
        r.get("detail")
            .filter(|d| js::truthy(d))
            .map(|d| js::to_string(Some(d)))
    };
    let unverified: Vec<&Value> = results.iter().filter(did_not_run).collect();
    if !unverified.is_empty() {
        let list: Vec<String> = unverified
            .iter()
            .map(|r| {
                format!(
                    "{} ({})",
                    text_of(r, "tool"),
                    detail(r).unwrap_or_else(|| status(r))
                )
            })
            .collect();
        lines.push(format!(
            "NOT VERIFIED: {}. Say so in any report; do not claim these aspects pass.",
            list.join("; ")
        ));
    }
    for r in results.iter().filter(|r| !did_not_run(r)) {
        if let Some(detail) = detail(r) {
            lines.push(format!("  note: {}: {detail}", text_of(r, "tool")));
        }
    }
    Report {
        text: lines.join("\n"),
        errors,
        warnings,
        infos,
        unverified: unverified.iter().map(|r| text_of(r, "tool")).collect(),
        findings: all,
    }
}

/// `checkExitCode(rep, { strict })`: exit code of "dfw check": 2 for MUST findings; with
/// `--strict`, 3 when a configured tool did not run.
pub fn check_exit_code(report: &Report, strict: bool) -> i32 {
    if report.errors > 0 {
        2
    } else if strict && !report.unverified.is_empty() {
        3
    } else {
        0
    }
}
