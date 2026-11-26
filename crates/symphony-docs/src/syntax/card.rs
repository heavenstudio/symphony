//! Card lines and the entries under them: the one reading of a register that the docs build and the
//! Symphony app share.

use std::ops::Range;

use super::{is_fence, is_space};

/// Where a page's register begins and ends: the cards between them are regrouped by each save. A
/// card is read wherever it sits ([`cards`]), in a region or inline at the place its document
/// says what it decides.
pub const REGION_START: &str = "<!-- register:start -->";
pub const REGION_END: &str = "<!-- register:end -->";

/// A card's first line as git's extended pattern (`git grep -E`): the shape [`card_line`] reads, a
/// list item at the left margin, one of its marks and a name in bold. It finds the documents worth
/// reading and decides nothing: [`card_line`] reads each line found.
pub const CARD_LINE: &str = r"^-[[:space:]]+\[[ xX?r~a-]\][[:space:]]+\*\*";

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

/// How a card's text names the group or the person it is for, as `*Replaces:*` names the card it
/// replaces: `*For:* design` (docs/product/analysis/users-and-identity.md, "Which group a thing
/// belongs to"). The name overrides the board's map of places to groups where the board holds it.
pub const FOR: &str = "*For:*";

/// The name a card's `text` is written for ([`FOR`]): the first `*For:* NAME` in it, NAME a short
/// name's letters, digits and hyphens ([`is_short_name`] judges it on a board, or `owner`). None
/// where it names none.
pub fn card_for(text: &str) -> Option<&str> {
    let (_, rest) = text.split_once(FOR)?;
    let name = rest.trim_start_matches(is_space);
    if name.len() == rest.len() {
        return None;
    }
    let length = name
        .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        .unwrap_or(name.len());
    let name = name.get(..length)?.trim_end_matches('-');
    (!name.is_empty()).then_some(name)
}

