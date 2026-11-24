//! One card from the copies of it the owner could read: main's, a branch's posted one, and the
//! owner's own lines a worktree holds uncommitted. Each copy's dated lines are kept, in time order,
//! so the owner's line and the agent's answer show wherever the card is read.

use std::collections::HashSet;

use symphony_docs::syntax;

use crate::register::{Decision, QUESTION, State, Who, unseen_reply};
use crate::text::{rstrip, strip};

/// One line said under a card with the lines that continue it, as written.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Chunk {
    lines: Vec<String>,
    ts: Option<String>,
    who: Who,
    /// The mark the line sets: an owner's verdict, or an agent's resolved, auto-approved or
    /// dismissed line. An agent's question and every plain line set none.
    sets: Option<State>,
}

impl Chunk {
    /// What tells this line from another: its lines with trailing spaces and blank lines dropped.
    fn key(&self) -> Vec<String> {
        let mut key: Vec<String> = self.lines.iter().map(|l| rstrip(l).to_string()).collect();
        while key.last().is_some_and(String::is_empty) {
            key.pop();
        }
        key
    }

    /// Whether the owner said it.
    pub(crate) fn owner(&self) -> bool {
        self.who == Who::Owner
    }
}

/// The lines under a card as [`Chunk`]s, read as [`syntax::entries`] reads them: a dated line starts
/// one, a deeper-indented line continues it, a `↳` line is an agent's undated answer, a blank or
/// hidden line belongs to the line above, and anything else is an owner's undated note.
pub(crate) fn chunks(raw: &[String]) -> Vec<Chunk> {
    let mut found: Vec<Chunk> = Vec::new();
    for line in raw {
        let text = line.trim_matches(syntax::is_space);
        let continues =
            found.last().is_some_and(|c| c.ts.is_some()) && syntax::is_continuation(line);
        let start = if unseen_reply(line).is_some() || text.is_empty() || continues {
            None
        } else if let Some(entry) = syntax::entry_line(line) {
            // Any line not an agent's is a person's, whatever name it carries.
            let who = Who::of(entry.who);
            let sets = entry
                .verdict
                .as_deref()
                .filter(|verdict| !(who == Who::Agent && *verdict == QUESTION))
                .and_then(State::parse);
            Some((entry.ts, who, sets))
        } else if text.starts_with('↳') {
            Some((None, Who::Agent, None))
        } else {
            Some((None, Who::Owner, None))
        };
        match (start, found.last_mut()) {
            (Some((ts, who, sets)), _) => found.push(Chunk {
                lines: vec![line.clone()],
                ts,
                who,
                sets,
            }),
            (None, Some(last)) => last.lines.push(line.clone()),
            (None, None) => found.push(Chunk {
                lines: vec![line.clone()],
                ts: None,
                who: Who::Owner,
                sets: None,
            }),
        }
    }
    found
}

/// The latest dated line of `chunks` that `wanted` keeps, as its time.
fn latest(chunks: &[Chunk], wanted: impl Fn(&Chunk) -> bool) -> Option<&str> {
    chunks
        .iter()
        .filter(|c| wanted(c))
        .filter_map(|c| c.ts.as_deref())
        .max()
}

/// Where the latest dated line of `chunks` that `wanted` keeps sits: the latest by time, and of the
/// lines of one minute the last in place. Lines carry minutes, so a copy orders what was said in
/// one minute by where it wrote it.
fn last(chunks: &[Chunk], wanted: impl Fn(&Chunk) -> bool) -> Option<usize> {
    chunks
        .iter()
        .enumerate()
        .filter(|(_, c)| c.ts.is_some() && wanted(c))
        .max_by_key(|(_, c)| c.ts.as_deref()) // the last of equal times
        .map(|(at, _)| at)
}

/// The order of `chunks[at]` among the lines of `chunks`: by time, then by place.
fn when(chunks: &[Chunk], at: usize) -> (Option<&str>, usize) {
    (chunks[at].ts.as_deref(), at)
}

