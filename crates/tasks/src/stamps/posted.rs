//! The cards a task's branch posted for the owner's look, and whether the owner's approve of them
//! stands for the `owner` stamp at the branch's tip ([`read`]; docs/workflow/TASKS.md, "Needed stamps").
//! Everything is read from git in the main checkout, never from a worktree's files: the land
//! carries commits, so an owner's line no commit carries counts for nothing.
//!
//! It stands at the tip T when all of these hold:
//!
//! 1. **Posted.** The branch posted a card: one in a project document at T, inline where the
//!    thing it decides is said or in a register region, whose copy, the owner's lines and its
//!    mark aside, the merge base of T with the default branch holds in none of its documents
//!    ([`Written`]), as the docs pages read a branch's posted cards. A card the base holds that
//!    only the owner answered since, or that only moved to another document, is none the branch
//!    posted.
//! 2. **Approved.** Each card it posted has `approved` as its newest owner verdict at T
//!    ([`newest`]): a reject, a question, pending, dismissed or none, on any of them, and it does
//!    not stand. The owner's, or a decider's: a person a board named writes as the owner does
//!    (`ana · approved`), and decides the stamp while the board gives her the decide power
//!    ([`read_by`], docs/product/analysis/users-and-identity.md); a person's who holds none is no
//!    approve, and a later word of anyone's that is no approve keeps the approve from standing, as
//!    it opens the card again on every page ([`decided`]).
//! 3. **Nothing since.** Let B be the commit before the first commit that carries the newest of
//!    those approves: the commit the owner's page showed when the owner gave it. Each commit after
//!    B up to T, on T's first-parent line, adds the owner's lines under the posted cards and
//!    nothing else ([`owners_only`]: their marks and their places in the register may move with
//!    them, as a save regroups it), or is a merge of the default branch that brings only what the
//!    default branch had ([`of_main`]). A commit a reviewer approved is no exception: the owner
//!    asked for a look of their own.
//!
//! The walk goes from T down its first-parent line to the first commit that is neither; the
//! approve stands when that commit does not carry it yet, so it came after it.

use crate::land::carried;
use crate::sweep::git::{ancestor, blobs, git, pathspecs, rev, stdout, under};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use symphony_docs::syntax::{
    CARD_LINE, Card, REGION_END, REGION_START, Said, cards, entry_line, is_continuation, lines,
    said, unseen_owner,
};

/// Who writes the owner's lines under a card.
const OWNER: &str = "owner";
/// The owner's verdict that approves a card.
const APPROVED: &str = "approved";

/// What the cards a branch posted say of the `owner` stamp at its tip ([`read`]).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Posted {
    /// Nothing either way: the branch posted no card, one it posted lacks the owner's approve, or
    /// git could not say.
    #[default]
    Nothing,
    /// Every card it posted (`cards`, by name) carries the owner's approve, and only the owner's
    /// lines under them and merges of the default branch came after the newest: the stamp stands
    /// on them.
    Approved { cards: Vec<String> },
    /// Every card it posted carries the owner's approve, but `commit`, which is neither the
    /// owner's lines nor a merge of the default branch, came after the newest.
    Changed { cards: Vec<String>, commit: String },
}

/// What the cards a branch posted say of its `owner` stamp, and who gave the newest approve they
/// stand on: `owner`, or a person who holds decide ([`read_by`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    pub posted: Posted,
    pub by: String,
}

/// What the cards the branch at `tip` posted say of its `owner` stamp, read in the main checkout
/// `main` (the module's doc).
pub fn read(main: &Path, tip: &str) -> Posted {
    read_by(main, tip, &[]).posted
}

/// [`read`] on a board where `deciders` hold the decide power: a card's newest verdict of the
/// owner's or theirs is the one that must approve it (rule 2), and its approve the one rule 3
/// reads, as the owner's is.
pub fn read_by(main: &Path, tip: &str, deciders: &[String]) -> Approval {
    approval(main, tip, deciders).unwrap_or_else(|| Approval {
        posted: Posted::Nothing,
        by: OWNER.into(),
    })
}

