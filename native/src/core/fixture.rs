//! The file names test fixtures use: the app's own source files (what the write gate calls
//! implementation code), files outside the project, and the design's files (screens, masters,
//! themes) as a path helper of `core::paths::rel` takes them. Named once, by role, so a test
//! says which kind of file it writes. Test code only (`#[cfg(test)]` in
//! `core`); it uses nothing else, so the binary tests include it too
//! (`#[path = "../src/core/fixture.rs"] mod fixture;`).

/// The folder of the app's source files.
pub const APP_DIR: &str = "src";
/// A file of the app: implementation code the write gate holds back until G2, G3 and the
/// implement phase.
pub const APP_FILE: &str = "src/a.ts";
/// Another file of the app, for a second write in the same test.
pub const APP_MODULE: &str = "src/app.ts";
/// The app's entry point.
pub const APP_ENTRY: &str = "src/main.ts";
/// A file a patch adds to the app.
pub const ADDED_APP_FILE: &str = "src/x.ts";
/// A file in a folder of the app that does not exist yet.
pub const NEW_APP_FILE: &str = "src/new/b.ts";
/// A file of the app that does not exist yet; a dangling link points at it.
pub const LINKED_APP_FILE: &str = "src/new.ts";
/// A component of the app.
pub const APP_COMPONENT: &str = "src/App.tsx";
/// The test file of [`APP_COMPONENT`].
pub const APP_COMPONENT_TEST: &str = "src/App.test.tsx";
/// A component in a folder of components.
pub const NESTED_COMPONENT: &str = "src/components/Foo.tsx";
/// A component at the top of the source folder.
pub const TOP_COMPONENT: &str = "src/Foo.tsx";
/// A component whose name must never leave the machine (telemetry tests).
pub const SECRET_COMPONENT: &str = "src/SecretComponent.tsx";
/// A TypeScript JSX file of the app.
pub const APP_TSX: &str = "src/a.tsx";
/// A JavaScript JSX file of the app.
pub const APP_JSX: &str = "src/a.jsx";
/// A Vue component of the app.
pub const APP_VUE: &str = "src/App.vue";
/// An HTML page of the app, outside `design/`.
pub const APP_PAGE: &str = "src/app.html";

/// A folder next to the project's files that no command may reach.
pub const OUTSIDE_DIR: &str = "outside";
/// A page in [`OUTSIDE_DIR`].
pub const OUTSIDE_DIR_PAGE: &str = "outside/x.html";
/// A page at the project root, outside `design/`.
pub const OUTSIDE_PAGE: &str = "outside.html";
/// A JSON file at the project root, outside the folder a command reads.
pub const OUTSIDE_JSON: &str = "outside.json";
/// A file of another folder than the project, relative to the folder holding both.
pub const ELSEWHERE_FILE: &str = "elsewhere/x.ts";
/// A file of a sibling project, relative to the project root.
pub const SIBLING_FILE: &str = "../other/a.ts";

/// A symlink next to the project that points at itself: a path through it cannot be resolved.
pub const LOOP_LINK: &str = "loop";
/// A file under [`LOOP_LINK`].
pub const IN_LOOP_FILE: &str = "x.ts";

// The design's files, relative to the folder a `rel` helper takes them in: `rel::screen` for
// a screen's page, `rel::component` for a master, `rel::join(rel::THEMES, ..)` for a theme.

