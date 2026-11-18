//! Workflow commands of `core/lib/cli.mjs`: init, status, mode, phase, approve, reopen, pause, resume,
//! decide, decisions, config, lang, verify-snapshot (`cmdInit`, `cmdStatus`, `nextStep`, `cmdApprove`).

use super::Handler;

/// The commands of this group; `Forward` until ported.
pub fn commands() -> Vec<(&'static str, Handler)> {
    vec![
        ("init", Handler::Forward),
        ("status", Handler::Forward),
        ("mode", Handler::Forward),
        ("phase", Handler::Forward),
        ("approve", Handler::Forward),
        ("reopen", Handler::Forward),
        ("pause", Handler::Forward),
        ("resume", Handler::Forward),
        ("decide", Handler::Forward),
        ("decisions", Handler::Forward),
        ("config", Handler::Forward),
        ("lang", Handler::Forward),
        ("verify-snapshot", Handler::Forward),
    ]
}
