//! Ports `core/lib/config.mjs`: layered configuration (plugin defaults < global < project < project-local).
//!
//! Explicit user decisions live in `.dfw/decisions.jsonl` and are applied by the orchestrator
//! skill on top of this (they are not config values).
//!
//! [`load_config`] returns the merged config as a `Value` with the key order `loadConfig` gives in
//! Node. The typed accessors below it read the fields other modules use most, with the fallbacks
//! the Node callers apply (`config.checks?.maxFindings || 12` and the like).

use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::error::DfwError;
use super::paths::{global_dir, project_paths};
use super::util::{deep_merge, get_path, js_number, read_json, set_path, to_json_compact, write_json};

/// `DEFAULTS`: the plugin's own layer.
pub fn defaults() -> Value {
    json!({
        "schema": 1,
        "language": {
            // "session": ask on the first /dfw run in every session (user decision 2026-11-13).
            "askPolicy": "session",
            "default": "en",
        },
        "gate": {
            // Paths (globs, relative to project root) always writable regardless of gates.
            "allow": [".dfw/**", "design/handoff/**"],
            // Paths that belong to the design workspace (writable before G2, locked after).
            "designPaths": ["design/**", "PRODUCT.md", "DESIGN.md"],
            // Frozen snapshots are never writable.
            "immutable": ["design/approved/**"],
        },
        "checks": {
            // PostToolUse checks: "fast" (dfw lint), "full" (all tools) or false.
            "onEdit": "fast",
            "timeoutMs": 8000,
            "impeccable": true,
            "sloplint": true,
            "tasteCopyLint": false,
            "maxFindings": 12,
        },
        "studio": {
            "port": 4600,
            "breakpoints": [
                { "id": "desktop", "label": "Desktop", "width": 1440, "height": 900 },
                { "id": "tablet", "label": "Tablet", "width": 834, "height": 1112 },
                { "id": "mobile", "label": "Mobile", "width": 390, "height": 844 },
            ],
        },
        "review": {
            // dfw review: key that toggles comment mode on the running app ("off": the Comment button only).
            // Alt/Option+C rather than C, so the app's own single-key shortcuts keep working.
            "shortcut": "alt+c",
        },
        "accessibility": { "standard": "WCAG 2.2", "level": "AA" },
        "skills": {
            // Optional skills that must be enabled per project.
            "optional": { "hallmark": true, "taste-reference": true },
            "disabled": [],
        },
        "env": {
            // Applied to every external tool dfw launches.
            "DO_NOT_TRACK": "1",
            "IMPECCABLE_NO_UPDATE_CHECK": "1",
            "IMPECCABLE_HOOK_DISABLED": "1",
        },
    })
}

/// `globalConfigPath()`: `config.json` in the global dfw directory.
pub fn global_config_path() -> PathBuf {
    global_dir().join("config.json")
}

/// `loadConfig(root)`: defaults, then the global file, then (with a root) the project file and
/// the project-local file. A missing file is an empty layer; broken JSON is an error.
pub fn load_config(root: Option<&Path>) -> Result<Value, DfwError> {
    load_config_from(&global_config_path(), root)
}

