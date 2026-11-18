//! The `penpot` command of `core/lib/cli.mjs` (`cmdPenpot`).

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![("penpot", Handler::Forward)]
}