/// [`read_by`], none where git could not say.
fn approval(main: &Path, tip: &str, deciders: &[String]) -> Option<Approval> {
    let nothing = || {
        Some(Approval {
            posted: Posted::Nothing,
            by: OWNER.into(),
        })
    };
    // A default branch the project file's rule refuses names no branch to read the cards on.
    let default = crate::live::default_branch(Some(main))
        .ok()?
        .unwrap_or_else(|| crate::live::MAIN.into());
    let default_tip = rev(main, &format!("refs/heads/{default}^{{commit}}"))?;
    let base = fork(main, tip, &default_tip)?;
    let folders = crate::project::documents_at(main);
    let specs = pathspecs(&folders);
    let mut args = vec![
        "diff",
        "--name-only",
        "--no-renames",
        "-z",
        &base,
        tip,
        "--",
    ];
    args.extend(&specs);
    let changed = names(main, &args)?;
    let at_tip: Vec<Card> = (texts(main, tip, &changed)?.into_iter().flatten())
        .flat_map(|text| cards(&lines(&text)))
        .collect();
    if at_tip.is_empty() {
        return nothing();
    }
    let held = carded(main, &base, &specs)?;
    let mut forked: BTreeMap<String, Vec<Written>> = BTreeMap::new();
    for text in texts(main, &base, &held)?.into_iter().flatten() {
        for card in cards(&lines(&text)) {
            let written = Written::of(&card);
            forked.entry(card.slug).or_default().push(written);
        }
    }
    let posted: Vec<&Card> = (at_tip.iter())
        .filter(|card| {
            (forked.get(&card.slug)).is_none_or(|held| !held.contains(&Written::of(card)))
        })
        .collect();
    if posted.is_empty() {
        return nothing();
    }
    // The newest approve of them all, with the card it is under.
    let mut approve: Option<(&str, Said)> = None;
    for card in &posted {
        let verdict = newest(card, deciders)?;
        if verdict.entry.verdict.as_deref() != Some(APPROVED) || !decided(card) {
            return nothing();
        }
        if approve
            .as_ref()
            .is_none_or(|(_, kept)| kept.entry.ts <= verdict.entry.ts)
        {
            approve = Some((&card.slug, verdict));
        }
    }
    let (slug, line) = approve?;
    let names: Vec<String> = posted.iter().map(|card| card.slug.clone()).collect();
    let mut named: BTreeSet<String> = BTreeSet::new();
    named.extend(names.iter().cloned());
    // A tip with no commit of its own past the base posted nothing.
    let Some(before) = first_other(main, tip, &base, &default_tip, &named, &folders)? else {
        return nothing();
    };
    let carried = (carded(main, &before, &specs).and_then(|held| texts(main, &before, &held)))?
        .into_iter()
        .flatten()
        .any(|text| {
            (cards(&lines(&text)).iter())
                .any(|card| card.slug == slug && said(&card.raw).contains(&line))
        });
    let posted = if carried {
        Posted::Changed {
            cards: names,
            commit: before,
        }
    } else {
        Posted::Approved { cards: names }
    };
    Some(Approval {
        posted,
        by: line.name.clone(),
    })
}

/// Where `tip` forked from the default branch, at `default_tip`: their merge base. For a tip the
/// default branch holds already, its merge base with the default branch as it was before the
/// merge that brought it in (the oldest commit on the default branch's first-parent line that
/// holds it), so what its branch posted reads the same once it landed: on a done task's page, and
/// to a delivery that reads the evidence again after its merge. None where git could not say.
fn fork(main: &Path, tip: &str, default_tip: &str) -> Option<String> {
    let base = stdout(main, &["merge-base", default_tip, tip])?
        .trim()
        .to_string();
    if base != tip {
        return Some(base);
    }
    let range = format!("{tip}..{default_tip}");
    let after = stdout(
        main,
        &["rev-list", "--first-parent", "--ancestry-path", &range],
    )?;
    let Some(merge) = after.lines().last() else {
        return Some(base);
    };
    let before = rev(main, &format!("{merge}^1^{{commit}}"))?;
    let base = stdout(main, &["merge-base", &before, tip])?;
    Some(base.trim().to_string())
}

/// The newest verdict under `card` that decides it: the latest by its time of the owner's own
/// dated lines that give one, and of each of `deciders`', the last written of one minute; none
/// where there is none. A person's who holds no decide is none (the module's doc).
fn newest(card: &Card, deciders: &[String]) -> Option<Said> {
    newest_of(card, |said| {
        said.name == OWNER || deciders.contains(&said.name)
    })
}

/// Whether `card`'s newest verdict of every person's, the owner's and any a board named, is an
/// approve: a person's word after the owner's approve that is none keeps it from standing.
fn decided(card: &Card) -> bool {
    newest_of(card, |_| true).is_some_and(|said| said.entry.verdict.as_deref() == Some(APPROVED))
}

/// The newest verdict under `card` of a person's dated lines that `whose` takes, as [`newest`]
/// reads the owner's.
fn newest_of(card: &Card, whose: impl Fn(&Said) -> bool) -> Option<Said> {
    (said(&card.raw).into_iter())
        .filter(|said| said.entry.who == OWNER && whose(said))
        .filter(|said| said.entry.ts.is_some() && said.entry.verdict.is_some())
        .max_by(|a, b| a.entry.ts.cmp(&b.entry.ts))
}

