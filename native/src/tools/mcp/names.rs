//! The names the dfw MCP server answers with: its own, its tools' and the keys a client reads
//! them by. It uses nothing else, so the binary tests include it too
//! (`#[path = "../src/tools/mcp/names.rs"] mod mcp_names;`).

/// The server's name, its prompt's, and the name dfw is registered under in each tool's MCP
/// config.
pub const SERVER: &str = "dfw";

/// The tools of the server.
pub mod tool {
    /// The workflow instructions.
    pub const GUIDE: &str = "dfw_guide";
    /// A dfw CLI command.
    pub const CLI: &str = "dfw";
    /// The approval gates' answer for a list of files.
    pub const CAN_WRITE: &str = "dfw_can_write";
}

/// The keys of a reply.
pub mod key {
    /// The answer of a JSON-RPC request that worked.
    pub const RESULT: &str = "result";
    /// The name of the server, a tool or a prompt.
    pub const NAME: &str = "name";
}
