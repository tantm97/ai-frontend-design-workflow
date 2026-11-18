//! The "Concept" section: the style guide of the project (`design/system/style.json`) with
//! the project's own reasoning from DESIGN.md, which wins over the generic style-guide text.

use serde_json::Value;

use super::components::Inline;
use super::text::{bullets, has_heading, md_parts, part_body};
use crate::core::error::DfwError;
use crate::design::compose::escape_html as esc;
use crate::design::edit::js_regex;
use crate::design::tokens::js;

/// The strings of a JSON array (`(list || []).map(...)`); anything else is no list.
fn items(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|list| list.iter().map(|v| js::to_string(Some(v))).collect())
        .unwrap_or_default()
}

/// The section for style guide `style`; `md` is the text of DESIGN.md (empty without one).
pub(super) fn concept(style: &Value, md: &str, inline: &Inline) -> Result<String, DfwError> {
    let comment = js_regex(r"(?s)<!--.*?-->")?;
    let section = |h: &str| -> String {
        match md_parts(md)
            .into_iter()
            .find(|part| has_heading(part, &[h]))
        {
            Some(part) => js::trim(&comment.replace_all(part_body(part), "")).to_string(),
            None => String::new(),
        }
    };
    // `split(/\n\s*\n/)[0]`: the first paragraph.
    let direction = section("Direction");
    let blank_line = js_regex(r"\n\s*\n")?;
    let direction = blank_line.split(&direction).next().unwrap_or_default();
    let rationale = section("Rationale");
    let project_why = bullets(&rationale, 1);
    let lead = if !direction.is_empty() && !direction.starts_with("TODO") {
        inline.md(direction)
    } else {
        esc(&js::to_string(style.get("concept")))
    };
    let why: Vec<String> = if project_why.is_empty() {
        items(style.get("rationale"))
    } else {
        project_why.iter().map(|s| s.to_string()).collect()
    };
    let card = |title: &str, key: &str| {
        let list = items(style.get(key));
        if list.is_empty() {
            return String::new();
        }
        let lines: String = list
            .iter()
            .map(|x| format!("<li>{}</li>", inline.lead(x)))
            .collect();
        format!("<div class=\"ds-card\"><h4>{title}</h4><ul>{lines}</ul></div>")
    };
    let why_card = if why.is_empty() {
        String::new()
    } else {
        let rows: String = why.iter().map(|x| inline.why(x)).collect();
        format!(
            "<div class=\"ds-card ds-card--wide\"><h4>Why{}</h4><table class=\"ds-why\"><thead><tr><th>Decision</th><th>Because</th></tr></thead><tbody>{rows}</tbody></table></div>",
            if project_why.is_empty() { " (style guide)" } else { " (DESIGN.md)" }
        )
    };
    let layout = if js::truthy(style.get("layout")) {
        format!(
            "<div class=\"ds-card\"><h4>Layout and grid</h4><p>{}</p></div>",
            esc(&js::to_string(style.get("layout")))
        )
    } else {
        String::new()
    };
    let motifs = style
        .get("motifs")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .map_or(String::new(), |list| {
            let rows: String = list
                .iter()
                .map(|m| {
                    format!(
                        "<dt>{}</dt><dd>{}</dd>",
                        esc(&js::to_string(m.get("name"))),
                        esc(&js::to_string(m.get("use")))
                    )
                })
                .collect();
            format!("<div class=\"ds-card\"><h4>Signature motifs</h4><dl>{rows}</dl></div>")
        });
    Ok(format!(
        "<section data-dfw-id=\"system.style\"><h2>Concept</h2>
  <p class=\"ds-lead\">{lead}</p>
  <div class=\"ds-cards\">
  {why_card}
  {}
  {}
  {layout}
  {motifs}
  {}
  </div>
  </section>",
        card("Do", "do"),
        card("Don't", "dont"),
        card("Component rules", "components"),
    ))
}
