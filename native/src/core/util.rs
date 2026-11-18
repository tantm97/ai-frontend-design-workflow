//! Ports `core/lib/util.mjs`: JSON io, time, hashing, file listing, globs, config merging and
//! argument parsing. Path helpers are in `core::paths`, `DfwError` in `core::error`.
//!
//! JSON written here is byte-identical to `JSON.stringify(value, null, 2) + '\n'` for every value
//! `serde_json` can hold, with one known exception: JavaScript moves integer-like keys ("1", "42")
//! to the front of an object, `serde_json` keeps them where they were written.

use std::collections::HashMap;
use std::fs;
use std::io::{self, IsTerminal, Read, Write};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::Serialize;
use serde_json::ser::{CompactFormatter, Formatter, PrettyFormatter, Serializer};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::error::DfwError;
use super::paths::{env_nonempty, manifest_path, plugin_root};

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
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = file.as_os_str().to_owned();
    tmp.push(format!(".{}.tmp", std::process::id()));
    fs::write(&tmp, to_json_pretty(data) + "\n")?;
    fs::rename(&tmp, file)?;
    Ok(())
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

/// `String(number)` as ECMAScript defines it (Number::toString), for finite values.
pub fn js_number(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        // JSON has no NaN or Infinity; JSON.stringify prints them as null.
        return if v == 0.0 { "0".into() } else { "null".into() };
    }
    // `{:e}` gives the shortest digits that read back as the same double: "d.ddde<exp>".
    let sci = format!("{:e}", v.abs());
    let (mantissa, exp) = sci.split_once('e').unwrap_or((&sci, "0"));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp.parse::<i32>().unwrap_or(0) + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let sign = if e < 0 { '-' } else { '+' };
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{sign}{}", e.abs())
        } else {
            format!("{first}.{rest}e{sign}{}", e.abs())
        }
    };
    if v < 0.0 {
        format!("-{body}")
    } else {
        body
    }
}

/// `deepMerge(base, over)` for config layering: objects merge, arrays and scalars replace.
/// (The JavaScript `undefined` case, "no override", is the caller not calling this.)
pub fn deep_merge(base: Value, over: Value) -> Value {
    let Value::Object(over) = over else {
        return over;
    };
    let mut out = match base {
        Value::Object(map) => map,
        _ => Map::new(),
    };
    for (k, v) in over {
        let merged = match out.get_mut(&k) {
            Some(slot) => deep_merge(slot.take(), v),
            // A missing key still goes through the merge in Node; the result equals the override.
            None => v,
        };
        out.insert(k, merged);
    }
    Value::Object(out)
}

/// `getPath(obj, "a.b.c")`: `None` where JavaScript gives `undefined`.
pub fn get_path<'a>(obj: &'a Value, dotted: &str) -> Option<&'a Value> {
    dotted.split('.').try_fold(obj, |o, k| match o {
        Value::Object(map) => map.get(k),
        Value::Array(list) => k.parse::<usize>().ok().and_then(|i| list.get(i)),
        _ => None,
    })
}

/// `setPath(obj, "a.b.c", value)`: intermediate values that are not objects are replaced by objects.
pub fn set_path(obj: &mut Value, dotted: &str, value: Value) {
    let mut o = obj;
    for k in dotted.split('.') {
        if !o.is_object() {
            *o = Value::Object(Map::new());
        }
        let Value::Object(map) = o else { return };
        o = map.entry(k).or_insert(Value::Null);
    }
    *o = value;
}

/// `parseValue(v)`: a CLI value is JSON when it parses, else the raw string.
pub fn parse_value(v: &str) -> Value {
    serde_json::from_str(v).unwrap_or_else(|_| Value::String(v.to_string()))
}

// ---------------------------------------------------------------------------------------------
// Time and hashing
// ---------------------------------------------------------------------------------------------

