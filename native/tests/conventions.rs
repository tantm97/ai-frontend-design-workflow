//! Convention checks on the source of `src/` (and, for the project layout, of the tests).
//!
//! The growth guard of convention G15 (git-and-review.md): the number of suppressions
//! (`#[allow(...)]` in product code), and of string literals a test writes twice, may only go down. A higher count fails with the new ones; a lower count fails until the
//! baseline (`tests/conventions-baseline.json`) is lowered to it, so progress is recorded.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

fn native() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Every `allow` attribute of `src/` as `file:line`, the crate-level test exemption of
/// `main.rs` left out (it applies to test builds only).
fn allows() -> Vec<String> {
    let src = native().join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let rel = f.strip_prefix(native()).unwrap().display().to_string();
        let text = fs::read_to_string(&f).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim_start();
            // A `cfg_attr` is read up to its closing `)]`: its `allow(` may be on a later line.
            let cfg_attr = || {
                let mut whole = String::new();
                for l in &lines[i..] {
                    whole.push_str(l);
                    if l.trim_end().ends_with(")]") {
                        break;
                    }
                }
                whole
            };
            let attr = t.starts_with("#[allow(")
                || t.starts_with("#![allow(")
                || (t.starts_with("#[cfg_attr(") || t.starts_with("#![cfg_attr("))
                    && cfg_attr().contains("allow(");
            let test_exemption = rel.ends_with("main.rs") && t.starts_with("#![cfg_attr(");
            if attr && !test_exemption {
                out.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    out
}

fn baseline() -> Value {
    let text = fs::read_to_string(native().join("tests/conventions-baseline.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn compare(what: &str, key: &str, found: &[String]) {
    let base = baseline()[key].as_u64().unwrap() as usize;
    assert!(
        found.len() <= base,
        "{what}: {} now, baseline {base}. New ones need a reason and a review (G15); the list:\n{}",
        found.len(),
        found.join("\n")
    );
    assert!(
        found.len() == base,
        "{what}: down to {} from {base}. Good: set \"{key}\" to {} in native/tests/conventions-baseline.json.",
        found.len(),
        found.len()
    );
}

#[test]
fn allow_attributes_do_not_grow() {
    compare("allow attributes in src/", "allow", &allows());
}

/// The keys of `design/design.json` that only the manifest has: product code reads them through
/// `design::manifest` (its `key` constants and views), never by a literal. `penpot/diff` reads
/// the Penpot export document, whose `pages` are its own.
const MANIFEST_KEYS: &[&str] = &[
    "screens",
    "variants",
    "pages",
    "sections",
    "breakpoints",
    "studioPages",
    "designedStates",
    "blueprint",
    "flows",
];

#[test]
fn manifest_keys_are_read_through_design_manifest() {
    let src = native().join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    let mut found = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "src/design/manifest.rs"
            || rel.starts_with("src/penpot/diff/")
            || rel.ends_with("tests.rs")
        {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        let product = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (i, line) in product.lines().enumerate() {
            for k in MANIFEST_KEYS {
                if line.contains(&format!(".get(\"{k}\")"))
                    || line.contains(&format!(".get_mut(\"{k}\")"))
                {
                    found.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "design.json keys read by a literal; use design::manifest::key (or its views):\n{}",
        found.join("\n")
    );
}

/// Comment statuses are `review::status::Status`, never literals. "acknowledged", "reopened" and
/// "dismissed" mean nothing else, so they are refused everywhere; "open", "fixed" and
/// "resolved" also name other things (a gate, the Penpot thread field `resolved`, keys of
/// `dfw status --json`), so in the comment modules only a comparison or a value is refused:
/// `Some("fixed")`, `== "fixed"`, `"fixed" |`, `to("fixed")`.
#[test]
fn comment_statuses_are_review_status() {
    const ONLY_STATUS: &[&str] = &["acknowledged", "reopened", "dismissed"];
    const SHARED: &[&str] = &["open", "fixed", "resolved"];
    const COMMENT_MODULES: &[&str] = &[
        "src/review/",
        "src/cli/comments.rs",
        "src/cli/workflow.rs",
        "src/hooks/session_start.rs",
        "src/penpot/threads.rs",
        "src/penpot/live.rs",
        "src/tools/handoff.rs",
    ];
    let src = native().join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    files.sort();
    let mut found = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "src/review/status.rs" || rel.ends_with("tests.rs") {
            continue;
        }
        let comment_module = COMMENT_MODULES.iter().any(|m| rel.starts_with(m));
        let text = fs::read_to_string(&f).unwrap();
        let product = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (i, line) in product.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let bad = ONLY_STATUS
                .iter()
                .any(|k| code.contains(&format!("\"{k}\"")))
                || comment_module
                    && SHARED.iter().any(|k| {
                        [
                            format!("Some(\"{k}\")"),
                            format!("== \"{k}\""),
                            format!("\"{k}\" |"),
                            format!("| \"{k}\""),
                            format!("to(\"{k}\")"),
                            format!("[\"{k}\""),
                        ]
                        .iter()
                        .any(|p| code.contains(p.as_str()))
                    });
            if bad {
                found.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "comment statuses spelled as literals; use review::status::Status:\n{}",
        found.join("\n")
    );
}

/// Tests wait for a condition with a deadline, never a fixed sleep before an assert
/// (testing.md T12): a sleep that is long enough on a laptop is too short on a CI runner
/// (review::inbox lost this way on windows-latest). A sleep inside a polling loop is fine; a
/// sleep that proves something did *not* happen says so with `// T12: ` on the line before.
#[test]
fn tests_do_not_sleep_before_asserting() {
    let mut files = Vec::new();
    rust_files(&native().join("src"), &mut files);
    rust_files(&native().join("tests"), &mut files);
    files.sort();
    let mut found = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let text = fs::read_to_string(&f).unwrap();
        let tests = if rel.ends_with("tests.rs") || rel.starts_with("tests/") {
            text.as_str()
        } else {
            match text.find("#[cfg(test)]") {
                Some(at) => &text[at..],
                None => continue,
            }
        };
        let lines: Vec<&str> = tests.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim();
            if !(t.starts_with("thread::sleep(") || t.starts_with("std::thread::sleep(")) {
                continue;
            }
            let explained = i > 0 && lines[i - 1].contains("// T12: ");
            let next = lines.get(i + 1).map_or("", |l| l.trim());
            if next.starts_with("assert") && !explained {
                found.push(format!("{rel}: {t} / {next}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a fixed sleep right before an assert; poll the condition with a deadline (T12):\n{}",
        found.join("\n")
    );
}

/// Studio paths come from `studio::paths` (the Rust home of the route table; the browser's is
/// `core/studio/shared/paths.js`). `api::OVERLAY_TAG` is text a `const` cannot build; a test in
/// `studio::paths` checks it names the same path.
#[test]
fn studio_paths_are_studio_paths() {
    let mut files = Vec::new();
    rust_files(&native().join("src"), &mut files);
    files.sort();
    let mut found = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if rel == "src/studio/paths.rs" || rel.ends_with("tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        let product = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (i, line) in product.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            if code.contains("/__dfw/") && !code.contains("OVERLAY_TAG: &str") {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "Studio paths spelled as literals; use studio::paths:\n{}",
        found.join("\n")
    );
}

/// The servers name their HTTP methods, status codes and error bodies (`studio::http`'s
/// `method`, `status`, `message`, `error_body`, and `Response::{ok, created, error, ...}`), and
/// the MCP server its JSON-RPC codes (`rpc_code`), as Go's `net/http` does.
#[test]
fn servers_name_methods_statuses_and_errors() {
    let server_files = |rel: &str| {
        rel == "src/studio/server.rs"
            || rel == "src/studio/penpot_routes.rs"
            || rel == "src/studio/sse.rs"
            || rel.starts_with("src/studio/live_proxy/")
            || rel == "src/tools/mcp.rs"
    };
    let methods = [
        "\"GET\"",
        "\"HEAD\"",
        "\"POST\"",
        "\"PATCH\"",
        "\"OPTIONS\"",
    ];
    let codes = [
        "200", "201", "204", "304", "400", "403", "404", "413", "500", "503",
    ];
    let mut files = Vec::new();
    rust_files(&native().join("src"), &mut files);
    files.sort();
    let mut found = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if !server_files(&rel) || rel.ends_with("tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        let product = text.split("#[cfg(test)]").next().unwrap_or_default();
        for (i, line) in product.lines().enumerate() {
            let code = line.split("//").next().unwrap_or_default();
            let number = |n: &str| {
                code.match_indices(n).any(|(at, _)| {
                    let before = code[..at].chars().last();
                    let after = code[at + n.len()..].chars().next();
                    !before.is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_')
                        && !after.is_some_and(|c| c.is_alphanumeric() || c == '.' || c == '_')
                })
            };
            // The definitions themselves: the codes table and the one error reply of MCP.
            if code.trim_start().starts_with("pub const")
                || code.contains("\"error\": { \"code\": code")
            {
                continue;
            }
            let bad = methods.iter().any(|m| code.contains(m))
                || code.contains("\"error\":")
                || code.contains("-327")
                || code.contains("-326")
                || codes.iter().any(|n| number(n));
            if bad {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "HTTP methods, status codes or error bodies spelled out; use studio::http::{{method, status, message}} and Response helpers:\n{}",
        found.join("\n")
    );
}

/// The text of a source file before its test module (`#[cfg(test)]`, maybe more attributes,
/// then `mod name {`). A single `#[cfg(test)]` item before it (a fixture helper) and a test
/// module in its own file (`mod tests;`, read as a test file) do not end the product code.
fn product_part(text: &str) -> &str {
    let mut at = 0;
    let mut lines = text.split_inclusive('\n').peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "#[cfg(test)]" {
            let mut rest = lines.clone();
            let item = rest.find(|l| !l.trim_start().starts_with("#["));
            if item
                .is_some_and(|l| l.trim_start().starts_with("mod ") && l.trim_end().ends_with('{'))
            {
                return &text[..at];
            }
        }
        at += line.len();
    }
    text
}

/// The product source of every Rust file but the skipped ones, as `(path, lines)`.
fn product_sources(skip: &[&str]) -> Vec<(String, Vec<String>)> {
    let mut files = Vec::new();
    rust_files(&native().join("src"), &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if skip.contains(&rel.as_str()) || rel.ends_with("tests.rs") {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        out.push((
            rel,
            product_part(&text)
                .lines()
                .map(|l| l.split("//").next().unwrap_or_default().to_string())
                .collect(),
        ));
    }
    out
}

/// Finding levels are `core::level::Level`, never literals: `"level": "warn"`,
/// `add("warn", ...)`, `f.level == "error"` (also `add(` with the level on the next line).
#[test]
fn finding_levels_are_core_level() {
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&["src/core/level.rs"]) {
        for (i, code) in lines.iter().enumerate() {
            let prev_opens_call = i > 0
                && ["add(", "finding(", "layout("]
                    .iter()
                    .any(|c| lines[i - 1].trim_end().ends_with(c));
            let bad = ["error", "warn", "info"].iter().any(|k| {
                code.contains(&format!("\"level\": \"{k}\""))
                    || code.contains(&format!("level == \"{k}\""))
                    || ["add(", "finding(", "layout("]
                        .iter()
                        .any(|c| code.contains(&format!("{c}\"{k}\"")))
                    || prev_opens_call && code.trim() == format!("\"{k}\",")
            });
            if bad {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "finding levels spelled as literals; use core::level::Level:\n{}",
        found.join("\n")
    );
}

/// Exit codes other than 0 are named (`core::error::exit`, contracts.md C3) in the commands.
#[test]
fn exit_codes_are_named() {
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&[]) {
        if !(rel.starts_with("src/cli/") || rel.starts_with("src/tools/check")) {
            continue;
        }
        for (i, code) in lines.iter().enumerate() {
            let t = code.trim();
            let bad = ["1", "2", "3"].iter().any(|n| {
                code.contains(&format!("Ok({n})"))
                    || code.contains(&format!("{{ {n} }}"))
                    || t == *n
            });
            if bad {
                found.push(format!("{rel}:{}: {t}", i + 1));
            }
        }
    }
    assert!(
        found.is_empty(),
        "exit codes as numbers; use core::error::exit::{{FAILED, PROBLEMS, UNVERIFIED}}:\n{}",
        found.join("\n")
    );
}

/// The files that name the project layout, and so may spell it: `core::paths` and its
/// `layout` module.
const LAYOUT_HOME: [&str; 2] = ["src/core/paths.rs", "src/core/paths/layout.rs"];

/// The design system's folders and files are named once, in `core::paths` (`ProjectPaths`
/// fields, `file`, `in_design`, `rel`), never spelled in a `join` or a message. The Penpot set
/// prefix `themes/` and the Node regex ports that look for `tokens/` anywhere in a path
/// (`lint::is_token_file`, the post-edit hook) are other things and do not match.
#[test]
fn design_system_paths_are_core_paths() {
    let spelled = [
        "design/system",
        "design/tokens",
        "system/head.html",
        "system/components.css",
        "tokens/tokens.",
        "\"head.html\"",
        "\"style.json\"",
        "\"inventory.json\"",
        "\"components.css\"",
        "\"tokens.json\"",
        "\"tokens.css\"",
        "\"brief.json\"",
        "\"style-source.json\"",
        "\"extracted.tokens.json\"",
        "join(\"system\")",
        "join(\"tokens\")",
        "join(\"components\")",
        "join(\"icons\")",
        "join(\"themes\")",
    ];
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&LAYOUT_HOME) {
        for (i, code) in lines.iter().enumerate() {
            if spelled.iter().any(|s| code.contains(s)) {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "design system paths spelled out; use core::paths (ProjectPaths, file, in_design, rel):\n{}",
        found.join("\n")
    );
}

/// The rest of the project layout (the manifest, screens, styles, assets, snapshots, handoff
/// and the files of `.dfw/`) is named once too, in `core::paths`. JSON keys (`"screens"`,
/// `"approved"`), commands (`handoff`), SSE event names and the Studio's URLs (`/__dfw/`, and
/// `/design/`, `studio::api::DESIGN_BASE`) are other things and do not match.
#[test]
fn project_layout_paths_are_core_paths() {
    let spelled = [
        "design/",
        "design/design.json",
        "design/screens",
        "design/styles",
        "design/approved",
        "design/handoff",
        "styles/base.css",
        ".dfw/",
        "\".dfw\"",
        "\"design.json\"",
        "\"base.css\"",
        "\"state.json\"",
        "\"config.json\"",
        "\"config.local.json\"",
        "\"decisions.jsonl\"",
        "\"stack.json\"",
        "\"studio.json\"",
        "\"studio.log\"",
        "\"review.json\"",
        "\"review.log\"",
        "\"presence.json\"",
        "\"layout-cache.json\"",
        "\"approval.lock.json\"",
        "\".delivered.json\"",
        "\".listening.json\"",
        "\"tokens.penpot.json\"",
        "\"tokens.from-penpot.json\"",
        "\"imported.json\"",
        "\"threads.json\"",
        "\"setup.html\"",
        "join(\"design\")",
        "join(\"screens\")",
        "join(\"styles\")",
        "join(\"approved\")",
        "join(\"handoff\")",
        "join(\"reviews\")",
        "join(\"checks\")",
        "join(\"build\")",
        "join(\"shots\")",
        "join(\"edits\")",
        "join(\"penpot\")",
        "join(\"exports\")",
        "join(\"changes\")",
        "join(\"export.json\")",
    ];
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&LAYOUT_HOME) {
        for (i, code) in lines.iter().enumerate() {
            let code_but_urls = code.replace("/design/", "/");
            if spelled.iter().any(|s| code_but_urls.contains(s)) {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "project paths spelled out; use core::paths (ProjectPaths, file, in_design, rel):\n{}",
        found.join("\n")
    );
}

/// The test code of every Rust file, as `(path, line of its first line, text)`: the
/// `#[cfg(test)] mod` part of a source file, a `tests.rs` file, and the binary tests but this
/// one (it holds the patterns).
fn test_texts(skip: &[&str]) -> Vec<(String, usize, String)> {
    let mut files = Vec::new();
    rust_files(&native().join("src"), &mut files);
    rust_files(&native().join("tests"), &mut files);
    files.sort();
    let mut out = Vec::new();
    for f in files {
        let rel = f
            .strip_prefix(native())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if skip.contains(&rel.as_str()) || rel == "tests/conventions.rs" {
            continue;
        }
        let text = fs::read_to_string(&f).unwrap();
        let whole = rel.starts_with("tests/") || rel.ends_with("tests.rs");
        let test = if whole {
            text.clone()
        } else {
            text[product_part(&text).len()..].to_string()
        };
        let first = text[..text.len() - test.len()].lines().count();
        out.push((rel, first, test));
    }
    out
}

/// The test code of every Rust file, as `(path, lines)`, comments cut off; the lines before
/// the test code are empty, so a line's index is its line number less one.
fn test_sources(skip: &[&str]) -> Vec<(String, Vec<String>)> {
    test_texts(skip)
        .into_iter()
        .map(|(rel, first, test)| {
            let lines = std::iter::repeat_n(String::new(), first)
                .chain(
                    test.lines()
                        .map(|l| l.split("//").next().unwrap_or_default().to_string()),
                )
                .collect();
            (rel, lines)
        })
        .collect()
}

/// A line without the raw strings on it (`r#"..."#`, `r##"..."##`): fixture content. A raw
/// string that goes on past the line takes the rest of it.
fn without_raw_strings(code: &str) -> String {
    let mut out = String::new();
    let mut rest = code;
    while let Some(at) = rest.find("r#") {
        let hashes = rest[at + 1..].chars().take_while(|c| *c == '#').count();
        let open = format!("r{}\"", "#".repeat(hashes));
        if !rest[at..].starts_with(&open) {
            out.push_str(&rest[..at + 2]);
            rest = &rest[at + 2..];
            continue;
        }
        out.push_str(&rest[..at]);
        let close = format!("\"{}", "#".repeat(hashes));
        let body = &rest[at + open.len()..];
        rest = body
            .find(&close)
            .map_or("", |end| &body[end + close.len()..]);
    }
    out.push_str(rest);
    out
}

/// Tests build the project paths of their fixtures from `core::paths` too (the binary tests
/// include its `layout` module), so a layout change is made in one place. Fixture data is not
/// a path and may stay a literal: file content (a page's `<link href="../tokens/tokens.css">`,
/// a raw string `r#"..."#` of JSON or HTML), a component name, a Studio URL (`/design/...`).
#[test]
fn tests_build_project_paths_from_core_paths() {
    let spelled = [
        "design/",
        ".dfw/",
        "\".dfw\"",
        "\"design.json\"",
        "\"tokens.json\"",
        "\"tokens.css\"",
        "\"head.html\"",
        "\"components.css\"",
        "\"style.json\"",
        "\"inventory.json\"",
        "\"base.css\"",
        "\"state.json\"",
        "\"config.json\"",
        "\"config.local.json\"",
        "\"decisions.jsonl\"",
        "\"stack.json\"",
        "join(\"design\")",
        "join(\"system\")",
        "join(\"tokens\")",
        "join(\"components\")",
        "join(\"icons\")",
        "join(\"screens\")",
        "join(\"styles\")",
        "join(\"approved\")",
        "join(\"handoff\")",
        "join(\"reviews\")",
        "join(\"penpot\")",
        "join(\"edits\")",
        "join(\"shots\")",
    ];
    let mut found = Vec::new();
    for (rel, lines) in test_sources(&LAYOUT_HOME) {
        for (i, code) in lines.iter().enumerate() {
            let code_but_urls = without_raw_strings(code).replace("/design/", "/");
            if spelled.iter().any(|s| code_but_urls.contains(s)) {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "project paths spelled out in tests; build them from core::paths (project_paths, file, in_design, rel):\n{}",
        found.join("\n")
    );
}

/// Option themes are `design::tokens::option` (`PREFIX`, `theme`, `is_option`, `file`). The
/// flag `--opt-themes` of `dfw tokens export` is a flag, not a theme, and is left out.
#[test]
fn option_themes_are_design_tokens_option() {
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&["src/design/tokens.rs"]) {
        for (i, code) in lines.iter().enumerate() {
            if code.replace("opt-themes", "").contains("opt-") {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "option theme names spelled out; use design::tokens::option:\n{}",
        found.join("\n")
    );
}

/// The modules whose errors (and, for the write gate, refusals) take their text from the
/// module's `message` table.
const MESSAGE_TABLES: [&str; 3] = [
    "src/design/system.rs",
    "src/design/style.rs",
    "src/workflow/gates.rs",
];

/// The errors of the design system and style guide commands and the refusals of the write
/// gate take their text from the module's `message` table, never a literal or a `format!` at
/// the call.
#[test]
fn errors_come_from_their_message_table() {
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&[]) {
        if !MESSAGE_TABLES.contains(&rel.as_str()) {
            continue;
        }
        for (i, code) in lines.iter().enumerate() {
            for call in ["DfwError::new(", "DfwError::internal(", "Decision::deny("] {
                let next = i + 1 < lines.len() && code.trim_end().ends_with(call);
                let arg = match code.find(call) {
                    Some(at) if !next => code[at + call.len()..].trim_start(),
                    Some(_) => lines[i + 1].trim_start(),
                    None => continue,
                };
                if arg.starts_with('"') || arg.starts_with("r\"") || arg.starts_with("format!") {
                    found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "error text written at the call; add it to the module's `message` table:\n{}",
        found.join("\n")
    );
}

/// The hook events, the tool names and the apply_patch headers are named in
/// `core::host::wire` (`event`, `tool`, `patch`); product code never spells them.
#[test]
fn hook_wire_names_are_core_host_wire() {
    let spelled = [
        "\"SessionStart\"",
        "\"PreToolUse\"",
        "\"PostToolUse\"",
        "\"UserPromptSubmit\"",
        "\"Stop\"",
        "\"Write\"",
        "\"Edit\"",
        "\"MultiEdit\"",
        "\"NotebookEdit\"",
        "\"ApplyPatch\"",
        "\"apply_patch\"",
        "\"str_replace_editor\"",
        "\"*** ",
    ];
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&["src/core/host/wire.rs"]) {
        for (i, code) in lines.iter().enumerate() {
            if spelled.iter().any(|s| code.contains(s)) {
                found.push(format!("{rel}:{}: {}", i + 1, code.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "hook events, tool names or patch headers spelled out; use core::host::{{event, tool, patch}}:\n{}",
        found.join("\n")
    );
}

/// One string literal of [`literals`].
struct Literal {
    /// The line of the `#[test]` function it is in, if any.
    test: Option<usize>,
    line: usize,
    /// The text as written between the quotes.
    text: String,
    /// A `json!` object key: followed by a single `:`.
    key: bool,
}

/// The string literals of a piece of Rust code. Comments, raw strings (`r"..."`, `r#"..."#`:
/// fixture content) and character literals are left out.
fn literals(code: &str) -> Vec<Literal> {
    let chars: Vec<char> = code.chars().collect();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    let (mut i, mut line, mut depth) = (0, 1, 0usize);
    let mut pending_test = false;
    // The depth outside the open test function and the line of its `#[test]`.
    let mut test: Option<(usize, usize)> = None;
    let mut recent = String::new();
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        if c == '\n' {
            line += 1;
        }
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                line += usize::from(chars[i] == '\n');
                i += 1;
            }
            i += 2;
            continue;
        }
        if c == 'r' && !ident(prev) {
            let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
            if chars.get(i + 1 + hashes) == Some(&'"') {
                i += 2 + hashes;
                while i < chars.len() {
                    if chars[i] == '"' && chars[i + 1..].iter().take(hashes).all(|c| *c == '#') {
                        break;
                    }
                    line += usize::from(chars[i] == '\n');
                    i += 1;
                }
                i += 1 + hashes;
                continue;
            }
        }
        if c == '\'' {
            if next == Some('\\') {
                i += 3;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
                continue;
            }
            if chars.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
            i += 1;
            continue;
        }
        if c == '"' {
            let start = i + 1;
            i += 1;
            let first_line = line;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1;
                }
                line += usize::from(chars.get(i) == Some(&'\n'));
                i += 1;
            }
            let text: String = chars[start..i.min(chars.len())].iter().collect();
            i += 1;
            let rest: String = chars[i.min(chars.len())..]
                .iter()
                .skip_while(|c| c.is_whitespace())
                .take(2)
                .collect();
            let key = rest.starts_with(':') && !rest.starts_with("::");
            out.push(Literal {
                test: test.map(|(_, at)| at),
                line: first_line,
                text,
                key,
            });
            continue;
        }
        recent.push(c);
        if recent.len() > 16 {
            recent.remove(0);
        }
        if recent.ends_with("#[test]") {
            pending_test = true;
        }
        if c == '{' {
            if pending_test && test.is_none() {
                test = Some((depth, line));
                pending_test = false;
            }
            depth += 1;
        } else if c == '}' {
            depth = depth.saturating_sub(1);
            if test.is_some_and(|(outside, _)| outside == depth) {
                test = None;
            }
        }
        i += 1;
    }
    out
}

/// A string literal that a test function writes twice is a value with a meaning typed twice:
/// it becomes a `const` (in the test, or in the test module when tests share it), or the
/// product's own name for it (a message table, `core::paths`, `core::host::tool`). Counted: in
/// the test code (`#[cfg(test)] mod` of a source file, `*tests.rs`, `tests/*.rs` but this one),
/// every literal of 3 or more characters that one `#[test]` function writes 2 or more times,
/// once per function. Left out: raw strings `r#"..."#` (fixture content), `json!` object keys
/// (`"key":`), a format string that is one placeholder (`"{name}"`: it names a value), comments. The count may only go down (`"repeated_test_literals"` in the baseline).
#[test]
fn test_literals_are_named_once() {
    let mut found = Vec::new();
    for (rel, first, text) in test_texts(&[]) {
        let mut per_test: std::collections::BTreeMap<(usize, String), Vec<usize>> =
            std::collections::BTreeMap::new();
        for lit in literals(&text) {
            let (Some(at), false) = (lit.test, lit.key) else {
                continue;
            };
            let (line, literal) = (lit.line, lit.text);
            let placeholders = literal.starts_with('{')
                && literal.ends_with('}')
                && !literal[1..literal.len() - 1].contains(['{', '}', '"']);
            if literal.chars().count() < 3 || placeholders {
                continue;
            }
            per_test
                .entry((at, literal))
                .or_default()
                .push(line + first);
        }
        for ((_, literal), lines) in per_test {
            if lines.len() > 1 {
                found.push(format!(
                    "{rel}:{}: \"{literal}\" {} times",
                    lines[0],
                    lines.len()
                ));
            }
        }
    }
    compare(
        "string literals written twice in one test",
        "repeated_test_literals",
        &found,
    );
}

/// Where the fixture names of the app's files and of files outside the project live.
const FIXTURE_HOME: &str = "src/core/fixture.rs";

/// Tests name the app's source files they write (`src/a.ts`, `src/App.tsx`) from
/// `core::fixture` (the binary tests include it), never by a literal: a literal that is such a
/// path, or ends with one (`"{root}/src/a.ts"`), is refused. A source location
/// (`src/App.tsx:3:5`) and a path inside file content (a config's `'./src/index.ts'`) do not end
/// with the path and are fixture data.
#[test]
fn tests_name_app_files_from_core_fixture() {
    let app_path = regex::Regex::new(
        r"(^|/)(src|app|lib)/[A-Za-z0-9_./-]*\.(ts|tsx|js|jsx|mjs|vue|css|html)$",
    )
    .unwrap();
    let mut found = Vec::new();
    for (rel, first, text) in test_texts(&[FIXTURE_HOME]) {
        for lit in literals(&text) {
            if app_path.is_match(&lit.text) {
                found.push(format!("{rel}:{}: \"{}\"", lit.line + first, lit.text));
            }
        }
    }
    assert!(
        found.is_empty(),
        "app files spelled out in tests; use core::fixture (APP_FILE, APP_COMPONENT, ...):\n{}",
        found.join("\n")
    );
}

/// The project path helpers a test builds its fixture paths with.
const PATH_HELPERS: [&str; 4] = ["rel::design", "rel::screen", "rel::component", "rel::join"];

/// The string literals given to a [`PATH_HELPERS`] call, as `(line, call, literal)`: an
/// argument, or the format string of a `format!` that is one (`&format!("{id}/a.html")`). A
/// literal given to another function first (`&master_file("button")`: a component name) is
/// that function's. Comments and character literals are left out; a raw string is a literal
/// too; a format string that is one placeholder (`"{name}"`) names a value and is left out.
fn path_helper_literals(code: &str) -> Vec<(usize, &'static str, String)> {
    let chars: Vec<char> = code.chars().collect();
    let ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = Vec::new();
    // One entry per open `(`: the helper it calls, or the one it is nested in.
    let mut open: Vec<Option<&'static str>> = Vec::new();
    let (mut i, mut line) = (0, 1);
    let mut recent = String::new();
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        if c == '\n' {
            line += 1;
        }
        if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && next == Some('*') {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                line += usize::from(chars[i] == '\n');
                i += 1;
            }
            i += 2;
            continue;
        }
        let call = open.last().copied().flatten();
        let raw = c == 'r' && !ident(prev) && {
            let hashes = chars[i + 1..].iter().take_while(|c| **c == '#').count();
            chars.get(i + 1 + hashes) == Some(&'"')
        };
        if raw || c == '"' {
            let hashes = if raw {
                chars[i + 1..].iter().take_while(|c| **c == '#').count()
            } else {
                0
            };
            i += if raw { 2 + hashes } else { 1 };
            let (start, first_line) = (i, line);
            while i < chars.len() {
                if chars[i] == '"' && chars[i + 1..].iter().take(hashes).all(|c| *c == '#') {
                    break;
                }
                if !raw && chars[i] == '\\' {
                    i += 1;
                }
                line += usize::from(chars.get(i) == Some(&'\n'));
                i += 1;
            }
            let text: String = chars[start..i.min(chars.len())].iter().collect();
            i += 1 + hashes;
            let placeholder = text.starts_with('{')
                && text.ends_with('}')
                && !text[1..text.len() - 1].contains(['{', '}', '"']);
            if let (Some(call), false) = (call, placeholder) {
                out.push((first_line, call, text));
            }
            recent.clear();
            continue;
        }
        if c == '\'' {
            if next == Some('\\') {
                i += 3;
                while i < chars.len() && chars[i] != '\'' {
                    i += 1;
                }
                i += 1;
                continue;
            }
            if chars.get(i + 2) == Some(&'\'') {
                i += 3;
                continue;
            }
        }
        if c == '(' {
            let helper = PATH_HELPERS.iter().copied().find(|h| {
                recent.ends_with(h)
                    && !recent[..recent.len() - h.len()]
                        .chars()
                        .next_back()
                        .is_some_and(ident)
            });
            let built_here = recent.ends_with("format!");
            open.push(helper.or(call.filter(|_| built_here)));
        } else if c == ')' {
            open.pop();
        }
        recent.push(c);
        if recent.len() > 32 {
            recent.remove(0);
        }
        i += 1;
    }
    out
}

/// Tests name the design files of their fixtures (a screen page, a component master, a theme)
/// from `core::fixture`, and the product's own files from `core::paths::file`, never by a
/// literal passed to a path helper: `rel::screen("home/a.html")` is refused, so is
/// `rel::screen(&format!("{id}/a.html"))`.
#[test]
fn tests_name_design_files_from_core_fixture() {
    let mut found = Vec::new();
    for (rel, first, text) in test_texts(&[FIXTURE_HOME]) {
        for (line, call, literal) in path_helper_literals(&text) {
            found.push(format!(
                "{rel}:{}: {call}(.. \"{literal}\" ..)",
                line + first
            ));
        }
    }
    assert!(
        found.is_empty(),
        "design fixture files spelled out in tests; use core::fixture (SCREEN_PAGE, BUTTON_MASTER, ...) or core::paths::file:\n{}",
        found.join("\n")
    );
}

/// A binary test starts dfw only through `tests/support/home.rs` (§0 row 43, `testing.md` T9),
/// so every home a user-global file could be written to points into the test's folder.
#[test]
fn binary_tests_start_dfw_through_the_home_helper() {
    let mut found = Vec::new();
    for (rel, lines) in test_sources(&["tests/support/home.rs"]) {
        if !rel.starts_with("tests/") {
            continue;
        }
        for (i, line) in lines.iter().enumerate() {
            if ["Command::new(DFW", "Command::new(&self.exe"]
                .iter()
                .any(|p| line.contains(p))
            {
                found.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "binary tests start dfw with their own Command; use home::command (tests/support/home.rs):\n{}",
        found.join("\n")
    );
}

/// The Studio's route chain compares only `studio::paths` names, so `paths::ALL` holds every
/// path it answers and the refusal test reaches each one (S7, §0 rows 17 and 35).
#[test]
fn studio_routes_are_named_paths() {
    let routers = ["src/studio/server.rs", "src/studio/penpot_routes.rs"];
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&[]) {
        if !routers.contains(&rel.as_str()) {
            continue;
        }
        for (i, line) in lines.iter().enumerate() {
            let t = line.trim_start();
            let literal_arm = t.starts_with('"') && (t.contains("\" =>") || t.contains("\" if "));
            if line.contains("== \"/") || line.contains("strip_prefix(\"/") || literal_arm {
                found.push(format!("{rel}:{}: {}", i + 1, t));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a route compared with a literal; name it in studio::paths (and its ALL):\n{}",
        found.join("\n")
    );
}

/// Regular expressions that match HTML tags, in product code (`design-files.md` D1, §0 row 1):
/// HTML is read with the tokenizer. Each pattern-building call whose literal (on its line, or on
/// the next when the call's line has none) holds a `<` that is not a named group counts, as
/// `file:line`; the count may only go down.
fn html_regexes() -> Vec<String> {
    const CALLS: [&str; 4] = ["Regex::new(", "js_re!(", "js_regex(", "with_js_classes("];
    let literal = regex::Regex::new(r##"r#"(.*?)"#|r"([^"]*)"|"((?:[^"\\]|\\.)*)""##).unwrap();
    let tag = |text: &str| text.replace("(?P<", "").replace("(?<", "").contains('<');
    let mut out = Vec::new();
    for (rel, lines) in product_sources(&["src/core/js/regex.rs"]) {
        for (i, line) in lines.iter().enumerate() {
            let Some(at) = CALLS.iter().filter_map(|c| line.find(c)).min() else {
                continue;
            };
            let here = &line[at..];
            let window = if literal.is_match(here) {
                here.to_string()
            } else {
                format!("{here} {}", lines.get(i + 1).map_or("", String::as_str))
            };
            let first = literal.captures(&window).and_then(|c| {
                (1..=3)
                    .find_map(|g| c.get(g))
                    .map(|m| m.as_str().to_string())
            });
            if first.is_some_and(|text| tag(&text)) {
                out.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    out
}

#[test]
fn html_regexes_do_not_grow() {
    compare(
        "regular expressions with an HTML tag in src/",
        "html_regex",
        &html_regexes(),
    );
}

/// Every write of `.dfw/reviews/` holds the reviews lock: it goes through `write_reviews`, which
/// takes the lock's guard (`process-and-io.md` P7, §0 row 15). `Listening::start` writes the
/// marker of its own process once, without a read before it: the one exception.
#[test]
fn reviews_writes_hold_the_lock() {
    let files = ["src/review/comments.rs", "src/review/inbox.rs"];
    let allowed = ["fn write_reviews(", "fn start("];
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&[]) {
        if !files.contains(&rel.as_str()) {
            continue;
        }
        let mut inside = "";
        for (i, line) in lines.iter().enumerate() {
            if let Some(at) = line.find("fn ") {
                inside = &line[at..];
            }
            if line.contains("write_json_inside(") && !allowed.iter().any(|a| inside.starts_with(a))
            {
                found.push(format!("{rel}:{}: {}", i + 1, line.trim()));
            }
        }
    }
    assert!(
        found.is_empty(),
        "a reviews file written without the lock's guard; use write_reviews(held, ...):\n{}",
        found.join("\n")
    );
}

/// The JSON keys spelled as literals in the product code of the files `checked` picks (all but
/// `skip`), as `file:line: "key"`: a literal read (`.get("…")`, `.get_mut("…")`), write
/// (`insert("…"`, `shift_remove("…")`) or `json!` object key (`"…":`). A literal that is a
/// value does not match.
fn literal_keys(skip: &[&str], checked: impl Fn(&str) -> bool) -> Vec<String> {
    let call = regex::Regex::new(r#"(?:\.get|\.get_mut|insert|shift_remove)\("((?:[^"\\]|\\.)*)""#)
        .unwrap();
    let literal = regex::Regex::new(r#""((?:[^"\\]|\\.)*)""#).unwrap();
    let mut found = Vec::new();
    for (rel, lines) in product_sources(skip) {
        if !checked(&rel) {
            continue;
        }
        for (i, line) in lines.iter().enumerate() {
            let object_keys = literal.captures_iter(line).filter(|c| {
                let rest = line[c.get(0).unwrap().end()..].trim_start();
                rest.starts_with(':') && !rest.starts_with("::")
            });
            for c in call.captures_iter(line).chain(object_keys) {
                found.push(format!("{rel}:{}: \"{}\"", i + 1, &c[1]));
            }
        }
    }
    found
}

/// The keys of the files dfw and the dfw bridge plugin exchange (the export, the change file,
/// the live sync, the threads ledger) are named once, in `penpot::wire` (`mod.rs`), and read
/// and written through those names (the token files are `penpot::tokens`' own). A key
/// spelled as a literal compiles even when it is misspelled ([`literal_keys`]).
#[test]
fn penpot_keys_are_penpot_wire() {
    let found = literal_keys(&["src/penpot/mod.rs"], |rel| {
        rel.starts_with("src/penpot/") && !rel.starts_with("src/penpot/tokens/")
    });
    assert!(
        found.is_empty(),
        "JSON keys spelled as literals in src/penpot/; name them in penpot::wire (mod.rs):\n{}",
        found.join("\n")
    );
}

/// The keys of the workflow files (`.dfw/state.json` and its gate records and history,
/// `decisions.jsonl`, `stack.json`, a snapshot's `approval.lock.json`, `studio.json`,
/// `review.json`) are named once, by the module that owns the file (`workflow::state::field`,
/// `decisions::field`, `stack::field`, `freeze::field`, `server::field`). A misspelt key reads
/// nothing: for `design.locked` that opens the frozen-design check. Refused in `src/workflow/`,
/// `src/cli/workflow.rs` and `src/cli/studio.rs`: a literal key ([`literal_keys`]).
#[test]
fn workflow_keys_are_named() {
    let found = literal_keys(&[], |rel| {
        rel.starts_with("src/workflow/")
            || rel == "src/cli/workflow.rs"
            || rel == "src/cli/studio.rs"
    });
    assert!(
        found.is_empty(),
        "JSON keys spelled as literals; name them in the module that owns the file (workflow::state::field, ...):\n{}",
        found.join("\n")
    );
}

/// Untyped JSON reads by a literal key (`.get("…")`, `.get_mut("…")`) in product code of
/// `src/`, once per read as `file:line`: the JSON debt that later changes name or type. The
/// count may only go down (`"json_key_literals"` in the baseline).
#[test]
fn json_key_literals_do_not_grow() {
    let read = regex::Regex::new(r#"\.get(?:_mut)?\("(?:[^"\\]|\\.)*"\)"#).unwrap();
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&[]) {
        for (i, line) in lines.iter().enumerate() {
            for _ in read.find_iter(line) {
                found.push(format!("{rel}:{}", i + 1));
            }
        }
    }
    compare(
        "JSON reads by a literal key in src/",
        "json_key_literals",
        &found,
    );
}
