//! Ports `core/lib/inventory.mjs`: component inventory against masters, kit references.
//!
//! The design system is designed for the product, not copied from a kit:
//! `design/system/inventory.json` lists every component the product needs, why, and which pages
//! use it. The starter kit (`kits/base/components`) is a read-only reference, never copied.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::html::{Quote, Tag, Token, Tokenizer};
use super::tokens::{dir_names, relative_path};
use crate::assets::{self, Folder};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::level::Level;
use crate::core::paths::{file, project_paths, rel};
use crate::core::util::write_json_inside;

/// Rules that block the DS gate (escalated from SHOULD).
pub const DS_GATE_RULES: [&str; 4] = [
    "inventory-missing",
    "inventory-orphan",
    "inventory-unbuilt",
    "inventory-invalid",
];

/// The rule of a master or examples copied from the starter kit verbatim.
pub(crate) const KIT_COPY: &str = "kit-copy";

/// The blocks of a master a kit copy is recognised by.
const STYLE_BLOCK: &str = "style";
const TEMPLATE_BLOCK: &str = "template";
const EXAMPLES_BLOCK: &str = "dfw-examples";

const STATES: [&str; 11] = [
    "hover",
    "focus-visible",
    "active",
    "disabled",
    "aria-busy",
    "aria-invalid",
    "aria-selected",
    "aria-current",
    "checked",
    "placeholder",
    "empty",
];

const SKELETON_COMMENT: &str = "Component inventory (dfw system phase). One entry per component the product needs, derived from PRODUCT.md workflows and the screen blueprints, never from a kit list. name = master file name; purpose = the job it does in THIS product; usedBy = pages/screens that need it; states = every state its CSS must cover; ref = optional kit reference consulted with \"dfw system ref <name>\" (a checklist, never copied).";

/// The starter kit's masters (`KIT_COMPONENTS`), a folder of the embedded kits.
const KIT_COMPONENTS: &str = "base/components";

/// The file names of the kit's masters, in byte order.
fn kit_files() -> Vec<String> {
    assets::file_names(Folder::Kits, KIT_COMPONENTS)
        .into_iter()
        .filter(|f| f.ends_with(".html"))
        .collect()
}

/// The text of one kit master by its file name.
fn kit_text(file: &str) -> Option<String> {
    assets::text(Folder::Kits, &format!("{KIT_COMPONENTS}/{file}"))
}

/// Test fixtures only: the kit's masters in the source tree, to copy into a test project.
#[cfg(test)]
pub fn kit_components() -> Result<PathBuf, DfwError> {
    Ok(Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/kits/base/components"))
}

/// `inventoryPath`: `design/system/inventory.json`.
pub fn inventory_path(root: &Path) -> PathBuf {
    project_paths(root).inventory
}

/// `loadInventory`: the inventory, `None` when the file does not exist.
pub fn load_inventory(root: &Path) -> Result<Option<Value>, DfwError> {
    let f = inventory_path(root);
    if !f.exists() {
        return Ok(None);
    }
    let inv = js::read_json(&f, Some(Value::Null))?;
    if !inv.get("components").is_some_and(Value::is_array) {
        return Err(DfwError::new(format!(
            "{}: expected {{\"components\": [...]}}",
            relative_path(root, &f)
        )));
    }
    Ok(Some(inv))
}

/// `ensureInventory`: create the inventory skeleton if missing. Returns true when written.
pub fn ensure_inventory(root: &Path) -> Result<bool, DfwError> {
    let f = inventory_path(root);
    if f.exists() {
        return Ok(false);
    }
    write_json_inside(
        root,
        &f,
        &json!({ "$comment": SKELETON_COMMENT, "components": [] }),
    )?;
    Ok(true)
}

/// Comments removed, white space collapsed, trimmed: two blocks that differ only in layout or
/// comments compare equal.
fn norm(text: &str) -> String {
    let mut plain = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        let Some(len) = rest[start + 4..].find("-->") else {
            break;
        };
        plain.push_str(&rest[..start]);
        rest = &rest[start + 4 + len + 3..];
    }
    plain.push_str(rest);
    plain
        .split(js::is_space)
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// The normalised content of the first `<tag>` element of a file (empty when there is none).
fn block(text: &str, tag: &str) -> String {
    let mut tokens = Tokenizer::new(text);
    let open = tokens.find_map(|t| match t {
        Token::Open(open) if open.is(text, tag) => Some(open),
        _ => None,
    });
    let close = tokens.find_map(|t| match t {
        Token::Close(close) if close.is(text, tag) => Some(close),
        _ => None,
    });
    match (open, close) {
        (Some(open), Some(close)) => norm(&text[open.range.end..close.range.start]),
        _ => String::new(),
    }
}

/// The first `<dfw-component>` tag of a master file.
fn component_tag(text: &str) -> Option<Tag> {
    Tokenizer::new(text).find_map(|t| match t {
        Token::Open(tag) if tag.name(text) == "dfw-component" => Some(tag),
        _ => None,
    })
}

/// The value of a double-quoted attribute as written (the Node patterns read only `name="..."`).
/// The Node patterns also took the first `>` for the end of the tag, even inside a quoted
/// value: an attribute that ends after it was not seen, and is not seen here.
fn double_quoted<'a>(text: &'a str, tag: &Tag, name: &str) -> Option<&'a str> {
    let seen_end = tag.range.start + text[tag.range.clone()].find('>')?;
    let attr = tag
        .attr(text, name)
        .filter(|a| a.quote == Quote::Double && a.span.end <= seen_end)?;
    attr.value.clone().map(|r| &text[r])
}