/// [`load_config`] with the global file given, so tests need no environment variable.
fn load_config_from(global_file: &Path, root: Option<&Path>) -> Result<Value, DfwError> {
    let read = |file: PathBuf| -> Result<(Option<PathBuf>, Value), DfwError> {
        // deepMerge assigns `out['__proto__']`, which sets a prototype instead of a key: such a
        // key never reaches the merged config, so it is dropped here.
        let layer = without_proto(read_json(&file, Some(Value::Object(Map::new())))?);
        Ok((Some(file), layer))
    };
    let mut layers = vec![(None, defaults()), read(global_file.to_path_buf())?];
    // An empty root is falsy in Node: no project layers.
    if let Some(root) = root.filter(|r| !r.as_os_str().is_empty()) {
        let p = project_paths(root);
        layers.push(read(p.config)?);
        layers.push(read(p.config_local)?);
    }
    let mut merged = Value::Object(Map::new());
    for (_, layer) in &layers {
        merged = deep_merge(merged, layer.clone());
    }
    let Value::Object(mut merged) = merged else {
        // Only a last layer that is not an object ends here (Node: a TypeError, or for an array
        // a config whose keys JSON.stringify does not show).
        let file = layers.last().and_then(|(file, _)| file.as_ref()).map(|f| f.display().to_string()).unwrap_or_default();
        return Err(DfwError::new(format!("Config file {file} must hold a JSON object. Fix or delete it.")));
    };
    // Some lists are unions across layers rather than replacements, so a project can add to the
    // defaults without accidentally dropping them.
    let union = |group: &str, key: &str| union_of(layers.iter().map(|(_, layer)| layer), group, key);
    let mut skills = js_spread(merged.get("skills"));
    skills.insert("disabled".into(), union("skills", "disabled"));
    merged.insert("skills".into(), Value::Object(skills));
    let mut gate = js_spread(merged.get("gate"));
    gate.insert("allow".into(), union("gate", "allow"));
    gate.insert("immutable".into(), union("gate", "immutable"));
    merged.insert("gate".into(), Value::Object(gate));
    Ok(Value::Object(merged))
}

/// `[...new Set(layers.flatMap((l) => l?.[group]?.[key] || []))]`.
fn union_of<'a>(layers: impl Iterator<Item = &'a Value>, group: &str, key: &str) -> Value {
    let mut out: Vec<Value> = Vec::new();
    let mut add = |item: &Value| {
        // A Set compares objects and arrays by identity: two of them are never the same entry.
        let composite = item.is_object() || item.is_array();
        if composite || !out.iter().any(|seen| same_value_zero(seen, item)) {
            out.push(item.clone());
        }
    };
    for layer in layers {
        let found = layer.as_object().and_then(|l| l.get(group)).and_then(Value::as_object).and_then(|g| g.get(key));
        match found {
            Some(Value::Array(items)) => items.iter().for_each(&mut add),
            // flatMap keeps a value that is not an array as one entry ("allow": "docs/**").
            Some(other) if js_truthy(other) => add(other),
            _ => {}
        }
    }
    Value::Array(out)
}

/// How a JavaScript `Set` compares two primitives (`1` and `1.0` are one number).
fn same_value_zero(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        _ => a == b,
    }
}

/// Drop `__proto__` keys from objects (not from objects inside arrays: arrays are not merged).
fn without_proto(value: Value) -> Value {
    match value {
        Value::Object(map) => Value::Object(map.into_iter().filter(|(k, _)| k != "__proto__").map(|(k, v)| (k, without_proto(v))).collect()),
        other => other,
    }
}

/// JavaScript truthiness of a JSON value.
pub fn js_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `{ ...value }`: an object's own keys, an array's or a string's indexes, nothing for the rest.
pub fn js_spread(value: Option<&Value>) -> Map<String, Value> {
    match value {
        Some(Value::Object(map)) => map.clone(),
        Some(Value::Array(items)) => items.iter().enumerate().map(|(i, v)| (i.to_string(), v.clone())).collect(),
        // JavaScript indexes a string by UTF-16 unit; half of a surrogate pair has no JSON text here.
        Some(Value::String(s)) => s.encode_utf16().enumerate().map(|(i, unit)| (i.to_string(), Value::String(String::from_utf16_lossy(&[unit])))).collect(),
        _ => Map::new(),
    }
}

/// `String(value)` for a JSON scalar; an array or object as compact JSON.
fn js_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(js_number).unwrap_or_else(|| n.to_string()),
        other => to_json_compact(other),
    }
}

/// Which config file a `dfw config set` writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Global,
    Project,
    Local,
}

/// `configFileFor(scope, root)`.
pub fn config_file_for(scope: Scope, root: &Path) -> PathBuf {
    match scope {
        Scope::Global => global_config_path(),
        Scope::Local => project_paths(root).config_local,
        Scope::Project => project_paths(root).config,
    }
}

