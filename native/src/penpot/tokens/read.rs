//! Reading a Penpot token export: one JSON file (multi-set or one set) or a folder of set
//! files (`readPenpotFile`, `parsePenpotJson`), and which dfw file each set feeds (`mapSets`).

use super::*;

/// The tokens of one Penpot set (`$value`/`$type`, or the older `value`/`type`). Names with a
/// reserved segment are not walked into: they are collected in `reserved`.
pub(super) fn walk_set(tree: &Value, reserved: &mut Vec<String>) -> Flat {
    fn walk(
        node: &Value,
        trail: &mut Vec<String>,
        ty: Option<&Value>,
        out: &mut Flat,
        reserved: &mut Vec<String>,
    ) {
        let name = trail.join(".");
        if token_path(&name).is_none() {
            reserved.push(name);
            return;
        }
        let Value::Object(map) = node else {
            return;
        };
        let ty = js::truthy_value(map.get(key::TYPE))
            .or_else(|| js::truthy_value(map.get("type")))
            .or(ty);
        let legacy = map.contains_key("value") && map.contains_key("type") && !trail.is_empty();
        if map.contains_key(key::VALUE) || legacy {
            out.set(
                name,
                Token {
                    ty: ty.cloned(),
                    value: map
                        .get(key::VALUE)
                        .or_else(|| map.get("value"))
                        .cloned()
                        .unwrap_or(Value::Null),
                    description: js::present(map.get(key::DESCRIPTION))
                        .or_else(|| map.get("description"))
                        .cloned(),
                },
            );
            return;
        }
        for (key, child) in js::entries(map) {
            if key.starts_with('$') {
                continue;
            }
            let depth = trail.len();
            trail.extend(key.split('.').map(str::to_string));
            walk(child, trail, ty, out, reserved);
            trail.truncate(depth);
        }
    }
    let mut out = Flat::default();
    walk(tree, &mut Vec::new(), None, &mut out, reserved);
    out
}

/// A Penpot token export as read: its sets by name, in file order.
#[derive(Debug, Clone, PartialEq)]
pub struct PenpotFile {
    pub sets: Vec<(String, Value)>,
    pub themes: Value,
    pub metadata: Value,
    /// A document without `$themes`/`$metadata`: the whole file is one set.
    pub single: bool,
}

impl PenpotFile {
    pub(super) fn set(&self, name: &str) -> Option<&Value> {
        self.sets.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

/// ASCII punctuation in the order the root collation sorts it.
const PUNCTUATION: &str = " _-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

/// Order of `String.prototype.localeCompare` for file names, without the collation tables:
/// punctuation (ASCII in collation order), then digits, then letters; letters without regard
/// to case first. Accented letters sort after the plain ones here.
pub(super) fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let class = |c: char| -> (u8, usize, String) {
        let rank = if c.is_alphabetic() {
            2
        } else if c.is_numeric() {
            1
        } else {
            0
        };
        let place = PUNCTUATION.find(c).unwrap_or(PUNCTUATION.len());
        (rank, place, c.to_lowercase().collect())
    };
    let key = |s: &str| s.chars().map(class).collect::<Vec<_>>();
    key(a)
        .cmp(&key(b))
        // Lowercase before uppercase when nothing else differs.
        .then_with(|| b.cmp(a))
}

fn parse_json_file(f: &Path) -> Result<Value, DfwError> {
    let text = js::read_text(f)
        .map_err(|err| DfwError::new(format!("{}: {}", f.display(), err.message())))?;
    serde_json::from_str::<Value>(&text)
        .map(js::key_order)
        .map_err(|err| DfwError::new(format!("{}: {err}", f.display())))
}

fn walk_set_files(
    dir: &Path,
    rel: &mut Vec<String>,
    sets: &mut Vec<(String, Value)>,
) -> Result<(), DfwError> {
    let mut entries = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        entries.push((name, entry.file_type()?.is_dir()));
    }
    entries.sort_by(|a, b| locale_cmp(&a.0, &b.0));
    for (name, is_dir) in entries {
        if is_dir {
            rel.push(name.clone());
            walk_set_files(&dir.join(&name), rel, sets)?;
            rel.pop();
        } else if name.ends_with(".json") && !name.starts_with('$') {
            let stem = name.strip_suffix(".json").unwrap_or(&name);
            let key = rel
                .iter()
                .map(String::as_str)
                .chain([stem])
                .collect::<Vec<_>>()
                .join("/");
            let doc = parse_json_file(&dir.join(&name))?;
            match sets.iter_mut().find(|(k, _)| *k == key) {
                Some(slot) => slot.1 = doc,
                None => sets.push((key, doc)),
            }
        }
    }
    Ok(())
}

