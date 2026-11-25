//! The optional Penpot bridge.

pub mod diff;
pub mod export;
pub mod import;
pub mod live;
pub mod threads;
pub mod tokens;

/// The words of the files dfw and the dfw bridge plugin exchange: the export, the change file
/// the plugin sends back, the live comment sync and the record of synced threads. A key that
/// means one thing wherever it appears is named here; a record whose keys mean something else
/// has its own module (a snapshot's `style`, a border side, a token set, a ledger link, ...).
/// The keys of a change are `penpot::diff::field`, those of a dfw comment
/// `review::status::field`, those of a Penpot token file `penpot::tokens::key`.
pub(crate) mod wire {
    /* -------------------------------------------------------------- the export document */

    /// The format of the export or of the change file (`EXPORT_SCHEMA`, `CHANGES_SCHEMA`).
    pub(crate) const SCHEMA: &str = "schema";
    /// The id of the export, of a page, a breakpoint or a variant in a render job.
    pub(crate) const ID: &str = "id";
    pub(crate) const GENERATED_AT: &str = "generatedAt";
    /// The product name (from `design/design.json`).
    pub(crate) const PRODUCT: &str = "product";
    pub(crate) const DESIGN_VERSION: &str = "designVersion";
    /// The Penpot token file of the export; in a change file, the token sets the plugin read.
    pub(crate) const TOKENS: &str = "tokens";
    /// What the token conversion left out: `{ unsupported, lossy }`.
    pub(crate) const TOKEN_REPORT: &str = "tokenReport";
    pub(crate) const UNSUPPORTED: &str = "unsupported";
    pub(crate) const LOSSY: &str = "lossy";
    /// The colour tokens of the export: `{ name, hex, opacity }`.
    pub(crate) const COLORS: &str = "colors";
    pub(crate) const PAGES: &str = "pages";
    /// The screens that could not be rendered.
    pub(crate) const ERRORS: &str = "errors";
    /// The page of a render job: `{ id, title }`.
    pub(crate) const PAGE: &str = "page";
    pub(crate) const TITLE: &str = "title";
    /// The rendered tree of a render job.
    pub(crate) const SNAPSHOT: &str = "snapshot";
    /// A canvas page of `studioPages` that holds no screens: the Design System page, the flow.
    pub(crate) const SYSTEM: &str = "system";
    pub(crate) const FLOW: &str = "flow";

    /* --------------------------------------------------------------- boards and components */

    /// The key of a board (`screen|variant|breakpoint`) or of a component.
    pub(crate) const KEY: &str = "key";
    pub(crate) const NAME: &str = "name";
    pub(crate) const BOARDS: &str = "boards";
    pub(crate) const COMPONENTS: &str = "components";
    pub(crate) const LAYERS: &str = "layers";
    /// The board a change or a comment is on.
    pub(crate) const BOARD: &str = "board";
    pub(crate) const SCREEN: &str = "screen";
    pub(crate) const VARIANT: &str = "variant";
    pub(crate) const BREAKPOINT: &str = "breakpoint";
    /// The design file of a board or a render job.
    pub(crate) const FILE: &str = "file";
    pub(crate) const WIDTH: &str = "width";
    pub(crate) const HEIGHT: &str = "height";
    /// The height of the breakpoint's viewport (a board is as tall as its page).
    pub(crate) const VIEWPORT_HEIGHT: &str = "viewportHeight";
    /// The background paint of a board: `{ color, opacity, token }`.
    pub(crate) const BACKGROUND: &str = "background";
    /// A component's master layer tree.
    pub(crate) const ROOT: &str = "root";
    /// The master a component instance uses (on a layer), or a master's name (on a component).
    pub(crate) const COMPONENT: &str = "component";
    /// The size variant of a component.
    pub(crate) const SIZE: &str = "size";
    /// Where a component was first seen: `<board key>#<layer path>`.
    pub(crate) const FROM: &str = "from";

    /* ------------------------------------------------------------------------- a layer */

