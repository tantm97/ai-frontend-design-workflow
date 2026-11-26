//! Ports `core/lib/stack.mjs`: detect the technology stack of an existing project (input to gate G3).

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Map, Value};

use crate::core::error::DfwError;
use crate::core::js;
use crate::core::util::{iso_from_millis, read_json};

/// The keys of `.dfw/stack.json`: what [`detect_stack`] finds, and what `dfw stack` and
/// `dfw approve G3` add.
pub mod field {
    pub const DETECTED_AT: &str = "detectedAt";
    /// `javascript`, `static`, `php`, ... or `unknown`.
    pub const KIND: &str = "kind";
    /// `{ name, package?, version? }`.
    pub const FRAMEWORK: &str = "framework";
    /// A list of `{ name, package, version }`.
    pub const STYLING: &str = "styling";
    /// A list of `{ name, package, version? }`.
    pub const UI_LIBRARIES: &str = "uiLibraries";
    pub const LANGUAGE: &str = "language";
    pub const PACKAGE_MANAGER: &str = "packageManager";
    /// The files the detection read.
    pub const EVIDENCE: &str = "evidence";
    pub const IS_NEW_PROJECT: &str = "isNewProject";
    /// The display name of a framework or library.
    pub const NAME: &str = "name";
    /// The package (or the file) a framework or library was found by.
    pub const PACKAGE: &str = "package";
    /// The version range the project asks for.
    pub const VERSION: &str = "version";
    /// `proposed`, then `approved` (G3).
    pub const STATUS: &str = "status";
    /// `detected` for a proposal from `dfw stack detect --write`.
    pub const SOURCE: &str = "source";
    pub const PROPOSED_AT: &str = "proposedAt";
    pub const APPROVED_AT: &str = "approvedAt";
    /// The new packages the approved stack allows: a list of names (`name@version`) or of
    /// `{ name, version, dev?, license?, purpose? }`, or an object keyed by name.
    pub const DEPENDENCIES: &str = "dependencies";
    /// As [`DEPENDENCIES`], for development only.
    pub const DEV_DEPENDENCIES: &str = "devDependencies";
    /// Of a dependency: needed in development only.
    pub const DEV: &str = "dev";
    pub const LICENSE: &str = "license";
    /// Of a dependency: why the project needs it.
    pub const PURPOSE: &str = "purpose";
}

/// The keys of an app's `package.json` that dfw reads or writes (the stack detection and
/// `dfw inspector setup`).
pub mod package_json {
    /// The packages the app needs, by name.
    pub const DEPENDENCIES: &str = "dependencies";
    /// The packages needed in development only, by name.
    pub const DEV_DEPENDENCIES: &str = "devDependencies";
    /// The npm scripts, by name.
    pub const SCRIPTS: &str = "scripts";
    /// The script that starts the development server.
    pub const DEV_SCRIPT: &str = "dev";
}

/// The package whose presence makes a project TypeScript (with `tsconfig.json`).
const TYPESCRIPT: &str = "typescript";

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

/// The manifest of a JavaScript project.
use crate::core::paths::file::PACKAGE_JSON;
/// The config of shadcn/ui.
const SHADCN_CONFIG: &str = "components.json";
const YARN_LOCK: &str = "yarn.lock";
const DJANGO_MARKER: &str = "manage.py";
const PYPROJECT: &str = "pyproject.toml";
const CARGO_TOML: &str = "Cargo.toml";

const LOCK_FILES: [(&str, &str); 5] = [
    ("pnpm-lock.yaml", "pnpm"),
    (YARN_LOCK, "yarn"),
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
    (DJANGO_MARKER, "python", "Django"),
    (PYPROJECT, "python", "Python"),
    (CARGO_TOML, "rust", "Rust"),
];

