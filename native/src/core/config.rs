//! Ports `core/lib/config.mjs`: layered configuration (plugin defaults < global < project < project-local).
//!
//! Explicit user decisions live in `.dfw/decisions.jsonl` and are applied by the orchestrator
//! skill on top of this (they are not config values).
//!
//! [`load_config`] returns the merged config as a `Value` with the key order `loadConfig` gives in
//! Node. The typed accessors below it read the fields other modules use most, with the fallbacks
//! the Node callers apply (`config.checks?.maxFindings || 12` and the like).
//!
//! Over 800 lines only with its tests (the ported cases of the JavaScript tests sit next to the code).

use crate::core::js;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::error::DfwError;
use super::paths::rel::all_under;
use super::paths::{file, global_dir, project_paths, rel};
use super::util::{deep_merge, get_path, read_json, set_path, write_json, write_json_inside};

/// The sections of the config the code reads by name, and the dotted keys of single values.
pub mod key {
    /// The format of a config file (`1`).
    pub const SCHEMA: &str = "schema";
    pub const LANGUAGE: &str = "language";
    pub const GATE: &str = "gate";
    pub const CHECKS: &str = "checks";
    pub const STUDIO: &str = "studio";
    pub const REVIEW: &str = "review";
    pub const ACCESSIBILITY: &str = "accessibility";
    pub const SKILLS: &str = "skills";
    pub const ENV: &str = "env";
    /// Where telemetry goes and whether it is on (global config only).
    pub const TELEMETRY: &str = "telemetry";
    /// The Penpot bridge: `penpot.origins` (global config only).
    pub const PENPOT: &str = "penpot";
    pub const STUDIO_PORT: &str = "studio.port";
    pub const STUDIO_BREAKPOINTS: &str = "studio.breakpoints";
    pub const REVIEW_SHORTCUT: &str = "review.shortcut";
    /// The marketplace clone `dfw install --repo` recorded (global config only).
    pub const MARKETPLACE_REPO: &str = "marketplaceRepo";

    /* `language` */
    /// When dfw asks for the language (`session`: on the first run of every session).
    pub const ASK_POLICY: &str = "askPolicy";
    /// The language used until one is chosen.
    pub const DEFAULT: &str = "default";

    /* `gate` */
    /// Globs always writable, whatever the gates say.
    pub const ALLOW: &str = "allow";
    /// Globs of the design workspace (writable before G2, locked after).
    pub const DESIGN_PATHS: &str = "designPaths";
    /// Globs the agent's own tools may never write.
    pub const IMMUTABLE: &str = "immutable";

    /* `checks` */
    /// The checks after an edit: `"fast"`, `"full"` or `false`.
    pub const ON_EDIT: &str = "onEdit";
    pub const TIMEOUT_MS: &str = "timeoutMs";
    pub const IMPECCABLE: &str = "impeccable";
    pub const SLOPLINT: &str = "sloplint";
    pub const TASTE_COPY_LINT: &str = "tasteCopyLint";
    pub const MAX_FINDINGS: &str = "maxFindings";

    /* `studio` */
    pub const PORT: &str = "port";
    pub const BREAKPOINTS: &str = "breakpoints";

    /* `review` */
    /// The key that toggles comment mode on the running app.
    pub const SHORTCUT: &str = "shortcut";

    /* `accessibility` */
    pub const STANDARD: &str = "standard";
    pub const LEVEL: &str = "level";

    /* `skills` */
    /// The optional skills: `{ <skill>: true }`.
    pub const OPTIONAL: &str = "optional";
    /// The skills a project turned off (a union across layers).
    pub const DISABLED: &str = "disabled";

    /// The keys of one breakpoint: `{ id, label, width, height }` (also in `design/design.json`).
    pub mod breakpoint {
        pub const ID: &str = "id";
        pub const LABEL: &str = "label";
        pub const WIDTH: &str = "width";
        pub const HEIGHT: &str = "height";
    }

    /// The optional skills of `skills.optional`.
    pub mod skill {
        pub const HALLMARK: &str = "hallmark";
        pub const TASTE_REFERENCE: &str = "taste-reference";
    }

    /// The variables of `env` dfw sets for every external tool.
    pub mod env {
        pub const DO_NOT_TRACK: &str = "DO_NOT_TRACK";
        pub const IMPECCABLE_NO_UPDATE_CHECK: &str = "IMPECCABLE_NO_UPDATE_CHECK";
        pub const IMPECCABLE_HOOK_DISABLED: &str = "IMPECCABLE_HOOK_DISABLED";
        pub const IMPECCABLE_NO_TELEMETRY: &str = "IMPECCABLE_NO_TELEMETRY";
    }
}

/// The default `gate.immutable`: the frozen snapshots and every [`rel::GATE_STATE`] file.
fn default_immutable() -> Vec<String> {
    let gate_state = rel::GATE_STATE.iter().map(|r| {
        if rel::GATE_STATE_DIRS.contains(r) {
            all_under(r)
        } else {
            r.to_string()
        }
    });
    std::iter::once(all_under(rel::APPROVED))
        .chain(gate_state)
        .collect()
}

