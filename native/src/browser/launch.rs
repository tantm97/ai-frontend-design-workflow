//! Ports the browser half of `core/lib/cdp.mjs` (find Chrome / Edge / Chromium, start it headless
//! with a throwaway profile, read its DevTools URL, clean up whatever happens) and
//! `core/lib/runtime.mjs` (`open_renderer`, `fonts_ready`, `browser_status`).
//!
//! Over 800 lines with its tests: a third of the file is the fake browser of cdp.test.mjs and
//! the cases ported from it.

use std::collections::hash_map::RandomState;
use std::collections::HashMap;
use std::env;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStderr, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};

use super::cdp::{lock, Connection, Context, ContextOptions, Page};
use crate::core::error::DfwError;
use crate::core::paths::{env_nonempty, global_dir};

/* ---------------------------------------------------------------- finding a browser */

/// A browser dfw can drive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FoundBrowser {
    pub path: PathBuf,
    /// "env" (`DFW_BROWSER` / `CHROME_PATH`), "downloaded" (`dfw setup --browser`) or "system".
    pub source: &'static str,
    /// Known for the downloaded one only.
    pub version: Option<String>,
}

/// Where `dfw setup --browser` keeps the headless shell (`browserDir()`).
pub(crate) fn browser_dir() -> PathBuf {
    global_dir().join("browser")
}

/// The headless shell downloaded by "dfw setup --browser", if any.
fn downloaded_browser() -> Option<FoundBrowser> {
    let text = fs::read_to_string(browser_dir().join("current.json")).ok()?;
    let cur: Value = serde_json::from_str(&text).ok()?;
    let path = PathBuf::from(cur.get("path")?.as_str().filter(|p| !p.is_empty())?);
    path.exists().then(|| FoundBrowser {
        path,
        source: "downloaded",
        version: cur
            .get("version")
            .and_then(Value::as_str)
            .map(str::to_string),
    })
}

fn system_candidates() -> Vec<PathBuf> {
    let home = env::home_dir().unwrap_or_default();
    if cfg!(target_os = "macos") {
        let apps = [
            "Google Chrome",
            "Google Chrome Beta",
            "Google Chrome Dev",
            "Google Chrome Canary",
            "Chromium",
            "Microsoft Edge",
            "Brave Browser",
        ];
        return apps
            .iter()
            .flat_map(|a| {
                let rel = format!("Applications/{a}.app/Contents/MacOS/{a}");
                [Path::new("/").join(&rel), home.join(&rel)]
            })
            .collect();
    }
    if cfg!(windows) {
        let roots = ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"];
        let rels = [
            "Google\\Chrome\\Application\\chrome.exe",
            "Microsoft\\Edge\\Application\\msedge.exe",
            "Chromium\\Application\\chrome.exe",
            "BraveSoftware\\Brave-Browser\\Application\\brave.exe",
        ];
        return roots
            .iter()
            .filter_map(|r| env_nonempty(r))
            .flat_map(|r| rels.iter().map(move |x| Path::new(&r).join(x)))
            .collect();
    }
    let names = [
        "google-chrome",
        "google-chrome-stable",
        "chromium",
        "chromium-browser",
        "microsoft-edge",
        "microsoft-edge-stable",
        "brave-browser",
    ];
    let mut dirs: Vec<PathBuf> =
        env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect();
    dirs.extend(
        [
            "/usr/bin",
            "/usr/local/bin",
            "/snap/bin",
            "/opt/google/chrome",
        ]
        .map(PathBuf::from),
    );
    names
        .iter()
        .flat_map(|n| dirs.iter().map(move |d| d.join(n)))
        .collect()
}

