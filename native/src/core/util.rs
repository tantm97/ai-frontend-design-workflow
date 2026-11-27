//! Shared helpers: JSON io, atomic and project-bound writes, temporary names and folders, time,
//! hashing, file listing, globs, config merging and argument parsing. Path helpers are in `core::paths`, `DfwError` in `core::error`.
//!
//! JSON written here is byte-identical to `JSON.stringify(value, null, 2) + '\n'` for every value
//! `serde_json` can hold, with one known exception: JavaScript moves integer-like keys ("1", "42")
//! to the front of an object, `serde_json` keeps them where they were written.
//!
//! Over 800 lines only with its tests (the ported cases of the JavaScript tests sit next to the code).

use crate::core::js::number::number as js_number;
use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::error::DfwError;
use super::paths::{env_nonempty, inside, manifest_path, plugin_root, to_posix, Place, SafeName};

/// Name of the marketplace the plugins are published in.
pub const MARKETPLACE_NAME: &str = "dfw-marketplace";

// ---------------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------------

/// `readJson(file, fallback)`. With a fallback, a missing file returns it and broken JSON is an
/// error that names the file; without one, every failure is passed on as it is.
pub fn read_json(file: &Path, fallback: Option<Value>) -> Result<Value, DfwError> {
    let text = match fs::read_to_string(file) {
        Ok(text) => text,
        Err(err) => {
            return match fallback {
                Some(value)
                    if matches!(
                        err.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotADirectory
                    ) =>
                {
                    Ok(value)
                }
                _ => Err(DfwError::internal(format!("{}: {}", file.display(), err))),
            };
        }
    };
    serde_json::from_str(&text).map_err(|err| match fallback {
        Some(_) => DfwError::internal(format!("Invalid JSON in {}: {}", file.display(), err)),
        None => DfwError::internal(err.to_string()),
    })
}

/// `writeJson(file, data)`: two-space indentation and a trailing newline, written to a temporary
/// file and renamed so a reader never sees half a file.
pub fn write_json(file: &Path, data: &Value) -> Result<(), DfwError> {
    write_atomic(file, &(to_json_pretty(data) + "\n"))
}

/// Write a project file of `root` like [`write_json`], refusing a path whose real path leaves
/// the project (see [`write_atomic_inside`]).
pub fn write_json_inside(root: &Path, file: &Path, data: &Value) -> Result<(), DfwError> {
    write_atomic_inside(root, file, to_json_pretty(data) + "\n")
}

/// A name part no other call of this process and no other process uses at the same time:
/// `<pid>.<milliseconds>.<counter>`. The counter is the only state (an atomic, `process-and-io.md`
/// P9): two threads of the Studio writing in the same millisecond still get two names.
pub fn unique_suffix() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis());
    format!(
        "{}.{ms}.{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// Write a file through a temporary file and a rename, so a reader never sees half of it.
/// The temporary name is unique per call ([`unique_suffix`]): two writers never share one.
/// Like a plain write, it writes through a symbolic link (the link stays) and keeps the
/// permissions of the file it replaces (a config readable by its owner only stays so).
/// A file of a user's project goes through [`write_atomic_inside`] instead, so a link cannot
/// send the write out of the project; this one is for files whose link the user made on
/// purpose (user-global config, often a link into a dotfiles folder).
pub fn write_atomic(file: &Path, data: impl AsRef<[u8]>) -> Result<(), DfwError> {
    let linked = fs::symlink_metadata(file).is_ok_and(|m| m.file_type().is_symlink());
    let resolved = if linked {
        crate::core::paths::canonicalize(file).ok()
    } else {
        None
    };
    let file = resolved.as_deref().unwrap_or(file);
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", unique_suffix()));
    #[allow(
        clippy::disallowed_methods,
        reason = "write_atomic is the one home of plain file writes"
    )]
    fs::write(&tmp, data)?;
    if let Ok(meta) = fs::metadata(file) {
        // Best effort: the new content matters more than its mode.
        let _ = fs::set_permissions(&tmp, meta.permissions());
    }
    if let Err(err) = fs::rename(&tmp, file) {
        let _ = fs::remove_file(&tmp);
        return Err(err.into());
    }
    Ok(())
}

/// [`write_atomic`] for a file of the project at `root`: refused when the file, or a folder on
/// its way, is a symbolic link whose real path leaves the project (also a link whose target is
/// missing). A link that stays inside is written through, as before.
pub fn write_atomic_inside(
    root: &Path,
    file: &Path,
    data: impl AsRef<[u8]>,
) -> Result<(), DfwError> {
    ensure_inside(root, file)?;
    write_atomic(file, data)
}

/// The refusal of [`write_atomic_inside`]: `file`'s real path must stay under the real `root`.
pub fn ensure_inside(root: &Path, file: &Path) -> Result<(), DfwError> {
    let real = match inside(root, file) {
        // A cycle of links ends: the write fails on its own.
        Place::Inside { .. } | Place::Unresolved => return Ok(()),
        Place::Outside(real) => real,
    };
    let shown = file.strip_prefix(root).unwrap_or(file);
    Err(DfwError::new(format!(
        "{} leads outside the project (to {}) through a symbolic link; dfw does not write there. Replace the link with a regular file, or remove it, and run the command again.",
        to_posix(&shown.to_string_lossy()),
        real.display()
    )))
}

/// The most names [`create_first_free`] tries before it gives up.
const MAX_NAME_TRIES: u32 = 10_000;

/// Write `data` to a new file `<stem>.<ext>` of `dir` in the project at `root`; when that name
/// is taken, `<stem>-2.<ext>`, `<stem>-3.<ext>` and on. Never replaces a file
/// (`contracts.md` C9). Returns the file written.
pub fn create_unique(
    root: &Path,
    dir: &Path,
    stem: &SafeName,
    ext: &str,
    data: &[u8],
) -> Result<PathBuf, DfwError> {
    // A counter after a safe name keeps it safe.
    let names = |n| match n {
        0 => Some(stem.to_string()),
        n => Some(format!("{stem}-{}", n + 1)),
    };
    create_free(root, dir, ext, names, |_| data.to_vec()).map(|(file, _)| file)
}

