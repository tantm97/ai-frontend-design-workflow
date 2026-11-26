//! Ports `core/lib/hooks.mjs`: the five hook handlers, one file each. Every handler fails open.

pub mod post_tool_use;
pub mod pre_tool_use;
pub mod session_start;
pub mod stop;
pub mod user_prompt_submit;
