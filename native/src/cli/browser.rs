//! Browser commands of `core/lib/cli.mjs`: shoot, layout, diff (`cmdShoot`). The a11y check and the token
//! extraction have no command of their own: they run inside `check` and `tokens extract`.

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("shoot", Handler::Forward),
        ("layout", Handler::Forward),
        ("diff", Handler::Forward),
    ]
}
