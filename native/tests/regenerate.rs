//! The commands that write the design system (tokens, themes, masters, the head, the inventory)
//! keep the user's edits without `--force` (`design-files.md` D7, §0 row 26): run on a project
//! whose every file of `design/` carries an edit, each one leaves every file as it was, byte for
//! byte, or stops with an error. A command whose job is to replace a file the user named (the
//! option the user chose) is listed with that file and the reason. `system choose` and
//! `style apply` replace `tokens.json` and `head.html` whole: they refuse an edited one without
//! `--force` (`.dfw/written.json` holds what dfw wrote).

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;

#[path = "support/product.rs"]
mod product;
use layout::{file, rel};
use product::{layout, word};

use word::{command, flag, sub};

#[path = "support/home.rs"]
mod home;

const DFW: &str = env!("CARGO_BIN_EXE_dfw");

/// The key a JSON file of the fixture gains, and the text an HTML, CSS or SVG file ends with.
const EDIT_KEY: &str = "x-user-edit";
const EDIT_COMMENT: &str = "<!-- the user's edit -->";
const EDIT_CSS: &str = "/* the user's edit */";

/// Files a build derives from the others (and says so in their first line): rebuilding them is
/// not losing an edit.
fn generated() -> [String; 3] {
    [
        rel::join(rel::TOKENS, file::TOKENS_CSS),
        rel::COMPONENTS_CSS.to_string(),
        rel::SYSTEM_PAGE.to_string(),
    ]
}

/// The subcommands that write the design system, and the icon they add.
const APPLY: &str = "apply";
const OPTIONS: &str = "options";
const CHOOSE: &str = "choose";
const ICON: &str = "search";

/// Two presets and a style guide the commands are given.
const PRESET: &str = "crisp";
const OTHER_PRESET: &str = "soft";
const STYLE: &str = "editorial-luxe";

