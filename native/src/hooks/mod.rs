//! Ports `core/lib/hooks.mjs`: the five hook handlers, one file each. Every handler fails open.

/// The keys of a hook payload the hooks read, as Claude Code names them (`core::host` turns
/// another host's payload into these).
pub(crate) mod payload {
    /// The id of the agent's session.
    pub const SESSION_ID: &str = "session_id";
    /// The session's working directory.
    pub const CWD: &str = "cwd";
    /// The list of replacements of a MultiEdit call, inside its tool input.
    pub const EDITS: &str = "edits";
    /// The text an Edit call (or one of [`EDITS`]) replaces.
    pub const OLD_STRING: &str = "old_string";
}

pub mod post_tool_use;
pub mod pre_tool_use;
pub mod session_start;
pub mod stop;
pub mod user_prompt_submit;