/// Whether the latest dated line `wanted` keeps of `ours`, one copy's lines, is later than the
/// latest of `theirs`, another copy's: by time, or in the same minute when `ours` holds `theirs`'s
/// line before its own. A tie neither orders that way (the same line, or two lines no copy holds
/// together) is not later.
fn later(ours: &[Chunk], theirs: &[Chunk], wanted: impl Fn(&Chunk) -> bool + Copy) -> bool {
    let Some(mine) = last(ours, wanted) else {
        return false;
    };
    let Some(other) = last(theirs, wanted) else {
        return true;
    };
    let (mine_at, other) = (&ours[mine], &theirs[other]);
    mine_at.ts > other.ts
        || (mine_at.ts == other.ts && {
            let other = other.key();
            ours[..mine].iter().any(|c| c.key() == other)
        })
}

/// `card` with each line of `extra` it does not hold, placed after the last of its lines dated no
/// later, so after `card`'s lines of its own minute. Its mark becomes the one `extra`'s latest
/// marking line sets when that line, so placed, is later than every marking line `card` holds:
/// dated later, or in the same minute, the last of one minute's lines deciding (an owner's verdict
/// laid on a posted card committed in its minute). Otherwise it stays `card`'s.
pub(crate) fn with_lines(card: &Decision, extra: &[Chunk]) -> Decision {
    let own = chunks(&card.raw);
    let mut held: HashSet<Vec<String>> = own.iter().map(Chunk::key).collect();
    let marked = latest(&own, |c| c.sets.is_some());
    let mut merged = own.clone();
    let mut mark: Option<(&str, State)> = None;
    for chunk in extra {
        if !held.insert(chunk.key()) {
            continue;
        }
        let at = match chunk.ts.as_deref() {
            Some(ts) => merged
                .iter()
                .rposition(|c| c.ts.as_deref().is_none_or(|t| t <= ts))
                .map_or(0, |i| i + 1),
            None => merged
                .iter()
                .position(|c| c.ts.is_some())
                .unwrap_or(merged.len()),
        };
        merged.insert(at, chunk.clone());
        if let (Some(ts), Some(state)) = (chunk.ts.as_deref(), chunk.sets)
            && marked.is_none_or(|own| ts >= own)
            && mark.is_none_or(|(at, _)| ts >= at)
        {
            mark = Some((ts, state));
        }
    }
    let raw: Vec<String> = merged.into_iter().flat_map(|c| c.lines).collect();
    Decision {
        line: card.line,
        slug: card.slug.clone(),
        state: mark.map_or(card.state, |(_, state)| state),
        text: card.text.clone(),
        note: raw
            .iter()
            .filter(|line| unseen_reply(line).is_none())
            .map(|line| strip(line))
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect(),
        topic: card.topic.clone(),
        raw,
    }
}

/// The one card main's copy `main` and a branch's copy `branch` make. Every line either holds shows,
/// in time order. The mark is the copy's whose latest marking line is later ([`later`]: in the same
/// minute, the one a copy holding both wrote after the other), main's on a tie no copy orders, so
/// main's resolution after a stale branch copy stands. The text is the copy's whose last line is
/// later, the branch's on such a tie, a revision being the branch's proposal. Its place (line,
/// topic) is the branch's.
pub fn one_card(main: &Decision, branch: &Decision) -> Decision {
    let main_lines = chunks(&main.raw);
    let branch_lines = chunks(&branch.raw);
    let mut one = if later(&main_lines, &branch_lines, |_| true) {
        with_lines(main, &branch_lines)
    } else {
        with_lines(branch, &main_lines)
    };
    let marking = |c: &Chunk| c.sets.is_some();
    if later(&branch_lines, &main_lines, marking) {
        one.state = branch.state;
    } else if last(&main_lines, marking).is_some() {
        one.state = main.state;
    }
    one.line = branch.line;
    one.topic = branch.topic.clone();
    one
}

/// Main's copy `main` with the owner's lines `owner` holds: what a finished branch's worktree adds
/// to a card. Main's text and place stay; a verdict of the owner's no earlier than main's sets the
/// mark ([`with_lines`]): in a minute no copy orders, the owner's, the opposite of [`one_card`]'s
/// tie, so a tie never drops what the owner said on main's card.
pub fn with_owner_lines(main: &Decision, owner: &Decision) -> Decision {
    with_lines(main, &chunks(&owner.raw))
}

