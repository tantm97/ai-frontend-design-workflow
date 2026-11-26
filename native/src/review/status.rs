//! The statuses of a review comment and the moves between them, in one place. The strings are
//! the ones written to `.dfw/reviews/<session>.json` and printed by the CLI; they never change.

/// A comment's status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Open,
    Acknowledged,
    Fixed,
    Resolved,
    Reopened,
    Dismissed,
}

impl Status {
    /// Every status, in the order messages list them.
    pub const ALL: [Status; 6] = [
        Status::Open,
        Status::Acknowledged,
        Status::Fixed,
        Status::Resolved,
        Status::Reopened,
        Status::Dismissed,
    ];

    /// The name on disk and in the CLI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Status::Open => "open",
            Status::Acknowledged => "acknowledged",
            Status::Fixed => "fixed",
            Status::Resolved => "resolved",
            Status::Reopened => "reopened",
            Status::Dismissed => "dismissed",
        }
    }

    /// `None` for a name that is not a status (a hand-edited file): it counts in no group and
    /// cannot move.
    pub fn parse(name: &str) -> Option<Status> {
        Status::ALL.into_iter().find(|s| s.as_str() == name)
    }

    /// The statuses this one may move to. "fixed" means the agent applied a change; only the
    /// user resolves (checked where the move is made).
    pub const fn next(self) -> &'static [Status] {
        match self {
            Status::Open | Status::Reopened => {
                &[Status::Acknowledged, Status::Fixed, Status::Dismissed]
            }
            Status::Acknowledged => &[Status::Fixed, Status::Dismissed],
            Status::Fixed => &[Status::Resolved, Status::Reopened],
            Status::Resolved | Status::Dismissed => &[Status::Reopened],
        }
    }

    /// Waiting for the agent: open, acknowledged or reopened.
    pub const fn is_active(self) -> bool {
        matches!(self, Status::Open | Status::Acknowledged | Status::Reopened)
    }
}

/// The status of a comment record (`None` when missing or unknown).
pub fn of(comment: &serde_json::Value) -> Option<Status> {
    comment
        .get(field::STATUS)
        .and_then(serde_json::Value::as_str)
        .and_then(Status::parse)
}

/// The names of `statuses`, joined by ", " (for messages).
pub fn names(statuses: &[Status]) -> String {
    statuses
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Field names of a comment record.
pub mod field {
    pub const ID: &str = "id";
    pub const DESIGN_VERSION: &str = "designVersion";
    /// The design file the comment is on, relative to the project.
    pub const FILE: &str = "file";
    /// What the comment points at: `dfwId`, `selector`, `source`.
    pub const TARGET: &str = "target";
    /// `file:line` (or `file:line:col`) of the target, inside [`TARGET`].
    pub const SOURCE: &str = "source";
    /// The comment's text.
    pub const COMMENT: &str = "comment";
    pub const STATUS: &str = "status";
    /// The replies and status changes, oldest first.
    pub const THREAD: &str = "thread";
    /// Who posted the comment (`dfw-studio`, `dfw-live`, a Penpot bridge).
    pub const PROVIDER: &str = "provider";
    pub const UPDATED_AT: &str = "updatedAt";
    pub const CREATED_AT: &str = "createdAt";
    /// Where on the canvas the comment was made: screen, variant, breakpoint, state, theme.
    pub const SCREEN: &str = "screen";
    pub const VARIANT: &str = "variant";
    pub const BREAKPOINT: &str = "breakpoint";
    pub const STATE: &str = "state";
    pub const THEME: &str = "theme";
    /// The screenshot file of the comment, relative to the project.
    pub const SCREENSHOT: &str = "screenshot";
    /// How the comment was closed: `{ at, by, note, evidence }`.
    pub const RESOLUTION: &str = "resolution";
    /// `true` while a Studio draft waits for the user's "Send to AI".
    pub const DRAFT: &str = "draft";
    /// When a draft was sent.
    pub const SENT_AT: &str = "sentAt";
    /// The page of the running app a live comment is on.
    pub const URL: &str = "url";
    /// The id of the review session a comment was posted in (in what the commands print).
    pub const SESSION: &str = "session";

    /* inside [`TARGET`] */
    /// The `data-dfw-id` of the element.
    pub const DFW_ID: &str = "dfwId";
    /// A CSS selector of the element, when it has no dfw id.
    pub const SELECTOR: &str = "selector";
    /// The component master the element is an instance of.
    pub const COMPONENT: &str = "component";

    /* one line of [`THREAD`], and the [`RESOLUTION`] */
    pub const AT: &str = "at";
    /// Who wrote the line ([`super::by`]).
    pub const BY: &str = "by";
    pub const MESSAGE: &str = "message";
    /// The message of a resolution.
    pub const NOTE: &str = "note";
    /// What shows the fix (a screenshot, a check).
    pub const EVIDENCE: &str = "evidence";

    /* a review session file */
    /// The format of the session file (`1`).
    pub const SCHEMA: &str = "schema";
    pub const STARTED_AT: &str = "startedAt";
    /// When the session ended; a session without it is the open one.
    pub const CLOSED_AT: &str = "closedAt";
    /// The comments of the session, oldest first.
    pub const COMMENTS: &str = "comments";
}

/// The keys of `.dfw/reviews/listening.json`, the marker of a running `dfw comments wait`.
pub mod listening {
    /// The process id of the wait.
    pub const PID: &str = "pid";
    /// When the wait started.
    pub const SINCE: &str = "since";
}

/// Who changes a comment (`by` of a thread line). Only [`by::USER`] resolves.
pub mod by {
    pub const USER: &str = "user";
    /// The agent, when the change names no one.
    pub const CLAUDE: &str = "claude";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_unknown_names_are_none() {
        for s in Status::ALL {
            assert_eq!(Status::parse(s.as_str()), Some(s));
        }
        assert_eq!(Status::parse("later"), None);
        assert_eq!(Status::parse("Open"), None);
        assert_eq!(
            names(&Status::ALL),
            "open, acknowledged, fixed, resolved, reopened, dismissed"
        );
    }

    /// The moves of the table this replaced (review/comments.rs `NEXT`), in its order.
    #[test]
    fn transitions_and_groups() {
        use Status::*;
        let from_open = [Acknowledged, Fixed, Dismissed];
        assert_eq!(Open.next(), from_open);
        assert_eq!(Acknowledged.next(), [Fixed, Dismissed]);
        assert_eq!(Fixed.next(), [Resolved, Reopened]);
        assert_eq!(Reopened.next(), from_open);
        assert_eq!(Resolved.next(), [Reopened]);
        assert_eq!(Dismissed.next(), [Reopened]);
        let active: Vec<_> = Status::ALL.into_iter().filter(|s| s.is_active()).collect();
        assert_eq!(names(&active), "open, acknowledged, reopened");
        assert_eq!(
            of(&serde_json::json!({ "status": "fixed" })),
            Some(Status::Fixed)
        );
        assert_eq!(of(&serde_json::json!({ "status": 3 })), None);
    }
}
