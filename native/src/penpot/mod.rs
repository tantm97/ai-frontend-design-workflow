//! The optional Penpot bridge.

pub mod diff;
pub mod export;
pub mod import;
pub mod live;
pub mod threads;
pub mod tokens;

/// The words of the files dfw and the dfw bridge plugin exchange: the export, the change file
/// the plugin sends back and the live comment sync.
pub(crate) mod wire {
    /// The key of a board (`screen|variant|breakpoint`) or of a component.
    pub(crate) const KEY: &str = "key";
    pub(crate) const NAME: &str = "name";
    /// The kind of a layer (see [`kind`]).
    pub(crate) const KIND: &str = "kind";
    /// The structure signature of a layer tree.
    pub(crate) const SIG: &str = "sig";
    /// The text of a layer, a removed layer or a reply.
    pub(crate) const TEXT: &str = "text";
    pub(crate) const BOARDS: &str = "boards";
    pub(crate) const COMPONENTS: &str = "components";
    pub(crate) const LAYERS: &str = "layers";
    /// The layers of a board or a component that the export does not know.
    pub(crate) const ADDED: &str = "added";
    pub(crate) const MISSING_COMPONENTS: &str = "missingComponents";
    /// The comment threads of a change file, and the messages of one thread.
    pub(crate) const COMMENTS: &str = "comments";
    pub(crate) const EXPORT_ID: &str = "exportId";
    pub(crate) const FILE_ID: &str = "fileId";
    pub(crate) const PUSHED_AT: &str = "pushedAt";
    /// The result of a live sync: the comments whose status changed, the replies to post.
    pub(crate) const UPDATED: &str = "updated";
    pub(crate) const REPLIES: &str = "replies";

    /// The kinds of a layer.
    pub(crate) mod kind {
        pub(crate) const FRAME: &str = "frame";
        pub(crate) const TEXT: &str = "text";
    }
}