/// The newest commit on `tip`'s first-parent line past `base` that neither adds only the owner's
/// lines under the cards `posted` ([`owners_only`]) nor merges the default branch, at
/// `default_tip`, as git would ([`of_main`]); where every one does, the commit the oldest of them
/// follows. Inside, none where the line holds no commit; none at all where git could not say.
fn first_other(
    main: &Path,
    tip: &str,
    base: &str,
    default_tip: &str,
    posted: &BTreeSet<String>,
    folders: &[String],
) -> Option<Option<String>> {
    let exclude = format!("^{base}");
    let line = stdout(
        main,
        &["log", "--first-parent", "--format=%H %P", tip, &exclude],
    )?;
    let mut below = None;
    for commit in line.lines() {
        let mut ids = commit.split_whitespace();
        let Some(id) = ids.next() else { continue };
        let parents: Vec<&str> = ids.collect();
        let only = match parents[..] {
            [parent] => owners_only(main, parent, id, posted, folders)?,
            [ours, theirs] => of_main(main, id, ours, theirs, default_tip),
            _ => false,
        };
        if !only {
            return Some(Some(id.to_string()));
        }
        below = parents.first().map(|parent| parent.to_string());
    }
    Some(below)
}

/// Whether the commit `id`, of the one parent `parent`, changes nothing but the owner's lines
/// under the cards `posted`, adding them: every path it changes a document of the project's
/// `folders` that both hold, the same but for those lines, the cards' marks and where in the
/// register they sit ([`Doc`]). Each other card, and every other line, as it was.
fn owners_only(
    main: &Path,
    parent: &str,
    id: &str,
    posted: &BTreeSet<String>,
    folders: &[String],
) -> Option<bool> {
    let changed = names(
        main,
        &["diff", "--name-only", "--no-renames", "-z", parent, id],
    )?;
    if changed.iter().any(|file| !under(file, folders)) {
        return Some(false);
    }
    let (before, after) = (texts(main, parent, &changed)?, texts(main, id, &changed)?);
    Some(before.into_iter().zip(after).all(|pair| match pair {
        (Some(before), Some(after)) => {
            Doc::of(&before, posted).adds_owners(&Doc::of(&after, posted))
        }
        _ => false,
    }))
}

/// Whether the commit `id` merges `theirs`, a commit the default branch at `default_tip` holds,
/// into `ours` and brings only what the default branch had: its tree is git's own merge of the
/// two, which it made without a conflict. A merge git cannot read is not one.
fn of_main(main: &Path, id: &str, ours: &str, theirs: &str, default_tip: &str) -> bool {
    if !ancestor(main, theirs, default_tip) {
        return false;
    }
    let Ok((tree, conflicted)) = carried::merged(main, ours, theirs) else {
        return false;
    };
    conflicted.is_empty() && rev(main, &format!("{id}^{{tree}}")).is_some_and(|own| own == tree)
}

/// What a card says that the branch wrote: its text and its lines but the owner's dated lines,
/// the lines that continue them and blank lines; its mark aside, which the owner's verdict sets.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Written {
    text: String,
    lines: Vec<String>,
}

impl Written {
    fn of(card: &Card) -> Written {
        Written {
            text: card.text.clone(),
            lines: split(&card.raw).1,
        }
    }
}

/// The lines `raw` under a card, as the owner's dated entries (each with the lines that continue
/// it, as one) and every other line, blank lines in neither: a line a card's entries read as an
/// owner's dated line ([`symphony_docs::syntax::entries`]) starts one of the owner's, a deeper-indented line goes with
/// the dated line above it, and the hidden `review:unseen-owner` line is an agent's. A person's a
/// board named goes with the owner's: a word on the card, never what the branch posted.
fn split(raw: &[String]) -> (Vec<String>, Vec<String>) {
    let (mut owner, mut rest): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
    // Whose dated entry the lines under it continue: the owner's, an agent's, or none.
    let mut dated: Option<bool> = None;
    for line in raw {
        let line = line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if unseen_owner(line).is_some() {
            rest.push(line.to_string());
            continue;
        }
        let into = match dated {
            Some(owners) if is_continuation(line) => owners,
            _ => match entry_line(line) {
                Some(entry) => {
                    let owners = entry.who == OWNER;
                    dated = Some(owners);
                    if owners {
                        owner.push(line.to_string());
                        continue;
                    }
                    false
                }
                None => {
                    dated = None;
                    false
                }
            },
        };
        match (into, owner.last_mut()) {
            (true, Some(entry)) => {
                entry.push('\n');
                entry.push_str(line);
            }
            _ => rest.push(line.to_string()),
        }
    }
    (owner, rest)
}

