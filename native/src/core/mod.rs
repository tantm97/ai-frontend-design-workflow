//! Shared foundations: errors, JSON io, paths, globs, argument parsing, configuration, hosts.

// Most helpers have no caller until the module groups are ported: this allow goes with stage 1.
#![allow(dead_code)]

pub mod config;
pub mod error;
pub mod host;
pub mod js;
pub mod paths;
pub mod util;
