//! The owner's word that ends a task's block (docs/workflow/TASKS.md, "What a task waits on").
//!
//! A block on `decision:NAME` ends once card NAME carries the owner's verdict that decides it
//! (approved, rejected or dismissed) in an owner's line dated since the minute the block was
//! recorded, on any page's copy of the card, main's or a branch's ([`Said`]): so whichever
//! checkout the owner's save wrote, main's or the worktree of the task that posted the card. The
//! newest verdict there is the card's: one that leaves it undecided (a question, the card set back
//! to pending) ends nothing. An owner's line since the block that ends nothing (a message, a
//! question) leaves the block and is told to the task's orchestrator once ([`told`]). A block on
//! `check:owner`, the default landing's hold, ends once the land's `owner` stamp stands on the tip
//! of its branch, by its one reader ([`symphony_board::stamps::read::owner_at`]): the owner's
//! approve on the task's thread naming the tip, or the owner's approve of every card the branch
//! posted with only the owner's lines and merges of the default branch after it. Either goes back
//! to the status its block recorded, as a `task:` block's end does.
//!
//! The shared server alone judges, on its board clock ([`Kept::look`]), from the cards its pages
//! hold and the board it read that tick: a save needs no hook of its own, and a verdict written
//! while no server ran, or by another build's save, is read at the first tick that reads the
//! documents. The board's write is fenced on the block the server read
//! ([`symphony_board::Store::resume_judged`]): a move or a block anew in between, the owner's or a
//! session's, wins, and its line is written once.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use symphony_board::waits_on::{Blocked, Thing};
use symphony_board::{Board, Message, Store};

use crate::{Decision, Who};

/// How long the server goes at most without reading the thread of a task held on the owner's
/// approve while no line was written on the board, in seconds: a commit to its branch moves no
/// message, and an approve of the new tip is read within it.
pub const RESCAN_SECONDS: f64 = 60.0;

/// One dated line of the owner's under a card.
#[derive(Debug, Clone, PartialEq)]
pub struct OwnerLine {
    /// The page whose copy of the card holds it (`docs/X.md`, `@<tree>/docs/X.md`).
    pub page: String,
    /// Its stamp as written, `YYYY-MM-DD HH:MM`.
    pub stamp: String,
    /// The stamp in seconds since the epoch.
    pub at: f64,
    /// Its verdict word (`approved`, `question`, …); none for a line that gives none.
    pub verdict: Option<String>,
    pub text: String,
}

/// The owner's dated lines under each card the pages hold, by the card's slug, oldest first: on
/// every copy, main's and each branch page's, a line two copies carry kept once.
pub type Said = BTreeMap<String, Vec<OwnerLine>>;

/// [`Said`] of the cards `cards`, each with the page that holds it.
pub fn said<'a>(cards: impl IntoIterator<Item = (&'a str, &'a Decision)>) -> Said {
    let mut said = Said::new();
    for (page, card) in cards {
        for entry in card.entries() {
            if entry.who != Who::Owner {
                continue;
            }
            let Some(stamp) = entry.ts else {
                continue;
            };
            let Some(at) = crate::verdicts::epoch(&stamp) else {
                continue;
            };
            let line = OwnerLine {
                page: page.to_string(),
                stamp,
                at,
                verdict: entry.verdict,
                text: entry.text.trim().to_string(),
            };
            let lines = said.entry(card.slug.clone()).or_default();
            let same = |kept: &OwnerLine| {
                (&kept.stamp, &kept.verdict, &kept.text) == (&line.stamp, &line.verdict, &line.text)
            };
            if !lines.iter().any(same) {
                lines.push(line);
            }
        }
    }
    for lines in said.values_mut() {
        // Stable: lines of one minute keep the order their card holds them in.
        lines.sort_by(|a, b| a.at.total_cmp(&b.at));
    }
    said
}

/// The verdicts that decide a card, each by the word a block's end says it in.
const DECIDES: [(&str, &str); 3] = [
    ("approved", "approve"),
    ("rejected", "reject"),
    ("dismissed", "dismiss"),
];

