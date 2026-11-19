//! Render checks through the built binary: `layout check`, the stop hook's layout check,
//! `tokens extract` and the axe check. The tests that open a page need a real browser and are
//! ignored by default (`cargo test -- --ignored`); each gives the browser its own temporary
//! directory and fails when a browser process or profile is left behind. Nothing is downloaded:
//! axe-core is copied from this machine's dfw home when it is there.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::{json, Value};

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// A fresh folder with `project/` (the project root), `tmp/` (the browser's TMPDIR) and
/// `home/` (the dfw home).
fn sandbox(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-render-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    for sub in ["project/.dfw", "tmp", "home"] {
        fs::create_dir_all(dir.join(sub)).unwrap();
    }
    dir.canonicalize().unwrap()
}

fn write(root: &Path, rel: &str, text: &str) {
    let file = root.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, text).unwrap();
}

/// `dfw <args>` in the sandbox's project: (exit code, stdout, stderr).
fn dfw(sandbox: &Path, args: &[&str], stdin: &str) -> (i32, String, String) {
    let root = sandbox.join("project");
    let mut cmd = Command::new(DFW);
    cmd.args(args).current_dir(&root);
    for name in [
        "DFW_HOST",
        "CLAUDE_PROJECT_DIR",
        "DFW_SESSION_ID",
        "CLAUDE_ENV_FILE",
        "XDG_CONFIG_HOME",
    ] {
        cmd.env_remove(name);
    }
    cmd.env("DFW_HOME", sandbox.join("home"))
        .env("DFW_PROJECT_DIR", &root)
        .env("DFW_PLUGIN_ROOT", repo().join("core"))
        .env("DFW_NOW", "2026-01-02T03:04:05.678Z")
        .env("TMPDIR", sandbox.join("tmp"));
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// No browser profile and no browser process of this sandbox is left.
fn assert_no_browser_left(sandbox: &Path) {
    let tmp = sandbox.join("tmp");
    let profiles: Vec<_> = fs::read_dir(&tmp)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert!(profiles.is_empty(), "profiles left: {profiles:?}");
    // pgrep is a POSIX tool: on Windows the profile check above is the evidence.
    if cfg!(windows) {
        return;
    }
    let left = Command::new("pgrep")
        .args(["-f", &tmp.display().to_string()])
        .output()
        .unwrap();
    assert!(left.stdout.is_empty(), "browser processes left behind");
}

fn page(body: &str) -> String {
    format!("<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\"><title>T</title><style>body{{margin:0;font:16px/1.4 sans-serif}}</style></head><body>{body}</body></html>")
}

fn design(root: &Path, screens: &[(&str, &str)]) {
    let list: Vec<Value> = screens
        .iter()
        .map(|(id, _)| json!({ "id": id, "variants": [{ "id": "a", "file": format!("screens/{id}/a.html") }] }))
        .collect();
    let manifest = json!({ "schema": 1, "product": "T", "breakpoints": [{ "id": "mobile", "width": 390, "height": 844 }, { "id": "desktop", "width": 1440, "height": 900 }], "screens": list });
    write(root, "design/design.json", &manifest.to_string());
    for (id, html) in screens {
        write(root, &format!("design/screens/{id}/a.html"), html);
    }
}

fn findings(stdout: &str) -> Vec<Value> {
    let result: Value = serde_json::from_str(stdout).unwrap_or_else(|e| panic!("{e}: {stdout}"));
    assert_eq!(result["status"], "ran");
    result["findings"].as_array().unwrap().clone()
}

/// The axe-core of this machine's dfw home, copied into the sandbox's; false when there is none.
fn copy_axe(sandbox: &Path) -> bool {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let from = home.join(".config/dfw/tools/axe-core/4.14.0");
    if !from.join("axe.min.js").is_file() {
        return false;
    }
    let to = sandbox.join("home/tools/axe-core/4.14.0");
    fs::create_dir_all(&to).unwrap();
    for name in ["axe.min.js", "LICENSE"] {
        if from.join(name).is_file() {
            fs::copy(from.join(name), to.join(name)).unwrap();
        }
    }
    true
}

#[test]
fn usage_errors_need_no_browser() {
    let sb = sandbox("usage");
    let (code, _, stderr) = dfw(&sb, &["layout", "measure"], "");
    assert_eq!((code, stderr.as_str()), (1, "Usage: dfw layout check [--screen a,b] [--bp ids] [--states] [--no-stress] [--changed] [--no-system] [--json]\n"));
    let (code, _, stderr) = dfw(&sb, &["tokens", "extract"], "");
    assert_eq!((code, stderr.as_str()), (1, "Usage: dfw tokens extract --url <reference site or running app> [--out design/system/extracted.tokens.json]\n"));
}

/// Without axe-core the check is unverifiable, never a pass; no page is opened.
#[test]
fn a11y_without_axe_is_unverifiable() {
    let sb = sandbox("no-axe");
    let (code, stdout, stderr) = dfw(
        &sb,
        &["__debug", "a11y", "--url", "http://127.0.0.1:9/"],
        "",
    );
    assert_eq!(code, 0, "{stderr}");
    let result: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        (&result["tool"], &result["status"], &result["findings"]),
        (&json!("axe"), &json!("unverifiable"), &json!([]))
    );
    assert_no_browser_left(&sb);
}

