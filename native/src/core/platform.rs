//! What this build is and runs on: the dfw version, and the platform and architecture as Node
//! names them (`process.platform`, `process.arch`), which release asset names, browser downloads
//! and messages use.

use super::paths::env_nonempty;
use super::util::plugin_manifest;

/// The dfw version: the plugin manifest's version, else `DFW_VERSION`, else the version this
/// binary was built as.
pub fn dfw_version() -> String {
    let manifest = plugin_manifest(None).ok();
    let from_manifest = manifest
        .as_ref()
        .and_then(|m| m.get("version"))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty());
    match from_manifest {
        Some(v) => v.to_string(),
        None => {
            env_nonempty("DFW_VERSION").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
        }
    }
}

/// `process.platform` as Node names it.
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => "darwin",
        "windows" => "win32",
        other => other,
    }
}

/// `process.arch` as Node names it.
pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        "x86" => "ia32",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_ones_node_uses() {
        assert!(["darwin", "linux", "win32"].contains(&node_platform()));
        assert!(["arm64", "x64", "ia32"].contains(&node_arch()));
    }
}
