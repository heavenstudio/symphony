//! Decision cards: a task item with a bold slug, its verdict in the mark, its history under it.
//!
//! ```text
//! - [-] **lamp-count-141** — 141 lamps.
//!   [2026-09-24 14:02] owner · rejected — needs better names
//!   [2026-09-24 15:10] agent — Options are in **street-names**.
//! ```
//!
//! One dated line per event under a card, appended and never rewritten; deeper-indented lines
//! continue the entry above them. Ordinary task lists have no bold slug and are not read.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use sha2::{Digest, Sha256};
use symphony_docs::syntax;
pub use symphony_docs::syntax::{REGION_END, REGION_START, card_end, carded};

use crate::text::{NS, S, prefix_chars, repr, splitlines, strip};

/// Comment kinds the owner writes. A `like` is a record, not work, so it is never open; the others
/// stay open until an agent nests a `[!resolved]` under them.
pub const KINDS: [&str; 5] = ["comment", "like", "dislike", "question", "decision"];
pub const RESOLVED: &str = "resolved";
/// An agent line `[ts] agent · question — …` asks the owner something: the card waits for the owner.
pub const QUESTION: &str = "question";

fn pattern(source: &str) -> Regex {
    Regex::new(&source.replace("{S}", S).replace("{NS}", NS)).expect("a register pattern")
}

pub(crate) static CALLOUT: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"^{S}*>{S}*\[!(?P<kind>[A-Za-z]+)\](?P<title>.*)$"));
pub(crate) static QUOTE: LazyLock<Regex> = LazyLock::new(|| pattern(r"^{S}*>{S}?(?P<rest>.*)$"));
pub(crate) static NESTED_RESOLVED: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"(?i)^{S}*>{S}*\[!resolved\]{S}*(?P<text>.*)$"));
static TOPIC: LazyLock<Regex> = LazyLock::new(|| pattern(r"^###{S}+(?P<topic>.+?){S}*$"));
static GROUP_HEADING: LazyLock<Regex> = LazyLock::new(|| pattern(r"^##{S}+(?P<name>.+?){S}*$"));
pub(crate) static REPLACES: LazyLock<Regex> =
    LazyLock::new(|| pattern(r"\*Replaces:\* ([a-z0-9][a-z0-9-]*)"));

/// A card's own line, read: its slug as written (stripped) and where its mark sits.
pub(crate) struct CardLine<'a> {
    pub slug: &'a str,
    pub mark: Range<usize>,
}

/// The card a line opens, if it opens one, as the docs build reads it (`syntax::card_line`).
pub(crate) fn card_line(line: &str) -> Option<CardLine<'_>> {
    let (mark, slug, _) = syntax::card_line(line)?;
    // `-`, whitespace, then `[`: the line's first bracket holds the mark
    let at = line.find('[')? + 1;
    Some(CardLine {
        slug: strip(slug),
        mark: at..at + mark.len_utf8(),
    })
}

/// `line` with its card mark set to `mark`.
pub(crate) fn remark(line: &str, mark: char) -> String {
    match card_line(line) {
        Some(card) => format!(
            "{}{mark}{}",
            &line[..card.mark.start],
            &line[card.mark.end..]
        ),
        None => line.to_string(),
    }
}

pub(crate) use symphony_docs::syntax::{is_fence, is_indented};

/// The register could not be read or written as asked; nothing was written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewError(pub String);

impl fmt::Display for ReviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ReviewError {}

/// A card's verdict, which its mark carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum State {
    Pending,
    AutoApproved,
    Approved,
    Rejected,
    Question,
    Resolved,
    Dismissed,
}

impl State {
    /// The order the board counts them in.
    pub const ORDER: [State; 7] = [
        State::Pending,
        State::AutoApproved,
        State::Approved,
        State::Rejected,
        State::Question,
        State::Resolved,
        State::Dismissed,
    ];

    /// The state a mark sets: ` ` `x` `X` `-` `?` `r` `~` `a`.
    pub fn from_mark(mark: char) -> Option<State> {
        Some(match mark {
            ' ' => State::Pending,
            'x' | 'X' => State::Approved,
            '-' => State::Rejected,
            '?' => State::Question,
            'r' => State::Resolved,
            '~' => State::Dismissed,
            'a' => State::AutoApproved,
            _ => return None,
        })
    }

