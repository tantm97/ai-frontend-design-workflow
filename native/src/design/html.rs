//! The HTML tokenizer (new; no Node file, design decision 2): tags, attributes, comments and raw text with
//! byte offsets. Replaces the regular expressions of `compose.mjs`, `lint.mjs` and `edit.mjs`.
//!
//! Nothing here builds HTML: every token and helper returns byte ranges of the source, and a
//! caller changes a file by splicing those ranges, so untouched bytes stay as written.
//!
//! The grammar is the one the Node regular expressions accepted, so well-formed files give the
//! same result as before:
//! - a tag is `<name` or `</name` and runs to the first `>` outside quotes; a quote that never
//!   closes means it is not a tag (the `<` is text);
//! - a name is `[A-Za-z][A-Za-z0-9_-]*` and does not end in `-`;
//! - an attribute is the `ATTR_RE` of compose.mjs: a name, then optionally `=` and a double
//!   quoted, single quoted or bare value;
//! - a comment is `<!--` to the next `-->`; without one the `<!--` is text;
//! - `<!doctype ...>` and `<?xml ...?>` are text.
//!
//! What the regular expressions could not do: the content of `<script>` and `<style>` is one raw
//! text token (to the next `</script` or `</style`, as a browser reads it), and a tag is only
//! found where a tag can start (never inside a comment, a quoted attribute or raw text).

// Over 800 lines with its tests: the tokenizer and the helpers built on it are one unit that
// lint, edit and the Studio import as one module.
use crate::core::js;
use std::ops::Range;

/// Elements that never have content or a closing tag (`VOID` in compose.mjs).
pub const VOID: [&str; 13] = [
    "area", "base", "br", "col", "embed", "hr", "img", "input", "link", "meta", "source", "track",
    "wbr",
];

/// How an attribute value is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quote {
    /// `name="value"`
    Double,
    /// `name='value'`
    Single,
    /// `name=value`, or no value at all (`value` is `None`).
    Bare,
}

/// One attribute of a tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attr {
    /// The name.
    pub name: Range<usize>,
    /// The value without its quotes; `None` for `<input disabled>`.
    pub value: Option<Range<usize>>,
    /// Quote style of the value.
    pub quote: Quote,
    /// From the first byte of the name to the end of the value, closing quote included.
    pub span: Range<usize>,
}

/// An opening or closing tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tag {
    /// From `<` to `>` inclusive.
    pub range: Range<usize>,
    /// The name as written (compare with [`Tag::is`]).
    pub name: Range<usize>,
    /// Attributes in source order (empty for a closing tag).
    pub attrs: Vec<Attr>,
    /// The tag ends in `/>` (`/` then optional white space, as compose.mjs reads it).
    pub self_closing: bool,
}

impl Tag {
    /// The name as written.
    pub fn name<'a>(&self, src: &'a str) -> &'a str {
        &src[self.name.clone()]
    }

    /// True when the name is `name`, ignoring ASCII case.
    pub fn is(&self, src: &str, name: &str) -> bool {
        self.name(src).eq_ignore_ascii_case(name)
    }

    /// The attribute called exactly `name`; the last one when it is repeated (the value a
    /// JavaScript object built in source order ends with).
    pub fn attr(&self, src: &str, name: &str) -> Option<&Attr> {
        self.attrs
            .iter()
            .rev()
            .find(|a| &src[a.name.clone()] == name)
    }

    #[cfg(test)]
    /// The decoded value of attribute `name`: `None` when the attribute is missing, an empty
    /// string when it has no value.
    pub fn value(&self, src: &str, name: &str) -> Option<String> {
        self.attr(src, name).map(|a| attr_value(src, a))
    }
}

/// The decoded value of an attribute (empty when it has none).
pub fn attr_value(src: &str, attr: &Attr) -> String {
    attr.value
        .clone()
        .map(|r| decode_entities(&src[r]))
        .unwrap_or_default()
}

