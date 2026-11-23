//! Shared foundations: errors, JSON io, paths, globs, argument parsing, configuration, hosts.

pub mod config;
pub mod encoding;
pub mod error;
#[cfg(test)]
pub mod fixture;
pub mod host;
pub mod http;
pub mod js;
pub mod level;
pub mod lock;
pub mod paths;
pub mod platform;
pub mod process;
pub mod util;