    /// The mark a write sets for this state (`x` for an approval).
    pub fn mark(self) -> char {
        match self {
            State::Pending => ' ',
            State::Approved => 'x',
            State::Rejected => '-',
            State::Question => '?',
            State::Resolved => 'r',
            State::Dismissed => '~',
            State::AutoApproved => 'a',
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::AutoApproved => "auto-approved",
            State::Approved => "approved",
            State::Rejected => "rejected",
            State::Question => "question",
            State::Resolved => "resolved",
            State::Dismissed => "dismissed",
        }
    }

    /// The state a word names (`pending`, `auto-approved`, …).
    pub fn parse(word: &str) -> Option<State> {
        State::ORDER
            .into_iter()
            .find(|state| state.as_str() == word)
    }
}

/// Where a card sits in the register, in the order the owner reads them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Group {
    Decide,
    Auto,
    Confirm,
    Agent,
    Rejected,
    Resolved,
}

impl Group {
    pub const ALL: [Group; 6] = [
        Group::Decide,
        Group::Auto,
        Group::Confirm,
        Group::Agent,
        Group::Rejected,
        Group::Resolved,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Group::Decide => "decide",
            Group::Auto => "auto",
            Group::Confirm => "confirm",
            Group::Agent => "agent",
            Group::Rejected => "rejected",
            Group::Resolved => "resolved",
        }
    }

    /// The register heading the group's cards sit under.
    pub fn title(self) -> &'static str {
        match self {
            Group::Decide => "Needs your decision",
            // the triage tool approved it; the owner can overturn it
            Group::Auto => "Auto-approved: spot-check",
            Group::Confirm => "Answered: approve again or comment",
            Group::Agent => "Waiting on an agent",
            Group::Rejected => "Rejected and replaced",
            Group::Resolved => "Resolved",
        }
    }
}

/// Who said a line under a card: an agent, or a person (docs/product/analysis/users-and-identity.md,
/// "How a person's name is written": a person's line is any line that is not an agent's).
/// `Owner` is every person's line, the person at the machine's (`owner ·`) and a person's a board
/// named (`ana ·`) alike; [`Entry::name`] says whose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Who {
    Owner,
    Agent,
}

impl Who {
    pub fn as_str(self) -> &'static str {
        match self {
            Who::Owner => "owner",
            Who::Agent => "agent",
        }
    }
}

/// One line said under a card: dated, `[2026-09-24 14:02] owner · rejected — needs better names`,
/// or undated from before entries were dated.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub ts: Option<String>,
    pub who: Who,
    pub verdict: Option<String>,
    pub text: String,
    /// The name the line carries: `agent`, `owner`, or a person's short name.
    pub name: String,
}

impl Entry {
    /// The entry `said` reads: an agent's line by `agent`, any other a person's.
    pub(crate) fn of(said: syntax::Said) -> Entry {
        Entry {
            ts: said.entry.ts,
            who: Who::of(said.entry.who),
            verdict: said.entry.verdict,
            text: said.entry.text,
            name: said.name,
        }
    }
}

impl Who {
    /// The author a line's name word (`agent`, `owner`, a person's short name) reads as: an
    /// agent's for `agent`, a person's for any other.
    pub(crate) fn of(name: &str) -> Who {
        if name == Who::Agent.as_str() {
            Who::Agent
        } else {
            Who::Owner
        }
    }
}

/// Hidden card metadata pairs an owner entry with a reply written without seeing it.
/// Occurrence suffixes distinguish repeated identical posts without changing their words.
pub(crate) fn entry_keys(entries: &[Entry]) -> Vec<String> {
    let mut counts = HashMap::<String, usize>::new();
    entries
        .iter()
        .map(|entry| {
            // By the name, which is `owner` or `agent` for every line written before names, so a
            // key kept on a page reads as it was made.
            let data = serde_json::to_vec(&(&entry.ts, &entry.name, &entry.verdict, &entry.text))
                .expect("entry fields serialize");
            let hash = Sha256::digest(data)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            let count = counts.entry(hash.clone()).or_default();
            *count += 1;
            format!("{hash}:{count}")
        })
        .collect()
}

pub(crate) use symphony_docs::syntax::{
    unseen_owner as unseen_reply, unseen_owner_line as unseen_marker,
};

