//! The decision log (`.dfw/decisions.jsonl`): entries written by `approveGate` in `core/lib/state.mjs`
//! and by `dfw decide`, and the listing of `dfw decisions`, in `core/lib/cli.mjs`.

use std::path::Path;

use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, rel};
use crate::core::util::{append_jsonl_inside, now_iso, read_jsonl};

/// The keys of an entry of `.dfw/decisions.jsonl`:
/// `{ at, level, topic, decision, by, overrides? }`, and what a G2 freeze adds.
pub mod field {
    pub const AT: &str = "at";
    /// Who decided: 1 the user .. 5 an external skill's recommendation.
    pub const LEVEL: &str = "level";
    pub const TOPIC: &str = "topic";
    /// The decision's text.
    pub const DECISION: &str = "decision";
    /// `user` or `orchestrator`.
    pub const BY: &str = "by";
    /// The earlier decision this one overrides.
    pub const OVERRIDES: &str = "overrides";
    /// The MUST findings the user accepted at G2.
    pub const FINDINGS: &str = "findings";
    /// The checks that did not run when the user accepted freezing at G2.
    pub const UNVERIFIED: &str = "unverified";
}

/// The entry `approveGate` appends: a level 1 (user) decision named after the gate.
pub fn gate_entry(at: &str, gate: &str, title: &str, note: Option<&Value>) -> Value {
    let note = note.map_or(String::new(), |n| format!(" - {}", js::to_string(Some(n))));
    json!({
        (field::AT): at,
        (field::LEVEL): 1,
        (field::TOPIC): format!("gate:{gate}"),
        (field::DECISION): format!("{title}{note}"),
        (field::BY): "user",
    })
}

/// `dfw decide`: validate and append one decision; returns the entry written.
/// `level` is `Number(--level)`, a whole number; 1 is the user, 5 an external skill's
/// recommendation.
pub fn decide(
    root: &Path,
    level: f64,
    topic: Option<Value>,
    text: Option<Value>,
    overrides: Option<Value>,
) -> Result<Value, DfwError> {
    // A whole level only: 1.5 is no level of the table.
    if !(1.0..=5.0).contains(&level) || level.fract() != 0.0 {
        return Err(DfwError::new(
            "--level must be 1 (user) .. 5 (external skill recommendation).",
        ));
    }
    let Some(text) = text else {
        return Err(DfwError::new("Decision text is required."));
    };
    let entry = js::object(vec![
        (field::AT, Some(Value::String(now_iso()))),
        (field::LEVEL, Some(Value::from(level))),
        (
            field::TOPIC,
            Some(topic.unwrap_or_else(|| "general".into())),
        ),
        (field::DECISION, Some(text)),
        (
            field::BY,
            Some(if level == 1.0 { "user" } else { "orchestrator" }.into()),
        ),
        (field::OVERRIDES, overrides),
    ]);
    append_jsonl_inside(root, &project_paths(root).decisions, &entry)?;
    Ok(entry)
}

/// Every recorded decision, oldest first.
pub fn read_decisions(root: &Path) -> Result<Vec<Value>, DfwError> {
    read_jsonl(&project_paths(root).decisions)
}

/// The text of `dfw decisions`: one line per decision.
pub fn format_decisions(list: &[Value]) -> Result<String, DfwError> {
    if list.is_empty() {
        return Ok("No decisions recorded.".into());
    }
    let lines: Result<Vec<String>, DfwError> = list
        .iter()
        .map(|d| {
            let at = d.get(field::AT).and_then(Value::as_str).ok_or_else(|| {
                DfwError::new(format!(
                    "{}: a decision has no \"at\" time. Fix or remove that line.",
                    rel::DECISIONS
                ))
            })?;
            // slice(0, 16) counts UTF-16 units; a timestamp is ASCII.
            let units: Vec<u16> = at.encode_utf16().take(16).collect();
            Ok(format!(
                "{} L{} [{}] {}",
                String::from_utf16_lossy(&units),
                js::to_string(d.get(field::LEVEL)),
                js::to_string(d.get(field::TOPIC)),
                js::to_string(d.get(field::DECISION))
            ))
        })
        .collect();
    Ok(lines?.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::{test_dir, to_json_compact};

    #[test]
    fn decide_validates_and_appends() {
        let root = test_dir("decisions");
        assert!(decide(&root, f64::NAN, None, Some("x".into()), None).is_err());
        assert!(decide(&root, 6.0, None, Some("x".into()), None).is_err());
        // CO-13: only whole levels.
        for odd in [1.5, 0.5, 4.999, f64::INFINITY] {
            assert!(
                decide(&root, odd, None, Some("x".into()), None).is_err(),
                "{odd}"
            );
        }
        assert_eq!(
            decide(&root, 2.0, None, None, None).unwrap_err().message(),
            "Decision text is required."
        );
        assert!(read_decisions(&root).unwrap().is_empty());
        assert_eq!(format_decisions(&[]).unwrap(), "No decisions recorded.");
        let user = decide(&root, 1.0, None, Some("Blue accent".into()), None).unwrap();
        assert_eq!(user["by"], "user");
        assert_eq!(user["topic"], "general");
        let other = decide(
            &root,
            3.0,
            Some("type".into()),
            Some("Inter".into()),
            Some("d1".into()),
        )
        .unwrap();
        let text = to_json_compact(&other);
        assert!(
            text.contains("\"level\":3,\"topic\":\"type\",\"decision\":\"Inter\",\"by\":\"orchestrator\",\"overrides\":\"d1\""),
            "{text}"
        );
        let list = read_decisions(&root).unwrap();
        assert_eq!(list.len(), 2);
        let at = &list[0]["at"].as_str().unwrap()[..16];
        assert_eq!(
            format_decisions(&list).unwrap(),
            format!("{at} L1 [general] Blue accent\n{at} L3 [type] Inter")
        );
    }

    #[test]
    fn gate_entry_names_the_gate_and_the_note() {
        let e = gate_entry("t", "G1", "Design direction approved", Some(&"A".into()));
        assert_eq!(
            to_json_compact(&e),
            r#"{"at":"t","level":1,"topic":"gate:G1","decision":"Design direction approved - A","by":"user"}"#
        );
        assert_eq!(gate_entry("t", "G1", "T", None)["decision"], "T");
    }
}
