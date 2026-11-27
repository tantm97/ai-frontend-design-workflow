//! Input of the wrong shape fails the whole command; it never passes a gate in part
//! (`errors.md` E7, `security-privacy.md` S5, §0 rows 13 and 14). Every value of the untrusted
//! documents of a fixture project (the manifest, the state, the config, the stack, a review
//! session) is replaced in turn by a value of each other JSON type, and `dfw lint`, `dfw check`
//! and `dfw approve G2` run on a copy: a command that reads the value stops with an error, or
//! gives what it gives for the right shape because it does not use that value. G2 is never
//! recorded with a mutated value but where the value is on the list of ignored ones.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use serde_json::{json, Value};

#[path = "support/product.rs"]
mod product;
use layout::{file, rel};
use product::{fixture, layout, word};

use word::{command, flag, sub};

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

/// The fixed clock of every run, so two runs print the same.
const NOW: &str = "2026-01-02T03:04:05.000Z";

/// A fresh, empty folder (its real path: macOS reaches the temporary directory through a symlink).
fn tmp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-shapes-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn project(case: &Path) -> PathBuf {
    case.join("project")
}

/// What one run gave.
#[derive(Debug, Clone, PartialEq)]
struct Ran {
    code: Option<i32>,
    stdout: String,
    stderr: String,
}

/// `dfw <args>` in `case/project`, with the homes in `case/home`.
fn dfw(case: &Path, args: &[&str]) -> Ran {
    let home = case.join("home");
    let mut cmd: Command = home::command(DFW, &project(case), &home, &home.join(file::DFW));
    cmd.args(args)
        .env("DFW_TELEMETRY", "0")
        .env("DFW_NOW", NOW)
        .stdin(Stdio::null());
    let out = cmd.output().unwrap();
    Ran {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
    }
}

fn must(case: &Path, args: &[&str]) -> String {
    let r = dfw(case, args);
    assert_eq!(r.code, Some(0), "dfw {}: {}", args.join(" "), r.stderr);
    r.stdout
}

fn copy_dir(from: &Path, to: &Path) {
    fs::create_dir_all(to).unwrap();
    for e in fs::read_dir(from).unwrap().flatten() {
        let (src, dest) = (e.path(), to.join(e.file_name()));
        if src.is_dir() {
            copy_dir(&src, &dest);
        } else {
            fs::copy(&src, &dest).unwrap();
        }
    }
}

/// A project with a screen, a design system, a stack proposal and a dismissed comment; the checks
/// that need an outside tool are off, so every run is quick and the same.
fn fixture() -> PathBuf {
    let case = tmp_dir("fixture");
    let root = project(&case);
    fs::create_dir_all(&root).unwrap();
    must(&case, &[command::INIT]);
    must(&case, &[command::SYSTEM, sub::INIT]);
    fs::write(
        root.join(rel::CONFIG),
        json!({ "schema": 1, "checks": { "impeccable": false, "sloplint": false, "maxFindings": 12 },
            "gate": { "allow": ["docs/**"] }, "studio": { "port": 4601 } })
        .to_string(),
    )
    .unwrap();
    let page = "<!doctype html><html lang=\"en\"><head><title>Home</title><link rel=\"stylesheet\" href=\"../../tokens/tokens.css\"></head><body><main><h1>Home</h1><p>Hello.</p></main></body></html>\n";
    let screen_file = rel::join(file::SCREENS, fixture::SCREEN_PAGE);
    fs::create_dir_all(root.join(rel::SCREENS).join(fixture::SCREEN)).unwrap();
    fs::write(root.join(rel::design(&screen_file)), page).unwrap();
    let manifest = json!({ "schema": 1, "product": { "name": "Shop" },
        "breakpoints": [{ "id": "desktop", "width": 1440, "height": 900 }],
        "states": ["empty"],
        "screens": [{ "id": "home", "title": "Home", "states": ["loading"],
            "variants": [{ "id": "a", "file": screen_file }] }] });
    fs::write(
        root.join(rel::MANIFEST),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
    must(
        &case,
        &[
            command::STACK,
            sub::SET,
            "--json",
            r#"{"framework":"vite","language":"ts","styling":"css"}"#,
        ],
    );
    let comment = json!({ "target": { "file": rel::design(&screen_file), "selector": "h1" }, "comment": "Bigger title", "screen": "home", "variant": "a" });
    must(
        &case,
        &[command::COMMENTS, sub::ADD, "--json", &comment.to_string()],
    );
    let listed: Value =
        serde_json::from_str(&must(&case, &[command::COMMENTS, sub::LIST, flag::JSON])).unwrap();
    let id = listed[0]["id"].as_str().unwrap().to_string();
    must(
        &case,
        &[
            command::COMMENTS,
            "dismiss",
            &id,
            "--message",
            "The user said no",
        ],
    );
    case
}

/// The documents whose shape is mutated, by project path; the review session is the one file
/// of `.dfw/reviews`.
fn documents(root: &Path) -> Vec<String> {
    let mut docs: Vec<String> = [rel::MANIFEST, rel::STATE, rel::CONFIG, rel::STACK]
        .map(String::from)
        .to_vec();
    let mut sessions: Vec<String> = fs::read_dir(root.join(rel::REVIEWS))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".json") && !n.starts_with('.'))
        .collect();
    sessions.sort();
    docs.push(rel::join(rel::REVIEWS, &sessions[0]));
    docs
}

