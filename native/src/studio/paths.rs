//! Every path the Studio server and the review proxy answer under `/__dfw/`, defined once
//! (`contracts.md` C8). The browser code has the same table in `core/studio/shared/paths.js`;
//! the test below keeps the two equal.

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

#[cfg(test)]
mod tests {
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
