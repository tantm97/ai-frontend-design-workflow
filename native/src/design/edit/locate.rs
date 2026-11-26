//! Finding an element of a design file by its id: `locate`, `masterOf` and `locateOwn` of
//! `edit.mjs`, on the tokenizer instead of the tag scanner.

use std::path::Path;

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::js;
use crate::design::compose::{attr, attrs_of, load_components, Attrs, ComponentDef};
use crate::design::html::{element_at, Tag, Token, Tokenizer};

/// An element found by [`locate`], with the byte offsets an edit splices at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Located {
    /// `<` of the opening tag.
    pub start: usize,
    /// After `>` of the opening tag.
    pub open_end: usize,
    /// Start of the content (the same as `open_end`).
    pub inner_start: usize,
    /// End of the content: `<` of the closing tag.
    pub inner_end: usize,
    /// After `>` of the closing tag (`open_end` for a void or self-closing element).
    pub end: usize,
    /// The tag name in lower case.
    pub tag: String,
    /// The decoded attributes of the opening tag.
    pub attrs: Attrs,
    /// The element is a `<dfw-use>`.
    pub is_instance: bool,
    /// The opening tag, for attribute splices.
    pub open: Tag,
}

impl Located {
    /// True for a void or self-closing element: it has no content and no closing tag.
    pub fn is_empty_tag(&self) -> bool {
        self.end == self.open_end
    }
}

/// The opening tags the tag scanner of edit.mjs reported: every tag outside comments, and not
/// the `<script>` and `<style>` elements themselves (the scanner skipped them whole).
fn scanned_tags(text: &str) -> Vec<Tag> {
    let tokens: Vec<Token> = Tokenizer::new(text).collect();
    let mut tags = Vec::new();
    for (i, token) in tokens.iter().enumerate() {
        let Token::Open(tag) = token else { continue };
        if (tag.is(text, "script") || tag.is(text, "style")) && !tag.self_closing {
            let closed = tokens[i + 1..]
                .iter()
                .take(2)
                .any(|t| matches!(t, Token::Close(c) if c.is(text, tag.name(text))));
            if closed {
                continue;
            }
        }
        tags.push(tag.clone());
    }
    tags
}

fn has_id(text: &str, tag: &Tag, attrs: &Attrs, id: &str) -> bool {
    attrs.get(attr::DFW_ID) == Some(id)
        || ((tag.is(text, "dfw-use") || tag.is(text, "dfw-icon"))
            && attrs.get(attr::ID) == Some(id))
}

/// `locate`: the one element with `data-dfw-id="id"` (or `<dfw-use|dfw-icon id="id">`).
pub fn locate(text: &str, id: &str) -> Result<Located, DfwError> {
    if id.is_empty() {
        return Err(DfwError::new(
            "This element has no data-dfw-id, so it cannot be located in the source. Comment instead.",
        ));
    }
    let mut hits: Vec<(Tag, Attrs)> = Vec::new();
    for tag in scanned_tags(text) {
        let attrs = attrs_of(text, &tag.attrs);
        if has_id(text, &tag, &attrs, id) {
            hits.push((tag, attrs));
        }
    }
    if hits.len() > 1 {
        return Err(DfwError::new(format!(
            "\"{id}\" appears {} times in the source file; ids must be unique per file.",
            hits.len()
        )));
    }
    let Some((open, attrs)) = hits.pop() else {
        return Err(not_found(id));
    };
    let tag = open.name(text).to_ascii_lowercase();
    let el = element_at(text, open.range.start).filter(|el| !el.unclosed);
    let Some(el) = el else {
        return Err(DfwError::new(format!(
            "Could not find the end of <{tag}> for \"{id}\"."
        )));
    };
    Ok(Located {
        start: el.range.start,
        open_end: el.open.range.end,
        inner_start: el.inner.start,
        inner_end: el.inner.end,
        end: el.range.end,
        is_instance: tag == "dfw-use",
        tag,
        attrs,
        open,
    })
}

fn not_found(id: &str) -> DfwError {
    DfwError::new(format!(
        "\"{id}\" was not found in the source file (it may come from a component master or shared include)."
    ))
}

/// [`locate`] for an id as it came in a request: a missing or empty id cannot be located, and
/// an id that is not a string matches nothing.
pub fn locate_value(text: &str, id: Option<&Value>) -> Result<Located, DfwError> {
    match id {
        Some(Value::String(id)) => locate(text, id),
        other if js::truthy(other) => Err(not_found(&js::to_string(other))),
        _ => locate(text, ""),
    }
}

/// `masterOf`: the component master whose template declares `data-dfw-id="id"` (an element the
/// screen source does not own).
pub fn master_of(root: &Path, id: &str) -> Result<Option<ComponentDef>, DfwError> {
    let lib = load_components(root)?;
    let found = lib.iter().find(|def| {
        scanned_tags(&def.template)
            .iter()
            .any(|tag| attrs_of(&def.template, &tag.attrs).get(attr::DFW_ID) == Some(id))
    });
    Ok(found.cloned())
}

/// `locateOwn`: [`locate_value`] that explains ids built by a component template (app-shell
/// slots, layout masters).
pub fn locate_own(root: &Path, text: &str, id: Option<&Value>) -> Result<Located, DfwError> {
    let err = match locate_value(text, id) {
        Ok(el) => return Ok(el),
        Err(err) => err,
    };
    // The Node code tested the message, so an id that contains these words also gets the hint.
    let id_text = js::to_string(id);
    if !js::truthy(id) || !err.message().contains("was not found") {
        return Err(err);
    }
    match master_of(root, &id_text)? {
        Some(def) => Err(DfwError::new(format!(
            "\"{id_text}\" is part of the component master \"{}\" ({}), not of this screen, so the Studio cannot move or resize it here. Edit the master to change it, or leave a comment.",
            def.display_name(),
            def.file
        ))),
        None => Err(err),
    }
}
