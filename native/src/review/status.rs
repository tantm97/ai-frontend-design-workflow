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
    pub const STATUS: &str = "status";
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
        let next = |s: &str| names(Status::parse(s).unwrap().next());
        assert_eq!(next("open"), "acknowledged, fixed, dismissed");
        assert_eq!(next("acknowledged"), "fixed, dismissed");
        assert_eq!(next("fixed"), "resolved, reopened");
        assert_eq!(next("reopened"), "acknowledged, fixed, dismissed");
        assert_eq!(next("resolved"), "reopened");
        assert_eq!(next("dismissed"), "reopened");
        let active: Vec<_> = Status::ALL.into_iter().filter(|s| s.is_active()).collect();
        assert_eq!(names(&active), "open, acknowledged, reopened");
        assert_eq!(
            of(&serde_json::json!({ "status": "fixed" })),
            Some(Status::Fixed)
        );
        assert_eq!(of(&serde_json::json!({ "status": 3 })), None);
    }
}
