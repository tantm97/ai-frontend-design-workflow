//! Ports `core/lib/a11y.mjs`: the rendered accessibility check. axe-core (Deque, MPL-2.0) is
//! injected over CDP into each composed screen (every state and theme) or into a live URL.
//! axe.min.js is downloaded once, pinned and verified ("dfw setup --a11y"), and used unmodified
//! with its license file next to it. For live URLs the check also clicks stateful controls and
//! reports the ones that change nothing ("dead-control").

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use serde_json::{json, Value};
use sha2::{Digest, Sha512};

use crate::core::encoding::base64_encode;

/// How long unpacking the axe-core archive may take.
const UNPACK_DEADLINE: std::time::Duration = std::time::Duration::from_secs(60);
use super::cdp::{lock, method};
use super::launch::{
    browser_status, fonts_ready, open_renderer_with, BrowserStatus, LaunchOptions, Renderer,
};
use super::layout_check::{head, manifest_breakpoints, states_of, Finding};
use crate::core::config::load_config;
use crate::core::error::DfwError;
use crate::core::http;
use crate::core::js;
use crate::core::level::Level;
use crate::core::paths::file_url;
use crate::core::paths::{global_dir, project_paths, rel};
use crate::core::util::read_json;
use crate::design::compose::{build_composed, build_system};
use crate::design::manifest::{Manifest, Screen, Variant};

/// The pinned axe-core release (`AXE`).
pub const AXE_VERSION: &str = "4.14.0";
const AXE_TARBALL: &str = "https://registry.npmjs.org/axe-core/-/axe-core-4.14.0.tgz";
const AXE_INTEGRITY: &str = "sha512-9WTZxEjsZ7b13TH8JPmbV2z8CHbl80/2hm3XPEG4JgNdQLK81IBRXmSxHfMAOkSqQeRxT/0dwNDz2GOm3zzpcQ==";
/// The file of the axe-core package dfw runs, and its licence, the two files it keeps.
const AXE_FILE: &str = "axe.min.js";
const AXE_LICENSE: &str = "LICENSE";
const NOT_DOWNLOADED: &str = "axe-core not downloaded (dfw setup --a11y, ~550 KB, once)";

/// `runAxe` and `deadControls` of a11y.mjs, byte for byte: they run in the page.
const RUN_AXE: &str = include_str!("a11y_run_axe.js");
const DEAD_CONTROLS: &str = include_str!("a11y_dead_controls.js");
const SET_STATE_AND_THEME: &str = "(a) => {
            if (a.state) document.documentElement.setAttribute('data-state', a.state);
            if (a.theme) document.documentElement.dataset.theme = a.theme;
          }";

const CONCURRENCY: usize = 6;

fn axe_dir() -> PathBuf {
    global_dir()
        .join("tools")
        .join("axe-core")
        .join(AXE_VERSION)
}

/// `axeStatus()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AxeStatus {
    Installed { path: PathBuf },
    Missing { detail: &'static str },
}

impl AxeStatus {
    /// The status as Node prints it (`{ installed, path, version }` or `{ installed, detail }`).
    pub fn to_json(&self) -> Value {
        match self {
            AxeStatus::Installed { path } => {
                json!({ "installed": true, "path": path.display().to_string(), "version": AXE_VERSION })
            }
            AxeStatus::Missing { detail } => json!({ "installed": false, "detail": detail }),
        }
    }
}

/// Whether the pinned axe.min.js is in the dfw home, and where.
pub fn axe_status() -> AxeStatus {
    status_in(&axe_dir())
}

fn status_in(dir: &Path) -> AxeStatus {
    let path = dir.join(AXE_FILE);
    if path.exists() {
        AxeStatus::Installed { path }
    } else {
        AxeStatus::Missing {
            detail: NOT_DOWNLOADED,
        }
    }
}

/// `installAxe({ log })`: download the pinned axe-core tarball, check its npm integrity hash,
/// keep axe.min.js and LICENSE.
pub fn install_axe(log: &dyn Fn(&str)) -> Result<AxeStatus, DfwError> {
    install_axe_from(&axe_dir(), AXE_TARBALL, AXE_INTEGRITY, log)
}

