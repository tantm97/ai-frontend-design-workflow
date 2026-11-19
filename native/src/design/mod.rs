//! Design files: tokenizer, compose, tokens, lint, edits, design system.

pub mod color;
pub mod compose;
pub mod dsaudit;
pub mod edit;
pub mod fonts;
pub mod html;
pub mod icons;
pub mod inventory;
pub mod layout_cache;
pub mod lint;
pub mod pages;
pub mod style;
pub mod system;
pub mod system_page;
pub mod tokens;

use crate::core::error::DfwError;

/// A design file whose value has the wrong shape (`what` names the field): a user error that
/// says which file and field are wrong and what to do (`errors.md` E2, E7). The command stops:
/// a file of the wrong shape never passes a gate in part.
pub fn malformed(file: &str, what: &str) -> DfwError {
    DfwError::new(format!(
        "Malformed {file}: {what}. Correct it in {file} and run the command again."
    ))
}