/// A fresh, empty folder (its real path: macOS reaches the temporary directory through a symlink).
fn tmp_dir(label: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-regenerate-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// `dfw <args>` in `case/project`, with the homes in `case/home`.
fn dfw(case: &Path, args: &[&str]) -> Output {
    let root = project(case);
    let home = case.join("home");
    let mut cmd: Command = home::command(DFW, &root, &home, &home.join(file::DFW));
    cmd.args(args)
        .env("DFW_TELEMETRY", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd.output().unwrap()
}

fn project(case: &Path) -> PathBuf {
    case.join("project")
}

fn must(case: &Path, args: &[&str]) {
    let out = dfw(case, args);
    assert!(
        out.status.success(),
        "dfw {}: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
}

/// Every file under `dir`, as `/`-separated paths relative to `base`.
fn files(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            files(base, &p, out);
        } else {
            let rel = p
                .strip_prefix(base)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(rel, fs::read(&p).unwrap());
        }
    }
}

fn design_files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    files(root, &root.join(rel::DESIGN), &mut out);
    out
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

/// Give a file the user's edit: a key in a JSON object, a comment at the end of the others.
fn edit(path: &Path) {
    let text = fs::read_to_string(path).unwrap_or_default();
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    let edited = match ext {
        "json" => {
            let mut value: Value = serde_json::from_str(&text).unwrap();
            let Some(map) = value.as_object_mut() else {
                return;
            };
            map.insert(EDIT_KEY.into(), Value::Bool(true));
            serde_json::to_string_pretty(&value).unwrap() + "\n"
        }
        "css" => format!("{text}{EDIT_CSS}\n"),
        _ => format!("{text}{EDIT_COMMENT}\n"),
    };
    fs::write(path, edited).unwrap();
}

/// A project with a design system, option themes, an icon and a master of the user's own, then
/// an edit in every file of `design/` a build does not derive.
fn fixture() -> PathBuf {
    let case = tmp_dir("fixture");
    let root = project(&case);
    fs::create_dir_all(&root).unwrap();
    must(&case, &[command::INIT]);
    must(&case, &[command::SYSTEM, sub::INIT, "--preset", PRESET]);
    must(&case, &[command::SYSTEM, OPTIONS, PRESET, OTHER_PRESET]);
    must(&case, &[command::ICONS, sub::ADD, ICON]);
    fs::write(
        root.join(rel::master("card")),
        "<dfw-component name=\"card\"><template><div class=\"card\"><dfw-slot></dfw-slot></div></template></dfw-component>\n",
    )
    .unwrap();
    for f in design_files(&root).keys() {
        if !generated().contains(f) {
            edit(&root.join(f));
        }
    }
    case
}

/// What a command may do to a file of the user's on purpose.
#[derive(Debug, PartialEq)]
enum Allowed {
    /// Nothing: the file stays byte for byte.
    Nothing,
    /// Rewrite it, the user's edit kept (a managed block or one field of it changes).
    KeepEdit,
    /// Remove it.
    Remove,
}

/// The exceptions, each with its reason.
fn allowed(args: &[&str], f: &str) -> Allowed {
    let is = |cmd: &[&str]| args.starts_with(cmd);
    let option_theme = f.starts_with(&format!("{}/opt-", rel::THEMES));
    let choose = is(&[command::SYSTEM, CHOOSE]);
    let options = is(&[command::SYSTEM, OPTIONS]);
    match f {
        // `system choose` promotes the option theme the user compared (and edited) to the
        // tokens, then drops the options and the `themes` list of the manifest.
        rel::TOKENS_JSON if choose => Allowed::KeepEdit,
        _ if option_theme && choose => Allowed::Remove,
        // `system options` rewrites the font block between its markers in the head and the
        // `themes` list of the manifest; `system choose` the `themes` list.
        rel::HEAD if options => Allowed::KeepEdit,
        rel::MANIFEST if choose || options => Allowed::KeepEdit,
        // A new comparison drops the options it does not name (the ones it names stay).
        _ if option_theme && options => Allowed::Remove,
        // `type scale --write` writes the type tokens into tokens.json, the rest kept.
        rel::TOKENS_JSON if is(&[command::TYPE]) => Allowed::KeepEdit,
        _ => Allowed::Nothing,
    }
}

/// The user's edit is still in the file.
fn keeps_edit(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    [EDIT_KEY, EDIT_COMMENT, EDIT_CSS]
        .iter()
        .any(|m| text.contains(m))
}

#[test]
fn regenerating_commands_keep_the_users_edits() {
    let base = fixture();
    let before = design_files(&project(&base));
    assert!(
        before.len() >= 8,
        "the fixture has too few design files: {:?}",
        before.keys()
    );
    let penpot_tokens = base.join("tokens.penpot.json");
    must(
        &base,
        &[
            command::TOKENS,
            sub::EXPORT,
            flag::FORMAT,
            command::PENPOT,
            flag::OUT,
            penpot_tokens.to_str().unwrap(),
        ],
    );
    let runs: Vec<Vec<&str>> = vec![
        vec![command::INIT],
        vec![command::SYSTEM, sub::INIT],
        vec![command::SYSTEM, sub::INIT, "--preset", OTHER_PRESET],
        vec![command::SYSTEM, sub::INIT, "--style", STYLE],
        vec![command::STYLE, APPLY, STYLE],
        vec![command::STYLE, APPLY, STYLE, "--fonts", "latin"],
        vec![command::SYSTEM, OPTIONS, PRESET, OTHER_PRESET],
        vec![command::SYSTEM, OPTIONS, PRESET, STYLE],
        vec![command::SYSTEM, CHOOSE, PRESET],
        vec![command::SYSTEM, sub::INVENTORY],
        vec![command::SYSTEM, "ref", "button"],
        vec![command::PALETTE, command::BUILD, "--brand", "#2255aa"],
        vec![
            command::TYPE,
            "scale",
            "--base",
            "16",
            "--ratio",
            "1.25",
            "--write",
        ],
        vec![command::ICONS, sub::ADD, ICON],
        vec![command::ICONS, sub::ADD, ICON, "menu"],
        vec![command::BUILD],
        vec![command::TOKENS, command::BUILD],
        vec![
            command::TOKENS,
            sub::IMPORT,
            penpot_tokens.to_str().unwrap(),
            "--from",
            command::PENPOT,
        ],
        vec![command::HANDOFF],
    ];
    let mut breaches = Vec::new();
    for (i, args) in runs.iter().enumerate() {
        let case = tmp_dir(&format!("run-{i}"));
        copy_dir(&base, &case);
        let out = dfw(&case, args);
        let after = design_files(&project(&case));
        let failed = !out.status.success();
        for (f, bytes) in &before {
            if generated().contains(f) {
                continue;
            }
            let ok = match (allowed(args, f), after.get(f)) {
                (_, Some(now)) if now == bytes => true,
                (Allowed::KeepEdit, Some(now)) => keeps_edit(now),
                (Allowed::Remove, None) => true,
                _ => false,
            };
            if !ok {
                let how = if after.contains_key(f) {
                    "changed"
                } else {
                    "removed"
                };
                breaches.push(format!(
                    "dfw {} (exit {:?}{}): {how} {f}",
                    args.join(" "),
                    out.status.code(),
                    if failed {
                        ", and it wrote before stopping"
                    } else {
                        ""
                    }
                ));
            }
        }
        fs::remove_dir_all(&case).unwrap();
    }
    fs::remove_dir_all(&base).unwrap();
    assert!(
        breaches.is_empty(),
        "commands replaced the user's edits without --force:\n{}",
        breaches.join("\n")
    );
}

/// The text the refusal of an edited file holds.
const FORCE_HINT: &str = "--force";

fn read(case: &Path, rel: &str) -> String {
    fs::read_to_string(project(case).join(rel)).unwrap()
}

/// The SHA-256 `.dfw/written.json` holds for `rel`.
fn recorded(case: &Path, rel: &str) -> Option<String> {
    let text = fs::read_to_string(project(case).join(rel::WRITTEN)).ok()?;
    let record: Value = serde_json::from_str(&text).unwrap();
    record.get(rel).and_then(Value::as_str).map(str::to_string)
}

/// A project whose design system dfw wrote (`system init`), so both files have a record.
fn written_project(label: &str) -> PathBuf {
    let case = tmp_dir(label);
    fs::create_dir_all(project(&case)).unwrap();
    must(&case, &[command::INIT]);
    must(&case, &[command::SYSTEM, sub::INIT, "--preset", PRESET]);
    assert!(recorded(&case, rel::TOKENS_JSON).is_some());
    assert!(recorded(&case, rel::HEAD).is_some());
    case
}

/// The commands that replace the token file and the head whole.
fn replacing() -> [Vec<&'static str>; 3] {
    [
        vec![command::STYLE, APPLY, STYLE],
        vec![command::SYSTEM, CHOOSE, OTHER_PRESET],
        vec![command::SYSTEM, CHOOSE, STYLE],
    ]
}

#[test]
fn an_edited_token_file_or_head_is_refused_then_replaced_with_force() {
    for args in replacing() {
        for target in [rel::TOKENS_JSON, rel::HEAD] {
            let case = written_project("refuse");
            edit(&project(&case).join(target));
            let before = design_files(&project(&case));
            let out = dfw(&case, &args);
            let stderr = String::from_utf8_lossy(&out.stderr);
            let shown = format!("dfw {} on an edited {target}", args.join(" "));
            let refusal = format!("{shown}: {stderr}");
            assert_eq!(out.status.code(), Some(1), "{refusal}");
            assert!(
                stderr.contains(target) && stderr.contains(FORCE_HINT),
                "{refusal}"
            );
            assert_eq!(design_files(&project(&case)), before, "{shown} wrote");
            let mut forced = args.clone();
            forced.push(FORCE_HINT);
            must(&case, &forced);
            assert!(!keeps_edit(read(&case, target).as_bytes()), "{shown}");
            for f in [rel::TOKENS_JSON, rel::HEAD] {
                let bytes = fs::read(project(&case).join(f)).unwrap();
                assert_eq!(
                    recorded(&case, f).as_deref(),
                    Some(sha256(&bytes).as_str()),
                    "{shown}: {f}"
                );
            }
            fs::remove_dir_all(&case).unwrap();
        }
    }
}

#[test]
fn files_dfw_wrote_and_nobody_edited_are_replaced() {
    // The usual flow: init, compare options, choose; then a guide on top.
    let case = written_project("flow");
    must(&case, &[command::SYSTEM, OPTIONS, PRESET, OTHER_PRESET]);
    must(&case, &[command::SYSTEM, CHOOSE, OTHER_PRESET]);
    must(&case, &[command::STYLE, APPLY, STYLE]);
    fs::remove_dir_all(&case).unwrap();
}

#[test]
fn a_project_without_a_record_is_written_as_before_and_recorded() {
    let case = written_project("old");
    fs::remove_file(project(&case).join(rel::WRITTEN)).unwrap();
    edit(&project(&case).join(rel::TOKENS_JSON));
    must(&case, &[command::STYLE, APPLY, STYLE]);
    assert!(!keeps_edit(read(&case, rel::TOKENS_JSON).as_bytes()));
    assert!(recorded(&case, rel::TOKENS_JSON).is_some());
    assert!(recorded(&case, rel::HEAD).is_some());
    fs::remove_dir_all(&case).unwrap();
}

/// `system options` keeps an edit to the head outside its block, and the head stays the user's:
/// a later `system choose` refuses it.
#[test]
fn an_edited_head_stays_edited_through_options() {
    let case = written_project("options");
    edit(&project(&case).join(rel::HEAD));
    must(&case, &[command::SYSTEM, OPTIONS, PRESET, OTHER_PRESET]);
    let out = dfw(&case, &[command::SYSTEM, CHOOSE, OTHER_PRESET]);
    assert_eq!(out.status.code(), Some(1));
    assert!(keeps_edit(read(&case, rel::HEAD).as_bytes()));
    fs::remove_dir_all(&case).unwrap();
}

/// Lowercase hex SHA-256, as dfw records it.
fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