/// `DEFAULTS`: the plugin's own layer.
pub fn defaults() -> Value {
    json!({
        (key::SCHEMA): 1,
        (key::LANGUAGE): {
            // "session": ask on the first /dfw run in every session (user decision 2026-11-13).
            (key::ASK_POLICY): "session",
            (key::DEFAULT): "en",
        },
        (key::GATE): {
            // Paths (globs, relative to project root) always writable regardless of gates.
            (key::ALLOW): [all_under(rel::DFW), all_under(rel::HANDOFF)],
            // Paths that belong to the design workspace (writable before G2, locked after).
            (key::DESIGN_PATHS): [all_under(rel::DESIGN), rel::PRODUCT_DOC, rel::DESIGN_DOC],
            // Frozen snapshots are never writable, nor (by the agent's own tools) the files that
            // hold the gates, approvals, decisions, settings and review comments: dfw commands
            // write those directly. A union across layers: a project cannot drop one.
            (key::IMMUTABLE): default_immutable(),
        },
        (key::CHECKS): {
            // PostToolUse checks: "fast" (dfw lint), "full" (all tools) or false.
            (key::ON_EDIT): "fast",
            (key::TIMEOUT_MS): 8000,
            (key::IMPECCABLE): true,
            (key::SLOPLINT): true,
            (key::TASTE_COPY_LINT): false,
            (key::MAX_FINDINGS): 12,
        },
        (key::STUDIO): {
            (key::PORT): 4600,
            (key::BREAKPOINTS): [
                { (key::breakpoint::ID): "desktop", (key::breakpoint::LABEL): "Desktop", (key::breakpoint::WIDTH): 1440, (key::breakpoint::HEIGHT): 900 },
                { (key::breakpoint::ID): "tablet", (key::breakpoint::LABEL): "Tablet", (key::breakpoint::WIDTH): 834, (key::breakpoint::HEIGHT): 1112 },
                { (key::breakpoint::ID): "mobile", (key::breakpoint::LABEL): "Mobile", (key::breakpoint::WIDTH): 390, (key::breakpoint::HEIGHT): 844 },
            ],
        },
        (key::REVIEW): {
            // dfw review: key that toggles comment mode on the running app ("off": the Comment button only).
            // Alt/Option+C rather than C, so the app's own single-key shortcuts keep working.
            (key::SHORTCUT): "alt+c",
        },
        (key::ACCESSIBILITY): { (key::STANDARD): "WCAG 2.2", (key::LEVEL): "AA" },
        (key::SKILLS): {
            // Optional skills that must be enabled per project.
            (key::OPTIONAL): { (key::skill::HALLMARK): true, (key::skill::TASTE_REFERENCE): true },
            (key::DISABLED): [],
        },
        (key::ENV): {
            // Applied to every external tool dfw launches.
            (key::env::DO_NOT_TRACK): "1",
            (key::env::IMPECCABLE_NO_UPDATE_CHECK): "1",
            (key::env::IMPECCABLE_HOOK_DISABLED): "1",
        },
    })
}

/// `globalConfigPath()`: `config.json` in the global dfw directory.
pub fn global_config_path() -> PathBuf {
    global_dir().join(file::CONFIG)
}

/// `PROJECT_ENV_ALLOW`: the `env` names a project layer may set. Project layers come from the
/// repository being opened, so they are untrusted: their `env` may only set names that cannot
/// choose what runs or where it connects. An allowlist, not a denylist: loader, proxy and
/// tool-specific variables cannot be enumerated. The user's global config may set any variable.
pub const PROJECT_ENV_ALLOW: [&str; 5] = ["NO_COLOR", "FORCE_COLOR", "TZ", "LANG", "LC_ALL"];

/// The value that turns a [`PRIVACY_ENV`] switch on.
pub const PRIVACY_ON: &str = "1";

/// `PRIVACY_ENV`: switches a project layer may only keep on ([`PRIVACY_ON`]).
pub const PRIVACY_ENV: [&str; 4] = [
    key::env::DO_NOT_TRACK,
    key::env::IMPECCABLE_NO_UPDATE_CHECK,
    key::env::IMPECCABLE_HOOK_DISABLED,
    key::env::IMPECCABLE_NO_TELEMETRY,
];

/// `filterProjectEnv(layer, file, warn)`: a project layer without the `env` entries it may not
/// set; one warning per dropped key. The `env` key keeps its place in the layer.
pub fn filter_project_env(mut layer: Value, file: &Path, warn: &mut dyn FnMut(String)) -> Value {
    let Some(map) = layer.as_object_mut() else {
        return layer;
    };
    let Some(env) = map.get_mut(key::ENV) else {
        return layer;
    };
    let file = file.display();
    let mut kept = Map::new();
    if let Value::Object(entries) = env {
        // Object.entries order: integer-like keys first, as the warnings come out in Node.
        for (k, v) in js::entries(entries) {
            let allowed = v.as_str().is_some_and(|v| {
                PROJECT_ENV_ALLOW.contains(&k.as_str())
                    || (PRIVACY_ENV.contains(&k.as_str()) && v == PRIVACY_ON)
            });
            if allowed {
                kept.insert(k.clone(), v.clone());
            } else {
                warn(format!("dfw: ignored env.{k} in {file} (a project config cannot set it; use the global config: dfw config set env.{k} <value> --global)"));
            }
        }
    } else {
        warn(format!(
            "dfw: ignored env in {file} (it must be an object of strings)"
        ));
    }
    *env = Value::Object(kept);
    layer
}

/// `GLOBAL_ONLY`: the sections only the user's global config may set (`security-privacy.md`
/// S10): where telemetry goes, who may drive the Penpot bridge, the Impeccable engine opt-in and
/// the marketplace clone. Their readers read the global file; a project layer loses them in
/// [`load_config`] too, so a new reader of the merged config cannot take them from a project.
pub const GLOBAL_ONLY: [&str; 4] = [
    key::TELEMETRY,
    key::PENPOT,
    key::IMPECCABLE,
    key::MARKETPLACE_REPO,
];

/// The ends of a key name that choose what runs or where it connects (`bin`, `impeccableBin`,
/// `command`, `url`, `endpointUrl`): only the global config may set one, at any depth.
pub const GLOBAL_ONLY_NAMES: [&str; 3] = ["bin", "command", "url"];

