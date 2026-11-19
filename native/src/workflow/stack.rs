//! Ports `core/lib/stack.mjs`: detect the technology stack of an existing project (input to gate G3).

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::util::{iso_from_millis, read_json};

const FRAMEWORKS: [(&str, &str); 13] = [
    ("next", "Next.js (React)"),
    ("nuxt", "Nuxt (Vue)"),
    ("@sveltejs/kit", "SvelteKit"),
    ("@angular/core", "Angular"),
    ("@remix-run/react", "Remix (React)"),
    ("@builder.io/qwik", "Qwik"),
    ("astro", "Astro"),
    ("solid-js", "Solid"),
    ("svelte", "Svelte"),
    ("vue", "Vue"),
    ("preact", "Preact"),
    ("react", "React"),
    ("lit", "Lit (Web Components)"),
];

const STYLING: [(&str, &str); 9] = [
    ("tailwindcss", "Tailwind CSS"),
    ("styled-components", "styled-components"),
    ("@emotion/react", "Emotion"),
    ("@vanilla-extract/css", "vanilla-extract"),
    ("@pandacss/dev", "Panda CSS"),
    ("unocss", "UnoCSS"),
    ("sass", "Sass"),
    ("less", "Less"),
    ("bootstrap", "Bootstrap"),
];

const UI_LIBS: [(&str, &str); 16] = [
    ("@mui/material", "MUI"),
    ("antd", "Ant Design"),
    ("@chakra-ui/react", "Chakra UI"),
    ("@mantine/core", "Mantine"),
    ("@radix-ui/react-slot", "Radix (likely shadcn/ui)"),
    ("@headlessui/react", "Headless UI"),
    ("react-aria-components", "React Aria"),
    ("vuetify", "Vuetify"),
    ("primevue", "PrimeVue"),
    ("primereact", "PrimeReact"),
    ("element-plus", "Element Plus"),
    ("naive-ui", "Naive UI"),
    ("@angular/material", "Angular Material"),
    ("primeng", "PrimeNG"),
    ("bits-ui", "Bits UI"),
    ("@skeletonlabs/skeleton", "Skeleton"),
];

const LOCK_FILES: [(&str, &str); 5] = [
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("bun.lockb", "bun"),
    ("bun.lock", "bun"),
    ("package-lock.json", "npm"),
];

/// Marker file, kind and name of the stacks that are not JavaScript.
const OTHER: [(&str, &str, &str); 7] = [
    (
        "composer.json",
        "php",
        "PHP (Composer; check for Laravel/Symfony)",
    ),
    ("Gemfile", "ruby", "Ruby (check for Rails)"),
    ("go.mod", "go", "Go"),
    ("pubspec.yaml", "dart", "Flutter/Dart"),
    ("manage.py", "python", "Django"),
    ("pyproject.toml", "python", "Python"),
    ("Cargo.toml", "rust", "Rust"),
];

/// Every listed package the project depends on, as `{ name, package, version }`.
fn found(deps: &Map<String, Value>, table: &[(&str, &str)]) -> Vec<Value> {
    table
        .iter()
        .filter_map(|(dep, name)| {
            let version = deps.get(*dep).filter(|v| js::truthy(v))?;
            Some(json!({ "name": name, "package": dep, "version": version }))
        })
        .collect()
}