/// A decision card: `- [x] **five-workshops** — There are five workshops.`
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    /// 1-based, in the lines it was parsed from.
    pub line: usize,
    pub slug: String,
    pub state: State,
    pub text: String,
    /// The indented lines under the card, stripped.
    pub note: Vec<String>,
    /// The `###` heading above it, `General` when none.
    pub topic: String,
    /// The lines under the card as written, blank lines inside it included.
    pub raw: Vec<String>,
}

impl Decision {
    /// What was said under the card, oldest first.
    pub fn entries(&self) -> Vec<Entry> {
        let noted: Vec<String>;
        let lines = if self.raw.is_empty() {
            noted = self.note.iter().map(|n| format!("  {n}")).collect();
            &noted
        } else {
            &self.raw
        };
        syntax::said(lines).into_iter().map(Entry::of).collect()
    }

    /// Everything the card's thread says, from one read of its entries. The project's card kinds
    /// say whether an approval waits for an agent to apply it.
    pub fn thread(&self, plugins: &crate::plugins::Plugins) -> Thread {
        self.read(self.state == State::Approved && plugins.approval_needs_agent(self))
    }

    /// The thread, an approval waiting for an agent when `needs_agent`.
    fn read(&self, needs_agent: bool) -> Thread {
        let entries = self.entries();
        let last = entries.last();
        let reply = last
            .filter(|last| last.who == Who::Agent && last.verdict.is_none())
            .map(|last| last.text.clone());
        let posted_at = entries.iter().find_map(|entry| entry.ts.clone());
        let decided_at = entries
            .iter()
            .rev()
            .find(|entry| {
                entry.who == Who::Owner && entry.verdict.as_deref().is_some_and(|v| !v.is_empty())
            })
            .and_then(|entry| entry.ts.clone());
        // Owner comments with no agent reply after them, oldest first.
        let unseen: std::collections::HashSet<_> = self
            .raw
            .iter()
            .filter_map(|line| unseen_reply(line))
            .collect();
        let keys = if unseen.is_empty() {
            vec![String::new(); entries.len()]
        } else {
            entry_keys(&entries)
        };
        let mut pending: Vec<(&str, &Entry)> = Vec::new();
        for (entry, key) in entries.iter().zip(&keys) {
            if entry.who == Who::Agent && entry.verdict.as_deref().is_none_or(|v| v == QUESTION) {
                pending.retain(|(owner, _)| unseen.contains(&(*owner, key.as_str())));
            } else if entry.who == Who::Owner
                && (!entry.text.is_empty()
                    || unseen.iter().any(|(owner, _)| *owner == key.as_str()))
            {
                pending.push((key, entry));
            }
        }
        let unanswered: Vec<_> = pending
            .iter()
            .filter(|(_, entry)| !entry.text.is_empty())
            .map(|(_, entry)| entry.text.clone())
            .collect();
        let settled = matches!(self.state, State::Resolved | State::Dismissed);
        let needs_blessing = self.state == State::Approved
            && needs_agent
            && last.is_some_and(|entry| {
                entry.who == Who::Owner && entry.verdict.as_deref() == Some("approved")
            });
        // Agent work: unanswered words, a rejection/question, or an unapplied golden approval.
        let asks = matches!(self.state, State::Rejected | State::Question);
        let open = needs_blessing
            || !pending.is_empty()
            || (!settled
                && match last {
                    None => asks,
                    Some(last) => last.who == Who::Owner && asks,
                });
        // The last thing under the card is an agent's question to the owner.
        let asks_owner = last.is_some_and(|last| {
            last.who == Who::Agent && last.verdict.as_deref() == Some(QUESTION)
        });
        // An approval with nothing left to answer: plain, given again, or its note answered.
        let resolvable = self.state == State::Approved
            && !needs_blessing
            && pending.is_empty()
            && match last {
                None => true,
                Some(last) if last.who == Who::Agent => last.verdict.is_none(),
                Some(last) => last.verdict.as_deref() == Some("approved") && last.text.is_empty(),
            };
        let group = if asks_owner && !settled {
            Group::Decide // an agent is waiting on the owner's answer, whatever the card's mark
        } else if self.state == State::Dismissed {
            // the owner said the card is not needed
            if open { Group::Agent } else { Group::Resolved }
        } else if self.state == State::AutoApproved {
            // `[a]`, set only by the triage tool
            if open { Group::Agent } else { Group::Auto }
        } else if self.state == State::Resolved || resolvable {
            Group::Resolved
        } else if open {
            Group::Agent
        } else if self.state == State::Rejected {
            Group::Rejected
        } else if self.state == State::Pending {
            Group::Decide
        } else {
            Group::Confirm // approved or question, and an agent answered the owner's comment
        };
        Thread {
            entries,
            reply,
            posted_at,
            decided_at,
            unanswered,
            open,
            asks_owner,
            resolvable,
            group,
        }
    }

