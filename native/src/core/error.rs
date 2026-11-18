//! Ports the `DfwError` class of `core/lib/util.mjs` and the error handling of `core/bin/dfw.mjs`.
//!
//! The Node entry point prints `err.message` and exits with `err.exitCode` for a `DfwError`, and
//! prints the stack and exits with 1 for anything else. Here the two cases are the two variants.

use std::fmt;

/// The one error type of the crate.
#[derive(Debug)]
pub enum DfwError {
    /// A mistake the user can fix; the message says what to do next.
    User { message: String, code: i32 },
    /// Unexpected failure (io, broken JSON, a bug); the Node CLI printed a stack trace for these.
    Internal(String),
}

impl DfwError {
    /// User-facing error with exit code 1 (`new DfwError(message)`).
    pub fn new(message: impl Into<String>) -> Self {
        DfwError::User { message: message.into(), code: 1 }
    }

    /// User-facing error with a specific exit code (`new DfwError(message, code)`).
    pub fn with_code(message: impl Into<String>, code: i32) -> Self {
        DfwError::User { message: message.into(), code }
    }

    /// Unexpected error (a plain `Error` in the Node code).
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

    /// Process exit code: the error's own code, or 1 (`err.exitCode || 1`).
    pub fn exit_code(&self) -> i32 {
        match self {
            // A zero code is falsy in the Node entry point, so it also ends as 1.
            DfwError::User { code, .. } if *code != 0 => *code,
            _ => 1,
        }
    }

    /// The text for stderr. Node printed `err.stack` for internal errors: its first line is
    /// `Error: <message>`; the stack frames have no equivalent here.
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
        assert_eq!(DfwError::with_code("findings", 2).exit_code(), 2);
    }

    #[test]
    fn internal_error_exits_1_with_an_error_prefix() {
        let e = DfwError::internal("boom");
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.report(), "Error: boom");
    }

    #[test]
    fn zero_code_behaves_like_the_node_entry_point() {
        let e = DfwError::with_code("odd", 0);
        assert_eq!(e.exit_code(), 1);
        assert_eq!(e.report(), "Error: odd");
    }
}
