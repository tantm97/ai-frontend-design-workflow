//! The export cases of tests/unit/penpot-screens.test.mjs and penpot-hardening.test.mjs, and
//! the fixtures the other Penpot tests share.

use super::*;
use crate::core::js;
use crate::core::util::{copy_dir, list_files, test_dir};
use crate::design::tokens::build_tokens;

pub(in crate::penpot) fn fix() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/penpot")
}

pub(in crate::penpot) fn read(name: &str) -> Value {
    js::read_json(&fix().join(name), None).unwrap()
}

/// A copy of the fixture project with a workflow state (`design` is its design block) and
/// tokens.css built.
pub(in crate::penpot) fn project(label: &str, design: Value) -> PathBuf {
    let root = test_dir(label);
    copy_dir(&fix().join("project"), &root, &[]).unwrap();
    fs::create_dir_all(root.join(".dfw")).unwrap();
    fs::write(
        root.join(".dfw/state.json"),
        to_json_compact(&json!({ "design": design })),
    )
    .unwrap();
    build_tokens(&root).unwrap();
    root
}

/// The state of a project that is not frozen.
pub(in crate::penpot) fn draft() -> Value {
    json!({ "version": 1, "status": "draft", "locked": false })
}

pub(in crate::penpot) fn fixture_export_at(generated_at: &str) -> Value {
    let tokens = DfwTokens {
        base: read("project/design/tokens/tokens.json"),
        ..DfwTokens::default()
    };
    let renders = read("renders.json");
    build_export(&ExportInput {
        renders: renders.as_array().unwrap(),
        tokens: Some(&tokens),
        product: "Fixture".into(),
        design_version: "v1".into(),
        project_root: None,
        generated_at: generated_at.into(),
    })
}

pub(in crate::penpot) fn fixture_export() -> Value {
    fixture_export_at("2026-11-16T10:00:00.000Z")
}

/// Put an export in a project as if "dfw penpot export" had written it; returns the change
/// file (a fixture) for it.
pub(in crate::penpot) fn with_export(root: &Path, doc: &Value, changes: &str) -> Value {
    let dir = penpot_dir(root);
    fs::create_dir_all(dir.join("exports")).unwrap();
    let id = doc["id"].as_str().unwrap();
    fs::write(
        dir.join("exports").join(format!("{id}.json")),
        to_json_compact(doc),
    )
    .unwrap();
    fs::write(dir.join("export.json"), to_json_compact(doc)).unwrap();
    let mut changes = read(changes);
    changes["exportId"] = id.into();
    changes
}

/// Every file of `design/` with its hash: nothing there may change in an import.
pub(in crate::penpot) fn design_hash(root: &Path) -> String {
    let design = root.join("design");
    list_files(&design, &[])
        .unwrap()
        .iter()
        .map(|f| format!("{f}:{}", sha256(fs::read(design.join(f)).unwrap())))
        .collect::<Vec<_>>()
        .join("\n")
}

fn structure_fixture() -> Vec<String> {
    fs::read_to_string(fix().join("export.structure.txt"))
        .unwrap()
        .trim_end()
        .split('\n')
        .map(str::to_string)
        .collect()
}

