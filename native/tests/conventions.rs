//! Convention checks on the source of `src/` (and, for the project layout, of the tests).
//!
//! The growth guard of convention G15 (git-and-review.md): the number of suppressions
//! (`#[allow(...)]` in product code) may only go down. A higher count fails with the new ones; a lower count fails until the
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
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            let attr = t.starts_with("#[allow(")
                || t.starts_with("#![allow(")
                || (t.starts_with("#[cfg_attr(") || t.starts_with("#![cfg_attr("))
                    && t.contains("allow(");
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
        "{what}: {} now, baseline {base}. New suppressions need a reason and a review (G15); the list:\n{}",
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

/// The test code of every Rust file, as `(path, lines)`: the `#[cfg(test)] mod` part of a
/// source file, a `tests.rs` file, and the binary tests but this one (it holds the patterns).
fn test_sources(skip: &[&str]) -> Vec<(String, Vec<String>)> {
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
            &text[..]
        } else {
            &text[product_part(&text).len()..]
        };
        let first = text[..text.len() - test.len()].lines().count();
        out.push((
            rel,
            std::iter::repeat_n(String::new(), first)
                .chain(
                    test.lines()
                        .map(|l| l.split("//").next().unwrap_or_default().to_string()),
                )
                .collect(),
        ));
    }
    out
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

/// The errors of the design system and style guide commands take their text from the
/// module's `message` table, never a literal or a `format!` at the call.
#[test]
fn system_and_style_errors_come_from_their_message_table() {
    let mut found = Vec::new();
    for (rel, lines) in product_sources(&[]) {
        if rel != "src/design/system.rs" && rel != "src/design/style.rs" {
            continue;
        }
        for (i, code) in lines.iter().enumerate() {
            for call in ["DfwError::new(", "DfwError::internal("] {
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
