//! Ports `core/lib/system.mjs`: design system init, preset options, choose, summary.
//!
//! Scaffolding only: style or preset tokens and an empty component inventory, 2-3 options side
//! by side as token themes, then the chosen one promoted. Components are designed per product
//! from the inventory; the starter kit is a reference (see `inventory`).

use std::fs;
use std::path::Path;

use serde_json::{json, Map, Value};

use super::compose::{build_system, load_components};
use super::inventory::ensure_inventory;
use super::style::{apply_style, get_style, head_for, is_style, style_name, style_tokens};
use super::tokens::{build_tokens, dir_names, option};
use crate::assets::{self, Folder};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::{project_paths, rel};
use crate::core::util::{write_atomic_inside, write_json_inside};

/// The presets of the starter kit (`KIT_DIR/presets`), a folder of the embedded kits.
const PRESETS_DIR: &str = "base/presets";

/// The messages of the design system commands, named once (as `studio::http::message`).
pub mod message {
    use super::{option, rel};

    /// `dfw system options` takes 2 or 3 names.
    pub const OPTIONS_COUNT: &str = "Give 2-3 style guides or presets to compare, e.g. dfw system options industrial-utility swiss-grid ops-dense";

    /// No preset of the kit has this name.
    pub fn unknown_preset(name: &str, presets: &[String]) -> String {
        format!("Unknown preset \"{name}\". Presets: {}", presets.join(", "))
    }

    /// `system init --style` would replace a design system without `--force`.
    pub fn tokens_exist() -> String {
        format!(
            "{} already holds a design system; a style guide replaces its tokens, fonts and motif master. Pass --force to replace them.",
            rel::TOKENS_JSON
        )
    }

    /// Two of the names given to `dfw system options` make the same option theme.
    pub fn duplicate_option(theme: &str) -> String {
        format!("Two options are both named {theme}; give each guide its own \"id\".")
    }

    /// `dfw system choose <name>` for an option made from a guide file.
    pub fn choose_by_file(name: &str) -> String {
        format!(
            "Option {} came from a guide file: choose it by that file, e.g. dfw system choose {}",
            option::theme(name),
            rel::STYLE_SOURCE
        )
    }
}

/// `listPresets`: name and description of every preset of the kit.
pub fn list_presets() -> Result<Vec<(String, String)>, DfwError> {
    let mut presets = Vec::new();
    for f in assets::file_names(Folder::Kits, PRESETS_DIR)
        .iter()
        .filter(|f| f.ends_with(".tokens.json"))
    {
        let tokens =
            assets::json(Folder::Kits, &format!("{PRESETS_DIR}/{f}"))?.unwrap_or_else(|| json!({}));
        let description = tokens.get("$description").filter(|d| js::truthy(Some(d)));
        presets.push((
            f.replacen(".tokens.json", "", 1),
            description.map_or(String::new(), |d| js::to_string(Some(d))),
        ));
    }
    Ok(presets)
}

/// The tokens file text of a preset and the text of its head file (empty when it has none).
fn preset_files(name: &str) -> Result<(String, String), DfwError> {
    let Some(tokens) = assets::text(Folder::Kits, &format!("{PRESETS_DIR}/{name}.tokens.json"))
    else {
        let names: Vec<String> = list_presets()?.into_iter().map(|p| p.0).collect();
        return Err(DfwError::new(message::unknown_preset(name, &names)));
    };
    let head = assets::text(Folder::Kits, &format!("{PRESETS_DIR}/{name}.head.html"));
    Ok((tokens, head.unwrap_or_default()))
}

/// The mark in the description of the placeholder tokens `dfw init` writes.
const PLACEHOLDER: &str = "Placeholder tokens created by dfw init";

fn is_placeholder_tokens(file: &Path) -> Result<bool, DfwError> {
    if !file.exists() {
        return Ok(true);
    }
    Ok(js::read_text(file)?.contains(PLACEHOLDER))
}

/// Options of [`system_init`].
/// The token preset `system init` starts from when none is named.
pub const DEFAULT_PRESET: &str = "crisp";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InitOptions {
    pub preset: String,
    pub style: Option<String>,
    /// `vi` or `latin`.
    pub fonts: String,
    pub force: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        InitOptions {
            preset: DEFAULT_PRESET.into(),
            style: None,
            fonts: "vi".into(),
            force: false,
        }
    }
}

/// What [`system_init`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemInit {
    /// The preset name, or `style:<id>`.
    pub preset: String,
    pub tokens_written: bool,
    pub inventory_created: bool,
}

