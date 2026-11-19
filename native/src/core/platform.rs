//! The platform and architecture as Node names them (`process.platform`, `process.arch`):
//! release asset names, browser downloads and messages use these names.

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
