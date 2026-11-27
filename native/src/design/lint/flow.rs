//! The flow of the screens: whether each one is on a page and can be reached, and whether every
//! link between screens names a screen. A screen added midway that lands on no page, a link with
//! a typo in its target and a screen nothing leads to are silent in the Studio (a fallback page,
//! a missing arrow); these findings say so.
//!
//! - `manifest-unpaged-screen`: pages are declared and a public screen is on none of them.
//! - `link-unknown-screen`: a `data-dfw-goto`, or an `href` of an `<a>` into `screens/`, names
//!   no screen.
//! - `screen-unreachable`: the design has links, and a public screen on a page is not the first
//!   of its page, not marked `entry`, and no other screen links to it.

use std::path::Path;

use serde_json::Value;

use super::{finding, rule};
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::level::Level;
use crate::core::paths::{file, rel, resolve_inside};
use crate::design::compose::attr;
use crate::design::html::{attr_value, line_of, Token, Tokenizer};
use crate::design::manifest::{self, key, Screen};
use crate::design::pages::{declared_pages, goto_screen, linked_file};

/// A link from one screen to another, by screen id.
struct Link {
    from: String,
    to: String,
}

/// The findings about pages, links and reachability of the screens of `manifest` (already
/// shape-checked). Screen files are read only inside `design/`.
pub fn check_flow(design: &Path, manifest: &Value, out: &mut Vec<Value>) -> Result<(), DfwError> {
    let screens: Vec<Screen> = manifest::Manifest(manifest)
        .screens()
        // A screen without an id has its own finding (`manifest-screen-id`).
        .filter(|s| js::truthy(s.value().get(key::ID)) && !js::truthy(s.value().get(key::ARCHIVED)))
        .collect();
    let known: Vec<String> = manifest::Manifest(manifest)
        .screens()
        .map(Screen::id_text)
        .collect();
    let pages = declared_pages(manifest);
    let placed = |id: &str| pages.iter().flatten().any(|s| s.as_str() == Some(id));
    let declares_pages = manifest::items(manifest.get(key::PAGES))
        .iter()
        .any(|p| js::truthy(p.get(key::ID)));
    if declares_pages {
        for s in screens.iter().filter(|s| !s.is_private()) {
            let id = s.id_text();
            if !placed(&id) {
                out.push(on_manifest(rule::MANIFEST_UNPAGED_SCREEN, format!("Screen \"{id}\" is on no page of \"pages\"; the Studio shows it on a fallback \"Screens\" page, away from its module. Add its id to the \"screens\" of the page it belongs to, next to the screens it follows.")));
            }
        }
    }
    let links = read_links(design, manifest, &screens, &known, out)?;
    if links.is_empty() {
        return Ok(());
    }
    let first_of_page = |id: &str| {
        pages
            .iter()
            .any(|p| p.first().and_then(Value::as_str) == Some(id))
    };
    for s in screens.iter().filter(|s| !s.is_private()) {
        let id = s.id_text();
        let entry = s.value().get(key::ENTRY) == Some(&Value::Bool(true));
        if !placed(&id) || entry || first_of_page(&id) {
            continue;
        }
        if !links.iter().any(|l| l.to == id && l.from != id) {
            out.push(on_manifest(rule::SCREEN_UNREACHABLE, format!("Screen \"{id}\" is reached from no other screen (no data-dfw-goto or link to it) and is not the first screen of its page. Link it from the screens that lead to it, or mark it \"entry\": true when users reach it from outside the design.")));
        }
    }
    Ok(())
}

/// A finding on `design/design.json`, at line 1.
fn on_manifest(rule: &str, message: String) -> Value {
    finding(Level::Warn.as_str(), rule, rel::MANIFEST, 1, message)
}