/// layout: small tappable controls on touch breakpoints are a touch-target finding (needs Chrome)
#[test]
#[ignore = "needs Chrome"]
fn layout_small_tappable_controls_on_touch_breakpoints_are_a_touch_target_finding() {
    let sb = sandbox("touch");
    let root = sb.join("project");
    write(&root, ".dfw/state.json", "{}");
    fs::create_dir_all(root.join("design/tokens")).unwrap();
    write(
        &root,
        "design/design.json",
        r#"{"schema":1,"product":"T","screens":[]}"#,
    );
    let (code, _, stderr) = dfw(&sb, &["system", "init", "--preset", "crisp"], "");
    assert_eq!(code, 0, "{stderr}");
    design(&root, &[("s", "<!doctype html><html lang=\"en\"><head><dfw-head></dfw-head></head><body><main><button data-dfw-id=\"tiny\" style=\"height:30px;width:120px\">Small</button><button data-dfw-id=\"ok\" style=\"height:48px;width:120px\">Big</button><p>Read the <a href=\"#\" data-dfw-id=\"inline\">terms</a> first.</p></main></body></html>")]);
    let (_, stdout, stderr) = dfw(
        &sb,
        &["layout", "check", "--no-system", "--no-stress", "--json"],
        "",
    );
    let all = findings(&stdout);
    let touch: Vec<&str> = all
        .iter()
        .filter(|f| f["rule"] == "touch-target")
        .map(|f| f["message"].as_str().unwrap())
        .collect();
    assert_eq!(touch.len(), 1, "{all:?} {stderr}");
    assert!(
        touch[0].contains("@ mobile") && touch[0].contains("\"tiny\" 120x30"),
        "{touch:?}"
    );
    assert!(
        !touch[0].contains("\"main")
            && !touch[0].contains("\"ok\"")
            && !touch[0].contains("\"inline\"")
    );
    assert_no_browser_left(&sb);
}

