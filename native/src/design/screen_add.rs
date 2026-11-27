//! `dfw screen add`: one more screen in an existing design, placed next to its siblings.
//!
//! The screen gets an entry in `design/design.json` (after `--after` in its page and in the
//! screen list, else at the end of the page) and a skeleton file made from the screen template,
//! framed like its nearest sibling on the page (the `variant` and `current` of the first
//! neighbour's `app-shell`) and with the nearest blueprint layout pattern. The
//! screens that lead to it are only named: where a link goes is a design decision, so their files
//! are left as they are.
//!
//! Read, checks and both writes run under the project's edit lock (`design-files.md` D12); the
//! skeleton is made only under a free name (never replaces a file) and before the manifest, so a
//! failed manifest write never leaves an entry without its file. Refused while the design is
//! approved and locked (G2).

use std::path::Path;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::fixed_regex;
use crate::core::paths::{file, project_paths, rel, resolve_inside, ProjectPaths, SafeName};
use crate::core::state::{design_lock, field};
use crate::core::util::{create_first_free, read_json, write_json_inside};
use crate::design::compose::attr;
use crate::design::edit::edit_lock;
use crate::design::html::{attr_value, escape_text, set_attr, Splice, Token, Tokenizer};
use crate::design::manifest::{self, blueprint, key, malformed, Manifest};

/// The id of a screen this command adds: it names a folder of `design/screens/`
/// (`security-privacy.md` S3). No `_` first (a direction exploration has no page) and no `/`.
static SCREEN_ID: LazyLock<Regex> = LazyLock::new(|| fixed_regex(r"^[a-z0-9][a-z0-9-]*$"));

/// The variant the new screen starts with, and its file name.
const FIRST_VARIANT: &str = "a";
/// The label of [`FIRST_VARIANT`].
const FIRST_LABEL: &str = "A";
/// The screen template, in the templates folder.
const TEMPLATE: &str = "screen.html";

/// The parts of the template and of a screen the skeleton reads or sets.
mod part {
    /// A component instance.
    pub const USE: &str = "dfw-use";
    /// The master of an instance (the short form of `component`).
    pub const C: &str = "c";
    /// The master that frames every screen.
    pub const SHELL: &str = "app-shell";
    /// The master of the title bar.
    pub const HEADER: &str = "page-header";
    /// The navigation item the shell marks as the current one.
    pub const CURRENT: &str = "current";
    pub const VARIANT: &str = "variant";
    pub const TITLE: &str = "title";
    pub const HTML: &str = "html";
    pub const LANG: &str = "lang";
    /// The `data-dfw-id` of the template's page body.
    pub const BODY_ID: &str = "screen.body";
    /// The id suffixes of the header and the body of the new screen (`<screen>.header`).
    pub const HEADER_SUFFIX: &str = "header";
    pub const BODY_SUFFIX: &str = "body";
}

/// The keys of what `dfw screen add --json` prints.
pub mod out_key {
    pub const SCREEN: &str = "screen";
    pub const PAGE: &str = "page";
    /// The screen it follows on its page, or `null` at the end of the page.
    pub const AFTER: &str = "after";
    /// The skeleton file, project-relative.
    pub const FILE: &str = "file";
    /// The screen whose shell the skeleton copies, or `null`.
    pub const SHELL_FROM: &str = "shellFrom";
    /// The files that need a `data-dfw-goto` to the new screen, project-relative.
    pub const LINK_FROM: &str = "linkFrom";
}

/// What to add.
#[derive(Debug, Clone, Default)]
pub struct NewScreen {
    pub id: String,
    pub page: String,
    pub after: Option<String>,
    /// The screens that lead to it.
    pub from: Vec<String>,
    pub title: Option<String>,
    pub route: Option<String>,
}

/// What [`add_screen`] did.
#[derive(Debug, Clone, PartialEq)]
pub struct Added {
    pub screen: String,
    pub page: String,
    pub after: Option<String>,
    /// The skeleton, `design/screens/<id>/a.html`.
    pub file: String,
    pub shell_from: Option<String>,
    /// The files of the `from` screens, `design/...`.
    pub link_from: Vec<String>,
}