fn install_axe_from(
    dir: &Path,
    url: &str,
    integrity: &str,
    log: &dyn Fn(&str),
) -> Result<AxeStatus, DfwError> {
    if let installed @ AxeStatus::Installed { .. } = status_in(dir) {
        return Ok(installed);
    }
    log(&format!(
        "Downloading axe-core {AXE_VERSION} from registry.npmjs.org ..."
    ));
    let res = http::get(url, &[], Duration::from_secs(60), 64 << 20)
        .map_err(|e| DfwError::internal(format!("fetch failed: {url}: {e}")))?;
    if !res.ok() {
        return Err(DfwError::new(format!(
            "axe-core download failed: HTTP {}",
            res.status
        )));
    }
    let body = res.body;
    let got = format!("sha512-{}", base64_encode(&Sha512::digest(&body)));
    if got != integrity {
        return Err(DfwError::new(
            "axe-core download does not match the pinned integrity hash; refusing to use it.",
        ));
    }
    fs::create_dir_all(dir)?;
    let tgz = dir.join("axe-core.tgz");
    crate::core::util::write_atomic(&tgz, &body)?;
    // Through core::process: a deadline, and on Windows an absolute path (never a bare name).
    let tar_exe = crate::core::process::find_executable("tar")
        .map_or_else(|| "tar".to_string(), |p| p.display().to_string());
    let plan = crate::core::process::Spawn::direct(
        tar_exe,
        vec![
            "-xzf".into(),
            tgz.display().to_string(),
            "-C".into(),
            dir.display().to_string(),
            "--strip-components=1".into(),
            format!("package/{AXE_FILE}"),
            format!("package/{AXE_LICENSE}"),
        ],
    );
    let ran = crate::core::process::run_timed(&plan, None, &[], UNPACK_DEADLINE);
    let _ = fs::remove_file(&tgz);
    // Node reports a tar that cannot start with an empty reason.
    let (ok, stderr) = if ran.timed_out {
        (
            false,
            format!(
                "tar did not finish within {} s and was stopped; run the command again",
                UNPACK_DEADLINE.as_secs()
            ),
        )
    } else if ran.error.is_some() {
        (false, String::new())
    } else {
        (ran.status == Some(0), ran.stderr)
    };
    if !ok || !dir.join(AXE_FILE).exists() {
        return Err(DfwError::new(format!(
            "could not extract axe-core (needs tar): {}",
            head(&stderr, 200)
        )));
    }
    Ok(status_in(dir))
}

/// What to check (`opts` of `a11yCheck`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct A11yOptions {
    /// A live page instead of the design screens.
    pub url: Option<String>,
    /// Only these screen ids.
    pub screens: Option<Vec<String>>,
    /// Also every state of each screen.
    pub states: bool,
    /// Also every theme of the design.
    pub themes: bool,
}

/// The `status` of an [`A11yResult`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum A11yStatus {
    /// No browser, or axe not downloaded: reported, never treated as a pass.
    Unverifiable,
    /// Nothing to check.
    Skipped,
    Ran,
}

/// The result of [`a11y_check`] (`{ tool, status, detail, findings }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct A11yResult {
    pub status: A11yStatus,
    pub detail: Option<String>,
    pub findings: Vec<Finding>,
}

/// A finding as a11y.mjs writes it (`source` before `message`).
pub fn finding_json(f: &Finding) -> Value {
    json!({ "level": f.level, "rule": f.rule, "file": f.file, "line": 1, "source": f.source, "message": f.message })
}

impl A11yResult {
    /// The result as Node returns it.
    pub fn to_json(&self) -> Value {
        let status = match self.status {
            A11yStatus::Unverifiable => "unverifiable",
            A11yStatus::Skipped => "skipped",
            A11yStatus::Ran => "ran",
        };
        let mut out = json!({ "tool": "axe", "status": status });
        if let Some(detail) = &self.detail {
            out["detail"] = json!(detail);
        }
        out["findings"] = self.findings.iter().map(finding_json).collect();
        out
    }
}

/// One page to check.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Job {
    /// The design file relative to the project root, or the live URL.
    rel: String,
    label: String,
    state: Option<String>,
    theme: Option<String>,
    width: u32,
    height: u32,
    live: bool,
}