/// First usable browser: `$DFW_BROWSER` (or `$CHROME_PATH`), then the downloaded headless shell,
/// then an installed browser.
pub fn find_browser() -> Option<FoundBrowser> {
    let from_env = env_nonempty("DFW_BROWSER").or_else(|| env_nonempty("CHROME_PATH"));
    if let Some(path) = from_env.map(PathBuf::from).filter(|p| p.exists()) {
        return Some(FoundBrowser {
            path,
            source: "env",
            version: None,
        });
    }
    downloaded_browser().or_else(|| {
        system_candidates()
            .into_iter()
            .find(|p| p.is_file())
            .map(|path| FoundBrowser {
                path,
                source: "system",
                version: None,
            })
    })
}

/// `browserStatus()`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserStatus {
    Installed(FoundBrowser),
    Missing { detail: &'static str },
}

impl BrowserStatus {
    /// `status.installed`.
    // For the next browser groups (layout check, a11y, extract, Penpot export, Studio).
    #[allow(dead_code)]
    pub fn installed(&self) -> bool {
        matches!(self, BrowserStatus::Installed(_))
    }
}

/// Whether a browser for renders is available, and which.
pub fn browser_status() -> BrowserStatus {
    match find_browser() {
        Some(b) => BrowserStatus::Installed(b),
        None => BrowserStatus::Missing {
            detail:
                "no Chrome, Edge or Chromium found (dfw setup --browser downloads a headless one)",
        },
    }
}

/* ---------------------------------------------------------------- launch */

const BROWSER_ARGS: [&str; 24] = [
    "--no-first-run",
    "--no-default-browser-check",
    "--disable-extensions",
    "--disable-background-networking",
    "--disable-component-update",
    "--disable-default-apps",
    "--disable-sync",
    "--disable-features=Translate,MediaRouter,OptimizationHints,DialMediaRouteProvider,CalculateNativeWinOcclusion,InterestFeedContentSuggestions",
    // Renders run in parallel tabs: none of them is "in the background".
    "--disable-background-timer-throttling",
    "--disable-backgrounding-occluded-windows",
    "--disable-renderer-backgrounding",
    "--disable-ipc-flooding-protection",
    "--disable-hang-monitor",
    "--disable-breakpad",
    "--disable-client-side-phishing-detection",
    "--disable-popup-blocking",
    "--no-service-autorun",
    // A throwaway profile must never touch the user's keychain / password store.
    "--use-mock-keychain",
    "--password-store=basic",
    "--mute-audio",
    "--hide-scrollbars",
    "--force-color-profile=srgb",
    "--allow-file-access-from-files",
    "about:blank",
];

/// Options of [`launch_with`]; the defaults are those of `launch()` in cdp.mjs.
#[derive(Debug, Clone, Default)]
pub struct LaunchOptions {
    /// How long the browser may take to start and accept the connection (default 20 s).
    pub timeout_ms: Option<u64>,
    /// Per-command timeout of the connection (default 120 s).
    pub command_timeout_ms: Option<u64>,
    /// Start this executable instead of the one [`find_browser`] picks.
    pub browser: Option<PathBuf>,
    /// Parent of the temporary profile (default: the system temporary directory).
    pub temp_dir: Option<PathBuf>,
    /// Extra environment variables for the browser process.
    pub env: Vec<(String, String)>,
}

/// A running headless browser (`class Browser`). Closing it (or dropping it) ends the process
/// and removes its profile.
pub struct Browser {
    proc: Mutex<Child>,
    conn: Arc<Connection>,
    data_dir: PathBuf,
    guard: Mutex<Option<Guard>>,
    closed: AtomicBool,
}

impl Browser {
    /// `newContext(opts)`.
    pub fn new_context(&self, opts: ContextOptions) -> Result<Context, DfwError> {
        Context::create(&self.conn, opts)
    }

    /// The DevTools connection, for browser-level commands.
    // For the next browser groups (layout check, a11y, extract, Penpot export, Studio).
    #[allow(dead_code)]
    pub fn connection(&self) -> &Arc<Connection> {
        &self.conn
    }

    /// Process id of the browser.
    // For the next browser groups (layout check, a11y, extract, Penpot export, Studio).
    #[allow(dead_code)]
    pub fn pid(&self) -> u32 {
        lock(&self.proc).id()
    }

