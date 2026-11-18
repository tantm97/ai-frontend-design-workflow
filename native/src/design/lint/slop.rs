//! Generic-AI tells ("slop") and the interface-guideline rules checkable in CSS: `slopHtml`,
//! `slopCss` and `guidelineCss` of lint.mjs.

use super::jsre::{blank_matches, js_re};
use super::{style_blocks, Add};
use crate::design::color::hex_to_oklch;
use crate::design::html::is_js_space;

/// Colours AI-generated UIs default to. Exact values only: a brand that really uses one keeps
/// it with a recorded decision.
pub(super) const AI_DEFAULT_COLORS: [(&str, &str); 9] = [
    ("#6366f1", "Tailwind indigo"),
    ("#4f46e5", "Tailwind indigo"),
    ("#4338ca", "Tailwind indigo"),
    ("#3730a3", "Tailwind indigo"),
    ("#8b5cf6", "Tailwind violet"),
    ("#7c3aed", "Tailwind violet"),
    ("#a855f7", "Tailwind purple"),
    (
        "#d97757",
        "the terracotta accent of the \"warm cream + serif\" default",
    ),
    (
        "#f4f1ea",
        "the warm cream ground of the \"cream + serif + terracotta\" default",
    ),
];

/// What a lower-case hex colour is the default of, if any.
pub(super) fn ai_default_color(hex: &str) -> Option<&'static str> {
    AI_DEFAULT_COLORS
        .iter()
        .find(|(k, _)| *k == hex)
        .map(|(_, name)| *name)
}

js_re!(HEX6, r"#[0-9a-fA-F]{6}\b");
js_re!(BODY_OR_TEMPLATE, "<body|<template", "i");
js_re!(
    STYLE_OR_SCRIPT,
    r"<style[\s\S]*?<\/style>|<script[\s\S]*?<\/script>",
    "i"
);
js_re!(TEXT_BLOCK_OPEN, r"<(h[1-6]|button|li|label)\b[^>]*>", "i");
js_re!(TAG, "<[^>]*>");
js_re!(EMOJI, r"\p{Extended_Pictographic}", "u");
js_re!(
    ICON_CLASS,
    r#"class\s*=\s*"[^"]*\bicon\b[^"]*"[^>]*>\s*(\S)"#
);
js_re!(
    FILLER,
    r"lorem ipsum|\bfeature (?:one|two|three)\b|placeholder text|sample content|your (?:headline|tagline) here",
    "i"
);
js_re!(
    METRIC,
    r"\b\d+(?:\.\d+)?\s*[x×]\s+(?:faster|more|better|cheaper|productive)\b|\b99\.9+\s*%\s*uptime\b",
    "i"
);
js_re!(
    PLACEHOLDER_IMG,
    r"\b(?:images\.unsplash\.com|source\.unsplash\.com|placehold\.co|placekitten\.com|picsum\.photos|via\.placeholder\.com|placeholder\.com)\b",
    "i"
);
// Marketing filler that reads as machine-written in product UI (dfw's own list).
js_re!(
    COPY_TELL,
    r"\b(?:seamless(?:ly)?|effortless(?:ly)?|supercharge|unlock (?:the )?(?:power|potential)|elevate your|game[- ]changer|cutting[- ]edge|next[- ]level|revolutioni[sz]e|empower(?:s|ing)? (?:you|teams)|in today's fast[- ]paced|harness the power|at your fingertips|look no further|delve into)\b",
    "i"
);
js_re!(
    VAGUE_ERROR,
    r"\b(?:oops!?|something went wrong|an error (?:has )?occurred)\b",
    "i"
);
js_re!(
    CLICK_HERE,
    r"<a\b[^>]*>\s*(?:click here|here|read more|learn more)\s*<\/a>",
    "i"
);

js_re!(CSS_BLOCK, r"([^{}]*)\{([^{}]*)\}");
js_re!(FONT_SIZE_PX, r"font-size\s*:\s*(\d+(?:\.\d+)?)px");
js_re!(FIELD_SELECTOR, r"(^|[\s,>+~(])(input|select|textarea)\b");
js_re!(Z_INDEX, r"z-index\s*:\s*(-?\d+)");
js_re!(GRADIENT, r"linear-gradient\(([^;{}]*)\)", "i");
js_re!(GRADIENT_HEX, r"#[0-9a-fA-F]{6}\b|#[0-9a-fA-F]{3}\b");
js_re!(BRACES, r"\{([^{}]*)\}");
js_re!(
    THICK_LEFT_BORDER,
    r"border-left(?:-width)?\s*:\s*[^;]*\b(?:[2-9]|\d{2,})px"
);
js_re!(BORDER_RADIUS, r"border-radius\s*:");

