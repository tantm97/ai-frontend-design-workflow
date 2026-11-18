//! Ports `core/lib/locate.mjs`: locate sibling plugins (Impeccable, Hallmark, Taste reference)
//! and the external tools they ship.

// Its callers are the check runner and `dfw doctor` (tools group): this allow goes when they are ported.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::host::HOSTS;
use crate::core::paths::{
    claude_dir, env_nonempty, global_dir, manifest_path, plugin_root, resolve,
};
use crate::core::util::{read_json, MARKETPLACE_NAME};

/// The most recently changed folder directly under `dir`.
fn newest_dir(dir: &Path) -> Option<PathBuf> {
    let mut subs: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    subs.sort_by_key(|sub| std::cmp::Reverse(sub.0));
    subs.into_iter().next().map(|(_, path)| path)
}

/// `pluginRoot(name)`: root directory of a plugin from the dfw marketplace, installed or in the
/// development repository.
pub fn plugin_root_of(name: &str) -> Result<Option<PathBuf>, DfwError> {
    let host_dirs: Vec<PathBuf> = HOSTS
        .iter()
        .flat_map(|h| h.installed_plugin_dirs())
        .collect();
    plugin_root_in(
        name,
        plugin_root().as_deref(),
        &claude_dir(),
        &host_dirs,
        &global_dir(),
    )
}