/// `decode` of compose.mjs: the six entities an attribute value may carry, in one pass.
pub fn decode_entities(s: &str) -> String {
    const ENTITIES: [(&str, char); 6] = [
        ("&quot;", '"'),
        ("&#39;", '\''),
        ("&apos;", '\''),
        ("&lt;", '<'),
        ("&gt;", '>'),
        ("&amp;", '&'),
    ];
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        match ENTITIES.iter().find(|(e, _)| rest.starts_with(e)) {
            Some((e, c)) => {
                out.push(*c);
                rest = &rest[e.len()..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// One piece of the source. Concatenating the ranges of all tokens gives the source back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token {
    /// `<name ...>` or `<name ... />`.
    Open(Tag),
    /// `</name>`.
    Close(Tag),
    /// `<!-- ... -->`.
    Comment(Range<usize>),
    /// Anything else. A text token ends before the next `<`, so a `<` that starts no tag
    /// (`a < b`, `<!doctype html>`, a tag whose quote never closes) begins a text token.
    Text(Range<usize>),
    /// The content of `<script>` or `<style>`.
    RawText(Range<usize>),
}

#[cfg(test)]
impl Token {
    /// The bytes of the source this token covers.
    pub fn range(&self) -> Range<usize> {
        match self {
            Token::Open(tag) | Token::Close(tag) => tag.range.clone(),
            Token::Comment(r) | Token::Text(r) | Token::RawText(r) => r.clone(),
        }
    }
}

/// Iterator over the tokens of a source text.
#[derive(Debug, Clone)]
pub struct Tokenizer<'a> {
    src: &'a str,
    pos: usize,
    /// Name of the `<script>` or `<style>` just opened: the next token is its raw text.
    raw: Option<Range<usize>>,
}

impl<'a> Tokenizer<'a> {
    /// Tokens from the start of `src`.
    pub fn new(src: &'a str) -> Self {
        Tokenizer::at(src, 0)
    }

    /// Tokens from byte `pos`, which must be outside a tag, a comment and raw text.
    pub fn at(src: &'a str, pos: usize) -> Self {
        Tokenizer {
            src,
            pos,
            raw: None,
        }
    }

    /// Tokens of `src[range]`; nothing past `range.end` is read, offsets stay those of `src`.
    pub fn within(src: &'a str, range: Range<usize>) -> Self {
        Tokenizer::at(&src[..range.end], range.start)
    }

    /// Continue from byte `pos` (same condition as [`Tokenizer::at`]).
    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
        self.raw = None;
    }

    /// The raw text of the element whose opening tag was the previous token.
    fn raw_text(&mut self, name: Range<usize>) -> Option<Token> {
        let name = &self.src[name];
        let mut from = self.pos;
        let end = loop {
            match self.src[from..].find("</") {
                None => break self.src.len(),
                Some(i) => {
                    let at = from + i;
                    match parse_tag(self.src, at) {
                        Some((tag, true)) if tag.is(self.src, name) => break at,
                        _ => from = at + 2,
                    }
                }
            }
        };
        if end == self.pos {
            return None;
        }
        let range = self.pos..end;
        self.pos = end;
        Some(Token::RawText(range))
    }
}

impl Iterator for Tokenizer<'_> {
    type Item = Token;

    fn next(&mut self) -> Option<Token> {
        if let Some(name) = self.raw.take() {
            if let Some(raw) = self.raw_text(name) {
                return Some(raw);
            }
        }
        let src = self.src;
        let start = self.pos;
        if start >= src.len() {
            return None;
        }
        let rest = &src[start..];
        if let Some(body) = rest.strip_prefix("<!--") {
            if let Some(i) = body.find("-->") {
                self.pos = start + 4 + i + 3;
                return Some(Token::Comment(start..self.pos));
            }
        } else if rest.starts_with('<') {
            if let Some((tag, closing)) = parse_tag(src, start) {
                self.pos = tag.range.end;
                if closing {
                    return Some(Token::Close(tag));
                }
                if !tag.self_closing && (tag.is(src, "script") || tag.is(src, "style")) {
                    self.raw = Some(tag.name.clone());
                }
                return Some(Token::Open(tag));
            }
        }
        let skip = rest.chars().next().map_or(1, char::len_utf8);
        self.pos = match rest[skip..].find('<') {
            Some(i) => start + skip + i,
            None => src.len(),
        };
        Some(Token::Text(start..self.pos))
    }
}