/// A document as a commit adding the owner's lines may change it ([`Doc::adds_owners`]): its cards
/// by name, and every other line in order but blank lines and, inside a register, its headings,
/// which a save writes again as it regroups the cards.
#[derive(Debug, PartialEq, Eq)]
struct Doc {
    other: Vec<String>,
    cards: BTreeMap<String, Vec<Copy>>,
}

/// One copy of a card in a [`Doc`]: what the branch wrote, its mark (none for a card posted, whose
/// mark the owner's verdict sets), its topic, and the owner's lines under it, in order.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Copy {
    written: Written,
    mark: Option<char>,
    topic: String,
    owner: Vec<String>,
}

impl Doc {
    /// `text` read as a [`Doc`], the cards `posted` without their marks.
    fn of(text: &str, posted: &BTreeSet<String>) -> Doc {
        let all = lines(text);
        let found = cards(&all);
        let mut carded = vec![false; all.len()];
        let mut by_name: BTreeMap<String, Vec<Copy>> = BTreeMap::new();
        for card in found {
            let first = card.line - 1;
            let span = carded.iter_mut().skip(first).take(1 + card.raw.len());
            span.for_each(|at| *at = true);
            let (owner, lines) = split(&card.raw);
            let copy = Copy {
                written: Written {
                    text: card.text.clone(),
                    lines,
                },
                mark: (!posted.contains(&card.slug)).then_some(card.mark),
                topic: card.topic.clone(),
                owner,
            };
            by_name.entry(card.slug).or_default().push(copy);
        }
        for copies in by_name.values_mut() {
            crate::order::sort_by(copies, &Ord::cmp);
        }
        let mut other = Vec::new();
        let mut register = false;
        for (line, carded) in all.iter().zip(carded) {
            let trimmed = line.trim();
            if trimmed == REGION_START {
                register = true;
            } else if trimmed == REGION_END {
                register = false;
            } else if carded || trimmed.is_empty() || (register && trimmed.starts_with('#')) {
                continue;
            }
            other.push(line.trim_end().to_string());
        }
        Doc {
            other,
            cards: by_name,
        }
    }

    /// Whether `after` is this document with the owner's lines added under its cards and nothing
    /// else: the same other lines and the same cards, each as it was but for its owner's lines,
    /// which keep every one it had.
    fn adds_owners(&self, after: &Doc) -> bool {
        self.other == after.other
            && self.cards.len() == after.cards.len()
            && self.cards.iter().all(|(name, before)| {
                after.cards.get(name).is_some_and(|after| {
                    before.len() == after.len()
                        && before.iter().zip(after).all(|(was, now)| {
                            (&was.written, was.mark, &was.topic)
                                == (&now.written, now.mark, &now.topic)
                                && (was.mark.is_none() || was.owner == now.owner)
                                && kept(&was.owner, &now.owner)
                        })
                })
            })
    }
}

/// Whether every line of `was` is in `now`, as many times.
fn kept(was: &[String], now: &[String]) -> bool {
    let mut left: Vec<&String> = now.iter().collect();
    was.iter()
        .all(|line| match left.iter().position(|kept| *kept == line) {
            Some(at) => {
                left.swap_remove(at);
                true
            }
            None => false,
        })
}

/// The names git prints NUL-separated for `args` in `main`; none where it could not run them.
fn names(main: &Path, args: &[&str]) -> Option<Vec<String>> {
    let done = git(main, args).filter(|done| done.status.success())?;
    Some(
        (done.stdout.split(|b| *b == 0))
            .filter(|name| !name.is_empty())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .collect(),
    )
}

/// The documents of `rev` under the pathspecs `specs` that may hold a card, a register's marker
/// or a card's line anywhere in them; none where git could not say.
fn carded(main: &Path, rev: &str, specs: &[&str]) -> Option<Vec<String>> {
    let mut args = vec![
        "grep",
        "-l",
        "-z",
        "-E",
        "-e",
        REGION_START,
        "-e",
        CARD_LINE,
        rev,
        "--",
    ];
    args.extend(specs);
    let done = git(main, &args)?;
    // 1 is a grep that found none.
    if !matches!(done.status.code(), Some(0 | 1)) {
        return None;
    }
    let prefix = format!("{rev}:");
    Some(
        (done.stdout.split(|b| *b == 0))
            .filter(|name| !name.is_empty())
            .map(|name| String::from_utf8_lossy(name).into_owned())
            .map(|name| {
                name.strip_prefix(&prefix)
                    .map_or(name.clone(), str::to_string)
            })
            .collect(),
    )
}

/// The text of each of `files` at `rev`, none where it holds none; none at all where git could not
/// read them.
fn texts(main: &Path, rev: &str, files: &[String]) -> Option<Vec<Option<String>>> {
    let specs: Vec<String> = files.iter().map(|file| format!("{rev}:{file}")).collect();
    blobs(main, &specs).ok()
}

#[cfg(test)]
mod tests;
