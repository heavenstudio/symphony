//! The docs' markdown as the Symphony reads it: lines, headings and their anchors, fences and code,
//! links, document links, components, and card lines with their entries.
//!
//! No dependencies, so a project's docs build links it from its build script, and every crate that
//! reads the docs shares this one reading. Each function names the rule it keeps; `[^\]]`-style
//! classes are spelled out in prose.

use std::ops::Range;

mod fold;

use fold::fold;

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

/// A document link, `[[target#heading|label]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WikiLink {
    pub line: usize,
    pub whole: String,
    pub target: String,
    pub heading: Option<String>,
    pub label: Option<String>,
}

/// Every `[[target#heading|label]]` outside code, `\[\[([^\]|#]+)(?:#([^\]|]+))?(?:\|([^\]]+))?\]\]`.
/// A target holding `:` is a component or a task, not a document; it is kept, and the caller decides.
pub fn wikilinks(text: &str) -> Vec<WikiLink> {
    let starts = LineIndex::new(text);
    let mut found = Vec::new();
    for (offset, part) in outside_code(text) {
        let mut from = 0;
        while let Some(at) = part.get(from..).and_then(|rest| rest.find("[[")) {
            let open = from + at;
            match part.get(open..).and_then(wikilink_at) {
                Some((whole, target, heading, label)) => {
                    found.push(WikiLink {
                        line: starts.line(offset + open),
                        whole: whole.to_string(),
                        target: target.to_string(),
                        heading: heading.map(str::to_string),
                        label: label.map(str::to_string),
                    });
                    from = open + whole.len();
                }
                None => from = open + 1,
            }
        }
    }
    found
}

/// A wiki link's whole text, its target, heading and label.
type WikiParts<'a> = (&'a str, &'a str, Option<&'a str>, Option<&'a str>);

/// The wiki link `text` starts with (at its `[[`), if it is one.
fn wikilink_at(text: &str) -> Option<WikiParts<'_>> {
    let body = text.get(2..)?;
    let target_len = body.find([']', '|', '#']).unwrap_or(body.len());
    if target_len == 0 {
        return None;
    }
    let (target, mut tail) = body.split_at_checked(target_len)?;
    let mut heading = None;
    if let Some(rest) = tail.strip_prefix('#') {
        let length = rest.find([']', '|']).unwrap_or(rest.len());
        if length > 0 {
            let (named, after) = rest.split_at_checked(length)?;
            heading = Some(named);
            tail = after;
        }
    }
    let mut label = None;
    if let Some(rest) = tail.strip_prefix('|') {
        let length = rest.find(']').unwrap_or(rest.len());
        if length > 0 {
            let (named, after) = rest.split_at_checked(length)?;
            label = Some(named);
            tail = after;
        }
    }
    if !tail.starts_with("]]") {
        return None;
    }
    let whole = text.get(..text.len() - tail.len() + 2)?;
    Some((whole, target, heading, label))
}

/// A component, `[[name:argument|option]]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Component {
    pub line: usize,
    pub whole: String,
    pub name: String,
    pub arg: String,
    pub option: Option<String>,
}

/// Every `[[name:argument|option]]` outside code,
/// `\[\[([a-z][a-z-]*):([^\]|]*)(?:\|([^\]]*))?\]\]`.
pub fn components(text: &str) -> Vec<Component> {
    let starts = LineIndex::new(text);
    let mut found = Vec::new();
    for (offset, part) in outside_code(text) {
        let mut from = 0;
        while let Some(at) = part.get(from..).and_then(|rest| rest.find("[[")) {
            let open = from + at;
            match part.get(open..).and_then(component_at) {
                Some((whole, name, arg, option)) => {
                    found.push(Component {
                        line: starts.line(offset + open),
                        whole: whole.to_string(),
                        name: name.to_string(),
                        arg: arg.to_string(),
                        option: option.map(str::to_string),
                    });
                    from = open + whole.len();
                }
                None => from = open + 1,
            }
        }
    }
    found
}

/// A component's whole text, its name, argument and option.
type ComponentParts<'a> = (&'a str, &'a str, &'a str, Option<&'a str>);

