//! Markdown to HTML with comrak, set to read what Python-Markdown reads (`tables`, `fenced_code`, `toc`,
//! `sane_lists`) and to write the ids it writes. A long text converts a part at a time
//! ([`convert_part`]), each part rendered from one parse of the whole.

use std::collections::HashSet;
use std::fmt::{self, Write};
use std::ops::Range;
use std::sync::Mutex;

use comrak::adapters::{HeadingAdapter, HeadingMeta};
use comrak::nodes::{AstNode, ListType, NodeValue, Sourcepos};
use comrak::options::Plugins;
use comrak::{Arena, Node, Options, format_html_with_plugins, parse_document};

use crate::edit::heading_anchor;

/// The id the toc gave a heading that another in the same conversion already had: `_1` after it, then
/// `_2` (Python-Markdown's `unique`), and `_1` for a heading with no letters in it.
fn unique(id: &str, used: &mut HashSet<String>) -> String {
    let mut id = id.to_string();
    while used.contains(&id) || id.is_empty() {
        let counted = id
            .rsplit_once('_')
            .and_then(|(stem, n)| Some((stem.to_string(), n.parse::<u64>().ok()?)));
        id = match counted {
            Some((stem, n)) => format!("{stem}_{}", n + 1),
            None => format!("{id}_1"),
        };
    }
    used.insert(id.clone());
    id
}

/// Writes a heading's tags with the anchor every `[[NAME#Heading]]` and `#anchor` link spells. The ids
/// taken so far sit behind a lock because the adapter must be `Send + Sync`; one conversion owns it.
struct Ids(Mutex<HashSet<String>>);

impl HeadingAdapter for Ids {
    fn enter(
        &self,
        output: &mut dyn Write,
        heading: &HeadingMeta,
        _: Option<Sourcepos>,
    ) -> fmt::Result {
        let mut used = self.0.lock().map_err(|_| fmt::Error)?;
        let id = unique(&heading_anchor(&heading.content), &mut used);
        write!(output, "<h{} id=\"{id}\">", heading.level)
    }

    fn exit(&self, output: &mut dyn Write, heading: &HeadingMeta) -> fmt::Result {
        write!(output, "</h{}>", heading.level)
    }
}

/// Python-Markdown accepts `#Title` as an ATX heading. CommonMark requires a space. Keep those
/// existing headings and their anchors while leaving fenced and indented code byte-for-byte alone.
/// One pass over the lines, beside the code and HTML blocks in the order they begin.
fn spaced_headings(text: &str, options: &Options<'_>) -> String {
    let arena = Arena::new();
    let document = parse_document(&arena, text, options);
    let mut protected: Vec<_> = document
        .descendants()
        .filter_map(|node| {
            let data = node.data.borrow();
            matches!(
                data.value,
                NodeValue::CodeBlock(_) | NodeValue::HtmlBlock(_)
            )
            .then_some(data.sourcepos)
        })
        .collect();
    symphony_board::order::sort_by_key(&mut protected, &|range| range.start.line);
    let mut protected = protected.into_iter().peekable();
    let mut out = String::with_capacity(text.len());
    for (index, line) in text.split_inclusive('\n').enumerate() {
        // A block that ends before this line holds none after it either.
        while protected.next_if(|range| range.end.line <= index).is_some() {}
        let protected = protected
            .peek()
            .is_some_and(|range| range.start.line <= index + 1);
        let hashes = line.bytes().take_while(|byte| *byte == b'#').count().min(6);
        if !protected
            && hashes > 0
            && line
                .as_bytes()
                .get(hashes)
                .is_some_and(|byte| !byte.is_ascii_whitespace())
        {
            out.push_str(&line[..hashes]);
            out.push(' ');
            out.push_str(&line[hashes..]);
        } else {
            out.push_str(line);
        }
    }
    out
}

/// What every conversion reads: tables, and raw HTML passed through as it was.
fn options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.render.r#unsafe = true;
    options
}

/// The document `root` as HTML, its headings numbered after the ids `taken` holds.
fn html(root: Node<'_>, options: &Options<'_>, taken: HashSet<String>) -> String {
    let ids = Ids(Mutex::new(taken));
    let mut plugins = Plugins::default();
    plugins.render.heading_adapter = Some(&ids);
    let mut out = String::new();
    format_html_with_plugins(root, options, &mut out, &plugins)
        .expect("HTML written to a String, with the ids' lock held by this call alone");
    out
}

