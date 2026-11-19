//! Ports `core/lib/binary.mjs`: the single-file dfw binary, built per platform by CI and
//! attached to the GitHub release v<version>. "dfw setup --binary" downloads the one matching
//! this plugin version and verifies it against the release's SHA256SUMS; bin/dfw then prefers it.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::paths::global_dir;
use crate::core::platform::{dfw_version, node_arch, node_platform};
use crate::core::util::sha256;

/// The GitHub repository whose releases carry the binaries (also the default marketplace source).
pub const RELEASE_REPO: &str = "tantm97/ai-frontend-design-workflow";

/// `assetName(version, platform, arch)`: release asset name, e.g. dfw-0.4.0-darwin-arm64 /
/// dfw-0.4.0-win32-x64.exe.
pub fn asset_name(version: &str, platform: &str, arch: &str) -> String {
    let exe = if platform == "win32" { ".exe" } else { "" };
    format!("dfw-{version}-{platform}-{arch}{exe}")
}

/// `binaryPath(version)`: where the binary of a version is installed.
pub fn binary_path(version: &str) -> PathBuf {
    let exe = if cfg!(windows) { "dfw.exe" } else { "dfw" };
    global_dir().join("bin").join(version).join(exe)
}

/// `binaryStatus()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryStatus {
    /// True inside the dfw binary: always, here.
    pub running: bool,
    pub installed: bool,
    pub path: PathBuf,
    pub version: String,
}

/// Whether the binary of this plugin version is installed, and where it would be.
pub fn binary_status() -> BinaryStatus {
    let version = dfw_version();
    let path = binary_path(&version);
    BinaryStatus {
        running: true,
        installed: path.exists(),
        path,
        version,
    }
}

/// `get(url)`: the body of a URL, following redirects; `HTTP <status> for <url>` otherwise.
fn get(url: &str) -> Result<Vec<u8>, DfwError> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(120)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut res = agent
        .get(url)
        .call()
        .map_err(|e| DfwError::internal(format!("{url}: {e}")))?;
    if !res.status().is_success() {
        return Err(DfwError::new(format!(
            "HTTP {} for {url}",
            res.status().as_u16()
        )));
    }
    res.body_mut()
        .with_config()
        .limit(1 << 30)
        .read_to_vec()
        .map_err(|e| DfwError::internal(format!("{url}: {e}")))
}

/// The checksum SHA256SUMS lists for `name` (`<hash>  <file>` or `<hash> *<file>` per line).
fn wanted_sum(sums: &str, name: &str) -> Option<String> {
    // l.trim().split(/\s+\*?/)
    let split = Regex::new(r"\s+\*?").ok()?;
    sums.split('\n').find_map(|line| {
        let mut fields = split.split(line.trim());
        let (hash, file) = (fields.next()?, fields.next()?);
        (file == name).then(|| hash.to_string())
    })
}

/// `installBinary({ version, log })`: download and verify the binary for this plugin version
/// into `~/.config/dfw/bin/<version>/`. Returns `{ installed, path, version, sha256 }`.
pub fn install_binary(log: &dyn Fn(&str)) -> Result<Value, DfwError> {
    let version = dfw_version();
    let base = format!("https://github.com/{RELEASE_REPO}/releases/download/v{version}");
    install_binary_from(&base, &version, log)
}

