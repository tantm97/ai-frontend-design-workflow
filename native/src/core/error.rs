//! `DfwError`, the one error type of the crate, and how `main` reports it (`errors.md` E1).
//!
//! A user error prints its message alone and exits with its code; an internal error prints
//! `Error: <message>` and exits with 1.

use std::fmt;

/// The exit codes of dfw (`contracts.md` C3). Agents and CI scripts branch on them.
pub mod exit {
    /// Done, nothing to report, or findings below error level.
    pub const OK: i32 = 0;
    /// The command could not do its job (a `DfwError`).
    pub const FAILED: i32 = 1;
    /// The command ran and found problems (lint, check or layout errors, a failed shot, a
    /// modified snapshot, missing fonts, Penpot export errors).
    pub const PROBLEMS: i32 = 2;
    /// `dfw check --strict`: a configured tool could not run, and no error was found.
    pub const UNVERIFIED: i32 = 3;
}

/// The one error type of the crate.
#[derive(Debug)]
pub enum DfwError {
    /// A mistake the user can fix; the message says what to do next.
    User { message: String, code: i32 },
    /// Unexpected failure (io, broken JSON, a bug), reported with an `Error: ` prefix.
    Internal(String),
}

impl DfwError {
    /// User-facing error with exit code 1.
    pub fn new(message: impl Into<String>) -> Self {
        DfwError::User {
            message: message.into(),
            code: exit::FAILED,
        }
    }

    /// User-facing error with a specific exit code. No command needs one yet; the tests pin
    /// how such an error exits.
    #[cfg(test)]
    pub fn with_code(message: impl Into<String>, code: i32) -> Self {
        DfwError::User {
            message: message.into(),
            code,
        }
    }

    /// Unexpected error: something the user cannot fix by following a message.
    pub fn internal(message: impl Into<String>) -> Self {
        DfwError::Internal(message.into())
    }

    /// The message without any prefix.
    pub fn message(&self) -> &str {
        match self {
            DfwError::User { message, .. } => message,
            DfwError::Internal(message) => message,
        }
    }

    /// Process exit code: the error's own code, or 1 for an internal error and for code 0.
    pub fn exit_code(&self) -> i32 {
        match self {
            // An error never exits 0 (`errors.md` E9).
            DfwError::User { code, .. } if *code != exit::OK => *code,
            _ => exit::FAILED,
        }
    }

    /// The text for stderr: the message of a user error, `Error: <message>` otherwise (a user
    /// error with code 0 is reported as internal, as it exits 1).
    pub fn report(&self) -> String {
        match self {
            DfwError::User { message, code } if *code != 0 => message.clone(),
            other => format!("Error: {}", other.message()),
        }
    }
}

impl fmt::Display for DfwError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for DfwError {}

impl From<std::io::Error> for DfwError {
    fn from(err: std::io::Error) -> Self {
        DfwError::Internal(err.to_string())
    }
}

impl From<serde_json::Error> for DfwError {
    fn from(err: serde_json::Error) -> Self {
        DfwError::Internal(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_error_prints_the_message_and_keeps_its_code() {
        let e = DfwError::new("Unknown command \"x\". Run \"dfw help\".");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.report(), "Unknown command \"x\". Run \"dfw help\".");
    }

    #[test]
    fn internal_error_exits_1_with_an_error_prefix() {
        let e = DfwError::internal("boom");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.report(), "Error: boom");
    }

    #[test]
    fn zero_code_exits_1_like_an_internal_error() {
        let e = DfwError::with_code("odd", 0);
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.report(), "Error: odd");
    }
}
