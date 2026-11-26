//! The shape of `.dfw/presence.json`: the note the pre-tool-use hook writes and the Studio reads,
//! `{ file, dfwId, at }`.

/// The keys of the presence note.
pub mod key {
    /// The design file, relative to `design/`.
    pub const FILE: &str = "file";
    /// The `data-dfw-id` of the element.
    pub const DFW_ID: &str = "dfwId";
    /// When the note was written (ISO time).
    pub const AT: &str = "at";
}