/// `text` as HTML. Raw HTML in it passes through, as it did; the headings are numbered per call.
pub(crate) fn convert(text: &str) -> String {
    let options = options();
    let arena = Arena::new();
    let root = parse_document(&arena, &spaced_headings(text, &options), &options);
    html(root, &options, HashSet::new())
}

/// What a post is read with: a document's [`options`], with its raw HTML written as the text it is.
fn post_options() -> Options<'static> {
    let mut options = options();
    options.render.escape = true;
    options
}

/// A post's text as its parse reads it: every line that would open an HTML block a paragraph's
/// ([`unblocked`]), so the Markdown after it is read, and `#Title` spaced as a document's.
fn post_text(text: &str, options: &Options<'_>) -> String {
    spaced_headings(&unblocked(text.to_string(), options), options)
}

/// An agent's post outside a document (a task's message or brief, an ask's answer) as HTML: read
/// as [`convert`] reads a document, but raw HTML in it is the words typed (`Vec<String>`,
/// `<TokensPart key=x>`, `</div>`, a `<details>` named in prose), never markup, as in a note
/// ([`convert_note`]); its line breaks and headings are a document's.
pub(crate) fn convert_post(text: &str) -> String {
    let options = post_options();
    let arena = Arena::new();
    let root = parse_document(&arena, &post_text(text, &options), &options);
    html(root, &options, HashSet::new())
}

/// A card's note as HTML, read as [`convert`] reads a document but for what a note is not: each line
/// break in it is one (a blank line still a paragraph gap), no line of it is a heading
/// ([`unheaded`]), and raw HTML in it is the words typed (`Vec<String>`, `<b>`), never markup.
pub(crate) fn convert_note(text: &str) -> String {
    let mut options = post_options();
    options.render.hardbreaks = true;
    let arena = Arena::new();
    let root = parse_document(&arena, &unblocked(unheaded(text), &options), &options);
    html(root, &options, HashSet::new())
}

/// `text` with the `<` that begins each HTML block escaped, until it has none: in a note or a post
/// such a line (`<div>`, `<!-- … -->`, a tag alone) is words, and read as a paragraph's its lines
/// keep their breaks. An escape can let a later line begin a block of its own, so it parses again.
fn unblocked(mut text: String, options: &Options<'_>) -> String {
    loop {
        let arena = Arena::new();
        let root = parse_document(&arena, &text, options);
        let begins: Vec<(usize, usize)> = root
            .descendants()
            .filter_map(|node| {
                let data = node.data.borrow();
                matches!(data.value, NodeValue::HtmlBlock(_))
                    .then_some((data.sourcepos.start.line, data.sourcepos.start.column))
            })
            .collect();
        let starts = line_starts(&text);
        let mut escaped = text.clone();
        for (line, column) in begins.into_iter().rev() {
            let from = starts[line - 1] + column - 1;
            if let Some(angle) = text[from..].find('<') {
                escaped.insert(from + angle, '\\');
            }
        }
        if escaped == text {
            return text;
        }
        text = escaped;
    }
}

/// `text` with each line that would make a heading escaped, so it reads as the words it is: a `#`
/// line, in a block quote or a list item too (`> ## Q`, `- # Intro`), and an `=` or `-` line under a
/// line of text, which would underline it into one. Headings are the register's own structure; a
/// note under a card never has one. Fenced code is left alone.
pub(crate) fn unheaded(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    let (mut fenced, mut under_text) = (false, false);
    for line in text.split_inclusive('\n') {
        if crate::register::is_fence(line) || fenced {
            fenced ^= crate::register::is_fence(line);
            under_text = false;
            out.push_str(line);
            continue;
        }
        let quoted = past_markers(line, false);
        let words = line[quoted..].trim_end();
        let underline = under_text
            && !words.is_empty()
            && (words.chars().all(|c| c == '=') || words.chars().all(|c| c == '-'));
        let item = past_markers(line, true);
        let escape = if line[item..].starts_with('#') {
            Some(item)
        } else {
            underline.then_some(quoted)
        };
        if let Some(at) = escape {
            out.push_str(&line[..at]);
            out.push('\\');
            out.push_str(&line[at..]);
        } else {
            out.push_str(line);
        }
        under_text = !words.is_empty();
    }
    out
}