impl Added {
    pub fn to_json(&self) -> Value {
        json!({
            (out_key::SCREEN): self.screen,
            (out_key::PAGE): self.page,
            (out_key::AFTER): self.after,
            (out_key::FILE): self.file,
            (out_key::SHELL_FROM): self.shell_from,
            (out_key::LINK_FROM): self.link_from,
        })
    }
}

/// The refusal while the design is approved and locked.
fn frozen(version: &str) -> DfwError {
    DfwError::new(format!(
        "Design v{version} is approved and locked (G2); adding a screen changes the approved design. It needs the user's approval and \"dfw reopen --reason <why>\" first."
    ))
}

/// The ids of a list of screen ids (a page's `screens`), as text.
fn id_list(list: &[Value]) -> Vec<String> {
    list.iter().map(|v| js::to_string(Some(v))).collect()
}

/// Add the screen `new` to the project at `root`.
pub fn add_screen(root: &Path, new: &NewScreen) -> Result<Added, DfwError> {
    let Some(id) = SafeName::matching(&SCREEN_ID, &new.id) else {
        return Err(DfwError::new(format!(
            "Screen id \"{}\" is not valid: use lowercase letters, digits and dashes, starting with a letter or digit (for example order-review).",
            new.id
        )));
    };
    let p = project_paths(root);
    let _held = edit_lock(root)?;
    let mut m = open_manifest(&p)?;
    let known: Vec<String> = Manifest(&m).screens().map(|s| s.id_text()).collect();
    if known.iter().any(|k| k == id.as_str()) {
        return Err(DfwError::new(format!(
            "Screen \"{id}\" already exists in {}. Pick another id, or change that screen.",
            rel::MANIFEST
        )));
    }
    let page_index = find_page(&m, &new.page)?;
    let on_page = id_list(manifest::items_checked(
        m[key::PAGES][page_index].get(key::SCREENS),
        "\"screens\" of a page",
    )?);
    if let Some(after) = &new.after {
        if !on_page.contains(after) {
            return Err(DfwError::new(format!(
                "Screen \"{after}\" is not on page \"{}\" (its screens: {}). Give --after a screen of that page, or leave it out to add at the end.",
                new.page,
                names(&on_page)
            )));
        }
    }
    for from in &new.from {
        if !known.contains(from) {
            return Err(DfwError::new(format!(
                "--from names \"{from}\", which is no screen of {}.",
                rel::MANIFEST
            )));
        }
    }
    let dir = p.screens.join(id.as_str());
    let shown = rel::join(rel::SCREENS, &[id.as_str(), &page_file()].join("/"));
    if dir.join(page_file()).exists() {
        return Err(DfwError::new(format!(
            "{shown} already exists. Pick another id, or add that file to {} by hand.",
            rel::MANIFEST
        )));
    }
    let (shell, layout) =
        sibling_frame(&p.design, &m, &neighbours(&on_page, new.after.as_deref()))?;
    let link_from = link_files(&m, &new.from);
    let skeleton = skeleton(
        &crate::assets::template_text(TEMPLATE)?,
        id.as_str(),
        &Skeleton {
            title: new.title.as_deref().unwrap_or(id.as_str()),
            product: m.get(key::PRODUCT).and_then(Value::as_str),
            lang: m.get(key::LANG).and_then(Value::as_str),
            shell: shell.as_ref().map(|(_, shell)| shell),
        },
    );
    let entry = entry(id.as_str(), new, layout);
    insert_screen(
        &mut m,
        page_index,
        id.as_str(),
        new.after.as_deref(),
        &on_page,
        entry,
    );
    let name = SafeName::matching(&SCREEN_ID, FIRST_VARIANT);
    create_first_free(
        root,
        &dir,
        HTML_EXT,
        |n| if n == 0 { name.clone() } else { None },
        |_| skeleton.clone().into_bytes(),
    )?;
    write_json_inside(root, &p.manifest, &m)?;
    Ok(Added {
        screen: id.to_string(),
        page: new.page.clone(),
        after: new.after.clone(),
        file: shown,
        shell_from: shell.map(|(from, _)| from),
        link_from,
    })
}

