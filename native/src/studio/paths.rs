//! Every path the Studio server and the review proxy answer, defined once (`contracts.md` C8):
//! `ALL` and `penpot::ALL` list them for the tests that send every path a foreign request (S7). The browser code has the same table in `core/studio/shared/paths.js`;
//! the test below keeps the two equal.

/// The Studio page.
pub const ROOT: &str = "/";
/// The Studio page under its file name.
pub const INDEX: &str = "/index.html";
/// Answered with 204: the browser asks for it on its own.
pub const FAVICON: &str = "/favicon.ico";
/// Files of `design/` (`/design/<file>`), for the frames and the Design System page.
pub const DESIGN_BASE: &str = "/design/";
/// The prefix of every dfw path.
pub const PREFIX: &str = "/__dfw/";
/// The Design System page shown in its own frame.
pub const SYSTEM_PAGE: &str = "/__dfw/system.html";
/// Server-Sent Events: `design`, `presence`, `comments`.
pub const EVENTS: &str = "/__dfw/events";
pub const OVERLAY_JS: &str = "/__dfw/overlay.js";
pub const LIVE_JS: &str = "/__dfw/live.js";
/// Static Studio files (`/__dfw/static/<file>`).
pub const STATIC: &str = "/__dfw/static/";
/// Screenshots (`/__dfw/shots/<version>/<file>`).
pub const SHOTS: &str = "/__dfw/shots/";
/// The Penpot bridge (`/__dfw/penpot/<file>`).
pub const PENPOT: &str = "/__dfw/penpot/";
/// Answers `{"dfw":"studio"}`: how `dfw studio start` knows the server is up.
pub const PING: &str = "/__dfw/api/ping";
/// The review proxy's own ping.
pub const LIVE: &str = "/__dfw/api/live";
pub const MANIFEST: &str = "/__dfw/api/manifest";
pub const STATE: &str = "/__dfw/api/state";
pub const TOKENS: &str = "/__dfw/api/tokens";
pub const COMPONENTS: &str = "/__dfw/api/components";
pub const COMMENTS: &str = "/__dfw/api/comments";
/// One comment: `/__dfw/api/comments/<id>` (PATCH).
pub const COMMENT: &str = "/__dfw/api/comments/";
/// Hand every draft comment to the agent (POST).
pub const COMMENTS_SEND: &str = "/__dfw/api/comments/send";
/// Whether a `dfw comments wait` listens for this project (GET).
pub const LISTENING: &str = "/__dfw/api/listening";
pub const SESSIONS: &str = "/__dfw/api/sessions";
pub const SHOOT: &str = "/__dfw/api/shoot";
pub const EDIT: &str = "/__dfw/api/edit";
pub const UNDO: &str = "/__dfw/api/undo";

/// Every path above as `(name, path)`.
#[cfg(test)]
pub const ALL: [(&str, &str); 26] = [
    ("ROOT", ROOT),
    ("INDEX", INDEX),
    ("FAVICON", FAVICON),
    ("DESIGN_BASE", DESIGN_BASE),
    ("PREFIX", PREFIX),
    ("SYSTEM_PAGE", SYSTEM_PAGE),
    ("EVENTS", EVENTS),
    ("OVERLAY_JS", OVERLAY_JS),
    ("LIVE_JS", LIVE_JS),
    ("STATIC", STATIC),
    ("SHOTS", SHOTS),
    ("PENPOT", PENPOT),
    ("PING", PING),
    ("LIVE", LIVE),
    ("MANIFEST", MANIFEST),
    ("STATE", STATE),
    ("TOKENS", TOKENS),
    ("COMPONENTS", COMPONENTS),
    ("COMMENTS", COMMENTS),
    ("COMMENT", COMMENT),
    ("COMMENTS_SEND", COMMENTS_SEND),
    ("LISTENING", LISTENING),
    ("SESSIONS", SESSIONS),
    ("SHOOT", SHOOT),
    ("EDIT", EDIT),
    ("UNDO", UNDO),
];