/// [`install_binary`] from a given release folder URL.
fn install_binary_from(base: &str, version: &str, log: &dyn Fn(&str)) -> Result<Value, DfwError> {
    let name = asset_name(version, node_platform(), node_arch());
    log(&format!(
        "Downloading {name} from github.com/{RELEASE_REPO} (release v{version}) ..."
    ));
    let sums = get(&format!("{base}/SHA256SUMS")).map_err(|e| {
        DfwError::new(format!(
            "No dfw binary release for v{version} yet ({e}). dfw keeps running on Node."
        ))
    })?;
    let want = wanted_sum(&String::from_utf8_lossy(&sums), &name).ok_or_else(|| {
        DfwError::new(format!(
            "Release v{version} has no binary for {}-{}. dfw keeps running on Node.",
            node_platform(),
            node_arch()
        ))
    })?;
    let buf = get(&format!("{base}/{name}"))?;
    let got = sha256(&buf);
    if got != want {
        return Err(DfwError::new(format!(
            "Checksum mismatch for {name} (expected {want}, got {got}); not installed."
        )));
    }
    let dest = binary_path(version);
    if let Some(dir) = dest.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = dest.clone().into_os_string();
    tmp.push(".download");
    fs::write(&tmp, &buf)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&tmp, &dest)?;
    Ok(
        json!({ "installed": true, "path": dest.display().to_string(), "version": version, "sha256": got }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::serve_routes as serve;
    use std::collections::HashMap;

    #[test]
    fn asset_names_and_paths() {
        assert_eq!(
            asset_name("0.4.0", "darwin", "arm64"),
            "dfw-0.4.0-darwin-arm64"
        );
        assert_eq!(
            asset_name("0.4.0", "win32", "x64"),
            "dfw-0.4.0-win32-x64.exe"
        );
        assert!(["darwin", "linux", "win32"].contains(&node_platform()));
        assert!(["arm64", "x64", "ia32"].contains(&node_arch()));
        let p = binary_path("9.9.9");
        assert!(
            p.ends_with(PathBuf::from("bin").join("9.9.9").join(if cfg!(windows) {
                "dfw.exe"
            } else {
                "dfw"
            }))
        );
    }

    #[test]
    fn checksum_lines_in_both_formats() {
        let sums = "aaa  dfw-1.0.0-linux-x64\nbbb *dfw-1.0.0-darwin-arm64\n\n  ccc\tdfw-1.0.0-win32-x64.exe  \nbroken\n";
        assert_eq!(
            wanted_sum(sums, "dfw-1.0.0-linux-x64").as_deref(),
            Some("aaa")
        );
        assert_eq!(
            wanted_sum(sums, "dfw-1.0.0-darwin-arm64").as_deref(),
            Some("bbb")
        );
        assert_eq!(
            wanted_sum(sums, "dfw-1.0.0-win32-x64.exe").as_deref(),
            Some("ccc")
        );
        assert_eq!(wanted_sum(sums, "dfw-1.0.0-linux-arm64"), None);
    }

    /// The download against a local server: wrong checksum, missing asset, missing release.
    /// The good path writes under the global dir, so it is covered by the golden test
    /// (its own DFW_HOME), not here.
    #[test]
    fn download_is_verified_before_anything_is_installed() {
        let name = asset_name("7.7.7", node_platform(), node_arch());
        let (base, _) = serve(HashMap::from([
            (
                "/bad/SHA256SUMS".to_string(),
                (200, format!("{}  {name}\n", "0".repeat(64)).into_bytes()),
            ),
            (format!("/bad/{name}"), (200, b"binary".to_vec())),
            (
                "/other/SHA256SUMS".to_string(),
                (200, b"abc  dfw-7.7.7-plan9-mips\n".to_vec()),
            ),
        ]));
        let quiet = |_: &str| {};
        let err = install_binary_from(&format!("{base}/bad"), "7.7.7", &quiet).unwrap_err();
        assert_eq!(
            err.message(),
            format!(
                "Checksum mismatch for {name} (expected {}, got {}); not installed.",
                "0".repeat(64),
                sha256(b"binary")
            )
        );
        assert!(!binary_path("7.7.7").exists());
        let err = install_binary_from(&format!("{base}/other"), "7.7.7", &quiet).unwrap_err();
        assert_eq!(
            err.message(),
            format!(
                "Release v7.7.7 has no binary for {}-{}. dfw keeps running on Node.",
                node_platform(),
                node_arch()
            )
        );
        let err = install_binary_from(&format!("{base}/none"), "7.7.7", &quiet).unwrap_err();
        assert_eq!(
            err.message(),
            format!("No dfw binary release for v7.7.7 yet (HTTP 404 for {base}/none/SHA256SUMS). dfw keeps running on Node.")
        );
    }
}