/// The component `text` starts with (at its `[[`), if it is one.
fn component_at(text: &str) -> Option<ComponentParts<'_>> {
    let body = text.get(2..)?;
    if !body.starts_with(|c: char| c.is_ascii_lowercase()) {
        return None;
    }
    let name_len = body
        .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .unwrap_or(body.len());
    let (name, rest) = body.split_at_checked(name_len)?;
    let rest = rest.strip_prefix(':')?;
    let arg_len = rest.find([']', '|']).unwrap_or(rest.len());
    let (arg, mut tail) = rest.split_at_checked(arg_len)?;
    let mut option = None;
    if let Some(rest) = tail.strip_prefix('|') {
        let length = rest.find(']').unwrap_or(rest.len());
        let (named, after) = rest.split_at_checked(length)?;
        option = Some(named);
        tail = after;
    }
    if !tail.starts_with("]]") {
        return None;
    }
    let whole = text.get(..text.len() - tail.len() + 2)?;
    Some((whole, name, arg, option))
}

/// Where a page's register begins and ends; a card outside it is prose.
pub const REGION_START: &str = "<!-- register:start -->";
pub const REGION_END: &str = "<!-- register:end -->";

/// A card line, `^-\s+\[(mark)\]\s+\*\*(slug)\*\*\s*(?:[—–-]\s*)?(text)$`: its mark, slug and text as
/// written (the caller strips them).
pub fn card_line(line: &str) -> Option<(char, &str, &str)> {
    let rest = line.strip_prefix('-')?;
    let after = rest.trim_start_matches(is_space);
    if after.len() == rest.len() {
        return None;
    }
    let mut chars = after.chars();
    if chars.next()? != '[' {
        return None;
    }
    let mark = chars.next()?;
    if !" xX?r~a-".contains(mark) || chars.next()? != ']' {
        return None;
    }
    let rest = after.get(2 + mark.len_utf8()..)?;
    let after = rest.trim_start_matches(is_space);
    if after.len() == rest.len() {
        return None;
    }
    let body = after.strip_prefix("**")?;
    let (slug, tail) = body.split_at_checked(body.find('*')?)?;
    let tail = tail.strip_prefix("**").filter(|_| !slug.is_empty())?;
    let mut text = tail.trim_start_matches(is_space);
    if let Some(dashed) = text.strip_prefix(['—', '–', '-']) {
        text = dashed.trim_start_matches(is_space);
    }
    Some((mark, slug, text))
}

/// The state a card's mark stands for.
pub fn card_state(mark: char) -> &'static str {
    match mark {
        'x' | 'X' => "approved",
        '-' => "rejected",
        '?' => "question",
        'r' => "resolved",
        '~' => "dismissed",
        'a' => "auto-approved",
        _ => "pending",
    }
}

/// A card with the lines under it, as the register reads them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Card {
    pub line: usize,
    pub slug: String,
    pub mark: char,
    pub text: String,
    pub topic: String,
    /// The indented lines under the card as written, blank lines inside a post kept.
    pub raw: Vec<String>,
}

impl Card {
    pub fn state(&self) -> &'static str {
        card_state(self.mark)
    }

    /// The card's dated lines and the lines they continue, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        entries(&self.raw)
    }
}

fn topic(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("###")?;
    if !rest.chars().next().is_some_and(is_space) {
        return None;
    }
    let title = rest.trim_start_matches(is_space).trim_end_matches(is_space);
    if title.is_empty() {
        let last = rest.chars().next_back()?;
        return rest.get(rest.len() - last.len_utf8()..);
    }
    Some(title)
}

fn indented(line: &str) -> bool {
    let body = line.trim_start_matches(is_space);
    body.len() < line.len() && !body.is_empty()
}

/// Every card in `lines` with its indented lines and the `###` topic above it; cards in a fence are code.
pub fn cards(lines: &[&str]) -> Vec<Card> {
    let mut found: Vec<Card> = Vec::new();
    let mut open = false;
    let mut gap: Vec<String> = Vec::new();
    let mut current_topic = "General".to_string();
    let mut fenced = false;
    for (index, line) in lines.iter().enumerate() {
        if is_fence(line) {
            fenced = !fenced;
            open = false;
            gap.clear();
            continue;
        }
        if fenced {
            continue;
        }
        if let Some(title) = topic(line) {
            current_topic = title.to_string();
            open = false;
            gap.clear();
            continue;
        }
        if let Some((mark, slug, text)) = card_line(line) {
            found.push(Card {
                line: index + 1,
                slug: slug.trim_matches(is_space).to_string(),
                mark,
                text: text.trim_matches(is_space).to_string(),
                topic: current_topic.clone(),
                raw: Vec::new(),
            });
            open = true;
        } else if open && line.trim_matches(is_space).is_empty() {
            gap.push(line.to_string());
        } else if open && indented(line) {
            let card = found.last_mut().expect("an open card");
            card.raw.append(&mut gap);
            card.raw.push(line.to_string());
        } else {
            open = false;
        }
        if !open {
            gap.clear();
        }
    }
    found
}

