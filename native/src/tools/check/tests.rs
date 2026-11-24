//! Tests of the check runner. No test starts the real Impeccable engine, sloplint or copy-lint:
//! the tools are stand-in scripts in a temporary folder, found through an injected lookup.
// Over 800 lines: every tool and every way it can fail runs through a stand-in, with the
// stand-ins' texts and the expected findings written out.

use std::cell::{Cell, RefCell};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::report::{flatten, locale_compare, tool_status};
use super::*;
use crate::core::config::load_config;
use crate::core::fixture::{
    APP_COMPONENT_TEST, APP_DIR, BUTTON_MASTER, DESIGN_VUE, HTML_FILE, INDEX_PAGE, INDEX_PAGE_B,
    INDEX_SCREEN, MISSING_HTML_FILE, NOTES_FILE,
};
#[cfg(unix)]
use crate::core::fixture::{APP_MODULE, CHIP_MASTER, DESIGN_PAGE};
use crate::core::fixture::{HTML_FILE_B, OUTSIDE_PAGE};
use crate::core::util::test_dir;
use field::STATUS;
#[cfg(unix)]
use field::{DETAIL, FINDINGS};
#[cfg(unix)]
use outcome::UNAVAILABLE;
use outcome::{ERROR, NOT_INSTALLED, RAN, SKIPPED, TIMEOUT};
use tool::{DFW_LINT, IMPECCABLE, SLOPLINT};
#[cfg(unix)]
use tool::{DS_AUDIT, TASTE_COPY_LINT};

/// The levels of findings, by the label the report gives them.
const MUST: &str = Level::Error.as_str();
const SHOULD: &str = Level::Warn.as_str();
const NICE: &str = Level::Info.as_str();

fn write(root: &Path, rel: &str, text: &str) -> PathBuf {
    let file = root.join(rel);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(&file, text).unwrap();
    file
}

/// An executable stand-in for a tool.
#[cfg(unix)]
fn script(root: &Path, rel: &str, body: &str) -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let file = write(root, rel, &format!("#!/bin/sh\n{body}\n"));
    fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
    file
}

fn strings(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

#[cfg_attr(not(unix), allow(dead_code, reason = "used by the Unix-only tests"))]
fn no_env() -> ToolRun<'static> {
    ToolRun {
        env: &[],
        timeout_ms: 5000,
    }
}

/// A lookup with no tool installed; a test sets the ones it fakes.
fn nothing<'a>() -> Lookup<'a> {
    Lookup {
        engine: &|| Ok(None),
        sloplint: &|| Ok(None),
        taste: &|| Ok(None),
        node: &|| None,
    }
}

/// The scripts are sh scripts, so "Node.js" is sh in these tests.
#[cfg_attr(not(unix), allow(dead_code, reason = "used by the Unix-only tests"))]
fn sh() -> Option<String> {
    Some("/bin/sh".to_string())
}

#[cfg_attr(not(unix), allow(dead_code, reason = "used by the Unix-only tests"))]
fn ready(cmd: &Path, env: &[(&str, &str)]) -> Engine {
    Engine {
        ready: true,
        verified: true,
        cmd: Some(cmd.to_path_buf()),
        env: env
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..Engine::default()
    }
}

fn not_ready(unverified: &[&str]) -> Engine {
    Engine {
        launcher: "/s/impeccable".into(),
        unverified: unverified.iter().map(PathBuf::from).collect(),
        detail: Some("Engine not downloaded yet; run \"dfw check --setup\" once.".into()),
        ..Engine::default()
    }
}

#[cfg_attr(not(unix), allow(dead_code, reason = "used by the Unix-only tests"))]
fn statuses(results: &[Value]) -> Vec<String> {
    results
        .iter()
        .map(|r| {
            tool_status(
                r[field::TOOL].as_str().unwrap(),
                r[STATUS].as_str().unwrap(),
            )
        })
        .collect()
}

/// [`statuses`] of results with these tools and statuses.
#[cfg(unix)]
fn want(pairs: &[(&str, &str)]) -> Vec<String> {
    pairs.iter().map(|(t, o)| tool_status(t, o)).collect()
}