/// The card line `line` written for `name` ([`FOR`]): its `*For:* NAME` given the new name where
/// it has one, else `*For:* NAME.` put first in its text, after the slug and its dash. None where
/// `line` is no card's line or `name` no short name's form, nor `owner`.
pub fn card_line_for(line: &str, name: &str) -> Option<String> {
    if !(is_short_name(name) || name == "owner") {
        return None;
    }
    let (_, _, text) = card_line(line)?;
    // The card's text is the end of its line.
    let head = line.strip_suffix(text)?;
    if let Some(held) = card_for(text) {
        let (before, rest) = text.split_once(FOR)?;
        let name_at = rest.len() - rest.trim_start_matches(is_space).len();
        let (gap, written) = rest.split_at_checked(name_at)?;
        let after = written.strip_prefix(held)?;
        return Some(format!("{head}{before}{FOR}{gap}{name}{after}"));
    }
    let gap = if head.ends_with(is_space) { "" } else { " " };
    let rest = if text.is_empty() {
        String::new()
    } else {
        format!(" {text}")
    };
    Some(format!("{head}{gap}{FOR} {name}.{rest}"))
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
    /// The lines under the card as written ([`card_end`]): its indented lines, blank lines inside it
    /// kept.
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

/// Whether `line` is indented, `^\s+\S`: it continues the card above it.
pub fn is_indented(line: &str) -> bool {
    let body = line.trim_start_matches(is_space);
    body.len() < line.len() && !body.is_empty()
}

/// Index just past the card whose line is `lines[index]`: its indented lines, across the blank lines
/// inside it. The card's lines are its thread, an indented fence among them included (a fence a list
/// item opens closes with the item, as the `CommonMark` rules have it), so no fence under a card hides the cards
/// after it.
pub fn card_end<L: AsRef<str>>(lines: &[L], index: usize) -> usize {
    let mut end = index + 1;
    let mut probe = end;
    while let Some(line) = lines.get(probe) {
        let line = line.as_ref();
        if is_indented(line) {
            probe += 1;
            end = probe;
        } else if line.trim_matches(is_space).is_empty() {
            probe += 1;
        } else {
            break;
        }
    }
    end
}

/// Every card in `lines` with its lines ([`card_end`]) and the `###` topic above it; a card in a fence
/// the document opens is code.
pub fn cards<L: AsRef<str>>(lines: &[L]) -> Vec<Card> {
    let mut found: Vec<Card> = Vec::new();
    let mut current_topic = "General".to_string();
    let mut fenced = false;
    let mut index = 0;
    while let Some(line) = lines.get(index) {
        let line = line.as_ref();
        if is_fence(line) {
            fenced = !fenced;
        } else if fenced {
            // a document-level example is not a register
        } else if let Some(title) = topic(line) {
            current_topic = title.to_string();
        } else if let Some((mark, slug, text)) = card_line(line) {
            let end = card_end(lines, index);
            found.push(Card {
                line: index + 1,
                slug: slug.trim_matches(is_space).to_string(),
                mark,
                text: text.trim_matches(is_space).to_string(),
                topic: current_topic.clone(),
                raw: lines[index + 1..end]
                    .iter()
                    .map(|line| line.as_ref().to_string())
                    .collect(),
            });
            index = end;
            continue;
        }
        index += 1;
    }
    found
}

/// For each of `lines`, whether it is a card's own ([`cards`]): the card's line or a line under it,
/// wherever the card sits. A card's lines are the page's record, never its prose.
pub fn carded<L: AsRef<str>>(lines: &[L]) -> Vec<bool> {
    let mut own = vec![false; lines.len()];
    for card in cards(lines) {
        let first = card.line - 1;
        for line in own.iter_mut().skip(first).take(1 + card.raw.len()) {
            *line = true;
        }
    }
    own
}

/// The start of the hidden line under a card that pairs an owner entry with an agent reply written
/// without seeing it: `  <!-- review:unseen-owner OWNER REPLY -->`, the two entries' keys.
const UNSEEN_OWNER: &str = "  <!-- review:unseen-owner ";

/// The owner and reply keys a hidden `review:unseen-owner` line holds; it is no entry.
pub fn unseen_owner(line: &str) -> Option<(&str, &str)> {
    line.strip_prefix(UNSEEN_OWNER)?
        .trim_end_matches('\n')
        .strip_suffix(" -->")?
        .split_once(' ')
}

/// The hidden line that pairs the owner entry `owner` with the reply `reply`, its newline included.
pub fn unseen_owner_line(owner: &str, reply: &str) -> String {
    format!("{UNSEEN_OWNER}{owner} {reply} -->\n")
}

/// One event under a card: a person's or an agent's line. `who` is `agent` for an agent's line and
/// `owner` for any person's (docs/product/analysis/users-and-identity.md, "How a person's name is
/// written": a person's line is any line that is not an agent's), whichever short name it carries:
/// [`said`] and [`said_line`] read the name as well.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ts: Option<String>,
    pub who: &'static str,
    pub verdict: Option<String>,
    pub text: String,
}

/// An entry with the name its line carries ([`said`], [`said_line`]): `agent` for an agent's line,
/// `owner` for the person at the machine's and for an undated owner's note, a person's short name
/// ([`is_short_name`]) for theirs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Said {
    pub entry: Entry,
    pub name: String,
}

/// The words never given to a person as a short name: an agent's line, the person at the machine's
/// while a board names nobody, the board's own lines on a thread, and the thread vocabulary's
/// author it cannot read.
pub const RESERVED_NAMES: [&str; 4] = ["agent", "owner", "board", "unknown"];

/// The longest short name.
pub const SHORT_NAME_MAX: usize = 32;

/// Whether `word` has a short name's form: lowercase ASCII letters and digits in parts joined by
/// single hyphens, starting with a letter, at most [`SHORT_NAME_MAX`] long, its last part no number
/// alone (the board numbers a session's handle, `amber-heron-12`, once its plain pairs are taken),
/// and none of [`RESERVED_NAMES`]. It is the form a person's name takes on a line and the form a
/// board gives one in. The form alone never tells a person from a session: a handle may be
/// `amber-heron`, so a thread line is a person's by its row's mark, never by its sender's shape.
pub fn is_short_name(word: &str) -> bool {
    let well_formed = word.len() <= SHORT_NAME_MAX
        && word.starts_with(|c: char| c.is_ascii_lowercase())
        && word.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    let last = word.rsplit('-').next().unwrap_or(word);
    let numbered = word.contains('-') && last.bytes().all(|b| b.is_ascii_digit());
    well_formed && !numbered && !RESERVED_NAMES.contains(&word)
}