    /// Whether the browser is ended and its profile removed if this process dies first.
    #[cfg(test)]
    pub fn guarded(&self) -> bool {
        lock(&self.guard).is_some()
    }

    /// `close()`: end the browser, remove its profile, drop the guard. Safe to call twice.
    pub fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let mut proc = lock(&self.proc);
            if matches!(proc.try_wait(), Ok(None)) {
                // Browser.close often gets no reply (the socket dies first): wait for the
                // process instead.
                let (tx, _) = mpsc::channel();
                let _ = self.conn.post("Browser.close", json!({}), None, tx);
                if !exited_within(&mut proc, 1500) {
                    let _ = proc.kill();
                    exited_within(&mut proc, 1500);
                }
            }
        }
        self.conn.close();
        // Our own temporary profile directory (created by make_temp_dir below).
        remove_dir(&self.data_dir);
        if let Some(guard) = lock(&self.guard).take() {
            guard.disarm();
        }
    }
}

impl Drop for Browser {
    fn drop(&mut self) {
        self.close();
    }
}

fn exited_within(proc: &mut Child, ms: u64) -> bool {
    let deadline = Instant::now() + Duration::from_millis(ms);
    loop {
        match proc.try_wait() {
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            Ok(None) => return false,
            _ => return true,
        }
    }
}

/// `rmSync(dir, { recursive, force, maxRetries: 3 })`: the browser's helpers may still be
/// writing for a moment after it ended.
fn remove_dir(dir: &Path) {
    for _ in 0..4 {
        match fs::remove_dir_all(dir) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => {
                thread::sleep(Duration::from_millis(100))
            }
            _ => return,
        }
    }
}

/// `fs.mkdtempSync(path.join(base, prefix))`: a new private directory with a random suffix.
pub(crate) fn make_temp_dir(base: &Path, prefix: &str) -> io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    for _ in 0..100 {
        // RandomState is keyed per process by the standard library; time and a counter make
        // every call different.
        let mut hasher = RandomState::new().build_hasher();
        hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        hasher.write_u128(nanos);
        let mut n = hasher.finish();
        let mut name = String::from(prefix);
        for _ in 0..6 {
            name.push(char::from(DIGITS[(n % 36) as usize]));
            n /= 36;
        }
        let dir = base.join(name);
        match private_dir_builder().create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free temporary directory name",
    ))
}

/// A directory only its owner can enter, as mkdtemp makes it.
#[cfg(unix)]
fn private_dir_builder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
}

#[cfg(not(unix))]
fn private_dir_builder() -> fs::DirBuilder {
    fs::DirBuilder::new()
}

/// Ends the browser and removes its profile when this process ends without `Browser::close`
/// (crash, kill, SIGTERM from "dfw studio stop" or the MCP tool timeout): `guardProcess`.
///
/// The standard library has no signal handlers, so a small `sh` watchdog holds the read end of
/// a pipe whose write end only this process has. When this process ends, for any reason, the
/// pipe closes and the watchdog cleans up. That also covers SIGKILL, which Node could not.
struct Guard {
    watchdog: Child,
}

// $1 = browser pid, $2 = profile directory. Signals of the terminal's process group must not
// end the watchdog before it has done its work.
#[cfg(unix)]
const WATCHDOG: &str = r#"trap '' INT HUP TERM
read -r _
kill "$1" 2>/dev/null
n=0
while kill -0 "$1" 2>/dev/null && [ "$n" -lt 30 ]; do sleep 0.1; n=$((n+1)); done
kill -9 "$1" 2>/dev/null
rm -rf "$2""#;

