//! Ports `core/lib/system-page.mjs`: the generated "Design System" page: foundations (from the
//! DTCG tokens) and every component master with its variants and examples. The page still
//! contains `<dfw-use>`/`<dfw-icon>`; callers compose it like any other design file.
//!
//! Split by job: `foundations` (tokens as palette, roles, type, spacing, shape), `concept` (the
//! style guide and DESIGN.md), `components` (one section per master), `tokens` (lookups),
//! `text` (the JavaScript text rules the page depends on).

mod components;
mod concept;
mod foundations;
#[cfg(test)]
mod tests;
mod text;
mod tokens;

use std::path::Path;

use serde_json::{json, Value};

use self::components::{comp_section, Inline};
use self::text::encode_uri;
use self::tokens::{load_tokens, Tokens, Vars};
use super::compose::{escape_html as esc, list_icons, load_components, ComponentDef};
use super::tokens::dir_names;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::paths::project_paths;
use crate::design::dsaudit::audit_design_system;

/// The page's own styles (the literal of system-page.mjs, kept as a file).
const CSS: &str = include_str!("../system_page.css");

/// `renderSystemPage`: the Design System page of a project. `design_base` is the URL prefix
/// under which `design/` is served (`../` from `design/system/`).
// Over 120 lines: the page template in one piece, in page order; the sections are built by
// the other files of this module.
#[allow(clippy::too_many_lines)]
pub fn render_system_page(root: &Path, design_base: &str) -> Result<String, DfwError> {
    let p = project_paths(root);
    let manifest = js::read_json(&p.design.join("design.json"), Some(json!({})))?;
    let all = load_tokens(root)?;
    let tokens = Tokens::new(&all);
    let lib = load_components(root)?;
    let icons = list_icons(root)?;
    let head_file = p.design.join("system").join("head.html");
    let head = if head_file.exists() {
        js::read_text(&head_file)?.replace("{{base}}", design_base)
    } else {
        String::new()
    };
    let vars = Vars::load(root)?;
    let inline = Inline::new()?;
    let style_file = p.design.join("system").join("style.json");
    let style = if style_file.exists() {
        js::read_json(&style_file, Some(Value::Null))?
    } else {
        Value::Null
    };
    let style = Some(&style).filter(|s| js::truthy(Some(s)));
    let assets_dir = p.design.join("assets");
    let mut assets: Vec<String> = dir_names(&assets_dir)?
        .into_iter()
        .filter(|f| !f.starts_with('.'))
        .collect();
    assets.sort_by(|a, b| js::cmp_utf16(a, b));
    let manifest_text = |key: &str, fallback: &str| {
        let v = manifest.get(key);
        if js::truthy(v) {
            js::to_string(v)
        } else {
            fallback.to_string()
        }
    };

    let mut groups: Vec<(&str, Vec<&ComponentDef>)> = Vec::new();
    for d in lib.iter() {
        match groups.iter_mut().find(|(g, _)| *g == d.group) {
            Some((_, list)) => list.push(d),
            None => groups.push((&d.group, vec![d])),
        }
    }

    let design_md = root.join("DESIGN.md");
    let md = if design_md.exists() {
        js::read_text(&design_md)?
    } else {
        String::new()
    };
    let audit = audit_design_system(root)?;

    let mut out = String::new();
    out.push_str(&format!(
        "<!doctype html>
<html lang=\"{lang}\">
<head>
<meta charset=\"utf-8\">
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">
<title>Design System - {title}</title>
<meta name=\"description\" content=\"Foundations and components of the {product} design system.\">
{head}
<link rel=\"stylesheet\" href=\"{design_base}tokens/tokens.css\">
<link rel=\"stylesheet\" href=\"{design_base}system/components.css\">
<style>
{CSS}</style>
</head>
<body class=\"ds\" data-dfw-id=\"system\">
  <header data-dfw-id=\"system.header\"><h1>Design System</h1><p>{product}{style_name}</p></header>

  ",
        lang = esc(&manifest_text("lang", "en")),
        title = esc(&manifest_text("product", "Product")),
        product = esc(&manifest_text("product", "")),
        style_name = style.map_or(String::new(), |s| format!(
            " · {}",
            esc(&js::to_string(s.get("name")))
        )),
    ));

    if let Some(style) = style {
        out.push_str(&concept::concept(style, &md, &inline)?);
    }

    let level = |x: &Value| js::to_string(x.get("level"));
    let count = |name: &str| audit.iter().filter(|x| level(x) == name).count();
    let audit_title = if audit.is_empty() {
        "clean".to_string()
    } else {
        format!("{} must, {} should", count("error"), count("warn"))
    };
    let audit_html = if audit.is_empty() {
        "<p>Contrast, semantic colours, neutral temperature, type rhythm and rationale all pass.</p>".to_string()
    } else {
        let rows: String = audit
            .iter()
            .map(|x| {
                let level = level(x);
                let label = match level.as_str() {
                    "error" => "MUST",
                    "warn" => "SHOULD",
                    "info" => "NOTE",
                    _ => "undefined",
                };
                format!(
                    "<li class=\"lv-{level}\"><b>{label}</b>{}</li>",
                    esc(&js::to_string(x.get("message")))
                )
            })
            .collect();
        format!("<ul class=\"ds-audit\">{rows}</ul>")
    };
    let palette = foundations::palette(&all, &tokens)?;
    let proportion = foundations::proportion(&tokens);
    let roles_html = foundations::roles(&tokens)?;
    let status_html = foundations::status(&tokens);
    let family_html = foundations::families(&all, &tokens);
    let scale_note = tokens.description("font.size.md");
    let sample = manifest_text("sampleText", "The quick brown fox · 0123456789");
    let text_html = foundations::text_styles(lib.get(Some("text")).is_some(), &tokens, &sample);
    let spacing = foundations::spacing(&all);
    let shapes = foundations::shapes(&all);
    let scale = if scale_note.is_empty() {
        String::new()
    } else {
        format!(
            "<p class=\"ds-legend\" style=\"margin:0 0 8px\">{}</p>",
            esc(&scale_note)
        )
    };
    let icons_html = if icons.is_empty() {
        "<p class=\"ds-empty\">No icons yet. Add them with: dfw icons add &lt;name&gt; …</p>"
            .to_string()
    } else {
        let cells: String = icons
            .iter()
            .map(|n| {
                format!(
                    "<div><dfw-icon name=\"{n}\"></dfw-icon>{n}</div>",
                    n = esc(n)
                )
            })
            .collect();
        format!("<div class=\"ds-icons\">{cells}</div>")
    };
    let assets_html = if assets.is_empty() {
        "<p class=\"ds-empty\">No assets in design/assets/ yet (logos, ornaments, photography). Placeholders must be labelled as placeholders.</p>".to_string()
    } else {
        let figures: String = assets
            .iter()
            .map(|f| {
                let lower = f.to_ascii_lowercase();
                let image = [".png", ".jpg", ".jpeg", ".svg", ".webp", ".gif"]
                    .iter()
                    .any(|ext| lower.ends_with(ext));
                format!(
                    "<figure>{}<figcaption>{}</figcaption></figure>",
                    if image {
                        format!(
                            "<img src=\"{design_base}assets/{}\" alt=\"\">",
                            encode_uri(f)
                        )
                    } else {
                        String::new()
                    },
                    esc(f)
                )
            })
            .collect();
        format!("<div class=\"ds-assets\">{figures}</div><p class=\"ds-legend\">Brand assets are used as supplied: never redrawn, recoloured or cropped into new marks.</p>")
    };
    let mut components = String::new();
    for (group, list) in &groups {
        components.push_str(&format!("<div class=\"ds-group\"><h4>{}</h4>", esc(group)));
        for d in list {
            components.push_str(&comp_section(d, &vars, &inline)?);
        }
        components.push_str("</div>");
    }
    if components.is_empty() {
        components.push_str("<p class=\"ds-empty\">No components yet. Create design/system/components/&lt;name&gt;.html (see the design-format reference).</p>");
    }

    out.push_str(&format!(
        "

  <section data-dfw-id=\"system.audit\"><h2>Senior review · {audit_title}</h2>
  {audit_html}
  </section>

  <section data-dfw-id=\"system.colors\"><h2>Colour</h2>
  {proportion}
  {roles_html}
  <h4>Status</h4>
  {status_html}
  {palette}
  </section>

  <section data-dfw-id=\"system.typography\"><h2>Typography</h2>
  {family_html}
  {scale}
  {text_html}
  </section>

  <section data-dfw-id=\"system.spacing\"><h2>Spacing</h2>
  <div class=\"ds-space\">{spacing}</div>
  </section>

  <section data-dfw-id=\"system.shape\"><h2>Shape and elevation</h2>
  <div class=\"ds-shapes\">{shapes}</div>
  </section>

  <section data-dfw-id=\"system.icons\"><h2>Icons · {} (Lucide)</h2>
  {icons_html}
  </section>

  <section data-dfw-id=\"system.assets\"><h2>Assets · {}</h2>
  {assets_html}
  </section>

  <section data-dfw-id=\"system.components\"><h2>Components · {}</h2>
  {components}
  </section>
</body>
</html>
",
        icons.len(),
        assets.len(),
        lib.len(),
    ));
    Ok(out)
}
