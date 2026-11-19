//! Project IO: reading the dfw tokens of a project, `dfw tokens export --format penpot`,
//! `dfw tokens import --from penpot` and the text both print.

use super::*;

/// The tokens of a project: `tokens.json`, the themes by name, and the option themes left out.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DfwTokens {
    pub base: Value,
    pub themes: Map<String, Value>,
    pub skipped: Vec<String>,
}

/// `readDfwTokens`: `design/tokens/tokens.json` and the themes. `opt-*` themes (temporary
/// preset comparisons) are left out unless asked.
pub fn read_dfw_tokens(root: &Path, opt_themes: bool) -> Result<DfwTokens, DfwError> {
    let dir = project_paths(root).tokens;
    let base_file = dir.join("tokens.json");
    if !base_file.exists() {
        return Err(DfwError::new("design/tokens/tokens.json not found."));
    }
    let mut tokens = DfwTokens {
        base: js::read_json(&base_file, None)?,
        ..DfwTokens::default()
    };
    let themes_dir = dir.join("themes");
    let mut files: Vec<String> = dir_names(&themes_dir)?
        .into_iter()
        .filter(|f| f.ends_with(".json"))
        .collect();
    files.sort_by(|a, b| js::cmp_utf16(a, b));
    for file in files {
        let name = file.strip_suffix(".json").unwrap_or(&file).to_string();
        if name.starts_with("opt-") && !opt_themes {
            tokens.skipped.push(name);
        } else {
            let tree = js::read_json(&themes_dir.join(&file), None)?;
            tokens.themes.insert(name, tree);
        }
    }
    Ok(tokens)
}

/// What [`export_penpot_tokens`] wrote.
#[derive(Debug, Clone, PartialEq)]
pub struct TokenExport {
    pub file: PathBuf,
    pub json: Value,
    pub report: ExportReport,
}

/// `exportPenpotTokens`: write the project's tokens as a Penpot token file (`out` is relative
/// to the project; default `.dfw/penpot/tokens.penpot.json`).
pub fn export_penpot_tokens(
    root: &Path,
    out: Option<&str>,
    opt_themes: bool,
) -> Result<TokenExport, DfwError> {
    let tokens = read_dfw_tokens(root, opt_themes)?;
    let (json, mut report) = dfw_to_penpot(Some(&tokens.base), &tokens.themes);
    report.skipped_themes = tokens.skipped;
    let file = match out.filter(|o| !o.is_empty()) {
        // A file the user named may be outside the project on purpose.
        Some(out) => {
            let file = resolve(root.join(out));
            write_json(&file, &json)?;
            file
        }
        None => {
            let file = resolve(root.join(".dfw").join("penpot").join("tokens.penpot.json"));
            write_json_inside(root, &file, &json)?;
            file
        }
    };
    Ok(TokenExport { file, json, report })
}

/// The result of [`import_penpot_tokens`].
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TokenImport {
    /// Files written, relative to the project.
    pub written: Vec<String>,
    /// The diff of every imported set, by set name.
    pub diff: Vec<(String, SetDiff)>,
    pub unsupported: Vec<Value>,
    pub warnings: Vec<Value>,
    pub skipped_sets: Vec<String>,
}

impl TokenImport {
    /// The result as the object the Node code printed.
    pub fn to_json(&self) -> Value {
        let diff: Map<String, Value> = self
            .diff
            .iter()
            .map(|(set, d)| (set.clone(), d.to_json()))
            .collect();
        json!({
            "written": self.written,
            "diff": diff,
            "unsupported": self.unsupported,
            "warnings": self.warnings,
            "skippedSets": self.skipped_sets,
        })
    }
}

