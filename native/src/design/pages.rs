//! Ports `core/studio/pages.mjs`: groups manifest screens into canvas pages (Figma-style) and
//! finds the links between screens.
//! - "Design System" (id "system") always comes first.
//! - Declared `manifest.pages` `[{id,title,description,notes,sections,screens}]` plus `screen.page`.
//! - Unassigned screens go to "Screens"; ids starting with "_" go to "Explorations".

use crate::core::js;
use crate::design::manifest::{self, key};
use std::collections::HashMap;
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{json, Map, Value};

// The patterns of this file, compiled once: the Studio reads the flows on every manifest
// request. One that did not compile would be a typo the tests catch; callers then take their
// "no match" path.
static TAG: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new("<[^>]*>").ok());
static LINK_OPEN: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"(?i)<(a|button)(?-u:\b)([^>]*)>").ok());
static GOTO: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"data-dfw-goto="([^"]+)""#).ok());
static HREF: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r##"(?-u:\b)href="([^"#?]+\.html)[^"]*""##).ok());
static EXTERNAL: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(r"(?i)^[a-z]+:|^/").ok());

/// The ids of the pages the Studio makes itself.
pub(crate) mod page {
    /// The Design System page, always first; a declared page with this id only adds its
    /// description and notes.
    pub const SYSTEM: &str = "system";
    /// The page of the links between screens.
    pub const FLOW: &str = "flow";
    /// The page of the screens on no other page.
    pub const SCREENS: &str = "screens";
    /// The page of the screens whose id starts with `_`.
    pub const EXPLORATIONS: &str = "explorations";
}

fn screens_of(manifest: &Value) -> Vec<&Value> {
    manifest
        .get(key::SCREENS)
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .filter(|s| !js::truthy(s.get("archived")))
                .collect()
        })
        .unwrap_or_default()
}

fn variants_of(screen: &Value) -> &[Value] {
    screen
        .get(key::VARIANTS)
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
}

fn id_of(screen: &Value) -> Value {
    screen.get("id").cloned().unwrap_or(Value::Null)
}

