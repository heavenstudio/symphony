//! The docs' markdown as the Symphony reads it: lines, headings and their anchors, fences and code,
//! links, document links, components, and card lines with their entries.
//!
//! No dependencies, so a project's docs build links it from its build script, and every crate that
//! reads the docs shares this one reading. Each function names the rule it keeps; `[^\]]`-style
//! classes are spelled out in prose.

use std::ops::Range;

mod card;
mod fold;
mod wiki;

pub use card::{
    Card, Entry, REGION_END, REGION_START, card_end, card_line, card_state, carded, cards, entries,
    entry_line, is_continuation, is_indented, unseen_owner, unseen_owner_line,
};
use fold::fold;
pub use wiki::{Component, WikiLink, components, wikilinks};

/// Python's `str.isspace()`, which `\s` and `strip()` use: Unicode whitespace plus the ASCII
/// separators `\x1c`..`\x1f`.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\x1c'..='\x1f').contains(&c)
}

/// Python's `\w` on text: a letter, a digit or `_`.
pub fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The lines of `text` as Python's `str.splitlines()` gives them: split on `\n`, `\r\n`, `\r`, `\v`,
/// `\f`, `\x1c`..`\x1e`, `\u{85}`, `\u{2028}` and `\u{2029}`, the breaks dropped; no last empty line.
pub fn lines(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((at, c)) = chars.next() {
        let breaks = matches!(
            c,
            '\n' | '\r'
                | '\x0b'
                | '\x0c'
                | '\x1c'
                | '\x1d'
                | '\x1e'
                | '\u{85}'
                | '\u{2028}'
                | '\u{2029}'
        );
        if !breaks {
            continue;
        }
        out.extend(text.get(start..at));
        start = at + c.len_utf8();
        if c == '\r' && chars.peek().map(|&(_, next)| next) == Some('\n') {
            chars.next();
            start += 1;
        }
    }
    if start < text.len() {
        out.extend(text.get(start..));
    }
    out
}

/// Whether `line` opens or closes a fence: it starts, after any whitespace, with ```` ``` ```` or `~~~`.
/// A fence toggles; the kind of the closing marker is not compared.
pub fn is_fence(line: &str) -> bool {
    let body = line.trim_start_matches(is_space);
    body.starts_with("```") || body.starts_with("~~~")
}

/// Each line with whether it is prose: `None` for a fence line or a line inside a fence.
pub fn outside_fences<'a>(lines: &[&'a str]) -> Vec<Option<&'a str>> {
    let mut fenced = false;
    lines
        .iter()
        .map(|line| {
            if is_fence(line) {
                fenced = !fenced;
                None
            } else if fenced {
                None
            } else {
                Some(*line)
            }
        })
        .collect()
}

/// A heading line, `^(#{1,6})\s+(.+?)\s*#*\s*$`: its level and its title.
pub fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.bytes().take_while(|&b| b == b'#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = line.get(level..)?;
    if !rest.chars().next().is_some_and(is_space) {
        return None;
    }
    let source = rest.trim_start_matches(is_space);
    if source.is_empty() {
        // all whitespace: `\s+` gives back its last character to the title
        let last = rest.chars().next_back()?;
        return Some((level, rest.get(rest.len() - last.len_utf8()..)?));
    }
    let title = source
        .trim_end_matches(is_space)
        .trim_end_matches('#')
        .trim_end_matches(is_space);
    if title.is_empty() {
        let first = source.chars().next()?;
        return Some((level, source.get(..first.len_utf8())?));
    }
    Some((level, title))
}

/// The id a heading's title gets, the rule of markdown's `toc` (`slugify` with `-`): folded to
/// ASCII, everything but letters, digits, `_`, whitespace and `-` dropped, trimmed, lowercased, and
/// each run of `-` and whitespace made one `-`.
pub fn heading_anchor(title: &str) -> String {
    let mut ascii = String::with_capacity(title.len());
    for c in title.chars() {
        if c.is_ascii() {
            ascii.push(c);
        } else {
            ascii.push_str(fold(c));
        }
    }
    let kept: String = ascii
        .chars()
        .filter(|&c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || is_space(c))
        .collect();
    let mut out = String::with_capacity(kept.len());
    let mut dash = false;
    for c in kept.trim_matches(is_space).chars() {
        if c == '-' || is_space(c) {
            dash = true;
            continue;
        }
        if dash {
            out.push('-');
            dash = false;
        }
        out.push(c.to_ascii_lowercase());
    }
    if dash {
        out.push('-');
    }
    out
}

/// The anchor of every heading in `text`; a `#` line inside a fence is a comment, not a heading.
pub fn anchors(text: &str) -> Vec<String> {
    let mut found: Vec<String> = outside_fences(&lines(text))
        .into_iter()
        .flatten()
        .filter_map(heading)
        .map(|(_, title)| heading_anchor(title))
        .collect();
    found.sort();
    found.dedup();
    found
}

/// `line` with each inline code span (`` `[^`]*` ``) masked as `x`s, so a link whose text is code
/// still reads as a link and a link inside code is not one.
pub fn mask_code_spans(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some((before, after)) = rest.split_once('`') {
        let Some((code, tail)) = after.split_once('`') else {
            break;
        };
        out.push_str(before);
        // the span: its two backticks and the code between them
        out.extend(std::iter::repeat_n('x', code.chars().count() + 2));
        rest = tail;
    }
    out.push_str(rest);
    out
}

