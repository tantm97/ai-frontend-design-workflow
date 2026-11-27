//! What dfw last wrote whole: `.dfw/written.json` maps a project-relative path
//! (`design/tokens/tokens.json`) to the SHA-256 of the bytes dfw wrote there. A command that
//! replaces such a file compares first: a file that no longer has that hash holds the user's
//! edits, and the command refuses without `--force` (`design-files.md` D7). A path without a
//! record (a project made before this record existed) is written as before, then recorded.
//!
//! [`Written::open`] holds the project's edit lock until the record is saved, so the compare,
//! the write and the record run as one against the Studio's edits.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use super::edit::edit_lock;
use crate::core::error::DfwError;
use crate::core::lock::Lock;
use crate::core::paths::rel;
use crate::core::util::{sha256, write_json_inside};

/// The messages of the record, named once.
pub mod message {
    use super::rel;

    /// A file changed since dfw wrote it, and the command would replace it whole.
    pub fn edited(file: &str) -> String {
        format!(
            "{file} changed since dfw wrote it, and this command replaces the whole file. Keep your edits (copy what you need first), or pass --force to replace it."
        )
    }

    /// `.dfw/written.json` is not an object of path to hash.
    pub fn malformed(detail: &str) -> String {
        format!(
            "{} is not an object of file path to SHA-256 ({detail}). Delete it and run the command again: dfw records the files it writes next.",
            rel::WRITTEN
        )
    }
}

/// The record of one project, open under the edit lock.
#[derive(Debug)]
pub struct Written {
    root: PathBuf,
    hashes: Map<String, Value>,
    changed: bool,
    _held: Lock,
}

/// The SHA-256 of a file, `None` when it does not exist.
fn hash_of(file: &Path) -> Result<Option<String>, DfwError> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(sha256(bytes))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

impl Written {
    /// Take the edit lock and read the record (none yet is an empty record).
    pub fn open(root: &Path) -> Result<Self, DfwError> {
        let held = edit_lock(root)?;
        let file = root.join(rel::WRITTEN);
        let hashes = match fs::read_to_string(&file) {
            Ok(text) => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Object(map)) => map,
                Ok(_) => return Err(DfwError::new(message::malformed("not an object"))),
                Err(e) => return Err(DfwError::new(message::malformed(&e.to_string()))),
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Map::new(),
            Err(e) => return Err(e.into()),
        };
        Ok(Written {
            root: root.to_path_buf(),
            hashes,
            changed: false,
            _held: held,
        })
    }

    /// The hash recorded for `file`; a value that is not a string is refused.
    fn recorded(&self, file: &str) -> Result<Option<&str>, DfwError> {
        match self.hashes.get(file) {
            None => Ok(None),
            Some(Value::String(hash)) => Ok(Some(hash)),
            Some(_) => Err(DfwError::new(message::malformed(&format!(
                "the value of {file} is not a string"
            )))),
        }
    }

    /// The file is as dfw last wrote it, or missing: replacing it loses nothing. A file without
    /// a record is not known to be dfw's.
    pub fn is_ours(&self, file: &str) -> Result<bool, DfwError> {
        let Some(hash) = hash_of(&self.root.join(file))? else {
            return Ok(true);
        };
        Ok(self.recorded(file)? == Some(hash.as_str()))
    }

    /// Refuse to replace `file` when it has a record and changed since, unless `force`. A file
    /// without a record, or missing, may be replaced.
    pub fn check(&self, file: &str, force: bool) -> Result<(), DfwError> {
        if force {
            return Ok(());
        }
        let Some(recorded) = self.recorded(file)? else {
            return Ok(());
        };
        match hash_of(&self.root.join(file))? {
            Some(now) if now != recorded => Err(DfwError::new(message::edited(file))),
            _ => Ok(()),
        }
    }

    /// Record what is in `file` now (just written by dfw).
    pub fn record(&mut self, file: &str) -> Result<(), DfwError> {
        if let Some(hash) = hash_of(&self.root.join(file))? {
            self.hashes.insert(file.to_string(), Value::String(hash));
            self.changed = true;
        }
        Ok(())
    }

    /// Write the record when it changed, then let the edit lock go.
    pub fn save(self) -> Result<(), DfwError> {
        if self.changed {
            let file = self.root.join(rel::WRITTEN);
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            write_json_inside(&self.root, &file, &Value::Object(self.hashes.clone()))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;

    /// The file the tests write, as a command would.
    const FILE: &str = rel::TOKENS_JSON;
    const DFW_TEXT: &str = "{\"a\":1}\n";
    const USER_TEXT: &str = "{\"a\":2}\n";

    fn project() -> PathBuf {
        let root = test_dir("written");
        fs::create_dir_all(root.join(rel::TOKENS)).unwrap();
        root
    }

    fn put(root: &Path, text: &str) {
        fs::write(root.join(FILE), text).unwrap();
    }

    /// dfw writes the file and records it.
    fn dfw_writes(root: &Path) {
        put(root, DFW_TEXT);
        let mut w = Written::open(root).unwrap();
        w.record(FILE).unwrap();
        w.save().unwrap();
    }

    #[test]
    fn a_file_without_a_record_may_be_replaced() {
        let root = project();
        put(&root, USER_TEXT);
        let w = Written::open(&root).unwrap();
        assert!(w.check(FILE, false).is_ok());
        assert!(!w.is_ours(FILE).unwrap());
        drop(w);
        assert!(
            !root.join(rel::WRITTEN).exists(),
            "nothing recorded, nothing written"
        );
    }

    #[test]
    fn an_unchanged_file_may_be_replaced_and_an_edited_one_needs_force() {
        let root = project();
        dfw_writes(&root);
        let record: Value =
            serde_json::from_str(&fs::read_to_string(root.join(rel::WRITTEN)).unwrap()).unwrap();
        assert_eq!(record[FILE], Value::String(sha256(DFW_TEXT)));
        let w = Written::open(&root).unwrap();
        assert!(w.check(FILE, false).is_ok());
        assert!(w.is_ours(FILE).unwrap());
        drop(w);
        put(&root, USER_TEXT);
        let w = Written::open(&root).unwrap();
        let refused = w.check(FILE, false).unwrap_err().to_string();
        assert!(
            refused.contains(FILE) && refused.contains("--force"),
            "{refused}"
        );
        assert!(!w.is_ours(FILE).unwrap());
        assert!(w.check(FILE, true).is_ok());
    }

    #[test]
    fn a_missing_file_is_no_edit_to_lose() {
        let root = project();
        dfw_writes(&root);
        fs::remove_file(root.join(FILE)).unwrap();
        let w = Written::open(&root).unwrap();
        assert!(w.check(FILE, false).is_ok());
        assert!(w.is_ours(FILE).unwrap());
    }

    #[test]
    fn a_record_of_the_wrong_shape_is_refused() {
        let root = project();
        put(&root, USER_TEXT);
        fs::create_dir_all(root.join(rel::DFW)).unwrap();
        for bad in ["[]", "{", &format!("{{\"{FILE}\": 1}}")] {
            fs::write(root.join(rel::WRITTEN), bad).unwrap();
            let err = Written::open(&root)
                .and_then(|w| w.check(FILE, false))
                .unwrap_err()
                .to_string();
            assert!(err.contains(rel::WRITTEN), "{bad}: {err}");
        }
    }
}
