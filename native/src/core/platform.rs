//! What this build is and runs on: the dfw version, and the platform and architecture as Node
//! names them (`process.platform`, `process.arch`), which release asset names, browser downloads
//! and messages use.

use super::paths::{env_nonempty, MANIFEST_VERSION};
use super::util::plugin_manifest;

/// The dfw version: the plugin manifest's version, else `DFW_VERSION`, else the version this
/// binary was built as.
pub fn dfw_version() -> String {
    let manifest = plugin_manifest(None).ok();
    let from_manifest = manifest
        .as_ref()
        .and_then(|m| m.get(MANIFEST_VERSION))
        .and_then(|v| v.as_str())
        .filter(|v| !v.is_empty());
    match from_manifest {
        Some(v) => v.to_string(),
        None => {
            env_nonempty("DFW_VERSION").unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
        }
    }
}

/// Platforms as Node names them (`process.platform`).
pub mod os {
    pub const DARWIN: &str = "darwin";
    pub const LINUX: &str = "linux";
    pub const WIN32: &str = "win32";
}

/// Architectures as Node names them (`process.arch`).
pub mod arch {
    pub const ARM64: &str = "arm64";
    pub const X64: &str = "x64";
    pub const IA32: &str = "ia32";
}

/// `process.platform` as Node names it.
pub fn node_platform() -> &'static str {
    match std::env::consts::OS {
        "macos" => os::DARWIN,
        "windows" => os::WIN32,
        other => other,
    }
}

/// `process.arch` as Node names it.
pub fn node_arch() -> &'static str {
    match std::env::consts::ARCH {
        "aarch64" => arch::ARM64,
        "x86_64" => arch::X64,
        "x86" => arch::IA32,
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_the_ones_node_uses() {
        assert!([os::DARWIN, os::LINUX, os::WIN32].contains(&node_platform()));
        assert!([arch::ARM64, arch::X64, arch::IA32].contains(&node_arch()));
    }
}