fn is_component_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn html_files(dir: &Path) -> Result<Vec<String>, DfwError> {
    Ok(dir_names(dir)?
        .into_iter()
        .filter(|f| f.ends_with(".html"))
        .collect())
}

fn file_stem(file: &str) -> String {
    file.strip_suffix(".html").unwrap_or(file).to_string()
}

/// One starter-kit master, as a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KitRef {
    pub name: String,
    pub group: String,
    pub description: String,
    pub variants: String,
    pub sizes: String,
    /// The states its CSS covers.
    pub states: Vec<String>,
}

impl KitRef {
    /// The object the Node CLI printed with `--json`.
    pub fn to_json(&self) -> Value {
        json!({ "name": self.name, "group": self.group, "description": self.description, "variants": self.variants, "sizes": self.sizes, "states": self.states })
    }
}

/// `kitRefs`: name, group, description, variants/sizes and covered states of every kit master.
pub fn kit_refs() -> Result<Vec<KitRef>, DfwError> {
    let mut files = kit_files();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    let mut refs = Vec::new();
    for f in files {
        let text = kit_text(&f).unwrap_or_default();
        let open = component_tag(&text);
        let attr = |name: &str| {
            open.as_ref()
                .and_then(|tag| double_quoted(&text, tag, name))
                .unwrap_or_default()
                .to_string()
        };
        let css = block(&text, STYLE_BLOCK);
        refs.push(KitRef {
            name: file_stem(&f),
            group: attr("group"),
            description: attr("description"),
            variants: attr("variants"),
            sizes: attr("sizes"),
            states: STATES
                .iter()
                .filter(|s| css.contains(**s))
                .map(|s| s.to_string())
                .collect(),
        });
    }
    Ok(refs)
}

/// `kitRef`: full source of one kit master, for reading as a reference.
pub fn kit_ref(name: &str) -> Result<String, DfwError> {
    if let Some(text) = kit_text(&file::master(name)) {
        return Ok(text);
    }
    let names: Vec<String> = kit_refs()?.into_iter().map(|r| r.name).collect();
    Err(DfwError::new(format!(
        "No kit reference \"{name}\". Available: {}",
        names.join(", ")
    )))
}

/// Normalised style, template and examples blocks of the kit, each with the master it is from.
struct KitFingerprints {
    styles: HashMap<String, String>,
    templates: HashMap<String, String>,
    examples: HashMap<String, String>,
}

fn kit_fingerprints() -> Result<KitFingerprints, DfwError> {
    let mut kit = KitFingerprints {
        styles: HashMap::new(),
        templates: HashMap::new(),
        examples: HashMap::new(),
    };
    for f in kit_files() {
        let text = kit_text(&f).unwrap_or_default();
        for (map, tag) in [
            (&mut kit.styles, STYLE_BLOCK),
            (&mut kit.templates, TEMPLATE_BLOCK),
            (&mut kit.examples, EXAMPLES_BLOCK),
        ] {
            let b = block(&text, tag);
            if !b.is_empty() {
                map.insert(b, file_stem(&f));
            }
        }
    }
    Ok(kit)
}