/// The links between screens found in their files, reporting each link to a screen that does
/// not exist. A `data-dfw-goto` counts on any element (Present mode follows it there), an `href`
/// on an `<a>`.
fn read_links(
    design: &Path,
    manifest: &Value,
    screens: &[Screen],
    known: &[String],
    out: &mut Vec<Value>,
) -> Result<Vec<Link>, DfwError> {
    // Every variant file of the manifest, by its path relative to `design/`.
    let by_file: Vec<(&str, String)> = manifest::Manifest(manifest)
        .screens()
        .flat_map(|s| {
            s.variants()
                .filter_map(move |v| Some((v.file()?, s.id_text())))
        })
        .collect();
    let mut links = Vec::new();
    for s in screens {
        let from = s.id_text();
        for file_name in s
            .variants()
            .filter_map(|v| v.file())
            .filter(|f| !f.is_empty())
        {
            // design.json is user input: a file outside design/ is not read (rule 5).
            let Ok(abs) = resolve_inside(design, file_name) else {
                continue;
            };
            if !abs.is_file() {
                continue;
            }
            let html = js::read_text(&abs)?;
            let mut dir: Vec<&str> = file_name.split('/').collect();
            dir.pop();
            let shown = rel::design(file_name);
            for token in Tokenizer::new(&html) {
                let Token::Open(tag) = token else { continue };
                let line = || line_of(&html, tag.range.start);
                if let Some(goto) = tag.attr(&html, attr::GOTO).map(|a| attr_value(&html, a)) {
                    let to = goto_screen(goto.trim());
                    if to.is_empty() {
                        continue;
                    }
                    if !known.iter().any(|k| k == to) {
                        out.push(finding(Level::Warn.as_str(), rule::LINK_UNKNOWN_SCREEN, &shown, line(), format!("data-dfw-goto=\"{goto}\" names no screen of {}. Correct the id, or add the screen first (dfw screen add).", rel::MANIFEST)));
                        continue;
                    }
                    links.push(Link {
                        from: from.clone(),
                        to: to.to_string(),
                    });
                    continue;
                }
                if !tag.is(&html, "a") {
                    continue;
                }
                let Some(href) = tag.attr(&html, attr::HREF).map(|a| attr_value(&html, a)) else {
                    continue;
                };
                let Some(target) = linked_file(&dir, &href) else {
                    continue;
                };
                match by_file.iter().find(|(f, _)| *f == target) {
                    Some((_, to)) => links.push(Link {
                        from: from.clone(),
                        to: to.clone(),
                    }),
                    None if rel::under(file::SCREENS, &target) => {
                        out.push(finding(Level::Warn.as_str(), rule::LINK_UNKNOWN_SCREEN, &shown, line(), format!("href=\"{href}\" links to {}, which is the file of no screen of {}. Correct the link, or add the screen first (dfw screen add).", rel::design(&target), rel::MANIFEST)));
                    }
                    None => {}
                }
            }
        }
    }
    Ok(links)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::HTML_FILE;
    use crate::core::util::test_dir;
    use serde_json::json;
    use std::fs;

    const LIST: &str = "order-list";
    const DETAIL: &str = "order-detail";
    const REVIEW: &str = "order-review";
    const PAGE: &str = "orders";
    const EMPTY_PAGE: &str = "<p></p>";

    /// The design folder of a new project with `files` (relative to `design/`) written.
    fn design_with(files: &[(&str, &str)]) -> std::path::PathBuf {
        let design = test_dir("flow").join(file::DESIGN);
        for (name, text) in files {
            let f = design.join(name);
            fs::create_dir_all(f.parent().unwrap()).unwrap();
            fs::write(f, text).unwrap();
        }
        design
    }

    fn screen(id: &str) -> Value {
        json!({ "id": id, "variants": [{ "id": "a", "file": page_file(id) }] })
    }

    /// The first variant file of the screen `id`, relative to `design/`.
    fn page_file(id: &str) -> String {
        [file::SCREENS, id, HTML_FILE].join("/")
    }

    fn message(f: &Value) -> &str {
        f["message"].as_str().unwrap()
    }

    fn lint(design: &Path, m: &Value) -> Vec<(String, String)> {
        let mut out = Vec::new();
        check_flow(design, m, &mut out).unwrap();
        out.iter()
            .map(|f| {
                assert_eq!(f["level"], "warn");
                (
                    f["rule"].as_str().unwrap().to_string(),
                    message(f).to_string(),
                )
            })
            .collect()
    }

    fn rules(found: &[(String, String)]) -> Vec<&str> {
        found.iter().map(|(r, _)| r.as_str()).collect()
    }

    #[test]
    fn a_public_screen_on_no_declared_page_is_reported() {
        let design = design_with(&[]);
        let m = json!({
            "pages": [{ "id": PAGE, "screens": [LIST] }],
            "screens": [screen(LIST), screen(REVIEW), screen("_dir"), screen("own"), { "id": "gone", "archived": true }]
        });
        let mut m2 = m.clone();
        m2["screens"][3]["page"] = json!("settings");
        let found = lint(&design, &m2);
        assert_eq!(rules(&found), [rule::MANIFEST_UNPAGED_SCREEN], "{found:?}");
        assert!(found[0].1.contains(&format!("\"{REVIEW}\"")));
        // A screen listed only in a section is on the page.
        let m3 = json!({
            "pages": [{ "id": PAGE, "screens": [LIST], "sections": [{ "screens": [format!("{REVIEW}/b")] }] }],
            "screens": [screen(LIST), screen(REVIEW)]
        });
        assert!(lint(&design, &m3).is_empty());
        // No pages declared: the default "Screens" page is the design's choice.
        let m4 = json!({ "screens": [screen(LIST), screen(REVIEW)] });
        assert!(lint(&design, &m4).is_empty());
    }

    #[test]
    fn a_link_to_an_unknown_screen_is_reported_with_its_file_and_line() {
        let list = "<main>\n<!-- <a data-dfw-goto=\"old-screen\">x</a> -->\n<button data-dfw-goto='order-reveiw'>Review</button>\n<div data-dfw-goto=\"order-detail/b\"></div>\n<a href=\"../missing/a.html#top\">x</a>\n<a href=\"https://example.com/x.html\">x</a>\n<a href=\"../../system/index.html\">DS</a>\n</main>";
        let design = design_with(&[(&page_file(LIST), list), (&page_file(DETAIL), EMPTY_PAGE)]);
        let m = json!({ "screens": [screen(LIST), screen(DETAIL)] });
        let mut out = Vec::new();
        check_flow(&design, &m, &mut out).unwrap();
        let found: Vec<(&str, u64, &str)> = out
            .iter()
            .map(|f| {
                (
                    f["rule"].as_str().unwrap(),
                    f["line"].as_u64().unwrap(),
                    f["file"].as_str().unwrap(),
                )
            })
            .collect();
        let file = rel::design(&page_file(LIST));
        assert_eq!(
            found,
            [
                (rule::LINK_UNKNOWN_SCREEN, 3, file.as_str()),
                (rule::LINK_UNKNOWN_SCREEN, 5, file.as_str())
            ]
        );
        assert!(message(&out[0]).contains("order-reveiw"));
        assert!(message(&out[1]).contains(&rel::design(&page_file("missing"))));
        assert!(!out.iter().any(|f| message(f).contains("old-screen")));
    }

    #[test]
    fn a_screen_no_other_screen_links_to_is_unreachable_unless_it_is_an_entry() {
        let design = design_with(&[
            (&page_file(LIST), "<a href=\"../order-detail/a.html\">Open</a><a data-dfw-goto=\"order-list\">Self</a>"),
            (&page_file(DETAIL), EMPTY_PAGE),
            (&page_file(REVIEW), "<button data-dfw-goto=\"order-review\">Self</button>"),
        ]);
        let mut m = json!({
            "pages": [{ "id": PAGE, "screens": [LIST, DETAIL, REVIEW] }],
            "screens": [screen(LIST), screen(DETAIL), screen(REVIEW)]
        });
        let found = lint(&design, &m);
        assert_eq!(rules(&found), [rule::SCREEN_UNREACHABLE], "{found:?}");
        assert!(
            found[0].1.contains(&format!("\"{REVIEW}\"")),
            "a link to itself does not count"
        );
        m["screens"][2]["entry"] = json!(true);
        assert!(lint(&design, &m).is_empty(), "an entry");
    }

    #[test]
    fn an_unwired_design_gets_no_unreachable_finding() {
        let design = design_with(&[
            (&page_file(LIST), EMPTY_PAGE),
            (&page_file(DETAIL), EMPTY_PAGE),
        ]);
        let m = json!({
            "pages": [{ "id": PAGE, "screens": [LIST, DETAIL] }],
            "screens": [screen(LIST), screen(DETAIL)]
        });
        assert!(lint(&design, &m).is_empty());
    }

    #[test]
    fn a_variant_file_outside_design_is_not_read() {
        let design = design_with(&[(&page_file(LIST), EMPTY_PAGE)]);
        let outside = design.parent().unwrap().join("outside.html");
        fs::write(&outside, "<a data-dfw-goto=\"nowhere\">x</a>").unwrap();
        let m = json!({ "screens": [screen(LIST), { "id": "x", "variants": [{ "file": "../outside.html" }] }] });
        assert!(lint(&design, &m).is_empty());
    }
}