/// A dated line, `^\s+\[(date( time)?)\]\s+(name)(?:\s+·\s+(verdict))?(?:\s+—\s*(text))?\s*$`,
/// its name `agent`, `owner` or a person's short name ([`is_short_name`]), as an [`Entry`]
/// ([`said_line`] keeps the name).
pub fn entry_line(line: &str) -> Option<Entry> {
    said_line(line).map(|said| said.entry)
}

/// [`entry_line`], with the name the line carries.
pub fn said_line(line: &str) -> Option<Said> {
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
    let length = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-'))
        .unwrap_or(after.len());
    let (name, rest) = after.split_at(length);
    let who = match name {
        "agent" => "agent",
        "owner" => "owner",
        person if is_short_name(person) => "owner",
        _ => return None,
    };
    let (verdict, rest) = match verdict_part(rest) {
        Some((verdict, tail)) if tail_ends(tail).is_some() => (Some(verdict), tail),
        _ => (None, rest),
    };
    let text = tail_ends(rest)?;
    Some(Said {
        entry: Entry {
            ts: Some(ts.to_string()),
            who,
            verdict: verdict.map(str::to_string),
            text: text.trim_matches(is_space).to_string(),
        },
        name: name.to_string(),
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

/// Whether `line` is deeper-indented than an entry's two spaces, by more than two whitespace
/// characters of any kind: under a dated entry it continues that entry, even when it reads as a dated
/// line itself (REVIEW.md, "Deeper-indented lines continue the entry above").
pub fn is_continuation(line: &str) -> bool {
    let body = line.trim_start_matches(is_space);
    !body.trim_matches(is_space).is_empty() && line.chars().count() - body.chars().count() > 2
}

/// A card's lines as entries: a dated line starts one, a deeper-indented line continues it (a blank
/// line between making a new paragraph; [`is_continuation`]), a `↳` line is an agent's undated
/// answer, a hidden `review:unseen-owner` line ([`unseen_owner`]) is skipped, and anything else is an
/// owner's undated note. [`said`] keeps each one's name.
pub fn entries<L: AsRef<str>>(raw: &[L]) -> Vec<Entry> {
    said(raw).into_iter().map(|said| said.entry).collect()
}

/// [`entries`], each with the name its line carries ([`Said`]).
pub fn said<L: AsRef<str>>(raw: &[L]) -> Vec<Said> {
    let mut found: Vec<Said> = Vec::new();
    let mut paragraph = false;
    for line in raw {
        let line = line.as_ref();
        if unseen_owner(line).is_some() {
            continue;
        }
        let text = line.trim_matches(is_space);
        let dated = |said: &Said| said.entry.ts.is_some();
        let continues = found.last().is_some_and(dated) && is_continuation(line);
        if text.is_empty() {
            paragraph = found.last().is_some_and(dated);
        } else if !continues && let Some(line) = said_line(line) {
            paragraph = false;
            found.push(line);
        } else if continues {
            let last = &mut found.last_mut().expect("a dated entry").entry;
            let joined = format!(
                "{}{}{}",
                last.text,
                if paragraph { "\n\n" } else { "\n" },
                text
            );
            last.text = joined.trim_matches(is_space).to_string();
            paragraph = false;
        } else if let Some(answer) = text.strip_prefix('↳') {
            found.push(undated("agent", answer.trim_matches(is_space)));
        } else {
            found.push(undated("owner", text));
        }
    }
    found
}

/// An undated line of `who`'s, `agent` or `owner`, saying `text`.
fn undated(who: &'static str, text: &str) -> Said {
    Said {
        entry: Entry {
            ts: None,
            who,
            verdict: None,
            text: text.to_string(),
        },
        name: who.to_string(),
    }
}