/// Where `line`'s words begin past its indentation and the block quote markers (`>`) before them;
/// with `items`, past list item markers too (`-`, `*`, `+`, `1.` or `1)`, then a space or a tab),
/// which begin a block that can hold a heading but never underline the line before.
fn past_markers(line: &str, items: bool) -> usize {
    let mut at = 0;
    loop {
        let rest = &line[at..];
        let body = rest.trim_start_matches([' ', '\t']);
        at += rest.len() - body.len();
        let bytes = body.as_bytes();
        let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        let spaced = |marker: usize| matches!(bytes.get(marker), Some(b' ' | b'\t'));
        let marker = match bytes.first() {
            Some(b'>') => 1,
            Some(b'-' | b'*' | b'+') if items && spaced(1) => 1,
            Some(b'0'..=b'9')
                if items
                    && digits <= 9
                    && matches!(bytes.get(digits), Some(b'.' | b')'))
                    && spaced(digits + 1) =>
            {
                digits + 1
            }
            _ => return at,
        };
        at += marker;
    }
}

/// Where each line of `text` begins, as CommonMark ends a line (a line feed, a carriage return, or
/// both together): line `n` of a source position begins at the `n - 1`th.
pub(crate) fn line_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = vec![0];
    let mut at = 0;
    while let Some(&byte) = bytes.get(at) {
        at += 1;
        if byte == b'\r' && bytes.get(at) == Some(&b'\n') {
            at += 1;
        }
        if byte == b'\n' || byte == b'\r' {
            starts.push(at);
        }
    }
    starts
}

/// A part of a converted text, and where the next part begins.
pub(crate) struct Part {
    pub(crate) html: String,
    /// Where the next part begins: the text's end when this part reaches it.
    pub(crate) next: usize,
    /// Whether this part is the whole text, converted as [`convert_post`] converts it.
    pub(crate) whole: bool,
}

/// The blocks of the post `text` ([`convert_post`]) from the one holding `from`, as many as
/// `limit` holds (one at least), as HTML, and where the next part begins. One parse of the whole
/// text decides every part: a part renders that parse's nodes, so a reference resolves wherever its
/// definition sits, a fence of any kind holds its lines, a list keeps its numbering and spacing,
/// and a heading the id the whole gives it. A block longer than `block` is cut between its parts
/// ([`cut`]), never inside a line, and at the same places whatever `from` and `limit` are, so the
/// parts of a text read with one `block` meet with no gap and no repeat. `at(n)` is where the
/// 0-based line `n` of `text` begins in the measure of `from`, `limit`, `block` and `end`, the
/// text's length in it: a part begins only where a line does.
pub(crate) fn convert_part(
    text: &str,
    at: &dyn Fn(usize) -> usize,
    end: usize,
    from: usize,
    limit: usize,
    block: usize,
) -> Part {
    if from >= end && end > 0 {
        return Part {
            html: String::new(),
            next: end,
            whole: false,
        };
    }
    let options = post_options();
    let arena = Arena::new();
    // The escapes the post's reading adds sit inside its lines, so its lines are `text`'s.
    let root = parse_document(&arena, &post_text(text, &options), &options);
    let size = |lines: Sourcepos| {
        at(lines.end.line).saturating_sub(at(lines.start.line.saturating_sub(1)))
    };
    let mut pieces = Vec::new();
    for node in root.children() {
        cut(node, &[], &size, block, &mut pieces);
    }
    // Where each part may begin, and its first piece: at the text's start, then where a piece
    // begins past the last such place and before the end. Pieces that `at` puts together (on one
    // line, or past the end) stay together.
    let mut starts: Vec<(usize, usize)> = Vec::new();
    for (index, piece) in pieces.iter().enumerate() {
        let offset = at(piece.line.saturating_sub(1));
        match starts.last() {
            None => starts.push((0, index)),
            Some(&(last, _)) if last < offset && offset < end => starts.push((offset, index)),
            Some(_) => {}
        }
    }
    let first = starts
        .partition_point(|&(offset, _)| offset <= from)
        .saturating_sub(1);
    let begin = starts.get(first).map_or(0, |&(offset, _)| offset);
    let finish = |part: usize| starts.get(part + 1).map_or(end, |&(offset, _)| offset);
    let mut last = first;
    while last + 1 < starts.len() && finish(last + 1) - begin <= limit {
        last += 1;
    }
    if first == 0 && last + 1 >= starts.len() {
        return Part {
            html: html(root, &options, HashSet::new()),
            next: end,
            whole: true,
        };
    }
    let after = starts
        .get(last + 1)
        .map_or(pieces.len(), |&(_, piece)| piece);
    let shown = &pieces[starts[first].1..after];
    let taken = shown
        .first()
        .map_or_else(HashSet::new, |piece| ids_before(root, piece.line));
    Part {
        html: html(assemble(&arena, shown), &options, taken),
        next: finish(last),
        whole: false,
    }
}

