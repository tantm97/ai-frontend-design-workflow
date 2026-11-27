//! The records of the running servers: `.dfw/studio.json` (`dfw studio start`) and
//! `.dfw/review.json` (`dfw review start`), both written by `cli::studio`. Their keys live here,
//! below every layer that reads them (`browser::shoot`, `hooks`, `cli`).

use std::path::Path;

use serde_json::Value;

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::core::process::{is_alive, Pid};
use crate::core::util::read_json;

/// The keys of a server record: `{ pid, port, url, target?, shortcut?, startedAt }`.
pub mod field {
    /// The server's process.
    pub const PID: &str = "pid";
    pub const PORT: &str = "port";
    /// The address the server answers on; the review server's live route answers with the
    /// same key.
    pub const URL: &str = "url";
    /// The app the review proxy serves (`review.json` only).
    pub const TARGET: &str = "target";
    /// The comment-mode key combination (`review.json` only).
    pub const SHORTCUT: &str = "shortcut";
    pub const STARTED_AT: &str = "startedAt";
}

/// The address of the project's Studio when its process runs (`.dfw/studio.json`), else `None`.
pub fn running_studio(root: &Path) -> Result<Option<String>, DfwError> {
    let info = read_json(&project_paths(root).studio, Some(Value::Null))?;
    if !Pid::of(info.get(field::PID)).is_some_and(is_alive) {
        return Ok(None);
    }
    Ok(js::truthy_value(info.get(field::URL)).map(|u| js::to_string(Some(u))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    #[test]
    fn a_studio_runs_only_while_its_process_does() {
        let root = test_dir("running-studio");
        assert_eq!(running_studio(&root).unwrap(), None, "no record");
        let studio = project_paths(&root).studio;
        std::fs::create_dir_all(studio.parent().unwrap()).unwrap();
        let url = "http://127.0.0.1:4601/";
        let record = |pid: u32| serde_json::json!({ (field::PID): pid, (field::URL): url });
        std::fs::write(&studio, record(std::process::id()).to_string()).unwrap();
        assert_eq!(running_studio(&root).unwrap().as_deref(), Some(url));
        std::fs::write(&studio, record(2_147_483_646).to_string()).unwrap();
        assert_eq!(running_studio(&root).unwrap(), None, "its process is gone");
    }
}