/// `systemInit`: start the design system: tokens + fonts (+ the style's motif master) and an
/// empty component inventory. Components are not copied from the kit. Existing tokens are kept
/// unless `force` is set.
pub fn system_init(root: &Path, opts: &InitOptions) -> Result<SystemInit, DfwError> {
    let p = project_paths(root);
    fs::create_dir_all(&p.components)?;
    let inventory_created = ensure_inventory(root)?;
    let tok_file = &p.tokens_json;
    if let Some(style) = &opts.style {
        if !opts.force && !is_placeholder_tokens(tok_file)? {
            return Err(DfwError::new(message::tokens_exist()));
        }
        apply_style(root, style, &opts.fonts)?;
        build_tokens(root)?;
        build_system(root)?;
        return Ok(SystemInit {
            preset: format!("style:{style}"),
            tokens_written: true,
            inventory_created,
        });
    }
    let mut tokens_written = false;
    let (tokens, head) = preset_files(&opts.preset)?;
    if opts.force || is_placeholder_tokens(tok_file)? {
        write_atomic_inside(root, tok_file, &tokens)?;
        tokens_written = true;
    }
    let head_file = &p.head;
    if opts.force || !head_file.exists() {
        write_atomic_inside(root, head_file, head)?;
    }
    build_tokens(root)?;
    build_system(root)?;
    Ok(SystemInit {
        preset: opts.preset.clone(),
        tokens_written,
        inventory_created,
    })
}

/// Remove the option themes (`opt-*`) of a themes folder.
fn remove_options(themes_dir: &Path) -> Result<(), DfwError> {
    for f in dir_names(themes_dir)?
        .iter()
        .filter(|f| option::is_option(f))
    {
        fs::remove_file(themes_dir.join(f))?;
    }
    Ok(())
}