/// [`create_unique`] with the names chosen by the caller, for ids whose order is their name (a
/// clock, a sequence): `names(0)`, `names(1)`, ... (each with `.<ext>`) until one is free; the
/// content is `data(name)`, for a file that holds its own id. Returns the file and its name.
///
/// The file is written whole under a temporary name and linked to its name, which fails when the
/// name is taken, so a reader never sees half a file and nothing is replaced. Where the volume
/// has no links, the file is made with `create_new` and written in place.
pub fn create_first_free(
    root: &Path,
    dir: &Path,
    ext: &str,
    mut names: impl FnMut(u32) -> Option<SafeName>,
    data: impl Fn(&str) -> Vec<u8>,
) -> Result<(PathBuf, String), DfwError> {
    create_free(root, dir, ext, |n| names(n).map(|s| s.to_string()), data)
}

fn create_free(
    root: &Path,
    dir: &Path,
    ext: &str,
    mut names: impl FnMut(u32) -> Option<String>,
    data: impl Fn(&str) -> Vec<u8>,
) -> Result<(PathBuf, String), DfwError> {
    ensure_inside(root, dir)?;
    fs::create_dir_all(dir)?;
    for n in 0..MAX_NAME_TRIES {
        let Some(name) = names(n) else { break };
        let file = dir.join(format!("{name}.{ext}"));
        if create_new_file(&file, &data(&name))? {
            return Ok((file, name));
        }
    }
    Err(DfwError::internal(format!(
        "No free file name in {}.",
        dir.display()
    )))
}

/// Make `file` with `data` unless it exists (also as a link): `false` when it does.
fn create_new_file(file: &Path, data: &[u8]) -> Result<bool, DfwError> {
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", unique_suffix()));
    let tmp = PathBuf::from(tmp);
    write_atomic(&tmp, data)?;
    let linked = fs::hard_link(&tmp, file);
    let _ = fs::remove_file(&tmp);
    match linked {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
        // No hard links on this volume.
        Err(_) => match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(file)
        {
            Ok(mut f) => {
                f.write_all(data)?;
                Ok(true)
            }
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(err) => Err(err.into()),
        },
    }
}

/// `fs.mkdtempSync(path.join(base, prefix))`: a new directory only its owner can enter, with a
/// random six-letter suffix.
pub fn make_temp_dir(base: &Path, prefix: &str) -> io::Result<std::path::PathBuf> {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    for _ in 0..100 {
        // RandomState is keyed per process by the standard library; time and a counter make
        // every call different.
        let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
        hasher.write_u64(NEXT.fetch_add(1, Ordering::Relaxed));
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        hasher.write_u128(nanos);
        let mut n = hasher.finish();
        let mut name = String::from(prefix);
        for _ in 0..6 {
            name.push(char::from(DIGITS[(n % 36) as usize]));
            n /= 36;
        }
        let dir = base.join(name);
        match private_dir_builder().create(&dir) {
            Ok(()) => return Ok(dir),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "no free temporary directory name",
    ))
}

/// A directory only its owner can enter, as mkdtemp makes it.
#[cfg(unix)]
fn private_dir_builder() -> fs::DirBuilder {
    let mut builder = fs::DirBuilder::new();
    std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
    builder
}

#[cfg(not(unix))]
fn private_dir_builder() -> fs::DirBuilder {
    fs::DirBuilder::new()
}

/// `appendJsonl(file, entry)`: one compact JSON value per line.
pub fn append_jsonl(file: &Path, entry: &Value) -> Result<(), DfwError> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut f = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    f.write_all((to_json_compact(entry) + "\n").as_bytes())?;
    Ok(())
}

/// [`append_jsonl`] for a file of the project at `root`, refused like [`write_atomic_inside`].
pub fn append_jsonl_inside(root: &Path, file: &Path, entry: &Value) -> Result<(), DfwError> {
    ensure_inside(root, file)?;
    append_jsonl(file, entry)
}

/// `readJsonl(file)`: every non-blank line parsed; a missing file is an empty list.
pub fn read_jsonl(file: &Path) -> Result<Vec<Value>, DfwError> {
    if !file.exists() {
        return Ok(Vec::new());
    }
    fs::read_to_string(file)?
        .split('\n')
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).map_err(DfwError::from))
        .collect()
}

/// `JSON.stringify(value, null, 2)`.
pub fn to_json_pretty(value: &Value) -> String {
    serialize(value, JsFormatter(PrettyFormatter::with_indent(b"  ")))
}

/// `JSON.stringify(value)`.
pub fn to_json_compact(value: &Value) -> String {
    serialize(value, JsFormatter(CompactFormatter))
}

fn serialize<F: Formatter>(value: &Value, formatter: F) -> String {
    let mut buf = Vec::new();
    // Writing a `Value` into a `Vec` cannot fail: no io, and every map key is a string.
    let _ = value.serialize(&mut Serializer::with_formatter(&mut buf, formatter));
    String::from_utf8(buf).unwrap_or_default()
}

/// Largest integer JavaScript holds exactly; `JSON.parse` rounds anything beyond it to a double.
const MAX_SAFE_INTEGER: u64 = 1 << 53;

/// Lays values out like the wrapped formatter and prints numbers the way JavaScript does
/// (`1.0` is `1`, `1e21` is `1e+21`), so a file read and written back does not change.
struct JsFormatter<F>(F);

impl<F: Formatter> Formatter for JsFormatter<F> {
    fn write_f64<W: ?Sized + Write>(&mut self, w: &mut W, v: f64) -> io::Result<()> {
        w.write_all(js_number(v).as_bytes())
    }
    fn write_u64<W: ?Sized + Write>(&mut self, w: &mut W, v: u64) -> io::Result<()> {
        let text = if v > MAX_SAFE_INTEGER {
            js_number(v as f64)
        } else {
            v.to_string()
        };
        w.write_all(text.as_bytes())
    }
    fn write_i64<W: ?Sized + Write>(&mut self, w: &mut W, v: i64) -> io::Result<()> {
        let text = if v.unsigned_abs() > MAX_SAFE_INTEGER {
            js_number(v as f64)
        } else {
            v.to_string()
        };
        w.write_all(text.as_bytes())
    }
    fn begin_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_array(w)
    }
    fn end_array<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array(w)
    }
    fn begin_array_value<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_array_value(w, first)
    }
    fn end_array_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_array_value(w)
    }
    fn begin_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object(w)
    }
    fn end_object<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object(w)
    }
    fn begin_object_key<W: ?Sized + Write>(&mut self, w: &mut W, first: bool) -> io::Result<()> {
        self.0.begin_object_key(w, first)
    }
    fn begin_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.begin_object_value(w)
    }
    fn end_object_value<W: ?Sized + Write>(&mut self, w: &mut W) -> io::Result<()> {
        self.0.end_object_value(w)
    }
}