/// The manifest, read under the edit lock: refused while the design is approved and locked
/// (a paused workflow does not lock), missing, or of the wrong shape.
fn open_manifest(p: &ProjectPaths) -> Result<Value, DfwError> {
    let state = read_json(&p.state, Some(Value::Null))?;
    if state.get(field::ACTIVE) != Some(&Value::Bool(false)) {
        if let Some(version) = design_lock(&state) {
            return Err(frozen(&version));
        }
    }
    if !p.manifest.is_file() {
        return Err(DfwError::new(format!(
            "{} is missing. Run \"dfw init\" first, then add the screen.",
            rel::MANIFEST
        )));
    }
    let m = read_json(&p.manifest, None)?;
    if !m.is_object() {
        return Err(malformed("the file must hold a JSON object"));
    }
    Manifest(&m).shape_checked()?;
    Ok(m)
}

/// The screens of the page nearest to the new one first: the screen it follows (`after`, else
/// the page's last), then the ones before it, then the ones after it.
fn neighbours(on_page: &[String], after: Option<&str>) -> Vec<String> {
    let at = after
        .and_then(|a| on_page.iter().position(|s| s == a))
        .unwrap_or(on_page.len().saturating_sub(1));
    let before = on_page.iter().take(at + 1).rev();
    before.chain(on_page.iter().skip(at + 1)).cloned().collect()
}

/// A shell and the id of the screen it was read from.
type Framed = (String, Shell);

/// What the new screen takes from its nearest neighbours: the shell of the first one whose first
/// variant file (read only inside `design/`) has an `app-shell`, with that screen's id, and the
/// first blueprint layout pattern.
fn sibling_frame(
    design: &Path,
    m: &Value,
    neighbours: &[String],
) -> Result<(Option<Framed>, Option<Value>), DfwError> {
    let mut shell = None;
    let mut layout = None;
    for id in neighbours {
        let Some(entry) = Manifest(m).screens().find(|x| &x.id_text() == id) else {
            continue;
        };
        if shell.is_none() {
            shell = entry
                .variants()
                .filter_map(|v| v.file())
                .find(|f| !f.is_empty())
                .and_then(|f| resolve_inside(design, f).ok())
                .filter(|abs| abs.is_file())
                .map(|abs| js::read_text(&abs))
                .transpose()?
                .and_then(|html| shell_of(&html))
                .map(|found| (id.clone(), found));
        }
        if layout.is_none() {
            layout = entry
                .value()
                .get(key::BLUEPRINT)
                .and_then(|b| b.get(blueprint::LAYOUT))
                .filter(|l| l.is_string())
                .cloned();
        }
    }
    Ok((shell, layout))
}

/// The extension of a screen file, without its dot.
const HTML_EXT: &str = "html";

/// The file name of the first variant (`a.html`).
fn page_file() -> String {
    [FIRST_VARIANT, HTML_EXT].join(".")
}

/// The ids as a message lists them.
fn names(ids: &[String]) -> String {
    if ids.is_empty() {
        "none".into()
    } else {
        ids.join(", ")
    }
}

/// The index of the page `page` in `pages`, or an error that lists the pages.
fn find_page(m: &Value, page: &str) -> Result<usize, DfwError> {
    let pages = manifest::items_checked(m.get(key::PAGES), "\"pages\"")?;
    if let Some(i) = pages
        .iter()
        .position(|p| p.get(key::ID).and_then(Value::as_str) == Some(page))
    {
        if !pages[i].is_object() {
            return Err(malformed("a page must be an object"));
        }
        return Ok(i);
    }
    let ids: Vec<String> = pages
        .iter()
        .filter_map(|p| p.get(key::ID).and_then(Value::as_str))
        .map(str::to_string)
        .collect();
    Err(DfwError::new(format!(
        "Page \"{page}\" is not in \"pages\" of {} (pages: {}). Use one of them, or add the page (id, title, description) to {} first, after the user agreed to it.",
        rel::MANIFEST,
        names(&ids),
        rel::MANIFEST
    )))
}