/// Component names a file uses (`<dfw-use c="name">`), outside `<dfw-examples>`.
fn uses_in(text: &str, used: &mut BTreeSet<String>) {
    let mut tokens = Tokenizer::new(text);
    while let Some(token) = tokens.next() {
        let Token::Open(tag) = token else { continue };
        if &text[tag.range.clone()] == "<dfw-examples>" {
            let close = tokens.clone().find_map(|t| match t {
                Token::Close(c) if &text[c.range.clone()] == "</dfw-examples>" => Some(c.range.end),
                _ => None,
            });
            if let Some(end) = close {
                tokens.seek(end);
            }
        } else if tag.name(text) == "dfw-use" {
            if let Some(name) = double_quoted(text, &tag, "c").filter(|n| is_component_name(n)) {
                used.insert(name.to_string());
            }
        }
    }
}

/// Names used in screens and in other masters.
fn usages(root: &Path) -> Result<BTreeSet<String>, DfwError> {
    fn scan(dir: &Path, used: &mut BTreeSet<String>) -> Result<(), DfwError> {
        for name in dir_names(dir)? {
            let abs = dir.join(&name);
            if abs.is_dir() {
                scan(&abs, used)?;
            } else if name.ends_with(".html") {
                uses_in(&js::read_text(&abs)?, used);
            }
        }
        Ok(())
    }
    let p = project_paths(root);
    let mut used = BTreeSet::new();
    scan(&p.screens, &mut used)?;
    scan(&p.components, &mut used)?;
    Ok(used)
}