/// Interface-guideline rules checkable in CSS: zoom-triggering input text, raw z-index.
fn guideline_css(css: &str, base: usize, add: &mut Add) {
    for b in CSS_BLOCK.captures_iter(css) {
        let at = base + b.get(0).map_or(0, |m| m.start());
        let (sel, body) = (&b[1], &b[2]);
        if let Some(fs) = FONT_SIZE_PX.captures(body) {
            let px = &fs[1];
            if px.parse::<f64>().is_ok_and(|n| n < 16.0) && FIELD_SELECTOR.is_match(sel) {
                add("warn", "input-font-size", Some(at), format!("Form field text is {px}px: iOS zooms the page on focus below 16px. Use at least 16px on touch breakpoints."));
            }
        }
        if let Some(z) = Z_INDEX.captures(body) {
            let n = &z[1];
            if n.parse::<f64>().is_ok_and(|v| v.abs() > 1.0) {
                add("warn", "z-index-raw", Some(at), format!("Raw z-index {n}: stacking order drifts when every component picks its own number. Use a z-index token (e.g. var(--z-overlay))."));
            }
        }
    }
}

/// Generic-AI tells in CSS: two-stop purple/blue "trust" gradients and the rounded card with a
/// coloured left border. `base` is the offset of `css` in the text findings point into.
pub(super) fn slop_css(css: &str, base: usize, add: &mut Add) {
    guideline_css(css, base, add);
    for g in GRADIENT.captures_iter(css) {
        let hues: Vec<f64> = GRADIENT_HEX
            .find_iter(&g[1])
            .map(|m| hex_to_oklch(m.as_str()))
            .filter(|c| c.c > 0.1)
            .map(|c| c.h)
            .collect();
        let (min, max) = hues
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), h| {
                (lo.min(*h), hi.max(*h))
            });
        if hues.len() >= 2 && hues.iter().all(|h| (190.0..360.0).contains(h)) && max - min > 20.0 {
            let at = base + g.get(0).map_or(0, |m| m.start());
            add("warn", "ai-gradient", Some(at), "Purple/blue/cyan two-stop gradient: the textbook AI \"trust\" gradient. Use a flat surface and intentional type, or a gradient built from the palette with a reason.".into());
        }
    }
    for b in BRACES.captures_iter(css) {
        if THICK_LEFT_BORDER.is_match(&b[1]) && BORDER_RADIUS.is_match(&b[1]) {
            let at = base + b.get(0).map_or(0, |m| m.start());
            add("warn", "left-border-card", Some(at), "Rounded box with a thick coloured left border: the stock \"AI dashboard tile\". Drop the radius or the left border.".into());
        }
    }
}

/// The first `</name>` at or after byte `from`, ignoring ASCII case (the `<\/\1>` of the
/// JavaScript expression, which the `regex` crate cannot express).
fn closing_tag(text: &str, from: usize, name: &str) -> Option<(usize, usize)> {
    let bytes = text.as_bytes();
    let mut at = from;
    while let Some(i) = text[at..].find("</") {
        let start = at + i;
        let name_end = start + 2 + name.len();
        if bytes
            .get(start + 2..name_end)
            .is_some_and(|b| b.eq_ignore_ascii_case(name.as_bytes()))
            && bytes.get(name_end) == Some(&b'>')
        {
            return Some((start, name_end + 1));
        }
        at = start + 2;
    }
    None
}

/// Emoji inside headings, buttons, list items and labels:
/// `/<(h[1-6]|button|li|label)\b[^>]*>([\s\S]*?)<\/\1>/gi`.
fn emoji_in_text_blocks(markup: &str, body: usize, add: &mut Add) {
    let mut pos = 0;
    while let Some(open) = TEXT_BLOCK_OPEN.captures_at(markup, pos) {
        let (Some(whole), Some(name)) = (open.get(0), open.get(1)) else {
            break;
        };
        match closing_tag(markup, whole.end(), name.as_str()) {
            Some((close, after)) => {
                if EMOJI.is_match(&TAG.replace_all(&markup[whole.end()..close], "")) {
                    add("error", "emoji-icon", Some(body + whole.start()), format!("Emoji in <{}> used as an icon. Use a monoline SVG icon (dfw icons add <name>) with currentColor.", name.as_str()));
                }
                pos = after;
            }
            // Never closed: the expression fails here and is tried again one character on.
            None => pos = whole.start() + 1,
        }
    }
}

