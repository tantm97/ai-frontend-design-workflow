//! The `hook` command of `core/lib/cli.mjs`: reads the host's JSON on stdin, runs a handler of `hooks/`,
//! prints the host-specific result; fails open.

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("hook", Handler::Forward),
    ]
}
