//! The records of the running servers: `.dfw/studio.json` (`dfw studio start`) and
//! `.dfw/review.json` (`dfw review start`), both written by `cli::studio`. Their keys live here,
//! below every layer that reads them (`browser::shoot`, `cli`).

/// The keys of a server record: `{ pid, port, url, target?, shortcut?, startedAt }`.
pub mod field {
    /// The server's process.
    pub const PID: &str = "pid";
    pub const PORT: &str = "port";
    /// The address the server answers on; the review server's live route answers with the
    /// same key.
    pub const URL: &str = "url";
    /// The app the review proxy serves (`review.json` only).
    pub const TARGET: &str = "target";
    /// The comment-mode key combination (`review.json` only).
    pub const SHORTCUT: &str = "shortcut";
    pub const STARTED_AT: &str = "startedAt";
}
