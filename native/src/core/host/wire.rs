//! What a host sends to a hook, named once: the hook events, the names of the tools that write
//! files and the body of an apply_patch call; and what a hook answers it ([`reply`]). It uses nothing else, so the binary tests include it too
//! (`#[path = "../src/core/host/wire.rs"] mod wire;`) and build their hook payloads from it.

/// Hook events, by the names Claude Code gives them (`hook_event_name`); every host's output is
/// formatted for one of them.
pub mod event {
    pub const SESSION_START: &str = "SessionStart";
    pub const PRE_TOOL_USE: &str = "PreToolUse";
    pub const POST_TOOL_USE: &str = "PostToolUse";
    pub const USER_PROMPT_SUBMIT: &str = "UserPromptSubmit";
    pub const STOP: &str = "Stop";
}

/// Tool names. The constants at the top are dfw's canonical names, the ones `core::host` turns
/// a host's own name into and the write gate reads; [`alias`] holds the hosts' own names.
pub mod tool {
    pub const WRITE: &str = "Write";
    pub const EDIT: &str = "Edit";
    pub const MULTI_EDIT: &str = "MultiEdit";
    pub const NOTEBOOK_EDIT: &str = "NotebookEdit";
    /// A patch of several files; its body is read with [`super::patch`].
    pub const APPLY_PATCH: &str = "ApplyPatch";
    /// Every canonical tool that writes files.
    pub const WRITES: [&str; 5] = [EDIT, WRITE, MULTI_EDIT, NOTEBOOK_EDIT, APPLY_PATCH];

    /// A host's own name for a tool that writes files, as the host sends it (GitHub Copilot's
    /// camelCase payloads).
    pub mod alias {
        pub const EDIT: &str = "edit";
        pub const STR_REPLACE_EDITOR: &str = "str_replace_editor";
        pub const CREATE: &str = "create";
        pub const WRITE: &str = "write";
        pub const APPLY_PATCH: &str = "apply_patch";
    }
}

/// The body of an apply_patch call: a line `*** Begin Patch`, one header line per file
/// (`*** Update File: a/b.ts`, `*** Move to: c.ts`) with the changes under it, `*** End Patch`.
pub mod patch {
    /// The first line of a body.
    #[cfg(test)]
    pub const BEGIN: &str = "*** Begin Patch";
    /// The last line of a body.
    #[cfg(test)]
    pub const END: &str = "*** End Patch";
    /// The headers that name a file, each followed by its path.
    pub const ADD: &str = "*** Add File: ";
    pub const UPDATE: &str = "*** Update File: ";
    pub const DELETE: &str = "*** Delete File: ";
    pub const MOVE_TO: &str = "*** Move to: ";
    /// Every header that names a file the patch writes.
    pub const HEADERS: [&str; 4] = [ADD, UPDATE, DELETE, MOVE_TO];

    /// Header lines, one per `(header, path)`, joined by `\n`.
    #[cfg(test)]
    pub fn lines(lines: &[(&str, &str)]) -> String {
        let lines: Vec<String> = lines.iter().map(|(h, p)| format!("{h}{p}")).collect();
        lines.join("\n")
    }

    /// A whole body: [`BEGIN`], the header [`lines`], [`END`].
    #[cfg(test)]
    pub fn body(headers: &[(&str, &str)]) -> String {
        format!("{BEGIN}\n{}\n{END}", lines(headers))
    }
}

/// What a hook prints for the host: its keys and the values that refuse or hold back.
pub mod reply {
    /// Claude Code's wrapper around a hook's answer.
    pub const HOOK_SPECIFIC_OUTPUT: &str = "hookSpecificOutput";
    /// The event the wrapped answer is for.
    pub const HOOK_EVENT_NAME: &str = "hookEventName";
    /// Text added to the model's context.
    pub const ADDITIONAL_CONTEXT: &str = "additionalContext";
    /// Whether a tool call may run; [`DENY`] refuses it.
    pub const PERMISSION_DECISION: &str = "permissionDecision";
    /// Why a tool call is refused.
    pub const PERMISSION_DECISION_REASON: &str = "permissionDecisionReason";
    /// The value of [`PERMISSION_DECISION`] that refuses a tool call.
    pub const DENY: &str = "deny";
    /// Whether the agent may stop; [`BLOCK`] holds it back.
    pub const DECISION: &str = "decision";
    /// Why the agent may not stop yet.
    pub const REASON: &str = "reason";
    /// The value of [`DECISION`] that keeps the agent from stopping.
    pub const BLOCK: &str = "block";
}

/// The keys of a hook payload: each field by the name Claude Code (and Copilot's PascalCase
/// format) gives it, then by the name of Copilot's camelCase format. `core::host` reads either
/// and writes the first.
pub mod input {
    /// The hook event ([`super::event`]).
    pub const HOOK_EVENT_NAME: &str = "hook_event_name";
    pub const HOOK_EVENT_NAME_CAMEL: &str = "hookEventName";
    pub const SESSION_ID: &str = "session_id";
    pub const SESSION_ID_CAMEL: &str = "sessionId";
    /// The project folder the host runs in.
    pub const CWD: &str = "cwd";
    pub const TOOL_NAME: &str = "tool_name";
    pub const TOOL_NAME_CAMEL: &str = "toolName";
    /// The arguments of the tool call (an object, or JSON text in the camelCase format).
    pub const TOOL_INPUT: &str = "tool_input";
    pub const TOOL_ARGS: &str = "toolArgs";
    /// Whether the agent is already stopping because a Stop hook held it back once.
    pub const STOP_HOOK_ACTIVE: &str = "stop_hook_active";
    pub const STOP_HOOK_ACTIVE_CAMEL: &str = "stopHookActive";
    /// The text the user submitted (UserPromptSubmit).
    pub const PROMPT: &str = "prompt";
    /// The event of the normalized payload (dfw's own key).
    pub const EVENT: &str = "event";
    /// Tool arguments that were text but not JSON (a raw apply_patch body), as dfw wraps them.
    pub const RAW_INPUT: &str = "input";
}

/// The keys of the neutral result of a hook handler: `{ context }`, `{ deny }` or `{ block }`.
pub mod result {
    /// Text added to the model's context.
    pub const CONTEXT: &str = "context";
    /// The reason a tool call is refused.
    pub const DENY: &str = "deny";
    /// The reason the agent may not stop yet.
    pub const BLOCK: &str = "block";
}