/// The design files of the screens `from`, project-relative.
fn link_files(m: &Value, from: &[String]) -> Vec<String> {
    let mut files = Vec::new();
    for s in Manifest(m)
        .screens()
        .filter(|s| from.contains(&s.id_text()))
    {
        for f in s
            .variants()
            .filter_map(|v| v.file())
            .filter(|f| !f.is_empty())
        {
            files.push(rel::design(f));
        }
    }
    files
}

/// The manifest entry of the new screen.
fn entry(id: &str, new: &NewScreen, layout: Option<Value>) -> Value {
    let mut e = Map::new();
    e.insert(key::ID.into(), json!(id));
    if let Some(title) = &new.title {
        e.insert(key::TITLE.into(), json!(title));
    }
    if let Some(route) = &new.route {
        e.insert(key::ROUTE.into(), json!(route));
    }
    let variant_file = [file::SCREENS, id, &page_file()].join("/");
    e.insert(
        key::VARIANTS.into(),
        json!([{ (key::ID): FIRST_VARIANT, (key::LABEL): FIRST_LABEL, (key::FILE): variant_file }]),
    );
    if let Some(layout) = layout {
        e.insert(
            key::BLUEPRINT.into(),
            json!({ (blueprint::LAYOUT): layout }),
        );
    }
    Value::Object(e)
}

/// Put `entry` in the screen list and `id` in the page: after `after`, else at the end of the
/// page. In the screen list it follows `after`, else the page's last screen, else it goes last.
fn insert_screen(
    m: &mut Value,
    page_index: usize,
    id: &str,
    after: Option<&str>,
    on_page: &[String],
    entry: Value,
) {
    let anchor = after.or_else(|| on_page.last().map(String::as_str));
    if let Some(Value::Object(page)) = m.get_mut(key::PAGES).and_then(|p| p.get_mut(page_index)) {
        let mut list: Vec<Value> = on_page.iter().map(|s| json!(s)).collect();
        let at = after
            .and_then(|a| on_page.iter().position(|s| s == a))
            .map_or(list.len(), |i| i + 1);
        list.insert(at, json!(id));
        page.insert(key::SCREENS.into(), Value::Array(list));
    }
    if !m.get(key::SCREENS).is_some_and(Value::is_array) {
        m[key::SCREENS] = json!([]);
    }
    let Some(Value::Array(screens)) = m.get_mut(key::SCREENS) else {
        return;
    };
    let at = anchor
        .and_then(|a| {
            screens
                .iter()
                .position(|s| s.get(key::ID).and_then(Value::as_str) == Some(a))
        })
        .map_or(screens.len(), |i| i + 1);
    screens.insert(at, entry);
}

/// The `variant` and `current` of the first `app-shell` instance of a screen.
#[derive(Debug, Clone, Default, PartialEq)]
struct Shell {
    variant: Option<String>,
    current: Option<String>,
}

fn shell_of(html: &str) -> Option<Shell> {
    Tokenizer::new(html).find_map(|token| {
        let Token::Open(tag) = token else { return None };
        let value = |name: &str| tag.attr(html, name).map(|a| attr_value(html, a));
        let is_shell = tag.is(html, part::USE)
            && (value(part::C).or_else(|| value(attr::COMPONENT))).as_deref() == Some(part::SHELL);
        is_shell.then(|| Shell {
            variant: value(part::VARIANT).filter(|v| !v.is_empty()),
            current: value(part::CURRENT).filter(|v| !v.is_empty()),
        })
    })
}

/// What the skeleton is filled with.
struct Skeleton<'a> {
    title: &'a str,
    product: Option<&'a str>,
    lang: Option<&'a str>,
    shell: Option<&'a Shell>,
}