/// Screens with one known problem each: the MUST rules end the command with 2 and leave the
/// design unmarked; a clean design is marked as checked.
#[test]
#[ignore = "needs Chrome"]
fn layout_check_finds_overlap_overflow_anchor_and_void() {
    let sb = sandbox("problems");
    let root = sb.join("project");
    design(&root, &[
        ("overlap", &page("<main><p data-dfw-id=\"first\" style=\"position:absolute;left:20px;top:20px;width:200px;height:40px;margin:0\">First block of text</p><p data-dfw-id=\"second\" style=\"position:absolute;left:60px;top:30px;width:200px;height:40px;margin:0\">Second block of text</p><div style=\"height:860px;background:#eee\"></div></main>")),
        ("overflow", &page("<main><div data-dfw-id=\"wide\" style=\"width:2000px;height:880px;background:#ccd\">Wider than any screen</div></main>")),
        ("anchor", &page("<main><div style=\"height:1500px;background:#eee\">Long content</div><button data-dfw-anchor=\"save\" style=\"height:48px;width:160px\">Save</button></main>")),
        ("void", &page("<main><p data-dfw-id=\"top\" style=\"margin:0;height:60px;background:#dde\">Top</p></main>")),
    ]);
    let (code, stdout, stderr) = dfw(&sb, &["layout", "check", "--json"], "");
    assert_eq!(code, 2, "{stderr}");
    let found: Vec<String> = findings(&stdout)
        .iter()
        .map(|f| {
            let at = f["message"].as_str().unwrap().split(':').next().unwrap();
            format!(
                "{} {} {at}",
                f["level"].as_str().unwrap(),
                f["rule"].as_str().unwrap()
            )
        })
        .collect();
    for expected in [
        "error overlap overlap/a @ desktop",
        "error h-overflow overflow/a @ mobile",
        "warn reflow overflow/a @ 320px, text 125%",
        "error anchor anchor/a @ desktop",
        "error anchor anchor/a @ desktop [stress",
        "warn void void/a @ desktop",
    ] {
        assert!(
            found.iter().any(|f| f == expected),
            "{expected}: {found:#?}"
        );
    }
    assert!(!root.join(".dfw/layout-cache.json").exists());
    let (code, stdout, _) = dfw(&sb, &["layout", "check", "--screen", "void"], "");
    assert_eq!(code, 0);
    assert!(
        stdout.starts_with("Layout: 5 render(s), 0 must, "),
        "{stdout}"
    );
    assert!(stdout.contains("- [SHOULD] (void) void/a @ mobile: content ends early: 784px empty at the bottom of the first screen"), "{stdout}");
    assert!(root.join(".dfw/layout-cache.json").exists());
    assert_eq!(
        dfw(&sb, &["layout", "check", "--changed"], "").1,
        "Layout: nothing changed since the last check.\n"
    );
    assert_no_browser_left(&sb);
}

/// hooks: stop re-renders what changed, blocks once on a MUST finding and reports each state of
/// the design once (no JavaScript unit test covers this part of `stop`).
#[test]
#[ignore = "needs Chrome"]
fn stop_hook_blocks_once_on_a_layout_must_finding() {
    let sb = sandbox("stop");
    let root = sb.join("project");
    write(
        &root,
        ".dfw/state.json",
        r#"{"schema":1,"phase":"concepts","active":true}"#,
    );
    design(&root, &[("wide", &page("<main><div data-dfw-id=\"wide\" style=\"width:2000px;height:880px;background:#ccd\">Wider than any screen</div></main>"))]);
    let payload = json!({ "hook_event_name": "Stop", "cwd": root }).to_string();
    let (code, stdout, stderr) = dfw(&sb, &["hook", "stop"], &payload);
    assert_eq!(code, 0, "{stderr}");
    let out: Value =
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("{e}: {stdout} {stderr}"));
    assert_eq!(out["decision"], "block");
    let reason = out["reason"].as_str().unwrap();
    assert!(
        reason.starts_with(
            "dfw layout check after your edits (1 screen(s)):\n- [MUST] (h-overflow) wide/a @ "
        ),
        "{reason}"
    );
    assert!(reason.ends_with("\nFix these in the design system (masters, page layout) where possible, re-run \"dfw layout check --changed\", then finish. See references/layout-patterns.md."));
    assert!(reason.lines().count() <= 17);
    // Reported once: the same design does not block the next stop.
    assert_eq!(
        dfw(&sb, &["hook", "stop"], &payload),
        (0, String::new(), String::new())
    );
    // Switched off: an edit is not checked, and stays unchecked.
    write(
        &root,
        ".dfw/config.json",
        r#"{"checks":{"layoutOnStop":false}}"#,
    );
    fs::write(
        root.join("design/screens/wide/a.html"),
        page("<main>changed</main>"),
    )
    .unwrap();
    assert_eq!(dfw(&sb, &["hook", "stop"], &payload).1, "");
    assert_eq!(
        dfw(&sb, &["layout", "check", "--changed", "--no-stress"], "")
            .1
            .lines()
            .next(),
        Some("Layout: 2 render(s), 0 must, 2 should.")
    );
    assert_no_browser_left(&sb);
}

