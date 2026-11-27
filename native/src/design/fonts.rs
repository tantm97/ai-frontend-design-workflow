//! Ports `core/lib/fonts.mjs`: subsets a Google Font ships.
//!
//! Asks the Google Fonts CSS API (free, no key) which subsets a family has (for example
//! "vietnamese"). Only the family name is sent, and only when the user runs `dfw fonts check`.

use std::time::Duration;

use regex::Regex;
use serde_json::{json, Value};

use crate::core::error::DfwError;
use crate::core::http::{self, Failed, Source, Verify};

const USER_AGENT: &str =
    "Mozilla/5.0 (Macintosh) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130 Safari/537.36";

/// The keys of [`FontSubsets::to_json`].
pub mod key {
    pub const FAMILY: &str = "family";
    /// The service answered with the family's CSS.
    pub const OK: &str = "ok";
    /// The HTTP status of an error answer.
    pub const STATUS: &str = "status";
    /// Why the request failed.
    pub const ERROR: &str = "error";
    pub const SUBSETS: &str = "subsets";
}

/// The answer for one family.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FontSubsets {
    pub family: String,
    pub ok: bool,
    /// HTTP status when the service answered with an error.
    pub status: Option<u16>,
    /// Why the request itself failed (no network, ...).
    pub error: Option<String>,
    pub subsets: Vec<String>,
}

impl FontSubsets {
    /// The object the Node CLI printed with `--json`.
    pub fn to_json(&self) -> Value {
        if self.ok {
            json!({ (key::FAMILY): self.family, (key::OK): true, (key::SUBSETS): self.subsets })
        } else if let Some(error) = &self.error {
            json!({ (key::FAMILY): self.family, (key::OK): false, (key::ERROR): error, (key::SUBSETS): [] })
        } else {
            json!({ (key::FAMILY): self.family, (key::OK): false, (key::STATUS): self.status, (key::SUBSETS): [] })
        }
    }
}

/// `encodeURIComponent(family).replace(/%20/g, '+')`.
fn encode_family(family: &str) -> String {
    let mut out = String::new();
    for b in family.bytes() {
        match b {
            b' ' => out.push('+'),
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(char::from(b)),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The subset names in the comments of a Google Fonts stylesheet (`/* vietnamese */`), each once,
/// in order.
fn subsets_in(css: &str) -> Result<Vec<String>, DfwError> {
    let comment = Regex::new(r"/\*\s*([A-Za-z0-9_-]+)\s*\*/")
        .map_err(|e| DfwError::internal(e.to_string()))?;
    let mut subsets: Vec<String> = Vec::new();
    for m in comment.captures_iter(css) {
        let name = m[1].to_string();
        if !subsets.contains(&name) {
            subsets.push(name);
        }
    }
    Ok(subsets)
}

/// How long the Google Fonts lookup may take, and how large its stylesheet may be (a real one
/// is a few kilobytes).
const LOOKUP_DEADLINE: Duration = Duration::from_secs(15);
const CSS_LIMIT: u64 = 1024 * 1024;
/// The Google Fonts CSS API: a stylesheet dfw only reads for its subset names.
const GOOGLE_FONTS: Source = Source {
    host: "fonts.googleapis.com",
    redirects: &[],
    verify: Verify::Read,
};

/// `fontSubsets`: which subsets Google Fonts ships for `family`. A failed request is not an
/// error here: it is reported in the result (`ok` false), as the Node CLI printed it.
pub fn font_subsets(family: &str) -> Result<FontSubsets, DfwError> {
    let url = format!(
        "https://fonts.googleapis.com/css2?family={}&display=swap",
        encode_family(family)
    );
    let failed = |status: Option<u16>, error: Option<String>| FontSubsets {
        family: family.to_string(),
        ok: false,
        status,
        error,
        subsets: Vec::new(),
    };
    let css = match http::download(
        &GOOGLE_FONTS,
        &url,
        None,
        &[("user-agent", USER_AGENT)],
        LOOKUP_DEADLINE,
        CSS_LIMIT,
    ) {
        Ok(res) => String::from_utf8_lossy(&res.body).into_owned(),
        Err(Failed::Status(status)) => return Ok(failed(Some(status), None)),
        Err(err) => return Ok(failed(None, Some(err.to_string()))),
    };
    Ok(FontSubsets {
        family: family.to_string(),
        ok: true,
        status: None,
        error: None,
        subsets: subsets_in(&css)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn family_names_are_encoded_like_encode_uri_component() {
        assert_eq!(encode_family("Noto Serif Display"), "Noto+Serif+Display");
        assert_eq!(encode_family("A&B/é"), "A%26B%2F%C3%A9");
    }

    #[test]
    fn subsets_come_from_the_stylesheet_comments() {
        let css = "/* vietnamese */\n@font-face {}\n/* latin-ext */\n@font-face {}\n/* latin */\n/* latin */\n/* two words */";
        assert_eq!(
            subsets_in(css).unwrap(),
            ["vietnamese", "latin-ext", "latin"]
        );
        let failed = FontSubsets {
            family: "X".into(),
            ok: false,
            status: Some(400),
            error: None,
            subsets: vec![],
        };
        assert_eq!(
            failed.to_json().to_string(),
            r#"{"family":"X","ok":false,"status":400,"subsets":[]}"#
        );
    }
}