/// `readPenpotFile`: a single JSON file (multi-set or one set) or a folder of set files with
/// `$themes.json` / `$metadata.json`.
pub fn read_penpot_file(file: &Path) -> Result<PenpotFile, DfwError> {
    if !file.exists() {
        return Err(DfwError::new(format!("{} not found.", file.display())));
    }
    if !file.is_dir() {
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let fallback = name.strip_suffix(".json").unwrap_or(&name);
        return parse_penpot_json(parse_json_file(file)?, fallback);
    }
    let mut sets = Vec::new();
    walk_set_files(file, &mut Vec::new(), &mut sets)?;
    let optional = |name: &str, fallback: Value| -> Result<Value, DfwError> {
        let f = file.join(name);
        if f.exists() {
            parse_json_file(&f)
        } else {
            Ok(fallback)
        }
    };
    Ok(PenpotFile {
        sets,
        themes: optional("$themes.json", json!([]))?,
        metadata: optional("$metadata.json", json!({}))?,
        single: false,
    })
}

/// `parsePenpotJson`: like Penpot, a document without `$themes`/`$metadata` is one set;
/// otherwise the top-level keys are sets.
pub fn parse_penpot_json(doc: Value, fallback_name: &str) -> Result<PenpotFile, DfwError> {
    let single = |doc: Value| PenpotFile {
        sets: vec![(fallback_name.to_string(), doc)],
        themes: json!([]),
        metadata: json!({}),
        single: true,
    };
    let map = match doc {
        Value::Object(map) => map,
        Value::Array(_) => return Ok(single(doc)),
        _ => {
            return Err(DfwError::new(
                "Not a Penpot token file (expected a JSON object).",
            ))
        }
    };
    if !map.contains_key(key::THEMES) && !map.contains_key(key::METADATA) {
        return Ok(single(Value::Object(map)));
    }
    let or =
        |key: &str, fallback: Value| js::truthy_value(map.get(key)).cloned().unwrap_or(fallback);
    let (themes, metadata) = (or(key::THEMES, json!([])), or(key::METADATA, json!({})));
    let sets = js::entries(&map)
        .into_iter()
        .filter(|(k, _)| !k.starts_with('$'))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    Ok(PenpotFile {
        sets,
        themes,
        metadata,
        single: false,
    })
}

/// Which dfw file a Penpot set feeds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SetKind {
    /// `design/tokens/tokens.json`
    Base,
    /// `design/tokens/themes/<name>.json`
    Theme(String),
}

/// The result of [`map_sets`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetMap {
    pub map: Vec<(String, SetKind)>,
    pub skipped: Vec<String>,
    pub base_name: Option<String>,
}

impl SetMap {
    /// What the set with this name feeds.
    #[cfg(test)]
    pub fn get(&self, name: &str) -> Option<&SetKind> {
        self.map.iter().find(|(n, _)| n == name).map(|(_, k)| k)
    }
}

/// `mapSets`: `base` feeds tokens.json, `themes/<n>` (or `theme/<n>`) feeds a theme, the rest
/// is skipped. A theme name that starts with a dot (`..`, `.`, `.env`) is skipped too: it would
/// name a hidden file under design/tokens/themes/.
pub fn map_sets(parsed: &PenpotFile) -> Result<SetMap, DfwError> {
    let listed: Vec<String> = match js::truthy_value(parsed.metadata.get("tokenSetOrder")) {
        None => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(s)) => s.chars().map(String::from).collect(),
        Some(_) => {
            return Err(DfwError::new(
                "\"$metadata.tokenSetOrder\" in the Penpot token file must be a list of set names. Export the tokens again from Penpot's Tokens panel, or fix the file.",
            ))
        }
    };
    let mut order: Vec<String> = Vec::new();
    for name in listed
        .into_iter()
        .chain(parsed.sets.iter().map(|(n, _)| n.clone()))
    {
        if parsed.set(&name).is_some() && !order.contains(&name) {
            order.push(name);
        }
    }
    // A lone "themes/<n>" set in a multi-set file is still a theme, never the base (it would
    // overwrite tokens.json).
    let base_name = if parsed.single {
        order.first().cloned()
    } else if parsed.set(BASE_SET).is_some() {
        Some(BASE_SET.to_string())
    } else {
        order
            .iter()
            .find(|s| !THEME_SET_PREFIX.is_match(s))
            .cloned()
    };
    let mut map = Vec::new();
    let mut skipped = Vec::new();
    for s in order {
        let theme = THEME_SET
            .captures(&s)
            .map(|m| m[1].to_string())
            .filter(|name| THEME_NAME.is_match(name) && !name.starts_with('.'));
        if Some(&s) == base_name.as_ref() {
            map.push((s, SetKind::Base));
        } else if let Some(name) = theme {
            map.push((s, SetKind::Theme(name)));
        } else {
            skipped.push(s);
        }
    }
    Ok(SetMap {
        map,
        skipped,
        base_name,
    })
}