#[cfg(unix)]
fn guard_process(pid: u32, data_dir: &Path) -> Option<Guard> {
    Command::new("/bin/sh")
        .args(["-c", WATCHDOG, "dfw-browser-guard"])
        .arg(pid.to_string())
        .arg(data_dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()
        .map(|watchdog| Guard { watchdog })
}

/// No watchdog off POSIX: `Browser::close` and `Drop` cover every exit dfw makes itself.
#[cfg(not(unix))]
fn guard_process(_pid: u32, _data_dir: &Path) -> Option<Guard> {
    None
}

impl Guard {
    /// The undo: the browser was closed in the normal way. The watchdog is killed before its
    /// pipe closes, so it never acts on a process id that may belong to someone else by now.
    fn disarm(mut self) {
        let _ = self.watchdog.kill();
        let _ = self.watchdog.wait();
    }
}

/// `launch()`: start the browser [`find_browser`] picks and connect to it.
// For the next browser groups (layout check, a11y, extract, Penpot export, Studio).
#[allow(dead_code)]
pub fn launch() -> Result<Browser, DfwError> {
    launch_with(&LaunchOptions::default())
}

/// `launch({ timeoutMs, commandTimeoutMs })`: start a headless browser and connect to it.
pub fn launch_with(opts: &LaunchOptions) -> Result<Browser, DfwError> {
    let exe = match &opts.browser {
        Some(path) => path.clone(),
        None => find_browser().map(|b| b.path).ok_or_else(|| {
            DfwError::new("No Chrome, Edge or Chromium found. Install one, set DFW_BROWSER=/path/to/chrome, or run: dfw setup --browser")
        })?,
    };
    let timeout_ms = opts.timeout_ms.unwrap_or(20_000);
    let base = opts.temp_dir.clone().unwrap_or_else(env::temp_dir);
    let data_dir = make_temp_dir(&base, "dfw-browser-")?;
    let mut command = Command::new(&exe);
    command
        .args(["--headless", "--remote-debugging-port=0"])
        .arg(format!("--user-data-dir={}", data_dir.display()))
        .args(&BROWSER_ARGS[..BROWSER_ARGS.len() - 1]);
    if is_linux_root() {
        command.arg("--no-sandbox");
    }
    command
        .arg(BROWSER_ARGS[BROWSER_ARGS.len() - 1])
        .envs(opts.env.iter().map(|(k, v)| (k, v)))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut proc = match command.spawn() {
        Ok(proc) => proc,
        Err(e) => {
            remove_dir(&data_dir);
            return Err(DfwError::internal(format!(
                "cannot start {}: {e}",
                exe.display()
            )));
        }
    };
    let guard = guard_process(proc.id(), &data_dir);
    let connected = startup_url(&mut proc, timeout_ms, &exe)
        .and_then(|url| Connection::connect(&url, timeout_ms, opts.command_timeout_ms));
    match connected {
        Ok(conn) => Ok(Browser {
            proc: Mutex::new(proc),
            conn: Arc::new(conn),
            data_dir,
            guard: Mutex::new(guard),
            closed: AtomicBool::new(false),
        }),
        Err(e) => {
            // Never leave a browser or its temporary profile behind when the start fails.
            let _ = proc.kill();
            let _ = proc.wait();
            remove_dir(&data_dir);
            if let Some(guard) = guard {
                guard.disarm();
            }
            Err(e)
        }
    }
}

/// Chrome refuses to run as root with its sandbox on (containers, CI).
fn is_linux_root() -> bool {
    #[cfg(target_os = "linux")]
    {
        // /proc/self belongs to the effective user of this process.
        use std::os::unix::fs::MetadataExt;
        fs::metadata("/proc/self").is_ok_and(|m| m.uid() == 0)
    }
    #[cfg(not(target_os = "linux"))]
    false
}

enum Startup {
    Ready(String),
    /// stderr closed before the URL appeared; what was printed until then.
    Ended(String),
}

/// The `ws://` URL of "DevTools listening on ...", once the whole line is there.
fn devtools_url(buf: &str) -> Option<String> {
    const MARK: &str = "DevTools listening on ";
    let rest = &buf[buf.find("DevTools listening on ws://")? + MARK.len()..];
    rest.find(char::is_whitespace)
        .map(|end| rest[..end].to_string())
}

fn watch_stderr(mut stderr: ChildStderr) -> Receiver<Startup> {
    let (tx, rx) = mpsc::channel();
    let spawned = thread::Builder::new()
        .name("dfw-browser-stderr".into())
        .spawn(move || {
            let mut buf = String::new();
            let mut chunk = [0u8; 4096];
            let mut ready = false;
            loop {
                match stderr.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    // Keep draining after the URL so the browser never blocks on a full pipe.
                    Ok(_) if ready => {}
                    Ok(n) => {
                        buf.push_str(&String::from_utf8_lossy(&chunk[..n]));
                        if let Some(url) = devtools_url(&buf) {
                            ready = true;
                            let _ = tx.send(Startup::Ready(url));
                        }
                    }
                }
            }
            if !ready {
                let _ = tx.send(Startup::Ended(buf));
            }
        });
    // Without the thread the sender is dropped and the caller reports the start as timed out.
    drop(spawned);
    rx
}

