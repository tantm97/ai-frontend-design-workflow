//! Design commands of `core/lib/cli.mjs`: build, tokens, lint, check, system, style, type, palette, icons,
//! fonts, blueprint.

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("build", Handler::Forward),
        ("tokens", Handler::Forward),
        ("lint", Handler::Forward),
        ("check", Handler::Forward),
        ("system", Handler::Forward),
        ("style", Handler::Forward),
        ("type", Handler::Forward),
        ("palette", Handler::Forward),
        ("icons", Handler::Forward),
        ("fonts", Handler::Forward),
        ("blueprint", Handler::Forward),
    ]
}
