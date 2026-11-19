//! Ports `screenshot(jobs, opts)` of `core/lib/runtime.mjs`: a list of screenshots in one
//! browser run, a few pages at a time.

use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::thread;

use super::cdp::{lock, ShotOptions};
use super::launch::{fonts_ready, open_renderer_with, LaunchOptions, Renderer};
use crate::core::error::DfwError;

/// Pages rendered at the same time unless the caller says otherwise.
pub const CONCURRENCY: usize = 6;

/// One screenshot to take (`{ url, width, height, out, fullPage, selector, scale }`).
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub url: String,
    pub width: u32,
    pub height: u32,
    /// The PNG file to write (folders are created).
    pub out: PathBuf,
    /// The whole document (true, the default in Node) or only the viewport.
    pub full_page: bool,
    /// Only this element.
    pub selector: Option<String>,
    /// Device scale factor (1 = CSS pixels).
    pub scale: f64,
}

/// The outcome of one job (`{ out, ok, error }`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShotResult {
    pub out: PathBuf,
    pub ok: bool,
    pub error: Option<String>,
}

/// `screenshot(jobs, { concurrency })`: results in the order of the jobs. A job that fails is
/// reported in its result; only a browser that cannot start is an error.
pub fn screenshot(jobs: &[Job], concurrency: usize) -> Result<Vec<ShotResult>, DfwError> {
    screenshot_with(jobs, concurrency, &LaunchOptions::default())
}

/// [`screenshot`] with launch options.
pub fn screenshot_with(
    jobs: &[Job],
    concurrency: usize,
    launch: &LaunchOptions,
) -> Result<Vec<ShotResult>, DfwError> {
    let renderer = open_renderer_with(launch)?;
    let results: Mutex<Vec<Option<ShotResult>>> = Mutex::new(vec![None; jobs.len()]);
    let next = AtomicUsize::new(0);
    thread::scope(|scope| {
        for _ in 0..concurrency.min(jobs.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::SeqCst);
                let Some(job) = jobs.get(i) else {
                    break;
                };
                let error = shoot(&renderer, job).err().map(|e| e.message().to_string());
                lock(&results)[i] = Some(ShotResult {
                    out: job.out.clone(),
                    ok: error.is_none(),
                    error,
                });
            });
        }
    });
    renderer.close();
    let results = results
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(results
        .into_iter()
        .zip(jobs)
        // A worker that panicked left its job without a result.
        .map(|(result, job)| {
            result.unwrap_or_else(|| ShotResult {
                out: job.out.clone(),
                ok: false,
                error: Some("screenshot worker stopped".to_string()),
            })
        })
        .collect())
}

fn shoot(renderer: &Renderer, job: &Job) -> Result<(), DfwError> {
    let page = renderer.page(job.width, job.height, job.scale)?;
    let shot = (|| {
        // "load" + fonts, not "network idle": the studio keeps a live-reload connection open.
        page.goto(&job.url, 30_000)?;
        fonts_ready(&page, 3000)?;
        page.wait_for_timeout(60);
        page.screenshot(&ShotOptions {
            path: Some(&job.out),
            full_page: job.full_page,
            selector: job.selector.as_deref(),
        })
    })();
    page.close();
    shot.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browser::pixeldiff::decode_rgba;
    use crate::core::util::test_dir;
    use std::process::Command;

    fn job(url: &str, out: PathBuf) -> Job {
        Job {
            url: url.to_string(),
            width: 320,
            height: 200,
            out,
            full_page: true,
            selector: None,
            scale: 1.0,
        }
    }

    /// Real browser: viewport, full page, element and scale screenshots, a failing job, and
    /// no browser or profile left behind.
    #[test]
    #[ignore = "needs Chrome"]
    fn screenshots_in_a_real_browser() {
        let dir = test_dir("shots");
        let tmp = dir.join("tmp");
        std::fs::create_dir(&tmp).unwrap();
        let page = "data:text/html,<body style='margin:0;background:%23fff'><div id='box' style='width:100px;height:50px;background:%23d03030'></div><div style='height:900px'></div></body>";
        let jobs = vec![
            job(page, dir.join("full.png")),
            Job {
                full_page: false,
                ..job(page, dir.join("deep/er/viewport.png"))
            },
            Job {
                selector: Some("#box".into()),
                ..job(page, dir.join("box.png"))
            },
            Job {
                selector: Some("#missing".into()),
                ..job(page, dir.join("missing.png"))
            },
            Job {
                full_page: false,
                scale: 2.0,
                ..job(page, dir.join("retina.png"))
            },
            job("http://127.0.0.1:9/refused", dir.join("refused.png")),
        ];
        let launch = LaunchOptions {
            temp_dir: Some(tmp.clone()),
            ..LaunchOptions::default()
        };
        let results = screenshot_with(&jobs, 3, &launch).unwrap();
        let ok: Vec<bool> = results.iter().map(|r| r.ok).collect();
        assert_eq!(ok, [true, true, true, false, true, false], "{results:?}");
        assert_eq!(
            results[3].error.as_deref(),
            Some("selector not found: #missing")
        );
        assert!(
            results[5]
                .error
                .as_deref()
                .unwrap()
                .starts_with("cannot open http://127.0.0.1:9/refused: net::ERR_"),
            "{results:?}"
        );
        let size = |name: &str| {
            let image = decode_rgba(&dir.join(name)).unwrap();
            (image.w, image.h)
        };
        assert_eq!(size("full.png"), (320, 950));
        assert_eq!(size("deep/er/viewport.png"), (320, 200));
        assert_eq!(size("box.png"), (100, 50));
        assert_eq!(size("retina.png"), (640, 400));
        let red = decode_rgba(&dir.join("box.png")).unwrap();
        assert_eq!(&red.rgba[..4], [0xd0, 0x30, 0x30, 0xff]);
        assert!(!dir.join("missing.png").exists());
        assert_eq!(
            std::fs::read_dir(&tmp).unwrap().count(),
            0,
            "profile removed"
        );
        let left = Command::new("pgrep")
            .args(["-f", &tmp.display().to_string()])
            .output()
            .unwrap();
        assert!(left.stdout.is_empty(), "browser processes left behind");
    }
}
