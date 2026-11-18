//! The `comments` command of `core/lib/cli.mjs` (`cmdComments`, `cmdCommentsWait`).

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![("comments", Handler::Forward)]
}