/// Whether a project layer may not set the key `name`, by [`GLOBAL_ONLY_NAMES`].
fn names_what_runs(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    GLOBAL_ONLY_NAMES.iter().any(|end| {
        lower == *end
            || name.len() > end.len() && lower.ends_with(end) && {
                // camelCase (`impeccableBin`) or a separator (`impeccable_bin`, `tool-url`)
                let at = name.len() - end.len();
                name[at..].starts_with(|c: char| c.is_ascii_uppercase())
                    || name[..at].ends_with(['_', '-', '.'])
            }
    })
}

/// `filterProjectGlobalOnly`: a project layer without the [`GLOBAL_ONLY`] sections and the
/// [`GLOBAL_ONLY_NAMES`] keys; one warning per dropped key, as for `env`.
pub fn filter_project_global_only(
    mut layer: Value,
    file: &Path,
    warn: &mut dyn FnMut(String),
) -> Value {
    fn drop_names(
        map: &mut Map<String, Value>,
        at: &str,
        file: &str,
        warn: &mut dyn FnMut(String),
    ) {
        let names: Vec<String> = map.keys().cloned().collect();
        for name in names {
            let dotted = if at.is_empty() {
                name.clone()
            } else {
                format!("{at}.{name}")
            };
            if names_what_runs(&name) {
                map.remove(&name);
                warn(global_only_warning(&dotted, "", file));
            } else if let Some(Value::Object(inner)) = map.get_mut(&name) {
                drop_names(inner, &dotted, file, warn);
            }
        }
    }
    let Some(map) = layer.as_object_mut() else {
        return layer;
    };
    let file = file.display().to_string();
    for section in GLOBAL_ONLY {
        if map.remove(section).is_some() {
            warn(global_only_warning(section, ".<key>", &file));
        }
    }
    drop_names(map, "", &file, warn);
    layer
}

/// The warning for a dropped global-only key; `rest` completes the key `dfw config set` takes.
fn global_only_warning(dotted: &str, rest: &str, file: &str) -> String {
    format!("dfw: ignored {dotted} in {file} (a project config cannot set it; use the global config: dfw config set {dotted}{rest} <value> --global)")
}

/// A project layer as [`load_config`] merges it: the `env` it may set, nothing global-only.
fn project_layer(file: &Path, warn: &mut dyn FnMut(String)) -> Result<Value, DfwError> {
    let layer = filter_project_env(read_layer(file)?, file, warn);
    Ok(filter_project_global_only(layer, file, warn))
}

/// `readLayer(file)`: a config file's content (`{}` when missing); anything but a JSON object is
/// the user's error to fix.
fn read_layer(file: &Path) -> Result<Value, DfwError> {
    let data = read_json(file, Some(Value::Object(Map::new())))?;
    if !data.is_object() {
        return Err(DfwError::new(format!(
            "Config file {} must hold a JSON object. Fix it or delete it.",
            file.display()
        )));
    }
    Ok(data)
}

/// `loadConfig(root)`: defaults, then the global file, then (with a root) the project file and
/// the project-local file. A missing file is an empty layer; broken JSON or a file that is not an
/// object is an error. Dropped project `env` entries are reported on stderr.
pub fn load_config(root: Option<&Path>) -> Result<Value, DfwError> {
    load_config_from(&global_config_path(), root, &mut |m| eprintln!("{m}"))
}

/// [`load_config`] without the warnings, for a reader that runs after a command already loaded
/// the config and said what it dropped (the layout cache in the stop hook).
pub fn load_config_quietly(root: &Path) -> Result<Value, DfwError> {
    load_config_from(&global_config_path(), Some(root), &mut |_| {})
}