    /// The agent's answer, when it is the last thing said under the card.
    pub fn reply(&self) -> Option<String> {
        self.read(false).reply
    }

    /// When the card went up: its first dated line.
    pub fn posted_at(&self) -> Option<String> {
        self.read(false).posted_at
    }

    /// When the owner last gave a verdict on it.
    pub fn decided_at(&self) -> Option<String> {
        self.read(false).decided_at
    }

    /// Owner comments with no agent reply after them, oldest first.
    pub fn unanswered(&self) -> Vec<String> {
        self.read(false).unanswered
    }

    /// Agent work: unanswered words, a rejection/question, or an approval the project's card kinds
    /// say waits for an agent.
    pub fn open(&self, plugins: &crate::plugins::Plugins) -> bool {
        self.thread(plugins).open
    }

    /// The last thing under the card is an agent's question to the owner.
    pub fn asks_owner(&self) -> bool {
        self.read(false).asks_owner
    }

    /// An approval with nothing left to answer: plain, given again, or its note answered.
    pub fn resolvable(&self, plugins: &crate::plugins::Plugins) -> bool {
        self.thread(plugins).resolvable
    }

    /// Where the card sits in its register.
    pub fn group(&self, plugins: &crate::plugins::Plugins) -> Group {
        self.thread(plugins).group
    }
}

/// What a card's thread says, worked out in one pass ([`Decision::thread`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub entries: Vec<Entry>,
    pub reply: Option<String>,
    pub posted_at: Option<String>,
    pub decided_at: Option<String>,
    pub unanswered: Vec<String>,
    pub open: bool,
    pub asks_owner: bool,
    pub resolvable: bool,
    pub group: Group,
}

/// Every decision card in `lines`, with its history and the `###` topic above it.
/// The cards are the docs build's (`syntax::cards`): each reaches exactly the boundary edits and the
/// loss guard use ([`card_end`]), so an indented fence belongs to the thread, even an unclosed one,
/// and cannot hide later document-level cards.
pub fn parse_decisions<L: AsRef<str>>(lines: &[L]) -> Vec<Decision> {
    syntax::cards(lines)
        .into_iter()
        .filter_map(|card| {
            let note = card
                .raw
                .iter()
                .filter(|line| unseen_reply(line).is_none())
                .map(|line| strip(line))
                .filter(|line| !line.is_empty())
                .map(str::to_string)
                .collect();
            Some(Decision {
                line: card.line,
                state: State::from_mark(card.mark)?,
                slug: card.slug,
                text: card.text,
                note,
                topic: card.topic,
                raw: card.raw,
            })
        })
        .collect()
}

/// The register region's bounds: the indexes of its start and end markers.
fn region<L: AsRef<str>>(lines: &[L]) -> Option<(usize, usize)> {
    let start = lines
        .iter()
        .position(|line| line.as_ref() == REGION_START)?;
    let end = start
        + lines[start..]
            .iter()
            .position(|line| line.as_ref() == REGION_END)?;
    Some((start, end))
}

/// Every register region's bounds in `lines`, the indexes of its start and end markers, paired as
/// [`sort_register`] pairs them: each start marker with the first end marker after it, the next
/// region looked for after that end. A start marker no end marker follows opens none.
pub fn regions<L: AsRef<str>>(lines: &[L]) -> Vec<(usize, usize)> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some((start, end)) = region(&lines[from..]) {
        found.push((from + start, from + end));
        from += end + 1;
    }
    found
}

