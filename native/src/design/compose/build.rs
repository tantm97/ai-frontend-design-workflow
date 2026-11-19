//! Build outputs: `design/system/components.css` (`buildSystem`) and the composed copies in
//! `.dfw/build/` (`buildComposed`).

use super::*;

#[cfg(test)]
/// The result of [`compose_file`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComposedFile {
    pub composed: Composed,
    /// The file uses components and does not link `system/components.css` itself.
    pub needs_components_css: bool,
}

#[cfg(test)]
/// `composeFile`: compose a design HTML file (path relative to the project root).
pub fn compose_file(root: &Path, rel: &str) -> Result<ComposedFile, DfwError> {
    let text = js::read_text(&root.join(rel))?;
    let base = design_base_for(rel);
    let composed = compose_html(
        root,
        &text,
        ComposeOptions {
            lib: None,
            design_base: Some(&base),
        },
    )?;
    let needs_components_css =
        !text.contains("system/components.css") && !composed.instances.is_empty();
    Ok(ComposedFile {
        composed,
        needs_components_css,
    })
}

/// What [`build_system`] found and wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltSystem {
    /// Number of masters.
    pub components: usize,
    /// Files written (relative to the project root): `components.css` when its text changed.
    pub written: Vec<String>,
}

/// `buildSystem`: write `design/system/components.css` from the masters when it changed.
pub fn build_system(root: &Path) -> Result<BuiltSystem, DfwError> {
    let lib = load_components(root)?;
    let sys = system_dir(root);
    let mut written = Vec::new();
    if !lib.is_empty() {
        fs::create_dir_all(&sys)?;
        let file = sys.join("components.css");
        let css = components_css(&lib);
        if !file.exists() || js::read_text(&file)? != css {
            fs::write(&file, css)?;
            written.push(to_posix(&relative_path(root, &file)));
        }
    }
    Ok(BuiltSystem {
        components: lib.len(),
        written,
    })
}

/// One entry of [`build_composed`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuiltFile {
    /// The design file, relative to the project root.
    pub rel: String,
    /// Plain HTML to hand to a tool: the copy under `.dfw/build/`, or the file itself when it
    /// uses no dfw element.
    pub out: PathBuf,
    pub composed: bool,
}

/// The local stylesheet a `<link rel="stylesheet" href="...">` tag names, as written.
fn stylesheet_href<'a>(html: &'a str, tag: &Tag) -> Option<&'a str> {
    let quoted = |attr: &&Attr| attr.quote != crate::design::html::Quote::Bare;
    let raw = |attr: &Attr| attr.value.clone().map(|r| &html[r]).unwrap_or_default();
    tag.attrs.iter().filter(quoted).find(|a| {
        html[a.name.clone()].eq_ignore_ascii_case("rel")
            && raw(a).eq_ignore_ascii_case("stylesheet")
    })?;
    let href = tag
        .attrs
        .iter()
        .filter(quoted)
        .find(|a| &html[a.name.clone()] == "href")
        .map(raw)
        .filter(|h| !h.is_empty())?;
    let remote = ["//", "http://", "https://"]
        .iter()
        .any(|p| href.starts_with(p));
    (!remote).then_some(href)
}

/// `buildComposed`: compose each design file into `.dfw/build/<same path>` for tools that need
/// plain HTML. Local stylesheets are inlined so static checkers see the real styles, and a
/// `<base>` keeps other relative links (images, fonts) working from the copy.
pub fn build_composed(root: &Path, rels: &[String]) -> Result<Vec<BuiltFile>, DfwError> {
    let out_dir = project_paths(root).dfw.join("build");
    let lib = load_components(root)?;
    let mut res = Vec::new();
    for rel in rels {
        let abs = root.join(rel);
        if !abs.exists() {
            continue;
        }
        let text = js::read_text(&abs)?;
        let uses_dfw = ["<dfw-use", "<dfw-icon", "<dfw-head"].iter().any(|p| {
            text.match_indices(p)
                .any(|(i, _)| starts_tag(&text[i..], p))
        });
        if !uses_dfw {
            res.push(BuiltFile {
                rel: rel.clone(),
                out: abs,
                composed: false,
            });
            continue;
        }
        let base = design_base_for(rel);
        let composed = compose_html(
            root,
            &text,
            ComposeOptions {
                lib: Some(&lib),
                design_base: Some(&base),
            },
        )?;
        let out = out_dir.join(rel);
        let (from_dir, out_parent) = (
            abs.parent().unwrap_or(root),
            out.parent().unwrap_or(&out_dir),
        );
        fs::create_dir_all(out_parent)?;
        let html = &composed.html;
        let mut inlined = String::with_capacity(html.len());
        let mut done = 0;
        for token in Tokenizer::new(html) {
            let Token::Open(tag) = token else { continue };
            if !tag.is(html, "link") {
                continue;
            }
            let Some(href) = stylesheet_href(html, &tag) else {
                continue;
            };
            let file = resolve(from_dir.join(href.split('?').next().unwrap_or_default()));
            if file.exists() {
                inlined.push_str(&html[done..tag.range.start]);
                inlined.push_str(&format!(
                    "<style data-dfw-inlined=\"{}\">\n{}\n</style>",
                    escape_html(href),
                    js::read_text(&file)?
                ));
                done = tag.range.end;
            }
        }
        inlined.push_str(&html[done..]);
        let base_href = match to_posix(&relative_path(out_parent, from_dir)) {
            rel if rel.is_empty() => ".".to_string(),
            rel => rel,
        };
        // The Node pattern was `<head[^>]*>`: the first tag whose name starts with "head".
        let head = Tokenizer::new(&inlined).find_map(|token| match token {
            Token::Open(tag)
                if tag
                    .name(&inlined)
                    .get(..4)
                    .is_some_and(|n| n.eq_ignore_ascii_case("head")) =>
            {
                Some(tag.range.end)
            }
            _ => None,
        });
        if let Some(at) = head {
            inlined.insert_str(at, &format!("\n<base href=\"{base_href}/\">"));
        }
        fs::write(&out, inlined)?;
        res.push(BuiltFile {
            rel: rel.clone(),
            out,
            composed: true,
        });
    }
    Ok(res)
}
