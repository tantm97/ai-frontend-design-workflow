//! Ports `core/lib/compose.mjs`: component masters, instances, slots, conditions, `build_system`.
//!
//! Screens reuse components with `<dfw-use>` and icons with `<dfw-icon>`; this module expands
//! them into plain HTML. Studio, checks, screenshots and handoff all see the composed output, so
//! a change to a component master updates every screen.
//!
//! The Node module found tags with regular expressions; here every tag comes from
//! `design::html`. Well-formed files compose to the same bytes. A tag written inside a comment,
//! a quoted attribute, `<script>` or `<style>` is no longer taken for a real one.
//!
//! This file holds the attribute helpers every part shares; one file per job: `library`,
//! `template`, `expand`, `build` (the tests of the whole module are in `tests.rs`).

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{json, Map, Value};

use super::html::{element_at, escape_html, line_of, parse_attrs, Attr, Tag, Token, Tokenizer};
use super::tokens::{dir_names, relative_path};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{in_design, project_paths, rel, resolve, to_posix};

/// Attributes of `<dfw-use>` that are not props and are not copied to the root element.
const RESERVED: [&str; 6] = ["c", "component", "variant", "size", "id", "class"];

/// The object `parseAttrs` built: decoded values by name, in the order the names first appear;
/// a repeated name keeps its place and takes the later value.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Attrs(Vec<(String, String)>);

impl Attrs {
    /// The value of `name` (empty for an attribute without a value).
    pub fn get(&self, name: &str) -> Option<&str> {
        self.0
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    /// The value of `name` when it is not empty (`attrs.name || ...` in the Node code).
    pub fn nonempty(&self, name: &str) -> Option<&str> {
        self.get(name).filter(|v| !v.is_empty())
    }

    /// Set `name`, keeping its place when it exists.
    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        match self.0.iter_mut().find(|(k, _)| k == name) {
            Some(slot) => slot.1 = value.into(),
            None => self.0.push((name.to_string(), value.into())),
        }
    }

    /// Name and value pairs in order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }
}

/// `parseAttrs`: the attributes of a tag as decoded name and value pairs.
pub fn attrs_of(src: &str, attrs: &[Attr]) -> Attrs {
    let mut out = Attrs::default();
    for attr in attrs {
        out.set(&src[attr.name.clone()], super::html::attr_value(src, attr));
    }
    out
}

/// `attrsToString`: ` name="value"` for each attribute; an empty value prints the bare name
/// except for `data-*`, `aria-*`, `id`, `class`, `alt` and `value`.
pub fn attrs_to_string(attrs: &Attrs) -> String {
    let keeps_empty = |k: &str| {
        k.starts_with("data-")
            || k.starts_with("aria-")
            || matches!(k, "id" | "class" | "alt" | "value")
    };
    attrs
        .iter()
        .map(|(k, v)| {
            if v.is_empty() && !keeps_empty(k) {
                format!(" {k}")
            } else {
                format!(" {k}=\"{}\"", escape_html(v))
            }
        })
        .collect()
}

mod build;
mod expand;
mod library;
mod template;

pub use build::*;
pub use expand::*;
pub use library::*;
use template::*;

#[cfg(test)]
mod tests;
