//! The layout of a project: the names of its folders and files, one home for every path
//! `dfw` reads or writes in a project. It uses nothing else, so the binary tests include it too
//! (`#[path = "../src/core/paths/layout.rs"] mod layout;`) and build their fixture paths from it.

/// Names of the folders and files of a project, one level each: what a `join` takes. A tokens
/// folder (`design/tokens`, or an approved copy of it) holds [`file::TOKENS_JSON`],
/// [`file::TOKENS_CSS`] and [`file::THEMES`]; a snapshot (`design/approved/v1`) holds a copy of
/// `design/` and [`file::APPROVAL_LOCK`]; `.dfw/penpot` holds the `PENPOT_*` files.
pub mod file {
    pub const DESIGN: &str = "design";
    pub const SYSTEM: &str = "system";
    pub const TOKENS: &str = "tokens";
    pub const COMPONENTS: &str = "components";
    pub const ICONS: &str = "icons";
    pub const THEMES: &str = "themes";
    pub const HEAD: &str = "head.html";
    pub const STYLE: &str = "style.json";
    pub const INVENTORY: &str = "inventory.json";
    pub const COMPONENTS_CSS: &str = "components.css";
    pub const TOKENS_JSON: &str = "tokens.json";
    pub const TOKENS_CSS: &str = "tokens.css";
    /// The design manifest.
    pub const MANIFEST: &str = "design.json";
    pub const SCREENS: &str = "screens";
    pub const STYLES: &str = "styles";
    pub const BASE_CSS: &str = "base.css";
    /// Brand images: logos, ornaments, photography.
    pub const ASSETS: &str = "assets";
    /// Frozen snapshots, one folder per version.
    pub const APPROVED: &str = "approved";
    pub const HANDOFF: &str = "handoff";
    /// The hashes of a snapshot's files.
    pub const APPROVAL_LOCK: &str = "approval.lock.json";

    /// The workflow folder.
    pub const DFW: &str = ".dfw";
    /// The manifest of a JavaScript project: its dependencies and scripts.
    pub const PACKAGE_JSON: &str = "package.json";
    pub const STATE: &str = "state.json";
    /// The project config in `.dfw`, and the global config in the dfw home.
    pub const CONFIG: &str = "config.json";
    pub const CONFIG_LOCAL: &str = "config.local.json";
    pub const DECISIONS: &str = "decisions.jsonl";
    pub const STACK: &str = "stack.json";
    pub const STUDIO: &str = "studio.json";
    pub const STUDIO_LOG: &str = "studio.log";
    pub const REVIEW: &str = "review.json";
    pub const REVIEW_LOG: &str = "review.log";
    pub const REVIEWS: &str = "reviews";
    /// The comment ids already handed to the agent, in [`REVIEWS`].
    pub const REVIEWS_DELIVERED: &str = ".delivered.json";
    /// Whether an agent session listens for comments, in [`REVIEWS`].
    pub const REVIEWS_LISTENING: &str = ".listening.json";
    /// The lock folder of [`REVIEWS`] (`core::lock`).
    pub const REVIEWS_LOCK: &str = ".lock";
    /// The file in a lock folder that names who holds it (`core::lock`).
    pub const LOCK_OWNER: &str = "owner";
    pub const CHECKS: &str = "checks";
    /// The setup page written in [`CHECKS`].
    pub const SETUP_PAGE: &str = "setup.html";
    pub const BUILD: &str = "build";
    pub const SHOTS: &str = "shots";
    /// The undo journal of direct edits.
    pub const EDITS: &str = "edits";
    pub const PRESENCE: &str = "presence.json";
    pub const LAYOUT_CACHE: &str = "layout-cache.json";
    pub const GITIGNORE: &str = ".gitignore";
    pub const PENPOT: &str = "penpot";
    pub const PENPOT_EXPORT: &str = "export.json";
    pub const PENPOT_EXPORTS: &str = "exports";
    pub const PENPOT_TOKENS: &str = "tokens.penpot.json";
    pub const PENPOT_TOKENS_IMPORTED: &str = "tokens.from-penpot.json";
    pub const PENPOT_IMPORTED: &str = "imported.json";
    pub const PENPOT_THREADS: &str = "threads.json";
    pub const PENPOT_CHANGES: &str = "changes";
    /// The lock of the Penpot ledgers (`threads.json`, `imported.json`), in `.dfw/penpot`.
    pub const PENPOT_LOCK: &str = ".lock";
    /// The extension of a JSON file, without its dot.
    pub const JSON_EXT: &str = "json";
    /// Studio screenshots in [`SHOTS`].
    pub const SHOTS_STUDIO: &str = "studio";
    /// Screenshots of a URL in [`SHOTS`].
    pub const SHOTS_URL: &str = "url";

    /// The file of the icon `name` (`search.svg`), in [`ICONS`].
    pub fn icon(name: &str) -> String {
        format!("{name}.svg")
    }

    /// The file of the theme `name` (`dark.json`), in [`THEMES`].
    pub fn theme(name: &str) -> String {
        format!("{name}.json")
    }

    /// The master file of the component `name` (`button.html`), in [`COMPONENTS`].
    pub fn master(name: &str) -> String {
        format!("{name}.html")
    }
}

