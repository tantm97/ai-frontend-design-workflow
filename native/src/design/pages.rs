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

/// `studioPages(manifest)`: the canvas pages, Design System first.
// Over 120 lines: the page order rules (declared pages, `screen.page`, Screens,
// Explorations, Flow) read in one sequence, the order the pages are shown in.
#[allow(clippy::too_many_lines, reason = "A10 debt")]
pub fn studio_pages(manifest: &Value) -> Vec<Value> {
    let screens = screens_of(manifest);
    let by_id: Vec<Value> = screens.iter().map(|s| id_of(s)).collect();
    let mut pages: Vec<Page> = Vec::new();
    let mut assigned: Vec<Value> = Vec::new();
    let mut system_extra = Map::new();

    fn ensure(pages: &mut Vec<Page>, id: &Value, title: Option<&Value>) -> usize {
        if let Some(i) = pages.iter().position(|p| p.map.get("id") == Some(id)) {
            return i;
        }
        let title = title
            .filter(|t| js::truthy(Some(t)))
            .cloned()
            .unwrap_or_else(|| id.clone());
        let mut map = Map::new();
        map.insert("id".into(), id.clone());
        map.insert("title".into(), title);
        map.insert(key::SCREENS.into(), Value::Null);
        pages.push(Page {
            map,
            screens: Vec::new(),
        });
        pages.len() - 1
    }
    let add = |pages: &mut Vec<Page>, assigned: &mut Vec<Value>, page: usize, sid: &Value| {
        if !by_id.contains(sid) || pages[page].screens.contains(sid) {
            return;
        }
        pages[page].screens.push(sid.clone());
        assigned.push(sid.clone());
    };

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
        if pid == json!("system") {
            for key in ["description", "notes"] {
                if let Some(v) = p.get(key) {
                    system_extra.insert(key.into(), v.clone());
                }
            }
            continue;
        }
        let page = ensure(&mut pages, &pid, p.get("title"));
        for key in [key::DESCRIPTION, key::NOTES, key::SECTIONS] {
            if let Some(v) = p.get(key).filter(|v| js::truthy(Some(v))) {
                pages[page].map.insert(key.into(), v.clone());
            }
        }
        for sid in manifest::items(p.get(key::SCREENS)).iter() {
            add(&mut pages, &mut assigned, page, sid);
        }
        // Screens referenced only from sections ("id" or "id/variant") belong to the page too.
        for sec in manifest::items(p.get(key::SECTIONS)).iter() {
            for r in manifest::items(sec.get(key::SCREENS)).iter() {
                let text = js::to_string(Some(r));
                let id = json!(text.split('/').next().unwrap_or_default());
                add(&mut pages, &mut assigned, page, &id);
            }
        }
    }
    for s in &screens {
        if let Some(page_id) = s
            .get("page")
            .filter(|p| js::truthy(Some(p)) && **p != json!("system"))
        {
            let page = ensure(&mut pages, page_id, Some(page_id));
            add(&mut pages, &mut assigned, page, &id_of(s));
        }
    }
    let starts_with_underscore = |s: &Value| {
        s.get("id")
            .and_then(Value::as_str)
            .is_some_and(|id| id.starts_with('_'))
    };
    let rest: Vec<&Value> = screens
        .iter()
        .copied()
        .filter(|s| !assigned.contains(&id_of(s)))
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
    if !main.is_empty() || pages.is_empty() {
        for s in &main {
            let page = ensure(&mut pages, &json!("screens"), Some(&json!("Screens")));
            add(&mut pages, &mut assigned, page, &id_of(s));
        }
    }
    if pages.is_empty() && main.is_empty() && explore.is_empty() {
        ensure(&mut pages, &json!("screens"), Some(&json!("Screens")));
    }
    for s in &explore {
        let page = ensure(
            &mut pages,
            &json!("explorations"),
            Some(&json!("Explorations")),
        );
        add(&mut pages, &mut assigned, page, &id_of(s));
    }
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

    let mut system = Map::new();
    system.insert("id".into(), json!("system"));
    system.insert("title".into(), json!("Design System"));
    system.insert("system".into(), json!(true));
    system.insert(key::SCREENS.into(), json!([]));
    for (k, v) in system_extra {
        system.insert(k, v);
    }
    let mut out = vec![Value::Object(system)];
    if !flow_screens.is_empty() {
        out.push(json!({
            "id": "flow",
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

    fn ids(pages: &[Value]) -> Vec<&str> {
        pages.iter().map(|p| p["id"].as_str().unwrap()).collect()
    }

    /// studio pages: system first, declared pages, Screens + Explorations fallback
    #[test]
    fn studio_pages_system_first_declared_pages_and_fallbacks() {
        let screens = json!([{ "id": "a", "variants": [] }, { "id": "b", "page": "flow", "variants": [] }, { "id": "_dir", "variants": [] }, { "id": "c", "variants": [] }]);
        let p1 = studio_pages(&json!({ "screens": screens }));
        let pairs: Vec<(Value, Value)> = p1
            .iter()
            .map(|p| (p["id"].clone(), p["screens"].clone()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                (json!("system"), json!([])),
                (json!("flow"), json!(["b"])),
                (json!("screens"), json!(["a", "c"])),
                (json!("explorations"), json!(["_dir"]))
            ]
        );
        let p2 = studio_pages(
            &json!({ "screens": screens, "pages": [{ "id": "main", "title": "Main", "screens": ["c", "a"] }] }),
        );
        let triples: Vec<Value> = p2
            .iter()
            .map(|p| json!([p["id"], p["title"], p["screens"]]))
            .collect();
        assert_eq!(
            triples,
            vec![
                json!(["system", "Design System", []]),
                json!(["main", "Main", ["c", "a"]]),
                json!(["flow", "flow", ["b"]]),
                json!(["explorations", "Explorations", ["_dir"]])
            ]
        );
        assert_eq!(
            ids(&studio_pages(&json!({ "screens": [] }))),
            ["system", "screens"]
        );
        // Page metadata and section-only screen refs ("id/variant") pass through.
        let notes = json!([{ "title": "Frame", "items": ["Rail 72"] }]);
        let sections = json!([{ "kind": "states", "screens": ["c/x"], "states": ["empty"] }]);
        let p3 = studio_pages(
            &json!({ "screens": screens, "pages": [{ "id": "system", "notes": notes }, { "id": "m", "title": "M", "description": "Desc", "notes": notes, "sections": sections }] }),
        );
        assert_eq!(p3[0]["notes"], notes, "system page notes");
        assert_eq!(p3[1]["description"], "Desc");
        assert_eq!(p3[1]["sections"], sections);
        assert_eq!(
            p3[1]["screens"],
            json!(["c"]),
            "screen referenced only from a section belongs to the page"
        );
        // Key order of a page as Node builds it.
        let keys: Vec<&String> = p3[1].as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            ["id", "title", "screens", "description", "notes", "sections"]
        );
    }

    /// studio: screen flows from data-dfw-goto and links; Flow page only when there are links
    #[test]
    fn screen_flows_from_goto_and_links() {
        let mut m = json!({ "screens": [{ "id": "list", "variants": [{ "id": "a", "file": "screens/list/a.html" }] }, { "id": "detail", "variants": [{ "id": "a", "file": "screens/detail/a.html" }] }] });
        let files: HashMap<String, String> = [
            ("screens/list/a.html", r#"<a href="../detail/a.html">Open <b>drill</b></a><a href="https://example.com/x.html">ext</a>"#),
            ("screens/detail/a.html", r#"<button data-dfw-goto="list">Back to list</button><button data-dfw-goto="nope">x</button>"#),
        ]
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        let flows = screen_flows(&m, &files);
        assert_eq!(
            Value::Array(flows.clone()),
            json!([{ "from": "list", "to": "detail", "label": "Open drill" }, { "from": "detail", "to": "list", "label": "Back to list" }])
        );
        m["flows"] = Value::Array(flows);
        assert_eq!(ids(&studio_pages(&m)), ["system", "flow", "screens"]);
        m["flows"] = json!([]);
        assert!(!studio_pages(&m).iter().any(|p| p.get("flow").is_some()));
    }

    #[test]
    fn links_need_their_own_end_tag() {
        let found = links("<a x>1</A><button>2<a>3</a></button><a>open");
        assert_eq!(found, vec![(" x", "1"), ("", "2<a>3</a>")]);
        assert_eq!(link_label("  a <b>b</b>\n\n c  "), "a b c");
        assert_eq!(link_label(&"x".repeat(50)).len(), 40);
    }
}
