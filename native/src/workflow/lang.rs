//! Ports `core/lib/lang.mjs`: the communication language. Policy (user decision): ask on the
//! first /dfw run of every session; offer the last-used language as the default; English if
//! skipped. Technical artifacts are always English regardless of this setting.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{self, env_nonempty, file, global_dir};
use crate::core::util::{now_iso, read_json, write_json};

const SESSION_TTL_DAYS: u64 = 30;

/// `sessionId(explicit)`: the given id, else `DFW_SESSION_ID`, else `CLAUDE_SESSION_ID`.
pub fn session_id(explicit: Option<&str>) -> Option<String> {
    explicit
        .filter(|e| !e.is_empty())
        .map(str::to_string)
        .or_else(|| env_nonempty("DFW_SESSION_ID"))
        .or_else(|| env_nonempty("CLAUDE_SESSION_ID"))
}

/// The file of a session; every character outside `[A-Za-z0-9_.-]` becomes `_` (one per UTF-16
/// unit, as the Node expression replaces them), so an id cannot name another folder.
fn session_file(home: &Path, id: &str) -> PathBuf {
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c.to_string()
            } else {
                "_".repeat(c.len_utf16())
            }
        })
        .collect();
    home.join("sessions").join(format!("{safe}.json"))
}

fn empty() -> Value {
    Value::Object(Map::new())
}

fn last_used_in(home: &Path) -> Result<Value, DfwError> {
    let config = read_json(&home.join(file::CONFIG), Some(empty()))?;
    let last = js::truthy_value(config.get("language").and_then(|l| l.get("lastUsed")));
    Ok(last.cloned().unwrap_or_else(|| "en".into()))
}

/// `lastUsedLanguage()`: the language of the last `dfw lang set`, `en` when there was none.
// Not called by a command yet (the Node CLI has no caller either); kept as the module's API.
#[allow(
    dead_code,
    reason = "Not called by a command yet (the Node CLI has no caller either); kept as the module's API"
)]
pub fn last_used_language() -> Result<Value, DfwError> {
    last_used_in(&global_dir())
}

/// `languageStatus(explicitSession)`: `{ session, chosen, language, lastUsed }`.
pub fn language_status(explicit_session: Option<&str>) -> Result<Value, DfwError> {
    language_status_in(&global_dir(), session_id(explicit_session).as_deref())
}

fn language_status_in(home: &Path, id: Option<&str>) -> Result<Value, DfwError> {
    let last = last_used_in(home)?;
    let Some(id) = id else {
        return Ok(json!({ "session": null, "chosen": false, "language": null, "lastUsed": last }));
    };
    let session = read_json(&session_file(home, id), Some(Value::Null))?;
    let language = js::truthy_value(session.get("language")).cloned();
    Ok(json!({
        "session": id,
        "chosen": language.is_some(),
        "language": language,
        "lastUsed": last,
    }))
}

/// `setLanguage(code, explicitSession)`: remember the language for this session and as the
/// default offered to the next one. Returns `{ session, language }`.
pub fn set_language(code: Option<&str>, explicit_session: Option<&str>) -> Result<Value, DfwError> {
    set_language_in(&global_dir(), code, session_id(explicit_session).as_deref())
}

fn set_language_in(home: &Path, code: Option<&str>, id: Option<&str>) -> Result<Value, DfwError> {
    let code = code.filter(|c| !c.is_empty()).unwrap_or("en");
    let mut lang = js::trim(code).to_lowercase();
    if lang.is_empty() {
        lang = "en".into();
    }
    if let Some(id) = id {
        write_json(
            &session_file(home, id),
            &json!({ "language": lang, "setAt": now_iso() }),
        )?;
    }
    let file = home.join(paths::file::CONFIG);
    let mut config = read_json(&file, Some(empty()))?;
    if let Value::Object(map) = &mut config {
        let mut language = js::spread(js::truthy_value(map.get("language")));
        language.insert("lastUsed".into(), lang.clone().into());
        map.insert("language".into(), Value::Object(language));
    }
    write_json(&file, &config)?;
    Ok(json!({ "session": id, "language": lang }))
}

