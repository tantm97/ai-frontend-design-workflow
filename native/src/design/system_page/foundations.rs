//! The foundations of the page, from the tokens: palette ramps, colour roles with their
//! contrast checks, status colours, type families and styles, spacing, shape and elevation.

use serde_json::Value;

use super::text::{cut_note, dim, DOT};
use super::tokens::Tokens;
use crate::core::error::DfwError;
use crate::core::js;
use crate::core::js::regex::with_js_classes;
use crate::design::color::contrast;
use crate::design::html::escape_html as esc;
use crate::design::tokens::css_var_name;
use crate::design::tokens::TokenEntry;

const ROLE_GROUPS: [(&str, &[&str]); 5] = [
    (
        "Surfaces",
        &["canvas", "surface", "surface-sunken", "surface-hover"],
    ),
    ("Text", &["ink", "muted", "subtle", "accent-text"]),
    (
        "Lines and focus",
        &["rule", "border", "border-strong", "focus", "focus-ring"],
    ),
    (
        "Action",
        &["accent", "accent-hover", "accent-soft", "on-accent"],
    ),
    ("Brand and signature", &["brand", "on-brand", "signature"]),
];

/// What each role is checked against, and the WCAG target.
const ROLE_CHECK: [(&str, &str, f64); 10] = [
    ("ink", "surface", 7.0),
    ("muted", "surface", 4.5),
    ("subtle", "surface", 4.5),
    ("accent-text", "accent-soft", 4.5),
    ("border", "surface", 3.0),
    ("border-strong", "surface", 3.0),
    ("focus", "surface", 3.0),
    ("accent", "surface", 4.5),
    ("on-accent", "accent", 4.5),
    ("on-brand", "brand", 4.5),
];

const TEXT_STYLES: [(&str, &str, &str); 11] = [
    ("display-xl", "Display XL", "Hero numbers, one per screen"),
    (
        "display-lg",
        "Display L",
        "Page titles on marketing/overview",
    ),
    ("display-md", "Display M", "Section titles, key figures"),
    ("display-sm", "Display S", "Screen titles, names to confirm"),
    ("title", "Title", "Card and dialog titles"),
    ("subtitle", "Subtitle", "Emphasised lines"),
    ("body", "Body", "Default reading text"),
    ("body-sm", "Body S", "Dense UI, tables"),
    ("caption", "Caption", "Labels, meta, timestamps"),
    ("eyebrow", "Eyebrow", "Section openers"),
    ("mono", "Mono", "IDs, codes, numbers to compare"),
];

/// Palette: one compact strip per ramp, with a dot on the steps a role uses.
pub(super) fn palette(all: &[TokenEntry], tokens: &Tokens) -> Result<String, DfwError> {
    struct Step<'a> {
        step: String,
        key: String,
        token: &'a TokenEntry,
    }
    let step_re = with_js_classes(&format!(r"^({DOT}*)-(\d+)$"))?;
    let mut ramps: Vec<(String, Vec<Step>)> = Vec::new();
    for t in all
        .iter()
        .filter(|t| t.is_type("color") && t.name.starts_with("color.palette."))
    {
        let key = t.name.replacen("color.palette.", "", 1);
        let (ramp, step) = match step_re.captures(&key) {
            Some(m) => (m[1].to_string(), m[2].to_string()),
            None => (key.clone(), String::new()),
        };
        let at = match ramps.iter().position(|(name, _)| *name == ramp) {
            Some(at) => at,
            None => {
                ramps.push((ramp, Vec::new()));
                ramps.len() - 1
            }
        };
        ramps[at].1.push(Step {
            step,
            key,
            token: t,
        });
    }
    let mut used_steps: Vec<(String, Vec<&str>)> = Vec::new();
    for (_, roles) in ROLE_GROUPS {
        for r in roles {
            let a = tokens.alias_of(&format!("color.{r}"));
            if a.is_empty() {
                continue;
            }
            match used_steps.iter_mut().find(|(k, _)| *k == a) {
                Some((_, list)) => list.push(r),
                None => used_steps.push((a, vec![r])),
            }
        }
    }
    let mut palette_html = String::new();
    for (name, list) in &ramps {
        let intent = list
            .iter()
            .find(|x| x.step == "500")
            .map(|x| js::or_empty(x.token.description.as_ref()))
            .unwrap_or_default();
        let intent = cut_note(&cut_note(&intent, "Used for:")?, "Reserve step")?;
        let mut chips = String::new();
        for x in list {
            let hex = tokens.hex_of(&x.token.value).unwrap_or_default();
            let used = used_steps
                .iter()
                .find(|(k, _)| *k == x.key)
                .map(|(_, roles)| roles);
            let dark = contrast(&hex, "#ffffff") > contrast(&hex, "#000000");
            let title = format!(
                "{} {hex}{}",
                x.key,
                used.map_or(String::new(), |roles| format!(" · {}", roles.join(", ")))
            );
            chips.push_str(&format!(
                "<div class=\"ds-chip{}\" style=\"background:var({});color:{}\" title=\"{}\"><span>{}</span>{}</div>",
                if used.is_some() { " is-used" } else { "" },
                x.token.var(),
                if dark { "#fff" } else { "#000" },
                esc(&title),
                esc(&x.step),
                if used.is_some() { "<i></i>" } else { "" }
            ));
        }
        palette_html.push_str(&format!(
            "<div class=\"ds-ramp\" data-dfw-id=\"system.palette.{name}\">
  <div class=\"ds-ramp__head\"><b>{name}</b><span>{intent}</span></div>
  <div class=\"ds-ramp__chips\">{chips}</div>
</div>",
            name = esc(name),
            intent = esc(&intent),
        ));
    }
    let palette = if ramps.is_empty() {
        String::new()
    } else {
        format!("<h4>Palette</h4>{palette_html}<p class=\"ds-legend\">Each ramp is an OKLCH scale (50 lightest → 950 darkest). A dot marks a step used by a role; other steps are reserved for states and charts.</p>")
    };
    Ok(palette)
}