/// Emoji as the content of an element with class `icon`:
/// `/class\s*=\s*"[^"]*\bicon\b[^"]*"[^>]*>\s*(\S{1,4})/g`, the `{1,4}` counted in UTF-16 units.
fn emoji_in_icon_class(markup: &str, body: usize, add: &mut Add) {
    let mut pos = 0;
    while let Some(c) = ICON_CLASS.captures_at(markup, pos) {
        let (Some(whole), Some(first)) = (c.get(0), c.get(1)) else {
            break;
        };
        let mut end = first.start();
        let mut units = 0;
        for ch in markup[first.start()..].chars() {
            if is_js_space(ch) || units + ch.len_utf16() > 4 {
                break;
            }
            units += ch.len_utf16();
            end += ch.len_utf8();
        }
        if EMOJI.is_match(&markup[first.start()..end]) {
            add("error", "emoji-icon", Some(body + whole.start()), "Emoji used as an icon. Use a monoline SVG icon (dfw icons add <name>) with currentColor.".into());
        }
        pos = end;
    }
}

/// Three dots that are not part of a longer run: `/(?<![.])\.\.\.(?![.])/g`.
fn ellipses(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        let dots = &bytes[i..i + 3] == b"...";
        if dots && (i == 0 || bytes[i - 1] != b'.') && bytes.get(i + 3) != Some(&b'.') {
            out.push(i);
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// Generic-AI tells in an HTML file: default colours, emoji icons, filler copy, invented
/// metrics, placeholder image hosts, CSS tells.
pub(super) fn slop_html(text: &str, add: &mut Add) {
    for m in HEX6.find_iter(text) {
        if let Some(name) = ai_default_color(&m.as_str().to_ascii_lowercase()) {
            add("warn", "ai-default-color", Some(m.start()), format!("{} is {name}, a colour AI-generated UIs default to. Use the palette (dfw palette build); keep it only if the brand requires it (dfw decide).", m.as_str()));
        }
    }
    for block in style_blocks(text) {
        slop_css(block.css, block.offset, add);
    }
    let body = BODY_OR_TEMPLATE.find(text).map_or(0, |m| m.start());
    let markup = blank_matches(&text[body..], &STYLE_OR_SCRIPT);
    emoji_in_text_blocks(&markup, body, add);
    emoji_in_icon_class(&markup, body, add);
    for m in FILLER.find_iter(&markup) {
        add("error", "filler-copy", Some(body + m.start()), format!("Filler copy \"{}\". Write real, domain-specific content (PRODUCT.md data shapes), or solve the empty space with composition.", m.as_str()));
    }
    let text_only = blank_matches(&markup, &TAG);
    for m in COPY_TELL.find_iter(&text_only) {
        add("warn", "copy-tell", Some(body + m.start()), format!("\"{}\" is marketing filler that reads as machine-written. Say what the product does, in the user's words.", m.as_str()));
    }
    for m in VAGUE_ERROR.find_iter(&text_only) {
        add("warn", "vague-error", Some(body + m.start()), format!("\"{}\" says nothing. An error message states what happened, why if known, and what the user can do; keep their input.", m.as_str()));
    }
    for at in ellipses(&text_only) {
        add(
            "info",
            "ellipsis-char",
            Some(body + at),
            "Use the ellipsis character \"…\" instead of three dots.".into(),
        );
    }
    for m in CLICK_HERE.find_iter(&markup) {
        add("warn", "link-text", Some(body + m.start()), "Link text must say where it goes (\"View invoice 1042\"), not \"click here\" / \"learn more\" (WCAG 2.4.4).".into());
    }
    for m in METRIC.find_iter(&markup) {
        add("warn", "invented-metric", Some(body + m.start()), format!("\"{}\" looks like an invented metric. Use a real figure from the brief, or a clearly labelled placeholder.", m.as_str()));
    }
    for m in PLACEHOLDER_IMG.find_iter(text) {
        add("warn", "placeholder-image-host", Some(m.start()), format!("External placeholder image host \"{}\": fragile and obviously generic. Use a local asset in design/assets/ or a designed placeholder.", m.as_str()));
    }
}

/// Every constant expression of this file, for the test that compiles them.
#[cfg(test)]
pub(super) fn patterns() -> Vec<&'static regex::Regex> {
    vec![
        &HEX6,
        &BODY_OR_TEMPLATE,
        &STYLE_OR_SCRIPT,
        &TEXT_BLOCK_OPEN,
        &TAG,
        &EMOJI,
        &ICON_CLASS,
        &FILLER,
        &METRIC,
        &PLACEHOLDER_IMG,
        &COPY_TELL,
        &VAGUE_ERROR,
        &CLICK_HERE,
        &CSS_BLOCK,
        &FONT_SIZE_PX,
        &FIELD_SELECTOR,
        &Z_INDEX,
        &GRADIENT,
        &GRADIENT_HEX,
        &BRACES,
        &THICK_LEFT_BORDER,
        &BORDER_RADIUS,
    ]
}