    /// The kind of a layer (see [`kind`]).
    pub(crate) const KIND: &str = "kind";
    /// The structure signature of a layer tree.
    pub(crate) const SIG: &str = "sig";
    /// The place of a layer in its tree (`"0.1.3"`).
    pub(crate) const PATH: &str = "path";
    pub(crate) const CHILDREN: &str = "children";
    /// The `data-dfw-id` of the element a layer was made from.
    pub(crate) const DFW_ID: &str = "dfwId";
    /// The id of the element itself or of its nearest ancestor with one.
    pub(crate) const ANCHOR: &str = "anchor";
    pub(crate) const TAG: &str = "tag";
    pub(crate) const X: &str = "x";
    pub(crate) const Y: &str = "y";
    pub(crate) const W: &str = "w";
    pub(crate) const H: &str = "h";
    /// The content of a text layer.
    pub(crate) const CHARACTERS: &str = "characters";
    /// The text style of a text layer: `{ color, colorToken, ... }`.
    pub(crate) const TEXT_STYLE: &str = "text";
    /// The text of a removed layer, of a reply or of a comment target.
    pub(crate) const TEXT: &str = "text";
    /// A text that is a placeholder (an input's).
    pub(crate) const PLACEHOLDER: &str = "placeholder";
    pub(crate) const SVG: &str = "svg";
    pub(crate) const ICON: &str = "icon";
    pub(crate) const ALT: &str = "alt";
    pub(crate) const SRC: &str = "src";
    /// The style of a snapshot node (see [`style`]).
    pub(crate) const STYLE: &str = "style";
    /// The paints of a layer: `[{ color, opacity, token }]`.
    pub(crate) const FILLS: &str = "fills";
    /// The borders of a layer when every side is the same: `[{ color, opacity, width, style, token }]`.
    pub(crate) const STROKES: &str = "strokes";
    /// The borders of a layer whose sides differ, by side (see [`side`]).
    pub(crate) const SIDE_BORDERS: &str = "sideBorders";
    pub(crate) const SHADOWS: &str = "shadows";
    pub(crate) const RADIUS: &str = "radius";
    pub(crate) const RADIUS_TOKEN: &str = "radiusToken";
    pub(crate) const OPACITY: &str = "opacity";
    pub(crate) const CLIP: &str = "clip";
    /// What the export could not carry over (a background image).
    pub(crate) const NOTE: &str = "note";
    /// The colour of a paint, of a text style or of a shadow.
    pub(crate) const COLOR: &str = "color";
    pub(crate) const COLOR_TOKEN: &str = "colorToken";
    /// The hex of a colour (`#rrggbb`).
    pub(crate) const HEX: &str = "hex";
    /// The colour token of a paint in the export.
    pub(crate) const TOKEN: &str = "token";

    /* -------------------------------------------------------------------- the change file */

    pub(crate) const EXPORT_ID: &str = "exportId";
    pub(crate) const FILE_ID: &str = "fileId";
    pub(crate) const PUSHED_AT: &str = "pushedAt";
    /// The layers of a board or a component that the export does not know.
    pub(crate) const ADDED: &str = "added";
    pub(crate) const MISSING_COMPONENTS: &str = "missingComponents";
    pub(crate) const MISSING_BOARDS: &str = "missingBoards";
    /// A token file the plugin sent instead of token sets.
    pub(crate) const TOKEN_FILE: &str = "tokenFile";
    /// The comment threads of a change file, and the messages of one Penpot thread.
    pub(crate) const COMMENTS: &str = "comments";
    /// The board and the component a Penpot thread is on.
    pub(crate) const BOARD_KEY: &str = "boardKey";
    pub(crate) const COMPONENT_KEY: &str = "componentKey";
    /// The number of a Penpot thread (`seq` in Penpot) on a comment change.
    pub(crate) const THREAD: &str = "thread";
    pub(crate) const SEQ: &str = "seq";
    pub(crate) const RESOLVED: &str = "resolved";
    /// The messages of a comment change: `{ author, content, at }`.
    pub(crate) const MESSAGES: &str = "messages";
    pub(crate) const AUTHOR: &str = "author";
    pub(crate) const CONTENT: &str = "content";
    pub(crate) const AT: &str = "at";
    /// Who wrote a Penpot message, and when.
    pub(crate) const USER: &str = "user";
    pub(crate) const DATE: &str = "date";
    /// The box of a change or of a comment target: `[x, y, w, h]`.
    pub(crate) const BBOX: &str = "bbox";
    /// How far a layer moved and grew: `{ x, y, w, h }`.
    pub(crate) const DELTA: &str = "delta";
    /// The layer a change inside a master is on.
    pub(crate) const LAYER: &str = "layer";
    /// A change on the master's own top layer.
    pub(crate) const IS_ROOT: &str = "isRoot";
    /// The border width a spacing change includes.
    pub(crate) const BORDER: &str = "border";
    pub(crate) const WIDTH_TOKEN: &str = "widthToken";
    /// The dimension tokens of a size change, by `w` and `h`.
    pub(crate) const SIZE_TOKENS: &str = "tokens";
    /// Whether a comment target is the element itself (not an ancestor).
    pub(crate) const EXACT: &str = "exact";
    /// The CSS selector of a comment target.
    pub(crate) const SELECTOR: &str = "selector";

    /* ------------------------------------------------------------------------ the live sync */

    /// The live sync: the threads the plugin read, the replies it posted.
    pub(crate) const THREADS: &str = "threads";
    pub(crate) const POSTED: &str = "posted";
    /// The result of a live sync: the comments whose status changed, the replies to post.
    pub(crate) const UPDATED: &str = "updated";
    pub(crate) const REPLIES: &str = "replies";