/// `setConfigValue(scope, root, key, value)`: set a dotted key in one layer's file and return the file.
pub fn set_config_value(scope: Scope, root: &Path, key: &str, value: Value) -> Result<PathBuf, DfwError> {
    let file = config_file_for(scope, root);
    let mut data = read_json(&file, Some(Value::Object(Map::new())))?;
    set_path(&mut data, key, value);
    write_json(&file, &data)?;
    Ok(file)
}

/// `getConfigValue(root, key)`: a dotted key of the merged config; `None` where Node gives `undefined`.
pub fn get_config_value(root: Option<&Path>, key: &str) -> Result<Option<Value>, DfwError> {
    Ok(get_path(&load_config(root)?, key).cloned())
}

// ---------------------------------------------------------------------------------------------
// Typed accessors: each takes the value `load_config` returned.
// ---------------------------------------------------------------------------------------------

fn strings(config: &Value, path: &str) -> Vec<String> {
    let items = get_path(config, path).and_then(Value::as_array);
    items.map(|list| list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

/// A positive number at `path`, else `None` (the `value || default` of the Node callers).
fn positive(config: &Value, path: &str) -> Option<f64> {
    get_path(config, path).and_then(Value::as_f64).filter(|n| *n >= 1.0)
}

/// `gate.allow`: globs always writable regardless of gates.
pub fn gate_allow(config: &Value) -> Vec<String> {
    strings(config, "gate.allow")
}

/// `gate.designPaths`: globs of the design workspace (writable before G2, locked after).
pub fn gate_design_paths(config: &Value) -> Vec<String> {
    strings(config, "gate.designPaths")
}

/// `gate.immutable`: globs that are never writable.
pub fn gate_immutable(config: &Value) -> Vec<String> {
    strings(config, "gate.immutable")
}

/// `studio.port`, or 4600 when it is missing or not a port (`config.studio.port || 4600`).
pub fn studio_port(config: &Value) -> u16 {
    let port = match get_path(config, "studio.port") {
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(other) => other.as_f64(),
        None => None,
    };
    port.filter(|p| p.fract() == 0.0 && (1.0..=65535.0).contains(p)).map_or(4600, |p| p as u16)
}

/// One Studio breakpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Breakpoint {
    pub id: String,
    pub label: Option<String>,
    pub width: u32,
    pub height: u32,
}

impl Breakpoint {
    /// Read `{ id, label, width, height }`; `None` when id, width or height is missing.
    /// Also for the breakpoints of `design/design.json`, which win over the config.
    pub fn from_value(value: &Value) -> Option<Breakpoint> {
        let size = |key: &str| value.get(key).and_then(Value::as_f64).filter(|n| *n >= 1.0).map(|n| n as u32);
        Some(Breakpoint {
            id: value.get("id")?.as_str()?.to_string(),
            label: value.get("label").and_then(Value::as_str).map(str::to_string),
            width: size("width")?,
            height: size("height")?,
        })
    }
}

/// `studio.breakpoints`; entries that are not breakpoints are left out.
pub fn studio_breakpoints(config: &Value) -> Vec<Breakpoint> {
    let items = get_path(config, "studio.breakpoints").and_then(Value::as_array);
    items.map(|list| list.iter().filter_map(Breakpoint::from_value).collect()).unwrap_or_default()
}

/// What runs after an edit (`checks.onEdit`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnEdit {
    /// `false`: no checks.
    Off,
    /// `"fast"` and anything unknown: dfw lint only.
    Fast,
    /// `"full"` or `true`: every tool.
    Full,
}

/// `checks.onEdit` as the PostToolUse hook reads it.
pub fn checks_on_edit(config: &Value) -> OnEdit {
    match get_path(config, "checks.onEdit") {
        Some(Value::Bool(false)) => OnEdit::Off,
        Some(Value::Bool(true)) => OnEdit::Full,
        Some(Value::String(s)) if s == "full" => OnEdit::Full,
        _ => OnEdit::Fast,
    }
}