/// penpot export: structure snapshot (boards per screen x breakpoint, ids as layer names, tokens, components)
#[test]
fn structure_snapshot() {
    let doc = fixture_export();
    assert_eq!(doc["schema"], "dfw-penpot-export/1");
    let id = doc["id"].as_str().unwrap();
    assert!(
        id.starts_with("px_20261116100000_") && id.len() == 26,
        "{id}"
    );
    assert!(id[18..].bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(export_structure(&doc), structure_fixture());
    // Tokens travel with the export in Penpot's format; colours keep their token names.
    let keys: Vec<&String> = doc["tokens"].as_object().unwrap().keys().collect();
    assert_eq!(keys, ["base", "$themes", "$metadata"]);
    assert!(doc["colors"]
        .as_array()
        .unwrap()
        .iter()
        .any(|c| c["name"] == "color.accent" && c["hex"] == "#0066ff"));
    // Instances share their master's structure, so the plugin can use component instances.
    let layers = doc["pages"][0]["boards"][0]["layers"][0]["children"]
        .as_array()
        .unwrap();
    let add = layers.iter().find(|l| l["name"] == "items.add").unwrap();
    let main = doc["components"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["key"] == add["component"]["key"])
        .unwrap();
    assert_eq!(add["sig"], main["sig"]);
    let root = &doc["components"][0]["root"];
    assert_eq!(
        (js::to_number(root.get("x")), js::to_number(root.get("y"))),
        (0.0, 0.0)
    );
}

/// penpot export: layout wrappers are hoisted, signatures ignore text
#[test]
fn layout_wrappers_are_hoisted_signatures_ignore_text() {
    let tree = json!([{ "kind": "frame", "name": "div.wrap", "style": {}, "children": [{ "kind": "text", "name": "a", "characters": "x", "style": {} }, { "kind": "frame", "name": "div.empty", "style": {}, "children": [] }] }]);
    let names: Vec<Value> = simplify(tree.as_array().unwrap().clone())
        .iter()
        .map(|n| n["name"].clone())
        .collect();
    assert_eq!(names, ["a"]);
    let frame = |text: &str| json!({ "kind": "frame", "name": "b", "children": [{ "kind": "text", "name": "text", "characters": text }] });
    assert_eq!(signature(&frame("one")), signature(&frame("two")));
    assert_eq!(signature(&frame("one")), "frame:b[text:text]");
}

/// penpot export: real render of the fixture project matches the snapshot (needs Chrome)
#[test]
#[ignore = "needs Chrome"]
fn real_render_of_the_fixture_project_matches_the_snapshot() {
    let root = project("penpot-render", draft());
    let (file, doc) = export_penpot(
        &root,
        &ExportOptions {
            bp: Some("desktop".into()),
            ..ExportOptions::default()
        },
    )
    .unwrap();
    assert_eq!(doc["errors"], json!([]));
    let id = doc["id"].as_str().unwrap();
    assert!(root.join(format!(".dfw/penpot/exports/{id}.json")).exists());
    assert!(root.join(".dfw/penpot/tokens.penpot.json").exists());
    assert_eq!(file, root.join(".dfw/penpot/export.json"));
    assert_eq!(export_structure(&doc), structure_fixture());
    let (_, both) = export_penpot(&root, &ExportOptions::default()).unwrap();
    let boards: Vec<Value> = both["pages"][0]["boards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|b| {
            json!([
                b["key"],
                js::to_number(b.get("x")),
                js::to_number(b.get("y")) > 0.0
            ])
        })
        .collect();
    assert_eq!(
        to_json_compact(&json!(boards)),
        r#"[["items|a|desktop",0,false],["items|a|mobile",0,true]]"#
    );
}

/// penpot export: only the newest exports are kept
#[test]
fn only_the_newest_exports_are_kept() {
    let dir = test_dir("penpot-prune");
    fs::create_dir_all(dir.join("exports")).unwrap();
    let ids: Vec<String> = (1..=25)
        .map(|i| format!("px_202611{i:02}100000_abcdef12"))
        .collect();
    for id in &ids {
        fs::write(dir.join("exports").join(format!("{id}.json")), "{}").unwrap();
    }
    fs::write(dir.join("exports/notes.txt"), "mine").unwrap();
    let removed = prune_exports(&dir, KEEP_EXPORTS).unwrap();
    assert_eq!(removed.len(), 5);
    let mut want: Vec<String> = ids[5..].iter().map(|id| format!("{id}.json")).collect();
    want.push("notes.txt".into());
    want.sort();
    assert_eq!(dir_names(&dir.join("exports")).unwrap(), want);
    assert!(prune_exports(&dir.join("nope"), KEEP_EXPORTS)
        .unwrap()
        .is_empty());
}

/// The function that runs in the page is the Node file's, byte for byte.
#[test]
fn the_page_function_is_the_one_of_the_node_module() {
    let node = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core/lib/penpot.mjs");
    // The Node module goes when the port is complete; the function then lives here alone.
    if let Ok(source) = fs::read_to_string(node) {
        assert!(source.contains(SNAPSHOT_PAGE));
    }
    assert!(SNAPSHOT_PAGE.starts_with("function snapshotPage() {\n"));
    assert!(SNAPSHOT_PAGE.ends_with("\n}\n"));
}

/// The pages an export renders: declared pages, screens assigned by `page`, the rest.
#[test]
fn pages_that_hold_screens() {
    let manifest = json!({
        "pages": [{ "id": "system", "title": "DS" }, { "id": "stock", "title": "Stock", "screens": ["items", "gone"], "sections": [{ "screens": ["detail/a"] }] }],
        "screens": [{ "id": "items" }, { "id": "detail" }, { "id": "cart", "page": "shop" }, { "id": "home" }, { "id": "_try" }, { "id": "old", "archived": true }],
    });
    let pages: Vec<Value> = screen_pages(&manifest)
        .into_iter()
        .map(|(id, title, screens)| json!([id, title, screens]))
        .collect();
    assert_eq!(
        to_json_compact(&json!(pages)),
        r#"[["stock","Stock",["items","detail"]],["shop","shop",["cart"]],["screens","Screens",["home"]],["explorations","Explorations",["_try"]]]"#
    );
    assert_eq!(screen_pages(&json!({})).len(), 1);
}

/// File URLs both ways, as the browser and Node write them.
#[test]
fn file_urls() {
    let url = path_to_file_url(Path::new("/tmp/a b/é#1.html"));
    assert_eq!(url, "file:///tmp/a%20b/%C3%A9%231.html");
    assert_eq!(file_url_to_path(&url), Path::new("/tmp/a b/é#1.html"));
}