/// `inventoryFindings`: inventory and kit-copy findings for the design system.
/// - kit-copy (MUST): a master whose styles + template, or whose examples, are the kit's verbatim.
/// - inventory-missing / inventory-orphan / inventory-unbuilt (SHOULD; MUST at the DS gate).
/// - unused-master (SHOULD): once real screens exist, a master no screen or other master uses.
pub fn inventory_findings(root: &Path) -> Result<Vec<Value>, DfwError> {
    const INVENTORY: &str = rel::INVENTORY;
    let p = project_paths(root);
    let dir = &p.components;
    let mut findings = Vec::new();
    let mut add = |level: Level, rule: &str, file: &str, message: String| {
        findings.push(json!({ "level": level.as_str(), "rule": rule, "file": file, "line": 1, "message": message, "source": "dfw-ds" }));
    };
    let mut masters = html_files(dir)?;
    masters.sort_by(|a, b| js::cmp_utf16(a, b));
    let kit = kit_fingerprints()?;
    let mut names: Vec<(String, String)> = Vec::new();
    for f in &masters {
        let rel = rel::component(f);
        let text = js::read_text(&dir.join(f))?;
        for token in Tokenizer::new(&text) {
            let Token::Open(tag) = token else { continue };
            if tag.name(&text) != "dfw-component" {
                continue;
            }
            if let Some(name) = double_quoted(&text, &tag, "name").filter(|n| is_component_name(n))
            {
                names.push((name.to_string(), rel.clone()));
            }
        }
        let st = block(&text, STYLE_BLOCK);
        let tp = block(&text, TEMPLATE_BLOCK);
        let ex = block(&text, EXAMPLES_BLOCK);
        match (kit.templates.get(&tp), kit.examples.get(&ex)) {
            (Some(from), _) if !st.is_empty() && !tp.is_empty() && kit.styles.contains_key(&st) => {
                add(Level::Error, KIT_COPY, &rel, format!("Master is the starter kit's \"{from}\" copied verbatim. Design it for this product (its job, content, states and the direction's look); the kit is a reference only (\"dfw system ref {from}\")."));
            }
            (_, Some(from)) if !ex.is_empty() => {
                add(Level::Error, KIT_COPY, &rel, format!("Examples are the starter kit's \"{from}\" examples (another product's content). Show this product's real content in every variant and state."));
            }
            _ => {}
        }
    }
    let inv = match load_inventory(root) {
        Ok(inv) => inv,
        Err(err) => {
            add(
                Level::Error,
                "inventory-invalid",
                INVENTORY,
                err.message().to_string(),
            );
            return Ok(findings);
        }
    };
    let entries: Vec<Value> = inv
        .as_ref()
        .and_then(|i| i.get("components"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if entries.is_empty() {
        if !masters.is_empty() {
            add(Level::Warn, "inventory-missing", INVENTORY, format!("No component inventory. List every component this product needs in {INVENTORY} (name, purpose, usedBy, states) before designing masters, so the system holds only what the screens use."));
        }
        return Ok(findings);
    }
    let built = |name: &Value| names.iter().any(|(n, _)| name.as_str() == Some(n));
    for c in &entries {
        let name = c.get("name").filter(|n| js::truthy(Some(n)));
        match name {
            None => add(
                Level::Warn,
                "inventory-invalid",
                INVENTORY,
                "An inventory entry has no name.".to_string(),
            ),
            Some(name) => {
                let used_by = c
                    .get("usedBy")
                    .and_then(Value::as_array)
                    .is_some_and(|u| !u.is_empty());
                if !js::truthy(c.get("purpose")) || !used_by {
                    add(Level::Warn, "inventory-invalid", INVENTORY, format!("Inventory entry \"{}\" needs a purpose and at least one usedBy page/screen: a component nobody needs does not belong in the system.", js::to_string(Some(name))));
                }
                if !built(name) {
                    add(
                        Level::Warn,
                        "inventory-unbuilt",
                        INVENTORY,
                        format!(
                            "Inventory lists \"{}\" but there is no master for it in {}/.",
                            js::to_string(Some(name)),
                            rel::COMPONENTS
                        ),
                    );
                }
            }
        }
    }
    for (name, rel) in &names {
        let listed = entries
            .iter()
            .any(|c| c.get("name").and_then(Value::as_str) == Some(name));
        if !listed {
            add(Level::Warn, "inventory-orphan", rel, format!("Master \"{name}\" is not in {INVENTORY}. Add it with the screens that need it, or delete it."));
        }
    }
    // A malformed design.json is reported by lint (manifest-json).
    let manifest = js::read_json(&p.manifest, Some(json!({}))).unwrap_or_else(|_| json!({}));
    let real_screens = manifest
        .get("screens")
        .and_then(Value::as_array)
        .is_some_and(|screens| {
            screens
                .iter()
                .any(|s| !js::to_string(s.get("id")).starts_with('_'))
        });
    if real_screens {
        let used = usages(root)?;
        for (name, rel) in &names {
            if !used.contains(name) {
                add(Level::Warn, "unused-master", rel, format!("No screen or master uses \"{name}\". Remove it (and its inventory entry) or use it."));
            }
        }
    }
    Ok(findings)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A component of the starter kit.
    const KIT_BUTTON: &str = "button";

    #[test]
    fn blocks_compare_without_comments_and_layout() {
        let a = "<dfw-component name=\"x\">\n<style>\n/* c */ .a {  color: red; }\n</style>\n<template>\n<!-- root -->\n<b>\n x</b>\n</template></dfw-component>";
        assert_eq!(block(a, STYLE_BLOCK), "/* c */ .a { color: red; }");
        assert_eq!(block(a, TEMPLATE_BLOCK), "<b> x</b>");
        assert_eq!(block(a, EXAMPLES_BLOCK), "");
        assert_eq!(block("<template>never closed", TEMPLATE_BLOCK), "");
    }

    #[test]
    fn usages_skip_examples_and_read_double_quoted_names() {
        let mut used = BTreeSet::new();
        uses_in("<dfw-use c=\"a\"></dfw-use><dfw-examples><dfw-use c=\"b\"></dfw-use></dfw-examples><dfw-use c='c'></dfw-use><dfw-use id=\"x\" c=\"d-1\"/><!-- <dfw-use c=\"e\"> --><dfw-use c=\"F\">", &mut used);
        assert_eq!(used.into_iter().collect::<Vec<_>>(), ["a", "d-1"]);
    }

    #[test]
    fn kit_references_list_states() {
        let refs = kit_refs().unwrap();
        assert!(refs
            .iter()
            .any(|r| r.name == KIT_BUTTON && r.states.iter().any(|s| s == "focus-visible")));
        assert!(kit_ref(KIT_BUTTON)
            .unwrap()
            .contains("<dfw-component name=\"button\""));
        let err = kit_ref("nope").unwrap_err();
        assert!(
            err.message()
                .starts_with("No kit reference \"nope\". Available: "),
            "{err}"
        );
    }
}