/// Wait for the browser to print its DevTools URL (`connect()` in cdp.mjs, first half).
fn startup_url(proc: &mut Child, timeout_ms: u64, exe: &Path) -> Result<String, DfwError> {
    let deadline = Instant::now() + Duration::from_millis(timeout_ms);
    let late = || {
        DfwError::internal(format!(
            "browser did not start within {timeout_ms} ms ({})",
            exe.display()
        ))
    };
    let Some(stderr) = proc.stderr.take() else {
        return Err(late());
    };
    match watch_stderr(stderr).recv_timeout(Duration::from_millis(timeout_ms)) {
        Ok(Startup::Ready(url)) => Ok(url),
        Ok(Startup::Ended(printed)) => loop {
            match proc.try_wait() {
                Ok(Some(status)) => return Err(exited_early(status, &printed)),
                Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
                _ => return Err(late()),
            }
        },
        Err(_) => Err(late()),
    }
}

fn exited_early(status: ExitStatus, printed: &str) -> DfwError {
    let code = status
        .code()
        .map_or_else(|| "null".to_string(), |c| c.to_string());
    let lines: Vec<&str> = printed.trim().split('\n').collect();
    let tail = lines[lines.len().saturating_sub(3)..].join(" ");
    DfwError::internal(format!(
        "browser exited ({code}) before it was ready: {tail}"
    ))
}

/* ---------------------------------------------------------------- renderer (runtime.mjs) */

/// A browser plus one shared context per device scale (shared memory cache, cached fonts).
pub struct Renderer {
    browser: Browser,
    ctxs: Mutex<HashMap<u64, Arc<Context>>>,
}

impl Renderer {
    /// `r.page({ width, height, scale })`: a new blank tab with that viewport.
    pub fn page(&self, width: u32, height: u32, scale: f64) -> Result<Page, DfwError> {
        let ctx = {
            // Held while the context is created, so parallel callers share it.
            let mut ctxs = lock(&self.ctxs);
            match ctxs.get(&scale.to_bits()) {
                Some(ctx) => Arc::clone(ctx),
                None => {
                    let ctx = Arc::new(self.browser.new_context(ContextOptions {
                        device_scale_factor: scale,
                        ..ContextOptions::default()
                    })?);
                    ctxs.insert(scale.to_bits(), Arc::clone(&ctx));
                    ctx
                }
            }
        };
        let mut page = ctx.new_page()?;
        page.set_viewport_size(width, height)?;
        Ok(page)
    }

    /// The browser behind the renderer.
    // For the next browser groups (layout check, a11y, extract, Penpot export, Studio).
    #[allow(dead_code)]
    pub fn browser(&self) -> &Browser {
        &self.browser
    }

    /// `r.close()`: close the browser.
    pub fn close(&self) {
        self.browser.close();
    }
}