/// Every path of a JSON value (object keys, the first item of a list), `""` for the root.
fn paths(value: &Value, at: &str, out: &mut Vec<String>) {
    out.push(at.to_string());
    let join = |k: &str| {
        if at.is_empty() {
            k.to_string()
        } else {
            format!("{at}.{k}")
        }
    };
    match value {
        Value::Object(map) => map.iter().for_each(|(k, v)| paths(v, &join(k), out)),
        Value::Array(items) => {
            if let Some(first) = items.first() {
                paths(first, &join("0"), out);
            }
        }
        _ => {}
    }
}

fn get_mut<'a>(value: &'a mut Value, path: &str) -> &'a mut Value {
    if path.is_empty() {
        return value;
    }
    path.split('.').fold(value, |v, k| match v {
        Value::Array(items) => &mut items[k.parse::<usize>().unwrap()],
        other => &mut other[k],
    })
}

/// A value of every JSON type but the one of `value`.
fn wrong_types(value: &Value) -> Vec<Value> {
    let all = [Value::Null, json!(7), json!("x"), json!([]), json!({})];
    let kind = |v: &Value| std::mem::discriminant(v);
    // A boolean and a number are both scalars that a reader may test the same way: both stay.
    all.into_iter().filter(|v| kind(v) != kind(value)).collect()
}

/// The runs of one case.
fn commands() -> [Vec<&'static str>; 3] {
    [
        vec![command::LINT, flag::JSON],
        vec![command::CHECK, "--no-a11y", flag::JSON],
        vec![
            command::APPROVE,
            "G2",
            "--accept-findings",
            flag::ACCEPT_UNVERIFIED,
            flag::NOTE,
            "x",
        ],
    ]
}

/// The output of a run with the design hash left out (a changed manifest changes it).
fn masked(r: &Ran) -> (Option<i32>, String) {
    let hash = regex::Regex::new(r"sha256 [0-9a-f]{12}").unwrap();
    (r.code, hash.replace_all(&r.stdout, "sha256 _").into_owned())
}

/// The design was frozen: G2 recorded.
fn frozen(case: &Path) -> bool {
    fs::read_dir(project(case).join(rel::APPROVED))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// Values the three commands do not read (or read only to fall back to the default, as Node
/// did): a wrong type there changes nothing, so G2 may pass as for the right shape.
/// `(document, paths separated by spaces)`; the review session is named by its folder.
const IGNORED: &[(&str, &str)] = &[
    // The write gate's globs and the Studio port are read by the hooks and the Studio only;
    // a bad `maxFindings` falls back to 12.
    (rel::CONFIG, "schema checks.maxFindings gate gate.allow gate.allow.0 studio studio.port"),
    // G2 reads a comment's `status` only (a wrong one blocks it): the rest is shown, not used.
    (
        rel::REVIEWS,
        "schema id designVersion startedAt comments.0.id comments.0.createdAt comments.0.updatedAt \
         comments.0.designVersion comments.0.provider comments.0.screen comments.0.variant \
         comments.0.breakpoint comments.0.state comments.0.theme comments.0.file comments.0.comment \
         comments.0.screenshot comments.0.resolution comments.0.target comments.0.target.file \
         comments.0.target.selector comments.0.target.source comments.0.thread comments.0.thread.0 \
         comments.0.thread.0.at comments.0.thread.0.by comments.0.thread.0.message \
         comments.0.thread.0.status",
    ),
    // The stack is G3's input (`approve G3` refuses a stack.json that is not an object).
    (rel::STACK, " framework language styling status proposedAt"),
    // Times, the history and the waivers: shown or appended to, never decided on by G2.
    (rel::STATE, "schema createdAt updatedAt active waived history.0 history.0.at history.0.event history.0.mode"),
    // Shown only (names, titles, the format number).
    (rel::MANIFEST, "schema product product.name screens.0.title"),
];

/// Wrong shapes a reader takes today though it should not (a pass in part, or a value skipped
/// where it should stop the gate): each is a known gap, with the follow-up it needs.
/// `(document, paths separated by spaces, the gap)`. An entry that no longer fails must go.
const KNOWN: &[(&str, &str, &str)] = &[];

/// `path` of `doc` is in one of the lists.
fn listed<'a>(list: impl IntoIterator<Item = (&'a str, &'a str)>, doc: &str, path: &str) -> bool {
    list.into_iter()
        .any(|(d, paths)| d == doc && paths.split(' ').any(|p| p == path))
}

