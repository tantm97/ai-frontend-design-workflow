//! Ports `core/lib/state.mjs`: workflow state machine, phases, modes and approval gates (`.dfw/state.json`).
//!
//! The state is a `serde_json::Value` (the user owns the file; key order is kept). Where the Node
//! code would throw a `TypeError` on a state that lacks a field, the functions here return an
//! internal error, so a hook fails open and a command exits with 1, as in Node.

use std::path::Path;

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::core::util::{append_jsonl, now_iso, read_json, write_json, ArgValue, Args};

use super::decisions::gate_entry;

/// Phases in workflow order.
pub const PHASES: [&str; 9] = [
    "discovery",
    "direction",
    "system",
    "concepts",
    "stack",
    "implement",
    "verify",
    "audit",
    "done",
];

/// Approval gates and what each one records.
pub const GATES: [(&str, &str); 5] = [
    ("G1", "Design direction approved"),
    ("DS", "Design system (foundations + components) approved"),
    ("G2", "Design frozen (approved version)"),
    ("G3", "Technology stack and major dependencies approved"),
    ("G4", "Implementation verified; deviations accepted"),
];

/// A workflow mode: where it starts, which gates it waives and whether it reaches code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mode {
    pub name: &'static str,
    pub entry: &'static str,
    pub waive: &'static [&'static str],
    pub writes_code: bool,
    pub requires_approved_design: bool,
}

const fn mode(
    name: &'static str,
    entry: &'static str,
    waive: &'static [&'static str],
    writes_code: bool,
) -> Mode {
    Mode {
        name,
        entry,
        waive,
        writes_code,
        requires_approved_design: false,
    }
}

/// `MODES`, in the order of the Node table.
pub const MODES: [Mode; 7] = [
    mode("new-product", "discovery", &[], true),
    mode("existing-project", "discovery", &[], true),
    mode("design-only", "discovery", &[], false),
    Mode {
        name: "implement-approved",
        entry: "stack",
        waive: &[],
        writes_code: true,
        requires_approved_design: true,
    },
    mode("ui-refinement", "concepts", &["G1", "DS"], true),
    mode("quick-change", "concepts", &["G1", "DS"], true),
    mode("review-only", "audit", &[], false),
];

/// Short names of the modes.
pub const MODE_ALIASES: [(&str, &str); 8] = [
    ("new", "new-product"),
    ("existing", "existing-project"),
    ("design", "design-only"),
    ("implement", "implement-approved"),
    ("refine", "ui-refinement"),
    ("refinement", "ui-refinement"),
    ("quick", "quick-change"),
    ("review", "review-only"),
];

/// `normalizeMode(mode)`: an alias becomes the full name.
pub fn normalize_mode(mode: &str) -> &str {
    MODE_ALIASES
        .iter()
        .find(|(alias, _)| *alias == mode)
        .map_or(mode, |(_, full)| full)
}

/// `MODES[name]`.
pub fn mode_info(name: &str) -> Option<&'static Mode> {
    MODES.iter().find(|m| m.name == name)
}

/// `GATES[gate]`: what the gate records.
pub fn gate_title(gate: &str) -> Option<&'static str> {
    GATES.iter().find(|(g, _)| *g == gate).map(|(_, t)| *t)
}

// ---------------------------------------------------------------------------------------------
// JavaScript value rules the state, comment and command code depend on
// ---------------------------------------------------------------------------------------------

/// A flag as the JavaScript value `parseArgs` gives it: `true` alone, else its text.
pub fn arg_raw(args: &Args, key: &str) -> Option<Value> {
    match args.get(key) {
        Some(ArgValue::Flag) => Some(Value::Bool(true)),
        Some(ArgValue::Text(t)) => Some(Value::String(t.clone())),
        None => None,
    }
}

/// A flag when it is truthy (`a.key || undefined`): given alone, or with a non-empty text.
pub fn arg_truthy(args: &Args, key: &str) -> Option<Value> {
    arg_raw(args, key).filter(|v| js::truthy(v))
}

/// The `TypeError` JavaScript throws when a property is read from `undefined`.
pub fn type_error(property: &str) -> DfwError {
    DfwError::internal(format!(
        "Cannot read properties of undefined (reading '{property}')"
    ))
}