/// `openRenderer()`: start a browser for a batch of renders.
// For the next browser groups (layout check, a11y, extract, Penpot export, Studio).
#[allow(dead_code)]
pub fn open_renderer() -> Result<Renderer, DfwError> {
    open_renderer_with(&LaunchOptions::default())
}

/// [`open_renderer`] with launch options.
pub fn open_renderer_with(opts: &LaunchOptions) -> Result<Renderer, DfwError> {
    Ok(Renderer {
        browser: launch_with(opts)?,
        ctxs: Mutex::new(HashMap::new()),
    })
}

/// `fontsReady(page, ms)`: wait until web fonts are in (bounded: a missing font must not stall
/// a check). Node's default is 3000 ms.
pub fn fonts_ready(page: &Page, ms: u64) -> Result<(), DfwError> {
    const READY: &str = "(t) => Promise.race([document.fonts ? document.fonts.ready.then(() => true) : true, new Promise((r) => setTimeout(r, t))])";
    page.evaluate(READY, Some(&json!(ms))).map(|_| ())
}

#[cfg(all(test, unix))]
mod tests {
    //! tests/unit/cdp.test.mjs, against a fake browser: a shell script that prints the DevTools
    //! line and a WebSocket server in the test that speaks just enough of the protocol. It
    //! never answers Page.navigate or Runtime.evaluate, like a dev server that stalls or a page
    //! stuck in a loop. No Chrome needed.
    use super::*;
    use crate::browser::cdp::ShotOptions;
    use crate::core::util::test_dir;
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::fs::PermissionsExt;
    use tungstenite::Message;

    const FAKE: &str = r#"#!/bin/sh
echo $$ > "$DFW_FAKE_BROWSER_PID"
[ "$DFW_FAKE_BROWSER_MODE" = exit ] && exit 3
echo "DevTools listening on ws://127.0.0.1:$DFW_FAKE_BROWSER_PORT/devtools/browser/x" >&2
exec sleep 300
"#;

    struct Fake {
        tmp: PathBuf,
        pid_file: PathBuf,
        opts: LaunchOptions,
    }

    impl Fake {
        fn pid(&self) -> String {
            fs::read_to_string(&self.pid_file)
                .unwrap()
                .trim()
                .to_string()
        }
    }

    fn alive(pid: &str) -> bool {
        Command::new("kill")
            .args(["-0", pid])
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    }