/// A leaf of a cut text: a block or an inline whole, or some lines of a code block, with
/// the containers it sits in (lists, items, quotes, and a paragraph with its emphasis and links),
/// outermost first.
struct Piece<'a> {
    /// The 1-based line it begins on.
    line: usize,
    within: Vec<Node<'a>>,
    what: What<'a>,
}

enum What<'a> {
    Block(Node<'a>),
    /// Lines of a code block: a range of the bytes of its literal.
    Literal(Node<'a>, Range<usize>),
}

impl<'a> What<'a> {
    fn block(&self) -> Node<'a> {
        match *self {
            What::Block(block) | What::Literal(block, _) => block,
        }
    }
}

/// The pieces of `block`, which sits in `within`, in order: the block whole when its lines take at
/// most `longest` by `size`, else its parts, each cut the same way: a list's items, an item's or a
/// quote's blocks, a code block's lines, and a paragraph's lines ([`inline_pieces`]). Any
/// other block is never cut.
fn cut<'a>(
    block: Node<'a>,
    within: &[Node<'a>],
    size: &dyn Fn(Sourcepos) -> usize,
    longest: usize,
    out: &mut Vec<Piece<'a>>,
) {
    let data = block.data();
    let lines = data.sourcepos;
    let mut piece = |line: usize, what: What<'a>| {
        out.push(Piece {
            line,
            within: within.to_vec(),
            what,
        });
    };
    if size(lines) <= longest {
        piece(lines.start.line, What::Block(block));
        return;
    }
    match &data.value {
        NodeValue::List(_) | NodeValue::Item(_) | NodeValue::BlockQuote
            if block.first_child().is_some() =>
        {
            let inner: Vec<Node<'a>> = within.iter().copied().chain([block]).collect();
            for child in block.children() {
                cut(child, &inner, size, longest, out);
            }
        }
        NodeValue::CodeBlock(code) => {
            let first = lines.start.line + usize::from(code.fenced);
            literal_lines(block, &code.literal, first, lines.end.line, &mut piece);
        }
        NodeValue::Paragraph if block.first_child().is_some() => {
            let inner: Vec<Node<'a>> = within.iter().copied().chain([block]).collect();
            let mut place = Lines {
                line: lines.start.line,
                last: lines.end.line,
                broken: false,
            };
            inline_pieces(block, &inner, &mut place, out);
        }
        _ => piece(lines.start.line, What::Block(block)),
    }
}

/// Where a paragraph's next inline begins: its line, the paragraph's last line, and whether a
/// line break came just before it.
struct Lines {
    line: usize,
    last: usize,
    broken: bool,
}

/// A piece for each inline of `container`, a paragraph or an inline in one, that holds no line
/// break: its text, code, breaks and pictures, each with the inline containers it sits in
/// (emphasis, links, …). A part that begins or ends inside one of those closes and reopens it, so
/// a paragraph is cut at its line breaks however deep in its markup they sit.
fn inline_pieces<'a>(
    container: Node<'a>,
    within: &[Node<'a>],
    lines: &mut Lines,
    out: &mut Vec<Piece<'a>>,
) {
    for inline in container.children() {
        let (start, opens) = {
            let data = inline.data();
            let opens =
                inline.first_child().is_some() && !matches!(data.value, NodeValue::Image(_));
            (data.sourcepos.start.line, opens)
        };
        if lines.broken {
            lines.line = start.max(lines.line + 1).min(lines.last);
            lines.broken = false;
        }
        if opens {
            let inner: Vec<Node<'a>> = within.iter().copied().chain([inline]).collect();
            inline_pieces(inline, &inner, lines, out);
        } else {
            out.push(Piece {
                line: lines.line,
                within: within.to_vec(),
                what: What::Block(inline),
            });
            lines.broken = matches!(
                inline.data().value,
                NodeValue::SoftBreak | NodeValue::LineBreak
            );
        }
    }
}