/// The screen template with the new screen's title, language, ids and its sibling's shell, by
/// byte splices of the tags found with the tokenizer; every other byte as in the template.
fn skeleton(template: &str, id: &str, s: &Skeleton) -> String {
    let mut splices: Vec<Splice> = Vec::new();
    let mut tokens = Tokenizer::new(template);
    while let Some(token) = tokens.next() {
        let Token::Open(tag) = token else { continue };
        let value = |name: &str| tag.attr(template, name).map(|a| attr_value(template, a));
        let is_use =
            |master: &str| tag.is(template, part::USE) && value(part::C).as_deref() == Some(master);
        let mut sets: Vec<(&str, String)> = Vec::new();
        if tag.is(template, part::HTML) {
            if let Some(lang) = s.lang.filter(|l| !l.is_empty()) {
                sets.push((part::LANG, lang.to_string()));
            }
        } else if tag.is(template, part::TITLE) {
            if let Some(Token::Text(text)) = tokens.clone().next() {
                let title = match s.product.filter(|p| !p.is_empty()) {
                    Some(product) => format!("{} - {product}", s.title),
                    None => s.title.to_string(),
                };
                splices.push(Splice {
                    range: text,
                    text: escape_text(&title),
                });
            }
        } else if is_use(part::SHELL) {
            if let Some(shell) = s.shell {
                if let Some(current) = &shell.current {
                    sets.push((part::CURRENT, current.clone()));
                }
                if let Some(variant) = &shell.variant {
                    sets.push((part::VARIANT, variant.clone()));
                }
            }
        } else if is_use(part::HEADER) {
            sets.push((part::TITLE, s.title.to_string()));
            sets.push((attr::ID, [id, part::HEADER_SUFFIX].join(".")));
        } else if value(attr::DFW_ID).as_deref() == Some(part::BODY_ID) {
            sets.push((attr::DFW_ID, [id, part::BODY_SUFFIX].join(".")));
        }
        splices.extend(
            sets.iter()
                .filter_map(|(name, v)| set_attr(template, &tag, name, Some(v))),
        );
    }
    // From the end, so each range still points at the bytes it was computed on.
    splices.sort_by_key(|s| std::cmp::Reverse(s.range.start));
    splices
        .iter()
        .fold(template.to_string(), |text, splice| splice.apply(&text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::test_dir;
    use std::fs;

    const LIST: &str = "order-list";
    const DETAIL: &str = "order-detail";
    const REVIEW: &str = "order-review";
    const HISTORY: &str = "order-history";
    const BILLING: &str = "billing";
    const PAGE: &str = "orders";
    const OTHER_PAGE: &str = "settings";
    const ROUTE: &str = "/orders/review";
    /// What a refused id says.
    const NOT_VALID: &str = "not valid";
    const EMPTY_PAGE: &str = "<p></p>";
    const LIST_HTML: &str = "<!doctype html><html><body><dfw-use c=\"app-shell\" variant=\"counter\" current=\"orders\" id=\"shell\"></dfw-use></body></html>";

    /// The first variant file of the screen `id`, relative to `design/`.
    fn in_design(id: &str) -> String {
        [file::SCREENS, id, &page_file()].join("/")
    }

    /// The same, relative to the project.
    fn in_project(id: &str) -> String {
        rel::design(&in_design(id))
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let f = root.join(rel);
        fs::create_dir_all(f.parent().unwrap()).unwrap();
        fs::write(f, text).unwrap();
    }

    fn project() -> std::path::PathBuf {
        let root = test_dir("screen-add");
        write(&root, &in_project(LIST), LIST_HTML);
        write(&root, &in_project(DETAIL), EMPTY_PAGE);
        let m = json!({
            "schema": 1, "product": "Shop", "lang": "vi",
            "pages": [{ "id": PAGE, "title": "Orders", "screens": [LIST, DETAIL] }, { "id": OTHER_PAGE, "screens": [] }],
            "screens": [
                { "id": LIST, "variants": [{ "id": "a", "file": in_design(LIST) }], "blueprint": { "layout": "task" } },
                { "id": DETAIL, "variants": [{ "id": "a", "file": in_design(DETAIL) }] }
            ]
        });
        write(
            &root,
            rel::MANIFEST,
            &serde_json::to_string_pretty(&m).unwrap(),
        );
        root
    }

    fn manifest(root: &Path) -> Value {
        serde_json::from_str(&fs::read_to_string(root.join(rel::MANIFEST)).unwrap()).unwrap()
    }

    fn req(id: &str) -> NewScreen {
        NewScreen {
            id: id.into(),
            page: PAGE.into(),
            ..NewScreen::default()
        }
    }

    fn files(root: &Path) -> Vec<(String, Vec<u8>)> {
        let design = project_paths(root).design;
        let mut out: Vec<_> = crate::core::util::list_files(&design, &[])
            .unwrap()
            .into_iter()
            .map(|f| {
                let bytes = fs::read(design.join(&f)).unwrap();
                (f, bytes)
            })
            .collect();
        out.sort();
        out
    }

    fn page_screens(m: &Value, page: usize) -> Vec<String> {
        id_list(m["pages"][page]["screens"].as_array().unwrap())
    }

    fn screen_ids(m: &Value) -> Vec<String> {
        m["screens"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["id"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn a_screen_added_after_a_sibling_sits_between_it_and_the_next_one() {
        let root = project();
        let list_file = root.join(in_project(LIST));
        let list_before = fs::read(&list_file).unwrap();
        let added = add_screen(
            &root,
            &NewScreen {
                after: Some(LIST.into()),
                from: vec![LIST.into()],
                title: Some("Review <order> & pay".into()),
                route: Some(ROUTE.into()),
                ..req(REVIEW)
            },
        )
        .unwrap();
        let m = manifest(&root);
        assert_eq!(page_screens(&m, 0), [LIST, REVIEW, DETAIL]);
        assert_eq!(screen_ids(&m), [LIST, REVIEW, DETAIL]);
        let e = &m["screens"][1];
        assert_eq!(e["route"], ROUTE);
        assert_eq!(e["variants"][0]["file"], in_design(REVIEW));
        assert_eq!(
            e["blueprint"],
            json!({ "layout": "task" }),
            "the sibling's layout pattern"
        );
        assert_eq!(added.file, in_project(REVIEW));
        assert_eq!(added.link_from, [in_project(LIST)]);
        assert_eq!(added.shell_from.as_deref(), Some(LIST));
        let html = fs::read_to_string(root.join(&added.file)).unwrap();
        assert!(html.contains("<html lang=\"vi\">"), "{html}");
        assert!(
            html.contains("<title>Review &lt;order&gt; &amp; pay - Shop</title>"),
            "{html}"
        );
        assert!(
            html.contains("c=\"app-shell\" current=\"orders\" id=\"shell\" variant=\"counter\""),
            "{html}"
        );
        assert!(
            html.contains("title=\"Review &lt;order> &amp; pay\" id=\"order-review.header\""),
            "{html}"
        );
        assert!(html.contains("data-dfw-id=\"order-review.body\""), "{html}");
        // The screens that lead to it are named, not edited.
        assert_eq!(fs::read(&list_file).unwrap(), list_before);
    }

    #[test]
    fn without_after_the_screen_goes_to_the_end_of_its_page() {
        let root = project();
        let added = add_screen(&root, &req(HISTORY)).unwrap();
        let m = manifest(&root);
        assert_eq!(page_screens(&m, 0), [LIST, DETAIL, HISTORY]);
        assert_eq!(screen_ids(&m), [LIST, DETAIL, HISTORY]);
        assert_eq!(added.after, None);
        assert_eq!(
            added.shell_from.as_deref(),
            Some(LIST),
            "the nearest screen with a shell"
        );
        assert_eq!(
            m[key::SCREENS][2][key::BLUEPRINT],
            json!({ "layout": "task" })
        );
        // An empty page: first on it, last in the screen list.
        add_screen(
            &root,
            &NewScreen {
                page: OTHER_PAGE.into(),
                ..req(BILLING)
            },
        )
        .unwrap();
        let m = manifest(&root);
        assert_eq!(page_screens(&m, 1), [BILLING]);
        assert_eq!(screen_ids(&m)[3], BILLING);
        assert!(m[key::SCREENS][3].get(key::BLUEPRINT).is_none());
    }

    #[test]
    fn refused_input_writes_nothing() {
        let root = project();
        let before = files(&root);
        let cases: Vec<(NewScreen, &str)> = vec![
            (req("../escape"), NOT_VALID),
            (req("_dir"), NOT_VALID),
            (req("a/b"), NOT_VALID),
            (req("Order"), NOT_VALID),
            (req(""), NOT_VALID),
            (req(LIST), "already exists"),
            (
                NewScreen {
                    page: "nope".into(),
                    ..req(REVIEW)
                },
                "pages: orders, settings",
            ),
            (
                NewScreen {
                    page: OTHER_PAGE.into(),
                    after: Some(LIST.into()),
                    ..req(REVIEW)
                },
                "its screens: none",
            ),
            (
                NewScreen {
                    from: vec![BILLING.into()],
                    ..req(REVIEW)
                },
                "--from names \"billing\"",
            ),
        ];
        for (new, says) in cases {
            let err = add_screen(&root, &new).unwrap_err();
            assert!(err.message().contains(says), "{new:?}: {}", err.message());
            assert_eq!(files(&root), before, "{new:?}");
        }
        // A file already there under the new id.
        write(&root, &in_project(REVIEW), "mine");
        let before = files(&root);
        let err = add_screen(&root, &req(REVIEW)).unwrap_err();
        assert!(
            err.message()
                .contains(&format!("{} already exists", in_project(REVIEW))),
            "{}",
            err.message()
        );
        assert_eq!(files(&root), before);
    }

    #[test]
    fn a_locked_design_refuses_until_it_is_reopened() {
        let root = project();
        write(
            &root,
            rel::STATE,
            r#"{"active":true,"design":{"version":2,"locked":true}}"#,
        );
        let before = files(&root);
        let err = add_screen(&root, &req(REVIEW)).unwrap_err();
        assert!(
            err.message()
                .contains("Design v2 is approved and locked (G2)"),
            "{}",
            err.message()
        );
        assert!(err.message().contains("dfw reopen"));
        assert_eq!(files(&root), before);
        // After reopen the design is unlocked.
        write(
            &root,
            rel::STATE,
            r#"{"active":true,"design":{"version":3,"locked":false}}"#,
        );
        add_screen(&root, &req(REVIEW)).unwrap();
    }

    #[test]
    fn a_missing_or_malformed_manifest_is_refused() {
        let root = test_dir("screen-add-none");
        assert!(add_screen(&root, &req(REVIEW))
            .unwrap_err()
            .message()
            .contains("dfw init"));
        write(&root, rel::MANIFEST, r#"{"screens":[{"id":7}]}"#);
        let err = add_screen(&root, &req(REVIEW)).unwrap_err();
        assert!(err.message().contains("screens.0.id"), "{}", err.message());
        write(&root, rel::MANIFEST, r#"{"screens":[],"pages":{"x":1}}"#);
        assert!(add_screen(&root, &req(REVIEW)).is_err());
    }

    #[test]
    fn neighbours_are_the_nearest_screens_of_the_page_first() {
        let page: Vec<String> = ["a", "b", "c", "d"].map(String::from).to_vec();
        assert_eq!(neighbours(&page, Some("b")), ["b", "a", "c", "d"]);
        assert_eq!(neighbours(&page, None), ["d", "c", "b", "a"]);
        assert!(neighbours(&[], None).is_empty());
    }

    #[test]
    fn the_shell_is_read_with_the_tokenizer() {
        let html = "<!-- <dfw-use c=\"app-shell\" variant=\"old\"> --><dfw-use c='app-shell' current='items'>";
        assert_eq!(
            shell_of(html),
            Some(Shell {
                variant: None,
                current: Some("items".into())
            })
        );
        assert_eq!(shell_of(EMPTY_PAGE), None);
    }
}