/// `pruneSessions()`: delete session files not touched for 30 days.
pub fn prune_sessions() -> Result<(), DfwError> {
    prune_sessions_in(&global_dir())
}

fn prune_sessions_in(home: &Path) -> Result<(), DfwError> {
    let dir = home.join("sessions");
    if !dir.exists() {
        return Ok(());
    }
    let ttl = Duration::from_secs(SESSION_TTL_DAYS * 86_400);
    for entry in fs::read_dir(&dir)? {
        let path = entry?.path();
        // A file another process removed meanwhile is not an error.
        let age = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|at| SystemTime::now().duration_since(at).ok());
        if age.is_some_and(|age| age > ttl) {
            let _ = fs::remove_file(&path);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    /// language: asked once per session, last used remembered (tests/unit/dfw.test.mjs; the
    /// same case through the command line is in native/tests/hot.rs)
    #[test]
    fn language_asked_once_per_session_last_used_remembered() {
        let home = test_dir("lang");
        let st = language_status_in(&home, Some("abc")).unwrap();
        assert_eq!(st["chosen"], false);
        assert_eq!(st["lastUsed"], "en");
        set_language_in(&home, Some("vi"), Some("abc")).unwrap();
        assert_eq!(
            language_status_in(&home, Some("abc")).unwrap()["chosen"],
            true
        );
        let other = language_status_in(&home, Some("def")).unwrap();
        assert_eq!(other["chosen"], false, "new session asks again");
        assert_eq!(other["lastUsed"], "vi", "offers last used as default");
    }

    #[test]
    fn codes_are_normalised_and_other_config_is_kept() {
        let home = test_dir("lang-config");
        fs::write(
            home.join(file::CONFIG),
            r#"{"marketplaceRepo":"/r","language":{"askPolicy":"session"}}"#,
        )
        .unwrap();
        let set = set_language_in(&home, Some("  VI \n"), None).unwrap();
        assert_eq!(set, json!({ "session": null, "language": "vi" }));
        assert_eq!(
            fs::read_to_string(home.join(file::CONFIG)).unwrap(),
            "{\n  \"marketplaceRepo\": \"/r\",\n  \"language\": {\n    \"askPolicy\": \"session\",\n    \"lastUsed\": \"vi\"\n  }\n}\n"
        );
        assert_eq!(
            set_language_in(&home, Some("  "), None).unwrap()["language"],
            "en"
        );
        assert_eq!(
            set_language_in(&home, None, None).unwrap()["language"],
            "en"
        );
        assert!(!home.join("sessions").exists(), "no session, no file");
        let status = language_status_in(&home, None).unwrap();
        assert_eq!(
            status,
            json!({ "session": null, "chosen": false, "language": null, "lastUsed": "en" })
        );
    }

    #[test]
    fn a_session_id_cannot_leave_the_sessions_folder() {
        let home = Path::new("/h");
        assert_eq!(
            session_file(home, "../../etc/x y"),
            Path::new("/h/sessions/.._.._etc_x_y.json")
        );
        assert_eq!(
            session_file(home, "a-b_c.1"),
            Path::new("/h/sessions/a-b_c.1.json")
        );
        // One underscore per UTF-16 unit: two for a character outside the basic plane.
        assert_eq!(session_file(home, "é😀"), Path::new("/h/sessions/___.json"));
    }

    #[test]
    fn old_sessions_are_pruned() {
        let home = test_dir("lang-prune");
        prune_sessions_in(&home).unwrap();
        set_language_in(&home, Some("en"), Some("fresh")).unwrap();
        let old = session_file(&home, "old");
        fs::write(&old, "{}").unwrap();
        let long_ago = SystemTime::now() - Duration::from_secs(31 * 86_400);
        fs::File::options()
            .write(true)
            .open(&old)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        prune_sessions_in(&home).unwrap();
        assert!(!old.exists());
        assert!(session_file(&home, "fresh").exists());
    }
}