/// `RESERVED_KEYS`: names that reach an object's prototype in JavaScript; never accepted as a key
/// or a flag from input.
pub const RESERVED_KEYS: [&str; 3] = [PROTO, "constructor", "prototype"];
/// The key that is an object's prototype in JavaScript.
pub const PROTO: &str = "__proto__";

/// `deepMerge(base, over)` for config layering: objects merge, arrays and scalars replace.
/// Reserved keys are dropped. (The JavaScript `undefined` case, "no override", is the caller not
/// calling this.)
pub fn deep_merge(base: Value, over: Value) -> Value {
    let Value::Object(over) = over else {
        return over;
    };
    let mut out = match base {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for (k, v) in over {
        if RESERVED_KEYS.contains(&k.as_str()) {
            continue;
        }
        // A missing key still goes through the merge, so its reserved keys are dropped too.
        let slot = out.get_mut(&k).map_or(Value::Null, Value::take);
        let merged = deep_merge(slot, v);
        out.insert(k, merged);
    }
    Value::Object(out)
}

/// `getPath(obj, "a.b.c")`: `None` where JavaScript gives `undefined`. Own keys of objects and
/// indexes of lists only, never a prototype member (`toString`, `length`).
pub fn get_path<'a>(obj: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted.split('.').try_fold(obj, |o, k| match o {
        Value::Object(map) => map.get(k),
        // `Object.hasOwn(list, k)`: only the canonical form of an index ("1", not "01" or "+1").
        Value::Array(list)
            if k.bytes().all(|b| b.is_ascii_digit()) && (k == "0" || !k.starts_with('0')) =>
        {
            k.parse::<usize>().ok().and_then(|i| list.get(i))
        }
        _ => None,
    })
}

/// `setPath(obj, "a.b.c", value)`: intermediate values that are not objects are replaced by
/// objects. A reserved name, or a key that goes through a list, is the user's error.
pub fn set_path(obj: &mut Value, dotted: &str, value: Value) -> Result<(), DfwError> {
    let keys: Vec<&str> = dotted.split('.').collect();
    if let Some(reserved) = keys.iter().find(|k| RESERVED_KEYS.contains(k)) {
        return Err(DfwError::new(format!(
            "Invalid key \"{dotted}\": \"{reserved}\" is a reserved name."
        )));
    }
    let mut value = Some(value);
    let mut o = obj;
    for (i, k) in keys.iter().enumerate() {
        // A property set on a list is lost when the file is written, and an index would replace an item.
        if o.is_array() {
            return Err(DfwError::new(format!(
                "Cannot set \"{dotted}\": \"{}\" is a list; set the whole list instead.",
                keys[..i].join(".")
            )));
        }
        if !o.is_object() {
            *o = Value::Object(Map::new());
        }
        let Value::Object(map) = o else {
            return Ok(()); // made an object just above
        };
        if i + 1 == keys.len() {
            map.insert((*k).to_string(), value.take().unwrap_or(Value::Null));
            return Ok(());
        }
        let slot = map.entry(*k).or_insert(Value::Null);
        if !slot.is_object() && !slot.is_array() {
            *slot = Value::Object(Map::new());
        }
        o = slot;
    }
    Ok(())
}

/// `parseValue(v)`: a CLI value is JSON when it parses, else the raw string.
pub fn parse_value(v: &str) -> Value {
    serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_string()))
}

// ---------------------------------------------------------------------------------------------
// Time and hashing
// ---------------------------------------------------------------------------------------------

/// `nowIso()`: the current time as `Date.prototype.toISOString` prints it. `DFW_NOW` replaces it
/// (the golden tests fix the clock so written timestamps are the recorded ones).
pub fn now_iso() -> String {
    if let Some(fixed) = env_nonempty("DFW_NOW") {
        return fixed;
    }
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    iso_from_millis(ms)
}

/// `new Date(ms).toISOString()` for years 0000 to 9999.
pub fn iso_from_millis(ms: i64) -> String {
    let days = ms.div_euclid(86_400_000);
    let rem = ms.rem_euclid(86_400_000);
    // Days since 1970-01-01 to a civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        rem / 3_600_000,
        rem / 60_000 % 60,
        rem / 1000 % 60,
        rem % 1000
    )
}

/// `sha256(buf)`: lowercase hex digest.
pub fn sha256(data: impl AsRef<[u8]>) -> String {
    Sha256::digest(data.as_ref())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------------------------

/// `listFiles(dir, { exclude })`: regular files under `dir`, as relative paths with `/`, sorted.
/// An entry of `exclude` drops that path and everything below it. Symlinks are not followed.
pub fn list_files(dir: &Path, exclude: &[&str]) -> Result<Vec<String>, DfwError> {
    let mut out = Vec::new();
    if dir.exists() {
        walk(dir, "", exclude, &mut out)?;
    }
    // JavaScript's default sort compares UTF-16 code units.
    out.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(out)
}

fn walk(abs: &Path, rel: &str, exclude: &[&str], out: &mut Vec<String>) -> Result<(), DfwError> {
    for entry in fs::read_dir(abs)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let r = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        if exclude
            .iter()
            .any(|e| r == *e || r.starts_with(&format!("{e}/")))
        {
            continue;
        }
        let kind = entry.file_type()?;
        if kind.is_dir() {
            walk(&entry.path(), &r, exclude, out)?;
        } else if kind.is_file() {
            out.push(r);
        }
    }
    Ok(())
}

/// `copyDir(src, dest, { exclude })`: copies the files [`list_files`] returns, with their mode.
pub fn copy_dir(src: &Path, dest: &Path, exclude: &[&str]) -> Result<(), DfwError> {
    for rel in list_files(src, exclude)? {
        let to = dest.join(&rel);
        if let Some(dir) = to.parent() {
            fs::create_dir_all(dir)?;
        }
        // fs::copy carries the permission bits, as copyFileSync + chmodSync did.
        fs::copy(src.join(&rel), &to)?;
    }
    Ok(())
}

