//! The dfw binary of a release: built per platform by CI (cargo) and attached to the GitHub
//! release v<version>. "dfw setup --binary" downloads the one matching this plugin version and
//! verifies it against the release's SHA256SUMS; the plugin launcher (bin/dfw) runs it.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use regex::Regex;
use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::http;
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
/// Two minutes and 256 MB at most.
fn get(url: &str) -> Result<Vec<u8>, DfwError> {
    let res = http::get(url, &[], Duration::from_secs(120), 256 << 20)
        .map_err(|e| DfwError::internal(format!("{url}: {e}")))?;
    if !res.ok() {
        return Err(DfwError::new(format!("HTTP {} for {url}", res.status)));
    }
    Ok(res.body)
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
            "No dfw binary release for v{version} yet ({e}). The installed dfw keeps running."
        ))
    })?;
    let want = wanted_sum(&String::from_utf8_lossy(&sums), &name).ok_or_else(|| {
        DfwError::new(format!(
            "Release v{version} has no binary for {}-{}. The installed dfw keeps running.",
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
    // Its own temporary file: written, made executable, then renamed into place in one step.
    #[allow(
        clippy::disallowed_methods,
        reason = "the binary is chmod-ed before its rename, which write_atomic cannot do"
    )]
    fs::write(&tmp, &buf)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(&tmp, &dest)?;
    Ok(
        json!({ (key::INSTALLED): true, (key::PATH): dest.display().to_string(), (key::VERSION): version, (key::SHA256): got }),
    )
}

/// The keys of what [`install_binary`] returns (`dfw setup --binary` prints it).
pub mod key {
    pub const INSTALLED: &str = "installed";
    /// Where the binary was put.
    pub const PATH: &str = "path";
    pub const VERSION: &str = "version";
    /// The SHA-256 of the binary, as checked against the release's `SHA256SUMS`.
    pub const SHA256: &str = "sha256";
}

/// What the binary installer says.
pub mod message {
    /// `dfw install` removed the Node binaries of these versions.
    pub fn removed_node_binaries(versions: &[String]) -> String {
        format!(
            "Removed the Node binaries of dfw {} (native since 0.6.0)",
            versions.join(", ")
        )
    }
}

/// The first version whose binary is native (Rust). The binaries before it are the Node
/// builds of 0.5.x: no 0.6 plugin runs one, and each is tens of megabytes.
const FIRST_NATIVE: (u64, u64) = (0, 6);

/// Remove the binaries of the Node era (versions before 0.6.0) from `<dfw home>/bin/`, after an
/// install put the native one in place. Only a version folder that holds a dfw binary is
/// removed; every 0.6 or later version stays (another host's plugin may still run it). Returns
/// the versions removed.
pub fn remove_node_binaries() -> Vec<String> {
    remove_node_binaries_in(&global_dir().join("bin"))
}

fn remove_node_binaries_in(bin: &std::path::Path) -> Vec<String> {
    let Ok(entries) = fs::read_dir(bin) else {
        return Vec::new();
    };
    let mut removed: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let version = e.file_name().to_string_lossy().into_owned();
            let mut parts = version.split('.').map(str::parse::<u64>);
            let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
                return None;
            };
            let holds_dfw = ["dfw", "dfw.exe"]
                .iter()
                .any(|exe| e.path().join(exe).is_file());
            ((major, minor) < FIRST_NATIVE && holds_dfw && fs::remove_dir_all(e.path()).is_ok())
                .then_some(version)
        })
        .collect();
    removed.sort();
    removed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::serve_routes as serve;
    use std::collections::HashMap;

    #[test]
    fn asset_names_and_paths() {
        /// A release, and the platforms and architectures its assets are named after (Node's
        /// names).
        const RELEASE: &str = "0.4.0";
        use crate::core::platform::arch::{ARM64, IA32, X64};
        use crate::core::platform::os::{DARWIN, LINUX, WIN32};
        /// A version that is not installed.
        const MISSING: &str = "9.9.9";
        assert_eq!(asset_name(RELEASE, DARWIN, ARM64), "dfw-0.4.0-darwin-arm64");
        assert_eq!(asset_name(RELEASE, WIN32, X64), "dfw-0.4.0-win32-x64.exe");
        assert!([DARWIN, LINUX, WIN32].contains(&node_platform()));
        assert!([ARM64, X64, IA32].contains(&node_arch()));
        let p = binary_path(MISSING);
        assert!(
            p.ends_with(PathBuf::from("bin").join(MISSING).join(if cfg!(windows) {
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
        /// The release the local server offers.
        const RELEASE: &str = "7.7.7";
        /// The binary of the release with the wrong checksum.
        const BINARY: &[u8] = b"binary";
        let name = asset_name(RELEASE, node_platform(), node_arch());
        let (base, _) = serve(HashMap::from([
            (
                "/bad/SHA256SUMS".to_string(),
                (200, format!("{}  {name}\n", "0".repeat(64)).into_bytes()),
            ),
            (format!("/bad/{name}"), (200, BINARY.to_vec())),
            (
                "/other/SHA256SUMS".to_string(),
                (200, b"abc  dfw-7.7.7-plan9-mips\n".to_vec()),
            ),
        ]));
        let quiet = |_: &str| {};
        let err = install_binary_from(&format!("{base}/bad"), RELEASE, &quiet).unwrap_err();
        assert_eq!(
            err.message(),
            format!(
                "Checksum mismatch for {name} (expected {}, got {}); not installed.",
                "0".repeat(64),
                sha256(BINARY)
            )
        );
        assert!(!binary_path(RELEASE).exists());
        let err = install_binary_from(&format!("{base}/other"), RELEASE, &quiet).unwrap_err();
        assert_eq!(
            err.message(),
            format!(
                "Release v7.7.7 has no binary for {}-{}. The installed dfw keeps running.",
                node_platform(),
                node_arch()
            )
        );
        let err = install_binary_from(&format!("{base}/none"), RELEASE, &quiet).unwrap_err();
        assert_eq!(
            err.message(),
            format!("No dfw binary release for v7.7.7 yet (HTTP 404 for {base}/none/SHA256SUMS). The installed dfw keeps running.")
        );
    }

    /// The Node binaries of 0.5.x go; every native version, and any folder that is not a dfw
    /// binary of its own, stays.
    #[test]
    fn node_era_binaries_are_removed_and_native_ones_kept() {
        /// A version of the Node era, two native ones, and folders that are not dfw binaries.
        const NODE: &str = "0.5.2";
        const NATIVE: [&str; 2] = ["0.6.0", "0.10.1"];
        const NO_BINARY: &str = "0.4.0";
        const NOT_A_VERSION: &str = "latest";
        let bin = crate::core::util::test_dir("bin-node");
        for v in [NODE, NATIVE[0], NATIVE[1], NOT_A_VERSION] {
            fs::create_dir_all(bin.join(v)).unwrap();
            fs::write(bin.join(v).join("dfw"), "x").unwrap();
        }
        fs::create_dir_all(bin.join(NO_BINARY)).unwrap();
        assert_eq!(remove_node_binaries_in(&bin), [NODE]);
        for kept in [NATIVE[0], NATIVE[1], NOT_A_VERSION, NO_BINARY] {
            assert!(bin.join(kept).exists(), "{kept}");
        }
        assert!(!bin.join(NODE).exists());
        assert!(remove_node_binaries_in(&bin.join("missing")).is_empty());
        fs::remove_dir_all(&bin).unwrap();
    }
}
