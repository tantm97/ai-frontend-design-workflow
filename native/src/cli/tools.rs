//! Tooling commands of `core/lib/cli.mjs`: inspector, mcp, install, uninstall, setup, doctor, stack, handoff
//! (`cmdInspector`, `cmdDoctor`, `cmdHandoff`). Lighthouse has no command of its own: it runs inside `check`.

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("inspector", Handler::Forward),
        ("mcp", Handler::Forward),
        ("install", Handler::Forward),
        ("uninstall", Handler::Forward),
        ("setup", Handler::Forward),
        ("doctor", Handler::Forward),
        ("stack", Handler::Forward),
        ("handoff", Handler::Forward),
    ]
}