/// Roles: a grouped table with the palette step and the contrast check.
pub(super) fn roles(tokens: &Tokens) -> Result<String, DfwError> {
    let role_row = |r: &str| -> Result<String, DfwError> {
        let Some(hex) = tokens.role(r) else {
            return Ok(String::new());
        };
        let mut check = String::new();
        let target = ROLE_CHECK.iter().find(|(role, _, _)| *role == r);
        if let Some((_, against, min)) = target {
            if let Some(other) = tokens.role(against) {
                let c = contrast(&hex, &other);
                let ok = c >= *min;
                check = format!(
                    "<span class=\"ds-pass {}\">{}:1 {} <small>on {}</small></span>",
                    if ok { "ok" } else { "bad" },
                    js::to_fixed(c, 1),
                    if ok { "✓" } else { "✕" },
                    esc(against)
                );
            }
        }
        let name = format!("color.{r}");
        let step = tokens.alias_of(&name);
        Ok(format!(
            "<tr data-dfw-id=\"system.token.color.{r}\"><td><i class=\"ds-dot\" style=\"background:var({})\"></i></td><td><b>{r}</b></td><td><code>{}</code></td><td>{}</td><td>{check}</td></tr>",
            css_var_name(&name),
            esc(if step.is_empty() { &hex } else { &step }),
            esc(&cut_note(&tokens.description(&name), "Used for:")?),
            r = esc(r),
        ))
    };
    let mut roles_html = String::from("<table class=\"ds-table\">");
    for (group, roles) in ROLE_GROUPS {
        roles_html.push_str(&format!(
            "<tbody><tr class=\"ds-table__group\"><th colspan=\"5\">{}</th></tr>",
            esc(group)
        ));
        for r in roles {
            roles_html.push_str(&role_row(r)?);
        }
        roles_html.push_str("</tbody>");
    }
    roles_html.push_str("</table>");
    Ok(roles_html)
}

/// Status: four previews, not twelve swatches.
pub(super) fn status(tokens: &Tokens) -> String {
    let mut status_html = String::from("<div class=\"ds-status\">");
    for (k, label) in [
        ("ok", "Success"),
        ("info", "Info"),
        ("warn", "Warning"),
        ("err", "Error"),
    ] {
        if tokens.get(&format!("color.status.{k}")).is_none() {
            continue;
        }
        status_html.push_str(&format!(
            "<div class=\"ds-status__item\" data-dfw-id=\"system.status.{k}\">
  <div class=\"ds-status__fill\" style=\"background:var(--color-status-{k});color:var({on})\">{label} fill</div>
  <div class=\"ds-status__tint\" style=\"background:var(--color-status-{k}-tint);color:var(--color-status-{k}-text)\">{label} message text</div>
</div>",
            on = if k == "warn" { "--color-on-warn" } else { "--color-on-fill" },
        ));
    }
    status_html.push_str("</div>");
    status_html
}

/// The 60/30/10 proportion bar.
pub(super) fn proportion(tokens: &Tokens) -> String {
    if tokens.role("canvas").is_some() && tokens.role("accent").is_some() {
        format!(
            "<div class=\"ds-prop\" aria-label=\"Colour proportion\"><span style=\"flex:6;background:var(--color-canvas)\">60 · canvas + surfaces</span><span style=\"flex:3;background:var(--color-ink);color:var(--color-canvas)\">30 · ink + text</span><span style=\"flex:1;background:var(--color-accent);color:var(--color-on-accent)\">10 · accent</span>{}</div>",
            if tokens.role("signature").is_some() {
                "<span style=\"flex:.4;background:var(--color-signature)\" title=\"signature\"></span>"
            } else {
                ""
            }
        )
    } else {
        String::new()
    }
}