/// `pluginManifest(dir)`: the plugin's manifest (name, version...), whichever host format it
/// uses; an empty object when there is none. `None` means the running plugin.
pub fn plugin_manifest(dir: Option<&Path>) -> Result<Value, DfwError> {
    let empty = || Value::Object(Map::new());
    let file = match dir {
        Some(dir) => manifest_path(dir),
        None => plugin_root().and_then(|root| manifest_path(&root)),
    };
    match file {
        Some(file) => read_json(&file, Some(empty())),
        None => Ok(empty()),
    }
}

// ---------------------------------------------------------------------------------------------
// Globs
// ---------------------------------------------------------------------------------------------

/// `globToRegExp(glob)`: minimal glob matcher supporting `**`, `*` and `?` on posix-style paths.
pub fn glob_to_regex(glob: &str) -> Result<Regex, DfwError> {
    // What `.` matches in a JavaScript expression without the `s` flag.
    const DOT: &str = r"[^\n\r\u{2028}\u{2029}]";
    let chars: Vec<char> = glob.chars().collect();
    let mut re = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '*' {
            if chars.get(i + 1) == Some(&'*') {
                i += 1;
                if chars.get(i + 1) == Some(&'/') {
                    i += 1;
                    re.push_str(&format!("(?:{DOT}*/)?"));
                } else {
                    re.push_str(&format!("{DOT}*"));
                }
            } else {
                re.push_str("[^/]*");
            }
        } else if c == '?' {
            re.push_str("[^/]");
        } else if ".+^${}()|[]\\".contains(c) {
            re.push('\\');
            re.push(c);
        } else {
            re.push(c);
        }
        i += 1;
    }
    Regex::new(&format!("^{re}$"))
        .map_err(|err| DfwError::internal(format!("Invalid glob \"{glob}\": {err}")))
}

/// `matchesAny(rel, globs)`.
pub fn matches_any<S: AsRef<str>>(rel: &str, globs: &[S]) -> bool {
    globs.iter().any(|g| {
        glob_to_regex(g.as_ref())
            .map(|re| re.is_match(rel))
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------------------------------------
// Command line
// ---------------------------------------------------------------------------------------------

/// The value of a `--flag`: `true` when it stood alone, else the text given to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgValue {
    Flag,
    Text(String),
}

/// Result of [`parse_args`]: positionals (`_` in Node) and flags.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Args {
    pub positionals: Vec<String>,
    flags: HashMap<String, ArgValue>,
}

impl Args {
    /// The flag as parsed, `None` when it was not given (`undefined` in Node).
    pub fn get(&self, key: &str) -> Option<&ArgValue> {
        self.flags.get(key)
    }

    /// The flag's text; `None` when it is missing or was given without a value.
    pub fn text(&self, key: &str) -> Option<&str> {
        match self.flags.get(key) {
            Some(ArgValue::Text(t)) => Some(t),
            _ => None,
        }
    }

    /// JavaScript truthiness of the flag: given alone, or with a non-empty value.
    pub fn truthy(&self, key: &str) -> bool {
        match self.flags.get(key) {
            Some(ArgValue::Flag) => true,
            Some(ArgValue::Text(t)) => !t.is_empty(),
            None => false,
        }
    }

    /// Positional argument `i` (`a._[i]`).
    pub fn pos(&self, i: usize) -> Option<&str> {
        self.positionals.get(i).map(String::as_str)
    }
}

/// `parseArgs(argv)`: `--k v`, `--k=v`, `--k` alone (true; also when the next argument starts
/// with `--`), everything else positional. A later flag replaces an earlier one of the same name.
pub fn parse_args<S: AsRef<str>>(argv: &[S]) -> Args {
    let mut args = Args::default();
    let mut i = 0;
    while i < argv.len() {
        let a = argv[i].as_ref();
        if let Some(body) = a.strip_prefix("--") {
            let next = argv
                .get(i + 1)
                .map(AsRef::as_ref)
                .filter(|n| !n.starts_with("--"));
            let (key, value) = match (body.split_once('='), next) {
                (Some((k, inline)), _) => (k, ArgValue::Text(inline.to_string())),
                (None, Some(n)) => {
                    i += 1;
                    (body, ArgValue::Text(n.to_string()))
                }
                (None, None) => (body, ArgValue::Flag),
            };
            // "--_" would replace the positional list and a reserved key the prototype: dropped
            // with their value.
            if key != "_" && !RESERVED_KEYS.contains(&key) {
                args.flags.insert(key.to_string(), value);
            }
        } else {
            args.positionals.push(a.to_string());
        }
        i += 1;
    }
    args
}

/// `VALUE_FLAGS` of cli.mjs: flags that take a value. Given without one they would read as `true`:
/// a level 1 decision, a 1 second timeout, an approval note "true", a TypeError in path.resolve.
pub const VALUE_FLAGS: [&str; 57] = [
    "root",
    "port",
    "host",
    "scope",
    "source",
    "mode",
    "product",
    "note",
    "reason",
    "level",
    "topic",
    "overrides",
    "text",
    "timeout",
    "message",
    "m",
    "evidence",
    "by",
    "status",
    "url",
    "shortcut",
    "canvas",
    "bp",
    "out",
    "selector",
    "screen",
    "page",
    "variant",
    "state",
    "theme",
    "session",
    "file",
    "format",
    "from",
    "brief",
    "limit",
    "style",
    "preset",
    "fonts",
    "base",
    "ratio",
    "display-ratio",
    "brand",
    "chroma",
    "neutral-hue",
    "tint",
    "signature",
    "signature-chroma",
    "harmony",
    "accent",
    "names",
    "threshold",
    "max",
    "marketplace-repo",
    "after",
    "title",
    "route",
];

/// `requireFlagValues(a)`: the first flag of [`VALUE_FLAGS`] given without a value is an error.
pub fn require_flag_values(args: &Args) -> Result<(), DfwError> {
    let bare = VALUE_FLAGS
        .iter()
        .find(|k| args.get(k) == Some(&ArgValue::Flag));
    match bare {
        Some(k) => Err(DfwError::new(format!("--{k} needs a value."))),
        None => Ok(()),
    }
}

/// `readStdin()`: all of stdin as text, or an empty string when stdin is a terminal.
pub fn read_stdin() -> Result<String, DfwError> {
    let mut stdin = io::stdin();
    if stdin.is_terminal() {
        return Ok(String::new());
    }
    let mut bytes = Vec::new();
    stdin.read_to_end(&mut bytes)?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `console.log(text)`: the text and a newline on stdout. A closed pipe (`dfw help | head`) is
/// not an error.
pub fn out(text: &str) {
    let mut stdout = io::stdout().lock();
    let _ = stdout
        .write_all(text.as_bytes())
        .and_then(|()| stdout.write_all(b"\n"))
        .and_then(|()| stdout.flush());
}

/// Tests only: a local HTTP server where `routes` maps a path to (status, body). Returns the base
/// URL and a counter of the requests served. Nothing ever leaves the machine (the binary
/// download and the browser download are tested against it).
#[cfg(test)]
pub fn serve_routes(
    routes: std::collections::HashMap<String, (u16, Vec<u8>)>,
) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    let server = tiny_http::Server::http((crate::core::http::LOOPBACK, 0)).unwrap();
    let base = format!("http://{}", server.server_addr());
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&hits);
    std::thread::spawn(move || {
        for request in server.incoming_requests() {
            counter.fetch_add(1, Ordering::SeqCst);
            let (status, body) = routes
                .get(request.url())
                .cloned()
                .unwrap_or((404, b"not found".to_vec()));
            let _ = request.respond(tiny_http::Response::from_data(body).with_status_code(status));
        }
    });
    (base, hits)
}