fn is_name_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

/// The tag that starts at byte `at` (`<` or `</`), and whether it is a closing tag.
fn parse_tag(src: &str, at: usize) -> Option<(Tag, bool)> {
    let bytes = src.as_bytes();
    if bytes.get(at) != Some(&b'<') {
        return None;
    }
    let closing = bytes.get(at + 1) == Some(&b'/');
    let name_start = at + 1 + usize::from(closing);
    if !bytes.get(name_start)?.is_ascii_alphabetic() {
        return None;
    }
    let mut name_end = name_start + 1;
    while bytes.get(name_end).is_some_and(|b| is_name_byte(*b)) {
        name_end += 1;
    }
    // The regular expressions ended a name at a word boundary: trailing dashes are not part of
    // it. The first byte is a letter, so the loop stops there at the latest.
    while bytes[name_end - 1] == b'-' {
        name_end -= 1;
    }
    // Only ASCII bytes are inspected, so every index used below is a character boundary.
    let mut i = name_end;
    let gt = loop {
        match *bytes.get(i)? {
            b'>' => break i,
            q @ (b'"' | b'\'') => i += 2 + bytes[i + 1..].iter().position(|b| *b == q)?,
            _ => i += 1,
        }
    };
    let trimmed = src[name_end..gt].trim_end_matches(js::is_space);
    let self_closing = trimmed.ends_with('/');
    let attrs_end = if self_closing {
        name_end + trimmed.len() - 1
    } else {
        gt
    };
    let attrs = if closing {
        Vec::new()
    } else {
        parse_attrs(src, name_end..attrs_end)
    };
    let tag = Tag {
        range: at..gt + 1,
        name: name_start..name_end,
        attrs,
        self_closing,
    };
    Some((tag, closing))
}

/// The opening tag that starts at byte `at`, if one does.
pub fn open_tag_at(src: &str, at: usize) -> Option<Tag> {
    match parse_tag(src, at) {
        Some((tag, false)) => Some(tag),
        _ => None,
    }
}

/// The attributes written in `src[range]` (the part of a tag after its name), as `ATTR_RE` of
/// compose.mjs reads them.
pub fn parse_attrs(src: &str, range: Range<usize>) -> Vec<Attr> {
    let name_stop = |c: char| js::is_space(c) || matches!(c, '"' | '\'' | '<' | '>' | '/' | '=');
    let bare_stop = |c: char| js::is_space(c) || matches!(c, '"' | '\'' | '=' | '<' | '>' | '`');
    let end = range.end;
    let skip_space = |from: usize| {
        src[from..end]
            .find(|c: char| !js::is_space(c))
            .map_or(end, |i| from + i)
    };
    let mut attrs = Vec::new();
    let mut pos = range.start;
    while let Some(c) = src[pos..end].chars().next() {
        if name_stop(c) {
            pos += c.len_utf8();
            continue;
        }
        let name_end = src[pos..end].find(name_stop).map_or(end, |i| pos + i);
        let mut attr = Attr {
            name: pos..name_end,
            value: None,
            quote: Quote::Bare,
            span: pos..name_end,
        };
        let eq = skip_space(name_end);
        if src[eq..end].starts_with('=') {
            let v = skip_space(eq + 1);
            let value = match src[v..end].chars().next() {
                Some(q @ ('"' | '\'')) => src[v + 1..end].find(q).map(|i| {
                    let quote = if q == '"' {
                        Quote::Double
                    } else {
                        Quote::Single
                    };
                    (v + 1..v + 1 + i, quote, v + 1 + i + 1)
                }),
                Some(c) if !bare_stop(c) => {
                    let stop = src[v..end].find(bare_stop).map_or(end, |i| v + i);
                    Some((v..stop, Quote::Bare, stop))
                }
                _ => None,
            };
            if let Some((value, quote, stop)) = value {
                attr.value = Some(value);
                attr.quote = quote;
                attr.span.end = stop;
            }
        }
        pos = attr.span.end;
        attrs.push(attr);
    }
    attrs
}

