//! What the telemetry sends and keeps, named once: the keys of the OTLP/HTTP JSON encoding,
//! dfw's event and attribute names, and the files of the telemetry folder. It uses nothing
//! else, so the binary tests include it too (`#[path = "../src/telemetry/wire.rs"] mod
//! telemetry_wire;`) and read the requests with it.

/// The keys of the OTLP/HTTP JSON encoding of logs.
pub mod otlp {
    pub const RESOURCE_LOGS: &str = "resourceLogs";
    pub const RESOURCE: &str = "resource";
    pub const SCOPE_LOGS: &str = "scopeLogs";
    pub const SCOPE: &str = "scope";
    pub const NAME: &str = "name";
    pub const LOG_RECORDS: &str = "logRecords";
    pub const ATTRIBUTES: &str = "attributes";
    pub const KEY: &str = "key";
    pub const VALUE: &str = "value";
    /// A text value.
    pub const STRING_VALUE: &str = "stringValue";
    /// A 64-bit integer, written as a string.
    pub const INT_VALUE: &str = "intValue";
    pub const TIME_UNIX_NANO: &str = "timeUnixNano";
    pub const SEVERITY_NUMBER: &str = "severityNumber";
    pub const SEVERITY_TEXT: &str = "severityText";
    pub const EVENT_NAME: &str = "eventName";
    pub const BODY: &str = "body";
}

/// The names of dfw's events (`eventName`).
pub mod event {
    /// A command ran; its first attribute is [`super::attr::COMMAND`].
    pub const COMMAND: &str = "dfw.command";
    /// A hook ran; its first attribute is [`super::attr::HOOK`].
    pub const HOOK: &str = "dfw.hook";
    pub const GATE_APPROVED: &str = "dfw.gate_approved";
    pub const CHECKS: &str = "dfw.checks";
    pub const WORKFLOW: &str = "dfw.workflow";
}

/// The names of the attributes of the resource and of the events.
pub mod attr {
    pub const SERVICE_NAME: &str = "service.name";
    pub const SERVICE_VERSION: &str = "service.version";
    pub const OS_TYPE: &str = "os.type";
    pub const HOST_ARCH: &str = "host.arch";
    pub const INSTALL_ID: &str = "dfw.install_id";
    /// The command's name: the event's own name.
    pub const COMMAND: &str = super::event::COMMAND;
    pub const DURATION_BUCKET: &str = "dfw.duration_bucket";
    pub const EXIT_CODE: &str = "dfw.exit_code";
    /// The hook's name: the event's own name.
    pub const HOOK: &str = super::event::HOOK;
    pub const HOST: &str = "dfw.host";
    pub const HOOK_OUTCOME: &str = "dfw.hook_outcome";
    pub const GATE: &str = "dfw.gate";
    pub const MODE: &str = "dfw.mode";
    pub const PHASE: &str = "dfw.phase";
}

/// The files of the telemetry folder.
pub mod file {
    /// The events waiting to be sent, one JSON line each.
    pub const SPOOL: &str = "spool.jsonl";
    /// The install id.
    pub const ID: &str = "id";
    /// Marks that the one-time notice was printed.
    pub const NOTICE: &str = "notice";
}