    fn until(mut done: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if done() {
                return true;
            }
            thread::sleep(Duration::from_millis(25));
        }
        done()
    }

    fn serve(stream: TcpStream, pid_file: PathBuf) {
        let Ok(mut ws) = tungstenite::accept(stream) else {
            return;
        };
        while let Ok(message) = ws.read() {
            let Message::Text(text) = message else {
                continue;
            };
            let m: Value = serde_json::from_str(text.as_str()).unwrap();
            let result = match m["method"].as_str().unwrap() {
                "Page.navigate" | "Runtime.evaluate" => continue,
                "Browser.close" => {
                    let pid = fs::read_to_string(&pid_file).unwrap();
                    let _ = Command::new("kill").arg(pid.trim()).status();
                    continue;
                }
                "Target.createBrowserContext" => json!({ "browserContextId": "c1" }),
                "Target.createTarget" => json!({ "targetId": "t1" }),
                "Target.attachToTarget" => json!({ "sessionId": "s1" }),
                _ => json!({}),
            };
            let reply = json!({ "id": m["id"], "result": result }).to_string();
            if ws.send(Message::text(reply)).is_err() {
                return;
            }
        }
    }

    /// The fake browser as `LaunchOptions`, with an empty temp dir (so leftovers are visible).
    fn fake(mode: &str) -> Fake {
        let dir = test_dir("cdp");
        let exe = dir.join("fake-browser.sh");
        fs::write(&exe, FAKE).unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let tmp = dir.join("tmp");
        fs::create_dir(&tmp).unwrap();
        let pid_file = dir.join("pid");
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = if mode == "badws" {
            1
        } else {
            listener.local_addr().unwrap().port()
        };
        let for_server = pid_file.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let pid_file = for_server.clone();
                thread::spawn(move || serve(stream, pid_file));
            }
        });
        let env = [
            ("DFW_FAKE_BROWSER_MODE", mode.to_string()),
            ("DFW_FAKE_BROWSER_PID", pid_file.display().to_string()),
            ("DFW_FAKE_BROWSER_PORT", port.to_string()),
        ];
        Fake {
            opts: LaunchOptions {
                browser: Some(exe),
                temp_dir: Some(tmp.clone()),
                env: env.map(|(k, v)| (k.to_string(), v)).to_vec(),
                ..LaunchOptions::default()
            },
            tmp,
            pid_file,
        }
    }

    fn new_page(browser: &Browser) -> Page {
        let ctx = browser
            .new_context(ContextOptions {
                cache_fonts: false,
                ..ContextOptions::default()
            })
            .unwrap();
        ctx.new_page().unwrap()
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect()
    }

    /// cdp: a navigation that never answers is a caught timeout, not an unhandled rejection
    #[test]
    fn navigation_that_never_answers_is_a_timeout() {
        let f = fake("ok");
        let browser = launch_with(&f.opts).unwrap();
        assert!(
            browser.guarded(),
            "the browser is killed if this process ends first"
        );
        assert_eq!(leftovers(&f.tmp).len(), 1, "one temporary profile");
        let page = new_page(&browser);
        let t0 = Instant::now();
        let err = page.goto("http://127.0.0.1:9/slow", 200).unwrap_err();
        assert_eq!(
            err.message(),
            "timeout 200ms waiting for Page.loadEventFired"
        );
        assert!(
            t0.elapsed() < Duration::from_millis(2000),
            "goto returns at its timeout, not when Page.navigate answers"
        );
        browser.close();
        assert!(!browser.guarded(), "guard removed on close");
        assert!(until(|| !alive(&f.pid())), "browser process gone");
        assert_eq!(
            leftovers(&f.tmp),
            Vec::<String>::new(),
            "temporary profile removed"
        );
    }

    /// cdp: a command the browser never answers fails after the command timeout
    #[test]
    fn command_without_a_reply_fails_after_the_command_timeout() {
        let f = fake("ok");
        let browser = launch_with(&LaunchOptions {
            command_timeout_ms: Some(200),
            ..f.opts.clone()
        })
        .unwrap();
        let page = new_page(&browser);
        let err = page.evaluate("() => 1", None).unwrap_err();
        assert_eq!(
            err.message(),
            "Runtime.evaluate: no reply from the browser within 200 ms"
        );
        assert_eq!(
            page.send("Page.enable", json!({})).unwrap(),
            json!({}),
            "the connection still works"
        );
        // With a command timeout below the load timeout, the navigation fails on the former.
        let err = page.goto("http://127.0.0.1:9/slow", 30_000).unwrap_err();
        assert_eq!(
            err.message(),
            "Page.navigate: no reply from the browser within 200 ms"
        );
        // An answer without image data is an error, not an empty file.
        let err = page.screenshot(&ShotOptions::default()).unwrap_err();
        assert_eq!(err.message(), "Page.captureScreenshot: no image data");
        browser.close();
        assert!(page
            .send("Page.enable", json!({}))
            .is_err_and(|e| e.message() == "browser connection closed"));
    }

    /// cdp: a browser that fails to start leaves no process and no temporary profile
    #[test]
    fn failed_start_leaves_no_process_and_no_profile() {
        let f = fake("exit");
        let err = launch_with(&f.opts).err().unwrap();
        assert!(
            err.message()
                .starts_with("browser exited (3) before it was ready"),
            "{err}"
        );
        assert_eq!(leftovers(&f.tmp), Vec::<String>::new());

        let f = fake("badws");
        let err = launch_with(&LaunchOptions {
            timeout_ms: Some(5000),
            ..f.opts.clone()
        })
        .err()
        .unwrap();
        assert!(
            err.message().starts_with("cannot connect to the browser"),
            "{err}"
        );
        assert_eq!(leftovers(&f.tmp), Vec::<String>::new());
        assert!(until(|| !alive(&f.pid())), "browser process killed");

        let missing = launch_with(&LaunchOptions {
            browser: Some(f.tmp.join("no-such-browser")),
            ..f.opts.clone()
        })
        .err()
        .unwrap();
        assert!(missing.message().starts_with("cannot start "), "{missing}");
        assert_eq!(leftovers(&f.tmp), Vec::<String>::new());
    }

    #[test]
    fn a_browser_that_never_prints_its_url_times_out_and_is_killed() {
        let f = fake("ok");
        let exe = f.tmp.parent().unwrap().join("silent.sh");
        fs::write(
            &exe,
            "#!/bin/sh\necho $$ > \"$DFW_FAKE_BROWSER_PID\"\necho starting >&2\nexec sleep 300\n",
        )
        .unwrap();
        fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).unwrap();
        let err = launch_with(&LaunchOptions {
            browser: Some(exe.clone()),
            timeout_ms: Some(300),
            ..f.opts.clone()
        })
        .err()
        .unwrap();
        assert_eq!(
            err.message(),
            format!("browser did not start within 300 ms ({})", exe.display())
        );
        assert!(until(|| !alive(&f.pid())));
        assert_eq!(leftovers(&f.tmp), Vec::<String>::new());
    }

    /// The guard of cdp.mjs (exit and signal handlers), here a watchdog: when the owner ends
    /// without closing the browser, the browser is ended and its profile removed.
    #[test]
    fn guard_cleans_up_when_the_owner_dies() {
        let dir = test_dir("guard");
        let profile = dir.join("dfw-browser-test");
        fs::create_dir_all(profile.join("Default")).unwrap();
        let mut browser = Command::new("sleep").arg("300").spawn().unwrap();
        let mut guard = guard_process(browser.id(), &profile).unwrap();
        // What the kernel does when this process ends: the pipe's only write end closes.
        drop(guard.watchdog.stdin.take());
        assert!(guard.watchdog.wait().unwrap().success());
        assert!(until(|| matches!(browser.try_wait(), Ok(Some(_)))));
        assert!(!profile.exists());

        // Disarmed (normal close): nothing is touched afterwards.
        fs::create_dir_all(&profile).unwrap();
        let mut other = Command::new("sleep").arg("300").spawn().unwrap();
        guard_process(other.id(), &profile).unwrap().disarm();
        thread::sleep(Duration::from_millis(200));
        assert!(matches!(other.try_wait(), Ok(None)) && profile.exists());
        other.kill().unwrap();
        other.wait().unwrap();
    }

    #[test]
    fn devtools_url_needs_the_whole_line() {
        assert_eq!(
            devtools_url("DevTools listening on ws://127.0.0.1:9222/dev"),
            None
        );
        assert_eq!(
            devtools_url(
                "[1:2] warn\nDevTools listening on ws://127.0.0.1:9222/devtools/browser/ab-1\n"
            ),
            Some("ws://127.0.0.1:9222/devtools/browser/ab-1".to_string())
        );
        assert_eq!(devtools_url("nothing yet\n"), None);
    }

    #[test]
    fn temp_dirs_are_new_and_private() {
        let base = test_dir("mkdtemp");
        let a = make_temp_dir(&base, "dfw-browser-").unwrap();
        let b = make_temp_dir(&base, "dfw-browser-").unwrap();
        assert_ne!(a, b);
        let name = a.file_name().unwrap().to_str().unwrap();
        assert!(name.starts_with("dfw-browser-") && name.len() == "dfw-browser-".len() + 6);
        assert_eq!(
            fs::metadata(&a).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }
}