/// One event under a card: an owner's or an agent's line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ts: Option<String>,
    pub who: &'static str,
    pub verdict: Option<String>,
    pub text: String,
}

/// A dated line, `^\s+\[(date( time)?)\]\s+(owner|agent)(?:\s+·\s+(verdict))?(?:\s+—\s*(text))?\s*$`.
pub fn entry_line(line: &str) -> Option<Entry> {
    let rest = line.trim_start_matches(is_space);
    if rest.len() == line.len() {
        return None;
    }
    let rest = rest.strip_prefix('[')?;
    let (ts, rest) = rest.split_at_checked(rest.find(']')?)?;
    if !is_stamp(ts) {
        return None;
    }
    let rest = rest.strip_prefix(']')?;
    let after = rest.trim_start_matches(is_space);
    if after.len() == rest.len() {
        return None;
    }
    let (who, rest) = if let Some(r) = after.strip_prefix("owner") {
        ("owner", r)
    } else {
        ("agent", after.strip_prefix("agent")?)
    };
    let (verdict, rest) = match verdict_part(rest) {
        Some((verdict, tail)) if tail_ends(tail).is_some() => (Some(verdict), tail),
        _ => (None, rest),
    };
    let text = tail_ends(rest)?;
    Some(Entry {
        ts: Some(ts.to_string()),
        who,
        verdict: verdict.map(str::to_string),
        text: text.trim_matches(is_space).to_string(),
    })
}

fn is_stamp(ts: &str) -> bool {
    let b = ts.as_bytes();
    let digits = |range: Range<usize>| b[range].iter().all(u8::is_ascii_digit);
    match b.len() {
        10 => digits(0..4) && b[4] == b'-' && digits(5..7) && b[7] == b'-' && digits(8..10),
        16 => {
            ts.get(..10).is_some_and(is_stamp)
                && b[10] == b' '
                && digits(11..13)
                && b[13] == b':'
                && digits(14..16)
        }
        _ => false,
    }
}

fn verdict_part(rest: &str) -> Option<(&str, &str)> {
    let after = rest.trim_start_matches(is_space);
    if after.len() == rest.len() {
        return None;
    }
    let after = after.strip_prefix('·')?;
    let word = after.trim_start_matches(is_space);
    if word.len() == after.len() {
        return None;
    }
    let length = word
        .find(|c: char| !(c.is_ascii_lowercase() || c == '-'))
        .unwrap_or(word.len());
    let (verdict, tail) = word.split_at_checked(length)?;
    let well_formed = !verdict.is_empty()
        && !verdict.starts_with('-')
        && !verdict.ends_with('-')
        && !verdict.contains("--");
    well_formed.then_some((verdict, tail))
}

/// What follows the author and verdict: `(?:\s+—\s*(text))?\s*$`, the text ("" when there is none).
fn tail_ends(rest: &str) -> Option<&str> {
    if rest.trim_matches(is_space).is_empty() {
        return Some("");
    }
    let after = rest.trim_start_matches(is_space);
    if after.len() == rest.len() {
        return None;
    }
    Some(after.strip_prefix('—')?.trim_start_matches(is_space))
}

/// A card's lines as entries: a dated line starts one, a deeper-indented line continues it (a blank
/// line between making a new paragraph), a `↳` line is an agent's undated answer, and anything else an
/// owner's undated note.
pub fn entries(raw: &[String]) -> Vec<Entry> {
    let mut found: Vec<Entry> = Vec::new();
    let mut paragraph = false;
    for line in raw {
        let text = line.trim_matches(is_space);
        if text.is_empty() {
            paragraph = found.last().is_some_and(|e| e.ts.is_some());
        } else if let Some(dated) = entry_line(line) {
            paragraph = false;
            found.push(dated);
        } else if found.last().is_some_and(|e| e.ts.is_some())
            && line.chars().count() - line.trim_start_matches(is_space).chars().count() > 2
        {
            let last = found.last_mut().expect("a dated entry");
            let joined = format!(
                "{}{}{}",
                last.text,
                if paragraph { "\n\n" } else { "\n" },
                text
            );
            last.text = joined.trim_matches(is_space).to_string();
            paragraph = false;
        } else if let Some(answer) = text.strip_prefix('↳') {
            found.push(Entry {
                ts: None,
                who: "agent",
                verdict: None,
                text: answer.trim_matches(is_space).to_string(),
            });
        } else {
            found.push(Entry {
                ts: None,
                who: "owner",
                verdict: None,
                text: text.to_string(),
            });
        }
    }
    found
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