fn screen_jobs(root: &Path, opts: &A11yOptions) -> Result<Vec<Job>, DfwError> {
    let manifest = read_json(&project_paths(root).manifest, Some(json!({})))?;
    let bps = manifest_breakpoints(&manifest, &load_config(Some(root))?);
    let bp = bps.iter().find(|b| b.width >= 1024).or(bps.first());
    build_system(root)?;
    let texts = |value: &Value, key: &str| -> Vec<String> {
        let items = value.get(key).and_then(Value::as_array);
        items
            .into_iter()
            .flatten()
            .map(|v| js::to_string(Some(v)))
            .collect()
    };
    let mut themes = vec![None];
    if opts.themes {
        themes.extend(texts(&manifest, "themes").into_iter().map(Some));
    }
    let mut jobs = Vec::new();
    for s in Manifest(&manifest).screens().map(Screen::value) {
        let id = js::to_string(s.get("id"));
        if opts
            .screens
            .as_ref()
            .is_some_and(|only| !only.contains(&id))
        {
            continue;
        }
        let mut states = vec![None];
        if opts.states {
            states.extend(states_of(s, &manifest).into_iter().map(Some));
        }
        for v in Screen(s).variants().map(Variant::value) {
            // Node reads `bp.width` here and fails when the design has no breakpoint at all.
            let bp = bp.ok_or_else(|| {
                DfwError::internal("Cannot read properties of undefined (reading 'width')")
            })?;
            for st in &states {
                for th in &themes {
                    let truthy = |v: &Option<String>| v.clone().filter(|t| !t.is_empty());
                    let state = truthy(st).map(|t| format!(" [{t}]")).unwrap_or_default();
                    let theme = truthy(th).map(|t| format!(" {{{t}}}")).unwrap_or_default();
                    jobs.push(Job {
                        rel: rel::design(&js::to_string(v.get("file"))),
                        label: format!("{id}/{}{state}{theme}", js::to_string(v.get("id"))),
                        state: st.clone(),
                        theme: th.clone(),
                        width: bp.width,
                        height: bp.height,
                        live: false,
                    });
                }
            }
        }
    }
    Ok(jobs)
}

/// `a11yCheck(root, opts)`: run axe on a live URL or on every screen of the design.
pub fn a11y_check(root: &Path, opts: &A11yOptions) -> Result<A11yResult, DfwError> {
    a11y_check_with(root, opts, &LaunchOptions::default())
}

/// [`a11y_check`] with launch options for the browser.
pub fn a11y_check_with(
    root: &Path,
    opts: &A11yOptions,
    launch: &LaunchOptions,
) -> Result<A11yResult, DfwError> {
    let result = |status, detail: Option<&str>, findings| A11yResult {
        status,
        detail: detail.map(str::to_string),
        findings,
    };
    if let BrowserStatus::Missing { detail } = browser_status() {
        return Ok(result(A11yStatus::Unverifiable, Some(detail), Vec::new()));
    }
    let axe = match axe_status() {
        AxeStatus::Installed { path } => path,
        AxeStatus::Missing { detail } => {
            return Ok(result(A11yStatus::Unverifiable, Some(detail), Vec::new()))
        }
    };
    let axe_src = String::from_utf8_lossy(&fs::read(&axe)?).into_owned();
    // Each job with the URL to open; a design file that was not composed has none.
    let jobs: Vec<(Job, Option<String>)> = match opts.url.as_deref().filter(|u| !u.is_empty()) {
        Some(url) => {
            let job = Job {
                rel: url.to_string(),
                label: url.to_string(),
                state: None,
                theme: None,
                width: 1280,
                height: 800,
                live: true,
            };
            vec![(job, Some(url.to_string()))]
        }
        None => {
            let jobs = screen_jobs(root, opts)?;
            let mut rels: Vec<String> = Vec::new();
            for job in &jobs {
                if !rels.contains(&job.rel) {
                    rels.push(job.rel.clone());
                }
            }
            let built = build_composed(root, &rels)?;
            let url_of = |rel: &str| {
                built
                    .iter()
                    .find(|b| b.rel == rel)
                    .map(|b| file_url(&b.out))
            };
            jobs.into_iter()
                .map(|job| {
                    let url = url_of(&job.rel);
                    (job, url)
                })
                .collect()
        }
    };
    if jobs.is_empty() {
        return Ok(result(A11yStatus::Skipped, None, Vec::new()));
    }
    let renderer = open_renderer_with(launch)?;
    let slots: Mutex<Vec<Vec<Finding>>> = Mutex::new(vec![Vec::new(); jobs.len()]);
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..CONCURRENCY.min(jobs.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some((job, url)) = jobs.get(i) else {
                    break;
                };
                let found = check_page(&renderer, job, url.as_deref(), &axe_src);
                lock(&slots)[i] = found;
            });
        }
    });
    renderer.close();
    let slots = slots
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let findings = slots.into_iter().flatten().collect();
    Ok(result(A11yStatus::Ran, None, findings))
}