/// A piece for each line of the `literal` of a code block, whose first line is the text's
/// line `first` and whose last is at most `last`.
fn literal_lines<'a>(
    block: Node<'a>,
    literal: &str,
    first: usize,
    last: usize,
    piece: &mut dyn FnMut(usize, What<'a>),
) {
    let starts = line_starts(literal);
    for (index, &start) in starts.iter().enumerate() {
        let end = starts.get(index + 1).copied().unwrap_or(literal.len());
        if start < end {
            piece((first + index).min(last), What::Literal(block, start..end));
        }
    }
}

/// The ids the whole text's conversion gives the headings that begin before `line`, so the
/// headings of a part beginning there get the ids the whole gives them.
fn ids_before(root: Node<'_>, line: usize) -> HashSet<String> {
    let mut taken = HashSet::new();
    for node in root.descendants() {
        let before = {
            let data = node.data();
            matches!(data.value, NodeValue::Heading(_)) && data.sourcepos.start.line < line
        };
        if before {
            unique(&heading_anchor(&node.collect_text()), &mut taken);
        }
    }
    taken
}

/// A document of `pieces`, in their order. Each block moves out of the parse under copies of its
/// containers, one copy for the pieces in a row that share a container, and the lines of a block
/// cut into pieces are gathered in one copy of it. The parse is not converted whole after this.
fn assemble<'a>(arena: &'a Arena<'a>, pieces: &[Piece<'a>]) -> Node<'a> {
    let document: Node<'a> = arena.alloc(AstNode::from(NodeValue::Document));
    // The containers of the last piece, each beside its copy.
    let mut open: Vec<(Node<'a>, Node<'a>)> = Vec::new();
    // The block the last piece held some lines of, beside its copy, and the literal taken out of it.
    let mut lines: Option<(Node<'a>, Node<'a>)> = None;
    let mut literal = String::new();
    for piece in pieces {
        let kept = open
            .iter()
            .zip(&piece.within)
            .take_while(|((container, _), within)| container.same_node(within))
            .count();
        open.truncate(kept);
        for (depth, &container) in piece.within.iter().enumerate().skip(kept) {
            let held = piece
                .within
                .get(depth + 1)
                .copied()
                .unwrap_or_else(|| piece.what.block());
            let copy: Node<'a> = arena.alloc(container_copy(container, held));
            open.last()
                .map_or(document, |&(_, parent)| parent)
                .append(copy);
            open.push((container, copy));
        }
        let parent = open.last().map_or(document, |&(_, copy)| copy);
        let (block, range) = match &piece.what {
            What::Block(block) => {
                parent.append(block);
                lines = None;
                continue;
            }
            What::Literal(block, range) => (*block, range.clone()),
        };
        let copy = match lines {
            Some((had, copy)) if had.same_node(block) => copy,
            _ => {
                literal = take_literal(block);
                let copy: Node<'a> = arena.alloc(AstNode::from(block.data().value.clone()));
                parent.append(copy);
                lines = Some((block, copy));
                copy
            }
        };
        push_literal(copy, literal.get(range).unwrap_or_default());
    }
    document
}

/// A copy of `container` to hold `held`, one of its children, and those after it: an ordered list
/// numbers from the place of `held` in it.
fn container_copy<'a>(container: Node<'a>, held: Node<'a>) -> AstNode<'a> {
    let mut value = container.data().value.clone();
    if let NodeValue::List(list) = &mut value
        && list.list_type == ListType::Ordered
    {
        list.start += container
            .children()
            .take_while(|child| !child.same_node(held))
            .count();
    }
    AstNode::from(value)
}

/// The literal of a code block, taken out of it.
fn take_literal(block: Node<'_>) -> String {
    match &mut block.data_mut().value {
        NodeValue::CodeBlock(code) => std::mem::take(&mut code.literal),
        _ => String::new(),
    }
}

/// `lines` added to the literal of a code block.
fn push_literal(block: Node<'_>, lines: &str) {
    if let NodeValue::CodeBlock(code) = &mut block.data_mut().value {
        code.literal.push_str(lines);
    }
}

#[cfg(test)]
mod tests {
    use super::{convert_note, convert_post, unheaded};