/// `checks.timeoutMs || 8000`.
pub fn checks_timeout_ms(config: &Value) -> u64 {
    positive(config, "checks.timeoutMs").map_or(8000, |n| n as u64)
}

/// `checks.maxFindings || 12`.
pub fn checks_max_findings(config: &Value) -> usize {
    positive(config, "checks.maxFindings").map_or(12, |n| n as usize)
}

/// `checks.layoutOnStop !== false`: the layout check of the Stop hook runs unless it is switched off.
pub fn checks_layout_on_stop(config: &Value) -> bool {
    get_path(config, "checks.layoutOnStop") != Some(&Value::Bool(false))
}

/// `review.shortcut` as text; `None` when it is missing or null (the caller then uses its default).
pub fn review_shortcut(config: &Value) -> Option<String> {
    get_path(config, "review.shortcut").filter(|v| !v.is_null()).map(js_string)
}

/// `env`: variables applied to every external tool dfw launches, in file order.
pub fn env_map(config: &Value) -> Vec<(String, String)> {
    let Some(Value::Object(env)) = config.get("env") else { return Vec::new() };
    env.iter().filter(|(_, v)| !v.is_object() && !v.is_array()).map(|(k, v)| (k.clone(), js_string(v))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::fs;

    /// A project folder with the given `.dfw/config.json` and `.dfw/config.local.json`.
    fn project(label: &str, config: Option<&str>, local: Option<&str>) -> PathBuf {
        let root = test_dir(label);
        fs::create_dir_all(root.join(".dfw")).unwrap();
        for (name, text) in [("config.json", config), ("config.local.json", local)] {
            if let Some(text) = text {
                fs::write(root.join(".dfw").join(name), text).unwrap();
            }
        }
        root
    }

    fn load(root: &Path) -> Value {
        load_config_from(&root.join("no-global.json"), Some(root)).unwrap()
    }

    /// external tools run with telemetry and update checks off (tests/unit/dfw.test.mjs; the
    /// launcher assertions of that case test the overlays, not this module)
    #[test]
    fn external_tools_run_with_telemetry_and_update_checks_off() {
        let d = defaults();
        assert_eq!(d["env"]["DO_NOT_TRACK"], "1");
        assert_eq!(d["env"]["IMPECCABLE_NO_UPDATE_CHECK"], "1");
        assert_eq!(d["env"]["IMPECCABLE_HOOK_DISABLED"], "1");
    }

    /// config: gate.allow is a union across layers (tests/unit/dfw.test.mjs)
    #[test]
    fn gate_allow_is_a_union_across_layers() {
        let root = project("config-union", Some(r#"{"gate":{"allow":["docs/**"]},"checks":{"onEdit":false}}"#), None);
        let c = load(&root);
        assert!(gate_allow(&c).contains(&"docs/**".to_string()));
        assert!(gate_allow(&c).contains(&".dfw/**".to_string()), "defaults kept");
        assert_eq!(c["checks"]["onEdit"], false);
        assert_eq!(c["checks"]["sloplint"], defaults()["checks"]["sloplint"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn without_files_the_config_is_the_defaults_in_node_key_order() {
        let tmp = test_dir("config-none");
        let expected = to_json_compact(&defaults());
        assert_eq!(to_json_compact(&load_config_from(&tmp.join("none.json"), None).unwrap()), expected);
        assert_eq!(to_json_compact(&load_config_from(&tmp.join("none.json"), Some(Path::new(""))).unwrap()), expected, "an empty root is no root");
        assert_eq!(to_json_compact(&load(&tmp)), expected);
        assert!(expected.starts_with(r#"{"schema":1,"language":{"askPolicy":"session","default":"en"},"gate":{"allow":[".dfw/**","design/handoff/**"],"designPaths":["#));
        assert!(expected.ends_with(r#""skills":{"optional":{"hallmark":true,"taste-reference":true},"disabled":[]},"env":{"DO_NOT_TRACK":"1","IMPECCABLE_NO_UPDATE_CHECK":"1","IMPECCABLE_HOOK_DISABLED":"1"}}"#));
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn layers_merge_in_order_and_lists_are_unions() {
        let root = project(
            "config-layers",
            Some(r#"{"gate":{"allow":["docs/**",".dfw/**"],"immutable":["vendor/**"],"designPaths":["ui/**"]},"studio":{"port":4700},"extra":{"a":1}}"#),
            Some(r#"{"gate":{"allow":["notes/**","docs/**"]},"studio":{"port":4800},"skills":{"disabled":["hallmark"]},"extra":{"b":2}}"#),
        );
        let global = root.join("global.json");
        fs::write(&global, r#"{"gate":{"allow":["global/**"]},"studio":{"port":4650},"language":{"default":"vi"}}"#).unwrap();
        let c = load_config_from(&global, Some(&root)).unwrap();
        assert_eq!(gate_allow(&c), [".dfw/**", "design/handoff/**", "global/**", "docs/**", "notes/**"]);
        assert_eq!(gate_immutable(&c), ["design/approved/**", "vendor/**"]);
        assert_eq!(gate_design_paths(&c), ["ui/**"], "other lists replace");
        assert_eq!(studio_port(&c), 4800, "local wins");
        assert_eq!(c["language"], json!({"askPolicy": "session", "default": "vi"}));
        assert_eq!(c["skills"]["disabled"], json!(["hallmark"]));
        assert_eq!(to_json_compact(&c["extra"]), r#"{"a":1,"b":2}"#);
        let keys: Vec<&String> = c.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["schema", "language", "gate", "checks", "studio", "review", "accessibility", "skills", "env", "extra"]);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn odd_values_follow_the_javascript_rules() {
        // A scalar union entry is one entry; numbers dedupe by value, objects never; a spread of
        // a string or null replaces what the layer wrote; `__proto__` never becomes a key.
        let root = project(
            "config-odd",
            Some(r#"{"gate":{"allow":"docs/**","immutable":[1,{"a":1},null]},"skills":"ab","__proto__":{"x":1},"checks":{"__proto__":{"y":1}}}"#),
            Some(r#"{"gate":{"allow":0,"immutable":[1.0,{"a":1},null]},"studio":null}"#),
        );
        let c = load(&root);
        assert_eq!(c["gate"]["allow"], json!([".dfw/**", "design/handoff/**", "docs/**"]));
        assert_eq!(to_json_compact(&c["gate"]["immutable"]), r#"["design/approved/**",1,{"a":1},null,{"a":1}]"#);
        assert_eq!(to_json_compact(&c["skills"]), r#"{"0":"a","1":"b","disabled":[]}"#);
        assert_eq!(c["studio"], Value::Null);
        assert!(c.get("__proto__").is_none() && c["checks"].get("__proto__").is_none());
        assert_eq!(studio_port(&c), 4600);
        assert!(studio_breakpoints(&c).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_broken_or_non_object_file_is_an_error() {
        let root = project("config-broken", Some("{ nope"), None);
        let err = load_config_from(&root.join("none.json"), Some(&root)).unwrap_err();
        assert!(err.message().starts_with("Invalid JSON in "), "{err}");
        fs::write(root.join(".dfw").join("config.json"), "{}").unwrap();
        fs::write(root.join(".dfw").join("config.local.json"), "[1]").unwrap();
        let err = load_config_from(&root.join("none.json"), Some(&root)).unwrap_err();
        assert!(err.message().contains("config.local.json must hold a JSON object"), "{err}");
        // A non-object layer below another one is simply replaced, as deepMerge does.
        fs::write(root.join(".dfw").join("config.json"), "[1]").unwrap();
        fs::write(root.join(".dfw").join("config.local.json"), r#"{"a":1}"#).unwrap();
        assert_eq!(to_json_compact(&load(&root)), r#"{"a":1,"skills":{"disabled":[]},"gate":{"allow":[".dfw/**","design/handoff/**"],"immutable":["design/approved/**"]}}"#);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn typed_accessors_apply_the_fallbacks_of_the_node_callers() {
        let d = defaults();
        assert_eq!(studio_port(&d), 4600);
        let bps = studio_breakpoints(&d);
        assert_eq!(bps.len(), 3);
        assert_eq!(bps[1], Breakpoint { id: "tablet".into(), label: Some("Tablet".into()), width: 834, height: 1112 });
        assert_eq!(checks_on_edit(&d), OnEdit::Fast);
        assert_eq!(checks_timeout_ms(&d), 8000);
        assert_eq!(checks_max_findings(&d), 12);
        assert!(checks_layout_on_stop(&d));
        assert_eq!(review_shortcut(&d).as_deref(), Some("alt+c"));
        assert_eq!(env_map(&d), [("DO_NOT_TRACK".to_string(), "1".to_string()), ("IMPECCABLE_NO_UPDATE_CHECK".into(), "1".into()), ("IMPECCABLE_HOOK_DISABLED".into(), "1".into())]);

        let c = json!({
            "checks": { "onEdit": true, "timeoutMs": 0, "maxFindings": "many", "layoutOnStop": false },
            "studio": { "port": "4700", "breakpoints": [{ "id": "wide", "width": 1920, "height": 1080 }, "junk", { "id": "no-size" }] },
            "review": { "shortcut": false },
            "env": { "A": 1, "B": true, "C": null, "D": {} },
        });
        assert_eq!(checks_on_edit(&c), OnEdit::Full);
        assert_eq!(checks_on_edit(&json!({"checks": {"onEdit": "full"}})), OnEdit::Full);
        assert_eq!(checks_on_edit(&json!({"checks": {"onEdit": false}})), OnEdit::Off);
        assert_eq!(checks_on_edit(&json!({"checks": {"onEdit": null}})), OnEdit::Fast);
        assert_eq!(checks_timeout_ms(&c), 8000);
        assert_eq!(checks_max_findings(&c), 12);
        assert!(!checks_layout_on_stop(&c));
        assert_eq!(studio_port(&c), 4700);
        assert_eq!(studio_port(&json!({"studio": {"port": 99999}})), 4600);
        assert_eq!(studio_breakpoints(&c), [Breakpoint { id: "wide".into(), label: None, width: 1920, height: 1080 }]);
        assert_eq!(review_shortcut(&c).as_deref(), Some("false"));
        assert_eq!(review_shortcut(&json!({"review": {"shortcut": null}})), None);
        assert_eq!(review_shortcut(&json!({})), None);
        assert_eq!(env_map(&c), [("A".to_string(), "1".to_string()), ("B".into(), "true".into()), ("C".into(), "null".into())]);
        assert!(gate_allow(&json!({"gate": {"allow": "x"}})).is_empty());
    }

    #[test]
    fn set_and_get_a_config_value() {
        let root = project("config-set", Some(r#"{"schema":1}"#), None);
        assert_eq!(config_file_for(Scope::Local, &root), root.join(".dfw").join("config.local.json"));
        let file = set_config_value(Scope::Project, &root, "studio.port", json!(4777)).unwrap();
        assert_eq!(file, root.join(".dfw").join("config.json"));
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\n  \"schema\": 1,\n  \"studio\": {\n    \"port\": 4777\n  }\n}\n");
        set_config_value(Scope::Local, &root, "review.shortcut", json!("off")).unwrap();
        let c = load(&root);
        assert_eq!(get_path(&c, "studio.port"), Some(&json!(4777)));
        assert_eq!(get_path(&c, "review.shortcut"), Some(&json!("off")));
        assert_eq!(get_path(&c, "studio.nope"), None);
        fs::remove_dir_all(&root).unwrap();
    }
}