/// Whether `other` is a stale copy of main's `main`: main's copy is `closed` (resolved, or rejected
/// and answered) with a line later than every line `other` holds ([`later`]). A stale copy adds
/// nothing, so a branch's old copy never reopens what main settled after it.
pub fn stale(main: &Decision, closed: bool, other: &Decision) -> bool {
    closed && later(&chunks(&main.raw), &chunks(&other.raw), |_| true)
}

/// Whether `a` and `b` say the same: mark, text and every line.
pub fn same(a: &Decision, b: &Decision) -> bool {
    a.slug == b.slug
        && a.state == b.state
        && a.text == b.text
        && chunks(&a.raw).iter().map(Chunk::key).collect::<Vec<_>>()
            == chunks(&b.raw).iter().map(Chunk::key).collect::<Vec<_>>()
}

/// The owner's lines of `worktree`'s copy that `posted` does not hold: what the owner said on a
/// branch's page that no commit has yet.
pub(crate) fn owner_lines(worktree: &Decision, posted: Option<&Decision>) -> Vec<Chunk> {
    owner_lines_beside(worktree, posted.map(|p| p.raw.as_slice()))
}

/// The owner's dated lines of `card` that none of `copies` (other copies' lines) holds.
pub(crate) fn owner_lines_beside<'a>(
    card: &Decision,
    copies: impl IntoIterator<Item = &'a [String]>,
) -> Vec<Chunk> {
    let held = keys(copies);
    chunks(&card.raw)
        .into_iter()
        .filter(|c| c.owner() && c.ts.is_some() && !held.contains(&c.key()))
        .collect()
}

/// The thread the owner started on `card` that none of `copies` holds: the owner's dated lines no
/// copy holds, and from the first of them on (by time, then by place: an agent's line written above
/// it in its minute came before it), every agent's dated line no copy holds. What a finished
/// branch's worktree adds to a card: the owner's question and the answer under it.
pub(crate) fn thread_beside<'a>(
    card: &Decision,
    copies: impl IntoIterator<Item = &'a [String]>,
) -> Vec<Chunk> {
    let held = keys(copies);
    let lines: Vec<Chunk> = chunks(&card.raw)
        .into_iter()
        .filter(|c| c.ts.is_some() && !held.contains(&c.key()))
        .collect();
    let Some(first) = lines
        .iter()
        .enumerate()
        .filter(|(_, c)| c.owner())
        .min_by_key(|(_, c)| c.ts.as_deref()) // the first of equal times
        .map(|(at, c)| (c.ts.clone(), at))
    else {
        return Vec::new();
    };
    lines
        .into_iter()
        .enumerate()
        .filter(|(at, c)| c.owner() || (c.ts.as_deref(), *at) > (first.0.as_deref(), first.1))
        .map(|(_, c)| c)
        .collect()
}

/// The cards of a file a merge stopped on, as its branch has them: `ours`, the cards of the
/// branch's side of the file, each read as the branch's committed copy of it (in `head`, the
/// committed file's cards) with the dated lines its side holds under it that neither the committed
/// file nor the other side (`theirs`) holds anywhere: what was saved since the merge began. A line
/// git left outside a conflict, after a line two cards share, is the other side's and never counts.
/// A card the committed file lacks keeps only such lines.
pub(crate) fn since_merge(
    ours: Vec<Decision>,
    head: &[Decision],
    theirs: &[Decision],
) -> Vec<Decision> {
    let held = keys(head.iter().chain(theirs).map(|card| card.raw.as_slice()));
    ours.into_iter()
        .map(|card| {
            let saved: Vec<Chunk> = chunks(&card.raw)
                .into_iter()
                .filter(|c| c.ts.is_some() && !held.contains(&c.key()))
                .collect();
            match head.iter().find(|committed| committed.slug == card.slug) {
                Some(committed) => with_lines(committed, &saved),
                None => with_lines(
                    &Decision {
                        raw: Vec::new(),
                        note: Vec::new(),
                        ..card
                    },
                    &saved,
                ),
            }
        })
        .collect()
}