fn plugin_root_in(
    name: &str,
    own_root: Option<&Path>,
    claude: &Path,
    host_dirs: &[PathBuf],
    global: &Path,
) -> Result<Option<PathBuf>, DfwError> {
    let is_plugin = |dir: &Path| manifest_path(dir).is_some();
    if let Some(own) = own_root {
        // 1. Sibling in the same marketplace checkout (dev mode, or plugin cache layout).
        let sibling = resolve(own.join("..").join(name));
        if is_plugin(&sibling) {
            return Ok(Some(sibling));
        }
        // 1b. Dev checkout: core/ next to plugins/.
        let dev = resolve(own.join("..").join("plugins").join(name));
        if is_plugin(&dev) {
            return Ok(Some(dev));
        }
    }
    // 2. Plugin cache: <claudeDir>/plugins/cache/<marketplace>/<plugin>/<version>/
    let cache = claude
        .join("plugins")
        .join("cache")
        .join(MARKETPLACE_NAME)
        .join(name);
    if let Some(cached) = newest_dir(&cache).filter(|c| is_plugin(c)) {
        return Ok(Some(cached));
    }
    // 2b. Other hosts' install directories (e.g. ~/.copilot/installed-plugins/dfw-marketplace/<plugin>).
    if let Some(p) = host_dirs
        .iter()
        .map(|d| d.join(name))
        .find(|p| is_plugin(p))
    {
        return Ok(Some(p));
    }
    // 3. Marketplace repo recorded in global config.
    let config = read_json(&global.join("config.json"), Some(json!({})))?;
    if let Some(repo) = config
        .get("marketplaceRepo")
        .and_then(Value::as_str)
        .filter(|r| !r.is_empty())
    {
        let p = Path::new(repo).join("plugins").join(name);
        if is_plugin(&p) {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

/// How to start Impeccable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImpeccableLauncher {
    pub cmd: PathBuf,
    /// The engine is downloaded: the launcher fetches a checksum-verified engine on first use,
    /// and hooks must not trigger that download (it can exceed the hook timeout).
    pub ready: bool,
    /// The engine version the plugin pins; `None` for `IMPECCABLE_BIN`.
    pub version: Option<String>,
}

/// `impeccableLauncher()`: `IMPECCABLE_BIN` when it exists, else the launcher of the installed
/// plugin; `None` when Impeccable is not installed.
pub fn impeccable_launcher() -> Result<Option<ImpeccableLauncher>, DfwError> {
    if let Some(bin) = env_nonempty("IMPECCABLE_BIN").filter(|b| Path::new(b).exists()) {
        return Ok(Some(ImpeccableLauncher {
            cmd: bin.into(),
            ready: true,
            version: None,
        }));
    }
    let Some(root) = plugin_root_of("impeccable")? else {
        return Ok(None);
    };
    let home = env_nonempty("IMPECCABLE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::home_dir().unwrap_or_default().join(".impeccable"));
    Ok(impeccable_in(&root, &home))
}

fn impeccable_in(root: &Path, home: &Path) -> Option<ImpeccableLauncher> {
    let scripts = root.join("skills").join("impeccable").join("scripts");
    let launcher = scripts.join("impeccable");
    if !launcher.exists() {
        return None;
    }
    let version = fs::read_to_string(scripts.join("VERSION"))
        .map(|v| v.trim().to_string())
        .unwrap_or_default();
    let ready = !version.is_empty() && home.join("bin").join(&version).join("impeccable").exists();
    Some(ImpeccableLauncher {
        cmd: launcher,
        ready,
        version: Some(version),
    })
}

fn script_of(plugin: &str, script: &str) -> Result<Option<PathBuf>, DfwError> {
    Ok(plugin_root_of(plugin)?
        .map(|root| {
            root.join("skills")
                .join(plugin)
                .join("scripts")
                .join(script)
        })
        .filter(|p| p.exists()))
}

/// `sloplintScript()`: Hallmark's sloplint checker, when Hallmark is installed.
pub fn sloplint_script() -> Result<Option<PathBuf>, DfwError> {
    script_of("hallmark", "sloplint.mjs")
}

/// `tasteCopyLintScript()`: the copy checker of the Taste reference, when it is installed.
pub fn taste_copy_lint_script() -> Result<Option<PathBuf>, DfwError> {
    script_of("taste-reference", "copy-lint.mjs")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    fn plugin(dir: &Path) {
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(dir.join(".claude-plugin/plugin.json"), "{}").unwrap();
    }

    #[test]
    fn a_plugin_is_found_next_to_this_one_in_the_dev_checkout_in_the_cache_or_in_the_recorded_repo()
    {
        let tmp = test_dir("locate");
        let (claude, global) = (tmp.join("claude"), tmp.join("global"));
        let hosts = [tmp.join("copilot/installed-plugins")];
        let find =
            |own: &Path| plugin_root_in("impeccable", Some(own), &claude, &hosts, &global).unwrap();
        // Nothing installed.
        let own = tmp.join("market/plugins/dfw");
        assert_eq!(find(&own), None);
        assert_eq!(
            plugin_root_in("impeccable", None, &claude, &hosts, &global).unwrap(),
            None
        );
        // 3. The marketplace repo recorded in the global config.
        let repo = tmp.join("repo");
        plugin(&repo.join("plugins/impeccable"));
        fs::create_dir_all(&global).unwrap();
        fs::write(
            global.join("config.json"),
            json!({ "marketplaceRepo": repo }).to_string(),
        )
        .unwrap();
        assert_eq!(find(&own), Some(repo.join("plugins/impeccable")));
        // 2b. Another host's install folder wins over it.
        plugin(&hosts[0].join("impeccable"));
        assert_eq!(find(&own), Some(hosts[0].join("impeccable")));
        // 2. The plugin cache wins over both: the newest version folder.
        let cache = claude
            .join("plugins/cache")
            .join(MARKETPLACE_NAME)
            .join("impeccable");
        plugin(&cache.join("1.0.0"));
        assert_eq!(find(&own), Some(cache.join("1.0.0")));
        // 1b. A dev checkout (core/ next to plugins/) wins over the cache.
        let core = tmp.join("checkout/core");
        plugin(&tmp.join("checkout/plugins/impeccable"));
        assert_eq!(find(&core), Some(tmp.join("checkout/plugins/impeccable")));
        // 1. A sibling wins over everything.
        plugin(&tmp.join("market/plugins/impeccable"));
        assert_eq!(find(&own), Some(tmp.join("market/plugins/impeccable")));
    }

    #[test]
    fn impeccable_is_ready_only_when_its_engine_is_downloaded() {
        let tmp = test_dir("locate-impeccable");
        let (root, home) = (tmp.join("impeccable"), tmp.join("home"));
        assert_eq!(impeccable_in(&root, &home), None);
        let scripts = root.join("skills/impeccable/scripts");
        fs::create_dir_all(&scripts).unwrap();
        fs::write(scripts.join("impeccable"), "#!/bin/sh\n").unwrap();
        let l = impeccable_in(&root, &home).unwrap();
        assert_eq!((l.ready, l.version.as_deref()), (false, Some("")));
        fs::write(scripts.join("VERSION"), "3.2.1\n").unwrap();
        assert!(!impeccable_in(&root, &home).unwrap().ready);
        fs::create_dir_all(home.join("bin/3.2.1")).unwrap();
        fs::write(home.join("bin/3.2.1/impeccable"), "").unwrap();
        let l = impeccable_in(&root, &home).unwrap();
        assert_eq!(l.cmd, scripts.join("impeccable"));
        assert_eq!((l.ready, l.version.as_deref()), (true, Some("3.2.1")));
    }
}