/// An element: its opening tag, its content and its full extent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// The opening tag.
    pub open: Tag,
    /// Between the opening and the closing tag (empty for a void or self-closing element).
    pub inner: Range<usize>,
    /// From `<` of the opening tag to `>` of the closing tag.
    pub range: Range<usize>,
    /// No closing tag was found: `inner` and `range` run to the end of the text.
    pub unclosed: bool,
}

/// `findElement` of compose.mjs: the element whose opening tag starts at byte `start`.
/// Elements of the same name nest; tags inside comments, quoted attributes and raw text do not
/// count.
pub fn element_at(src: &str, start: usize) -> Option<Element> {
    element_within(src, start, src.len())
}

/// [`element_at`] that reads nothing past byte `end` (an element still open there is unclosed).
pub fn element_within(src: &str, start: usize, end: usize) -> Option<Element> {
    let open = open_tag_at(&src[..end], start)?;
    let name = open.name(src);
    let open_end = open.range.end;
    if open.self_closing || VOID.iter().any(|v| name.eq_ignore_ascii_case(v)) {
        let range = open.range.clone();
        return Some(Element {
            open,
            inner: open_end..open_end,
            range,
            unclosed: false,
        });
    }
    let mut tokens = Tokenizer::within(src, start..end);
    tokens.next(); // the opening tag itself (this also arms raw text for <script> and <style>)
    let mut depth = 1;
    for token in tokens {
        match token {
            Token::Open(tag) if tag.is(src, name) && !tag.self_closing => depth += 1,
            Token::Close(tag) if tag.is(src, name) => {
                depth -= 1;
                if depth == 0 {
                    return Some(Element {
                        inner: open_end..tag.range.start,
                        range: start..tag.range.end,
                        open,
                        unclosed: false,
                    });
                }
            }
            _ => {}
        }
    }
    Some(Element {
        open,
        inner: open_end..end,
        range: start..end,
        unclosed: true,
    })
}