/// The type families, each in its own font.
pub(super) fn families(all: &[TokenEntry], tokens: &Tokens) -> String {
    let has_families = all.iter().any(|t| t.is_type("fontFamily"));
    let mut family_html = String::new();
    if has_families {
        family_html.push_str("<div class=\"ds-fams\">");
        for (n, note) in [
            ("display", "Display: titles and key numbers"),
            ("body", "UI and reading text"),
            ("mono", "Identifiers and codes"),
        ] {
            let Some(fam) = tokens.get(&format!("font.family.{n}")) else {
                continue;
            };
            let name = match &fam.value {
                Value::Array(list) => js::to_string(list.first()),
                other => js::or_empty(Some(other)),
            };
            family_html.push_str(&format!(
                "<div class=\"ds-fam\"><span class=\"ds-fam__aa\" style=\"font-family:var(--font-family-{n})\">Aa</span><b>{}</b><small>{}</small></div>",
                esc(&name),
                esc(note)
            ));
        }
        family_html.push_str("</div>");
    }
    family_html
}

/// The named text styles in their real font (needs the `text` master).
pub(super) fn text_styles(has_text: bool, tokens: &Tokens, sample: &str) -> String {
    let mut text_html = String::new();
    if has_text {
        text_html.push_str("<table class=\"ds-table ds-types\">");
        for (v, label, usage) in TEXT_STYLES {
            let size = match v {
                _ if v.starts_with("display") => tokens.size_of(v),
                "title" => tokens.size_of("xl"),
                "subtitle" => tokens.size_of("lg"),
                "body" => tokens.size_of("md"),
                "body-sm" | "mono" => tokens.size_of("sm"),
                "caption" => tokens.size_of("xs"),
                "eyebrow" => tokens
                    .get("font.eyebrow.size")
                    .and_then(|t| t.value.get("value")),
                _ => None,
            };
            let size = if js::truthy(size) {
                format!(" · {}px", js::to_string(size))
            } else {
                String::new()
            };
            let shown = if v.starts_with("display") {
                js::trim(sample.split('·').next().unwrap_or_default()).to_string()
            } else if v == "mono" {
                "TP-TOOL-00421 · 20417".to_string()
            } else {
                sample.to_string()
            };
            text_html.push_str(&format!(
                "<tr><td class=\"ds-types__meta\"><b>{}</b><span>{}</span><code>text {}{size}</code></td><td><dfw-use c=\"text\" variant=\"{v}\" as=\"div\">{}</dfw-use></td></tr>",
                esc(label),
                esc(usage),
                esc(v),
                esc(&shown)
            ));
        }
        text_html.push_str("</table>");
    }
    if text_html.is_empty() {
        text_html
            .push_str("<p class=\"ds-empty\">Add the text component to see the type styles.</p>");
    }
    text_html
}

fn dimension<'a>(all: &'a [TokenEntry], prefix: &str) -> Vec<&'a TokenEntry> {
    all.iter()
        .filter(|t| t.is_type("dimension") && t.name.starts_with(prefix))
        .collect()
}

/// The spacing scale, smallest first.
pub(super) fn spacing(all: &[TokenEntry]) -> String {
    let mut spaces = dimension(all, "space.");
    let amount = |t: &TokenEntry| {
        let v = t.value.get("value");
        if js::truthy(v) {
            js::to_number(v)
        } else {
            0.0
        }
    };
    spaces.sort_by(|a, b| {
        (amount(a) - amount(b))
            .partial_cmp(&0.0)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut spacing: String = spaces
        .iter()
        .map(|s| {
            format!(
                "<div data-dfw-id=\"system.token.{}\"><code>{} · {}</code><i style=\"width:var({})\"></i></div>",
                esc(&s.name),
                esc(&s.name.replacen("space.", "space-", 1)),
                esc(&dim(&s.value)),
                s.var()
            )
        })
        .collect();
    if spacing.is_empty() {
        spacing.push_str("<p class=\"ds-empty\">No spacing tokens yet.</p>");
    }
    spacing
}

/// Radii (the pill aside) and shadows.
pub(super) fn shapes(all: &[TokenEntry]) -> String {
    let radii = dimension(all, "radius.");
    let mut shapes = String::new();
    for r in radii.iter().filter(|r| !r.name.contains("pill")) {
        shapes.push_str(&format!(
            "<div><i style=\"border-radius:var({})\"></i><code>{} · {}</code></div>",
            r.var(),
            esc(&r.name.replacen("radius.", "radius-", 1)),
            esc(&dim(&r.value))
        ));
    }
    for s in all.iter().filter(|t| t.is_type("shadow")) {
        shapes.push_str(&format!(
            "<div><i class=\"sh\" style=\"border-radius:var(--radius-md, 4px);box-shadow:var({})\"></i><code>{}</code></div>",
            s.var(),
            esc(&s.name.replacen("shadow.", "shadow-", 1))
        ));
    }
    shapes
}