/// `nowIso()`: the current time as `Date.prototype.toISOString` prints it. `DFW_NOW` replaces it
/// (conformance tests fix the clock so both implementations write the same timestamps).
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
            args.flags.insert(key.to_string(), value);
        } else {
            args.positionals.push(a.to_string());
        }
        i += 1;
    }
    args
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
    dir.canonicalize().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// glob matching (tests/unit/dfw.test.mjs)
    #[test]
    fn glob_matching() {
        assert!(glob_to_regex("design/**")
            .unwrap()
            .is_match("design/screens/a.html"));
        assert!(glob_to_regex("design/approved/**")
            .unwrap()
            .is_match("design/approved/v1/x.html"));
        assert!(!glob_to_regex("design/**").unwrap().is_match("src/design/x"));
        assert!(glob_to_regex("PRODUCT.md").unwrap().is_match("PRODUCT.md"));
    }

    #[test]
    fn glob_translation_matches_the_node_function() {
        // Expected sources printed by globToRegExp(...).source, with `.` spelled out.
        let dot = r"[^\n\r\u{2028}\u{2029}]";
        assert_eq!(
            glob_to_regex("**/*.test.?s").unwrap().as_str(),
            format!(r"^(?:{dot}*/)?[^/]*\.test\.[^/]s$")
        );
        assert_eq!(
            glob_to_regex("src/**").unwrap().as_str(),
            format!(r"^src/{dot}*$")
        );
        assert_eq!(
            glob_to_regex("a+(b)[c]{d}|^$\\").unwrap().as_str(),
            r"^a\+\(b\)\[c\]\{d\}\|\^\$\\$"
        );
        assert!(!glob_to_regex("PRODUCT.md").unwrap().is_match("PRODUCTxmd"));
        assert!(!glob_to_regex("*.html").unwrap().is_match("a/b.html"));
        assert!(glob_to_regex("**/b.html").unwrap().is_match("b.html"));
        assert!(glob_to_regex("a?c").unwrap().is_match("abc"));
        assert!(!glob_to_regex("a?c").unwrap().is_match("a/c"));
        assert!(matches_any("design/x.html", &["src/**", "design/**"]));
        assert!(!matches_any("design/x.html", &[] as &[&str]));
    }

    #[test]
    fn parse_args_follows_the_node_rules() {
        let a = parse_args(&[
            "comments",
            "--status",
            "open",
            "--json",
            "--message=a=b",
            "show",
            "--empty=",
            "--force",
            "--x",
            "-y",
        ]);
        assert_eq!(a.positionals, ["comments", "show"]);
        assert_eq!(a.text("status"), Some("open"));
        assert_eq!(a.get("json"), Some(&ArgValue::Flag));
        assert_eq!(a.text("message"), Some("a=b"));
        assert_eq!(a.get("empty"), Some(&ArgValue::Text(String::new())));
        assert!(!a.truthy("empty"));
        assert!(a.truthy("force"), "a flag followed by another flag is true");
        assert_eq!(a.text("x"), Some("-y"), "a single dash is a value");
        assert_eq!(a.get("missing"), None);
        assert_eq!(parse_args(&["--k", "1", "--k", "2"]).text("k"), Some("2"));
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
            (9007199254740993.0, "9007199254740992"),
        ] {
            assert_eq!(js_number(v), s);
        }
        assert_eq!(
            to_json_compact(&json!(9007199254740993u64)),
            "9007199254740992"
        );
    }

    #[test]
    fn write_json_is_atomic_and_read_json_falls_back() {
        let dir = test_dir("json");
        let file = dir.join("deep").join("state.json");
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
            fs::read_dir(dir.join("deep")).unwrap().count(),
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
        let file = dir.join(".dfw").join("decisions.jsonl");
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
        set_path(&mut v, "s.deep.k", json!(true));
        set_path(&mut v, "a.c", json!(1));
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
        let dir = test_dir("list");
        let src = dir.join("src");
        for f in [
            "b.txt",
            "a/z.txt",
            "a/b/c.txt",
            "approved/v1/x.html",
            "approved.txt",
            "Z.txt",
        ] {
            let p = src.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, f).unwrap();
        }
        assert_eq!(
            list_files(&src, &[]).unwrap(),
            [
                "Z.txt",
                "a/b/c.txt",
                "a/z.txt",
                "approved.txt",
                "approved/v1/x.html",
                "b.txt"
            ]
        );
        assert_eq!(
            list_files(&src, &["approved", "a/b"]).unwrap(),
            ["Z.txt", "a/z.txt", "approved.txt", "b.txt"]
        );
        assert!(list_files(&dir.join("missing"), &[]).unwrap().is_empty());
        copy_dir(&src, &dir.join("dest"), &["approved"]).unwrap();
        assert_eq!(
            list_files(&dir.join("dest"), &[]).unwrap(),
            ["Z.txt", "a/b/c.txt", "a/z.txt", "approved.txt", "b.txt"]
        );
        assert_eq!(
            fs::read_to_string(dir.join("dest").join("a").join("z.txt")).unwrap(),
            "a/z.txt"
        );
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn plugin_manifest_reads_any_host_format() {
        let dir = test_dir("manifest");
        assert_eq!(plugin_manifest(Some(&dir)).unwrap(), json!({}));
        fs::write(
            dir.join("plugin.json"),
            r#"{"name":"dfw","version":"9.9.9"}"#,
        )
        .unwrap();
        assert_eq!(plugin_manifest(Some(&dir)).unwrap()["version"], "9.9.9");
        fs::create_dir_all(dir.join(".claude-plugin")).unwrap();
        fs::write(
            dir.join(".claude-plugin").join("plugin.json"),
            r#"{"version":"1.0.0"}"#,
        )
        .unwrap();
        assert_eq!(
            plugin_manifest(Some(&dir)).unwrap()["version"],
            "1.0.0",
            "the Claude Code manifest wins"
        );
        fs::remove_dir_all(&dir).unwrap();
    }
}