#[cfg(test)]
/// Every opening tag that carries attribute `name` with the decoded value `value`.
pub fn tags_with_attr(src: &str, name: &str, value: &str) -> Vec<Tag> {
    Tokenizer::new(src)
        .filter_map(|token| match token {
            Token::Open(tag) if tag.value(src, name).as_deref() == Some(value) => Some(tag),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
/// The first element that carries attribute `name` with the decoded value `value`
/// (`find_by_attr(text, "data-dfw-id", "items.add")`).
pub fn find_by_attr(src: &str, name: &str, value: &str) -> Option<Element> {
    let tag = Tokenizer::new(src).find_map(|token| match token {
        Token::Open(tag) if tag.value(src, name).as_deref() == Some(value) => Some(tag),
        _ => None,
    })?;
    element_at(src, tag.range.start)
}

#[cfg(test)]
/// A direct child of an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    /// A child element (unclosed when it is still open where its parent ends).
    Element(Element),
    /// A comment.
    Comment(Range<usize>),
    /// Text, raw text, or a closing tag that closes nothing.
    Text(Range<usize>),
}

#[cfg(test)]
impl Node {
    /// The bytes of the source this child covers.
    pub fn range(&self) -> Range<usize> {
        match self {
            Node::Element(el) => el.range.clone(),
            Node::Comment(r) | Node::Text(r) => r.clone(),
        }
    }
}

#[cfg(test)]
/// The direct children of an element in source order; their ranges cover `el.inner` exactly.
pub fn children(src: &str, el: &Element) -> Vec<Node> {
    if el.open.is(src, "script") || el.open.is(src, "style") {
        return if el.inner.is_empty() {
            Vec::new()
        } else {
            vec![Node::Text(el.inner.clone())]
        };
    }
    nodes(src, el.inner.clone())
}

#[cfg(test)]
/// The top-level nodes of `src[range]`, which must be ordinary content (not the inside of a
/// tag, a comment, `<script>` or `<style>`); their ranges cover `range` exactly.
pub fn nodes(src: &str, range: Range<usize>) -> Vec<Node> {
    let mut nodes = Vec::new();
    let mut tokens = Tokenizer::within(src, range.clone());
    while let Some(token) = tokens.next() {
        match token {
            Token::Open(tag) => match element_within(src, tag.range.start, range.end) {
                Some(el) => {
                    tokens.seek(el.range.end);
                    nodes.push(Node::Element(el));
                }
                None => nodes.push(Node::Text(tag.range)),
            },
            Token::Comment(r) => nodes.push(Node::Comment(r)),
            Token::Close(tag) => nodes.push(Node::Text(tag.range)),
            Token::Text(r) | Token::RawText(r) => nodes.push(Node::Text(r)),
        }
    }
    nodes
}

/// A replacement of `range` by `text`, to apply to the source it was computed from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Splice {
    /// The bytes to replace (empty for an insertion).
    pub range: Range<usize>,
    /// What to put there.
    pub text: String,
}

impl Splice {
    /// `src` with the replacement made; every other byte is copied as it is.
    pub fn apply(&self, src: &str) -> String {
        let mut out = String::with_capacity(src.len() + self.text.len());
        out.push_str(&src[..self.range.start]);
        out.push_str(&self.text);
        out.push_str(&src[self.range.end..]);
        out
    }
}

/// `escAttr` of edit.mjs: a value written between double quotes.
pub fn escape_attr(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
}

/// `setAttr` of edit.mjs as a splice: set (`Some`) or remove (`None`) one attribute of `tag`.
/// The name matches ignoring ASCII case; a set writes ` name="value"` over the old attribute or
/// before the closing `>` (or `/>`). `None` when there is nothing to remove.
pub fn set_attr(src: &str, tag: &Tag, name: &str, value: Option<&str>) -> Option<Splice> {
    let text = value
        .map(|v| format!(" {name}=\"{}\"", escape_attr(v)))
        .unwrap_or_default();
    // The white space in front of an attribute goes with it.
    let space_before = |at: usize| {
        let lead = &src[tag.name.end..at];
        tag.name.end + lead.trim_end_matches(js::is_space).len()
    };
    let existing = tag
        .attrs
        .iter()
        .find(|a| src[a.name.clone()].eq_ignore_ascii_case(name));
    if let Some(attr) = existing {
        return Some(Splice {
            range: space_before(attr.span.start)..attr.span.end,
            text,
        });
    }
    value?;
    let mut at = tag.range.end - 1;
    if src[..at].ends_with('/') {
        at -= 1;
    }
    let at = space_before(at);
    Some(Splice {
        range: at..at,
        text,
    })
}

/// 1-based line of byte `index` (`lineOf` of compose.mjs).
pub fn line_of(src: &str, index: usize) -> usize {
    src.as_bytes()[..index.min(src.len())]
        .iter()
        .filter(|b| **b == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    fn html_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                html_files(&path, out);
            } else if path.extension().is_some_and(|e| e == "html") {
                out.push(path);
            }
        }
    }

    fn joined(src: &str) -> String {
        let mut at = 0;
        let mut out = String::new();
        for token in Tokenizer::new(src) {
            let range = token.range();
            assert_eq!(range.start, at, "tokens are contiguous in {src:?}");
            assert!(range.end > range.start, "no empty token in {src:?}");
            at = range.end;
            out.push_str(&src[range]);
        }
        assert_eq!(at, src.len());
        out
    }

    fn kinds(src: &str) -> Vec<String> {
        Tokenizer::new(src)
            .map(|t| {
                let text = &src[t.range()];
                match t {
                    Token::Open(_) => format!("open {text}"),
                    Token::Close(_) => format!("close {text}"),
                    Token::Comment(_) => format!("comment {text}"),
                    Token::Text(_) => format!("text {text}"),
                    Token::RawText(_) => format!("raw {text}"),
                }
            })
            .collect()
    }

    fn open(src: &str) -> Tag {
        open_tag_at(src, 0).unwrap()
    }

    #[test]
    fn every_fixture_kit_and_template_file_round_trips() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut files = Vec::new();
        for dir in ["tests/fixtures", "core/kits", "core/templates"] {
            html_files(&repo.join(dir), &mut files);
        }
        assert!(files.len() >= 30, "found {} html files", files.len());
        for file in files {
            let bytes = fs::read(&file).unwrap();
            let src = String::from_utf8(bytes.clone()).unwrap();
            assert_eq!(joined(&src).as_bytes(), bytes, "{}", file.display());
            // The same through the tree helpers: top-level nodes cover the file.
            let all: String = nodes(&src, 0..src.len())
                .iter()
                .map(|n| &src[n.range()])
                .collect();
            assert_eq!(all, src, "{}", file.display());
        }
    }

    #[test]
    fn hostile_inputs_round_trip() {
        for src in [
            "",
            "<",
            "<<<>>>",
            "a < b && c > d",
            "<!--",
            "<!-->",
            "<!-- a --><!-- b",
            "<p title=\"unclosed>text</p>",
            "<p title='a>b' class=x>é ü 日本</p\n>",
            "<script>if (a</b>) '</script>'</script>",
            "<style>a{}</style",
            "<SCRIPT>x</ScRiPt >y",
            "<a-></a->",
            "<!doctype html>\r\n<html>\r\n</html>\r\n",
            "</>< /p><1>",
            "\u{feff}<p>\u{2028}</p>",
        ] {
            assert_eq!(joined(src), src);
        }
    }

    #[test]
    fn comments_containing_tags_are_one_token() {
        let src = "<div><!-- </div> <dfw-use c=\"x\"> --></div>";
        assert_eq!(
            kinds(src),
            [
                "open <div>",
                "comment <!-- </div> <dfw-use c=\"x\"> -->",
                "close </div>"
            ]
        );
        let el = element_at(src, 0).unwrap();
        assert_eq!(el.range, 0..src.len());
        assert!(!el.unclosed);
        // An unterminated comment is text (the regular expressions did not match it either).
        assert_eq!(
            kinds("<!-- x <b>y</b>"),
            ["text <!-- x ", "open <b>", "text y", "close </b>"]
        );
    }

    #[test]
    fn a_gt_inside_a_quoted_attribute_does_not_end_the_tag() {
        let src = "<a title=\"x > y\" data-x='</a>'>go</a>";
        let el = element_at(src, 0).unwrap();
        assert_eq!(
            &src[el.open.range.clone()],
            "<a title=\"x > y\" data-x='</a>'>"
        );
        assert_eq!(&src[el.inner.clone()], "go");
        assert_eq!(el.open.value(src, "title").as_deref(), Some("x > y"));
        assert_eq!(el.open.value(src, "data-x").as_deref(), Some("</a>"));
    }

    #[test]
    fn attribute_values_double_single_bare_and_missing() {
        let src = "<input type=\"text\" name='q' size=20 disabled data-x = \"1\" a=&quot;b>";
        let tag = open(src);
        let got: Vec<(&str, Option<&str>, Quote)> = tag
            .attrs
            .iter()
            .map(|a| {
                (
                    &src[a.name.clone()],
                    a.value.clone().map(|r| &src[r]),
                    a.quote,
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("type", Some("text"), Quote::Double),
                ("name", Some("q"), Quote::Single),
                ("size", Some("20"), Quote::Bare),
                ("disabled", None, Quote::Bare),
                ("data-x", Some("1"), Quote::Double),
                ("a", Some("&quot;b"), Quote::Bare),
            ]
        );
        assert_eq!(&src[tag.attrs[1].span.clone()], "name='q'");
        assert_eq!(&src[tag.attrs[4].span.clone()], "data-x = \"1\"");
        assert_eq!(tag.value(src, "a").as_deref(), Some("\"b"));
        assert_eq!(tag.value(src, "disabled").as_deref(), Some(""));
        assert_eq!(tag.value(src, "nope"), None);
        // A repeated attribute: the last one wins, as in the object parseAttrs built.
        let twice = "<i a=1 a=2>";
        assert_eq!(open(twice).value(twice, "a").as_deref(), Some("2"));
        assert_eq!(
            decode_entities("&amp;lt; &#39;&apos; &nbsp; &"),
            "&lt; '' &nbsp; &"
        );
    }

    #[test]
    fn self_closing_and_void_elements_have_no_content() {
        let src = "<br><img src=a.png /><dfw-icon name=\"x\"/><p>t</p>";
        let br = element_at(src, 0).unwrap();
        assert_eq!((br.range.clone(), br.inner.is_empty()), (0..4, true));
        let img = element_at(src, 4).unwrap();
        assert!(img.open.self_closing);
        assert_eq!(img.open.value(src, "src").as_deref(), Some("a.png"));
        let icon = element_at(src, img.range.end).unwrap();
        assert!(icon.open.self_closing && !icon.unclosed);
        assert_eq!(&src[icon.range.clone()], "<dfw-icon name=\"x\"/>");
    }

    #[test]
    fn crlf_line_endings_are_kept_and_counted() {
        let src = "<div\r\n  class=\"a\"\r\n>\r\n<p>x</p>\r\n</div>\r\n";
        assert_eq!(joined(src), src);
        let el = element_at(src, 0).unwrap();
        assert_eq!(el.open.value(src, "class").as_deref(), Some("a"));
        assert_eq!(&src[el.inner.clone()], "\r\n<p>x</p>\r\n");
        assert_eq!(line_of(src, src.find("<p>").unwrap()), 4);
    }

    #[test]
    fn unclosed_elements_are_reported() {
        let src = "<main><dfw-use c=\"a\"><p>x</p></main>";
        let el = element_at(src, 6).unwrap();
        assert!(el.unclosed);
        assert_eq!(el.range, 6..src.len());
        assert_eq!(el.inner, 21..src.len());
        // A quote that never closes: not a tag at all.
        assert_eq!(element_at("<p title=\"x>y</p>", 0), None);
        assert_eq!(
            kinds("<p title=\"x>y</p>"),
            ["text <p title=\"x>y", "close </p>"]
        );
        assert_eq!(element_at("<p", 0), None);
    }

    #[test]
    fn nested_elements_of_the_same_name_are_counted() {
        let src = "<div id=a><div id=b><div/></div><DIV></div></div><div id=c></div>";
        let a = element_at(src, 0).unwrap();
        assert_eq!(
            &src[a.range.clone()],
            "<div id=a><div id=b><div/></div><DIV></div></div>"
        );
        let kids = children(src, &a);
        assert_eq!(kids.len(), 2);
        assert_eq!(&src[kids[0].range()], "<div id=b><div/></div>");
        assert_eq!(&src[kids[1].range()], "<DIV></div>");
        // A longer name is another element: <dfw-use-x> does not nest in <dfw-use>.
        let src = "<dfw-use><dfw-use-x></dfw-use></dfw-use-x>";
        assert_eq!(
            &src[element_at(src, 0).unwrap().range],
            "<dfw-use><dfw-use-x></dfw-use>"
        );
    }

    #[test]
    fn script_and_style_content_is_raw_text() {
        // As in a browser, the first </script ends the script, also inside a string.
        let src = "<script>var s = \"<div></script>\"; </script><style>a > b {}</style><p>";
        assert_eq!(
            kinds(src),
            [
                "open <script>",
                "raw var s = \"<div>",
                "close </script>",
                "text \"; ",
                "close </script>",
                "open <style>",
                "raw a > b {}",
                "close </style>",
                "open <p>",
            ]
        );
        // Tags and comments inside raw text are not tokens, so they do not nest or close anything.
        let src =
            "<div><script>document.write('</div><!--')</script><style>/* </div> */</style></div>";
        let el = element_at(src, 0).unwrap();
        assert_eq!(el.range, 0..src.len());
        assert!(!el.unclosed);
        let script = element_at(src, 5).unwrap();
        assert_eq!(&src[script.inner.clone()], "document.write('</div><!--')");
        assert_eq!(children(src, &script), [Node::Text(script.inner.clone())]);
        assert_eq!(
            kinds("<script></script>"),
            ["open <script>", "close </script>"]
        );
        assert_eq!(kinds("<style>a{}"), ["open <style>", "raw a{}"]);
    }

    #[test]
    fn find_by_attribute_value() {
        let src = "<main data-dfw-id=\"m\"><p data-dfw-id='m.t' class=\"x\">a &amp; b</p><!-- <i data-dfw-id=\"c\"> --></main>";
        let p = find_by_attr(src, "data-dfw-id", "m.t").unwrap();
        assert_eq!(&src[p.inner.clone()], "a &amp; b");
        assert_eq!(
            find_by_attr(src, "data-dfw-id", "c"),
            None,
            "not inside a comment"
        );
        assert_eq!(tags_with_attr(src, "class", "x").len(), 1);
        let src = "<a title=\"a &amp; b\">";
        let a = find_by_attr(src, "title", "a & b").unwrap();
        assert!(a.unclosed);
        assert_eq!(a.range, 0..src.len());
    }

    #[test]
    fn direct_children_cover_the_content() {
        let src = "<ul> <li>a<ul><li>b</li></ul></li><!-- c --><li>d</ul>";
        let ul = element_at(src, 0).unwrap();
        let kids = children(src, &ul);
        let texts: Vec<&str> = kids.iter().map(|k| &src[k.range()]).collect();
        assert_eq!(
            texts,
            [" ", "<li>a<ul><li>b</li></ul></li>", "<!-- c -->", "<li>d"]
        );
        assert!(matches!(&kids[3], Node::Element(el) if el.unclosed));
        assert_eq!(texts.concat(), &src[ul.inner.clone()]);
    }

    #[test]
    fn rewrite_one_attribute_in_place() {
        let src = "<p  class='a'\n   ID=x hidden>t</p>";
        let tag = open(src);
        let set = |name: &str, value: Option<&str>| {
            set_attr(src, &tag, name, value).map(|s| s.apply(src))
        };
        assert_eq!(
            set("class", Some("b \"c\"")).unwrap(),
            "<p class=\"b &quot;c&quot;\"\n   ID=x hidden>t</p>"
        );
        assert_eq!(
            set("id", Some("y")).unwrap(),
            "<p  class='a' id=\"y\" hidden>t</p>"
        );
        assert_eq!(set("hidden", None).unwrap(), "<p  class='a'\n   ID=x>t</p>");
        assert_eq!(
            set("title", Some("a<b")).unwrap(),
            "<p  class='a'\n   ID=x hidden title=\"a&lt;b\">t</p>"
        );
        assert_eq!(set("title", None), None);
        let src = "<br />";
        assert_eq!(
            set_attr(src, &open(src), "class", Some("x"))
                .unwrap()
                .apply(src),
            "<br class=\"x\" />"
        );
    }

    #[test]
    fn names_follow_the_old_word_boundary() {
        let src = "<a- b>";
        let tag = open(src);
        assert_eq!(tag.name(src), "a");
        assert_eq!(&src[tag.attrs[0].name.clone()], "-");
        let src = "<x_y-z1 >";
        assert_eq!(open(src).name(src), "x_y-z1");
        assert_eq!(open_tag_at("<1a>", 0), None);
        assert_eq!(open_tag_at("</a>", 0), None);
    }
}