/// The name the lists give a document: the review session by its folder.
fn label(doc: &str) -> &str {
    if doc.starts_with(rel::REVIEWS) {
        rel::REVIEWS
    } else {
        doc
    }
}

#[test]
fn a_value_of_the_wrong_type_fails_the_command_or_is_ignored() {
    let base = fixture();
    let root = project(&base);
    let baseline: Vec<Ran> = commands()
        .iter()
        .map(|args| {
            let case = tmp_dir("baseline");
            copy_dir(&base, &case);
            let r = dfw(&case, args);
            fs::remove_dir_all(&case).unwrap();
            r
        })
        .collect();
    for (args, r) in commands().iter().zip(&baseline) {
        assert_eq!(
            r.code,
            Some(0),
            "baseline dfw {}: {}{}",
            args.join(" "),
            r.stdout,
            r.stderr
        );
    }
    let mut cases = Vec::new();
    for doc in documents(&root) {
        let value: Value =
            serde_json::from_str(&fs::read_to_string(root.join(&doc)).unwrap()).unwrap();
        let mut all = Vec::new();
        paths(&value, "", &mut all);
        for path in all {
            for wrong in wrong_types(get_mut(&mut value.clone(), &path)) {
                cases.push((doc.clone(), path.clone(), wrong));
            }
        }
    }
    assert!(
        cases.len() < 400,
        "{} cases: bound the fixture",
        cases.len()
    );
    let found = Mutex::new(Vec::new());
    // Known paths with a case that still shows the gap.
    let gaps = Mutex::new(Vec::new());
    let next = AtomicU32::new(0);
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed) as usize;
                let Some((doc, path, wrong)) = cases.get(i) else {
                    break;
                };
                let case = tmp_dir("case");
                copy_dir(&base, &case);
                let file = project(&case).join(doc);
                let mut value: Value =
                    serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
                *get_mut(&mut value, path) = wrong.clone();
                fs::write(&file, value.to_string()).unwrap();
                let ignored = listed(IGNORED.iter().copied(), label(doc), path);
                let known = listed(KNOWN.iter().map(|(d, p, _)| (*d, *p)), label(doc), path);
                let mut why = Vec::new();
                for (k, (args, base_run)) in commands().iter().zip(&baseline).enumerate() {
                    let r = dfw(&case, args);
                    let same = masked(&r) == masked(base_run);
                    if r.stderr.contains("panicked") {
                        why.push(format!("{} crashed: {}", args[0], r.stderr.trim()));
                    }
                    let passed = r.code == Some(0);
                    if passed && !same {
                        // It read the value and went on with it: a pass in part.
                        why.push(format!("{} passed with another result", args[0]));
                    } else if k == 2 && passed && frozen(&case) && !ignored {
                        // The gate passed as for the right shape: only a value no command
                        // reads may do that, and it is listed.
                        why.push(format!(
                            "{} recorded G2 (the value is not on the ignored list)",
                            args[0]
                        ));
                    }
                }
                if why.is_empty() {
                    // as it should be
                } else if known {
                    gaps.lock().unwrap().push(format!("{} {path}", label(doc)));
                } else {
                    found
                        .lock()
                        .unwrap()
                        .push(format!("{doc} {path} = {wrong}: {}", why.join("; ")));
                }
                fs::remove_dir_all(&case).unwrap();
            });
        }
    });
    fs::remove_dir_all(&base).unwrap();
    let mut found = found.into_inner().unwrap();
    found.sort();
    let gaps = gaps.into_inner().unwrap();
    let fixed: Vec<String> = KNOWN
        .iter()
        .flat_map(|(d, paths, _)| paths.split(' ').map(move |p| format!("{d} {p}")))
        .filter(|k| !gaps.contains(k))
        .collect();
    assert!(
        fixed.is_empty(),
        "known gaps that no case shows any more: take them off KNOWN:\n{}",
        fixed.join("\n")
    );
    assert!(
        found.is_empty(),
        "{} wrong shapes taken in part ({} cases):\n{}",
        found.len(),
        cases.len(),
        found.join("\n")
    );
}
