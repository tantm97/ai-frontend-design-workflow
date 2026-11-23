//! The tokens the page shows: every `design/tokens/*.json`, the css variables with aliases
//! followed, and the colour lookups (`HEX_LOOKUP`, `hexOf`, `aliasOf`).

use std::collections::HashMap;
use std::path::Path;

use regex::{Captures, Regex};
use serde_json::{json, Value};

use super::text::{alias, hex_field, slice_inner};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::with_js_classes;
use crate::core::paths::project_paths;
use crate::design::tokens::{collect_tokens, TokenEntry};
use crate::design::tokens::{dir_names, file_declarations};

/// `loadTokens`: the tokens of every `design/tokens/*.json`.
pub(super) fn load_tokens(root: &Path) -> Result<Vec<TokenEntry>, DfwError> {
    let dir = project_paths(root).tokens;
    let mut out = Vec::new();
    for f in dir_names(&dir)? {
        if f.ends_with(".json") {
            let tree = js::read_json(&dir.join(&f), Some(json!({})))?;
            out.extend(collect_tokens(&tree));
        }
    }
    Ok(out)
}

/// `varMap`: css variable to css value, with aliases followed.
pub(super) struct Vars {
    map: HashMap<String, String>,
    var: Regex,
}

impl Vars {
    pub(super) fn load(root: &Path) -> Result<Vars, DfwError> {
        let file = project_paths(root).tokens_json;
        let tree = if file.exists() {
            js::read_json(&file, Some(json!({})))?
        } else {
            json!({})
        };
        Ok(Vars {
            map: file_declarations(root, &file, &tree)?.into_iter().collect(),
            var: with_js_classes(r"var\((--[\w-]+)\)")?,
        })
    }

    pub(super) fn resolve(&self, v: &str, depth: usize) -> String {
        if depth > 8 {
            return v.to_string();
        }
        self.var
            .replace_all(v, |m: &Captures| match self.map.get(&m[1]) {
                Some(value) => self.resolve(value, depth + 1),
                None => m[0].to_string(),
            })
            .into_owned()
    }
}

/// The tokens by name (the last one of a name wins, as in a `Map`).
pub(super) struct Tokens<'a> {
    by_name: HashMap<&'a str, &'a TokenEntry>,
}

impl<'a> Tokens<'a> {
    pub(super) fn new(all: &'a [TokenEntry]) -> Self {
        Tokens {
            by_name: all.iter().map(|t| (t.name.as_str(), t)).collect(),
        }
    }

    pub(super) fn get(&self, name: &str) -> Option<&'a TokenEntry> {
        self.by_name.get(name).copied()
    }

    /// `HEX_LOOKUP`: the hex colour of a token, aliases followed.
    pub(super) fn hex(&self, name: &str, depth: usize) -> Option<String> {
        let t = self.get(name).filter(|_| depth <= 8)?;
        if let Some(inner) = alias(&t.value) {
            return self.hex(inner, depth + 1);
        }
        hex_field(&t.value).or_else(|| {
            t.value
                .as_str()
                .filter(|s| s.starts_with('#'))
                .map(str::to_string)
        })
    }

    pub(super) fn role(&self, r: &str) -> Option<String> {
        self.hex(&format!("color.{r}"), 0)
    }

    /// `hexOf`: the hex colour of a token value.
    pub(super) fn hex_of(&self, v: &Value) -> Option<String> {
        match v {
            Value::String(s) if s.starts_with('#') => Some(s.clone()),
            Value::String(_) => alias(v).and_then(|inner| self.hex(inner, 0)),
            other => hex_field(other),
        }
    }

    /// The palette step a token points at (`{color.palette.blue-500}` is `blue-500`).
    pub(super) fn alias_of(&self, name: &str) -> String {
        match self.get(name).and_then(|t| t.value.as_str()) {
            Some(v) if v.starts_with('{') => {
                let inner = slice_inner(v);
                inner
                    .strip_prefix("color.palette.")
                    .unwrap_or(inner)
                    .to_string()
            }
            _ => String::new(),
        }
    }

    pub(super) fn description(&self, name: &str) -> String {
        js::or_empty(self.get(name).and_then(|t| t.description.as_ref()))
    }

    /// `byName.get('font.size.<k>')?.value?.value`.
    pub(super) fn size_of(&self, k: &str) -> Option<&'a Value> {
        self.get(&format!("font.size.{k}"))
            .and_then(|t| t.value.get("value"))
    }
}
