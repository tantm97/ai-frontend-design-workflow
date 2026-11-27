//! The words of the command line, named once: the commands of the table, the words that follow
//! a command, the flags as a user types them and the names of the hooks. It uses nothing else,
//! so the binary tests include it too (`#[path = "../src/core/word.rs"] mod word;`) and build
//! their command lines from it.

/// The commands of the table (`cli::commands`), by group.
pub mod command {
    pub const VERSION: &str = "version";
    pub const HELP: &str = "help";

    // workflow
    pub const INIT: &str = "init";
    pub const STATUS: &str = "status";
    pub const MODE: &str = "mode";
    pub const PHASE: &str = "phase";
    pub const APPROVE: &str = "approve";
    pub const REOPEN: &str = "reopen";
    pub const PAUSE: &str = "pause";
    pub const RESUME: &str = "resume";
    pub const DECIDE: &str = "decide";
    pub const DECISIONS: &str = "decisions";
    pub const CONFIG: &str = "config";
    pub const LANG: &str = "lang";
    pub const VERIFY_SNAPSHOT: &str = "verify-snapshot";

    // hook, comments
    pub const HOOK: &str = "hook";
    pub const COMMENTS: &str = "comments";

    // design
    pub const BUILD: &str = "build";
    pub const TOKENS: &str = "tokens";
    pub const LINT: &str = "lint";
    pub const CHECK: &str = "check";
    pub const SYSTEM: &str = "system";
    pub const STYLE: &str = "style";
    pub const TYPE: &str = "type";
    pub const PALETTE: &str = "palette";
    pub const ICONS: &str = "icons";
    pub const FONTS: &str = "fonts";
    pub const BLUEPRINT: &str = "blueprint";
    pub const SCREEN: &str = "screen";

    // studio
    pub const STUDIO: &str = "studio";
    pub const REVIEW: &str = "review";
    /// Internal: `dfw studio start` spawns it.
    pub const STUDIO_SERVER: &str = "__studio-server";
    /// Internal: `dfw review start` spawns it.
    pub const REVIEW_SERVER: &str = "__review-server";

    // browser
    pub const SHOOT: &str = "shoot";
    pub const LAYOUT: &str = "layout";
    pub const DIFF: &str = "diff";

    // penpot
    pub const PENPOT: &str = "penpot";

    // tools
    pub const INSPECTOR: &str = "inspector";
    pub const MCP: &str = "mcp";
    pub const INSTALL: &str = "install";
    pub const UNINSTALL: &str = "uninstall";
    pub const SETUP: &str = "setup";
    pub const DOCTOR: &str = "doctor";
    pub const STACK: &str = "stack";
    pub const HANDOFF: &str = "handoff";

    // telemetry, debug
    pub const TELEMETRY: &str = "telemetry";
    /// Hidden: not in the help text (`cli/debug.rs`).
    pub const DEBUG: &str = "__debug";
}

/// The words that follow a command (`dfw config set`, `dfw telemetry off`). A word that is also
/// a command is that command's constant.
pub mod sub {
    pub const SET: &str = "set";
    pub const GET: &str = "get";
    pub const LIST: &str = "list";
    pub const ADD: &str = "add";
    pub const SHOW: &str = "show";
    pub const ON: &str = "on";
    pub const OFF: &str = "off";
    pub const FLUSH: &str = "flush";
    pub const ENDPOINT: &str = "endpoint";
    pub const EXPORT: &str = "export";
    pub const IMPORT: &str = "import";
    pub const PLUGIN: &str = "plugin";
    pub const EXTRACT: &str = "extract";
    pub const START: &str = "start";
    pub const INVENTORY: &str = "inventory";
    pub const STATUS: &str = super::command::STATUS;
    pub const INIT: &str = super::command::INIT;
    pub const CHECK: &str = super::command::CHECK;
    pub const INSTALL: &str = super::command::INSTALL;
    pub const UNINSTALL: &str = super::command::UNINSTALL;
}

/// The flags, as a user types them (the product reads them by key: `args.truthy("json")`).
#[cfg(test)]
pub mod flag {
    pub const HELP: &str = "--help";
    pub const VERSION: &str = "--version";
    pub const ROOT: &str = "--root";
    pub const JSON: &str = "--json";
    pub const NOTE: &str = "--note";
    pub const FORCE: &str = "--force";
    pub const HOST: &str = "--host";
    pub const GLOBAL: &str = "--global";
    pub const RAW: &str = "--raw";
    pub const SESSION: &str = "--session";
    pub const ACCEPT_UNVERIFIED: &str = "--accept-unverified";
    pub const DRY_RUN: &str = "--dry-run";
    pub const FORMAT: &str = "--format";
    pub const URL: &str = "--url";
    pub const OUT: &str = "--out";
    pub const SCREEN: &str = "--screen";
    pub const PRINT: &str = "--print";
    pub const PRODUCT: &str = "--product";
    pub const CANVAS: &str = "--canvas";
}

/// The hooks `dfw hook <name>` runs, one per host event (`cli::hook`).
pub mod hook_name {
    pub const SESSION_START: &str = "session-start";
    pub const PRE_TOOL_USE: &str = "pre-tool-use";
    pub const POST_TOOL_USE: &str = "post-tool-use";
    pub const USER_PROMPT_SUBMIT: &str = "user-prompt-submit";
    pub const STOP: &str = "stop";
}

#[cfg(test)]
mod tests {
    use super::flag::*;

    /// Every flag is typed with two dashes and named once.
    #[test]
    fn flags_are_typed_with_two_dashes_and_named_once() {
        let mut all = [
            HELP,
            VERSION,
            ROOT,
            JSON,
            NOTE,
            FORCE,
            HOST,
            GLOBAL,
            RAW,
            SESSION,
            ACCEPT_UNVERIFIED,
            DRY_RUN,
            FORMAT,
            URL,
            OUT,
            SCREEN,
            PRINT,
            PRODUCT,
            CANVAS,
        ];
        assert!(all.iter().all(|f| f.starts_with("--") && f.len() > 3));
        all.sort_unstable();
        let count = all.len();
        let mut unique = all.to_vec();
        unique.dedup();
        assert_eq!(unique.len(), count);
    }
}