/// Every listed package the project depends on, as `{ name, package, version }`.
fn found(deps: &Map<String, Value>, table: &[(&str, &str)]) -> Vec<Value> {
    table
        .iter()
        .filter_map(|(dep, name)| {
            let version = deps.get(*dep).filter(|v| js::truthy(v))?;
            Some(json!({ (field::NAME): name, (field::PACKAGE): dep, (field::VERSION): version }))
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

    let pkg_path = root.join(PACKAGE_JSON);
    if pkg_path.exists() {
        let pkg = read_json(&pkg_path, Some(json!({})))?;
        let mut deps = js::spread(pkg.get(package_json::DEPENDENCIES));
        deps.extend(js::spread(pkg.get(package_json::DEV_DEPENDENCIES)));
        kind = "javascript";
        evidence.push(PACKAGE_JSON.into());
        framework = found(&deps, &FRAMEWORKS)
            .into_iter()
            .next()
            .unwrap_or(Value::Null);
        styling = found(&deps, &STYLING);
        ui_libraries = found(&deps, &UI_LIBS);
        if root.join(SHADCN_CONFIG).exists() {
            ui_libraries
                .push(json!({ (field::NAME): "shadcn/ui", (field::PACKAGE): SHADCN_CONFIG }));
            evidence.push(SHADCN_CONFIG.into());
        }
        let typescript = deps.get(TYPESCRIPT).is_some_and(js::truthy);
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
                framework = json!({ (field::NAME): name });
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
            framework = json!({ (field::NAME): "Plain HTML/CSS/JS" });
            evidence.extend(html.into_iter().take(3));
        }
    }
    let is_new_project = evidence.is_empty();
    Ok(json!({
        (field::DETECTED_AT): iso_from_millis(now),
        (field::KIND): kind,
        (field::FRAMEWORK): framework,
        (field::STYLING): styling,
        (field::UI_LIBRARIES): ui_libraries,
        (field::LANGUAGE): language,
        (field::PACKAGE_MANAGER): package_manager,
        (field::EVIDENCE): evidence,
        (field::IS_NEW_PROJECT): is_new_project,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::fixture::INDEX_FILE;
    use crate::core::util::{test_dir, to_json_compact};

    // The fields of the detected stack.
    const KIND: &str = "kind";
    const FRAMEWORK: &str = "framework";
    const EVIDENCE: &str = "evidence";
    const IS_NEW_PROJECT: &str = "isNewProject";

    #[test]
    fn an_empty_folder_is_a_new_project() {
        let root = test_dir("stack-empty");
        let d = detect_stack(&root).unwrap();
        let keys: Vec<&str> = d.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(
            keys,
            [
                "detectedAt",
                KIND,
                FRAMEWORK,
                "styling",
                "uiLibraries",
                "language",
                "packageManager",
                EVIDENCE,
                IS_NEW_PROJECT
            ]
        );
        assert_eq!(d[KIND], "unknown");
        assert_eq!(d[IS_NEW_PROJECT], true);
        assert_eq!(d[FRAMEWORK], Value::Null);
    }

    #[test]
    fn a_javascript_project_names_framework_styling_libraries_and_package_manager() {
        let root = test_dir("stack-js");
        fs::write(
            root.join(PACKAGE_JSON),
            r#"{"dependencies":{"react":"^19.0.0","next":"15.1.0","@mui/material":"6"},"devDependencies":{"tailwindcss":"4.0.0","sass":"1","typescript":"5.7"}}"#,
        )
        .unwrap();
        fs::write(root.join(SHADCN_CONFIG), "{}").unwrap();
        fs::write(root.join(YARN_LOCK), "").unwrap();
        fs::write(root.join("package-lock.json"), "{}").unwrap();
        fs::write(root.join(CARGO_TOML), "").unwrap();
        let d = detect_stack(&root).unwrap();
        assert_eq!(d[KIND], "javascript");
        assert_eq!(
            to_json_compact(&d[FRAMEWORK]),
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
            d[EVIDENCE],
            json!([PACKAGE_JSON, SHADCN_CONFIG, YARN_LOCK, CARGO_TOML])
        );
        assert_eq!(d[IS_NEW_PROJECT], false);
    }

    #[test]
    fn other_stacks_and_plain_html() {
        let root = test_dir("stack-other");
        fs::write(root.join(DJANGO_MARKER), "").unwrap();
        fs::write(root.join(PYPROJECT), "").unwrap();
        fs::write(root.join(INDEX_FILE), "").unwrap();
        let d = detect_stack(&root).unwrap();
        assert_eq!(d[KIND], "python");
        assert_eq!(d[FRAMEWORK], json!({ "name": "Django" }));
        assert_eq!(d[EVIDENCE], json!([DJANGO_MARKER, PYPROJECT]));

        let site = test_dir("stack-static");
        fs::write(site.join(INDEX_FILE), "").unwrap();
        let d = detect_stack(&site).unwrap();
        assert_eq!(d[KIND], "static");
        assert_eq!(d[FRAMEWORK], json!({ "name": "Plain HTML/CSS/JS" }));
        assert_eq!(d[EVIDENCE], json!([INDEX_FILE]));
        assert_eq!(d["language"], Value::Null);
    }
}
