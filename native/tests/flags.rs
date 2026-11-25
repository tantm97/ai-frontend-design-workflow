//! Every flag the help shows with a value refuses to run without one (§0 row 9, `contracts.md`
//! C2): `dfw <command> --<flag>` exits 1 with `--<flag> needs a value.` and writes nothing,
//! instead of reading the flag as `true` (a level 1 decision, a 1 second timeout, an approval
//! note "true").

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Output, Stdio};

#[path = "../src/core/paths/layout.rs"]
#[allow(
    dead_code,
    reason = "each test binary uses a part of the project layout"
)]
mod layout;
use layout::file;

#[path = "../src/core/word.rs"]
#[allow(dead_code, reason = "each test binary uses a part of the command line")]
mod word;
use word::{command, flag, hook_name};

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

/// Left out by name: a boolean on most commands; on `comments add` and `stack set` its JSON text
/// is checked by the command itself.
const NOT_A_VALUE_FLAG: &str = "json";

fn tmp_dir(label: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("dfw-flags-{label}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn run(root: &Path, args: &[&str], stdin: &str) -> Output {
    let mut cmd = home::command(DFW, root, &root.join(".user"), &root.join(".home"));
    cmd.args(args)
        .env("DFW_TELEMETRY", "off")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cmd.spawn().unwrap();
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().unwrap()
}

/// Every file under `.dfw/` and `design/` with its bytes.
fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.insert(p.clone(), fs::read(&p).unwrap_or_default());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(&root.join(file::DFW), &mut out);
    walk(&root.join(file::DESIGN), &mut out);
    out
}

/// A value in the help's grammar: `<…>`, a word list with `|`, a number, or `...`.
fn is_value(word: &str) -> bool {
    let w = word.trim_end_matches(']');
    let placeholder = w.starts_with('<') && w.ends_with('>');
    let choices = w.contains('|') && !w.starts_with('-') && w.split('|').all(|p| !p.is_empty());
    let number =
        w.chars().any(|c| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_digit() || c == '.');
    placeholder || choices || number || w == "..."
}

/// The names of `VALUE_FLAGS`, read from its source.
fn value_flags() -> Vec<String> {
    let src =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("src/core/util.rs")).unwrap();
    let start = src.find("pub const VALUE_FLAGS").unwrap();
    let body = &src[start..start + src[start..].find("];").unwrap()];
    body.lines()
        .skip(1)
        .filter_map(|l| l.trim().strip_prefix('"')?.strip_suffix("\","))
        .map(str::to_string)
        .collect()
}

/// `(command words, flag)` for every flag the help shows with a value, and every flag of
/// `VALUE_FLAGS` the help shows at all.
fn flags_from_help(help: &str, known: &[String]) -> Vec<(Vec<String>, String)> {
    let mut out: Vec<(Vec<String>, String)> = Vec::new();
    let mut words: Vec<String> = Vec::new();
    for line in help.lines() {
        let t = line.trim_start();
        if let Some(rest) = t.strip_prefix("dfw ") {
            words = rest
                .split_whitespace()
                .take_while(|w| {
                    w.chars()
                        .all(|c| c.is_ascii_lowercase() || c == '-' || c == '|')
                })
                .take_while(|w| *w != "|" && !w.starts_with('-'))
                .map(|w| w.split('|').next().unwrap_or_default().to_string())
                .collect();
        }
        // `[--screen <id>|--page <id>]`: each alternative flag on its own.
        let spaced = t.replace("|--", " | --");
        let tokens: Vec<&str> = spaced.split_whitespace().collect();
        for (i, tok) in tokens.iter().enumerate() {
            let tok = tok.trim_start_matches('[');
            let Some(name) = tok.strip_prefix("--") else {
                continue;
            };
            let name = name.trim_end_matches(']').trim_end_matches(':');
            if name.is_empty() || name == NOT_A_VALUE_FLAG || name.contains('|') {
                continue;
            }
            let valued = tokens.get(i + 1).is_some_and(|n| is_value(n));
            if (valued || known.iter().any(|k| k == name))
                && !out.iter().any(|(w, f)| *w == words && f == name)
            {
                out.push((words.clone(), name.to_string()));
            }
        }
    }
    out
}

#[test]
fn every_value_flag_in_the_help_needs_a_value() {
    let root = tmp_dir("walk");
    let init = run(&root, &[command::INIT], "");
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let help = String::from_utf8_lossy(&run(&root, &[command::HELP], "").stdout).into_owned();
    let known = value_flags();
    let flags = flags_from_help(&help, &known);
    assert!(
        flags.len() > 40,
        "the help grammar found only {} flags",
        flags.len()
    );
    let before = snapshot(&root);
    let mut wrong = Vec::new();
    for (words, name) in &flags {
        if words.first().map(String::as_str) == Some(command::HOOK) {
            continue;
        }
        let bare = format!("--{name}");
        let mut args: Vec<&str> = words.iter().map(String::as_str).collect();
        args.push(&bare);
        let out = run(&root, &args, "");
        let stderr = String::from_utf8_lossy(&out.stderr);
        let expected = format!("{bare} needs a value.");
        if out.status.code() != Some(1) || !stderr.contains(&expected) {
            wrong.push(format!(
                "dfw {}: exit {:?}, stderr {:?}",
                args.join(" "),
                out.status.code(),
                stderr.trim()
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "flags the help shows with a value ran without one:\n{}",
        wrong.join("\n")
    );
    assert!(
        before == snapshot(&root),
        "a refused flag changed a project file"
    );
    let _ = fs::remove_dir_all(&root);
}

#[test]
fn a_hook_without_its_host_value_fails_open() {
    let root = tmp_dir("hook");
    let out = run(
        &root,
        &[command::HOOK, hook_name::SESSION_START, flag::HOST],
        "{}",
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&out.stderr).contains(&format!(
        "dfw {} {} error: {} needs a value.",
        command::HOOK,
        hook_name::SESSION_START,
        flag::HOST
    )));
}