/// `tokens extract --url` on a local page (no JavaScript unit test covers extract.mjs).
#[test]
#[ignore = "needs Chrome"]
fn tokens_extract_writes_a_seed_from_a_local_page() {
    let sb = sandbox("extract");
    let root = sb.join("project");
    write(&root, "site/index.html", &page("<header style=\"background:#0d6b78;color:#fff;padding:16px 24px\"><h1 style=\"font:600 24px Georgia,serif;margin:0\">Shop</h1></header><main style=\"padding:24px\"><button style=\"border:1px solid #cbd5e1;border-radius:6px;padding:8px 12px;background:#f8fafc\">Buy</button></main>"));
    let url = format!("file://{}/site/index.html", root.display());
    let (code, stdout, stderr) = dfw(&sb, &["tokens", "extract", "--url", &url], "");
    assert_eq!(code, 0, "{stderr}");
    assert!(
        stdout.starts_with("Wrote design/system/extracted.tokens.json: "),
        "{stdout}"
    );
    assert!(stdout.ends_with(
        " spacing values. It is a seed: review it with the user before using any of it.\n"
    ));
    let text = fs::read_to_string(root.join("design/system/extracted.tokens.json")).unwrap();
    assert!(text.ends_with("}\n"));
    let seed: Value = serde_json::from_str(&text).unwrap();
    assert!(seed["$description"].as_str().unwrap().starts_with(&format!(
        "Extracted from {url} on 2026-01-02 by \"dfw tokens extract\"."
    )));
    let keys: Vec<&String> = seed.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["$description", "color", "font", "radius", "space"]);
    let teal = seed["color"]
        .as_object()
        .unwrap()
        .values()
        .find(|c| c["$value"]["hex"] == "#0d6b78")
        .unwrap();
    assert_eq!(teal["$value"]["components"], json!([0.051, 0.4196, 0.4706]));
    assert_eq!(
        teal["$extensions"]["dfw.extracted"]["roles"],
        json!(["background"])
    );
    assert_eq!(
        seed["radius"]["r1"]["$value"],
        json!({ "value": 6, "unit": "px" })
    );
    // A page that cannot be opened is an error, and the browser is closed all the same.
    let (code, _, stderr) = dfw(
        &sb,
        &[
            "tokens",
            "extract",
            "--url",
            "http://127.0.0.1:9/",
            "--out",
            "x.json",
        ],
        "",
    );
    assert_eq!(code, 1);
    assert!(
        stderr.starts_with("Error: cannot open http://127.0.0.1:9/: net::ERR_"),
        "{stderr}"
    );
    assert!(!root.join("x.json").exists());
    assert_no_browser_left(&sb);
}

/// axe on a page with known violations, and the dead-control probe (no JavaScript unit test
/// covers a11y.mjs). Runs the check only when axe-core is on this machine.
#[test]
#[ignore = "needs Chrome"]
fn a11y_check_reports_known_violations_and_dead_controls() {
    let sb = sandbox("axe");
    if !copy_axe(&sb) {
        eprintln!("skipped: axe-core 4.14.0 is not in ~/.config/dfw (dfw setup --a11y)");
        return;
    }
    let root = sb.join("project");
    write(&root, "bad.html", &page("<main><h1>Orders</h1><img src=\"data:image/gif;base64,R0lGODlhAQABAAAAACw=\" width=\"40\" height=\"40\"><form><input type=\"text\" name=\"q\"></form><p style=\"color:#bbb;background:#fff\">Hard to read</p><button aria-pressed=\"false\">Dead toggle</button></main>"));
    let url = format!("file://{}/bad.html", root.display());
    let (code, stdout, stderr) = dfw(&sb, &["__debug", "a11y", "--url", &url], "");
    assert_eq!(code, 0, "{stderr}");
    let all = findings(&stdout);
    let rules: Vec<String> = all
        .iter()
        .map(|f| {
            format!(
                "{} {}",
                f["level"].as_str().unwrap(),
                f["rule"].as_str().unwrap()
            )
        })
        .collect();
    for expected in [
        "error axe-image-alt",
        "error axe-label",
        "error axe-color-contrast",
        "warn dead-control",
    ] {
        assert!(rules.iter().any(|r| r == expected), "{expected}: {rules:?}");
    }
    let first = &all[0];
    assert_eq!(
        (&first["file"], &first["line"], &first["source"]),
        (&json!(url), &json!(1), &json!("axe"))
    );
    assert!(first["message"]
        .as_str()
        .unwrap()
        .starts_with(&format!("{url}: ")));
    // `setup --a11y` with axe in place answers without a download.
    let (code, stdout, _) = dfw(&sb, &["setup", "--a11y"], "");
    let status: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(
        (code, &status["installed"], &status["version"]),
        (0, &json!(true), &json!("4.14.0"))
    );
    assert_no_browser_left(&sb);
}
