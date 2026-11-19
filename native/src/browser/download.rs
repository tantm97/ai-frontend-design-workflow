//! Ports `installBrowser` and `checkedDownloadUrl` of `core/lib/cdp.mjs`: the one-time download
//! of Google's chrome-headless-shell ("dfw setup --browser") for machines without Chrome, Edge
//! or Chromium.

use std::fs;
use std::io::{self, Cursor};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};

use super::launch::browser_dir;
use crate::core::error::DfwError;
use crate::core::http;
use crate::core::platform::{node_arch, node_platform};
use crate::core::util::{sha256, to_json_compact, to_json_pretty};

const CFT_URL: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";
const DOWNLOAD_HOST: &str = "storage.googleapis.com";
/// The archive is about 100 MB: 20 minutes is a slow line, not a hung one.
const ZIP_TIMEOUT_S: u64 = 20 * 60;
/// Size caps: the index is a few hundred kB, an archive about 100 MB.
const INDEX_LIMIT: u64 = 16 << 20;
const ZIP_LIMIT: u64 = 1 << 30;

/// Chrome for Testing's name of this platform, or `None` when it has no build for it.
fn cft_platform() -> Option<&'static str> {
    match (node_platform(), node_arch()) {
        ("darwin", "arm64") => Some("mac-arm64"),
        ("darwin", _) => Some("mac-x64"),
        ("win32", "ia32") => Some("win32"),
        ("win32", _) => Some("win64"),
        ("linux", "arm64") => Some("linux-arm64"),
        ("linux", "x64") => Some("linux64"),
        _ => None,
    }
}

/// `checkedDownloadUrl(url)`: the archive is unpacked and run, so it is accepted only over
/// https from Google's storage host, whatever the index says. Returns the URL, or an error.
///
/// Stricter than `new URL()` in Node on purpose: a URL the WHATWG parser would first repair
/// (backslashes, an empty user name, a percent-encoded host, tabs) is refused, never repaired.
// The tests prove the URL rules with it; the download itself checks the index entry as it
// came (`checked`).
#[cfg(test)]
pub fn checked_download_url(url: &str) -> Result<String, DfwError> {
    checked(&Value::String(url.to_string()))
}

fn checked(url: &Value) -> Result<String, DfwError> {
    url.as_str().and_then(storage_href).ok_or_else(|| {
        DfwError::new(format!(
            "refusing to download a browser from {}: expected https://{DOWNLOAD_HOST}/...",
            // JSON.stringify(undefined) is "undefined".
            if url.is_null() {
                "undefined".to_string()
            } else {
                to_json_compact(url)
            }
        ))
    })
}

/// The normalised URL when it is `https://storage.googleapis.com[:port]/...` without credentials.
fn storage_href(url: &str) -> Option<String> {
    let url = url.trim_matches(|c: char| c <= ' ');
    if url.chars().any(|c| c.is_control() || c == '\\' || c == ' ') {
        return None;
    }
    let rest = url
        .get(..8)
        .filter(|p| p.eq_ignore_ascii_case("https://"))
        .map(|_| &url[8..])?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);
    let (host, port) = match authority.split_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (authority, None),
    };
    if !host.eq_ignore_ascii_case(DOWNLOAD_HOST) {
        return None;
    }
    let port = match port {
        None | Some("") | Some("443") => String::new(),
        Some(p) if p.bytes().all(|b| b.is_ascii_digit()) && p.parse::<u16>().is_ok() => {
            format!(":{}", p.trim_start_matches('0'))
        }
        Some(_) => return None,
    };
    let slash = if tail.starts_with('/') { "" } else { "/" };
    Some(format!("https://{DOWNLOAD_HOST}{port}{slash}{tail}"))
}

/// `installBrowser({ log })`: download Google's chrome-headless-shell (Chrome for Testing,
/// stable) into `~/.config/dfw/browser`. Returns `{ installed, path, source, version }`.
pub fn install_browser(log: &dyn Fn(&str)) -> Result<Value, DfwError> {
    let plat = cft_platform().ok_or_else(|| {
        DfwError::new(format!(
            "No headless Chrome build for {}-{}; install Chromium with your package manager (it is found automatically) or set DFW_BROWSER.",
            node_platform(),
            node_arch()
        ))
    })?;
    install_from(CFT_URL, plat, &browser_dir(), checked, log)
}