/// A screen id: a folder of `design/screens`.
pub const SCREEN: &str = "home";
/// The page of [`SCREEN`]: its first variant.
pub const SCREEN_PAGE: &str = "home/a.html";
/// The second variant of [`SCREEN`].
pub const SCREEN_PAGE_B: &str = "home/b.html";
/// The index page of [`SCREEN`].
pub const SCREEN_INDEX: &str = "home/index.html";
/// An HTML file straight in a folder (`design/a.html`, `design/screens/a.html`).
pub const HTML_FILE: &str = "a.html";
/// A second HTML file next to [`HTML_FILE`].
pub const HTML_FILE_B: &str = "b.html";
/// Another HTML file, where the name does not matter.
pub const OTHER_HTML_FILE: &str = "x.html";
/// An HTML file that is never written.
pub const MISSING_HTML_FILE: &str = "nope.html";
/// The page a screen folder opens with.
pub const INDEX_FILE: &str = "index.html";
/// The screen of [`INDEX_PAGE`].
pub const INDEX_SCREEN: &str = "a";
/// The index page of a screen.
pub const INDEX_PAGE: &str = "a/index.html";
/// The index page of a second screen.
pub const INDEX_PAGE_B: &str = "b/index.html";
/// The index page of a third screen.
pub const INDEX_PAGE_C: &str = "c/index.html";
/// A page one folder down (`design_base_for` is `../../` in `design/screens`).
pub const NESTED_PAGE: &str = "a/b.html";
/// A page that uses components.
pub const COMPOSED_PAGE: &str = "s/a.html";
/// A second page next to [`COMPOSED_PAGE`].
pub const COMPOSED_PAGE_B: &str = "s/b.html";
/// A third page next to [`COMPOSED_PAGE`].
pub const COMPOSED_PAGE_C: &str = "s/c.html";
/// A page next to [`COMPOSED_PAGE`] that uses no component.
pub const PLAIN_PAGE: &str = "s/plain.html";
/// The page of the hook tests' screen.
pub const DEMO_PAGE: &str = "demo/a.html";
/// The page of the Penpot and Studio tests' screen.
pub const ITEMS_PAGE: &str = "items/a.html";
/// A page edited many times in a row.
pub const MANY_EDITS_PAGE: &str = "many/a.html";
/// A page of `design/` outside the screens.
pub const DESIGN_PAGE: &str = "pages/plain.html";
/// A second page next to [`DESIGN_PAGE`].
pub const DESIGN_PAGE_B: &str = "pages/b.html";
/// A component file of a framework next to [`DESIGN_PAGE`].
pub const DESIGN_VUE: &str = "pages/components/x.vue";
/// A stylesheet.
pub const CSS_FILE: &str = "a.css";
/// Another stylesheet, where the name does not matter.
pub const OTHER_CSS_FILE: &str = "x.css";
/// A file of notes: not a design file.
pub const NOTES_FILE: &str = "notes.md";
/// A tokens file of a brand, outside `design/tokens`.
pub const BRAND_TOKENS: &str = "brand.tokens.json";
/// The master of the component `button`.
pub const BUTTON_MASTER: &str = "button.html";
/// The master of the component `app-shell`.
pub const APP_SHELL_MASTER: &str = "app-shell.html";
/// The master of the component `divider`.
pub const DIVIDER_MASTER: &str = "divider.html";
/// The master of the component `chip`.
pub const CHIP_MASTER: &str = "chip.html";
/// The master of the component `logo`.
pub const LOGO_MASTER: &str = "logo.html";
/// A theme file of `design/tokens/themes`.
pub const DARK_THEME: &str = "dark.json";
/// A handoff file of `design/handoff`.
pub const HANDOFF_NOTE: &str = "v1.md";
/// A review session file of `.dfw/reviews`.
pub const REVIEW_SESSION: &str = "rv_20200101T000000.json";

/// The first page ([`HTML_FILE`]) of the screen `id`, in `design/screens`.
pub fn screen_page(id: &str) -> String {
    format!("{id}/{HTML_FILE}")
}

/// The index page ([`INDEX_FILE`]) of the screen `id`, in `design/screens`.
pub fn index_page(id: &str) -> String {
    format!("{id}/{INDEX_FILE}")
}

/// The page of the variant `variant` of the screen `screen`, in `design/screens`.
pub fn variant_page(screen: &str, variant: &str) -> String {
    format!("{screen}/{variant}.html")
}

