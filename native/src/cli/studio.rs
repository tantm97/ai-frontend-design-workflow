//! Server commands of `core/lib/cli.mjs`: studio, review (`cmdStudio`, `cmdReview`) and the internal
//! `__studio-server` and `__review-server` processes they spawn.

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("studio", Handler::Forward),
        ("review", Handler::Forward),
        ("__studio-server", Handler::Forward),
        ("__review-server", Handler::Forward),
    ]
}