/// A fresh directory for one test, under the system temporary directory.
#[cfg(test)]
pub fn test_dir(label: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicU32, Ordering};
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "dfw-native-{label}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    // macOS reaches its temporary directory through a symlink; tests compare real paths.
    crate::core::paths::canonicalize(&dir).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{APP_DIR, HTML_FILE, OTHER_HTML_FILE};
    use crate::core::paths::rel::all_under;
    use crate::core::paths::{file, rel, CLAUDE_MANIFEST, HOST_MANIFEST, MANIFEST_VERSION};
    use serde_json::json;

    /// glob matching (tests/unit/dfw.test.mjs)
    #[test]
    fn glob_matching() {
        let design = all_under(rel::DESIGN);
        assert!(glob_to_regex(&design)
            .unwrap()
            .is_match(&rel::screen(HTML_FILE)));
        assert!(glob_to_regex(&all_under(rel::APPROVED))
            .unwrap()
            .is_match(&rel::approved("v1/x.html")));
        assert!(!glob_to_regex(&design)
            .unwrap()
            .is_match(&rel::join(APP_DIR, &rel::design(OTHER_HTML_FILE))));
        assert!(glob_to_regex(rel::PRODUCT_DOC)
            .unwrap()
            .is_match(rel::PRODUCT_DOC));
    }

    #[test]
    fn glob_translation_matches_the_node_function() {
        // Expected sources printed by globToRegExp(...).source, with `.` spelled out.
        let dot = r"[^\n\r\u{2028}\u{2029}]";
        // `?` stands for one character, never a `/`.
        const ONE_CHAR: &str = "a?c";
        let app = all_under(APP_DIR);
        assert_eq!(
            glob_to_regex("**/*.test.?s").unwrap().as_str(),
            format!(r"^(?:{dot}*/)?[^/]*\.test\.[^/]s$")
        );
        assert_eq!(
            glob_to_regex(&app).unwrap().as_str(),
            format!(r"^{APP_DIR}/{dot}*$")
        );
        assert_eq!(
            glob_to_regex("a+(b)[c]{d}|^$\\").unwrap().as_str(),
            r"^a\+\(b\)\[c\]\{d\}\|\^\$\\$"
        );
        assert!(!glob_to_regex(rel::PRODUCT_DOC)
            .unwrap()
            .is_match("PRODUCTxmd"));
        assert!(!glob_to_regex("*.html").unwrap().is_match("a/b.html"));
        assert!(glob_to_regex("**/b.html").unwrap().is_match("b.html"));
        assert!(glob_to_regex(ONE_CHAR).unwrap().is_match("abc"));
        assert!(!glob_to_regex(ONE_CHAR).unwrap().is_match("a/c"));
        assert!(matches_any(
            &rel::design(OTHER_HTML_FILE),
            &[&app, &all_under(rel::DESIGN)]
        ));
        assert!(!matches_any(&rel::design(OTHER_HTML_FILE), &[] as &[&str]));
    }

    /// j: names that are prototype keys are not ids (tests/unit/node-stable.test.mjs; the util part)
    #[test]
    fn reserved_names_are_not_keys_or_flags() {
        let parsed = |argv: &[&str]| {
            let a = parse_args(argv);
            let mut keys: Vec<&String> = a.flags.keys().collect();
            keys.sort();
            (
                a.positionals.clone(),
                keys.into_iter().cloned().collect::<Vec<_>>(),
            )
        };
        assert_eq!(
            parsed(&["a", "--_=x", "b"]),
            (vec!["a".into(), "b".into()], vec![])
        );
        assert_eq!(
            parsed(&["a", "--_", "x", "b"]),
            (vec!["a".into(), "b".into()], vec![])
        );
        assert_eq!(
            parsed(&[
                "--__proto__",
                "x",
                "--constructor",
                "--prototype=1",
                "y",
                "--json"
            ]),
            (vec!["y".into()], vec!["json".into()])
        );
        let merged = deep_merge(
            json!({"studio": {"port": 4600}}),
            json!({"__proto__": {"polluted": "yes"}, "constructor": {"x": 1}, "studio": {"__proto__": {"port": 1}}, "new": {"prototype": 1, "k": 2}}),
        );
        assert_eq!(
            to_json_compact(&merged),
            r#"{"studio":{"port":4600},"new":{"k":2}}"#
        );
        const BREAKPOINT: &str = "desktop";
        let d = json!({"studio": {"breakpoints": [{"id": BREAKPOINT}]}, "s": "text"});
        for name in ["constructor", "toString", "__proto__", "hasOwnProperty"] {
            assert_eq!(get_path(&d, name), None);
            assert_eq!(get_path(&d, &format!("studio.{name}")), None);
        }
        assert_eq!(
            get_path(&d, "studio.breakpoints.0.id"),
            Some(&json!(BREAKPOINT))
        );
        for key in [
            "studio.breakpoints.length",
            "studio.breakpoints.00",
            "studio.breakpoints.+0",
            "s.length",
        ] {
            assert_eq!(get_path(&d, key), None, "{key}");
        }
        let mut v = json!({});
        let err = set_path(&mut v, "a.constructor", json!(1)).unwrap_err();
        assert_eq!(
            err.report(),
            "Invalid key \"a.constructor\": \"constructor\" is a reserved name."
        );
        assert_eq!(v, json!({}), "nothing set");
    }

    /// b, m: a flag that takes a value is refused without one (tests/unit/node-stable.test.mjs)
    #[test]
    fn a_flag_that_takes_a_value_is_refused_without_one() {
        const ROOT: &str = "--root";
        for flag in [
            "level",
            "timeout",
            "root",
            "port",
            "note",
            "reason",
            "message",
            "session",
            "status",
            "url",
            "host",
            "marketplace-repo",
        ] {
            let a = parse_args(&["x", &format!("--{flag}")]);
            assert_eq!(
                require_flag_values(&a).unwrap_err().report(),
                format!("--{flag} needs a value.")
            );
        }
        assert!(require_flag_values(&parse_args(&["x", "--json", ROOT, "/p"])).is_ok());
        assert!(require_flag_values(&parse_args(&["comments", "list", "--status=open"])).is_ok());
        // The first one in the Node list wins, whatever the order typed.
        let a = parse_args(&["x", "--level", ROOT]);
        assert_eq!(
            require_flag_values(&a).unwrap_err().message(),
            format!("{ROOT} needs a value.")
        );
    }

    #[test]
    fn parse_args_follows_the_node_rules() {
        const COMMAND: &str = "comments";
        const SUBCOMMAND: &str = "show";
        const STATUS: &str = "open";
        /// A flag given an empty value (`--empty=`).
        const EMPTY: &str = "empty";
        /// A flag given twice.
        const TWICE: &str = "--k";
        let a = parse_args(&[
            COMMAND,
            "--status",
            STATUS,
            "--json",
            "--message=a=b",
            SUBCOMMAND,
            "--empty=",
            "--force",
            "--x",
            "-y",
        ]);
        assert_eq!(a.positionals, [COMMAND, SUBCOMMAND]);
        assert_eq!(a.text("status"), Some(STATUS));
        assert_eq!(a.get("json"), Some(&ArgValue::Flag));
        assert_eq!(a.text("message"), Some("a=b"));
        assert_eq!(a.get(EMPTY), Some(&ArgValue::Text(String::new())));
        assert!(!a.truthy(EMPTY));
        assert!(a.truthy("force"), "a flag followed by another flag is true");
        assert_eq!(a.text("x"), Some("-y"), "a single dash is a value");
        assert_eq!(a.get("missing"), None);
        assert_eq!(parse_args(&[TWICE, "1", TWICE, "2"]).text("k"), Some("2"));
        assert_eq!(parse_args(&["--help"]).get("help"), Some(&ArgValue::Flag));
    }

    #[test]
    fn json_is_written_like_json_stringify() {
        let v: Value = serde_json::from_str(r#"{"z":1,"a":{"n":1.0,"f":0.5,"big":1e21,"small":1e-7,"neg":-0.0,"list":[],"obj":{}},"s":"é\n\u0001\"","arr":[1,[2,3],{"k":null}],"t":true}"#).unwrap();
        let expected = "{\n  \"z\": 1,\n  \"a\": {\n    \"n\": 1,\n    \"f\": 0.5,\n    \"big\": 1e+21,\n    \"small\": 1e-7,\n    \"neg\": 0,\n    \"list\": [],\n    \"obj\": {}\n  },\n  \"s\": \"é\\n\\u0001\\\"\",\n  \"arr\": [\n    1,\n    [\n      2,\n      3\n    ],\n    {\n      \"k\": null\n    }\n  ],\n  \"t\": true\n}";
        assert_eq!(to_json_pretty(&v), expected);
        assert_eq!(
            to_json_compact(&json!({"b": [1, 2.50], "a": "x"})),
            r#"{"b":[1,2.5],"a":"x"}"#
        );
    }

    #[test]
    fn numbers_print_like_javascript() {
        /// 2^53 + 1 rounds to 2^53.
        const ROUNDED: &str = "9007199254740992";
        for (v, s) in [
            (0.0, "0"),
            (1.0, "1"),
            (-1.5, "-1.5"),
            (100.0, "100"),
            (1e20, "100000000000000000000"),
            (1e21, "1e+21"),
            (1.5e22, "1.5e+22"),
            (0.000001, "0.000001"),
            (0.0000001, "1e-7"),
            (1.25e-9, "1.25e-9"),
            (0.1 + 0.2, "0.30000000000000004"),
            (123456.789, "123456.789"),
            (9007199254740993.0, ROUNDED),
        ] {
            assert_eq!(js_number(v), s);
        }
        assert_eq!(to_json_compact(&json!(9007199254740993u64)), ROUNDED);
    }

    /// A user's file replaced through a rename keeps its mode, and a link stays a link.
    #[cfg(unix)]
    #[test]
    fn write_atomic_keeps_the_mode_and_writes_through_a_link() {
        use std::os::unix::fs::PermissionsExt;
        let dir = test_dir("atomic-mode");
        let file = dir.join(file::CONFIG);
        fs::write(&file, "{}").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
        const CONTENT: &str = "{\"a\":1}";
        write_atomic(&file, CONTENT).unwrap();
        assert_eq!(fs::read_to_string(&file).unwrap(), CONTENT);
        let mode = fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        let link = dir.join("link.json");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        write_atomic(&link, b"[]").unwrap();
        assert!(fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(fs::read_to_string(&file).unwrap(), "[]");
    }

    /// `contracts.md` C9: two files named in the same second get two names; the first is kept,
    /// and nothing is left behind but the files.
    #[test]
    fn create_unique_never_replaces_a_file() {
        const STEM: &str = "20261116T100000Z";
        const EXT: &str = "json";
        let root = test_dir("create-unique");
        let dir = root.join(file::DFW).join(APP_DIR);
        let stem = SafeName::matching(&Regex::new("^[0-9TZ]+$").unwrap(), STEM).unwrap();
        let first = create_unique(&root, &dir, &stem, EXT, b"1").unwrap();
        let second = create_unique(&root, &dir, &stem, EXT, b"2").unwrap();
        let third = create_unique(&root, &dir, &stem, EXT, b"3").unwrap();
        assert_eq!(first, dir.join(format!("{STEM}.{EXT}")));
        assert_eq!(second, dir.join(format!("{STEM}-2.{EXT}")));
        assert_eq!(third, dir.join(format!("{STEM}-3.{EXT}")));
        assert_eq!(
            fs::read_to_string(&first).unwrap(),
            "1",
            "the first is kept"
        );
        assert_eq!(fs::read_to_string(&second).unwrap(), "2");
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            3,
            "no temporary file left"
        );
        // Names chosen by the caller: the first free one, its content made for its name.
        let names = |n: u32| {
            SafeName::matching(
                &Regex::new("^[0-9TZ-]+$").unwrap(),
                &format!("{STEM}-{}", n + 2),
            )
        };
        let (file, name) =
            create_first_free(&root, &dir, EXT, names, |name| name.as_bytes().to_vec()).unwrap();
        assert_eq!(name, format!("{STEM}-4"));
        assert_eq!(fs::read_to_string(file).unwrap(), name);
        fs::remove_dir_all(&root).unwrap();
    }

    /// The folder of a new file is checked like any project write.
    #[cfg(unix)]
    #[test]
    fn create_unique_refuses_a_folder_linked_out_of_the_project() {
        let tmp = test_dir("create-unique-link");
        let (root, elsewhere) = (tmp.join("project"), tmp.join("elsewhere"));
        fs::create_dir_all(root.join(file::DFW)).unwrap();
        fs::create_dir_all(&elsewhere).unwrap();
        let dir = root.join(file::DFW).join(APP_DIR);
        std::os::unix::fs::symlink(&elsewhere, &dir).unwrap();
        let stem = SafeName::matching(&Regex::new("^x$").unwrap(), "x").unwrap();
        assert!(create_unique(&root, &dir, &stem, "json", b"1").is_err());
        assert_eq!(fs::read_dir(&elsewhere).unwrap().count(), 0);
        fs::remove_dir_all(&tmp).unwrap();
    }

    /// A project file that is a link out of the project is refused; a link inside is written
    /// through (review CO-3: `.dfw/config.local.json -> ../../victim/settings.json`).
    #[cfg(unix)]
    #[test]
    fn write_atomic_inside_refuses_a_link_out_of_the_project() {
        /// The folder next to the project a link must not write into.
        const VICTIM: &str = "victim";
        const KEPT: &str = "{\"keep\":1}";
        /// A file of [`VICTIM`] that must not be created.
        const NEW: &str = "new.json";
        /// A file at the project root, the target of [`NEAR`].
        const REAL: &str = "real.json";
        /// A link in `.dfw` to [`REAL`].
        const NEAR: &str = "near.json";
        let dir = test_dir("atomic-inside");
        let root = dir.join("project");
        fs::create_dir_all(root.join(rel::DFW)).unwrap();
        let outside = dir.join(VICTIM);
        fs::create_dir_all(&outside).unwrap();
        /// The user-global file the link points at.
        const SETTINGS: &str = "settings.json";
        let victim = outside.join(SETTINGS);
        fs::write(&victim, KEPT).unwrap();
        let link = root.join(rel::CONFIG_LOCAL);
        std::os::unix::fs::symlink(Path::new("../..").join(VICTIM).join(SETTINGS), &link).unwrap();
        let err = write_json_inside(&root, &link, &json!({})).unwrap_err();
        assert!(
            err.message()
                .starts_with(&format!("{} leads outside the project", rel::CONFIG_LOCAL)),
            "{err}"
        );
        assert!(matches!(err, DfwError::User { .. }));
        assert_eq!(fs::read_to_string(&victim).unwrap(), KEPT);
        // A dangling link out of the project and a linked folder are refused too.
        let gone = root.join(rel::DFW).join("gone.json");
        std::os::unix::fs::symlink(outside.join(NEW), &gone).unwrap();
        assert!(write_atomic_inside(&root, &gone, "x").is_err());
        assert!(!outside.join(NEW).exists());
        std::os::unix::fs::symlink(&outside, root.join(rel::DESIGN)).unwrap();
        assert!(write_atomic_inside(&root, &root.join(rel::design(file::HEAD)), "x").is_err());
        // Inside the project: written through the link, a new file is created.
        fs::write(root.join(REAL), "1").unwrap();
        let near = root.join(rel::DFW).join(NEAR);
        std::os::unix::fs::symlink(Path::new("..").join(REAL), &near).unwrap();
        write_atomic_inside(&root, &near, "2").unwrap();
        assert_eq!(fs::read_to_string(root.join(REAL)).unwrap(), "2");
        write_atomic_inside(&root, &root.join(rel::DFW).join("new").join("a.json"), "3").unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }

    /// Threads of one process writing one file at the same time never share a temporary name
    /// (review CO-1: `<pid>.<ms>.tmp` was the same for two Studio threads).
    #[test]
    fn concurrent_atomic_writes_use_their_own_temporary_files() {
        let dir = test_dir("atomic-threads");
        let file = dir.join("a.txt");
        std::thread::scope(|s| {
            for i in 0..16 {
                let file = &file;
                s.spawn(move || {
                    for j in 0..20 {
                        write_atomic(file, format!("{i}-{j}")).unwrap();
                    }
                });
            }
        });
        assert_eq!(
            fs::read_dir(&dir).unwrap().count(),
            1,
            "no temporary file is left"
        );
        let names: std::collections::HashSet<String> = (0..1000).map(|_| unique_suffix()).collect();
        assert_eq!(names.len(), 1000);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn make_temp_dir_makes_a_new_private_directory_each_call() {
        const PREFIX: &str = "dfw-x-";
        let base = test_dir("tempdir");
        let a = make_temp_dir(&base, PREFIX).unwrap();
        let b = make_temp_dir(&base, PREFIX).unwrap();
        assert_ne!(a, b);
        assert!(a.is_dir() && a.file_name().unwrap().to_string_lossy().starts_with(PREFIX));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&a).unwrap().permissions().mode() & 0o777,
                0o700
            );
        }
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn write_json_is_atomic_and_read_json_falls_back() {
        let dir = test_dir("json");
        let deep = dir.join("deep");
        let file = deep.join(file::STATE);
        assert_eq!(read_json(&file, Some(Value::Null)).unwrap(), Value::Null);
        assert!(
            read_json(&file, None).is_err(),
            "no fallback: a missing file is an error"
        );
        write_json(&file, &json!({"schema": 1, "product": ""})).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n  \"schema\": 1,\n  \"product\": \"\"\n}\n"
        );
        assert_eq!(
            fs::read_dir(&deep).unwrap().count(),
            1,
            "no temporary file is left"
        );
        assert_eq!(read_json(&file, None).unwrap()["schema"], 1);
        // A path below a file is "missing" too (ENOTDIR).
        assert_eq!(
            read_json(&file.join("x.json"), Some(json!([]))).unwrap(),
            json!([])
        );
        fs::write(&file, "{ nope").unwrap();
        let err = read_json(&file, Some(Value::Null)).unwrap_err();
        assert!(
            err.message()
                .starts_with(&format!("Invalid JSON in {}: ", file.display())),
            "{err}"
        );
        assert!(!read_json(&file, None)
            .unwrap_err()
            .message()
            .starts_with("Invalid JSON in"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn jsonl_appends_and_reads_back() {
        let dir = test_dir("jsonl");
        let file = dir.join(rel::DECISIONS);
        assert!(read_jsonl(&file).unwrap().is_empty());
        append_jsonl(&file, &json!({"at": "t", "level": 1})).unwrap();
        append_jsonl(&file, &json!({"at": "u", "level": 2.0})).unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\"at\":\"t\",\"level\":1}\n{\"at\":\"u\",\"level\":2}\n"
        );
        fs::write(&file, "{\"a\":1}\n\n  \n{\"a\":2}").unwrap();
        assert_eq!(
            read_jsonl(&file).unwrap(),
            vec![json!({"a": 1}), json!({"a": 2})]
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn iso_time_has_the_javascript_shape() {
        assert_eq!(iso_from_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            iso_from_millis(1_709_164_799_999),
            "2024-02-28T23:59:59.999Z"
        );
        assert_eq!(
            iso_from_millis(1_709_210_096_007),
            "2024-02-29T12:34:56.007Z"
        );
        assert_eq!(
            iso_from_millis(1_794_960_000_000),
            "2026-11-18T00:00:00.000Z"
        );
        assert_eq!(iso_from_millis(-1), "1969-12-31T23:59:59.999Z");
        let now = now_iso();
        assert!(
            std::env::var("DFW_NOW").is_ok() || (now.len() == 24 && now.ends_with('Z')),
            "{now}"
        );
    }

    #[test]
    fn sha256_is_lowercase_hex() {
        assert_eq!(
            sha256("abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn deep_merge_merges_objects_and_replaces_the_rest() {
        let base = json!({"studio": {"port": 4600, "open": true}, "globs": ["a"], "x": 1});
        let over = json!({"studio": {"port": 4700}, "globs": ["b"], "x": null, "new": {"k": 1}});
        let merged = deep_merge(base, over);
        assert_eq!(
            merged,
            json!({"studio": {"port": 4700, "open": true}, "globs": ["b"], "x": null, "new": {"k": 1}})
        );
        assert_eq!(
            to_json_compact(&merged),
            r#"{"studio":{"port":4700,"open":true},"globs":["b"],"x":null,"new":{"k":1}}"#
        );
        assert_eq!(deep_merge(json!([1]), json!({"a": 1})), json!({"a": 1}));
        assert_eq!(deep_merge(json!({"a": 1}), json!("s")), json!("s"));
    }

    #[test]
    fn paths_in_values_and_cli_values() {
        let mut v = json!({"a": {"b": [10, 20]}, "s": "x"});
        assert_eq!(get_path(&v, "a.b.1"), Some(&json!(20)));
        assert_eq!(get_path(&v, "a.missing.c"), None);
        set_path(&mut v, "s.deep.k", json!(true)).unwrap();
        set_path(&mut v, "a.c", json!(1)).unwrap();
        assert_eq!(
            v,
            json!({"a": {"b": [10, 20], "c": 1}, "s": {"deep": {"k": true}}})
        );
        assert_eq!(parse_value("42"), json!(42));
        assert_eq!(parse_value("[1]"), json!([1]));
        assert_eq!(parse_value("vi"), json!("vi"));
    }

    #[test]
    fn list_files_sorts_excludes_and_copy_dir_copies() {
        const B: &str = "b.txt";
        const A_Z: &str = "a/z.txt";
        /// A file of `a/b`, the folder left out.
        const A_B_C: &str = "a/b/c.txt";
        /// The folder left out.
        const LEFT_OUT: &str = "approved";
        /// A file of [`LEFT_OUT`].
        const IN_LEFT_OUT: &str = "approved/v1/x.html";
        /// A file whose name starts like [`LEFT_OUT`]: it stays.
        const LIKE_LEFT_OUT: &str = "approved.txt";
        /// Upper case sorts first.
        const UPPER: &str = "Z.txt";
        let dir = test_dir("list");
        let src = dir.join("src");
        let dest = dir.join("dest");
        for f in [B, A_Z, A_B_C, IN_LEFT_OUT, LIKE_LEFT_OUT, UPPER] {
            let p = src.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, f).unwrap();
        }
        assert_eq!(
            list_files(&src, &[]).unwrap(),
            [UPPER, A_B_C, A_Z, LIKE_LEFT_OUT, IN_LEFT_OUT, B]
        );
        assert_eq!(
            list_files(&src, &[LEFT_OUT, "a/b"]).unwrap(),
            [UPPER, A_Z, LIKE_LEFT_OUT, B]
        );
        assert!(list_files(&dir.join("missing"), &[]).unwrap().is_empty());
        copy_dir(&src, &dest, &[LEFT_OUT]).unwrap();
        assert_eq!(
            list_files(&dest, &[]).unwrap(),
            [UPPER, A_B_C, A_Z, LIKE_LEFT_OUT, B]
        );
        assert_eq!(
            fs::read_to_string(dest.join("a").join("z.txt")).unwrap(),
            A_Z
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn plugin_manifest_reads_any_host_format() {
        let dir = test_dir("manifest");
        assert_eq!(plugin_manifest(Some(&dir)).unwrap(), json!({}));
        fs::write(
            dir.join(HOST_MANIFEST),
            r#"{"name":"dfw","version":"9.9.9"}"#,
        )
        .unwrap();
        assert_eq!(
            plugin_manifest(Some(&dir)).unwrap()[MANIFEST_VERSION],
            "9.9.9"
        );
        let claude = dir.join(CLAUDE_MANIFEST);
        fs::create_dir_all(claude.parent().unwrap()).unwrap();
        fs::write(&claude, r#"{"version":"1.0.0"}"#).unwrap();
        assert_eq!(
            plugin_manifest(Some(&dir)).unwrap()[MANIFEST_VERSION],
            "1.0.0",
            "the Claude Code manifest wins"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