    /// The kinds of a layer.
    pub(crate) mod kind {
        pub(crate) const FRAME: &str = "frame";
        pub(crate) const TEXT: &str = "text";
    }

    /// The computed style of a snapshot node (`snapshot_page.js`).
    pub(crate) mod style {
        pub(crate) const FILL: &str = "fill";
        pub(crate) const BORDERS: &str = "borders";
        pub(crate) const SHADOWS: &str = "shadows";
        pub(crate) const RADIUS: &str = "radius";
        pub(crate) const OPACITY: &str = "opacity";
        pub(crate) const CLIP: &str = "clip";
        pub(crate) const BACKGROUND_IMAGE: &str = "backgroundImage";
    }

    /// One side of a snapshot's border, and the stroke made from it.
    pub(crate) mod border {
        /// The width of a side in the snapshot (the stroke calls it `width`).
        pub(crate) const W: &str = "w";
        /// `solid`, `dashed` or `dotted`.
        pub(crate) const STYLE: &str = "style";
    }

    /// The sides of `sideBorders` and of a spacing change.
    pub(crate) mod side {
        pub(crate) const TOP: &str = "top";
        pub(crate) const RIGHT: &str = "right";
        pub(crate) const BOTTOM: &str = "bottom";
        pub(crate) const LEFT: &str = "left";
    }

    /// The value of a DTCG colour or dimension token, as `penpot::tokens` flattens it.
    pub(crate) mod dtcg {
        pub(crate) const ALPHA: &str = "alpha";
        pub(crate) const VALUE: &str = "value";
        pub(crate) const UNIT: &str = "unit";
    }

    /// The token sets and themes the plugin sends (`tokens` of a change file), and a token
    /// change of the token import.
    pub(crate) mod token {
        pub(crate) const SETS: &str = "sets";
        /// `sets.length`: what makes an object a list in JavaScript.
        pub(crate) const LENGTH: &str = "length";
        pub(crate) const THEMES: &str = "themes";
        pub(crate) const TOKENS: &str = "tokens";
        pub(crate) const ACTIVE: &str = "active";
        pub(crate) const VALUE: &str = "value";
        pub(crate) const TYPE: &str = "type";
        pub(crate) const DESCRIPTION: &str = "description";
        pub(crate) const GROUP: &str = "group";
        pub(crate) const IS_SOURCE: &str = "isSource";
        pub(crate) const SELECTED_TOKEN_SETS: &str = "selectedTokenSets";
        pub(crate) const TOKEN_SET_ORDER: &str = "tokenSetOrder";
        pub(crate) const ACTIVE_SETS: &str = "activeSets";
        /// The token a change is on, and its value before and after.
        pub(crate) const TOKEN: &str = "token";
        pub(crate) const FROM: &str = "from";
        pub(crate) const TO: &str = "to";
        pub(crate) const DESCRIPTION_ONLY: &str = "descriptionOnly";
    }

    /// A link of the threads ledger: the dfw comment a Penpot thread continues.
    pub(crate) mod link {
        /// The id of the dfw comment.
        pub(crate) const COMMENT: &str = "comment";
        /// How many messages of the thread are in the comment.
        pub(crate) const SEEN: &str = "seen";
        pub(crate) const RESOLVED: &str = "resolved";
        /// The replies already posted in Penpot.
        pub(crate) const POSTED: &str = "posted";
    }

    /// A line of a dfw comment's `thread`, as the live sync reads it.
    pub(crate) mod line {
        pub(crate) const MESSAGE: &str = "message";
        pub(crate) const BY: &str = "by";
        pub(crate) const STATUS: &str = "status";
        pub(crate) const AT: &str = "at";
    }

    /// What an import returns (`--json`) besides `comments` and `updated`, and the live sync's
    /// extra fields.
    pub(crate) mod report {
        /// The id of the export the changes were compared with.
        pub(crate) const BASELINE: &str = "baseline";
        pub(crate) const CHANGES: &str = "changes";
        pub(crate) const SKIPPED: &str = "skipped";
        pub(crate) const WARNINGS: &str = "warnings";
        pub(crate) const TOKEN_DIFF: &str = "tokenDiff";
        /// The token file written for review.
        pub(crate) const TOKEN_FILE: &str = "tokenFile";
        pub(crate) const FROZEN: &str = "frozen";
        /// Whether the export is older than the design.
        pub(crate) const STALE: &str = "stale";
    }

    /// The keys of `.dfw/state.json` the bridge reads.
    pub(crate) mod state {
        pub(crate) use crate::workflow::state::field::{DESIGN, VERSION};
        /// `design.locked`: the design is frozen.
        pub(crate) const LOCKED: &str = "locked";
    }

    /// The keys only the fingerprint of a change has (the ledger compares fingerprints).
    pub(crate) mod fingerprint {
        /// The layer path.
        pub(crate) const P: &str = "p";
        /// The master key.
        pub(crate) const COMP: &str = "comp";
    }
}