/// The targets of the inline links and pictures on one line, `!?\[[^\]]*\]\(target(\s+"[^"]*")?\)`,
/// a target being one or more characters that are neither `)` nor whitespace.
pub fn line_links(line: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = line.get(from..).and_then(|rest| rest.find('[')) {
        let open = from + offset;
        match link_at(line, open) {
            Some((target, end)) => {
                found.push(target);
                from = end;
            }
            None => from = open + 1,
        }
    }
    found
}

fn link_at(line: &str, open: usize) -> Option<(&str, usize)> {
    let close = open + 1 + line.get(open + 1..)?.find(']')?;
    let rest = line.get(close + 1..)?.strip_prefix('(')?;
    let length = rest
        .find(|c: char| c == ')' || is_space(c))
        .unwrap_or(rest.len());
    if length == 0 {
        return None;
    }
    let (target, after) = rest.split_at_checked(length)?;
    if let Some(stripped) = after.strip_prefix(')') {
        return Some((target, line.len() - stripped.len()));
    }
    // `\s+"[^"]*"\)`, a title
    let titled = after.trim_start_matches(is_space);
    if titled.len() == after.len() {
        return None;
    }
    let quoted = titled.strip_prefix('"')?;
    let close_quote = quoted.find('"')?;
    let tail = quoted.get(close_quote + 1..)?.strip_prefix(')')?;
    Some((target, line.len() - tail.len()))
}

/// `(line, target)` of every inline link outside fences, code spans masked: what the docs audit
/// followed and checked.
pub fn links(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in outside_fences(&lines(text)).into_iter().enumerate() {
        let Some(line) = line else { continue };
        let masked = mask_code_spans(line);
        found.extend(
            line_links(&masked)
                .into_iter()
                .map(|target| (index + 1, target.to_string())),
        );
    }
    found
}

/// The byte ranges of code the renderer leaves literal, the regex
/// `` ^(```|~~~).*?^\1[^\n]*$|(`+)[^\n]*?\2 `` (multiline, dot matching newlines): a fenced block from a
/// marker at a line's start to the same marker starting a later line, or an inline span between equal
/// runs of backticks on one line.
pub fn code_ranges(text: &str) -> Vec<Range<usize>> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let line_start = at == 0 || bytes[at - 1] == b'\n';
        if line_start && let Some(end) = fenced_block(text, at) {
            found.push(at..end);
            at = end;
            continue;
        }
        if bytes[at] == b'`'
            && let Some(end) = inline_code(bytes, at)
        {
            found.push(at..end);
            at = end;
            continue;
        }
        at += 1;
        while at < bytes.len() && !text.is_char_boundary(at) {
            at += 1;
        }
    }
    found
}

fn fenced_block(text: &str, at: usize) -> Option<usize> {
    let from = text.get(at..)?;
    let marker = ["```", "~~~"].into_iter().find(|m| from.starts_with(m))?;
    let mut line = at;
    loop {
        line += text.get(line..)?.find('\n')? + 1;
        let rest = text.get(line..)?;
        if rest.starts_with(marker) {
            return Some(line + rest.find('\n').unwrap_or(rest.len()));
        }
    }
}

fn inline_code(bytes: &[u8], at: usize) -> Option<usize> {
    let run = bytes[at..].iter().take_while(|&&b| b == b'`').count();
    for ticks in (1..=run).rev() {
        let mut probe = at + ticks;
        while probe < bytes.len() && bytes[probe] != b'\n' {
            if bytes.len() - probe >= ticks
                && bytes[probe..probe + ticks].iter().all(|&b| b == b'`')
            {
                return Some(probe + ticks);
            }
            probe += 1;
        }
    }
    None
}

/// The stretches of `text` outside code (`code_ranges`), with their byte offsets.
pub fn outside_code(text: &str) -> Vec<(usize, &str)> {
    let mut out = Vec::new();
    let mut last = 0;
    for range in code_ranges(text) {
        out.extend(text.get(last..range.start).map(|stretch| (last, stretch)));
        last = range.end;
    }
    out.extend(text.get(last..).map(|stretch| (last, stretch)));
    out
}

/// The 1-based line of byte offset `at` in `text`: how many pieces the newlines before it make.
pub fn line_of(text: &str, at: usize) -> usize {
    text.as_bytes()[..at].split(|&b| b == b'\n').count()
}

/// Where each line of a text starts, so a page with many matches finds each one's line by a search
/// rather than by counting from the top.
pub struct LineIndex(Vec<usize>);

impl LineIndex {
    pub fn new(text: &str) -> LineIndex {
        let starts = text
            .bytes()
            .enumerate()
            .filter(|(_, b)| *b == b'\n')
            .map(|(at, _)| at + 1);
        LineIndex(std::iter::once(0).chain(starts).collect())
    }

    /// The 1-based line of byte offset `at`, as [`line_of`] counts it.
    pub fn line(&self, at: usize) -> usize {
        self.0.partition_point(|&start| start <= at)
    }
}

/// The sources of the pictures in `text`, `!\[[^\]]*\]\(([^)\s]+)\)`.
pub fn pictures(text: &str) -> Vec<&str> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(at) = text.get(from..).and_then(|rest| rest.find("![")) {
        let open = from + at;
        let alt_end = text
            .get(open + 2..)
            .and_then(|rest| rest.find(']'))
            .map(|i| open + 2 + i);
        let Some(rest) = alt_end.and_then(|close| text.get(close + 1..)) else {
            break;
        };
        if let Some(inner) = rest.strip_prefix('(') {
            let length = inner
                .find(|c: char| c == ')' || is_space(c))
                .unwrap_or(inner.len());
            if let Some((source, after)) = inner.split_at_checked(length)
                && length > 0
                && after.starts_with(')')
            {
                found.push(source);
                from = text.len() - after.len() + 1;
                continue;
            }
        }
        from = open + 2;
    }
    found
}