    #[test]
    fn only_lines_that_would_make_a_heading_are_escaped_and_code_is_left_alone() {
        for (text, escaped) in [
            ("# one\n", "\\# one\n"),
            ("  ### three\n", "  \\### three\n"),
            ("#tag\n", "\\#tag\n"),
            ("Words\n===\n", "Words\n\\===\n"),
            ("Words\n-\n", "Words\n\\-\n"),
            ("# one\n---\n", "\\# one\n\\---\n"),
            // a rule after a blank line, a list item and a sentence with a # are no heading
            ("Words\n\n---\n", "Words\n\n---\n"),
            ("Words\n- item\n", "Words\n- item\n"),
            ("Issue #3\n", "Issue #3\n"),
            (
                "```\n# code\n---\n```\n# after\n",
                "```\n# code\n---\n```\n\\# after\n",
            ),
            // in a list item or a block quote, nested or not, a `#` line is a heading too
            ("- # Intro\n", "- \\# Intro\n"),
            ("> ## Q\n", "> \\## Q\n"),
            ("1. # one\n2) ## two\n", "1. \\# one\n2) \\## two\n"),
            (">> - > * # deep\n", ">> - > * \\# deep\n"),
            ("  +\t### tab\n", "  +\t\\### tab\n"),
            // and so is an underline under a quoted or listed line
            ("> Title\n> ---\n", "> Title\n> \\---\n"),
            ("- Title\n  ===\n", "- Title\n  \\===\n"),
            // a marker with no space after it, a list item of a rule and a number alone are none
            ("-# tag\n", "-# tag\n"),
            (">#tag\n", ">\\#tag\n"),
            ("Words\n- ---\n", "Words\n- ---\n"),
            ("1234567890. # ten digits\n", "1234567890. # ten digits\n"),
            ("3.# x\n", "3.# x\n"),
            ("> \n---\n", "> \n---\n"),
        ] {
            assert_eq!(unheaded(text), escaped, "{text:?}");
        }
    }

    #[test]
    fn a_heading_in_a_notes_list_or_quote_is_drawn_as_its_words() {
        assert_eq!(
            convert_note("- # Intro\n\n> ## Q"),
            "<ul>\n<li># Intro</li>\n</ul>\n<blockquote>\n<p>## Q</p>\n</blockquote>\n"
        );
    }

    #[test]
    fn raw_html_in_a_note_is_its_words_on_the_lines_written() {
        assert_eq!(
            convert_note(
                "Vec<String> and <b>raw</b>\n`<i>`\n\n<div>\nblock\n</div>\n\n> <!-- said -->\n\n<https://example.com>"
            ),
            "<p>Vec&lt;String&gt; and &lt;b&gt;raw&lt;/b&gt;<br />\n<code>&lt;i&gt;</code></p>\n\
             <p>&lt;div&gt;<br />\nblock<br />\n&lt;/div&gt;</p>\n\
             <blockquote>\n<p>&lt;!-- said --&gt;</p>\n</blockquote>\n\
             <p><a href=\"https://example.com\">https://example.com</a></p>\n"
        );
    }

    #[test]
    fn raw_html_in_a_post_is_its_words_and_its_markdown_a_documents() {
        assert_eq!(
            convert_post(
                "#Title\n\nVec<String> and <b>raw</b>\n`<i>`\n\n<div>\nblock **bold**\n</div>\n\n\
                 > <!-- said -->\n\n<TokensPart key=x/>\n\n<https://example.com>"
            ),
            "<h1 id=\"title\">Title</h1>\n\
             <p>Vec&lt;String&gt; and &lt;b&gt;raw&lt;/b&gt;\n<code>&lt;i&gt;</code></p>\n\
             <p>&lt;div&gt;\nblock <strong>bold</strong>\n&lt;/div&gt;</p>\n\
             <blockquote>\n<p>&lt;!-- said --&gt;</p>\n</blockquote>\n\
             <p>&lt;TokensPart key=x/&gt;</p>\n\
             <p><a href=\"https://example.com\">https://example.com</a></p>\n"
        );
    }

    #[test]
    fn a_note_keeps_each_line_break_and_paragraph_and_has_no_heading() {
        assert_eq!(
            convert_note("## title\nnext line\n\nNew paragraph.\nUnderlined\n---"),
            "<p>## title<br />\nnext line</p>\n<p>New paragraph.<br />\nUnderlined<br />\n---</p>\n"
        );
    }
}