/// Rewrite the register region grouped owner-first, topics and histories kept, each card grouped as
/// the project's card kinds read it.
pub fn sort_register(text: &str, plugins: &crate::plugins::Plugins) -> Result<String, ReviewError> {
    let lines = splitlines(text);
    let Some((start, end)) = region(&lines) else {
        return Ok(text.to_string());
    };
    if lines[end + 1..].contains(&REGION_START) {
        return Ok(sort_register(&(lines[..=end].join("\n") + "\n"), plugins)?
            + &sort_register(&(lines[end + 1..].join("\n") + "\n"), plugins)?);
    }
    let body = &lines[start + 1..end];
    let cards = parse_decisions(body);
    let mut duplicates: Vec<&str> = slug_counts(&cards)
        .into_iter()
        .filter(|(_, n)| *n > 1)
        .map(|(slug, _)| slug)
        .collect();
    if !duplicates.is_empty() {
        symphony_board::order::sort_by(&mut duplicates, &Ord::cmp);
        return Err(ReviewError(format!(
            "duplicate decision slug(s): {}",
            duplicates.join(", ")
        )));
    }
    for line in body {
        let known = strip(line).is_empty()
            || TOPIC.is_match(line)
            || GROUP_HEADING.is_match(line)
            || card_line(line).is_some()
            || is_indented(line);
        if !known {
            return Err(ReviewError(format!(
                "the register region holds text that is not a card and would be lost: {}",
                repr(line)
            )));
        }
    }
    let blocks: Vec<&[&str]> = cards
        .iter()
        .map(|card| {
            let first = card.line - 1;
            &body[first..(first + 1 + card.raw.len()).min(body.len())]
        })
        .collect();
    let groups: Vec<Group> = cards.iter().map(|card| card.group(plugins)).collect();
    let mut out: Vec<String> = Vec::new();
    for group in Group::ALL {
        let mut members: Vec<usize> = (0..cards.len()).filter(|&i| groups[i] == group).collect();
        if members.is_empty() {
            continue;
        }
        if group == Group::Decide {
            // the owner reads what is waiting newest first; undated cards last, in place
            let posted: Vec<String> = cards
                .iter()
                .map(|card| card.posted_at().unwrap_or_default())
                .collect();
            symphony_board::order::sort_by(&mut members, &|a, b| posted[*b].cmp(&posted[*a]));
        }
        out.push(format!("## {}", group.title())); // no count: the page and the board count live
        out.push(String::new());
        let mut topics: Vec<&str> = Vec::new();
        for &i in &members {
            if !topics.contains(&cards[i].topic.as_str()) {
                topics.push(&cards[i].topic);
            }
        }
        for topic in topics {
            out.push(format!("### {topic}"));
            out.push(String::new());
            for &i in members.iter().filter(|&&i| cards[i].topic == topic) {
                out.extend(blocks[i].iter().map(|line| line.to_string()));
            }
            out.push(String::new());
        }
    }
    while out.last().is_some_and(String::is_empty) {
        out.pop();
    }
    refuse_repeats(&cards, &parse_decisions(&out), "regrouping")?;
    let mut result: Vec<&str> = lines[..=start].to_vec();
    result.extend(out.iter().map(String::as_str));
    result.extend_from_slice(&lines[end..]);
    Ok(result.join("\n") + "\n")
}

/// Each card's own lines by slug, in the order first seen, its mark blanked so a verdict's change
/// of mark is not a loss.
fn card_lines(text: &str) -> Vec<(String, Vec<String>)> {
    let lines = splitlines(text);
    let mut found: Vec<(String, Vec<String>)> = Vec::new();
    for (index, line) in lines.iter().enumerate() {
        let Some(card) = card_line(line) else {
            continue;
        };
        let head = format!("{}?{}", &line[..card.mark.start], &line[card.mark.end..]);
        let own = std::iter::once(head).chain(
            lines[index + 1..card_end(&lines, index)]
                .iter()
                .filter(|n| !strip(n).is_empty())
                .map(|n| n.to_string()),
        );
        match found.iter_mut().find(|(slug, _)| slug == card.slug) {
            Some((_, kept)) => kept.extend(own),
            None => found.push((card.slug.to_string(), own.collect())),
        }
    }
    found
}

/// Refuse, naming the card, when `after` drops a line `before` had under any card.
pub fn check_nothing_lost(before: &str, after: &str, doing: &str) -> Result<(), ReviewError> {
    let kept = card_lines(after);
    for (card, lines) in card_lines(before) {
        let mut remaining: Vec<String> = kept
            .iter()
            .find(|(slug, _)| *slug == card)
            .map(|(_, lines)| lines.clone())
            .unwrap_or_default();
        let mut lost: Vec<String> = Vec::new();
        for line in lines {
            match remaining.iter().position(|kept| *kept == line) {
                Some(at) => {
                    remaining.remove(at);
                }
                None => lost.push(line),
            }
        }
        if let Some(first) = lost.first() {
            return Err(ReviewError(format!(
                "{doing} would drop {} line(s) of card {} (first: {}); nothing was written",
                lost.len(),
                repr(&card),
                repr(prefix_chars(first, 80))
            )));
        }
    }
    Ok(())
}