/// The app's files above.
pub const APP_FILES: [&str; 15] = [
    APP_FILE,
    APP_MODULE,
    APP_ENTRY,
    ADDED_APP_FILE,
    NEW_APP_FILE,
    LINKED_APP_FILE,
    APP_COMPONENT,
    APP_COMPONENT_TEST,
    NESTED_COMPONENT,
    TOP_COMPONENT,
    SECRET_COMPONENT,
    APP_TSX,
    APP_JSX,
    APP_VUE,
    APP_PAGE,
];

/// The names outside the app and the project above.
pub const OUTSIDE_FILES: [&str; 8] = [
    OUTSIDE_DIR,
    OUTSIDE_DIR_PAGE,
    OUTSIDE_PAGE,
    OUTSIDE_JSON,
    ELSEWHERE_FILE,
    SIBLING_FILE,
    LOOP_LINK,
    IN_LOOP_FILE,
];

/// The design's files above.
pub const DESIGN_FILES: [&str; 36] = [
    SCREEN,
    SCREEN_PAGE,
    SCREEN_PAGE_B,
    SCREEN_INDEX,
    HTML_FILE,
    HTML_FILE_B,
    OTHER_HTML_FILE,
    MISSING_HTML_FILE,
    INDEX_FILE,
    INDEX_SCREEN,
    INDEX_PAGE,
    INDEX_PAGE_B,
    INDEX_PAGE_C,
    NESTED_PAGE,
    COMPOSED_PAGE,
    COMPOSED_PAGE_B,
    COMPOSED_PAGE_C,
    PLAIN_PAGE,
    DEMO_PAGE,
    ITEMS_PAGE,
    MANY_EDITS_PAGE,
    DESIGN_PAGE,
    DESIGN_PAGE_B,
    DESIGN_VUE,
    CSS_FILE,
    OTHER_CSS_FILE,
    NOTES_FILE,
    BRAND_TOKENS,
    BUTTON_MASTER,
    APP_SHELL_MASTER,
    DIVIDER_MASTER,
    CHIP_MASTER,
    LOGO_MASTER,
    DARK_THEME,
    HANDOFF_NOTE,
    REVIEW_SESSION,
];

// design area: the starter kit's token presets and style guides, by id (design/system and
// design/style tests).

// The default preset is `design::system::DEFAULT_PRESET`.

/// A second token preset.
pub const SOFT_PRESET: &str = "soft";
/// A third token preset.
pub const INDUSTRIAL_PRESET: &str = "industrial";
/// A style guide of the kit.
pub const SWISS_GRID_STYLE: &str = "swiss-grid";
/// A second style guide of the kit.
pub const INDUSTRIAL_STYLE: &str = "industrial-utility";
/// A style guide that brings a motif and ranks first for a playful product.
pub const PLAYFUL_STYLE: &str = "playful-bold";

#[cfg(test)]
mod tests {
    use super::*;

    /// Every name is its own, the app's files are under [`APP_DIR`], the others are not.
    #[test]
    fn app_files_are_under_the_app_folder_and_the_others_are_not() {
        let app = format!("{APP_DIR}/");
        let mut all: Vec<&str> = APP_FILES.iter().chain(&OUTSIDE_FILES).copied().collect();
        assert!(APP_FILES.iter().all(|f| f.starts_with(&app)));
        assert!(!OUTSIDE_FILES.iter().any(|f| f.starts_with(&app)));
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), APP_FILES.len() + OUTSIDE_FILES.len());
    }

    /// Every design name is its own.
    #[test]
    fn design_files_are_named_once() {
        let mut all = DESIGN_FILES.to_vec();
        all.sort_unstable();
        all.dedup();
        assert_eq!(all.len(), DESIGN_FILES.len());
    }

    /// The pages of a screen are in its folder.
    #[test]
    fn the_screen_pages_are_in_the_screen_folder() {
        assert_eq!(SCREEN_PAGE, screen_page(SCREEN));
        assert_eq!(SCREEN_INDEX, index_page(SCREEN));
        assert_eq!(INDEX_PAGE, index_page(INDEX_SCREEN));
        assert_eq!(SCREEN_PAGE_B, variant_page(SCREEN, "b"));
    }
}