/// `importPenpotTokens`: import a Penpot token file into `design/tokens`. Refused while the
/// design is frozen (G2) or the project is review-only, unless `dry_run`.
pub fn import_penpot_tokens(
    root: &Path,
    file: &Path,
    state: Option<&Value>,
    dry_run: bool,
    prune: bool,
) -> Result<TokenImport, DfwError> {
    let design = state.and_then(|s| s.get("design"));
    if !dry_run && js::truthy(design.and_then(|d| d.get("locked"))) {
        return Err(DfwError::new(format!(
            "Design v{} is approved and frozen (G2): token changes from Penpot are not applied. Show them with --dry-run, and if the user wants them, run \"dfw reopen --reason <why>\" first.",
            js::to_string(design.and_then(|d| d.get("version")))
        )));
    }
    if !dry_run && state.and_then(|s| s.get("mode")).and_then(Value::as_str) == Some("review-only")
    {
        return Err(DfwError::new(
            "This project is in review-only mode: use --dry-run to report token differences.",
        ));
    }
    let parsed = read_penpot_file(&resolve(file))?;
    let SetMap { map, skipped, .. } = map_sets(&parsed)?;
    if !map.iter().any(|(_, kind)| *kind == SetKind::Base) {
        return Err(DfwError::new(
            "No base token set found in the Penpot file (expected a set named \"base\").",
        ));
    }
    let dir = project_paths(root).tokens;
    let cur = if dir.join("tokens.json").exists() {
        read_dfw_tokens(root, true)?
    } else {
        DfwTokens {
            base: json!({}),
            ..DfwTokens::default()
        }
    };
    let mut result = TokenImport {
        skipped_sets: skipped,
        ..TokenImport::default()
    };
    let mut writes = Vec::new();
    let null = Value::Null;
    for (set, kind) in &map {
        let (current, target) = match kind {
            SetKind::Base => (Some(&cur.base), dir.join("tokens.json")),
            SetKind::Theme(name) => (
                cur.themes.get(name),
                dir.join("themes").join(format!("{name}.json")),
            ),
        };
        if let (SetKind::Theme(name), None) = (kind, current) {
            // Node bug kept: a new theme with an inherited name read the built-in function as
            // the current tokens and failed on it, before anything was written.
            if INHERITED.contains(&name.as_str()) {
                return Err(DfwError::new(format!(
                    "The Penpot set \"{set}\" names the theme \"{name}\", a reserved name. Rename the set in Penpot (for example \"themes/{name}-theme\") and export the tokens again."
                )));
            }
        }
        let merged = merge_set(current, parsed.set(set).unwrap_or(&null), set, prune)?;
        if merged.diff.writes() || !target.exists() {
            writes.push((target, merged.tree));
        }
        result.diff.push((set.clone(), merged.diff));
        result.unsupported.extend(merged.unsupported);
        result.warnings.extend(merged.warnings);
    }
    result.diff = js::key_order_pairs(std::mem::take(&mut result.diff));
    if !dry_run {
        for (file, tree) in &writes {
            write_json_inside(root, file, tree)?;
            result.written.push(relative_path(root, file));
        }
    }
    Ok(result)
}

/// `shortValue`: a token value on one line of at most 60 characters.
pub fn short_value(v: Option<&Value>) -> String {
    let object = v.and_then(Value::as_object);
    let hex = object.and_then(|m| js::truthy_value(m.get("hex")).filter(|_| m.len() <= 4));
    let s = match (v, object, hex) {
        (Some(Value::String(s)), _, _) => s.clone(),
        (_, Some(m), Some(hex)) => match m.get("alpha") {
            Some(alpha) => format!(
                "{}/{}",
                js::to_string(Some(hex)),
                js::to_string(Some(alpha))
            ),
            None => js::to_string(Some(hex)),
        },
        (_, Some(m), None) if m.contains_key("unit") => {
            format!(
                "{}{}",
                js::to_string(m.get("value")),
                js::to_string(m.get("unit"))
            )
        }
        _ => js::stringify(v),
    };
    if js::len16(&s) > 60 {
        format!("{}...", js::slice16(&s, 0, 57))
    } else {
        s
    }
}

fn field(v: &Value, key: &str) -> String {
    js::to_string(v.get(key))
}