/// The text of a link with tags turned into spaces, spaces collapsed, at most 40 UTF-16 units.
fn link_label(inner: &str) -> String {
    let text = match TAG.as_ref() {
        Some(tag) => tag.replace_all(inner, " ").into_owned(),
        None => inner.to_string(),
    };
    let mut out = String::new();
    let mut space = false;
    for c in text.chars() {
        if crate::core::js::is_space(c) {
            space = true;
        } else {
            if space && !out.is_empty() {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    let mut units = 0;
    out.chars()
        .take_while(|c| {
            units += c.len_utf16();
            units <= 40
        })
        .collect()
}

/// Every `<a>`/`<button>` element of `html` with its attributes and inner text, as the
/// regular expression `<(a|button)\b([^>]*)>([\s\S]*?)<\/\1>`/gi of pages.mjs finds them (the
/// back reference is replaced by a search for the matching end tag).
fn links(html: &str) -> Vec<(&str, &str)> {
    let Some(open) = LINK_OPEN.as_ref() else {
        return Vec::new();
    };
    let lower = html.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut at = 0;
    while let Some(m) = open.captures_at(html, at) {
        let (Some(all), Some(name), Some(attrs)) = (m.get(0), m.get(1), m.get(2)) else {
            break;
        };
        let close = format!("</{}>", name.as_str().to_ascii_lowercase());
        match lower[all.end()..].find(&close) {
            Some(i) => {
                out.push((attrs.as_str(), &html[all.end()..all.end() + i]));
                at = all.end() + i + close.len();
            }
            None => {
                // No end tag: the match fails here and the search goes on one character later.
                at = all.start() + html[all.start()..].chars().next().map_or(1, char::len_utf8);
            }
        }
        if at > html.len() {
            break;
        }
    }
    out
}

/// `screenFlows(manifest, files)`: screen-to-screen links found in the design files
/// (`data-dfw-goto="screen[/variant]"` and `href` to another design file). `files` maps a
/// design-relative file to its HTML. Returns `[{ from, to, label }]` (screen ids).
pub fn screen_flows(manifest: &Value, files: &HashMap<String, String>) -> Vec<Value> {
    let (Some(goto_re), Some(href_re), Some(external)) =
        (GOTO.as_ref(), HREF.as_ref(), EXTERNAL.as_ref())
    else {
        return Vec::new();
    };
    let screens = screens_of(manifest);
    let mut by_file: HashMap<String, Value> = HashMap::new();
    for s in &screens {
        for v in variants_of(s) {
            by_file.insert(js::to_string(v.get("file")), id_of(s));
        }
    }
    let ids: Vec<Value> = screens.iter().map(|s| id_of(s)).collect();
    let mut edges: Vec<(String, Value)> = Vec::new();
    for s in &screens {
        let sid = id_of(s);
        for v in variants_of(s) {
            let Some(file) = v.get("file").and_then(Value::as_str) else {
                continue;
            };
            let Some(html) = files.get(file).filter(|h| !h.is_empty()) else {
                continue;
            };
            let mut dir: Vec<&str> = file.split('/').collect();
            dir.pop();
            for (attrs, inner) in links(html) {
                let goto = goto_re.captures(attrs).and_then(|c| c.get(1));
                let href = href_re.captures(attrs).and_then(|c| c.get(1));
                let mut to: Option<Value> =
                    goto.map(|g| json!(g.as_str().split('/').next().unwrap_or_default()));
                if to.is_none() {
                    if let Some(href) = href.filter(|h| !external.is_match(h.as_str())) {
                        let mut parts = dir.clone();
                        for seg in href.as_str().split('/') {
                            if seg == ".." {
                                parts.pop();
                            } else if seg != "." {
                                parts.push(seg);
                            }
                        }
                        to = by_file.get(&parts.join("/")).cloned();
                    }
                }
                let Some(to) = to else { continue };
                if to == sid || !ids.contains(&to) {
                    continue;
                }
                let key = format!("{}>{}", js::to_string(Some(&sid)), js::to_string(Some(&to)));
                if !edges.iter().any(|(k, _)| *k == key) {
                    edges.push((
                        key,
                        json!({ "from": sid, "to": to, "label": link_label(inner) }),
                    ));
                }
            }
        }
    }
    edges.into_iter().map(|(_, e)| e).collect()
}

struct Page {
    map: Map<String, Value>,
    screens: Vec<Value>,
}

/// The pages being built, in the order they are shown, and the screens already placed.
struct Pages<'a> {
    /// The ids of the screens of the manifest: only these are placed.
    by_id: &'a [Value],
    pages: Vec<Page>,
    assigned: Vec<Value>,
}

impl Pages<'_> {
    /// The index of the page `id`, made (with `title`, else the id) when there is none yet.
    fn ensure(&mut self, id: &Value, title: Option<&Value>) -> usize {
        if let Some(i) = self.pages.iter().position(|p| p.map.get("id") == Some(id)) {
            return i;
        }
        let title = title
            .filter(|t| js::truthy(Some(t)))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let mut map = Map::new();
        map.insert("id".into(), id.clone());
        map.insert(key::TITLE.into(), title);
        map.insert(key::SCREENS.into(), Value::Null);
        self.pages.push(Page {
            map,
            screens: Vec::new(),
        });
        self.pages.len() - 1
    }

    /// Places a known screen on a page, once per page.
    fn add(&mut self, page: usize, sid: &Value) {
        if !self.by_id.contains(sid) || self.pages[page].screens.contains(sid) {
            return;
        }
        self.pages[page].screens.push(sid.clone());
        self.assigned.push(sid.clone());
    }
}

/// `studioPages(manifest)`: the canvas pages, Design System first.
pub fn studio_pages(manifest: &Value) -> Vec<Value> {
    let screens = screens_of(manifest);
    let by_id: Vec<Value> = screens.iter().map(|s| id_of(s)).collect();
    let mut pages = Pages {
        by_id: &by_id,
        pages: Vec::new(),
        assigned: Vec::new(),
    };
    let system_extra = place_declared_pages(manifest, &mut pages);
    place_by_screen_page(&screens, &mut pages);
    place_unassigned(&screens, &mut pages);
    let flow_screens = flow_screens(manifest, &by_id);
    page_list(system_extra, flow_screens, pages.pages)
}

/// The pages of `manifest.pages` with their screens (listed or referenced from sections), in
/// their order. Returns the description and notes declared for the Design System page.
fn place_declared_pages(manifest: &Value, pages: &mut Pages<'_>) -> Map<String, Value> {
    let mut system_extra = Map::new();
    let declared = manifest
        .get(key::PAGES)
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for p in &declared {
        if !js::truthy(Some(p)) || !js::truthy(p.get("id")) {
            continue;
        }
        let pid = p.get("id").cloned().unwrap_or(Value::Null);
        if pid == json!(page::SYSTEM) {
            for key in ["description", "notes"] {
                if let Some(v) = p.get(key) {
                    system_extra.insert(key.into(), v.clone());
                }
            }
            continue;
        }
        let page = pages.ensure(&pid, p.get(key::TITLE));
        for key in [key::DESCRIPTION, key::NOTES, key::SECTIONS] {
            if let Some(v) = p.get(key).filter(|v| js::truthy(Some(v))) {
                pages.pages[page].map.insert(key.into(), v.clone());
            }
        }
        for sid in manifest::items(p.get(key::SCREENS)).iter() {
            pages.add(page, sid);
        }
        // Screens referenced only from sections ("id" or "id/variant") belong to the page too.
        for sec in manifest::items(p.get(key::SECTIONS)).iter() {
            for r in manifest::items(sec.get(key::SCREENS)).iter() {
                let text = js::to_string(Some(r));
                let id = json!(text.split('/').next().unwrap_or_default());
                pages.add(page, &id);
            }
        }
    }
    system_extra
}

/// Screens that name their page with `screen.page` (never the Design System page).
fn place_by_screen_page(screens: &[&Value], pages: &mut Pages<'_>) {
    for s in screens {
        if let Some(page_id) = s
            .get("page")
            .filter(|p| js::truthy(Some(p)) && **p != json!(page::SYSTEM))
        {
            let page = pages.ensure(page_id, Some(page_id));
            pages.add(page, &id_of(s));
        }
    }
}

/// The screens on no page yet: to "Screens", ids starting with "_" to "Explorations". An
/// empty "Screens" page when there would be no page at all.
fn place_unassigned(screens: &[&Value], pages: &mut Pages<'_>) {
    let starts_with_underscore = |s: &Value| {
        s.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with('_'))
    };
    let rest: Vec<&Value> = screens
        .iter()
        .copied()
        .filter(|s| !pages.assigned.contains(&id_of(s)))
        .collect();
    let main: Vec<&Value> = rest
        .iter()
        .copied()
        .filter(|s| !starts_with_underscore(s))
        .collect();
    let explore: Vec<&Value> = rest
        .iter()
        .copied()
        .filter(|s| starts_with_underscore(s))
        .collect();
    if !main.is_empty() || pages.pages.is_empty() {
        for s in &main {
            let page = pages.ensure(&json!(page::SCREENS), Some(&json!("Screens")));
            pages.add(page, &id_of(s));
        }
    }
    if pages.pages.is_empty() && main.is_empty() && explore.is_empty() {
        pages.ensure(&json!(page::SCREENS), Some(&json!("Screens")));
    }
    for s in &explore {
        let page = pages.ensure(&json!(page::EXPLORATIONS), Some(&json!("Explorations")));
        pages.add(page, &id_of(s));
    }
}

