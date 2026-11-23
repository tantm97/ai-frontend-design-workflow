//! Convention checks on the source of `src/`.
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
