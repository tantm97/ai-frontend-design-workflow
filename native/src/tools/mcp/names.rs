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

/// The keys of a JSON-RPC message and of the MCP records in it.
pub mod key {
    /// The answer of a JSON-RPC request that worked.
    pub const RESULT: &str = "result";
    /// The name of the server, a tool or a prompt.
    pub const NAME: &str = "name";
    /// The protocol version of a message (`2.0`).
    pub const JSONRPC: &str = "jsonrpc";
    /// The id of a request, repeated in its reply; a notification has none.
    pub const ID: &str = "id";
    pub const METHOD: &str = "method";
    pub const PARAMS: &str = "params";
    /// The answer of a request that failed: `{ code, message }`.
    pub const ERROR: &str = "error";
    pub const CODE: &str = "code";
    pub const MESSAGE: &str = "message";
    /// `initialize`: the MCP version the client asks for and the server answers with.
    pub const PROTOCOL_VERSION: &str = "protocolVersion";
    /// `initialize`: what the server offers (`tools`, `prompts`).
    pub const CAPABILITIES: &str = "capabilities";
    /// `initialize`: `{ name, version }` of the server.
    pub const SERVER_INFO: &str = "serverInfo";
    pub const VERSION: &str = "version";
    /// `initialize`: how a client should use the server.
    pub const INSTRUCTIONS: &str = "instructions";
    /// The tools of `tools/list` (and of [`CAPABILITIES`]).
    pub const TOOLS: &str = "tools";
    /// The prompts of `prompts/list` (and of [`CAPABILITIES`]).
    pub const PROMPTS: &str = "prompts";
    /// What a tool or a prompt is for.
    pub const DESCRIPTION: &str = "description";
    /// The JSON schema of a tool's arguments.
    pub const INPUT_SCHEMA: &str = "inputSchema";
    /// `tools/call`: the arguments, by name ([`super::arg`]).
    pub const ARGUMENTS: &str = "arguments";
    /// A tool's answer: a list of `{ type, text }`; also a prompt message's.
    pub const CONTENT: &str = "content";
    /// The kind of a content item (`text`); in a schema, the JSON type.
    pub const TYPE: &str = "type";
    pub const TEXT: &str = "text";
    /// A tool's answer that reports a failure.
    pub const IS_ERROR: &str = "isError";
    /// `prompts/get`: the prompt's messages, `{ role, content }`.
    pub const MESSAGES: &str = "messages";
    pub const ROLE: &str = "role";
    /// In a schema: the keys of an object.
    pub const PROPERTIES: &str = "properties";
    /// In a schema: the schema of each item of a list.
    pub const ITEMS: &str = "items";
    /// In a schema: the keys an object must have.
    pub const REQUIRED: &str = "required";
}

/// The arguments of the tools, by name.
pub mod arg {
    /// `dfw_guide`: the reference to read, none for the main guide.
    pub const TOPIC: &str = "topic";
    /// `dfw`: the command line, as a list.
    pub const ARGS: &str = "args";
    /// `dfw_can_write`: the files to check.
    pub const FILES: &str = "files";
    /// The project root (absolute path).
    pub const CWD: &str = "cwd";
}