fn impact_level(impact: Option<&str>) -> &'static str {
    match impact {
        Some("critical" | "serious") => Level::Error.as_str(),
        Some("minor") => Level::Info.as_str(),
        _ => Level::Warn.as_str(),
    }
}

/// Run axe (and the dead-control probe on a live page) in one tab. A page that cannot be
/// checked is a finding, after the findings made before it failed.
fn check_page(renderer: &Renderer, job: &Job, url: Option<&str>, axe_src: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let mut add = |level, rule: String, message: String| {
        findings.push(Finding {
            level,
            rule,
            file: job.rel.clone(),
            message: format!("{}: {message}", job.label),
            source: "axe",
        });
    };
    let mut run = || -> Result<(), DfwError> {
        let page = renderer.page(job.width, job.height, 1.0)?;
        let checked = (|| {
            let url = url.ok_or_else(|| {
                DfwError::internal(
                    "The \"path\" argument must be of type string. Received undefined",
                )
            })?;
            page.goto(url, 30_000)?;
            page.evaluate(
                SET_STATE_AND_THEME,
                Some(&json!({ "state": job.state, "theme": job.theme })),
            )?;
            fonts_ready(&page, 3000)?;
            page.send(method::RUNTIME_EVALUATE, json!({ "expression": axe_src }))?;
            let violations = page.evaluate(RUN_AXE.trim_end(), None)?;
            for v in violations.as_array().into_iter().flatten() {
                let text = |key: &str| js::to_string(v.get(key));
                let summary = text("summary");
                let summary = if summary.is_empty() {
                    String::new()
                } else {
                    format!(": {summary}")
                };
                add(
                    impact_level(v.get("impact").and_then(Value::as_str)),
                    format!("axe-{}", text("id")),
                    format!(
                        "{} ({} element(s), e.g. {}){summary}",
                        text("help"),
                        text("nodes"),
                        text("target")
                    ),
                );
            }
            if job.live {
                let dead = page.evaluate(DEAD_CONTROLS.trim_end(), None)?;
                for name in dead.as_array().into_iter().flatten() {
                    add(Level::Warn.as_str(), "dead-control".into(), format!("control \"{}\" announces a state (aria-pressed/expanded/selected/sort) but clicking it changes nothing.", js::to_string(Some(name))));
                }
            }
            Ok(())
        })();
        page.close();
        checked
    };
    if let Err(err) = run() {
        add(
            Level::Warn.as_str(),
            "axe-render".into(),
            format!("could not check ({})", head(err.message(), 120)),
        );
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::COMPOSED_PAGE;
    #[cfg(unix)]
    use std::process::Command;

    /// The JavaScript that runs in the page is one self-contained function per file; axe is
    /// pinned by version and integrity (the golden render tests check what they do in Chrome).
    #[test]
    fn page_scripts_are_self_contained_functions_and_axe_is_pinned() {
        assert!(RUN_AXE.starts_with("async function runAxe() {\n"));
        assert!(DEAD_CONTROLS.starts_with("async function deadControls() {\n"));
        assert!(AXE_VERSION.split('.').all(|n| n.parse::<u32>().is_ok()));
        assert!(AXE_TARBALL.starts_with("https://registry.npmjs.org/axe-core/-/axe-core-"));
        assert!(AXE_TARBALL.contains(AXE_VERSION));
        assert!(AXE_INTEGRITY.starts_with("sha512-"));
        assert!(!NOT_DOWNLOADED.is_empty() && !SET_STATE_AND_THEME.is_empty());
    }

    /// One HTTP response per connection, for as many connections as there are bodies.
    #[cfg(unix)]
    fn serve(responses: Vec<(u16, Vec<u8>)>) -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/axe-core.tgz", listener.local_addr().unwrap());
        thread::spawn(move || {
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = [0u8; 2048];
                let _ = stream.read(&mut request);
                let head = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(&body).unwrap();
            }
        });
        url
    }

    /// The download is a local tarball here: the real one is never fetched in tests.
    #[test]
    #[cfg(unix)]
    fn install_checks_the_integrity_and_keeps_two_files() {
        let tmp = crate::core::util::test_dir("axe-install");
        let package = tmp.join("src/package");
        fs::create_dir_all(&package).unwrap();
        for name in [AXE_FILE, AXE_LICENSE, "axe.js"] {
            fs::write(package.join(name), format!("// {name}")).unwrap();
        }
        let tgz = tmp.join("fake.tgz");
        let packed = Command::new("tar")
            .arg("-czf")
            .arg(&tgz)
            .arg("-C")
            .arg(tmp.join("src"))
            .arg("package")
            .status()
            .unwrap();
        assert!(packed.success());
        let body = fs::read(&tgz).unwrap();
        let integrity = format!("sha512-{}", base64_encode(&Sha512::digest(&body)));
        let url = serve(vec![(404, vec![]), (200, body.clone()), (200, body)]);
        let dir = tmp.join("home/tools/axe-core").join(AXE_VERSION);
        let logged = Mutex::new(Vec::new());
        let log = |m: &str| lock(&logged).push(m.to_string());
        let install = |integrity: &str| install_axe_from(&dir, &url, integrity, &log);

        let missing = install(&integrity).unwrap_err();
        assert_eq!(missing.message(), "axe-core download failed: HTTP 404");
        let tampered = install(AXE_INTEGRITY).unwrap_err();
        assert_eq!(
            tampered.message(),
            "axe-core download does not match the pinned integrity hash; refusing to use it."
        );
        assert!(
            !dir.exists(),
            "nothing is kept from a download that fails the check"
        );
        let path = dir.join(AXE_FILE);
        assert_eq!(
            install(&integrity).unwrap(),
            AxeStatus::Installed { path: path.clone() }
        );
        let mut kept: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        kept.sort();
        assert_eq!(kept, [AXE_LICENSE, AXE_FILE]);
        assert_eq!(fs::read_to_string(&path).unwrap(), format!("// {AXE_FILE}"));
        // Installed: no download (the server has no response left).
        assert_eq!(install(&integrity).unwrap(), AxeStatus::Installed { path });
        assert_eq!(
            *lock(&logged),
            ["Downloading axe-core 4.14.0 from registry.npmjs.org ..."; 3]
        );
        assert_eq!(
            status_in(&tmp.join("nowhere")),
            AxeStatus::Missing {
                detail: NOT_DOWNLOADED
            }
        );
    }

    #[test]
    fn results_and_findings_print_like_node() {
        let finding = Finding {
            level: impact_level(Some("serious")),
            rule: "axe-color-contrast".into(),
            file: rel::screen(COMPOSED_PAGE),
            message: "s/a: Elements must meet minimum color contrast ratio thresholds (2 element(s), e.g. p)".into(),
            source: "axe",
        };
        let ran = A11yResult {
            status: A11yStatus::Ran,
            detail: None,
            findings: vec![finding],
        };
        assert_eq!(
            crate::core::util::to_json_compact(&ran.to_json()),
            r#"{"tool":"axe","status":"ran","findings":[{"level":"error","rule":"axe-color-contrast","file":"design/screens/s/a.html","line":1,"source":"axe","message":"s/a: Elements must meet minimum color contrast ratio thresholds (2 element(s), e.g. p)"}]}"#
        );
        let missing = A11yResult {
            status: A11yStatus::Unverifiable,
            detail: Some(NOT_DOWNLOADED.into()),
            findings: vec![],
        };
        assert_eq!(
            crate::core::util::to_json_compact(&missing.to_json()),
            r#"{"tool":"axe","status":"unverifiable","detail":"axe-core not downloaded (dfw setup --a11y, ~550 KB, once)","findings":[]}"#
        );
        let levels = [Some("critical"), Some("moderate"), Some("minor"), None].map(impact_level);
        let warn = Level::Warn.as_str();
        assert_eq!(
            levels,
            [Level::Error.as_str(), warn, Level::Info.as_str(), warn]
        );
        assert_eq!(
            crate::core::util::to_json_compact(
                &AxeStatus::Missing {
                    detail: NOT_DOWNLOADED
                }
                .to_json()
            ),
            r#"{"installed":false,"detail":"axe-core not downloaded (dfw setup --a11y, ~550 KB, once)"}"#
        );
    }
}