/// Design system files relative to `design/`, as pages link them (`{base}tokens/tokens.css`).
pub mod in_design {
    pub const HEAD: &str = "system/head.html";
    pub const COMPONENTS_CSS: &str = "system/components.css";
    pub const TOKENS_CSS: &str = "tokens/tokens.css";
    pub const TOKENS_JSON: &str = "tokens/tokens.json";
    pub const BASE_CSS: &str = "styles/base.css";
}

/// Project paths relative to the project root, with `/` as messages, output and
/// project-relative targets write them. `ProjectPaths` has the same places as paths.
pub mod rel {
    pub const DESIGN: &str = "design";
    pub const MANIFEST: &str = "design/design.json";
    pub const SCREENS: &str = "design/screens";
    #[cfg(test)]
    pub const STYLES: &str = "design/styles";
    #[cfg(test)]
    pub const BASE_CSS: &str = "design/styles/base.css";
    pub const APPROVED: &str = "design/approved";
    pub const ASSETS: &str = "design/assets";
    pub const HANDOFF: &str = "design/handoff";
    pub const DFW: &str = ".dfw";
    pub const STATE: &str = ".dfw/state.json";
    pub const CONFIG: &str = ".dfw/config.json";
    pub const CONFIG_LOCAL: &str = ".dfw/config.local.json";
    pub const DECISIONS: &str = ".dfw/decisions.jsonl";
    pub const STACK: &str = ".dfw/stack.json";
    pub const REVIEWS: &str = ".dfw/reviews";
    pub const BUILD: &str = ".dfw/build";
    #[cfg(test)]
    pub const SHOTS: &str = ".dfw/shots";
    #[cfg(test)]
    pub const EDITS: &str = ".dfw/edits";
    #[cfg(test)]
    pub const PENPOT: &str = ".dfw/penpot";
    pub const PENPOT_EXPORT: &str = ".dfw/penpot/export.json";
    pub const PENPOT_TOKENS: &str = ".dfw/penpot/tokens.penpot.json";
    pub const SYSTEM: &str = "design/system";
    pub const TOKENS: &str = "design/tokens";
    pub const COMPONENTS: &str = "design/system/components";
    pub const ICONS: &str = "design/system/icons";
    pub const THEMES: &str = "design/tokens/themes";
    pub const HEAD: &str = "design/system/head.html";
    pub const STYLE: &str = "design/system/style.json";
    pub const INVENTORY: &str = "design/system/inventory.json";
    pub const COMPONENTS_CSS: &str = "design/system/components.css";
    pub const TOKENS_JSON: &str = "design/tokens/tokens.json";
    /// The brief `dfw init` creates and `dfw style match` reads.
    pub const BRIEF: &str = "design/system/brief.json";
    /// A project's own style guide file (core/workflow/references/phase-direction.md).
    pub const STYLE_SOURCE: &str = "design/system/style-source.json";
    /// Where `dfw tokens extract` writes its seed by default.
    pub const EXTRACTED_TOKENS: &str = "design/system/extracted.tokens.json";
    /// The built Design System page.
    pub const SYSTEM_PAGE: &str = "design/system/index.html";
    /// The files that hold the gates, approvals, decisions, settings and review comments: dfw
    /// commands write them, the agent's own file tools never do (`design-files.md` D11). A
    /// folder of [`GATE_STATE_DIRS`] stands for everything under it.
    pub const GATE_STATE: [&str; 6] = [STATE, CONFIG, CONFIG_LOCAL, DECISIONS, STACK, REVIEWS];
    /// The folders of [`GATE_STATE`].
    pub const GATE_STATE_DIRS: [&str; 1] = [REVIEWS];
    /// The product brief at the project root.
    pub const PRODUCT_DOC: &str = "PRODUCT.md";
    /// The design notes at the project root.
    pub const DESIGN_DOC: &str = "DESIGN.md";

    /// The glob of everything under the folder `dir` (`design/**`).
    pub fn all_under(dir: &str) -> String {
        format!("{dir}/**")
    }

    /// `rest` (one or more `/`-separated parts) inside the folder `dir`.
    pub fn join(dir: &str, rest: &str) -> String {
        format!("{dir}/{rest}")
    }

    /// A master file (`button.html`) of `design/system/components/`.
    pub fn component(file: &str) -> String {
        join(COMPONENTS, file)
    }

    /// A path relative to `design/` (`screens/a.html`, as the manifest writes it) made
    /// project-relative.
    pub fn design(rest: &str) -> String {
        join(DESIGN, rest)
    }

    /// The master of the component `name` (`design/system/components/button.html`).
    pub fn master(name: &str) -> String {
        component(&super::file::master(name))
    }

    /// A file or folder of `design/screens/` (`home/a.html`).
    #[cfg(test)]
    pub fn screen(rest: &str) -> String {
        join(SCREENS, rest)
    }

    /// A file or folder of `design/approved/` (`v1`, `v1/design.json`).
    pub fn approved(rest: &str) -> String {
        join(APPROVED, rest)
    }

    /// `rel` is inside the folder `dir` (a `/`-separated project-relative path).
    pub fn under(dir: &str, rel: &str) -> bool {
        inside(dir, rel).is_some()
    }

    /// The part of `rel` inside the folder `dir` (`design/a/b.html` in `design` is `a/b.html`).
    pub fn inside<'a>(dir: &str, rel: &'a str) -> Option<&'a str> {
        rel.strip_prefix(dir)?.strip_prefix('/')
    }
}
