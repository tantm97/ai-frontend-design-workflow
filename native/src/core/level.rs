//! The level of a finding (lint, checks, layout, audits), in one place. The names are the
//! ones written to `--json` output, `.dfw/` reports and the Design System page; they never
//! change.

/// How much a finding matters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Must be fixed (shown as MUST).
    Error,
    /// Should be fixed unless a recorded decision says otherwise (SHOULD).
    Warn,
    /// Worth knowing (NOTE).
    Info,
}

impl Level {
    /// Every level, most severe first.
    pub const ALL: [Level; 3] = [Level::Error, Level::Warn, Level::Info];

    /// The name in findings.
    pub const fn as_str(self) -> &'static str {
        match self {
            Level::Error => "error",
            Level::Warn => "warn",
            Level::Info => "info",
        }
    }

    /// `None` for a name that is not a level.
    pub fn parse(name: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|l| l.as_str() == name)
    }

    /// The label in text output: MUST, SHOULD, NOTE.
    pub const fn label(self) -> &'static str {
        match self {
            Level::Error => "MUST",
            Level::Warn => "SHOULD",
            Level::Info => "NOTE",
        }
    }
}

/// The level of a finding record (`None` when missing or unknown).
pub fn of(finding: &serde_json::Value) -> Option<Level> {
    finding
        .get("level")
        .and_then(serde_json::Value::as_str)
        .and_then(Level::parse)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_labels_and_order() {
        for l in Level::ALL {
            assert_eq!(Level::parse(l.as_str()), Some(l));
        }
        assert_eq!(Level::parse("critical"), None);
        let labels: Vec<_> = Level::ALL.iter().map(|l| l.label()).collect();
        assert_eq!(labels, ["MUST", "SHOULD", "NOTE"]);
        assert!(Level::Error < Level::Warn && Level::Warn < Level::Info);
        assert_eq!(
            of(&serde_json::json!({ "level": "warn" })),
            Some(Level::Warn)
        );
        assert_eq!(of(&serde_json::json!({ "level": 1 })), None);
    }
}