/// The known screens named by `manifest.flows` (`from`, then `to`), each once.
fn flow_screens(manifest: &Value, by_id: &[Value]) -> Vec<Value> {
    let mut flow_screens: Vec<Value> = Vec::new();
    for e in manifest::items(manifest.get(key::FLOWS)).iter() {
        for key in ["from", "to"] {
            let id = e.get(key).cloned().unwrap_or(Value::Null);
            if !flow_screens.contains(&id) {
                flow_screens.push(id);
            }
        }
    }
    flow_screens.retain(|id| by_id.contains(id));
    flow_screens
}

/// The pages in the order shown: Design System, Flow (when flows name screens), the others.
fn page_list(
    system_extra: Map<String, Value>,
    flow_screens: Vec<Value>,
    pages: Vec<Page>,
) -> Vec<Value> {
    let mut system = Map::new();
    system.insert(key::ID.into(), json!(page::SYSTEM));
    system.insert(key::TITLE.into(), json!("Design System"));
    system.insert("system".into(), json!(true));
    system.insert(key::SCREENS.into(), json!([]));
    for (k, v) in system_extra {
        system.insert(k, v);
    }
    let mut out = vec![Value::Object(system)];
    if !flow_screens.is_empty() {
        out.push(json!({
            "id": page::FLOW,
            "title": "Flow",
            "flow": true,
            "description": "How screens link to each other (buttons and links with data-dfw-goto or href). Arrows carry the control text.",
            "screens": flow_screens,
        }));
    }
    for Page { mut map, screens } in pages {
        map.insert(key::SCREENS.into(), Value::Array(screens));
        out.push(Value::Object(map));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::{screen_page, HTML_FILE};
    use crate::core::paths::{file, rel};

    /// A screen of the manifest kept for a direction exploration.
    const EXPLORATION: &str = "_dir";

    fn ids(pages: &[Value]) -> Vec<&str> {
        pages.iter().map(|p| p[key::ID].as_str().unwrap()).collect()
    }

    /// studio pages: system first, declared pages, Screens + Explorations fallback
    #[test]
    fn studio_pages_system_first_declared_pages_and_fallbacks() {
        let screens = json!([{ "id": "a", "variants": [] }, { "id": "b", "page": page::FLOW, "variants": [] }, { "id": EXPLORATION, "variants": [] }, { "id": "c", "variants": [] }]);
        let p1 = studio_pages(&json!({ "screens": screens }));
        let pairs: Vec<(Value, Value)> = p1
            .iter()
            .map(|p| (p[key::ID].clone(), p[key::SCREENS].clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                (json!(page::SYSTEM), json!([])),
                (json!(page::FLOW), json!(["b"])),
                (json!(page::SCREENS), json!(["a", "c"])),
                (json!(page::EXPLORATIONS), json!([EXPLORATION]))
            ]
        );
        /// A page the manifest declares, and its title.
        const MAIN: &str = "main";
        const MAIN_TITLE: &str = "Main";
        let p2 = studio_pages(
            &json!({ "screens": screens, "pages": [{ "id": MAIN, "title": MAIN_TITLE, "screens": ["c", "a"] }] }),
        );
        let triples: Vec<Value> = p2
            .iter()
            .map(|p| json!([p[key::ID], p[key::TITLE], p[key::SCREENS]]))
            .collect();
        assert_eq!(
            triples,
            vec![
                json!([page::SYSTEM, "Design System", []]),
                json!([MAIN, MAIN_TITLE, ["c", "a"]]),
                json!([page::FLOW, page::FLOW, ["b"]]),
                json!([page::EXPLORATIONS, "Explorations", [EXPLORATION]])
            ]
        );
        assert_eq!(
            ids(&studio_pages(&json!({ "screens": [] }))),
            [page::SYSTEM, page::SCREENS]
        );
        // Page metadata and section-only screen refs ("id/variant") pass through.
        let notes = json!([{ "title": "Frame", "items": ["Rail 72"] }]);
        let sections = json!([{ "kind": "states", "screens": ["c/x"], "states": ["empty"] }]);
        /// The description of a declared page.
        const DESCRIPTION: &str = "Desc";
        let p3 = studio_pages(
            &json!({ "screens": screens, "pages": [{ "id": page::SYSTEM, "notes": notes }, { "id": "m", "title": "M", "description": DESCRIPTION, "notes": notes, "sections": sections }] }),
        );
        assert_eq!(p3[0][key::NOTES], notes, "system page notes");
        assert_eq!(p3[1][key::DESCRIPTION], DESCRIPTION);
        assert_eq!(p3[1][key::SECTIONS], sections);
        assert_eq!(
            p3[1][key::SCREENS],
            json!(["c"]),
            "screen referenced only from a section belongs to the page"
        );
        // Key order of a page as Node builds it.
        let keys: Vec<&String> = p3[1].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                key::ID,
                key::TITLE,
                key::SCREENS,
                key::DESCRIPTION,
                key::NOTES,
                key::SECTIONS
            ]
        );
    }

    /// studio: screen flows from data-dfw-goto and links; Flow page only when there are links
    #[test]
    fn screen_flows_from_goto_and_links() {
        /// A list screen that links to a detail screen, which goes back to it.
        const LIST: &str = "list";
        const DETAIL: &str = "detail";
        let page_file = |id: &str| rel::join(file::SCREENS, &screen_page(id));
        let (list, detail) = (page_file(LIST), page_file(DETAIL));
        let mut m = json!({ "screens": [{ "id": LIST, "variants": [{ "id": "a", "file": list }] }, { "id": DETAIL, "variants": [{ "id": "a", "file": detail }] }] });
        let files: HashMap<String, String> = [
            (list, format!(r#"<a href="../{DETAIL}/{HTML_FILE}">Open <b>drill</b></a><a href="https://example.com/x.html">ext</a>"#)),
            (detail, format!(r#"<button data-dfw-goto="{LIST}">Back to list</button><button data-dfw-goto="nope">x</button>"#)),
        ]
        .into_iter()
        .collect();
        let flows = screen_flows(&m, &files);
        assert_eq!(
            Value::Array(flows.clone()),
            json!([{ "from": LIST, "to": DETAIL, "label": "Open drill" }, { "from": DETAIL, "to": LIST, "label": "Back to list" }])
        );
        m[key::FLOWS] = Value::Array(flows);
        assert_eq!(
            ids(&studio_pages(&m)),
            [page::SYSTEM, page::FLOW, page::SCREENS]
        );
        m[key::FLOWS] = json!([]);
        assert!(!studio_pages(&m).iter().any(|p| p.get(page::FLOW).is_some()));
    }

    #[test]
    fn links_need_their_own_end_tag() {
        let found = links("<a x>1</A><button>2<a>3</a></button><a>open");
        assert_eq!(found, vec![(" x", "1"), ("", "2<a>3</a>")]);
        assert_eq!(link_label("  a <b>b</b>\n\n c  "), "a b c");
        assert_eq!(link_label(&"x".repeat(50)).len(), 40);
    }
}
