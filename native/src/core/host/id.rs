//! The ids of the hosts, as `--host` and `DFW_HOST` name them. It uses nothing else, so the
//! binary tests include it too (`#[path = "../src/core/host/id.rs"] mod host_id;`).

pub const CLAUDE: &str = "claude";
pub const COPILOT: &str = "copilot";
pub const MCP: &str = "mcp";
pub const OPENCODE: &str = "opencode";
pub const CODEX: &str = "codex";
pub const CURSOR: &str = "cursor";