/// Rewrite the `themes` of `design/design.json` (when the manifest exists): the option themes
/// are dropped, then `add` is appended.
fn set_manifest_themes(root: &Path, manifest: &Path, add: &[String]) -> Result<(), DfwError> {
    let mut m = js::read_json(manifest, Some(Value::Null))?;
    let Some(map) = m.as_object_mut() else {
        return Ok(());
    };
    let mut themes: Vec<Value> = Vec::new();
    let kept = map
        .get("themes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let options = add.iter().map(|n| json!(n));
    for t in kept
        .into_iter()
        .filter(|t| !option::is_option(&js::to_string(Some(t))))
        .chain(options)
    {
        // A Set in the Node code: with options added, a repeated name is listed once.
        if add.is_empty() || !themes.contains(&t) {
            themes.push(t);
        }
    }
    map.insert("themes".into(), Value::Array(themes));
    write_json_inside(root, manifest, &m)
}

/// `systemOptions`: write 2-3 style guides or presets as token themes `opt-<name>` so the same
/// screens can be compared in the Studio. Returns the option themes.
pub fn system_options(root: &Path, names: &[String], fonts: &str) -> Result<Vec<String>, DfwError> {
    if names.len() < 2 || names.len() > 3 {
        return Err(DfwError::new(message::OPTIONS_COUNT));
    }
    // Validate every name before touching the current options: a typo must not leave half the
    // themes replaced.
    let mut options: Vec<String> = Vec::new();
    for n in names {
        if is_style(n) {
            get_style(n)?;
        } else {
            preset_files(n)?;
        }
        let option = option::theme(&style_name(n)?);
        if options.contains(&option) {
            return Err(DfwError::new(message::duplicate_option(&option)));
        }
        options.push(option);
    }
    let p = project_paths(root);
    let themes_dir = &p.themes;
    fs::create_dir_all(themes_dir)?;
    remove_options(themes_dir)?;
    let mut heads = Vec::new();
    for (n, option) in names.iter().zip(&options) {
        let theme = themes_dir.join(format!("{option}.json"));
        let head = if is_style(n) {
            let g = get_style(n)?;
            write_json_inside(root, &theme, &style_tokens(&g, fonts)?)?;
            head_for(&g, fonts)
        } else {
            let (tokens, head) = preset_files(n)?;
            write_atomic_inside(root, &theme, &tokens)?;
            head
        };
        heads.push(js::trim(&js::strip_comments(&head)).to_string());
    }
    let head_file = &p.head;
    let current = if head_file.exists() {
        js::read_text(head_file)?
    } else {
        String::new()
    };
    let mut lines: Vec<&str> = Vec::new();
    let all = heads.join("\n");
    for line in all.split('\n') {
        if !lines.contains(&line) {
            lines.push(line);
        }
    }
    const BEGIN: &str = "<!-- dfw-options-begin -->";
    const END: &str = "<!-- dfw-options-end -->";
    let block = format!("{BEGIN}\n{}\n{END}", lines.join("\n"));
    if let Some(dir) = head_file.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = match current.find(BEGIN) {
        Some(start) => match current[start + BEGIN.len()..].find(END) {
            Some(len) => js::replace_range(
                &current,
                start..start + BEGIN.len() + len + END.len(),
                &block,
            ),
            None => current.clone(),
        },
        None => format!("{}\n{block}\n", js::trim(&current)),
    };
    write_atomic_inside(root, head_file, text)?;
    set_manifest_themes(root, &p.manifest, &options)?;
    let built = build_tokens(root)?;
    Ok(built
        .themes
        .into_iter()
        .filter(|t| option::is_option(t))
        .collect())
}

/// `systemChoose`: promote one option (or preset) to the base tokens and remove the option
/// themes. The option's tokens win, edits included. Returns the chosen name.
pub fn system_choose(root: &Path, name: &str, fonts: &str) -> Result<String, DfwError> {
    let p = project_paths(root);
    let opt = p.themes.join(option::file(&style_name(name)?));
    let tok_file = &p.tokens_json;
    if is_style(name) {
        // Fonts, motif, style.json and rationale come from the guide; the tokens come from the
        // option theme the user compared and approved, so edits made during the comparison stay.
        let edited = if opt.exists() {
            Some(fs::read(&opt)?)
        } else {
            None
        };
        apply_style(root, name, fonts)?;
        if let Some(edited) = edited {
            write_atomic_inside(root, tok_file, edited)?;
        }
    } else {
        let (tokens, head) = preset_files(name).map_err(|e| {
            if opt.exists() {
                // An option made from a guide file: its fonts and motif are in the file.
                DfwError::new(message::choose_by_file(name))
            } else {
                e
            }
        })?;
        let tokens = if opt.exists() {
            fs::read(&opt)?
        } else {
            tokens.into_bytes()
        };
        write_atomic_inside(root, tok_file, tokens)?;
        write_atomic_inside(root, &p.head, head)?;
    }
    remove_options(&p.themes)?;
    set_manifest_themes(root, &p.manifest, &[])?;
    build_tokens(root)?;
    build_system(root)?;
    Ok(name.to_string())
}

/// `systemSummary`: `{ components: [{ name, group, variants, sizes, props, file }] }`.
pub fn system_summary(root: &Path) -> Result<Value, DfwError> {
    let lib = load_components(root)?;
    let components: Vec<Value> = lib
        .iter()
        .map(|d| {
            let mut c = Map::new();
            if let Some(name) = &d.name {
                c.insert("name".into(), json!(name));
            }
            c.insert("group".into(), json!(d.group));
            c.insert("variants".into(), json!(d.variants));
            c.insert("sizes".into(), json!(d.sizes));
            c.insert("props".into(), json!(d.props));
            c.insert("file".into(), json!(d.file));
            Value::Object(c)
        })
        .collect();
    Ok(json!({ "components": components }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{
        screen_page, COMPOSED_PAGE, INDUSTRIAL_PRESET, INDUSTRIAL_STYLE, PLAYFUL_STYLE,
        SOFT_PRESET, SWISS_GRID_STYLE,
    };
    use crate::core::level::Level;
    use crate::core::paths::in_design;
    use crate::core::util::test_dir;
    use crate::design::compose::{compose_file, compose_html, ComposeOptions};
    use crate::design::icons::add_icons;
    use crate::design::inventory::{
        inventory_findings, kit_components, kit_ref, kit_refs, KIT_COPY,
    };
    use crate::design::style::MOTIF_FILE;
    use std::path::PathBuf;

    /// A component of the starter kit the tests put into a project.
    const BUTTON: &str = "button";
    /// The master a style guide brings with it ([`MOTIF_FILE`]).
    const MOTIF: &str = "motif";

    const KIT_ICONS: [&str; 20] = [
        "chevron-down",
        "triangle-alert",
        "search",
        "x",
        "check",
        "package",
        "house",
        "file-clock",
        "settings",
        "layout-dashboard",
        "circle-check",
        "circle-x",
        "circle-alert",
        "octagon-alert",
        "package-open",
        "ellipsis",
        "wrench",
        "log-in",
        "log-out",
        "plus",
    ];

    /// Test fixture only: put starter-kit masters into a project (the CLI never copies them).
    fn use_kit(root: &Path, names: Option<&[&str]>) {
        let dir = project_paths(root).components;
        fs::create_dir_all(&dir).unwrap();
        let kit = kit_components().unwrap();
        for f in dir_names(&kit)
            .unwrap()
            .iter()
            .filter(|f| f.ends_with(".html"))
        {
            if names.is_none_or(|n| n.contains(&f.trim_end_matches(".html"))) {
                fs::copy(kit.join(f), dir.join(f)).unwrap();
            }
        }
        add_icons(root, &KIT_ICONS.map(str::to_string)).unwrap();
        build_system(root).unwrap();
    }

    fn project() -> PathBuf {
        let root = test_dir("system");
        let p = project_paths(&root);
        write(&root, &p.state, "{}");
        fs::create_dir_all(&p.tokens).unwrap();
        write(
            &root,
            &p.manifest,
            r#"{"schema":1,"product":"T","screens":[]}"#,
        );
        root
    }

    /// Write a file of the project: a path relative to `root`, or one of `project_paths`.
    fn write(root: &Path, file: impl AsRef<Path>, text: &str) {
        let file = root.join(file);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    fn read(root: &Path, file: impl AsRef<Path>) -> String {
        fs::read_to_string(root.join(file)).unwrap()
    }

    fn json_of(root: &Path, file: impl AsRef<Path>) -> Value {
        serde_json::from_str(&read(root, file)).unwrap()
    }

    /// The option themes of these names, as `system_options` returns them.
    fn themes_of(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| option::theme(n)).collect()
    }

    /// The option theme files of these names, as the themes folder lists them.
    fn files_of(names: &[&str]) -> Vec<String> {
        names.iter().map(|n| option::file(n)).collect()
    }

    fn init(root: &Path, preset: &str) -> SystemInit {
        system_init(
            root,
            &InitOptions {
                preset: preset.into(),
                ..InitOptions::default()
            },
        )
        .unwrap()
    }

    fn init_style(root: &Path, style: &str, force: bool) -> Result<SystemInit, DfwError> {
        system_init(
            root,
            &InitOptions {
                style: Some(style.into()),
                force,
                ..InitOptions::default()
            },
        )
    }

    fn compose(root: &Path, html: &str, base: Option<&str>) -> crate::design::compose::Composed {
        compose_html(
            root,
            html,
            ComposeOptions {
                lib: None,
                design_base: base,
                prop_values: false,
            },
        )
        .unwrap()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    /// compose: instances, slots, props, passthrough, nested, icons, head
    #[test]
    fn compose_instances_slots_props_passthrough_nested_icons_head() {
        let root = project();
        init(&root, DEFAULT_PRESET);
        use_kit(&root, Some(&[BUTTON, "card", "badge"]));
        let html = "<html><head><dfw-head></dfw-head></head><body>\n<dfw-use c=\"card\" title=\"A &quot;x&quot;\" id=\"c1\"><p>Body</p><dfw-fill slot=\"actions\"><dfw-use c=\"button\" variant=\"ghost\" size=\"sm\" id=\"c1.more\" aria-label=\"More\">More</dfw-use></dfw-fill></dfw-use>\n<dfw-use c=\"badge\" variant=\"err\" icon=\"triangle-alert\">Overdue</dfw-use>\n<dfw-use c=\"button\" variant=\"nope\">X</dfw-use>\n</body></html>";
        let r = compose(&root, html, Some("../../"));
        let has = |needle: &str| assert!(r.html.contains(needle), "missing {needle} in {}", r.html);
        has(&format!(
            "<link rel=\"stylesheet\" href=\"../../{}\">",
            in_design::TOKENS_CSS
        ));
        has(in_design::COMPONENTS_CSS);
        has("fonts.googleapis.com");
        let card = &r.html[r
            .html
            .find("<section class=\"card card--default\"")
            .unwrap()..];
        let card = &card[..card.find('>').unwrap()];
        assert!(
            card.find("data-dfw-id=\"c1\"").unwrap()
                < card.find("data-dfw-component=\"card\"").unwrap()
        );
        has("<h2 class=\"card__title\">A &quot;x&quot;</h2>");
        let more = &r.html[r.html.find("class=\"btn btn--ghost btn--sm\"").unwrap()..];
        let more = &more[..more.find('>').unwrap()];
        assert!(
            more.find("aria-label=\"More\"").unwrap()
                < more.find("data-dfw-id=\"c1.more\"").unwrap()
        );
        has("<p>Body</p>");
        has("data-dfw-icon=\"triangle-alert\"");
        for gone in ["dfw-use", "dfw-slot", "dfw-fill", "data-dfw-if"] {
            assert!(!r.html.contains(gone), "{gone} left in {}", r.html);
        }
        let rules: Vec<&str> = r.issues.iter().map(|i| i.rule).collect();
        assert_eq!(rules, ["component-variant"]);
        assert_eq!(r.issues[0].line, 4);
        assert_eq!(
            r.instances
                .iter()
                .filter(|i| i.component.as_deref() == Some(BUTTON))
                .count(),
            2
        );
    }

    /// system: init writes tokens + an empty inventory and copies no components; options and choose
    #[test]
    fn init_options_and_choose() {
        let root = project();
        let p = project_paths(&root);
        let placeholder = json!({ "$description": format!("{PLACEHOLDER}.") });
        write(&root, &p.tokens_json, &placeholder.to_string());
        let r = init(&root, SOFT_PRESET);
        assert!(r.tokens_written);
        assert!(r.inventory_created);
        let css = read(&root, &p.tokens_css);
        assert!(css.contains("--color-accent: #0f6e66"));
        assert!(css.contains("--size-control: 40px"));
        assert_eq!(
            load_components(&root).unwrap().len(),
            0,
            "no kit masters are copied"
        );
        assert_eq!(json_of(&root, &p.inventory)["components"], json!([]));
        assert!(
            !init(&root, SOFT_PRESET).inventory_created,
            "an existing inventory is kept"
        );
        let mut presets: Vec<String> = list_presets().unwrap().into_iter().map(|p| p.0).collect();
        presets.sort();
        assert_eq!(presets, [DEFAULT_PRESET, INDUSTRIAL_PRESET, SOFT_PRESET]);

        let compared = [DEFAULT_PRESET, INDUSTRIAL_PRESET];
        let o = system_options(&root, &names(&compared), "vi").unwrap();
        assert_eq!(o, themes_of(&compared));
        let industrial = format!("[data-theme=\"{}\"]", option::theme(INDUSTRIAL_PRESET));
        assert!(read(&root, &p.tokens_css).contains(&industrial));
        assert!(read(&root, &p.head).contains("IBM+Plex+Sans"));
        assert_eq!(
            json_of(&root, &p.manifest)["themes"],
            json!(themes_of(&compared))
        );
        system_choose(&root, INDUSTRIAL_PRESET, "vi").unwrap();
        assert!(read(&root, &p.tokens_css).contains("--color-accent: #0b5cad"));
        assert!(!read(&root, &p.tokens_css).contains(option::PREFIX));
    }

    /// kit: every reference master composes its examples without issues and lints clean (the
    /// lint and Design System page assertions belong to design/lint and design/system_page)
    #[test]
    fn every_kit_master_composes_its_examples_without_issues() {
        let root = project();
        init(&root, DEFAULT_PRESET);
        use_kit(&root, None);
        let lib = load_components(&root).unwrap();
        assert!(lib.len() > 20);
        for d in lib.iter() {
            let fallback = format!("<dfw-use c=\"{}\"></dfw-use>", d.display_name());
            let r = compose(
                &root,
                if d.examples.is_empty() {
                    &fallback
                } else {
                    &d.examples
                },
                None,
            );
            assert!(r.issues.is_empty(), "{}: {:?}", d.display_name(), r.issues);
        }
    }

    /// composeFile + buildSystem for a screen file
    #[test]
    fn compose_file_and_build_system_for_a_screen() {
        let root = project();
        init(&root, DEFAULT_PRESET);
        use_kit(&root, Some(&[BUTTON]));
        write(&root, rel::screen(COMPOSED_PAGE), "<html><head><dfw-head></dfw-head></head><body><dfw-use c=\"button\" id=\"go\">Go</dfw-use></body></html>");
        let r = compose_file(&root, &rel::screen(COMPOSED_PAGE)).unwrap();
        let link = format!("href=\"../../{}\"", in_design::COMPONENTS_CSS);
        assert!(r.composed.html.contains(&link));
        assert_eq!(build_system(&root).unwrap().components, 1);
    }

    /// style guides: library, search, apply as aliased tokens with usage rules, latin fonts,
    /// motifs, options (the Design System page assertions belong to design/system_page; library,
    /// search and latin fonts are tested in design/style)
    #[test]
    fn style_guides_apply_as_aliased_tokens_with_motifs_and_options() {
        let root = project();
        let p = project_paths(&root);
        init_style(&root, "heritage-signboard", false).unwrap();
        let lib = load_components(&root).unwrap();
        let masters: Vec<&str> = lib.iter().map(|d| d.display_name()).collect();
        assert_eq!(masters, [MOTIF], "a style brings only its motif master");
        use_kit(&root, Some(&[BUTTON, "text"]));
        let css = read(&root, &p.tokens_css);
        assert!(
            css.contains("--color-accent: var(--color-palette-giay-900)"),
            "heritage uses an ink accent: lacquer red is the error hue"
        );
        assert!(css.contains("--font-family-display: \"Noto Serif Display\""));
        assert!(css.contains("--font-size-display-xl: 85px"));
        let tok = json_of(&root, &p.tokens_json);
        let description = tok["color"]["palette"]["kim-500"]["$description"]
            .as_str()
            .unwrap();
        assert!(description.contains("ornament"));
        assert!(description.contains("Used for: signature motif"));
        assert!(p.components.join(MOTIF_FILE).exists());
        let lib = load_components(&root).unwrap();
        assert!(lib
            .get(Some(MOTIF))
            .unwrap()
            .variants
            .iter()
            .any(|v| v == "signboard-frame"));
        assert!(read(&root, &p.head).contains("Noto+Serif+Display"));
        // options with styles
        let compared = [INDUSTRIAL_STYLE, SWISS_GRID_STYLE];
        let o = system_options(&root, &names(&compared), "vi").unwrap();
        assert_eq!(o, themes_of(&compared));
        system_choose(&root, SWISS_GRID_STYLE, "vi").unwrap();
        assert!(
            read(&root, &p.tokens_css).contains("--color-accent: var(--color-palette-coban-600)")
        );
        assert_eq!(json_of(&root, &p.style)["id"], SWISS_GRID_STYLE);
        // text component with prop defaults
        let t = compose(&root, "<dfw-use c=\"text\" variant=\"display-lg\" as=\"h1\">Hi</dfw-use><dfw-use c=\"text\">p</dfw-use>", None);
        assert!(
            t.html
                .starts_with("<h1 class=\"t t--display-lg t--tone-default\""),
            "{}",
            t.html
        );
        let p = &t.html[t
            .html
            .find("<p class=\"t t--body t--tone-default\"")
            .unwrap()..];
        assert!(p[p.find('>').unwrap()..].starts_with(">p</p>"));
        build_tokens(&root).unwrap();
    }

    /// inventory: kit copies, orphans, unbuilt entries and unused masters are findings
    #[test]
    fn inventory_findings_for_kit_copies_orphans_unbuilt_and_unused() {
        let root = project();
        let p = project_paths(&root);
        let master = |name: &str| p.components.join(crate::core::paths::file::master(name));
        init(&root, DEFAULT_PRESET);
        let kit_copy = format!("{KIT_COPY}:{}", Level::Error.as_str());
        let rules = || {
            let mut r: Vec<String> = inventory_findings(&root)
                .unwrap()
                .iter()
                .map(|f| {
                    format!(
                        "{}:{}",
                        f["rule"].as_str().unwrap(),
                        f["level"].as_str().unwrap()
                    )
                })
                .collect();
            r.sort();
            r
        };
        assert!(rules().is_empty(), "empty system: nothing to report");
        use_kit(&root, Some(&[BUTTON]));
        assert_eq!(rules(), ["inventory-missing:warn", kit_copy.as_str()]);
        write(&root, master(BUTTON), "<dfw-component name=\"button\" variants=\"primary\"><style>.b { color: var(--color-ink); }</style><template><button class=\"b\" type=\"button\"><dfw-slot>Go</dfw-slot></button></template><dfw-examples><dfw-use c=\"button\">Thêm vào giỏ</dfw-use></dfw-examples></dfw-component>");
        let badge = kit_ref("badge").unwrap();
        const EXAMPLES_END: &str = "</dfw-examples>";
        /// A master whose examples are the kit's.
        const TAG: &str = "tag";
        let kit_examples = &badge[badge.find("<dfw-examples>").unwrap()
            ..badge.find(EXAMPLES_END).unwrap() + EXAMPLES_END.len()];
        write(&root, master(TAG), &format!("<dfw-component name=\"tag\"><style>.t {{ color: var(--color-muted); }}</style><template><span class=\"t\"><dfw-slot></dfw-slot></span></template>{kit_examples}</dfw-component>"));
        write(
            &root,
            &p.inventory,
            r#"{"components":[{"name":"button","purpose":"Add to bag","usedBy":["product"]},{"name":"price","purpose":"Price with old price","usedBy":["product"]}]}"#,
        );
        assert_eq!(
            rules(),
            [
                "inventory-orphan:warn",
                "inventory-unbuilt:warn",
                kit_copy.as_str()
            ]
        );
        fs::remove_file(master(TAG)).unwrap();
        write(&root, master("price"), "<dfw-component name=\"price\"><template><span><dfw-slot></dfw-slot></span></template></dfw-component>");
        assert!(rules().is_empty());
        write(
            &root,
            &p.manifest,
            r#"{"schema":1,"product":"T","screens":[{"id":"product","variants":[{"id":"a","file":"screens/product/a.html"}]}]}"#,
        );
        write(
            &root,
            rel::screen(&screen_page("product")),
            "<dfw-use c=\"button\">Thêm</dfw-use>",
        );
        assert_eq!(
            rules(),
            ["unused-master:warn"],
            "price is never used once screens exist"
        );
        assert!(kit_refs()
            .unwrap()
            .iter()
            .any(|r| r.name == BUTTON && r.states.iter().any(|s| s == "focus-visible")));
        assert!(kit_ref(BUTTON)
            .unwrap()
            .contains(&format!("<dfw-component name=\"{BUTTON}\"")));
    }

    /// tokens: presets ship target-wcag (24, floor) and target-touch (44); no ambiguous target-min
    #[test]
    fn presets_ship_both_target_sizes() {
        /// The DTCG group of sizes.
        const SIZE: &str = "size";
        /// A token's value.
        const VALUE: &str = "$value";
        /// The number of a dimension value.
        const NUMBER: &str = "value";
        for name in [DEFAULT_PRESET, SOFT_PRESET, INDUSTRIAL_PRESET] {
            let t = assets::json(Folder::Kits, &format!("{PRESETS_DIR}/{name}.tokens.json"))
                .unwrap()
                .unwrap();
            assert_eq!(t[SIZE]["target-wcag"][VALUE][NUMBER], 24);
            assert_eq!(t[SIZE]["target-touch"][VALUE][NUMBER], 44);
            assert!(t[SIZE].get("target-min").is_none());
        }
    }

    /// system choose keeps edits made to the opt-<style> theme during the comparison
    #[test]
    fn choose_keeps_edits_made_to_the_option_theme() {
        let root = project();
        let p = project_paths(&root);
        /// The accent picked by hand in the option theme.
        const EDITED: &str = "#0d6b78";
        /// Where the accent lives in the tokens.
        const COLOR: &str = "color";
        const ACCENT: &str = "accent";
        init_style(&root, PLAYFUL_STYLE, false).unwrap();
        system_options(&root, &names(&[PLAYFUL_STYLE, "warm-craft"]), "vi").unwrap();
        let opt = &p.themes.join(option::file(PLAYFUL_STYLE));
        let mut t = json_of(&root, opt);
        t[COLOR][ACCENT] = json!({ "$value": { "colorSpace": "srgb", "components": [0.051, 0.4196, 0.4706], "hex": EDITED }, "$description": "Edited during the comparison" });
        write(&root, opt, &t.to_string());
        system_choose(&root, PLAYFUL_STYLE, "vi").unwrap();
        assert_eq!(
            json_of(&root, &p.tokens_json)[COLOR][ACCENT]["$value"]["hex"],
            EDITED
        );
        assert!(read(&root, &p.tokens_css).contains(&format!("--color-accent: {EDITED}")));
        assert!(
            p.components.join(MOTIF_FILE).exists(),
            "motif still comes from the guide"
        );
        assert_eq!(json_of(&root, &p.style)["id"], PLAYFUL_STYLE);
        assert!(!opt.exists(), "option themes are removed");
    }

    /// 3: system init --style keeps existing tokens unless --force (tests/unit/audit-fixes.test.mjs)
    #[test]
    fn init_with_a_style_keeps_existing_tokens_unless_forced() {
        let style = crate::design::style::list_styles().unwrap()[0]["id"]
            .as_str()
            .unwrap()
            .to_string();
        let mine = r##"{"$description":"hand-tuned","color":{"$type":"color","ink":{"$value":"#123456"}}}"##;
        let root = test_dir("system-force");
        let p = project_paths(&root);
        write(&root, &p.tokens_json, mine);
        /// The user's own head, kept unless forced.
        const MY_HEAD: &str = "<!-- my fonts -->";
        write(&root, &p.head, MY_HEAD);
        let err = init_style(&root, &style, false).unwrap_err();
        assert_eq!(err.message(), message::tokens_exist());
        assert_eq!(read(&root, &p.tokens_json), mine);
        assert_eq!(read(&root, &p.head), MY_HEAD);
        assert!(init_style(&root, &style, true).unwrap().tokens_written);
        assert_ne!(read(&root, &p.tokens_json), mine);
        // No tokens yet (or the dfw init placeholder): the style applies without --force.
        assert!(
            init_style(&test_dir("system-fresh"), &style, false)
                .unwrap()
                .tokens_written
        );
    }

    /// system options: a mistyped name leaves the current options untouched
    #[test]
    fn a_mistyped_option_leaves_the_current_options_untouched() {
        let root = test_dir("system-typo");
        let p = project_paths(&root);
        write(&root, &p.manifest, r#"{"screens":[]}"#);
        /// A misspelt preset name.
        const TYPO: &str = "sotf";
        init(&root, DEFAULT_PRESET);
        let current = [DEFAULT_PRESET, SOFT_PRESET];
        system_options(&root, &names(&current), "vi").unwrap();
        let themes = || dir_names(&p.themes).unwrap();
        assert_eq!(themes(), files_of(&current));
        let err = system_options(&root, &names(&[INDUSTRIAL_PRESET, TYPO]), "vi").unwrap_err();
        let presets: Vec<String> = list_presets().unwrap().into_iter().map(|p| p.0).collect();
        assert_eq!(err.message(), message::unknown_preset(TYPO, &presets));
        assert_eq!(themes(), files_of(&current));
    }

    /// `system init --force` with `design/system/head.html` linked to a file outside the project
    /// refuses and leaves that file alone (review CO-3).
    #[cfg(unix)]
    #[test]
    fn init_never_writes_through_a_link_out_of_the_project() {
        use crate::core::paths::file;
        let dir = test_dir("system-link");
        let root = dir.join("project");
        let p = project_paths(&root);
        fs::create_dir_all(&p.system).unwrap();
        /// The text of the file outside the project.
        const THEIRS: &str = "theirs";
        fs::write(dir.join(file::HEAD), THEIRS).unwrap();
        std::os::unix::fs::symlink(Path::new("../../..").join(file::HEAD), &p.head).unwrap();
        let opts = InitOptions {
            force: true,
            ..InitOptions::default()
        };
        let err = system_init(&root, &opts).unwrap_err();
        let refused = format!("{} leads outside the project", rel::HEAD);
        assert!(err.message().starts_with(&refused), "{err}");
        assert_eq!(fs::read_to_string(dir.join(file::HEAD)).unwrap(), THEIRS);
        fs::remove_dir_all(&dir).unwrap();
    }

    /// A project guide file is an option under its own id (else its file name), never under its
    /// path: the theme used to land in `themes/opt-design/system/<file>.json.json`, unseen.
    #[test]
    fn a_guide_file_is_an_option_under_its_id() {
        /// The project's own id in the guide files, and a second file that reuses it.
        const ID: &str = "my-brand";
        const TWIN: &str = "twin.json";
        /// A guide file whose own id and file name are both unfit for a theme name.
        const UNFIT: &str = "Other Guide.json";
        let root = test_dir("system-guide-file");
        let p = project_paths(&root);
        write(&root, &p.manifest, r#"{"screens":[]}"#);
        init(&root, DEFAULT_PRESET);
        let guide = |id: &str| {
            let mut g = get_style("civic-public").unwrap();
            g["id"] = json!(id);
            g.to_string()
        };
        let path = |file: &Path| file.to_string_lossy().into_owned();
        let source = path(&root.join(rel::STYLE_SOURCE));
        let unfit = path(&p.system.join(UNFIT));
        let twin = path(&p.system.join(TWIN));
        write(&root, rel::STYLE_SOURCE, &guide(ID));
        write(&root, &unfit, &guide("Not An Id"));
        let options =
            system_options(&root, &[source.clone(), DEFAULT_PRESET.into()], "vi").unwrap();
        assert_eq!(options, themes_of(&[DEFAULT_PRESET, ID]));
        let themes = || dir_names(&p.themes).unwrap();
        assert_eq!(themes(), files_of(&[DEFAULT_PRESET, ID]));
        // No plain id and no plain file name: refused before anything is replaced.
        let err = system_options(&root, &[unfit.clone(), SOFT_PRESET.into()], "vi").unwrap_err();
        assert_eq!(
            err.message(),
            crate::design::style::message::guide_id_not_plain(&unfit)
        );
        // Two guides with one id would overwrite each other.
        write(&root, &twin, &guide(ID));
        let err = system_options(&root, &[source.clone(), twin], "vi").unwrap_err();
        assert_eq!(err.message(), message::duplicate_option(&option::theme(ID)));
        assert_eq!(themes(), files_of(&[DEFAULT_PRESET, ID]));
        // The short name points at the file; the file is chosen.
        let err = system_choose(&root, ID, "vi").unwrap_err();
        assert_eq!(err.message(), message::choose_by_file(ID));
        system_choose(&root, &source, "vi").unwrap();
        assert!(themes().is_empty());
        assert_eq!(json_of(&root, &p.style)["id"], ID);
        fs::remove_dir_all(&root).unwrap();
    }
}
