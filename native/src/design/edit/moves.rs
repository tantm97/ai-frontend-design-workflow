//! Moving the source of an element: `lineSpan`, `moveSource` and `moveInside` of `edit.mjs`.
//! The result is the original text with one range cut and put back elsewhere; every other byte
//! is copied as it is.

use super::locate::Located;
use crate::core::error::DfwError;
use crate::core::js;

/// Line facts for a source range.
struct LineSpan<'a> {
    /// Start of the line the range begins on.
    ls: usize,
    /// End of the line the range ends on (before its `\n`).
    le: usize,
    /// What is on the line before the range.
    indent: &'a str,
    /// The range sits alone on its line(s): only spaces and tabs around it.
    own: bool,
}

fn blank(s: &str) -> bool {
    s.bytes().all(|b| b == b' ' || b == b'\t')
}

fn line_span(text: &str, start: usize, end: usize) -> LineSpan<'_> {
    let ls = text[..start].rfind('\n').map_or(0, |i| i + 1);
    let le = text[end..].find('\n').map_or(text.len(), |i| end + i);
    let indent = &text[ls..start];
    LineSpan {
        ls,
        le,
        indent,
        own: blank(indent) && blank(&text[end..le]),
    }
}

/// True when `[ \t]*` is all there is before byte `at` on its line.
pub(super) fn starts_line(text: &str, at: usize) -> Option<&str> {
    let span = line_span(text, at, at);
    blank(span.indent).then_some(span.indent)
}

/// Start of the line of byte `at`.
pub(super) fn line_start(text: &str, at: usize) -> usize {
    line_span(text, at, at).ls
}

/// The lines of `start..end` when the range sits alone on them: `(line start, after the line's
/// newline, indentation)`.
pub(super) fn own_lines(text: &str, start: usize, end: usize) -> Option<(usize, usize, &str)> {
    let span = line_span(text, start, end);
    span.own
        .then(|| (span.ls, text.len().min(span.le + 1), span.indent))
}

/// The block with every line after the first moved from indentation `from` to `to`.
fn reindent(block: &str, from: &str, to: &str) -> String {
    let mut lines = block.split('\n');
    let mut out = lines.next().unwrap_or_default().to_string();
    for line in lines {
        out.push('\n');
        match line.strip_prefix(from) {
            Some(rest) => {
                out.push_str(to);
                out.push_str(rest);
            }
            None => out.push_str(line),
        }
    }
    out
}

/// Cut `cut` out of `text` and put `ins` at `at`, dropping `drop` bytes after `at`.
fn cut_and_insert(
    text: &str,
    cut: (usize, usize),
    at: usize,
    ins: &str,
    drop: usize,
) -> Result<String, DfwError> {
    if at > cut.0 && at < cut.1 {
        return Err(DfwError::new("Cannot move the element there."));
    }
    let mut out = String::with_capacity(text.len() + ins.len());
    if at >= cut.1 {
        out.push_str(&text[..cut.0]);
        out.push_str(&text[cut.1..at]);
        out.push_str(ins);
        out.push_str(&text[at + drop..]);
    } else {
        out.push_str(&text[..at]);
        out.push_str(ins);
        // The dropped range never reaches the cut (an element is not inside blank content).
        out.push_str(&text[(at + drop).min(cut.0)..cut.0]);
        out.push_str(&text[cut.1..]);
    }
    Ok(out)
}

/// `moveSource`: the text with element `el` moved before or after element `target`, keeping
/// indentation. `before` is the position; anything else is after.
pub fn move_source(
    text: &str,
    el: &Located,
    target: &Located,
    before: bool,
) -> Result<String, DfwError> {
    if target.start >= el.start && target.end <= el.end {
        return Err(DfwError::new("An element cannot be moved into itself."));
    }
    if el.start >= target.start && el.end <= target.end {
        return Err(DfwError::new(
            "Drop next to a sibling, not next to a parent of the element.",
        ));
    }
    let a = line_span(text, el.start, el.end);
    let b = line_span(text, target.start, target.end);
    let block = &text[el.start..el.end];
    if a.own && b.own {
        // Whole lines: cut the element's lines and insert them at the target's line with the
        // target's indentation.
        let cut = (a.ls, text.len().min(a.le + 1));
        let body = reindent(block, a.indent, b.indent);
        let (at, ins) = if before {
            (b.ls, format!("{}{body}\n", b.indent))
        } else if b.le < text.len() {
            (b.le + 1, format!("{}{body}\n", b.indent))
        } else {
            (b.le, format!("\n{}{body}", b.indent))
        };
        return cut_and_insert(text, cut, at, &ins, 0);
    }
    let at = if before { target.start } else { target.end };
    cut_and_insert(text, (el.start, el.end), at, block, 0)
}

/// `moveInside`: the text with element `el` moved to the end of container `target` (a drop
/// into an empty flex or grid container).
pub fn move_inside(text: &str, el: &Located, target: &Located) -> Result<String, DfwError> {
    if target.start >= el.start && target.end <= el.end {
        return Err(DfwError::new("An element cannot be moved into itself."));
    }
    if target.is_empty_tag() {
        return Err(DfwError::new(format!(
            "<{}> cannot contain other elements.",
            target.tag
        )));
    }
    if target.is_instance {
        return Err(DfwError::new(
            "Dropping into a component instance is not supported; its content is defined by the master and its slots.",
        ));
    }
    let a = line_span(text, el.start, el.end);
    let block = &text[el.start..el.end];
    let inner_blank = js::trim(&text[target.inner_start..target.inner_end]).is_empty();
    let box_line = line_span(text, target.start, target.start);
    let close_line = line_span(text, target.inner_end, target.inner_end);
    let mut cut = (el.start, el.end);
    let mut at = target.inner_end;
    let mut ins = block.to_string();
    if a.own {
        cut = (a.ls, text.len().min(a.le + 1));
        if blank(close_line.indent) && close_line.ls > target.inner_start {
            // The closing tag sits on its own line: insert a new line above it, one level deeper.
            let ind = format!("{}  ", close_line.indent);
            at = close_line.ls;
            ins = format!("{ind}{}\n", reindent(block, a.indent, &ind));
        } else if inner_blank && blank(box_line.indent) {
            // An empty one-line container: open it up.
            let ind = format!("{}  ", box_line.indent);
            at = target.inner_start;
            ins = format!(
                "\n{ind}{}\n{}",
                reindent(block, a.indent, &ind),
                box_line.indent
            );
        }
    }
    // White space that is all a one-line container holds is replaced; otherwise nothing is removed.
    let drop = if at == target.inner_start && inner_blank {
        target.inner_end - target.inner_start
    } else {
        0
    };
    cut_and_insert(text, cut, at, &ins, drop)
}
