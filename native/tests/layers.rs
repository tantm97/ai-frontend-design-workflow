//! Architecture rule A2 (docs/conventions/architecture.md), checked: every `crate::<group>` edge
//! in product code goes down the layer table. A cycle or an upward call fails this test with the
//! file, the line and both layers. Test code (`#[cfg(test)]` items, `tests.rs` files) is skipped:
//! a test may build fixtures from any layer.

#![allow(
    clippy::disallowed_methods,
    reason = "binary tests build fixtures with std::fs and std::process directly"
)]

use std::fs;
use std::path::{Path, PathBuf};

/// The layer table of A2, as data. `None` = the side module `assets` (usable by every layer).
const LAYERS: &[(&str, Option<u8>)] = &[
    ("core", Some(0)),
    ("workflow", Some(1)),
    ("design", Some(1)),
    ("review", Some(2)),
    ("browser", Some(2)),
    ("penpot", Some(3)),
    ("tools", Some(3)),
    ("hooks", Some(3)),
    ("studio", Some(4)),
    ("cli", Some(5)),
    ("telemetry", Some(5)),
    ("main", Some(5)),
    ("assets", None),
];

/// Edges A2 lists under "Known exceptions", each with its reason.
const KNOWN_EXCEPTIONS: &[(&str, &str, &str)] = &[(
    "hooks",
    "tools",
    "hooks/post_tool_use.rs runs tools::check, a module of the same layer; tools never uses hooks",
)];

fn layer(group: &str) -> Option<Option<u8>> {
    LAYERS.iter().find(|(g, _)| *g == group).map(|(_, l)| *l)
}

/// Whether `from` may use `to`: a lower layer, its own group, the side module, or anything from
/// layer 5. `assets` itself may use only `core`.
fn allowed(from: &str, to: &str) -> bool {
    if from == to || to == "assets" {
        return true;
    }
    if KNOWN_EXCEPTIONS
        .iter()
        .any(|(f, t, _)| *f == from && *t == to)
    {
        return true;
    }
    match (layer(from), layer(to)) {
        (Some(None), _) => to == "core",
        (Some(Some(5)), Some(_)) => true,
        (Some(Some(a)), Some(Some(b))) => b < a,
        _ => false,
    }
}

/// The group a source file belongs to: its first folder under `src/`, or the file stem
/// (`main.rs` -> `main`, `assets.rs` -> `assets`).
fn group_of(rel: &Path) -> String {
    let mut parts = rel.components();
    let first = parts
        .next()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .unwrap_or_default();
    match first.strip_suffix(".rs") {
        Some(stem) if parts.next().is_none() => stem.to_string(),
        _ => first,
    }
}

/// The product lines of a file: `#[cfg(test)]` items (a module or a function, by brace depth)
/// are left out. Returns (line number, text).
fn product_lines(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut skipping = false;
    let mut pending = false;
    let mut depth: i64 = 0;
    for (i, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if !skipping && t.starts_with("#[cfg(test)]") {
            pending = true;
            continue;
        }
        if pending {
            if t.starts_with("#[") || t.is_empty() {
                continue;
            }
            pending = false;
            skipping = true;
            depth = 0;
        }
        if skipping {
            depth += line.matches('{').count() as i64 - line.matches('}').count() as i64;
            // An item without a body (`mod tests;`, `use ...;`) ends at its semicolon.
            if depth <= 0 && (line.contains('}') || t.ends_with(';')) {
                skipping = false;
            }
            continue;
        }
        out.push((i + 1, line));
    }
    out
}

/// The `crate::<group>` edges of a line, comments left out.
fn edges(line: &str) -> Vec<String> {
    let code = line.split("//").next().unwrap_or("");
    let mut out = Vec::new();
    let mut rest = code;
    while let Some(at) = rest.find("crate::") {
        rest = &rest[at + "crate::".len()..];
        let name: String = rest
            .chars()
            .take_while(|c| c.is_ascii_lowercase() || *c == '_')
            .collect();
        if !name.is_empty() {
            out.push(name);
        }
    }
    out
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

/// Every upward or sideways edge of product code, as printable lines.
fn violations(src: &Path) -> Vec<String> {
    let mut files = Vec::new();
    rust_files(src, &mut files);
    files.sort();
    let mut out = Vec::new();
    for file in files {
        let rel = file.strip_prefix(src).unwrap().to_path_buf();
        if rel.file_name().is_some_and(|n| n == "tests.rs") {
            continue;
        }
        let from = group_of(&rel);
        let text = fs::read_to_string(&file).unwrap();
        for (n, line) in product_lines(&text) {
            for to in edges(line) {
                if layer(&to).is_none() || allowed(&from, &to) {
                    continue;
                }
                out.push(format!(
                    "src/{}:{n}: `{from}` (layer {}) uses `crate::{to}` (layer {}): A2 allows only lower layers (docs/conventions/architecture.md)",
                    rel.display(),
                    show(layer(&from)),
                    show(layer(&to)),
                ));
            }
        }
    }
    out
}

fn show(l: Option<Option<u8>>) -> String {
    match l {
        Some(Some(n)) => n.to_string(),
        Some(None) => "side".into(),
        None => "?".into(),
    }
}

#[test]
fn product_code_follows_the_layer_table() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let found = violations(&src);
    assert!(
        found.is_empty(),
        "{} layer violation(s):\n{}",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn every_group_under_src_is_in_the_table() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut missing = Vec::new();
    for e in fs::read_dir(&src).unwrap().flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let group = name.strip_suffix(".rs").unwrap_or(&name).to_string();
        if layer(&group).is_none() {
            missing.push(group);
        }
    }
    assert!(
        missing.is_empty(),
        "add these to LAYERS (and to the A2 table): {missing:?}"
    );
}

#[test]
fn the_checker_reports_an_upward_edge_and_skips_test_code() {
    let dir = std::env::temp_dir().join(format!("dfw-layers-{}", std::process::id()));
    let src = dir.join("src");
    fs::create_dir_all(src.join("core")).unwrap();
    fs::write(
        src.join("core/bad.rs"),
        "use crate::core::util;\nfn f() { crate::browser::cdp::x(); }\n#[cfg(test)]\nmod tests {\n    use crate::studio::api;\n}\n#[cfg(test)]\nfn helper() {\n    crate::cli::run();\n}\nfn g() { crate::design::y(); } // crate::studio in a comment\n",
    )
    .unwrap();
    let found = violations(&src);
    fs::remove_dir_all(&dir).ok();
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(found[0].contains("src/core/bad.rs:2") && found[0].contains("`crate::browser`"));
    assert!(found[1].contains("src/core/bad.rs:11") && found[1].contains("`crate::design`"));
}
