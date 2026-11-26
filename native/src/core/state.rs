//! The shape of `.dfw/state.json` that more than the workflow module reads: its keys, and the
//! two gate facts every writer of design files asks (AGENTS.md rule 1). The state machine itself
//! is `workflow::state`, which re-exports these.

use serde_json::Value;

use crate::core::js;

/// The keys of `.dfw/state.json`, of its gate records (`gates.G1`, ...), of the entries of
/// `design.approved` and of the history entries. The other workflow files name their own keys:
/// `decisions::field`, `stack::field`, `freeze::field`, `server::field`.
pub mod field {
    /* ------------------------------------------------------------------------ the state */

    /// The format of the file (`1`).
    pub const SCHEMA: &str = "schema";
    /// The product name `dfw init --product` gave.
    pub const PRODUCT: &str = "product";
    /// The workflow mode (`workflow::names::mode`).
    pub const MODE: &str = "mode";
    pub const PHASE: &str = "phase";
    /// `false` while the workflow is paused (`dfw pause`): gates and hooks are off.
    pub const ACTIVE: &str = "active";
    pub const CREATED_AT: &str = "createdAt";
    /// Stamped by every save.
    pub const UPDATED_AT: &str = "updatedAt";
    /// `{ version, status, locked, approved }`.
    pub const DESIGN: &str = "design";
    /// The approval record of each gate, `null` while it is open.
    pub const GATES: &str = "gates";
    /// The gates the mode waives, each with why.
    pub const WAIVED: &str = "waived";
    pub const HISTORY: &str = "history";

    /* ---------------------------------------------------------------------- the design */

    /// `design.version`: the design version being worked on; in an entry of `design.approved`
    /// and in the G2 record, the snapshot name (`v1`).
    pub const VERSION: &str = "version";
    /// `design.status`: `draft` or `approved`.
    pub const STATUS: &str = "status";
    /// `design.locked`: the design is approved and frozen (G2).
    pub const LOCKED: &str = "locked";
    /// `design.approved`: the list of approved versions, `{ version, hash, at, note? }`.
    pub const APPROVED: &str = "approved";
    /// The hash of a frozen design (an entry of `design.approved`, the G2 record).
    pub const HASH: &str = "hash";

    /* ------------------------------------------------- gate records and history entries */

    /// When a gate was approved (a gate record).
    pub const APPROVED_AT: &str = "approvedAt";
    /// When it happened (a history entry, an entry of `design.approved`).
    pub const AT: &str = "at";
    /// What the user approved, or why (a gate record, an entry, a history entry).
    pub const NOTE: &str = "note";
    /// What a history entry records: `init`, `phase`, `mode`, `approve`, `reopen`, ...
    pub const EVENT: &str = "event";
    /// The gate a history entry approves.
    pub const GATE: &str = "gate";
    /// The value before and after a history entry's change.
    pub const FROM: &str = "from";
    pub const TO: &str = "to";
    /// The blockers `dfw phase --force` overrode.
    pub const FORCED: &str = "forced";
    pub const REASON: &str = "reason";
}

/// The design version, as a refusal prints it (`String(design.version)`: `undefined` when
/// missing), when the design is locked (approved and frozen, G2): `design.locked` read by
/// JavaScript truthiness (`"0"` and `[]` lock, `0` and `""` do not). The one test every writer
/// of design files uses: the write gate, Studio edits, Penpot token import (AGENTS.md rule 1).
pub fn design_lock(state: &Value) -> Option<String> {
    let design = state.get(field::DESIGN)?;
    js::truthy(design.get(field::LOCKED)).then(|| js::to_string(design.get(field::VERSION)))
}

/// `design.approved` as found: callers check its type with their own error.
pub fn approved_versions(state: &Value) -> Option<&Value> {
    state.get(field::DESIGN)?.get(field::APPROVED)
}