/// `value.key` of something that must be there: an error where JavaScript would throw.
fn required<'a>(value: &'a Value, key: &str, child: &str) -> Result<&'a Value, DfwError> {
    match value.get(key) {
        Some(v) if !v.is_null() => Ok(v),
        _ => Err(type_error(child)),
    }
}

fn object_mut<'a>(
    state: &'a mut Value,
    key: &str,
    child: &str,
) -> Result<&'a mut Map<String, Value>, DfwError> {
    match state.get_mut(key) {
        Some(Value::Object(map)) => Ok(map),
        _ => Err(type_error(child)),
    }
}

/// `state.history.push(entry)`.
pub fn push_history(state: &mut Value, entry: Value) -> Result<(), DfwError> {
    match state.get_mut("history") {
        Some(Value::Array(history)) => {
            history.push(entry);
            Ok(())
        }
        _ => Err(type_error("push")),
    }
}

/// `state[key] = value`, or the key removed for `undefined`.
pub fn set_field(state: &mut Value, key: &str, value: Option<Value>) -> Result<(), DfwError> {
    let Value::Object(map) = state else {
        return Err(DfwError::internal(
            ".dfw/state.json does not hold an object.",
        ));
    };
    match value {
        Some(value) => {
            map.insert(key.to_string(), value);
        }
        None => {
            map.shift_remove(key);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------------------------

/// `loadState(root)`: the state, or `None` when the project has no workflow.
pub fn load_state(root: &Path) -> Result<Option<Value>, DfwError> {
    let state = read_json(&project_paths(root).state, Some(Value::Null))?;
    Ok(Some(state).filter(|v| js::truthy(v)))
}

/// `requireState(root)`: the state, or the error that says to run `dfw init`.
pub fn require_state(root: &Path) -> Result<Value, DfwError> {
    load_state(root)?.ok_or_else(|| {
        DfwError::new(format!(
            "No dfw workflow in {}. Run \"dfw init\" first.",
            root.display()
        ))
    })
}

/// `saveState(root, state)`: stamps `updatedAt` and writes `.dfw/state.json`.
pub fn save_state(root: &Path, state: &mut Value) -> Result<(), DfwError> {
    set_field(state, "updatedAt", Some(Value::String(now_iso())))?;
    write_json(&project_paths(root).state, state)
}

fn unknown_mode(mode: &str) -> DfwError {
    let names: Vec<&str> = MODES.iter().map(|m| m.name).collect();
    DfwError::new(format!(
        "Unknown mode \"{mode}\". Modes: {}",
        names.join(", ")
    ))
}

fn waived_by(mode: &Mode) -> Value {
    let map: Map<String, Value> = mode
        .waive
        .iter()
        .map(|g| {
            (
                g.to_string(),
                Value::String(format!("waived by mode {}", mode.name)),
            )
        })
        .collect();
    Value::Object(map)
}

/// `newState({ mode, product })`: the state `dfw init` writes.
pub fn new_state(mode: &str, product: Value) -> Result<Value, DfwError> {
    let mode = normalize_mode(mode);
    let info = mode_info(mode).ok_or_else(|| unknown_mode(mode))?;
    let at = now_iso();
    Ok(json!({
        "schema": 1,
        "product": product,
        "mode": mode,
        "phase": info.entry,
        "active": true,
        "createdAt": at,
        "updatedAt": at,
        "design": { "version": 1, "status": "draft", "locked": false, "approved": [] },
        "gates": { "G1": null, "DS": null, "G2": null, "G3": null, "G4": null },
        "waived": waived_by(info),
        "history": [{ "at": at, "event": "init", "mode": mode }],
    }))
}

/// `gatePassed(state, gate)`: approved or waived.
pub fn gate_passed(state: &Value, gate: &str) -> bool {
    let has = |key: &str| js::truthy_value(state.get(key).and_then(|g| g.get(gate))).is_some();
    has("gates") || has("waived")
}

/// `currentApproval(state)`: the current approved design (`None` while the latest version is
/// still a draft).
pub fn current_approval(state: &Value) -> Result<Option<Value>, DfwError> {
    let design = required(state, "design", "status")?;
    if design.get("status").and_then(Value::as_str) != Some("approved") {
        return Ok(None);
    }
    let approved = design
        .get("approved")
        .and_then(Value::as_array)
        .ok_or_else(|| type_error("at"))?;
    Ok(js::truthy_value(approved.last()).cloned())
}

fn state_mode(state: &Value) -> Result<&'static Mode, DfwError> {
    state
        .get("mode")
        .and_then(Value::as_str)
        .and_then(mode_info)
        .ok_or_else(|| type_error("writesCode"))
}

/// `transitionBlockers(state, to)`: reasons the transition is not allowed (empty = allowed).
/// `to` is `None` when the command line named no phase.
pub fn transition_blockers(state: &Value, to: Option<&str>) -> Result<Vec<String>, DfwError> {
    let Some(to) = to.filter(|t| PHASES.contains(t)) else {
        return Ok(vec![format!(
            "Unknown phase \"{}\". Phases: {}",
            to.unwrap_or("undefined"),
            PHASES.join(", ")
        )]);
    };
    let mut reasons = Vec::new();
    let mode = state_mode(state)?;
    let phase = state.get("phase").and_then(Value::as_str);
    if ["stack", "implement", "verify"].contains(&to)
        && !mode.writes_code
        && mode.name != "review-only"
    {
        reasons.push(format!(
            "Mode \"{}\" does not include implementation; switch mode first (dfw mode <mode>).",
            mode.name
        ));
    }
    const NO_G1: &str = "G1 (design direction) is not approved yet.";
    match to {
        "system" => {
            if !gate_passed(state, "G1") {
                reasons.push(NO_G1.into());
            }
        }
        "concepts" => {
            if !gate_passed(state, "G1") {
                reasons.push(NO_G1.into());
            }
            if !gate_passed(state, "DS") {
                reasons.push("DS (design system) is not approved yet: build foundations and components first (phase \"system\").".into());
            }
        }
        "implement" => {
            if current_approval(state)?.is_none() {
                reasons.push(
                    "G2: there is no approved, frozen design for the current version.".into(),
                );
            }
            if !gate_passed(state, "G3") {
                reasons.push("G3: the technology stack is not approved yet.".into());
            }
        }
        "verify" => {
            let after_implement = matches!(phase, Some("implement" | "verify"));
            if !after_implement && mode.name != "implement-approved" {
                reasons.push(format!(
                    "Verification follows implementation (current phase is {}).",
                    js::to_string(state.get("phase"))
                ));
            }
        }
        "done" => {
            if mode.writes_code && !gate_passed(state, "G4") {
                reasons.push("G4 (verification accepted) is not approved yet.".into());
            }
            if !mode.writes_code && mode.name == "design-only" && current_approval(state)?.is_none()
            {
                reasons.push("Design-only work finishes with an approved design (G2).".into());
            }
        }
        _ => {}
    }
    Ok(reasons)
}

/// `setPhase(root, state, to, { force, reason })`. With `force` the phase is set whatever the
/// blockers say (they are recorded); an unnamed phase then removes the field, as in Node.
pub fn set_phase(
    root: &Path,
    state: &mut Value,
    to: Option<&str>,
    force: bool,
    reason: Option<Value>,
) -> Result<(), DfwError> {
    let blockers = transition_blockers(state, to)?;
    // --force overrides gate blockers, never an unknown or missing phase.
    let known = to.is_some_and(|t| PHASES.contains(&t));
    if !blockers.is_empty() && (!force || !known) {
        return Err(DfwError::new(format!(
            "Cannot move to \"{}\":\n- {}",
            to.unwrap_or("undefined"),
            blockers.join("\n- ")
        )));
    }
    let from = state.get("phase").cloned();
    let to = to.map(|t| Value::String(t.to_string()));
    set_field(state, "phase", to.clone())?;
    let forced = (!blockers.is_empty()).then(|| Value::from(blockers));
    push_history(
        state,
        js::object(vec![
            ("at", Some(Value::String(now_iso()))),
            ("event", Some("phase".into())),
            ("from", from),
            ("to", to),
            ("forced", forced),
            ("reason", reason.filter(|v| js::truthy(v))),
        ]),
    )?;
    save_state(root, state)
}

/// `approveGate(root, state, gate, { note, extra })`: records the approval in the state, the
/// history and the decision log. The caller has the user's word for it (rule 1 of AGENTS.md).
pub fn approve_gate(
    root: &Path,
    state: &mut Value,
    gate: &str,
    note: Option<Value>,
    extra: Map<String, Value>,
) -> Result<(), DfwError> {
    let Some(title) = gate_title(gate) else {
        let names: Vec<&str> = GATES.iter().map(|(g, _)| *g).collect();
        return Err(DfwError::new(format!(
            "Unknown gate \"{gate}\". Gates: {}",
            names.join(", ")
        )));
    };
    let note = note.filter(|v| js::truthy(v));
    let p = project_paths(root);
    let phase = state.get("phase").and_then(Value::as_str);
    if gate == "G1" && phase == Some("discovery") && note.is_none() {
        return Err(DfwError::new("G1 is normally approved during the direction phase. Pass --note to record what was approved."));
    }
    if gate == "G3" && !p.stack.exists() {
        return Err(DfwError::new(
            "G3 needs .dfw/stack.json (run: dfw stack set ... or dfw stack detect --write).",
        ));
    }
    if gate == "G4" && phase != Some("verify") {
        return Err(DfwError::new(
            "G4 is approved at the end of the verify phase.",
        ));
    }
    let at = now_iso();
    let mut record = Map::new();
    record.insert("approvedAt".into(), Value::String(at.clone()));
    let mut event = Map::new();
    event.insert("at".into(), Value::String(at.clone()));
    event.insert("event".into(), "approve".into());
    event.insert("gate".into(), gate.into());
    for target in [&mut record, &mut event] {
        if let Some(note) = &note {
            target.insert("note".into(), note.clone());
        }
        target.extend(extra.clone());
    }
    object_mut(state, "gates", gate)?.insert(gate.to_string(), Value::Object(record));
    push_history(state, Value::Object(event))?;
    append_jsonl(&p.decisions, &gate_entry(&at, gate, title, note.as_ref()))?;
    save_state(root, state)
}

/// `setMode(root, state, mode)`; `mode` is `None` when the command line named none.
pub fn set_mode(root: &Path, state: &mut Value, mode: Option<&str>) -> Result<(), DfwError> {
    let mode = normalize_mode(mode.unwrap_or("undefined"));
    let info = mode_info(mode).ok_or_else(|| unknown_mode(mode))?;
    if info.requires_approved_design && current_approval(state)?.is_none() {
        return Err(DfwError::new(format!(
            "Mode \"{mode}\" needs an approved design (G2). None exists for the current version."
        )));
    }
    let from = state.get("mode").cloned();
    set_field(state, "mode", Some(mode.into()))?;
    set_field(state, "waived", Some(waived_by(info)))?;
    push_history(
        state,
        js::object(vec![
            ("at", Some(Value::String(now_iso()))),
            ("event", Some("mode".into())),
            ("from", from),
            ("to", Some(mode.into())),
        ]),
    )?;
    save_state(root, state)
}

/// `summarize(state)`: the three status lines every command and the SessionStart hook print.
pub fn summarize(state: Option<&Value>) -> Result<String, DfwError> {
    let Some(state) = state else {
        return Ok("dfw: no workflow in this project (run /dfw to start).".into());
    };
    let gates = required(state, "gates", "G1")?;
    let g = |k: &str| {
        if js::truthy_value(gates.get(k)).is_some() {
            "approved"
        } else if js::truthy_value(state.get("waived").and_then(|w| w.get(k))).is_some() {
            "waived"
        } else {
            "open"
        }
    };
    let design = required(state, "design", "version")?;
    let appr = current_approval(state)?;
    let product = js::truthy_value(state.get("product"))
        .map_or("(unnamed)".to_string(), |p| js::to_string(Some(p)));
    Ok([
        format!(
            "dfw: product=\"{product}\" mode={} phase={}",
            js::to_string(state.get("mode")),
            js::to_string(state.get("phase"))
        ),
        format!(
            "design: v{} {}{}{}",
            js::to_string(design.get("version")),
            js::to_string(design.get("status")),
            if js::truthy_value(design.get("locked")).is_some() {
                " (locked)"
            } else {
                ""
            },
            appr.as_ref().map_or(String::new(), |a| format!(
                ", approved {}",
                js::to_string(a.get("version"))
            )),
        ),
        format!(
            "gates: G1={} DS={} G2={} G3={} G4={}",
            g("G1"),
            g("DS"),
            if appr.is_some() { "approved" } else { "open" },
            g("G3"),
            g("G4")
        ),
    ]
    .join("\n"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn fresh(mode: &str) -> Value {
        new_state(mode, Value::String(String::new())).unwrap()
    }

    /// state machine: system needs G1, concepts needs G1+DS, implement needs G2+G3
    /// (tests/unit/dfw.test.mjs)
    #[test]
    fn state_machine_system_needs_g1_concepts_needs_g1_ds_implement_needs_g2_g3() {
        let mut s = fresh("new-product");
        assert_eq!(
            transition_blockers(&s, Some("system")).unwrap(),
            ["G1 (design direction) is not approved yet."]
        );
        assert_eq!(transition_blockers(&s, Some("concepts")).unwrap().len(), 2);
        assert_eq!(transition_blockers(&s, Some("implement")).unwrap().len(), 2);
        s["gates"]["G1"] = json!({ "approvedAt": "x" });
        assert!(transition_blockers(&s, Some("concepts")).unwrap()[0].starts_with("DS"));
        s["gates"]["DS"] = json!({ "approvedAt": "x" });
        assert!(transition_blockers(&s, Some("concepts"))
            .unwrap()
            .is_empty());
        assert_eq!(fresh("quick")["mode"], "quick-change", "mode aliases");
        let q = fresh("quick-change");
        assert!(
            transition_blockers(&q, Some("concepts"))
                .unwrap()
                .is_empty(),
            "quick-change waives G1 and DS"
        );
        let d = fresh("design-only");
        assert!(transition_blockers(&d, Some("implement"))
            .unwrap()
            .iter()
            .any(|r| r.contains("does not include implementation")));
    }

    #[test]
    fn new_state_has_the_node_shape_and_key_order() {
        let s = fresh("refine");
        let keys: Vec<&str> = s.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "schema",
                "product",
                "mode",
                "phase",
                "active",
                "createdAt",
                "updatedAt",
                "design",
                "gates",
                "waived",
                "history"
            ]
        );
        assert_eq!(s["phase"], "concepts");
        assert_eq!(
            s["waived"],
            json!({ "G1": "waived by mode ui-refinement", "DS": "waived by mode ui-refinement" })
        );
        let err = new_state("nope", Value::Null).unwrap_err();
        assert_eq!(err.message(), "Unknown mode \"nope\". Modes: new-product, existing-project, design-only, implement-approved, ui-refinement, quick-change, review-only");
    }

    #[test]
    fn phase_mode_and_gate_changes_are_recorded() {
        let root = test_dir("state");
        let mut s = fresh("new-product");
        let err = set_phase(&root, &mut s, Some("concepts"), false, None).unwrap_err();
        assert!(err
            .message()
            .starts_with("Cannot move to \"concepts\":\n- G1"));
        set_phase(&root, &mut s, Some("direction"), false, None).unwrap();
        approve_gate(&root, &mut s, "G1", Some("Direction A".into()), Map::new()).unwrap();
        assert_eq!(s["gates"]["G1"]["note"], "Direction A");
        assert!(approve_gate(&root, &mut s, "G9", None, Map::new())
            .unwrap_err()
            .message()
            .starts_with("Unknown gate \"G9\". Gates: G1, DS, G2, G3, G4"));
        assert!(approve_gate(&root, &mut s, "G3", None, Map::new())
            .unwrap_err()
            .message()
            .starts_with("G3 needs .dfw/stack.json"));
        assert!(approve_gate(&root, &mut s, "G4", None, Map::new())
            .unwrap_err()
            .message()
            .starts_with("G4 is approved at the end"));
        // A forced move records what it skipped.
        set_phase(
            &root,
            &mut s,
            Some("implement"),
            true,
            Some("user said so".into()),
        )
        .unwrap();
        let last = s["history"].as_array().unwrap().last().unwrap().clone();
        assert_eq!(last["forced"].as_array().unwrap().len(), 2);
        assert_eq!(last["reason"], "user said so");
        assert!(set_mode(&root, &mut s, Some("implement"))
            .unwrap_err()
            .message()
            .contains("needs an approved design"));
        set_mode(&root, &mut s, Some("quick")).unwrap();
        assert_eq!(s["waived"]["DS"], "waived by mode quick-change");
        let saved = load_state(&root).unwrap().unwrap();
        assert_eq!(saved, s);
        let log = std::fs::read_to_string(root.join(".dfw/decisions.jsonl")).unwrap();
        assert!(log.contains("\"topic\":\"gate:G1\",\"decision\":\"Design direction approved - Direction A\",\"by\":\"user\""));
        assert_eq!(
            summarize(Some(&s)).unwrap(),
            "dfw: product=\"(unnamed)\" mode=quick-change phase=implement\ndesign: v1 draft\ngates: G1=approved DS=waived G2=open G3=open G4=open"
        );
        assert_eq!(
            summarize(None).unwrap(),
            "dfw: no workflow in this project (run /dfw to start)."
        );
    }

    /// c: phase --force never writes a missing or unknown phase (tests/unit/node-stable.test.mjs)
    #[test]
    fn phase_force_never_writes_a_missing_or_unknown_phase() {
        let root = test_dir("state-force");
        let mut s = fresh("new-product");
        for to in [None, Some("bogus")] {
            let err = set_phase(&root, &mut s, to, true, None).unwrap_err();
            assert!(
                err.message()
                    .contains(&format!("Unknown phase \"{}\"", to.unwrap_or("undefined"))),
                "{err}"
            );
        }
        assert_eq!(s["phase"], "discovery");
        set_phase(&root, &mut s, Some("concepts"), true, Some("test".into())).unwrap();
        assert_eq!(
            s["phase"], "concepts",
            "--force still overrides gate blockers"
        );
    }

    /// j: names that are prototype keys are not ids (tests/unit/node-stable.test.mjs; modes and gates)
    #[test]
    fn prototype_names_are_not_modes_or_gates() {
        let root = test_dir("state-proto");
        let mut s = fresh("new-product");
        for name in ["constructor", "toString", "__proto__", "hasOwnProperty"] {
            let unknown = format!("Unknown mode \"{name}\".");
            assert!(set_mode(&root, &mut s, Some(name))
                .unwrap_err()
                .message()
                .starts_with(&unknown));
            assert!(new_state(name, Value::Null)
                .unwrap_err()
                .message()
                .starts_with(&unknown));
            let gate = approve_gate(&root, &mut s, name, Some("x".into()), Map::new()).unwrap_err();
            assert!(gate
                .message()
                .starts_with(&format!("Unknown gate \"{name}\".")));
        }
    }

    #[test]
    fn a_state_without_its_fields_is_an_error_not_a_panic() {
        assert!(summarize(Some(&json!({}))).is_err());
        assert!(current_approval(&json!({ "design": { "status": "approved" } })).is_err());
        assert!(transition_blockers(&json!({ "mode": "x" }), Some("system")).is_err());
        assert!(!gate_passed(&json!([]), "G1"));
    }

    #[test]
    fn javascript_value_rules() {
        assert_eq!(js::to_string(None::<&Value>), "undefined");
        assert_eq!(js::to_string(Some(&json!(1.0))), "1");
        assert_eq!(js::to_string(Some(&json!([1, null, "a"]))), "1,,a");
        assert_eq!(js::trim("\u{FEFF} a\u{85} \n"), "a\u{85}");
        assert_eq!(js::str_to_number(" 12 "), 12.0);
        assert_eq!(js::str_to_number(""), 0.0);
        assert_eq!(js::str_to_number("0x10"), 16.0);
        assert_eq!(js::str_to_number("1e3"), 1000.0);
        assert_eq!(js::str_to_number("-Infinity"), f64::NEG_INFINITY);
        for bad in ["abc", "inf", "nan", "0x", "0x-1", "1 2", "--1"] {
            assert!(js::str_to_number(bad).is_nan(), "{bad}");
        }
    }
}