/// How many of `cards` carry each slug.
fn slug_counts(cards: &[Decision]) -> HashMap<&str, usize> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for card in cards {
        *counts.entry(card.slug.as_str()).or_default() += 1;
    }
    counts
}

/// Refuse, naming each card, when `after` holds a card twice, and more often than `before` did: a
/// write that copies a card leaves a file whose next write is refused as a duplicate slug.
fn refuse_repeats(before: &[Decision], after: &[Decision], doing: &str) -> Result<(), ReviewError> {
    let had = slug_counts(before);
    let mut repeated: Vec<&str> = slug_counts(after)
        .into_iter()
        .filter(|(slug, n)| *n > had.get(slug).copied().unwrap_or(0).max(1))
        .map(|(slug, _)| slug)
        .collect();
    if repeated.is_empty() {
        return Ok(());
    }
    symphony_board::order::sort_by(&mut repeated, &Ord::cmp);
    let named: Vec<String> = repeated.into_iter().map(repr).collect();
    Err(ReviewError(format!(
        "{doing} would repeat card(s) {}; nothing was written",
        named.join(", ")
    )))
}

/// Refuse, naming each card, when `after` repeats a card more often than `before` did.
pub fn check_nothing_repeated(before: &str, after: &str, doing: &str) -> Result<(), ReviewError> {
    refuse_repeats(
        &parse_decisions(&splitlines(before)),
        &parse_decisions(&splitlines(after)),
        doing,
    )
}

/// `lines` with `line` inserted at `at`, as Python's `list.insert` (past the end appends).
fn insert(lines: &mut Vec<String>, at: usize, line: String) {
    let at = at.min(lines.len());
    lines.insert(at, line);
}

/// Mark every card resolved that is resolvable as the project's card kinds read it, recording when.
/// The text and the slugs changed.
pub fn resolve(text: &str, now: &str, plugins: &crate::plugins::Plugins) -> (String, Vec<String>) {
    let mut lines: Vec<String> = splitlines(text).into_iter().map(str::to_string).collect();
    let cards = parse_decisions(&lines);
    let mut changed = Vec::new();
    for card in cards.iter().filter(|card| card.resolvable(plugins)).rev() {
        let index = card.line - 1;
        lines[index] = remark(&lines[index], 'r');
        insert(
            &mut lines,
            index + 1 + card.raw.len(),
            format!("  [{now}] agent · resolved"),
        );
        changed.push(card.slug.clone());
    }
    changed.reverse();
    (lines.join("\n") + "\n", changed)
}

/// Dismiss every card still asking the owner (pending, or an answered question) that a newer card
/// names in its `*Replaces:*`. A card the owner already decided keeps their verdict.
pub fn supersede(text: &str, now: &str) -> (String, Vec<String>) {
    let mut lines: Vec<String> = splitlines(text).into_iter().map(str::to_string).collect();
    let cards = parse_decisions(&lines);
    let mut replaced_by: HashMap<String, String> = HashMap::new();
    for card in &cards {
        for old in REPLACES.captures_iter(&lines[card.line - 1]) {
            if old[1] != card.slug {
                replaced_by
                    .entry(old[1].to_string())
                    .or_insert_with(|| card.slug.clone());
            }
        }
    }
    let mut changed = Vec::new();
    let replaced = cards.iter().filter(|card| {
        matches!(card.state, State::Pending | State::Question)
            && replaced_by.contains_key(&card.slug)
    });
    for card in replaced.collect::<Vec<_>>().into_iter().rev() {
        let index = card.line - 1;
        lines[index] = remark(&lines[index], '~');
        let by = &replaced_by[&card.slug];
        insert(
            &mut lines,
            index + 1 + card.raw.len(),
            format!("  [{now}] agent · dismissed — superseded by **{by}**"),
        );
        changed.push(card.slug.clone());
    }
    if changed.is_empty() {
        return (text.to_string(), changed);
    }
    changed.reverse();
    (lines.join("\n") + "\n", changed)
}