/// [`load_config`] with the global file and the warning sink given, so tests need no environment
/// variable.
fn load_config_from(
    global_file: &Path,
    root: Option<&Path>,
    warn: &mut dyn FnMut(String),
) -> Result<Value, DfwError> {
    let mut layers = vec![defaults(), read_layer(global_file)?];
    // An empty root is falsy in Node: no project layers.
    if let Some(root) = root.filter(|r| !r.as_os_str().is_empty()) {
        let p = project_paths(root);
        layers.push(project_layer(&p.config, warn)?);
        layers.push(project_layer(&p.config_local, warn)?);
    }
    let mut merged = Value::Object(Map::new());
    for layer in &layers {
        merged = deep_merge(merged, layer.clone());
    }
    let Value::Object(mut merged) = merged else {
        return Err(DfwError::internal("the merged config is not an object")); // every layer is one
    };
    // Some lists are unions across layers rather than replacements, so a project can add to the
    // defaults without accidentally dropping them.
    let union = |group: &str, key: &str| union_of(layers.iter(), group, key);
    let mut skills = js::spread(merged.get(key::SKILLS));
    skills.insert(key::DISABLED.into(), union(key::SKILLS, key::DISABLED));
    merged.insert(key::SKILLS.into(), Value::Object(skills));
    let mut gate = js::spread(merged.get(key::GATE));
    gate.insert(key::ALLOW.into(), union(key::GATE, key::ALLOW));
    gate.insert(key::IMMUTABLE.into(), union(key::GATE, key::IMMUTABLE));
    merged.insert(key::GATE.into(), Value::Object(gate));
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
        let found = layer
            .as_object()
            .and_then(|l| l.get(group))
            .and_then(Value::as_object)
            .and_then(|g| g.get(key));
        match found {
            Some(Value::Array(items)) => items.iter().for_each(&mut add),
            // flatMap keeps a value that is not an array as one entry ("allow": "docs/**").
            Some(other) if js::truthy(other) => add(other),
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
pub fn set_config_value(
    scope: Scope,
    root: &Path,
    key: &str,
    value: Value,
) -> Result<PathBuf, DfwError> {
    let file = config_file_for(scope, root);
    let mut data = read_layer(&file)?;
    set_path(&mut data, key, value)?;
    // A project layer is a file of the project: a link must not send the write out of it. The
    // global file may be a link the user made on purpose (a dotfiles folder).
    if scope == Scope::Global {
        write_json(&file, &data)?;
    } else {
        write_json_inside(root, &file, &data)?;
    }
    Ok(file)
}

/// `allowUnverifiedEngine(globalFile)`: the opt-in to run an Impeccable engine dfw cannot
/// checksum. Read from the global config only: a project must not grant it. Only `true` opts in.
pub fn allow_unverified_engine(global_file: &Path) -> Result<bool, DfwError> {
    let data = read_json(global_file, Some(Value::Object(Map::new())))?;
    Ok(get_path(&data, "impeccable.allowUnverifiedEngine") == Some(&Value::Bool(true)))
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
    items
        .map(|list| {
            list.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

/// A positive number at `path`, else `None` (the `value || default` of the Node callers).
fn positive(config: &Value, path: &str) -> Option<f64> {
    get_path(config, path)
        .and_then(Value::as_f64)
        .filter(|n| *n >= 1.0)
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
    let port = match get_path(config, key::STUDIO_PORT) {
        Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
        Some(other) => other.as_f64(),
        None => None,
    };
    port.filter(|p| p.fract() == 0.0 && (1.0..=65535.0).contains(p))
        .map_or(4600, |p| p as u16)
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
        let size = |key: &str| {
            value
                .get(key)
                .and_then(Value::as_f64)
                .filter(|n| *n >= 1.0)
                .map(|n| n as u32)
        };
        Some(Breakpoint {
            id: value.get(key::breakpoint::ID)?.as_str()?.to_string(),
            label: value
                .get(key::breakpoint::LABEL)
                .and_then(Value::as_str)
                .map(str::to_string),
            width: size(key::breakpoint::WIDTH)?,
            height: size(key::breakpoint::HEIGHT)?,
        })
    }
}

/// `studio.breakpoints`; entries that are not breakpoints are left out.
pub fn studio_breakpoints(config: &Value) -> Vec<Breakpoint> {
    let items = get_path(config, key::STUDIO_BREAKPOINTS).and_then(Value::as_array);
    items
        .map(|list| list.iter().filter_map(Breakpoint::from_value).collect())
        .unwrap_or_default()
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
    get_path(config, key::REVIEW_SHORTCUT)
        .filter(|v| !v.is_null())
        .map(js::to_string)
}

/// `env`: variables applied to every external tool dfw launches, in file order.
pub fn env_map(config: &Value) -> Vec<(String, String)> {
    let Some(Value::Object(env)) = config.get(key::ENV) else {
        return Vec::new();
    };
    env.iter()
        .filter(|(_, v)| !v.is_object() && !v.is_array())
        .map(|(k, v)| (k.clone(), js::to_string(v)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{NOTES_FILE, REVIEW_SESSION};
    use crate::core::util::{test_dir, to_json_compact, PROTO};

    /// A global config file a test never writes.
    const NO_GLOBAL: &str = "none.json";
    use std::fs;

    /// A project folder with the given `.dfw/config.json` and `.dfw/config.local.json`.
    fn project(label: &str, config: Option<&str>, local: Option<&str>) -> PathBuf {
        let root = test_dir(label);
        fs::create_dir_all(root.join(rel::DFW)).unwrap();
        for (name, text) in [(file::CONFIG, config), (file::CONFIG_LOCAL, local)] {
            if let Some(text) = text {
                fs::write(root.join(rel::DFW).join(name), text).unwrap();
            }
        }
        root
    }

    fn load(root: &Path) -> Result<Value, DfwError> {
        load_config_from(&root.join("no-global.json"), Some(root), &mut |_| {})
    }

    /// external tools run with telemetry and update checks off (tests/unit/dfw.test.mjs; the
    /// launcher assertions of that case test the overlays, not this module)
    #[test]
    fn external_tools_run_with_telemetry_and_update_checks_off() {
        let d = defaults();
        let env = &d[key::ENV];
        assert_eq!(env["DO_NOT_TRACK"], "1");
        assert_eq!(env["IMPECCABLE_NO_UPDATE_CHECK"], "1");
        assert_eq!(env["IMPECCABLE_HOOK_DISABLED"], "1");
    }

    /// config: gate.allow is a union across layers (tests/unit/dfw.test.mjs)
    #[test]
    fn gate_allow_is_a_union_across_layers() {
        let root = project(
            "config-union",
            Some(r#"{"gate":{"allow":["docs/**"]},"checks":{"onEdit":false}}"#),
            None,
        );
        let c = load(&root).unwrap();
        assert!(gate_allow(&c).contains(&"docs/**".to_string()));
        assert!(
            gate_allow(&c).contains(&all_under(rel::DFW)),
            "defaults kept"
        );
        let checks = &c[key::CHECKS];
        assert_eq!(checks["onEdit"], false);
        const SLOPLINT: &str = "sloplint";
        assert_eq!(checks[SLOPLINT], defaults()[key::CHECKS][SLOPLINT]);
        fs::remove_dir_all(&root).unwrap();
    }

    /// RV-2: the files that hold gates and approvals stay immutable whatever a project config
    /// says, so the agent's Write tool cannot open the gate by editing them.
    #[test]
    fn a_project_config_cannot_unfreeze_the_files_of_the_gates() {
        use crate::workflow::gates::decide_write;
        use crate::workflow::state::new_state;
        let root = project(
            "config-owned",
            Some(r#"{"gate":{"allow":["**"],"immutable":[]}}"#),
            Some(r#"{"gate":{"immutable":null}}"#),
        );
        let c = load(&root).unwrap();
        let state = new_state("new-product", "".into()).unwrap();
        for gate_file in rel::GATE_STATE {
            let file = if rel::GATE_STATE_DIRS.contains(&gate_file) {
                rel::join(gate_file, REVIEW_SESSION)
            } else {
                gate_file.to_string()
            };
            let d = decide_write(&root, Some(&state), &c, &root.join(&file)).unwrap();
            assert!(!d.allow, "{file}");
            assert!(
                d.reason.unwrap().contains("only dfw commands change it"),
                "{file}"
            );
        }
        let notes = decide_write(
            &root,
            Some(&state),
            &c,
            &root.join(rel::join(rel::DFW, NOTES_FILE)),
        )
        .unwrap();
        assert!(notes.allow, "the rest of {}/ stays writable", rel::DFW);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn without_files_the_config_is_the_defaults_in_node_key_order() {
        let tmp = test_dir("config-none");
        let expected = to_json_compact(&defaults());
        assert_eq!(
            to_json_compact(&load_config_from(&tmp.join(NO_GLOBAL), None, &mut |_| {}).unwrap()),
            expected
        );
        assert_eq!(
            to_json_compact(
                &load_config_from(&tmp.join(NO_GLOBAL), Some(Path::new("")), &mut |_| {}).unwrap()
            ),
            expected,
            "an empty root is no root"
        );
        assert_eq!(to_json_compact(&load(&tmp).unwrap()), expected);
        assert!(expected.starts_with(r#"{"schema":1,"language":{"askPolicy":"session","default":"en"},"gate":{"allow":[".dfw/**","design/handoff/**"],"designPaths":["#));
        assert!(expected.ends_with(r#""skills":{"optional":{"hallmark":true,"taste-reference":true},"disabled":[]},"env":{"DO_NOT_TRACK":"1","IMPECCABLE_NO_UPDATE_CHECK":"1","IMPECCABLE_HOOK_DISABLED":"1"}}"#));
        fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn layers_merge_in_order_and_lists_are_unions() {
        /// A section only the project layers write.
        const EXTRA: &str = "extra";
        let root = project(
            "config-layers",
            Some(
                r#"{"gate":{"allow":["docs/**",".dfw/**"],"immutable":["vendor/**"],"designPaths":["ui/**"]},"studio":{"port":4700},"extra":{"a":1}}"#,
            ),
            Some(
                r#"{"gate":{"allow":["notes/**","docs/**"]},"studio":{"port":4800},"skills":{"disabled":["hallmark"]},"extra":{"b":2}}"#,
            ),
        );
        let global = root.join("global.json");
        fs::write(&global, r#"{"gate":{"allow":["global/**"]},"studio":{"port":4650},"language":{"default":"vi"}}"#).unwrap();
        let c = load_config_from(&global, Some(&root), &mut |_| {}).unwrap();
        assert_eq!(
            gate_allow(&c),
            [
                &all_under(rel::DFW),
                &all_under(rel::HANDOFF),
                "global/**",
                "docs/**",
                "notes/**"
            ]
        );
        let immutable = gate_immutable(&c);
        assert_eq!(
            immutable.first().map(String::as_str),
            Some(all_under(rel::APPROVED).as_str())
        );
        assert_eq!(immutable.last().map(String::as_str), Some("vendor/**"));
        assert_eq!(gate_design_paths(&c), ["ui/**"], "other lists replace");
        assert_eq!(studio_port(&c), 4800, "local wins");
        assert_eq!(
            c[key::LANGUAGE],
            json!({"askPolicy": "session", "default": "vi"})
        );
        assert_eq!(c[key::SKILLS]["disabled"], json!(["hallmark"]));
        assert_eq!(to_json_compact(&c[EXTRA]), r#"{"a":1,"b":2}"#);
        let keys: Vec<&String> = c.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "schema",
                key::LANGUAGE,
                key::GATE,
                key::CHECKS,
                "studio",
                "review",
                "accessibility",
                key::SKILLS,
                key::ENV,
                EXTRA
            ]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn odd_values_follow_the_javascript_rules() {
        // A scalar union entry is one entry; numbers dedupe by value, objects never; a spread of
        // a string or null replaces what the layer wrote; `__proto__` never becomes a key.
        let root = project(
            "config-odd",
            Some(
                r#"{"gate":{"allow":"docs/**","immutable":[1,{"a":1},null]},"skills":"ab","__proto__":{"x":1},"checks":{"__proto__":{"y":1}}}"#,
            ),
            Some(r#"{"gate":{"allow":0,"immutable":[1.0,{"a":1},null]},"studio":null}"#),
        );
        let c = load(&root).unwrap();
        let gate = &c[key::GATE];
        assert_eq!(
            gate["allow"],
            json!([all_under(rel::DFW), all_under(rel::HANDOFF), "docs/**"])
        );
        assert_eq!(
            to_json_compact(&gate["immutable"]),
            r#"["design/approved/**",".dfw/state.json",".dfw/config.json",".dfw/config.local.json",".dfw/decisions.jsonl",".dfw/stack.json",".dfw/reviews/**",1,{"a":1},null,{"a":1}]"#
        );
        assert_eq!(
            to_json_compact(&c["skills"]),
            r#"{"0":"a","1":"b","disabled":[]}"#
        );
        assert_eq!(c["studio"], Value::Null);
        assert!(c.get(PROTO).is_none() && c[key::CHECKS].get(PROTO).is_none());
        assert_eq!(studio_port(&c), 4600);
        assert!(studio_breakpoints(&c).is_empty());
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_broken_file_is_an_error() {
        let root = project("config-broken", Some("{ nope"), None);
        let err = load_config_from(&root.join(NO_GLOBAL), Some(&root), &mut |_| {}).unwrap_err();
        assert!(err.message().starts_with("Invalid JSON in "), "{err}");
        fs::remove_dir_all(&root).unwrap();
    }

    /// k: a config file that is not a JSON object is a clear error (tests/unit/node-stable.test.mjs)
    #[test]
    fn a_config_file_that_is_not_a_json_object_is_a_clear_error() {
        let root = project("config-not-object", None, None);
        // The path the loader names (`\\` on Windows), not `rel::CONFIG` joined (`/`).
        let file = project_paths(&root).config;
        for content in ["[1,2]", "\"text\"", "5", "null", "true"] {
            fs::write(&file, content).unwrap();
            let err = load(&root).unwrap_err();
            assert_eq!(err.exit_code(), 1);
            assert_eq!(
                err.report(),
                format!(
                    "Config file {} must hold a JSON object. Fix it or delete it.",
                    file.display()
                )
            );
            let err = set_config_value(Scope::Project, &root, "x", json!(1)).unwrap_err();
            assert!(err.message().contains("must hold a JSON object"));
            assert_eq!(
                fs::read_to_string(&file).unwrap(),
                content,
                "left as it was"
            );
        }
        fs::write(&file, "{}").unwrap();
        let global = root.join("home-config.json");
        fs::write(&global, "[]").unwrap();
        let err = load_config_from(&global, Some(&root), &mut |_| {}).unwrap_err();
        assert!(err
            .message()
            .contains("home-config.json must hold a JSON object"));
        fs::remove_dir_all(&root).unwrap();
    }

    /// j: names that are prototype keys are not ids (tests/unit/node-stable.test.mjs; the config part)
    #[test]
    fn reserved_keys_in_a_config_file_are_dropped_and_not_set() {
        let root = project(
            "config-reserved",
            None,
            Some(
                r#"{"__proto__":{"polluted":"yes"},"constructor":{"x":1},"studio":{"__proto__":{"port":1}}}"#,
            ),
        );
        let c = load(&root).unwrap();
        assert!(c.get("polluted").is_none() && c.get("constructor").is_none());
        assert_eq!(c["studio"]["port"], 4600);
        for key in [
            "toString",
            "studio.breakpoints.length",
            "studio.constructor",
        ] {
            assert_eq!(get_path(&c, key), None, "{key}");
        }
        assert_eq!(
            get_path(&c, "studio.breakpoints.0.id"),
            Some(&json!("desktop"))
        );
        for key in ["__proto__.x", "a.constructor", "a.prototype.b"] {
            let err = set_config_value(Scope::Project, &root, key, json!(1)).unwrap_err();
            assert!(
                err.message()
                    .starts_with(&format!("Invalid key \"{key}\": \"")),
                "{err}"
            );
            assert!(err.message().ends_with("\" is a reserved name."));
        }
        assert!(!root.join(rel::CONFIG).exists());
        fs::remove_dir_all(&root).unwrap();
    }

    /// n: config set refuses a key that goes through a list (tests/unit/node-stable.test.mjs)
    #[test]
    fn config_set_refuses_a_key_that_goes_through_a_list() {
        let root = project("config-list", None, None);
        set_config_value(Scope::Project, &root, "gate.allow", json!(["src/**"])).unwrap();
        let err = set_config_value(Scope::Project, &root, "gate.allow.0.b", json!(5)).unwrap_err();
        assert_eq!(
            err.report(),
            "Cannot set \"gate.allow.0.b\": \"gate.allow\" is a list; set the whole list instead."
        );
        let err =
            set_config_value(Scope::Project, &root, "gate.allow.extra", json!(5)).unwrap_err();
        assert!(err.message().contains("\"gate.allow\" is a list"));
        assert_eq!(
            fs::read_to_string(root.join(rel::CONFIG)).unwrap(),
            "{\n  \"gate\": {\n    \"allow\": [\n      \"src/**\"\n    ]\n  }\n}\n"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// config: project layers cannot set engine or loader variables, or loosen the privacy
    /// defaults (tests/unit/impeccable-hardening.test.mjs)
    #[test]
    fn project_layers_cannot_set_engine_or_loader_variables() {
        let root = project(
            "config-env",
            Some(
                r#"{"env":{"IMPECCABLE_BIN":"/repo/evil","IMPECCABLE_API_URL":"http://evil.example","IMPECCABLE_UPDATE_HOST":"evil.example","IMPECCABLE_HOME":"/repo/cache","DO_NOT_TRACK":"0","IMPECCABLE_NO_UPDATE_CHECK":null,"IMPECCABLE_NO_TELEMETRY":"0","PATH":"/repo/bin","NODE_OPTIONS":"--require /repo/x.js","LD_PRELOAD":"/repo/x.so","DYLD_INSERT_LIBRARIES":"/repo/x.dylib","NO_COLOR":"1"}}"#,
            ),
            Some(r#"{"env":{"IMPECCABLE_HOOK_DISABLED":"0","IMPECCABLE_NO_TELEMETRY":"1"}}"#),
        );
        let global = root.join("home-config.json");
        fs::write(&global, "{}").unwrap();
        let mut warnings = Vec::new();
        let c = load_config_from(&global, Some(&root), &mut |m| warnings.push(m)).unwrap();
        assert_eq!(
            to_json_compact(&c["env"]),
            r#"{"DO_NOT_TRACK":"1","IMPECCABLE_NO_UPDATE_CHECK":"1","IMPECCABLE_HOOK_DISABLED":"1","NO_COLOR":"1","IMPECCABLE_NO_TELEMETRY":"1"}"#
        );
        let project_file = project_paths(&root).config.display().to_string();
        let local_file = project_paths(&root).config_local.display().to_string();
        let dropped = [
            "IMPECCABLE_BIN",
            "IMPECCABLE_API_URL",
            "IMPECCABLE_UPDATE_HOST",
            "IMPECCABLE_HOME",
            "DO_NOT_TRACK",
            "IMPECCABLE_NO_UPDATE_CHECK",
            "IMPECCABLE_NO_TELEMETRY",
            "PATH",
            "NODE_OPTIONS",
            "LD_PRELOAD",
            "DYLD_INSERT_LIBRARIES",
        ];
        for k in dropped {
            let n = warnings
                .iter()
                .filter(|w| w.contains(&format!("env.{k} ")) && w.contains(&project_file))
                .count();
            assert_eq!(n, 1, "one warning for {k} naming the file");
        }
        assert_eq!(
            warnings
                .iter()
                .filter(|w| w.contains("env.IMPECCABLE_HOOK_DISABLED ") && w.contains(&local_file))
                .count(),
            1
        );
        assert_eq!(
            warnings.len(),
            dropped.len() + 1,
            "allowed keys do not warn"
        );
        assert_eq!(warnings[0], format!("dfw: ignored env.IMPECCABLE_BIN in {project_file} (a project config cannot set it; use the global config: dfw config set env.IMPECCABLE_BIN <value> --global)"));
        fs::remove_dir_all(&root).unwrap();
    }

    /// A project layer cannot set a global-only section or a key that names what runs or where
    /// it connects, at any depth; the global config can (`security-privacy.md` S10).
    #[test]
    fn project_layers_cannot_set_global_only_keys() {
        let project_text = r#"{"telemetry":{"endpoint":"http://evil.example"},"penpot":{"origins":["http://evil.example"]},"impeccable":{"allowUnverifiedEngine":true},"marketplaceRepo":"/repo","checks":{"impeccableBin":"/repo/x","tool_url":"x","maxFindings":3},"review":{"command":"x","shortcut":"off"},"studio":{"url":"x","port":4700}}"#;
        let root = project(
            "config-global-only",
            Some(project_text),
            Some(r#"{"x":{"y":{"bin":"/repo/b"}}}"#),
        );
        let global = root.join("home-config.json");
        /// The address the global config names.
        const TRUSTED: &str = "https://ok.example";
        fs::write(&global, r#"{"telemetry":{"endpoint":"https://ok.example"},"penpot":{"origins":["https://ok.example"]},"checks":{"impeccableBin":"/usr/bin/x"}}"#).unwrap();
        let mut warnings = Vec::new();
        let c = load_config_from(&global, Some(&root), &mut |m| warnings.push(m)).unwrap();
        assert_eq!(c[key::TELEMETRY], json!({ "endpoint": TRUSTED }));
        assert_eq!(c[key::PENPOT], json!({ "origins": [TRUSTED] }));
        assert_eq!(c.get(key::IMPECCABLE), None);
        assert_eq!(c.get(key::MARKETPLACE_REPO), None);
        assert_eq!(
            c[key::CHECKS]["impeccableBin"],
            "/usr/bin/x",
            "the global value stays"
        );
        assert_eq!(c[key::CHECKS].get("tool_url"), None);
        assert_eq!(
            c[key::CHECKS][key::MAX_FINDINGS],
            3,
            "other keys of the section stay"
        );
        assert_eq!(c[key::REVIEW].get("command"), None);
        assert_eq!(c[key::REVIEW][key::SHORTCUT], "off");
        assert_eq!(c[key::STUDIO].get("url"), None);
        assert_eq!(c[key::STUDIO][key::PORT], 4700);
        assert_eq!(c["x"]["y"], json!({}));
        let project_file = project_paths(&root).config.display().to_string();
        let local_file = project_paths(&root).config_local.display().to_string();
        let dropped = [
            "telemetry ",
            "penpot ",
            "impeccable ",
            "marketplaceRepo ",
            "checks.impeccableBin ",
            "checks.tool_url ",
            "review.command ",
            "studio.url ",
        ];
        for k in dropped {
            let n = warnings
                .iter()
                .filter(|w| w.contains(&format!("ignored {k}")) && w.contains(&project_file))
                .count();
            assert_eq!(n, 1, "one warning for {k}naming the file: {warnings:?}");
        }
        assert!(warnings
            .iter()
            .any(|w| w.contains("ignored x.y.bin ") && w.contains(&local_file)));
        assert_eq!(warnings.len(), dropped.len() + 1);
        assert_eq!(warnings[0], format!("dfw: ignored telemetry in {project_file} (a project config cannot set it; use the global config: dfw config set telemetry.<key> <value> --global)"));
        for name in [
            "shortcut",
            "maxFindings",
            "breakpoints",
            "debug",
            "Url2",
            "curl",
            "cabin",
        ] {
            assert!(!names_what_runs(name), "{name}");
        }
        fs::remove_dir_all(&root).unwrap();
    }

    /// config: a project "env": null cannot erase the defaults; the global config may set
    /// anything (tests/unit/impeccable-hardening.test.mjs)
    #[test]
    fn a_project_env_null_cannot_erase_the_defaults() {
        let root = project("config-env-null", Some(r#"{"env":null}"#), None);
        let global = root.join("home-config.json");
        fs::write(
            &global,
            r#"{"env":{"DO_NOT_TRACK":"0","IMPECCABLE_API_URL":"http://mirror.internal"}}"#,
        )
        .unwrap();
        let mut warnings = Vec::new();
        let c = load_config_from(&global, Some(&root), &mut |m| warnings.push(m)).unwrap();
        let env = &c[key::ENV];
        assert_eq!(env["IMPECCABLE_NO_UPDATE_CHECK"], "1");
        assert_eq!(
            env["DO_NOT_TRACK"], "0",
            "the user may change a default in the global config"
        );
        assert_eq!(env["IMPECCABLE_API_URL"], "http://mirror.internal");
        assert_eq!(
            warnings,
            [format!(
                "dfw: ignored env in {} (it must be an object of strings)",
                project_paths(&root).config.display()
            )]
        );
        fs::remove_dir_all(&root).unwrap();
    }

    /// config: the unverified-engine opt-in is read from the global file only
    /// (tests/unit/impeccable-hardening.test.mjs)
    #[test]
    fn the_unverified_engine_opt_in_is_read_from_the_global_file_only() {
        let root = project(
            "config-opt-in",
            Some(r#"{"impeccable":{"allowUnverifiedEngine":true}}"#),
            None,
        );
        let global = root.join("home-config.json");
        fs::write(&global, "{}").unwrap();
        assert!(!allow_unverified_engine(&global).unwrap());
        fs::write(&global, r#"{"impeccable":{"allowUnverifiedEngine":true}}"#).unwrap();
        assert!(allow_unverified_engine(&global).unwrap());
        fs::write(
            &global,
            r#"{"impeccable":{"allowUnverifiedEngine":"true"}}"#,
        )
        .unwrap();
        assert!(
            !allow_unverified_engine(&global).unwrap(),
            "only the boolean true opts in"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn typed_accessors_apply_the_fallbacks_of_the_node_callers() {
        let d = defaults();
        assert_eq!(studio_port(&d), 4600);
        let bps = studio_breakpoints(&d);
        assert_eq!(bps.len(), 3);
        assert_eq!(
            bps[1],
            Breakpoint {
                id: "tablet".into(),
                label: Some("Tablet".into()),
                width: 834,
                height: 1112
            }
        );
        assert_eq!(checks_on_edit(&d), OnEdit::Fast);
        assert_eq!(checks_timeout_ms(&d), 8000);
        assert_eq!(checks_max_findings(&d), 12);
        assert!(checks_layout_on_stop(&d));
        assert_eq!(review_shortcut(&d).as_deref(), Some("alt+c"));
        assert_eq!(
            env_map(&d),
            [
                ("DO_NOT_TRACK".to_string(), "1".to_string()),
                ("IMPECCABLE_NO_UPDATE_CHECK".into(), "1".into()),
                ("IMPECCABLE_HOOK_DISABLED".into(), "1".into())
            ]
        );

        /// The one breakpoint with a size.
        const WIDE: &str = "wide";
        let c = json!({
            "checks": { "onEdit": true, "timeoutMs": 0, "maxFindings": "many", "layoutOnStop": false },
            "studio": { "port": "4700", "breakpoints": [{ "id": WIDE, "width": 1920, "height": 1080 }, "junk", { "id": "no-size" }] },
            "review": { "shortcut": false },
            "env": { "A": 1, "B": true, "C": null, "D": {} },
        });
        assert_eq!(checks_on_edit(&c), OnEdit::Full);
        assert_eq!(
            checks_on_edit(&json!({"checks": {"onEdit": "full"}})),
            OnEdit::Full
        );
        assert_eq!(
            checks_on_edit(&json!({"checks": {"onEdit": false}})),
            OnEdit::Off
        );
        assert_eq!(
            checks_on_edit(&json!({"checks": {"onEdit": null}})),
            OnEdit::Fast
        );
        assert_eq!(checks_timeout_ms(&c), 8000);
        assert_eq!(checks_max_findings(&c), 12);
        assert!(!checks_layout_on_stop(&c));
        assert_eq!(studio_port(&c), 4700);
        assert_eq!(studio_port(&json!({"studio": {"port": 99999}})), 4600);
        assert_eq!(
            studio_breakpoints(&c),
            [Breakpoint {
                id: WIDE.into(),
                label: None,
                width: 1920,
                height: 1080
            }]
        );
        assert_eq!(review_shortcut(&c).as_deref(), Some("false"));
        assert_eq!(
            review_shortcut(&json!({"review": {"shortcut": null}})),
            None
        );
        assert_eq!(review_shortcut(&json!({})), None);
        assert_eq!(
            env_map(&c),
            [
                ("A".to_string(), "1".to_string()),
                ("B".into(), "true".into()),
                ("C".into(), "null".into())
            ]
        );
        assert!(gate_allow(&json!({"gate": {"allow": "x"}})).is_empty());
    }

    #[test]
    fn set_and_get_a_config_value() {
        let root = project("config-set", Some(r#"{"schema":1}"#), None);
        assert_eq!(
            config_file_for(Scope::Local, &root),
            root.join(rel::CONFIG_LOCAL)
        );
        let file = set_config_value(Scope::Project, &root, key::STUDIO_PORT, json!(4777)).unwrap();
        assert_eq!(file, root.join(rel::CONFIG));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n  \"schema\": 1,\n  \"studio\": {\n    \"port\": 4777\n  }\n}\n"
        );
        const OFF: &str = "off";
        set_config_value(Scope::Local, &root, key::REVIEW_SHORTCUT, json!(OFF)).unwrap();
        let c = load(&root).unwrap();
        assert_eq!(get_path(&c, key::STUDIO_PORT), Some(&json!(4777)));
        assert_eq!(get_path(&c, key::REVIEW_SHORTCUT), Some(&json!(OFF)));
        assert_eq!(get_path(&c, "studio.nope"), None);
        fs::remove_dir_all(&root).unwrap();
    }

    /// `config set --local` through `.dfw/config.local.json -> ../../victim/settings.json`
    /// writes nothing outside the project (review CO-3); `--project` likewise.
    #[cfg(unix)]
    #[test]
    fn a_project_layer_linked_out_of_the_project_is_refused() {
        let base = test_dir("config-link-out");
        let root = base.join("work").join("app");
        let victim = base.join("victim");
        fs::create_dir_all(root.join(rel::DFW)).unwrap();
        fs::create_dir_all(&victim).unwrap();
        /// The user-global file a link points at.
        const SETTINGS: &str = "settings.json";
        fs::write(victim.join(SETTINGS), "{}").unwrap();
        for name in [file::CONFIG_LOCAL, file::CONFIG] {
            std::os::unix::fs::symlink(
                "../../../victim/settings.json",
                root.join(rel::DFW).join(name),
            )
            .unwrap();
        }
        for scope in [Scope::Local, Scope::Project] {
            let err = set_config_value(scope, &root, key::STUDIO_PORT, json!(1)).unwrap_err();
            assert!(
                err.message().contains("outside the project"),
                "{}",
                err.message()
            );
        }
        assert_eq!(fs::read_to_string(victim.join(SETTINGS)).unwrap(), "{}");
        fs::remove_dir_all(&base).unwrap();
    }
}