/// What the owner said on a card since a block on it was recorded ([`word`]).
#[derive(Debug, PartialEq)]
pub enum Word<'a> {
    /// The owner's newest verdict since then decides the card: its word (`approve`, `reject`,
    /// `dismiss`).
    Decided(&'static str),
    /// Nothing the owner said since then decides it: those lines that decide nothing, oldest first,
    /// each to tell once.
    Said(Vec<&'a OwnerLine>),
}

/// What the owner's `lines` under a card say of a block on it recorded at `since`, in seconds
/// since the epoch: the lines dated in that minute or after, the owner's stamps saying the minute
/// alone.
pub fn word(lines: &[OwnerLine], since: f64) -> Word<'_> {
    let since = (since / 60.0).floor() * 60.0;
    let after: Vec<&OwnerLine> = lines.iter().filter(|line| line.at >= since).collect();
    let decides = |verdict: &str| {
        DECIDES
            .iter()
            .find(|(said, _)| *said == verdict)
            .map(|(_, word)| *word)
    };
    // The card's verdict is the owner's newest; a line without one leaves it as it was.
    let newest = after.iter().rev().find_map(|line| line.verdict.as_deref());
    if let Some(word) = newest.and_then(decides) {
        return Word::Decided(word);
    }
    Word::Said(
        after
            .into_iter()
            .filter(|line| line.verdict.as_deref().and_then(decides).is_none())
            .filter(|line| line.verdict.is_some() || !line.text.is_empty())
            .collect(),
    )
}

/// Why a block on card `name` ended, as its line says it: `the owner decided NAME (approve)`.
pub fn decided_why(name: &str, word: &str) -> String {
    format!("the owner decided {name} ({word})")
}

/// Why a block on the owner's approve ended, `tip` the commit it named: `the owner approved
/// 1a2b3c4`.
pub fn approved_why(tip: &str) -> String {
    format!("the owner approved {}", short(tip))
}

/// Why a block on the owner's approve ended by the owner's approve of the cards `cards` the task's
/// branch posted, `tip` that branch's: `the owner approved card NAME, at 1a2b3c4`.
pub fn approved_cards_why(cards: &[String], tip: &str) -> String {
    let named = symphony_board::stamps::read::on(cards);
    format!("the owner approved {named}, at {}", short(tip))
}

/// A commit as a line says it: its first seven hex digits, as the land's hold names it.
fn short(commit: &str) -> &str {
    commit.get(..7).unwrap_or(commit)
}

/// How much of the owner's words a line telling them carries, in characters: the card has the rest.
const WORDS: usize = 300;

/// The line that tells a task's orchestrator what the owner said on card `name` that leaves its
/// block, and the key it is said once under on the task's thread.
pub fn told(name: &str, line: &OwnerLine) -> (String, String) {
    let words = match (line.verdict.as_deref(), line.text.as_str()) {
        (Some(verdict), "") => verdict.to_string(),
        (Some(verdict), text) => format!("{verdict} — {text}"),
        (None, text) => text.to_string(),
    };
    let words = match words.char_indices().nth(WORDS) {
        Some((cut, _)) => format!("{}…", &words[..cut]),
        None => words,
    };
    let text = format!(
        "the owner on card {name} [{}] ({}#{name}), deciding nothing, so it stays blocked on \
         decision:{name}: {words}",
        line.stamp, line.page
    );
    let key = format!(
        "decided:{name}:{}:{:016x}",
        line.stamp,
        fnv(&format!("{:?} {}", line.verdict, line.text))
    );
    (text, key)
}