/// 2: lint targets written as ./design/..., or absolute, are linted (no false pass)
/// (tests/unit/audit-fixes.test.mjs)
#[test]
fn lint_targets_in_every_form_are_linted() {
    let root = test_dir("check-targets");
    write(&root, rel::MANIFEST, r#"{"screens":[]}"#);
    write(
        &root,
        &rel::screen(INDEX_PAGE),
        "<html><head></head><body><img src=\"x.png\"><h1>Lorem ipsum</h1></body></html>",
    );
    let config = load_config(Some(&root)).unwrap();
    let rules = |target: &str| -> Vec<String> {
        let opts = CheckOptions {
            timeout_ms: None,
            tools: Some(&["dfw"]),
        };
        let results =
            run_checks_with(&root, Some(&strings(&[target])), &config, opts, &nothing()).unwrap();
        flatten(&results)
            .iter()
            .map(|f| f["rule"].as_str().unwrap().to_string())
            .collect()
    };
    let want = rules(&rel::screen(INDEX_PAGE));
    assert!(want.iter().any(|r| r == "img-alt"));
    assert_eq!(rules("./design/screens/a/index.html"), want);
    let abs = root.join(rel::screen(INDEX_PAGE));
    assert_eq!(rules(abs.to_str().unwrap()), want);
}

/// check: an unverified engine is "not-installed"; a verified one is run directly with the
/// skill dir (tests/unit/impeccable-hardening.test.mjs on fix/impeccable-hardening)
#[cfg(unix)]
#[test]
fn an_unverified_engine_is_not_installed_and_a_verified_one_runs_directly() {
    let root = test_dir("check-impeccable");
    let targets = strings(&[HTML_FILE]);
    let none = run_impeccable(&root, &targets, no_env(), Some(&not_ready(&[])));
    assert_eq!(none[STATUS], NOT_INSTALLED);
    assert_eq!(none[FINDINGS], json!([]));
    assert!(none[DETAIL].as_str().unwrap().contains("dfw check --setup"));
    assert_eq!(
        run_impeccable(&root, &targets, no_env(), None)[STATUS],
        UNAVAILABLE
    );

    // A stand-in script, not the Impeccable engine: it reports the environment it was started with.
    let stub = script(
        &root,
        "stub",
        r#"printf '[{"antipattern":"env","name":"%s","description":"%s","severity":"warning","file":"a.html"}]' "$IMPECCABLE_SKILL_DIR" "$DO_NOT_TRACK""#,
    );
    let env = [("DO_NOT_TRACK".to_string(), "1".to_string())];
    let opts = ToolRun {
        env: &env,
        timeout_ms: 5000,
    };
    let engine = ready(&stub, &[("IMPECCABLE_SKILL_DIR", "/skill")]);
    let ran = run_impeccable(&root, &targets, opts, Some(&engine));
    assert_eq!(ran[STATUS], RAN);
    assert_eq!(
        ran[FINDINGS][0],
        json!({ "level": SHOULD, "rule": "impeccable-env", "file": HTML_FILE, "line": 1, "message": "/skill: 1", "source": IMPECCABLE })
    );
}

/// check --setup: the launcher only downloads, and the result must verify
/// (tests/unit/impeccable-hardening.test.mjs on fix/impeccable-hardening)
#[test]
fn setup_only_downloads_and_the_result_must_verify() {
    /// The page the launcher is pointed at to download the engine.
    const PAGE: &str = "/p/setup.html";
    type Call = (Spawn, Vec<(String, String)>);
    let calls: RefCell<Vec<Call>> = RefCell::new(Vec::new());
    let launch = |command: &Spawn, env: &[(String, String)]| {
        calls.borrow_mut().push((command.clone(), env.to_vec()));
        Launched {
            stderr: "curl: no network".into(),
            error: None,
        }
    };
    let is_ready = Engine {
        ready: true,
        ..Engine::default()
    };

    let done = setup_engine(&[], "", &|| Ok(Some(is_ready.clone())), &launch).unwrap();
    assert_eq!(
        done,
        Setup {
            ok: true,
            text: "Impeccable engine ready.".into()
        }
    );
    assert_eq!(
        calls.borrow().len(),
        0,
        "a verified engine needs no launcher run"
    );

    let unverifiable = not_ready(&["/usr/local/bin/impeccable"]);
    let stopped = setup_engine(&[], "", &|| Ok(Some(unverifiable.clone())), &launch).unwrap();
    assert!(!stopped.ok);
    assert!(stopped
        .text
        .contains("cannot verify (/usr/local/bin/impeccable)"));
    assert!(stopped
        .text
        .ends_with("allowUnverifiedEngine true --global"));
    assert_eq!(
        calls.borrow().len(),
        0,
        "the launcher would run that binary, so it is not started"
    );

    let asked = Cell::new(0);
    let download = || {
        asked.set(asked.get() + 1);
        Ok(Some(if asked.get() == 1 {
            not_ready(&[])
        } else {
            is_ready.clone()
        }))
    };
    let env = [("DO_NOT_TRACK".to_string(), "1".to_string())];
    assert!(setup_engine(&env, PAGE, &download, &launch).unwrap().ok);
    assert_eq!(calls.borrow().len(), 1);
    let (command, given) = calls.borrow()[0].clone();
    if !cfg!(windows) {
        assert_eq!(command.cmd, "/s/impeccable");
        assert_eq!(command.args, ["detect", "--json", PAGE]);
    }
    assert_eq!(given, env);

    let failed = setup_engine(&[], PAGE, &|| Ok(Some(not_ready(&[]))), &launch).unwrap();
    assert_eq!(
        failed,
        Setup {
            ok: false,
            text: "Impeccable setup failed: curl: no network".into()
        },
        "exit 0 from the launcher is not enough"
    );
    let missing = setup_engine(&[], "", &|| Ok(None), &launch).unwrap();
    assert_eq!(
        missing,
        Setup {
            ok: false,
            text: "Impeccable plugin not found.".into()
        }
    );
    // Nothing on stderr: the reason is why the launcher did not start, then the engine's detail.
    let silent = |error: Option<&str>| {
        let launch = |_: &Spawn, _: &[(String, String)]| Launched {
            stderr: String::new(),
            error: error.map(String::from),
        };
        setup_engine(&[], "", &|| Ok(Some(not_ready(&[]))), &launch)
            .unwrap()
            .text
    };
    assert_eq!(
        silent(Some("spawnSync /s/impeccable ENOENT")),
        "Impeccable setup failed: spawnSync /s/impeccable ENOENT"
    );
    assert_eq!(
        silent(None),
        "Impeccable setup failed: Engine not downloaded yet; run \"dfw check --setup\" once."
    );
}

/// check: unverified tools are listed, and --strict turns them into exit 3
/// (tests/unit/impeccable-hardening.test.mjs on fix/impeccable-hardening)
#[test]
fn unverified_tools_are_listed_and_strict_exits_3() {
    let results = vec![
        json!({ "tool": DFW_LINT, "status": RAN, "findings": [] }),
        json!({ "tool": IMPECCABLE, "status": NOT_INSTALLED, "detail": "run \"dfw check --setup\" once", "findings": [] }),
        json!({ "tool": SLOPLINT, "status": SKIPPED, "findings": [] }),
    ];
    let report = format_report(&results, 50);
    assert_eq!(report.unverified, [IMPECCABLE]);
    assert_eq!(
        check_exit_code(&report, false),
        0,
        "default exit code does not change"
    );
    assert_eq!(check_exit_code(&report, true), 3);
    let mut with_must = results.clone();
    with_must.push(json!({ "tool": "x", "status": RAN, "findings": [{ "level": MUST, "file": "a", "line": 1, "rule": "r", "source": "x", "message": "m" }] }));
    let must = format_report(&with_must, 50);
    assert_eq!(check_exit_code(&must, false), 2);
    assert_eq!(check_exit_code(&must, true), 2, "MUST findings keep exit 2");
    let ran = format_report(&[results[0].clone(), results[2].clone()], 50);
    assert_eq!(
        check_exit_code(&ran, true),
        0,
        "skipped is not \"did not run\""
    );
}

#[test]
fn the_report_sorts_caps_and_names_what_did_not_run() {
    let finding = |level: &str, file: &str, line: u64| json!({ "level": level, "rule": "r", "file": file, "line": line, "message": format!("{level} {file}:{line}"), "source": "s" });
    let results = vec![
        json!({ "tool": "a", "status": RAN, "detail": "pinned 1.2", "findings": [finding(NICE, HTML_FILE_B, 1), finding(SHOULD, HTML_FILE_B, 9), finding(MUST, "z.html", 2), finding(SHOULD, "B.html", 3), finding(SHOULD, "a-b.html", 1)] }),
        json!({ "tool": "b", "status": TIMEOUT, "findings": [finding(MUST, HTML_FILE, 12), finding(MUST, HTML_FILE, 3), finding(SHOULD, HTML_FILE_B, 10)] }),
        json!({ "tool": "c", "status": ERROR, "detail": "boom", "findings": [] }),
        json!({ "tool": "d", "status": SKIPPED, "findings": [] }),
    ];
    let report = format_report(&results, 50);
    assert_eq!(
        report.text,
        "Checks: 3 must, 4 should, 1 nice. Tools: a=ran, b=timeout, c=error, d=skipped\n\
- [MUST] a.html:3 (s/r) error a.html:3\n\
- [MUST] a.html:12 (s/r) error a.html:12\n\
- [MUST] z.html:2 (s/r) error z.html:2\n\
- [SHOULD] a-b.html:1 (s/r) warn a-b.html:1\n\
- [SHOULD] b.html:9 (s/r) warn b.html:9\n\
- [SHOULD] b.html:10 (s/r) warn b.html:10\n\
- [SHOULD] B.html:3 (s/r) warn B.html:3\n\
- [NICE] b.html:1 (s/r) info b.html:1\n\
NOT VERIFIED: b (timeout); c (boom). Say so in any report; do not claim these aspects pass.\n  note: a: pinned 1.2"
    );
    assert_eq!(report.tally(), json!({ "error": 3, "warn": 4, "info": 1 }));
    assert_eq!(report.unverified, ["b", "c"]);
    assert_eq!(report.findings.len(), 8);
    let capped = format_report(&results, 2);
    assert_eq!(capped.text.lines().count(), 6);
    assert!(capped
        .text
        .contains("\n- ... 6 more (run \"dfw check --json\" for all).\n"));
    assert_eq!(
        format_report(&[], 50).text,
        "Checks: 0 must, 0 should, 0 nice. Tools: "
    );
}

/// The order `String.prototype.localeCompare` gives in Node 22 for these pairs.
#[test]
fn file_names_sort_as_locale_compare_sorts_them() {
    use std::cmp::Ordering::{Equal, Greater, Less};
    /// Names that come in two pairs each: with a space, a dash, a two-digit number.
    const SPACED: &str = "a b";
    const DASHED: &str = "a-b";
    const TEN: &str = "a10";
    for (a, b, want) in [
        ("a", "A", Less),
        ("ab", "Aa", Greater),
        (SPACED, "ab", Less),
        (DASHED, "ab", Less),
        ("a/b", DASHED, Greater),
        ("a\tb", "ab", Less),
        ("a\u{1}b", "ab", Equal),
        ("Ab", "aB", Greater),
        ("a1", TEN, Less),
        ("a2", TEN, Greater),
        ("a.html", "a/x", Less),
        (&rel::design(HTML_FILE), "DESIGN.md", Greater),
        ("", "a", Less),
        ("a_b", SPACED, Greater),
        ("ab", "a\u{1}c", Less),
        ("é", "f", Greater), // Node: before "f"; letters outside ASCII sort last here.
    ] {
        assert_eq!(locale_compare(a, b), want, "{a:?} {b:?}");
    }
    let mut ascii: Vec<String> = (32u8..127).map(|b| (b as char).to_string()).collect();
    ascii.sort_by(|a, b| locale_compare(a, b));
    assert_eq!(
        ascii.concat(),
        " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$0123456789aAbBcCdDeEfFgGhHiIjJkKlLmMnNoOpPqQrRsStTuUvVwWxXyYzZ"
    );
}

#[test]
fn extname_is_the_node_extname() {
    /// The extension of a page.
    const HTML: &str = ".html";
    assert_eq!(extname(&rel::screen(INDEX_PAGE)), HTML);
    assert_eq!(extname(APP_COMPONENT_TEST), ".tsx");
    /// A file whose name starts with its only dot.
    const DOTFILE: &str = ".env";
    assert_eq!(extname(&rel::join(APP_DIR, DOTFILE)), "");
    assert_eq!(extname("a.b/readme"), "");
    assert_eq!(extname("notes."), ".");
    assert_eq!(extname(".."), "");
    assert_eq!(extname(""), "");
    assert_eq!(extname("https://example.com/a.html"), HTML);
}

#[test]
fn loose_json_starts_at_the_first_bracket() {
    assert_eq!(
        parse_json_loose("warning: x\n[{\"a\":1}]\n"),
        Some(json!([{ "a": 1 }]))
    );
    assert_eq!(
        parse_json_loose("note {\"a\": [1]}"),
        Some(json!({ "a": [1] }))
    );
    assert_eq!(parse_json_loose("no json"), None);
    assert_eq!(parse_json_loose("[1] trailing"), None);
    assert_eq!(parse_json_loose(""), None);
}

#[test]
fn targets_are_made_project_relative() {
    let root = test_dir("check-rel");
    write(&root, &rel::design(HTML_FILE), "x");
    assert_eq!(
        project_rel(&root, &rel::design(HTML_FILE)),
        rel::design(HTML_FILE)
    );
    assert_eq!(
        project_rel(&root, "./design/../design/a.html"),
        rel::design(HTML_FILE)
    );
    assert_eq!(
        project_rel(&root, root.join(rel::design(HTML_FILE)).to_str().unwrap()),
        rel::design(HTML_FILE)
    );
    assert_eq!(
        project_rel(&root, &rel::design(MISSING_HTML_FILE)),
        rel::design(MISSING_HTML_FILE)
    );
    let above = format!("../{OUTSIDE_PAGE}");
    assert_eq!(project_rel(&root, &above), above);
    // The same file reached through a symlinked directory.
    #[cfg(unix)]
    {
        let link = test_dir("check-rel-link").join("alias");
        std::os::unix::fs::symlink(&root, &link).unwrap();
        assert_eq!(
            project_rel(&root, link.join(rel::design(HTML_FILE)).to_str().unwrap()),
            rel::design(HTML_FILE)
        );
    }
}

#[test]
fn the_design_walk_skips_snapshots_handoff_and_masters() {
    /// A page of the handoff.
    const HANDOFF_PAGE: &str = "v1.html";
    let root = test_dir("check-walk");
    for rel in [
        &rel::screen(INDEX_PAGE_B),
        &rel::screen(INDEX_PAGE),
        &rel::screen(&rel::join(INDEX_SCREEN, NOTES_FILE)),
        &rel::approved("v1/screens/a/index.html"),
        &rel::join(rel::HANDOFF, HANDOFF_PAGE),
        &rel::component(BUTTON_MASTER),
        rel::COMPONENTS_CSS,
        rel::SYSTEM_PAGE,
        &rel::design(DESIGN_VUE),
        &rel::join(rel::TOKENS, file::TOKENS_CSS),
        rel::TOKENS_JSON,
    ] {
        write(&root, rel, "x");
    }
    assert_eq!(
        list_design_ui(&root).unwrap(),
        [
            &rel::design(DESIGN_VUE),
            &rel::screen(INDEX_PAGE),
            &rel::screen(INDEX_PAGE_B),
            rel::COMPONENTS_CSS,
            rel::SYSTEM_PAGE,
            &rel::join(rel::TOKENS, file::TOKENS_CSS),
        ]
    );
    assert!(list_design_ui(&test_dir("check-walk-empty"))
        .unwrap()
        .is_empty());
}

/// A project with one master, one screen that uses it and one plain page.
#[cfg(unix)]
fn project(label: &str) -> PathBuf {
    let root = test_dir(label);
    write(&root, rel::STATE, "{}");
    write(&root, rel::MANIFEST, r#"{"screens":[]}"#);
    write(&root, &rel::component(CHIP_MASTER), "<dfw-component name=\"chip\">\n<style>.chip { display: inline-flex; }</style>\n<template>\n<span class=\"chip\"><dfw-slot></dfw-slot></span>\n</template>\n</dfw-component>\n");
    write(&root, &rel::screen(INDEX_PAGE), "<!doctype html><html lang=\"en\"><head><dfw-head></dfw-head></head><body><main data-dfw-id=\"m\"><dfw-use c=\"chip\" id=\"m.c\">New</dfw-use></main></body></html>");
    write(&root, &rel::design(DESIGN_PAGE), "<!doctype html><html lang=\"en\"><head><link rel=\"stylesheet\" href=\"../tokens/tokens.css\"></head><body><main data-dfw-id=\"m\">x</main></body></html>");
    write(&root, &rel::design(NOTES_FILE), "# notes");
    root
}

/// Every tool runs on composed copies, its findings point back at the sources, and each gets
/// the configured environment.
#[cfg(unix)]
#[test]
fn external_tools_run_on_composed_copies_and_findings_point_at_the_sources() {
    let root = project("check-external");
    let tools = test_dir("check-external-tools");
    // Each stand-in records its arguments and reports one finding on its first file.
    let engine_stub = script(
        &tools,
        "engine",
        r#"echo "$@" > "$0.args"; echo "$IMPECCABLE_SELF $DO_NOT_TRACK" > "$0.env"
printf 'update notice\n[{"antipattern":"flat","name":"Flat","description":"no depth","severity":"critical","file":"%s","line":7,"snippet":"<main>"},{"antipattern":"adv","name":"N","description":"d","advisory":true,"severity":"critical","file":"%s"}]' "$PWD/$3" "$PWD/$4""#,
    );
    let sloplint = script(
        &tools,
        "sloplint.mjs",
        r#"echo "$@" > "$0.args"
printf '{"findings":[{"gate":"3","grade":"FAIL","file":"%s","line":4,"evidence":"generic hero","fix":"rewrite"},{"gate":20,"grade":"FAIL","file":"%s","evidence":"stamp"},{"gate":"5","grade":"NOTE","evidence":"ignored"},{"gate":"6","grade":"REFLEX","evidence":"reflex"}]}' "$1" "$1""#,
    );
    let taste = script(
        &tools,
        "copy-lint.mjs",
        r#"echo "$@" > "$0.args"
printf '{"files":[{"findings":[{"id":"T1","sev":"FAIL","msg":"vague headline","fix":"be specific"},{"id":"T2","sev":"INFO"},{"id":"T3","sev":"WARN"}]},{"findings":[{"id":"T4","sev":"FAIL"}]}]}'"#,
    );
    let engine = ready(
        &engine_stub,
        &[("IMPECCABLE_SELF", "/skill/scripts/impeccable")],
    );
    let lookup = Lookup {
        engine: &|| Ok(Some(engine.clone())),
        sloplint: &|| Ok(Some(sloplint.clone())),
        taste: &|| Ok(Some(taste.clone())),
        node: &sh,
    };
    let config = json!({ "checks": { "tasteCopyLint": true }, "env": { "DO_NOT_TRACK": "1" } });
    let results = run_checks_with(&root, None, &config, CheckOptions::default(), &lookup).unwrap();
    assert_eq!(
        statuses(&results),
        want(&[
            (DFW_LINT, RAN),
            (DS_AUDIT, RAN),
            (IMPECCABLE, RAN),
            (SLOPLINT, RAN),
            (TASTE_COPY_LINT, RAN)
        ])
    );
    let args = |tool: &Path| fs::read_to_string(format!("{}.args", tool.display())).unwrap();
    // The screen is handed over as its composed copy, the plain page and the stylesheet as
    // they are; masters and the notes are not UI files of the design. (sh does not list the
    // script itself in "$@".)
    let page = rel::design(DESIGN_PAGE);
    let built = rel::join(rel::BUILD, &rel::screen(INDEX_PAGE));
    assert_eq!(
        args(&engine_stub).trim(),
        format!("detect --json {page} {built} {}", rel::COMPONENTS_CSS)
    );
    assert_eq!(
        args(&sloplint).trim(),
        format!("{page} {built} {} --json --scope app", rel::COMPONENTS_CSS)
    );
    assert_eq!(args(&taste).trim(), format!("{page} {built} --json"));
    assert_eq!(
        fs::read_to_string(format!("{}.env", engine_stub.display()))
            .unwrap()
            .trim(),
        "/skill/scripts/impeccable 1"
    );
    assert!(root.join(&built).exists());
    assert!(root.join(rel::COMPONENTS_CSS).exists());
    assert_eq!(
        results[2][FINDINGS],
        json!([
            { "level": MUST, "rule": "impeccable-flat", "file": &rel::design(DESIGN_PAGE), "line": 7, "message": "Flat: no depth [<main>]", "source": IMPECCABLE },
            // Reported on the composed copy (here by its absolute path): mapped to the screen.
            { "level": NICE, "rule": "impeccable-adv", "file": &rel::screen(INDEX_PAGE), "line": 1, "message": "N: d", "source": IMPECCABLE },
        ])
    );
    assert_eq!(
        results[3][FINDINGS],
        json!([
            { "level": SHOULD, "rule": "hallmark-3", "file": &rel::design(DESIGN_PAGE), "line": 4, "message": "generic hero | fix: rewrite", "source": SLOPLINT },
            { "level": SHOULD, "rule": "hallmark-6", "file": "", "line": 1, "message": "reflex", "source": SLOPLINT },
        ])
    );
    assert_eq!(
        results[4][FINDINGS],
        json!([
            { "level": SHOULD, "rule": "taste-T1", "file": &rel::design(DESIGN_PAGE), "line": 1, "message": "vague headline | fix: be specific", "source": TASTE_COPY_LINT },
            { "level": NICE, "rule": "taste-T3", "file": &rel::design(DESIGN_PAGE), "line": 1, "message": "T3", "source": TASTE_COPY_LINT },
            { "level": SHOULD, "rule": "taste-T4", "file": &rel::screen(INDEX_PAGE), "line": 1, "message": "T4", "source": TASTE_COPY_LINT },
        ])
    );

    // A marketing surface: Hallmark's FAIL blocks, and sloplint is told so.
    write(
        &root,
        rel::MANIFEST,
        r#"{"screens":[],"surface":"marketing"}"#,
    );
    let only = CheckOptions {
        timeout_ms: None,
        tools: Some(&[SLOPLINT]),
    };
    let page = strings(&[&rel::design(DESIGN_PAGE)]);
    let results = run_checks_with(&root, Some(&page), &config, only, &lookup).unwrap();
    assert_eq!(statuses(&results), want(&[(SLOPLINT, RAN)]));
    assert_eq!(results[0][FINDINGS][0]["level"], MUST);
    assert!(args(&sloplint).trim().ends_with("--json --scope page"));
}

/// Which tools run: the `tools` option, the `checks` settings, and what each tool reads.
#[cfg(unix)]
#[test]
fn tool_selection_and_what_each_tool_skips() {
    let root = project("check-selection");
    let tools = test_dir("check-selection-tools");
    let stub = script(&tools, "ok", "printf '[]'");
    let report = script(&tools, "report", "printf '{}'");
    let engine = ready(&stub, &[]);
    let asked = Cell::new(0);
    let lookup = Lookup {
        engine: &|| {
            asked.set(asked.get() + 1);
            Ok(Some(engine.clone()))
        },
        sloplint: &|| Ok(Some(report.clone())),
        taste: &|| Ok(Some(report.clone())),
        node: &sh,
    };
    let run = |targets: Option<&[&str]>, config: Value, tools: Option<&[&str]>| {
        let targets = targets.map(strings);
        let opts = CheckOptions {
            timeout_ms: Some(5000),
            tools,
        };
        statuses(&run_checks_with(&root, targets.as_deref(), &config, opts, &lookup).unwrap())
    };
    // dfw only: nothing external is even looked up, nothing is composed.
    assert_eq!(
        run(None, json!({}), Some(&["dfw"])),
        want(&[(DFW_LINT, RAN), (DS_AUDIT, RAN)])
    );
    assert_eq!(asked.get(), 0);
    assert!(!root.join(rel::BUILD).exists());
    // Taste is opt-in, the other two opt-out.
    assert_eq!(
        run(None, json!({}), None),
        want(&[
            (DFW_LINT, RAN),
            (DS_AUDIT, RAN),
            (IMPECCABLE, RAN),
            (SLOPLINT, RAN)
        ])
    );
    assert_eq!(
        run(
            None,
            json!({ "checks": { "impeccable": false, "sloplint": false, "tasteCopyLint": 1 } }),
            None
        ),
        want(&[(DFW_LINT, RAN), (DS_AUDIT, RAN), (TASTE_COPY_LINT, RAN)])
    );
    assert_eq!(
        run(
            None,
            json!({ "checks": { "tasteCopyLint": true } }),
            Some(&[IMPECCABLE, "taste"])
        ),
        want(&[(IMPECCABLE, RAN), (TASTE_COPY_LINT, RAN)])
    );
    // A file that is not design UI: lint has nothing to do, the audit is not run, each tool
    // skips what it cannot read.
    assert_eq!(
        run(
            Some(&[APP_MODULE]),
            json!({ "checks": { "tasteCopyLint": true } }),
            None
        ),
        want(&[
            (DFW_LINT, RAN),
            (IMPECCABLE, SKIPPED),
            (SLOPLINT, SKIPPED),
            (TASTE_COPY_LINT, SKIPPED)
        ])
    );
    assert_eq!(
        run(
            Some(&[&rel::design(NOTES_FILE), "https://example.com/"]),
            json!({ "checks": { "tasteCopyLint": true } }),
            None
        ),
        want(&[
            (DFW_LINT, RAN),
            (IMPECCABLE, RAN),
            (SLOPLINT, SKIPPED),
            (TASTE_COPY_LINT, RAN)
        ])
    );
    // A token file or a master asks for the design-system audit; a master is not handed to
    // the external tools.
    assert_eq!(
        run(Some(&[&rel::component(CHIP_MASTER)]), json!({}), None),
        want(&[
            (DFW_LINT, RAN),
            (DS_AUDIT, RAN),
            (IMPECCABLE, SKIPPED),
            (SLOPLINT, SKIPPED)
        ])
    );
    // No design folder at all.
    let empty = test_dir("check-selection-empty");
    let results =
        run_checks_with(&empty, None, &json!({}), CheckOptions::default(), &lookup).unwrap();
    assert_eq!(
        statuses(&results),
        want(&[
            (DFW_LINT, RAN),
            (DS_AUDIT, RAN),
            (IMPECCABLE, SKIPPED),
            (SLOPLINT, SKIPPED)
        ])
    );
}

/// A tool that is missing, slow, silent or broken is reported, never read as a pass.
#[cfg(unix)]
#[test]
fn tools_that_do_not_run_are_reported() {
    let root = project("check-broken");
    let tools = test_dir("check-broken-tools");
    let page = strings(&[&rel::design(DESIGN_PAGE)]);
    let config = json!({ "checks": { "tasteCopyLint": true } });
    let run = |lookup: &Lookup, timeout_ms: u64| {
        let opts = CheckOptions {
            timeout_ms: Some(timeout_ms),
            tools: Some(&[IMPECCABLE, SLOPLINT, "taste"]),
        };
        run_checks_with(&root, Some(&page), &config, opts, lookup).unwrap()
    };
    // Nothing installed.
    let results = run(&nothing(), 5000);
    assert_eq!(
        statuses(&results),
        want(&[
            (IMPECCABLE, UNAVAILABLE),
            (SLOPLINT, UNAVAILABLE),
            (TASTE_COPY_LINT, UNAVAILABLE)
        ])
    );
    assert!(results.iter().all(|r| r.get(DETAIL).is_none()));
    // Installed, but no Node.js for the scripts and no verified engine.
    let some = tools.join("x.mjs");
    let unverified = not_ready(&[]);
    let no_node = Lookup {
        engine: &|| Ok(Some(unverified.clone())),
        sloplint: &|| Ok(Some(some.clone())),
        taste: &|| Ok(Some(some.clone())),
        node: &|| None,
    };
    let results = run(&no_node, 5000);
    assert_eq!(
        statuses(&results),
        want(&[
            (IMPECCABLE, NOT_INSTALLED),
            (SLOPLINT, UNAVAILABLE),
            (TASTE_COPY_LINT, UNAVAILABLE)
        ])
    );
    assert_eq!(results[1][DETAIL], "needs Node.js on PATH");
    let report = format_report(&results, 50);
    assert_eq!(report.unverified, [IMPECCABLE, SLOPLINT, TASTE_COPY_LINT]);
    assert!(report.text.ends_with("NOT VERIFIED: impeccable (Engine not downloaded yet; run \"dfw check --setup\" once.); sloplint (needs Node.js on PATH); taste-copy-lint (needs Node.js on PATH). Say so in any report; do not claim these aspects pass."));
    // Too slow: killed at the budget.
    let slow = script(&tools, "slow", "exec sleep 20");
    let slow_engine = ready(&slow, &[]);
    let lookup = Lookup {
        engine: &|| Ok(Some(slow_engine.clone())),
        sloplint: &|| Ok(Some(slow.clone())),
        taste: &|| Ok(Some(slow.clone())),
        node: &sh,
    };
    let started = std::time::Instant::now();
    let results = run(&lookup, 300);
    assert!(started.elapsed().as_secs() < 15);
    // Copy-lint has no timeout status in the Node code: no output is an error.
    assert_eq!(
        statuses(&results),
        want(&[
            (IMPECCABLE, TIMEOUT),
            (SLOPLINT, TIMEOUT),
            (TASTE_COPY_LINT, ERROR)
        ])
    );
    // Not JSON, or JSON of the wrong shape; stderr is preferred for the detail, cut at 400.
    let noisy = script(
        &tools,
        "noisy",
        &format!("echo 'not json'; echo '{}' >&2; exit 3", "e".repeat(450)),
    );
    let wrong = script(&tools, "wrong", "printf '{\"an\":\"object\"}'");
    let quiet = script(&tools, "quiet", "echo 'plain words'");
    let wrong_engine = ready(&wrong, &[]);
    let lookup = Lookup {
        engine: &|| Ok(Some(wrong_engine.clone())),
        sloplint: &|| Ok(Some(noisy.clone())),
        taste: &|| Ok(Some(quiet.clone())),
        node: &sh,
    };
    let results = run(&lookup, 5000);
    assert_eq!(
        statuses(&results),
        want(&[
            (IMPECCABLE, ERROR),
            (SLOPLINT, ERROR),
            (TASTE_COPY_LINT, ERROR)
        ])
    );
    assert_eq!(results[0][DETAIL], "{\"an\":\"object\"}");
    assert_eq!(results[1][DETAIL], "e".repeat(400));
    assert_eq!(results[2][DETAIL], "plain words\n");
    // The executable is gone.
    let gone = ready(&tools.join("missing"), &[]);
    let lookup = Lookup {
        engine: &|| Ok(Some(gone.clone())),
        ..nothing()
    };
    assert_eq!(run(&lookup, 5000)[0][STATUS], ERROR);
    assert_eq!(run(&lookup, 5000)[0][DETAIL], "");
}

/// A design.json that holds `null` names the file and the fix, as a user error (review E2:
/// it was the JavaScript TypeError text).
#[test]
fn a_null_design_manifest_is_a_user_error_that_names_the_file() {
    let root = test_dir("check-null-manifest");
    write(&root, rel::MANIFEST, "null");
    let err = sloplint_scope(&root).unwrap_err();
    assert!(
        err.message()
            .starts_with(&format!("{} holds null", rel::MANIFEST)),
        "{err}"
    );
    assert!(!err.report().starts_with("Error:"));
    fs::remove_dir_all(&root).unwrap();
}