/// The routes under [`PENPOT`]: what the dfw bridge panel reads and writes, and its files.
pub mod penpot {
    /// The token and the origins the panel needs before it writes (GET).
    pub const SESSION: &str = "session.json";
    /// The last export (GET reads it, POST renders a new one).
    pub const EXPORT: &str = "export.json";
    /// Designer changes sent back as comments (POST).
    pub const CHANGES: &str = "changes";
    /// The file's comment threads, both ways (POST).
    pub const SYNC: &str = "sync";
    /// The plugin manifest Penpot installs.
    pub const MANIFEST: &str = "manifest.json";
    pub const PLUGIN_JS: &str = "plugin.js";
    pub const ICON: &str = "icon.svg";
    /// The panel page, framed only by Penpot.
    pub const UI: &str = "ui.html";
    /// The panel's decisions, loaded by the panel page.
    pub const UI_JS: &str = "ui.js";

    /// The routes that write: each needs the panel's token.
    #[cfg(test)]
    pub const WRITES: [&str; 3] = [EXPORT, CHANGES, SYNC];

    /// Every route above as `(name, path)`.
    #[cfg(test)]
    pub const ALL: [(&str, &str); 9] = [
        ("SESSION", SESSION),
        ("EXPORT", EXPORT),
        ("CHANGES", CHANGES),
        ("SYNC", SYNC),
        ("MANIFEST", MANIFEST),
        ("PLUGIN_JS", PLUGIN_JS),
        ("ICON", ICON),
        ("UI", UI),
        ("UI_JS", UI_JS),
    ];
}

#[cfg(test)]
mod tests {
    /// `(name, value)` of every `pub const NAME: &str = "value";` at this indent of `source`.
    fn declared(source: &str, indent: &str) -> Vec<(String, String)> {
        source
            .lines()
            .filter_map(|l| l.strip_prefix(indent)?.strip_prefix("pub const "))
            .filter_map(|l| {
                let (name, rest) = l.split_once(": &str = \"")?;
                Some((name.to_string(), rest.strip_suffix("\";")?.to_string()))
            })
            .collect()
    }

    /// `ALL` and `penpot::ALL` name every path of this file under its own name, so the refusal
    /// tests reach a path added later too.
    #[test]
    fn all_lists_every_path() {
        let source = include_str!("paths.rs");
        let source = source
            .split("\n#[cfg(test)]\nmod tests")
            .next()
            .unwrap_or_default();
        let (top, penpot) = source.split_once("pub mod penpot {").unwrap();
        let listed = |all: &[(&str, &str)]| -> Vec<(String, String)> {
            all.iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect()
        };
        assert_eq!(declared(top, ""), listed(&super::ALL));
        assert_eq!(declared(penpot, "    "), listed(&super::penpot::ALL));
    }

    /// Tags written as text name the same paths (a `const` cannot be built from these).
    #[test]
    fn script_tags_use_these_paths() {
        assert!(crate::studio::api::OVERLAY_TAG.contains(&format!("src=\"{}\"", super::OVERLAY_JS)));
    }

    /// Every path of `core/studio/shared/paths.js` is here, under the same name and value.
    #[test]
    fn the_browser_table_matches() {
        /// The browser's table of the paths, in `core/studio`.
        const TABLE: &str = "shared/paths.js";
        let js = crate::assets::studio_text(TABLE).expect(TABLE);
        let ours = [
            ("SYSTEM_PAGE", super::SYSTEM_PAGE),
            ("EVENTS", super::EVENTS),
            ("MANIFEST", super::MANIFEST),
            ("STATE", super::STATE),
            ("TOKENS", super::TOKENS),
            ("COMPONENTS", super::COMPONENTS),
            ("COMMENTS", super::COMMENTS),
            ("COMMENTS_SEND", super::COMMENTS_SEND),
            ("LISTENING", super::LISTENING),
            ("SESSIONS", super::SESSIONS),
            ("SHOOT", super::SHOOT),
            ("EDIT", super::EDIT),
            ("UNDO", super::UNDO),
        ];
        let mut in_js = 0;
        let table = js
            .split("export const PATH = Object.freeze({")
            .nth(1)
            .and_then(|rest| rest.split("});").next())
            .expect("the PATH table in shared/paths.js");
        for line in table.lines() {
            let Some((name, rest)) = line.trim().split_once(": '") else {
                continue;
            };
            let value = rest.trim_end_matches("',");
            in_js += 1;
            let rust = ours.iter().find(|(n, _)| *n == name);
            assert_eq!(rust.map(|(_, v)| *v), Some(value), "PATH.{name}");
        }
        assert_eq!(
            in_js,
            ours.len(),
            "every Rust path the browser calls is in paths.js"
        );
    }
}