/// The 64-bit FNV-1a hash of `text`: a stable key for what a line says, the same in every build.
fn fnv(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// Why the land's `owner` stamp stands on `tip`, read from the task's `thread` and the cards its
/// branch posted in the main checkout `root` as the land reads it
/// ([`symphony_board::stamps::read::owner_at`]): [`approved_why`] or [`approved_cards_why`]; none
/// while it does not.
pub fn approves(thread: &[Message], tip: &str, root: &Path) -> Option<String> {
    let reading = symphony_board::stamps::read::owner_at(thread, tip, root);
    reading.met.then(|| match reading.cards.as_slice() {
        [] => approved_why(tip),
        cards => approved_cards_why(cards, tip),
    })
}

/// The tip of the branch of the task `slug` in the checkout at `root`, as the land reads it.
fn tip(root: &Path, slug: &str) -> Option<String> {
    symphony_board::sweep::git::rev(root, &format!("refs/heads/{slug}^{{commit}}"))
        .filter(|tip| !tip.is_empty())
}

/// What one look did ([`Kept::look`]).
#[derive(Debug, Default, PartialEq)]
pub struct Looked {
    /// How many tasks it moved out of their blocks.
    pub moved: usize,
    /// What it did and what failed, a line each, for the server's log.
    pub lines: Vec<String>,
}

/// What the shared server keeps from one look to the next.
#[derive(Debug, Default)]
pub struct Kept {
    /// The lines told this server's life, by task and key: a look asks the board for none again
    /// (and the board would say each once anyway).
    told: BTreeSet<(String, String)>,
    /// When the thread of each task held on the owner's approve was last read, and the board's
    /// newest message then.
    read: BTreeMap<String, (i64, f64)>,
}

impl Kept {
    /// One look at the blocked tasks of `board`, the board on `store` as this tick read it, against
    /// the owner's lines `said` that the pages hold of the project at `root`, at `now`: each block
    /// the owner's word ended moved back, and each line of the owner's that leaves one told once.
    pub fn look(
        &mut self,
        store: &Store,
        root: &Path,
        board: &Board,
        said: &Said,
        now: f64,
    ) -> Looked {
        let mut looked = Looked::default();
        let mut held = BTreeSet::new();
        for task in &board.tasks {
            let Some(block) = Blocked::of(task) else {
                continue;
            };
            match block.thing() {
                Some(Thing::Decision(name)) => {
                    let lines = said.get(name).map_or(&[][..], Vec::as_slice);
                    match word(lines, block.at) {
                        Word::Decided(word) => {
                            let why = decided_why(name, word);
                            resume(store, &task.slug, &block, &why, &mut looked);
                        }
                        Word::Said(lines) => {
                            for line in lines {
                                self.tell(store, &task.slug, name, line, &mut looked);
                            }
                        }
                    }
                }
                Some(Thing::Check(symphony_board::land::needs::OWNER)) => {
                    held.insert(task.slug.clone());
                    if self.due(&task.slug, board.latest_message, now) {
                        approve(store, root, &task.slug, &block, &mut looked);
                    }
                }
                _ => {}
            }
        }
        // A task no longer held is read anew if it is held again.
        self.read.retain(|slug, _| held.contains(slug));
        looked
    }

    /// Whether the thread of `slug`, held on the owner's approve, is read now: never read, a line
    /// written on the board since (`latest` its newest message), or [`RESCAN_SECONDS`] gone by.
    fn due(&mut self, slug: &str, latest: i64, now: f64) -> bool {
        let due = self
            .read
            .get(slug)
            .is_none_or(|(seen, at)| *seen != latest || now - at >= RESCAN_SECONDS || now < *at);
        if due {
            self.read.insert(slug.to_string(), (latest, now));
        }
        due
    }

    /// The owner's `line` on card `name` told to the orchestrator of `slug`, once.
    fn tell(
        &mut self,
        store: &Store,
        slug: &str,
        name: &str,
        line: &OwnerLine,
        looked: &mut Looked,
    ) {
        let (text, key) = told(name, line);
        let id = (slug.to_string(), key);
        if self.told.contains(&id) {
            return;
        }
        match store.tell_orchestrator_once(slug, &text, &id.1) {
            Ok(()) => {
                looked.lines.push(format!(
                    "{slug}: told the owner's line on {name} [{}]",
                    line.stamp
                ));
                self.told.insert(id);
            }
            Err(error) => looked.lines.push(format!(
                "{slug}: the owner's line on {name} not told: {error}"
            )),
        }
    }
}

/// The task `slug`, held on the owner's approve as `block`, moved back once the land's `owner` stamp
/// stands on the tip of its branch in the checkout at `root` ([`approves`]).
fn approve(store: &Store, root: &Path, slug: &str, block: &Blocked, looked: &mut Looked) {
    let Some(tip) = tip(root, slug) else {
        return;
    };
    match store
        .thread(slug)
        .map(|thread| approves(&thread, &tip, root))
    {
        Ok(Some(why)) => resume(store, slug, block, &why, looked),
        Ok(None) => {}
        Err(error) => looked
            .lines
            .push(format!("{slug}: its thread not read: {error}")),
    }
}

/// `slug` moved out of `block` for `why`, when it is still blocked so.
fn resume(store: &Store, slug: &str, block: &Blocked, why: &str, looked: &mut Looked) {
    match store.resume_judged(slug, block, why) {
        Ok(Some(back)) => {
            looked.moved += 1;
            looked
                .lines
                .push(format!("{slug}: unblocked, {why}; back to {back}"));
        }
        Ok(None) => {}
        Err(error) => looked
            .lines
            .push(format!("{slug}: not unblocked ({why}): {error}")),
    }
}

#[cfg(test)]
#[path = "decided_tests.rs"]
mod tests;