/// [`install_browser`] with the index URL, platform, target folder and URL check given.
fn install_from(
    index_url: &str,
    plat: &str,
    browser_dir: &Path,
    check: fn(&Value) -> Result<String, DfwError>,
    log: &dyn Fn(&str),
) -> Result<Value, DfwError> {
    let index: Value = serde_json::from_slice(&fetch(index_url, 30, INDEX_LIMIT)?.1)?;
    let stable = index.get("channels").and_then(|c| c.get("Stable"));
    let dl = stable
        .and_then(|s| s.get("downloads")?.get("chrome-headless-shell")?.as_array())
        .and_then(|list| {
            list.iter()
                .find(|d| d.get("platform").and_then(Value::as_str) == Some(plat))
        })
        .ok_or_else(|| {
            DfwError::new(format!(
                "Chrome for Testing has no headless shell for {plat}."
            ))
        })?;
    let from = check(dl.get("url").unwrap_or(&Value::Null))?;
    let version = stable
        .and_then(|s| s.get("version"))
        .unwrap_or(&Value::Null);
    // The version becomes a folder name: digits and dots only.
    let version = version
        .as_str()
        .filter(|v| {
            !v.is_empty()
                && v.split('.')
                    .all(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        })
        .ok_or_else(|| {
            DfwError::new(format!(
                "unexpected Chrome version {} in the Chrome for Testing index.",
                if version.is_null() {
                    "undefined".to_string()
                } else {
                    to_json_compact(version)
                }
            ))
        })?;
    let dir = browser_dir.join(version);
    let exe = dir
        .join(format!("chrome-headless-shell-{plat}"))
        .join(if plat.starts_with("win") {
            "chrome-headless-shell.exe"
        } else {
            "chrome-headless-shell"
        });
    if !exe.exists() {
        fs::create_dir_all(&dir)?;
        let host = from.split('/').nth(2).unwrap_or_default();
        log(&format!(
            "Downloading headless Chrome {version} ({plat}) from {host} ..."
        ));
        let (landed, zip) = fetch(&from, ZIP_TIMEOUT_S, ZIP_LIMIT)?;
        check(&Value::String(landed.clone()))?; // a redirect must stay on the same host
                                                // Chrome for Testing publishes no checksum for its archives: what was unpacked is
                                                // recorded instead, so a later question "which file was this" has an answer.
        fs::write(
            dir.join("archive.json"),
            to_json_pretty(
                &json!({ "url": from, "landed": landed, "bytes": zip.len(), "sha256": sha256(&zip) }),
            ),
        )?;
        let extracted = unpack(&zip, &dir);
        if extracted.is_err() || !exe.exists() {
            let why = extracted
                .err()
                .map_or_else(|| format!("no {} in it", exe.display()), |e| e.to_string());
            return Err(DfwError::new(format!(
                "could not extract the headless Chrome archive ({why})."
            )));
        }
    }
    let exe_text = exe.display().to_string();
    fs::write(
        browser_dir.join("current.json"),
        to_json_pretty(&json!({ "version": version, "platform": plat, "path": exe_text })),
    )?;
    Ok(json!({ "installed": true, "path": exe_text, "source": "downloaded", "version": version }))
}

/// GET a URL: the URL it ended at (after redirects) and the body. `HTTP <status>` is an error.
fn fetch(url: &str, timeout_s: u64, limit: u64) -> Result<(String, Vec<u8>), DfwError> {
    let res = http::get(url, &[], Duration::from_secs(timeout_s), limit)
        .map_err(|e| DfwError::internal(format!("fetch failed: {url}: {e}")))?;
    if !res.ok() {
        return Err(DfwError::new(format!(
            "download failed: HTTP {}",
            res.status
        )));
    }
    Ok((res.landed, res.body))
}

/// Unpack a zip archive into `dir`, keeping executable bits. An entry that would land outside
/// `dir` (an absolute name, `..`) or a symbolic link fails the whole archive. Links are refused
/// outright: a link checked on its own can still be chained (`d/l -> ..`, then `e -> d/l/..`,
/// then `e/x`) to write outside `dir`, and the headless shell archives hold none (plain files
/// and folders: the binary, its libraries and resources).
fn unpack(zip_bytes: &[u8], dir: &Path) -> io::Result<()> {
    let bad = |what: &str| io::Error::new(io::ErrorKind::InvalidData, what.to_string());
    let mut archive = zip::ZipArchive::new(Cursor::new(zip_bytes)).map_err(io::Error::other)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(io::Error::other)?;
        let rel: PathBuf = entry
            .enclosed_name()
            .ok_or_else(|| bad("an entry points outside the archive"))?;
        let to = dir.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&to)?;
            continue;
        }
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        if entry.is_symlink() {
            return Err(bad("the archive holds a symbolic link"));
        }
        let mode = entry.unix_mode();
        let mut file = fs::File::create(&to)?;
        io::copy(&mut entry, &mut file)?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&to, fs::Permissions::from_mode(mode & 0o777))?;
        }
        #[cfg(not(unix))]
        let _ = mode;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::serve_routes as serve;
    use crate::core::util::test_dir;
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::atomic::Ordering;
    use zip::write::SimpleFileOptions;

    /// cdp: the headless Chrome download is https on storage.googleapis.com only
    #[test]
    fn download_is_https_on_storage_googleapis_com_only() {
        let ok = "https://storage.googleapis.com/chrome-for-testing-public/131.0.6778.85/mac-arm64/chrome-headless-shell-mac-arm64.zip";
        assert_eq!(checked_download_url(ok).unwrap(), ok);
        for bad in [
            "http://storage.googleapis.com/chrome-for-testing-public/x.zip",
            "https://storage.googleapis.com.evil.example/x.zip",
            "https://evil.example/storage.googleapis.com/x.zip",
            "https://user:pw@storage.googleapis.com/x.zip",
            "https://storage.googleapis.com@evil.example/x.zip",
            "file:///tmp/x.zip",
            "not a url",
        ] {
            let err = checked_download_url(bad).unwrap_err();
            assert!(
                err.message().starts_with("refusing to download a browser"),
                "{bad}"
            );
        }
        // `undefined` in the JS test: the index entry has no url.
        assert_eq!(
            checked(&Value::Null).unwrap_err().message(),
            "refusing to download a browser from undefined: expected https://storage.googleapis.com/..."
        );
        assert_eq!(
            checked_download_url("http://x/\"y").unwrap_err().message(),
            "refusing to download a browser from \"http://x/\\\"y\": expected https://storage.googleapis.com/..."
        );
    }

    #[test]
    fn url_check_normalises_like_the_url_parser_and_refuses_what_it_would_repair() {
        assert_eq!(
            checked_download_url("HTTPS://Storage.GoogleAPIs.com").unwrap(),
            "https://storage.googleapis.com/"
        );
        assert_eq!(
            checked_download_url("https://storage.googleapis.com:443/a?b#c").unwrap(),
            "https://storage.googleapis.com/a?b#c"
        );
        assert_eq!(
            checked_download_url("https://storage.googleapis.com:8443/a").unwrap(),
            "https://storage.googleapis.com:8443/a"
        );
        for bad in [
            "https://storage.googleapis.com\\@evil.example/x.zip",
            "https:\\\\storage.googleapis.com/x.zip",
            "https://storage.googleapis.com:99999/x.zip",
            "https://storage.googleapis.com:80x/x.zip",
            "https://storage.googleapis.com./x.zip",
            "https://storage%2Egoogleapis.com/x.zip",
            "https://@storage.googleapis.com/x.zip",
            "https://storage.googleapis.com/a b.zip",
            "https://storage.googleapis.com/a\tb.zip",
            "https:/storage.googleapis.com/x.zip",
            "",
        ] {
            assert!(checked_download_url(bad).is_err(), "{bad}");
        }
    }

    fn zip_of(files: &[(&str, &[u8], u32)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, body, mode) in files {
            writer
                .start_file(*name, SimpleFileOptions::default().unix_permissions(*mode))
                .unwrap();
            writer.write_all(body).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn index(version: &Value, url: &str) -> Vec<u8> {
        json!({ "channels": { "Stable": { "version": version, "downloads": { "chrome-headless-shell": [
            { "platform": "other-plat", "url": "https://storage.googleapis.com/other.zip" },
            { "platform": "test-plat", "url": url },
        ] } } } })
        .to_string()
        .into_bytes()
    }

    /// The test's stand-in for the host check: the local fixture server only.
    fn local_only(url: &Value) -> Result<String, DfwError> {
        url.as_str()
            .filter(|u| u.starts_with("http://127.0.0.1:"))
            .map(str::to_string)
            .ok_or_else(|| DfwError::new("not the fixture server"))
    }

    /// The whole download against a local HTTP server: index, archive, unzip, where it is
    /// stored, and no second download once it is there.
    #[test]
    // One scenario end to end: the steps share the server and the folder they fill.
    #[allow(clippy::too_many_lines)]
    fn install_from_a_local_index() {
        let home = test_dir("download").join("browser");
        let archive = zip_of(&[
            (
                "chrome-headless-shell-test-plat/chrome-headless-shell",
                b"#!/bin/sh\necho shell\n",
                0o755,
            ),
            ("chrome-headless-shell-test-plat/LICENSE", b"license", 0o644),
        ]);
        // The index names its own server, so it is built once the port is known: two servers.
        let (files, file_hits) = serve(HashMap::from([("/shell.zip".to_string(), (200, archive))]));
        let (base, _) = serve(HashMap::from([
            (
                "/index.json".to_string(),
                (
                    200,
                    index(&json!("131.0.6778.85"), &format!("{files}/shell.zip")),
                ),
            ),
            (
                "/evil.json".to_string(),
                (
                    200,
                    index(
                        &json!("131.0.6778.85"),
                        "https://storage.googleapis.com.evil.example/x.zip",
                    ),
                ),
            ),
            (
                "/version.json".to_string(),
                (
                    200,
                    index(&json!("../../131"), &format!("{files}/shell.zip")),
                ),
            ),
            (
                "/gone.json".to_string(),
                (
                    200,
                    index(&json!("132.0.1.2"), &format!("{files}/gone.zip")),
                ),
            ),
        ]));
        let logged = std::cell::RefCell::new(Vec::new());
        let log = |m: &str| logged.borrow_mut().push(m.to_string());

        let r = install_from(
            &format!("{base}/index.json"),
            "test-plat",
            &home,
            local_only,
            &log,
        )
        .unwrap();
        let exe = home.join("131.0.6778.85/chrome-headless-shell-test-plat/chrome-headless-shell");
        assert_eq!(
            r,
            json!({ "installed": true, "path": exe.display().to_string(), "source": "downloaded", "version": "131.0.6778.85" })
        );
        assert_eq!(fs::read(&exe).unwrap(), b"#!/bin/sh\necho shell\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&exe).unwrap().permissions().mode() & 0o777,
                0o755
            );
            assert_eq!(
                fs::metadata(exe.with_file_name("LICENSE"))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o644
            );
        }
        assert_eq!(
            fs::read_to_string(home.join("current.json")).unwrap(),
            // The path as a JSON string: a Windows path has its backslashes escaped.
            format!("{{\n  \"version\": \"131.0.6778.85\",\n  \"platform\": \"test-plat\",\n  \"path\": {}\n}}", json!(exe.display().to_string()))
        );
        assert_eq!(
            *logged.borrow(),
            [format!(
                "Downloading headless Chrome 131.0.6778.85 (test-plat) from {} ...",
                files.trim_start_matches("http://")
            )]
        );
        assert_eq!(file_hits.load(Ordering::SeqCst), 1);
        // No checksum is published: the archive that was unpacked is recorded.
        let record: Value = serde_json::from_str(
            &fs::read_to_string(home.join("131.0.6778.85/archive.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(record["url"], json!(format!("{files}/shell.zip")));
        assert!(record["bytes"].as_u64().unwrap() > 0);
        assert_eq!(record["sha256"].as_str().unwrap().len(), 64);

        // Already there: the index is read again, the archive is not.
        install_from(
            &format!("{base}/index.json"),
            "test-plat",
            &home,
            local_only,
            &log,
        )
        .unwrap();
        assert_eq!(file_hits.load(Ordering::SeqCst), 1);

        // The real check refuses the fixture server and any look-alike host, before any download.
        for index in ["index.json", "evil.json"] {
            let err = install_from(
                &format!("{base}/{index}"),
                "test-plat",
                &home,
                checked,
                &log,
            )
            .unwrap_err();
            assert!(
                err.message()
                    .starts_with("refusing to download a browser from \"http"),
                "{err}"
            );
        }
        assert_eq!(file_hits.load(Ordering::SeqCst), 1);

        let err = install_from(
            &format!("{base}/version.json"),
            "test-plat",
            &home,
            local_only,
            &log,
        )
        .unwrap_err();
        assert_eq!(
            err.message(),
            "unexpected Chrome version \"../../131\" in the Chrome for Testing index."
        );
        let err = install_from(
            &format!("{base}/index.json"),
            "mars-x1",
            &home,
            local_only,
            &log,
        )
        .unwrap_err();
        assert_eq!(
            err.message(),
            "Chrome for Testing has no headless shell for mars-x1."
        );
        let err = install_from(
            &format!("{base}/gone.json"),
            "test-plat",
            &home,
            local_only,
            &log,
        )
        .unwrap_err();
        assert_eq!(err.message(), "download failed: HTTP 404");
        assert!(install_from(
            &format!("{base}/missing.json"),
            "test-plat",
            &home,
            local_only,
            &log
        )
        .is_err());
    }

    #[test]
    fn archive_entries_stay_inside_the_folder() {
        let dir = test_dir("unzip");
        let inside = dir.join("in");
        fs::create_dir(&inside).unwrap();
        let evil = zip_of(&[("ok.txt", b"ok", 0o644), ("../escaped.txt", b"no", 0o644)]);
        assert!(unpack(&evil, &inside).is_err());
        assert!(!dir.join("escaped.txt").exists());
        assert!(unpack(b"not a zip", &inside).is_err());

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_directory("d/", SimpleFileOptions::default())
            .unwrap();
        writer
            .add_symlink("d/out", "../../outside", SimpleFileOptions::default())
            .unwrap();
        let link_out = writer.finish().unwrap().into_inner();
        assert!(unpack(&link_out, &inside).is_err());

        // A link checked on its own looks inside; chained, the next entry lands outside.
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .add_symlink("d/l", "..", SimpleFileOptions::default())
            .unwrap();
        writer
            .add_symlink("e", "d/l/..", SimpleFileOptions::default())
            .unwrap();
        writer
            .start_file("e/x", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"escaped").unwrap();
        let chained = writer.finish().unwrap().into_inner();
        let chain = dir.join("chain");
        fs::create_dir(&chain).unwrap();
        let err = unpack(&chained, &chain).unwrap_err();
        assert_eq!(err.to_string(), "the archive holds a symbolic link");
        assert!(!dir.join("x").exists(), "nothing written outside");

        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        writer
            .start_file("d/real", SimpleFileOptions::default())
            .unwrap();
        writer.write_all(b"real").unwrap();
        writer
            .add_symlink("d/link", "real", SimpleFileOptions::default())
            .unwrap();
        let link_in = writer.finish().unwrap().into_inner();
        let fine = dir.join("fine");
        fs::create_dir(&fine).unwrap();
        assert!(
            unpack(&link_in, &fine).is_err(),
            "even a link that stays inside"
        );
    }
}