/// What tells a line from another, its hidden lines left out: what it says.
fn said_key(chunk: &Chunk) -> Vec<String> {
    let mut key = chunk.key();
    key.retain(|line| unseen_reply(line).is_none());
    key
}

/// Whether `card` rewrites the heading or an existing part of the body in `text` and `raw`.
/// Appending thread history alone keeps the proposal it replies to: every existing chunk stays
/// at the beginning, with blank padding and unseen-reply markers ignored as in [`changed_since`].
pub(crate) fn rewrites(card: &Decision, text: &str, raw: &[String]) -> bool {
    let body = |raw: &[String]| {
        chunks(raw)
            .iter()
            .map(said_key)
            .filter(|key| !key.is_empty())
            .collect::<Vec<_>>()
    };
    card.text != text || !body(&card.raw).starts_with(&body(raw))
}

/// Whether `worktree`, a card's copy in its worktree, says anything its posted copy `posted` does
/// not besides the owner's dated lines: another text, a line of an agent's, or a line taken out.
/// Its agent is changing it, so a verdict on it would cover what the owner has not seen.
pub(crate) fn changed_since(worktree: &Decision, posted: &Decision) -> bool {
    let here: HashSet<Vec<String>> = chunks(&worktree.raw).iter().map(said_key).collect();
    let there = chunks(&posted.raw);
    let posted_keys: HashSet<Vec<String>> = there.iter().map(said_key).collect();
    worktree.text != posted.text
        || there.iter().any(|c| !here.contains(&said_key(c)))
        || chunks(&worktree.raw)
            .iter()
            .any(|c| !(c.owner() && c.ts.is_some()) && !posted_keys.contains(&said_key(c)))
}

/// Every line `copies` hold, as [`Chunk::key`]s.
fn keys<'a>(copies: impl IntoIterator<Item = &'a [String]>) -> HashSet<Vec<String>> {
    copies
        .into_iter()
        .flat_map(|raw| chunks(raw).into_iter().map(|c| c.key()))
        .collect()
}

/// What `card` says that none of `copies` (other copies' text and lines) does: its text when no
/// copy has it, and the lines no copy holds. Against the copies it was written from, that is what
/// its author wrote: a branch's agent answering main's card wrote its answer, not main's pictures.
pub(crate) fn written<'a>(
    card: &Decision,
    copies: impl IntoIterator<Item = (&'a str, &'a [String])>,
) -> Decision {
    let mut texts: HashSet<&str> = HashSet::new();
    let mut raws: Vec<&[String]> = Vec::new();
    for (text, raw) in copies {
        texts.insert(text);
        raws.push(raw);
    }
    let held = keys(raws);
    let raw: Vec<String> = chunks(&card.raw)
        .into_iter()
        .filter(|c| !held.contains(&c.key()))
        .flat_map(|c| c.lines)
        .collect();
    Decision {
        text: if texts.contains(card.text.as_str()) {
            String::new()
        } else {
            card.text.clone()
        },
        note: Vec::new(),
        raw,
        ..card.clone()
    }
}

/// Whether the owner spoke last on `card`: a dated line of the owner's later than every dated line
/// of an agent's, by time and, in one minute, by place. Such a line waits on an agent, wherever it
/// was saved, and so always counts.
pub(crate) fn awaits_agent(card: &Decision) -> bool {
    let lines = chunks(&card.raw);
    match (last(&lines, Chunk::owner), last(&lines, |c| !c.owner())) {
        (Some(owner), Some(agent)) => when(&lines, owner) > when(&lines, agent),
        (owner, _) => owner.is_some(),
    }
}

/// The card as the register writes it, its first line and its lines: what a page renders for it.
pub fn markdown(card: &Decision) -> String {
    let mut text = format!(
        "- [{}] **{}** — {}\n",
        card.state.mark(),
        card.slug,
        card.text
    );
    for line in &card.raw {
        text.push_str(line);
        text.push('\n');
    }
    text
}

#[cfg(test)]
#[path = "one_tests.rs"]
mod tests;