/// `detectStack(root)`: what the files at the project root say about its stack. The time is
/// the real clock (Node does not read `DFW_NOW` here).
pub fn detect_stack(root: &Path) -> Result<Value, DfwError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let mut kind = "unknown";
    let mut framework = Value::Null;
    let mut styling = Vec::new();
    let mut ui_libraries = Vec::new();
    let mut language = Value::Null;
    let mut package_manager = Value::Null;
    let mut evidence: Vec<String> = Vec::new();

    let pkg_path = root.join("package.json");
    if pkg_path.exists() {
        let pkg = read_json(&pkg_path, Some(json!({})))?;
        let mut deps = js::spread(pkg.get("dependencies"));
        deps.extend(js::spread(pkg.get("devDependencies")));
        kind = "javascript";
        evidence.push("package.json".into());
        framework = found(&deps, &FRAMEWORKS)
            .into_iter()
            .next()
            .unwrap_or(Value::Null);
        styling = found(&deps, &STYLING);
        ui_libraries = found(&deps, &UI_LIBS);
        if root.join("components.json").exists() {
            ui_libraries.push(json!({ "name": "shadcn/ui", "package": "components.json" }));
            evidence.push("components.json".into());
        }
        let typescript = deps.get("typescript").is_some_and(js::truthy);
        language = if typescript || root.join("tsconfig.json").exists() {
            "TypeScript"
        } else {
            "JavaScript"
        }
        .into();
        if let Some((file, pm)) = LOCK_FILES.iter().find(|(f, _)| root.join(f).exists()) {
            package_manager = (*pm).into();
            evidence.push((*file).into());
        }
    }
    for (file, other_kind, name) in OTHER {
        if root.join(file).exists() {
            evidence.push(file.into());
            if kind == "unknown" {
                kind = other_kind;
                framework = json!({ "name": name });
            }
        }
    }
    if kind == "unknown" && root.exists() {
        let mut html = Vec::new();
        for entry in fs::read_dir(root)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if name.ends_with(".html") {
                html.push(name);
            }
        }
        if !html.is_empty() {
            kind = "static";
            framework = json!({ "name": "Plain HTML/CSS/JS" });
            evidence.extend(html.into_iter().take(3));
        }
    }
    let is_new_project = evidence.is_empty();
    Ok(json!({
        "detectedAt": iso_from_millis(now),
        "kind": kind,
        "framework": framework,
        "styling": styling,
        "uiLibraries": ui_libraries,
        "language": language,
        "packageManager": package_manager,
        "evidence": evidence,
        "isNewProject": is_new_project,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::util::{test_dir, to_json_compact};

    #[test]
    fn an_empty_folder_is_a_new_project() {
        let root = test_dir("stack-empty");
        let d = detect_stack(&root).unwrap();
        let keys: Vec<&str> = d.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "detectedAt",
                "kind",
                "framework",
                "styling",
                "uiLibraries",
                "language",
                "packageManager",
                "evidence",
                "isNewProject"
            ]
        );
        assert_eq!(d["kind"], "unknown");
        assert_eq!(d["isNewProject"], true);
        assert_eq!(d["framework"], Value::Null);
    }

    #[test]
    fn a_javascript_project_names_framework_styling_libraries_and_package_manager() {
        let root = test_dir("stack-js");
        fs::write(
            root.join("package.json"),
            r#"{"dependencies":{"react":"^19.0.0","next":"15.1.0","@mui/material":"6"},"devDependencies":{"tailwindcss":"4.0.0","sass":"1","typescript":"5.7"}}"#,
        )
        .unwrap();
        fs::write(root.join("components.json"), "{}").unwrap();
        fs::write(root.join("yarn.lock"), "").unwrap();
        fs::write(root.join("package-lock.json"), "{}").unwrap();
        fs::write(root.join("Cargo.toml"), "").unwrap();
        let d = detect_stack(&root).unwrap();
        assert_eq!(d["kind"], "javascript");
        assert_eq!(
            to_json_compact(&d["framework"]),
            r#"{"name":"Next.js (React)","package":"next","version":"15.1.0"}"#
        );
        let names = |key: &str| -> Vec<String> {
            d[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x["name"].as_str().unwrap().to_string())
                .collect()
        };
        assert_eq!(names("styling"), ["Tailwind CSS", "Sass"]);
        assert_eq!(names("uiLibraries"), ["MUI", "shadcn/ui"]);
        assert_eq!(d["language"], "TypeScript");
        assert_eq!(d["packageManager"], "yarn");
        assert_eq!(
            d["evidence"],
            json!(["package.json", "components.json", "yarn.lock", "Cargo.toml"])
        );
        assert_eq!(d["isNewProject"], false);
    }

    #[test]
    fn other_stacks_and_plain_html() {
        let root = test_dir("stack-other");
        fs::write(root.join("manage.py"), "").unwrap();
        fs::write(root.join("pyproject.toml"), "").unwrap();
        fs::write(root.join("index.html"), "").unwrap();
        let d = detect_stack(&root).unwrap();
        assert_eq!(d["kind"], "python");
        assert_eq!(d["framework"], json!({ "name": "Django" }));
        assert_eq!(d["evidence"], json!(["manage.py", "pyproject.toml"]));

        let site = test_dir("stack-static");
        fs::write(site.join("index.html"), "").unwrap();
        let d = detect_stack(&site).unwrap();
        assert_eq!(d["kind"], "static");
        assert_eq!(d["framework"], json!({ "name": "Plain HTML/CSS/JS" }));
        assert_eq!(d["evidence"], json!(["index.html"]));
        assert_eq!(d["language"], Value::Null);
    }
}
