//! Card lines and the entries under them: the one reading of a register that the docs build and the
//! Symphony app share.

use std::ops::Range;

use super::{is_fence, is_space};

/// Where a page's register begins and ends: the cards between them are regrouped by each save. A
/// card is read wherever it sits ([`cards`]), in a region or inline at the place its document
/// says what it decides.
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
/// owner's undated note.
pub fn entries<L: AsRef<str>>(raw: &[L]) -> Vec<Entry> {
    let mut found: Vec<Entry> = Vec::new();
    let mut paragraph = false;
    for line in raw {
        let line = line.as_ref();
        if unseen_owner(line).is_some() {
            continue;
        }
        let text = line.trim_matches(is_space);
        let continues = found.last().is_some_and(|e| e.ts.is_some()) && is_continuation(line);
        if text.is_empty() {
            paragraph = found.last().is_some_and(|e| e.ts.is_some());
        } else if !continues && let Some(dated) = entry_line(line) {
            paragraph = false;
            found.push(dated);
        } else if continues {
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