/// `formatImport`: the summary the CLI prints.
pub fn format_import(r: &TokenImport, dry_run: bool) -> String {
    let mut lines = Vec::new();
    for (set, d) in &r.diff {
        let n = d.added.len() + d.changed.len() + d.removed.len();
        let counts = if n > 0 {
            format!(
                "{} changed, {} added, {} removed",
                d.changed.len(),
                d.added.len(),
                d.removed.len()
            )
        } else {
            "no changes".to_string()
        };
        let kept = if d.kept.is_empty() {
            String::new()
        } else {
            format!(
                " ({} missing in Penpot, kept; --prune removes them)",
                d.kept.len()
            )
        };
        lines.push(format!("{set}: {counts}{kept}"));
        for c in &d.changed {
            lines.push(if js::truthy(c.get("descriptionOnly")) {
                format!(
                    "  ~ {}: description changed (value kept)",
                    field(c, "token")
                )
            } else {
                format!(
                    "  ~ {}: {} -> {}",
                    field(c, "token"),
                    short_value(c.get("from")),
                    short_value(c.get("to"))
                )
            });
        }
        for c in &d.added {
            lines.push(format!(
                "  + {} ({}): {}",
                field(c, "token"),
                field(c, "type"),
                short_value(c.get("value"))
            ));
        }
        for c in &d.removed {
            lines.push(format!("  - {}", field(c, "token")));
        }
    }
    if !r.skipped_sets.is_empty() {
        lines.push(format!(
            "Skipped Penpot sets (name them \"base\" or \"themes/<name>\" to import): {}",
            r.skipped_sets.join(", ")
        ));
    }
    for u in &r.unsupported {
        lines.push(format!(
            "Not imported: {} {} ({}): {}",
            field(u, "set"),
            field(u, "token"),
            field(u, "type"),
            field(u, "reason")
        ));
    }
    for w in &r.warnings {
        lines.push(format!(
            "Note: {} {}: {}",
            field(w, "set"),
            field(w, "token"),
            field(w, "note")
        ));
    }
    lines.push(if dry_run {
        "Dry run: nothing written.".to_string()
    } else if r.written.is_empty() {
        "Nothing to write.".to_string()
    } else {
        format!("Wrote {}; tokens.css rebuilt.", r.written.join(", "))
    });
    lines.join("\n")
}

/// `formatExport`: the summary the CLI prints.
pub fn format_export(export: &TokenExport, root: &Path) -> String {
    let report = &export.report;
    let rel = relative_path(root, &export.file);
    let shown = if rel.is_empty() {
        export.file.display().to_string()
    } else {
        rel
    };
    let mut lines = vec![format!(
        "Wrote {shown}: {} token(s) in Penpot format (sets: {}).",
        report.exported,
        report.sets.join(", ")
    )];
    if !report.skipped_themes.is_empty() {
        lines.push(format!(
            "Left out option themes: {} (temporary preset comparisons; --opt-themes includes them).",
            report.skipped_themes.join(", ")
        ));
    }
    for u in &report.unsupported {
        lines.push(format!(
            "Not exported: {} {} ({}): {}",
            field(u, "set"),
            field(u, "token"),
            field(u, "type"),
            field(u, "reason")
        ));
    }
    let mut lossy: Vec<(String, Vec<String>)> = Vec::new();
    for l in &report.lossy {
        let (note, token) = (field(l, "note"), field(l, "token"));
        match lossy.iter_mut().find(|(n, _)| *n == note) {
            Some(slot) => slot.1.push(token),
            None => lossy.push((note, vec![token])),
        }
    }
    for (note, tokens) in lossy {
        let more = if tokens.len() > 6 {
            format!(", +{}", tokens.len() - 6)
        } else {
            String::new()
        };
        let first: Vec<&str> = tokens.iter().take(6).map(String::as_str).collect();
        lines.push(format!("Lossy: {note}: {}{more}", first.join(", ")));
    }
    lines.push("Import it in Penpot: Tokens panel > Tools > Import (replaces the file's token sets). Re-import changes with \"dfw tokens import <file>\".".to_string());
    lines.join("\n")
}
