//! The cap on each orchestrator's open work: its slots. Each orchestrator has a maximum of slots,
//! so that it pushes work to done, as new work cannot start until other tasks truly finish; the
//! cap need not count tasks that are blocked or not started (before in progress).
//!
//! A task is **open** while it is in progress or in review ([`OPEN`]), or blocked under a hold that
//! keeps its slot (`Store::hold_blocked`, a landing waiting on its stamps: [`Entry::held`]); other blocked
//! work and work not started hold no slot, and a slot frees only when its task is done or cancelled
//! (or leaves the open statuses some other way), or moves to review (below). All descendants share
//! their parent feature's slot, charged to that feature's owner, regardless of their instruction
//! flag. Active work under an inactive or finished umbrella still holds that slot; accounting
//! changes no task state.
//!
//! - a review mirror counts with the task it reviews: `review/BRANCH` (`review-N/BRANCH`) with the
//!   task whose slug or recorded executor branch is `BRANCH`, and `SLUG/review` with `SLUG`.
//!
//! Work in review takes no slot ([`Ledger::reviewing`], [`Ledger::reviews`]). The cap counts the
//! tasks being built, not the review of them: a task frees its slot when it moves to review, and
//! the work under it goes with it, a reviewer's mirror or instruction subtask in progress
//! included, which so starts at any count.
//! A refusal names the work in review apart, as not counted. The task's return to in progress, on a
//! changes verdict or a move, goes between open statuses and so starts nothing: never refused, it
//! counts again from then, as it was being worked already.
//!
//! Red work takes no slot ([`Ledger::red`]): an open task filed for a red that stops every landing
//! ([`crate::red`]) and the work under it, its mirrors and instructions included, start at any count
//! and free none, and a refusal names the red work its owner holds apart, as not counted. The
//! work that fixes a red never waits for a slot. A red done or cancelled exempts nothing: the
//! work still open under it, and any started there since, counts as other work does. So its
//! close, never refused, raises its owners' counts: the write holds their slot counters
//! ([`changed`]) and the red's own counter, which a start that took no slot under it holds too
//! ([`reds`]), so no start decided on what it read before the close passes the cap after it.
//!
//! A task no orchestrator holds counts against its executor's parent session (`register
//! --parent`). The cap is the project's (`[limits] orchestrator_slots`, else `[board]`'s, in the main
//! checkout's `symphony.toml`, [`crate::limits`]); a board with none configured caps nothing. It
//! bites where work
//! starts: a change that would raise an orchestrator's open count past the cap is refused, naming
//! the open tasks it holds ([`started`]). It is not retroactive: an orchestrator above the cap
//! keeps every task, and starts nothing new until it is below it.

use crate::Board;
use crate::on_review::branch_of;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;

mod change;
pub use change::{changed, reds, started};

/// The statuses a task holds a slot in.
pub const OPEN: [&str; 2] = ["in_progress", "review"];
/// The open status the cap does not count: built, its work is the reviewer's ([`Ledger::reviews`]).
const REVIEW: &str = "review";
/// How a refusal names the status of blocked work a hold keeps open.
pub const HELD: &str = "blocked, held";
/// The key of the project file naming the cap, in `[limits]` or, where an older build reads it,
/// `[board]` ([`crate::limits`]).
pub const KEY: &str = "orchestrator_slots";

/// One task as the ledger reads it.
///
/// A task a custom workflow holds is counted by its position's slot policy (docs/workflow/WORKFLOW.md,
/// "Operations"): it holds a slot while that policy is `acquire` or `retain` ([`Entry::slot`]),
/// whatever status the position projects to, so a reviewer's position that acquires counts as
/// work, a held block that retains keeps its slot, and a position that releases frees it. That slot
/// is task WIP, the cap on an orchestrator's open work, never an execution attempt's capacity.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    pub slug: &'a str,
    /// The latest nonempty recorded executor branch.
    pub branch: Option<&'a str>,
    pub parent: Option<&'a str>,
    pub status: &'a str,
    /// An instruction subtask, which counts with the task it is part of.
    pub instruction: bool,
    /// Blocked under a hold that keeps its slot (`Store::hold_blocked`): open, as it was before the hold.
    pub held: bool,
    /// The sessions holding the orchestrator role on it that are still there
    /// ([`crate::Health::holds`]).
    pub orchestrators: Vec<&'a str>,
    pub executors: Vec<&'a str>,
    /// Filed for a red ([`crate::red`]): while it is open, it and the work under it take no slot.
    pub red: bool,
    /// For a task a custom workflow holds, whether its position's slot policy holds a slot; none
    /// for any other task, which holds one by its status.
    pub slot: Option<bool>,
}

impl<'a> Entry<'a> {
    /// Whether it holds a slot: by its position's slot policy where a workflow holds it, else in
    /// an open status, or blocked under a hold.
    pub fn open(&self) -> bool {
        self.slot.unwrap_or_else(|| {
            OPEN.contains(&self.status) || (self.held && self.status == "blocked")
        })
    }

    /// Whether it is work in review the cap does not count: a task in review that no workflow
    /// holds. A held task's position counts by its own slot policy, whatever it projects to.
    fn in_review(&self) -> bool {
        self.slot.is_none() && self.status == REVIEW
    }

    /// Its status as a refusal names it.
    fn shown(&self) -> &'a str {
        if self.held && self.status == "blocked" {
            HELD
        } else {
            self.status
        }
    }
}

/// Whose slot a task takes, and which.
#[derive(Debug, Clone, PartialEq)]
pub struct Place<'a> {
    /// The orchestrators it counts against: its own, or, for a task no orchestrator holds, its
    /// executors' parents. Empty when there is no one to count it against.
    pub owners: Vec<&'a str>,
    /// The slot: the task whose work it is ([`Ledger::work`]).
    pub key: &'a str,
    /// Whether the owners are parents standing in for an orchestrator.
    pub by_parent: bool,
}

/// The open tasks of one slot, each with its status, by slug.
pub type Slot<'a> = Vec<(&'a str, &'a str)>;
/// The open slots of one owner, by the task whose work each is.
pub type Held<'a> = BTreeMap<&'a str, Slot<'a>>;

/// The task a branch belongs to: its slug first, then a recorded executor branch. If legacy
/// records name multiple tasks, prefer a top-level feature, then choose by slug. A mirror's own
/// executor launch never makes it the primary task of the branch it reviews.
pub fn branch_task<'a>(
    branch: &str,
    tasks: impl IntoIterator<Item = (&'a str, Option<&'a str>, Option<&'a str>)>,
) -> Option<&'a str> {
    let tasks: Vec<_> = tasks.into_iter().collect();
    let mut found = None;
    for &(slug, recorded, parent) in &tasks {
        if slug == branch {
            return Some(slug);
        }
        let mirror = branch_of(slug) != slug
            || slug
                .strip_suffix("/review")
                .is_some_and(|base| tasks.iter().any(|task| task.0 == base));
        let candidate = (parent.is_some(), slug);
        if !mirror && recorded == Some(branch) && found.is_none_or(|previous| candidate < previous)
        {
            found = Some(candidate);
        }
    }
    found.map(|(_, slug)| slug)
}

/// The tasks a ledger of `tasks` (each slug with its parent) needs read as well, to place them: each
/// one's parent, and the task its slug names as the one it reviews, which the board may not hold.
pub fn wanted<'a>(
    tasks: impl IntoIterator<Item = (&'a str, Option<&'a str>)>,
) -> BTreeSet<&'a str> {
    let mut wanted = BTreeSet::new();
    for (slug, parent) in tasks {
        wanted.extend(parent);
        let branch = branch_of(slug);
        if branch != slug {
            wanted.insert(branch);
        }
        wanted.extend(slug.strip_suffix("/review"));
    }
    wanted
}

/// The board as the cap reads it: the open tasks, the tasks above them and the tasks they review,
/// each with its status, orchestrators and executors, and each session's parent.
#[derive(Debug, Clone, Default)]
pub struct Ledger<'a> {
    tasks: HashMap<&'a str, Entry<'a>>,
    parents: HashMap<&'a str, &'a str>,
}

impl<'a> Ledger<'a> {
    /// The ledger of `tasks`, with `parents` naming each session's parent.
    pub fn new(
        tasks: impl IntoIterator<Item = Entry<'a>>,
        parents: impl IntoIterator<Item = (&'a str, &'a str)>,
    ) -> Ledger<'a> {
        Ledger {
            tasks: tasks.into_iter().map(|t| (t.slug, t)).collect(),
            parents: parents.into_iter().collect(),
        }
    }

    /// The ledger of a whole board as one read gave it, which reads no hold: blocked work a hold
    /// keeps open is not counted. Use [`crate::Store::slot_holdings`] for the canonical counted
    /// feature keys including valid holds and the admission view's presence handling. A task a
    /// custom workflow holds is counted by its position's slot policy on `operating`, when given
    /// ([`Ledger::of_board_on`]); here, by the status it projects to.
    pub fn of_board(board: &'a Board) -> Ledger<'a> {
        Ledger::of_board_on(board, None)
    }

    /// [`Ledger::of_board`], each task a custom workflow holds counted by its position's slot
    /// policy on `operating` ([`Entry::slot`]).
    pub fn of_board_on(
        board: &'a Board,
        operating: Option<&crate::workflow::Operating>,
    ) -> Ledger<'a> {
        let tasks = board.tasks.iter().map(|t| Entry {
            slug: &t.slug,
            branch: t.branch.as_deref(),
            parent: t.parent.as_deref(),
            status: &t.status,
            instruction: t.instruction,
            held: false,
            orchestrators: (t.roles.iter())
                .filter(|r| r.role == "orchestrator" && r.health.holds())
                .map(|r| r.handle.as_str())
                .collect(),
            executors: (t.roles.iter())
                .filter(|r| r.role == "executor")
                .map(|r| r.handle.as_str())
                .collect(),
            red: t.red(),
            slot: operating.and_then(|o| o.holds_slot(t)),
        });
        let parents = (board.sessions.iter())
            .filter_map(|s| Some((s.row.handle.as_str(), s.row.parent.as_deref()?)));
        Ledger::new(tasks, parents)
    }

    /// The ledger once `slug` is in `status` (when given: a move ends a hold), `orchestrator` alone
    /// holds the role on it (when given) and `executors` execute it as well as those that do.
    pub fn changed(
        &self,
        slug: &str,
        status: Option<&'a str>,
        orchestrator: Option<&'a str>,
        executors: &[&'a str],
    ) -> Ledger<'a> {
        let mut after = self.clone();
        if let Some(task) = after.tasks.get_mut(slug) {
            if let Some(status) = status {
                task.status = status;
                task.held = false;
            }
            if let Some(orchestrator) = orchestrator {
                task.orchestrators = vec![orchestrator];
            }
            for executor in executors {
                if !task.executors.contains(executor) {
                    task.executors.push(executor);
                }
            }
        }
        after
    }

    /// The ledger once `slug`, a task a custom workflow holds, stands at a position whose slot
    /// policy holds a slot (`holds`) or releases it ([`Entry::slot`]).
    pub fn slotted(&self, slug: &str, holds: bool) -> Ledger<'a> {
        let mut after = self.clone();
        if let Some(task) = after.tasks.get_mut(slug) {
            task.slot = Some(holds);
        }
        after
    }

    /// The ledger once `slug` is under `parent`, or at the top with none; the tasks below it move
    /// with it. A hold lasts no change of parent.
    pub fn reparented(&self, slug: &str, parent: Option<&'a str>) -> Ledger<'a> {
        let mut after = self.clone();
        if let Some(task) = after.tasks.get_mut(slug) {
            task.parent = parent;
            task.held = false;
        }
        after
    }

    /// The task a review mirror reviews, by slug or recorded branch, using [`branch_task`].
    /// `SLUG/review` reviews `SLUG`. Any other task is its own.
    pub fn reviewed(&self, slug: &'a str) -> &'a str {
        let mut at = slug;
        let mut seen = HashSet::new();
        while seen.insert(at) {
            let branch = branch_of(at);
            let next = if branch != at {
                branch_task(
                    branch,
                    self.tasks.values().map(|t| (t.slug, t.branch, t.parent)),
                )
            } else {
                at.strip_suffix("/review")
                    .filter(|base| self.tasks.contains_key(base))
            };
            match next {
                Some(next) if seen.contains(next) => return slug,
                Some(next) => at = next,
                None => break,
            }
        }
        at
    }

    /// The task whose work `slug` is, the key of its slot: the task a mirror reviews, and the task a
    /// subtask is part of, at every depth. Missing parents stop the walk. A cycle falls back to the
    /// reviewed task rather than inventing a feature root.
    pub fn work(&self, slug: &'a str) -> &'a str {
        let mut at = self.reviewed(slug);
        let mut seen = HashSet::new();
        while seen.insert(at) {
            let parent = (self.tasks.get(at))
                .and_then(|t| t.parent)
                .filter(|p| self.tasks.contains_key(p));
            match parent {
                Some(parent) => {
                    let next = self.reviewed(parent);
                    if seen.contains(next) {
                        return self.reviewed(slug);
                    }
                    at = next;
                }
                None => break,
            }
        }
        at
    }

    /// The reviewed task and its reviewed ancestors, as far as the ledger holds them, once round
    /// a loop at most. A repair below a review mirror follows that mirror's actual task too.
    fn chain(&self, slug: &'a str) -> Vec<&'a str> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        let mut at = self.tasks.get(self.reviewed(slug));
        while let Some(task) = at.filter(|t| seen.insert(t.slug)) {
            found.push(task.slug);
            at = task.parent.and_then(|p| self.tasks.get(self.reviewed(p)));
        }
        found
    }

    /// Whose slot `slug` takes and which ([`Place`]), its executors' parents standing in for an
    /// orchestrator when none holds it.
    pub fn place(&self, slug: &'a str) -> Place<'a> {
        let key = self.work(slug);
        let executors = self
            .tasks
            .get(key)
            .filter(|t| !t.executors.is_empty())
            .or_else(|| self.tasks.get(slug))
            .map_or(&[][..], |t| &t.executors[..]);
        self.place_by(slug, executors)
    }

    /// As [`Ledger::place`], `sessions` standing in for its executors: whose parents take the slot
    /// when no orchestrator holds the task. A mirror is its reviewed task's orchestrator's.
    pub fn place_by(&self, slug: &'a str, sessions: &[&'a str]) -> Place<'a> {
        let key = self.work(slug);
        let parent_owners = |sessions: &[&'a str]| -> Vec<&'a str> {
            sessions
                .iter()
                .filter_map(|s| self.parents.get(s).copied())
                .collect()
        };
        let root = self.tasks.get(key);
        let root_parents = root.map_or_else(Vec::new, |t| parent_owners(&t.executors));
        let (mut owners, by_parent) =
            if let Some(root) = root.filter(|t| !t.orchestrators.is_empty()) {
                (root.orchestrators.clone(), false)
            } else if !root_parents.is_empty() {
                (root_parents, true)
            } else {
                // Legacy inactive umbrellas can lack an owner. Keep their active children's work
                // visible under the nearest remaining holder until ancestry is normalized.
                let nearest = self
                    .chain(self.reviewed(slug))
                    .into_iter()
                    .map(|at| &self.tasks[at].orchestrators)
                    .find(|held| !held.is_empty());
                nearest.map_or_else(
                    || (parent_owners(sessions), true),
                    |held| (held.clone(), false),
                )
            };
        owners.sort_unstable();
        owners.dedup();
        Place {
            owners,
            key,
            by_parent,
        }
    }

    /// Whether `slug` is red work: its reviewed task or an ancestor is an open red. Red repair
    /// children share the feature key but exempt neither their ordinary parent nor siblings.
    /// A red done or cancelled exempts nothing under it.
    pub fn red(&self, slug: &'a str) -> bool {
        self.exempting(slug).next().is_some()
    }

    /// Whether `slug` is work in review: it, the task it reviews or a task above them is in review.
    /// So a reviewer's mirror or instruction subtask under a task in review is that task's review,
    /// whatever its own status, as red work is judged by the red above it.
    fn reviewing(&self, slug: &'a str) -> bool {
        let in_review = |at: &str| self.tasks.get(at).is_some_and(Entry::in_review);
        in_review(slug) || self.chain(slug).into_iter().any(in_review)
    }

    /// Every owner's open slots, in one pass over the open tasks: those the cap counts, then the
    /// red work and the work in review it does not.
    fn tally(&self) -> [HashMap<&'a str, Held<'a>>; 3] {
        let mut tally: [HashMap<&'a str, Held<'a>>; 3] = Default::default();
        let open = self.tasks.values().filter(|t| t.open());
        for task in open {
            let place = self.place(task.slug);
            let apart = if self.red(task.slug) {
                1
            } else if self.reviewing(task.slug) {
                2
            } else {
                0
            };
            let all = &mut tally[apart];
            for owner in place.owners {
                let slot = all.entry(owner).or_default().entry(place.key).or_default();
                slot.push((task.slug, task.shown()));
            }
        }
        let slots = (tally.iter_mut())
            .flat_map(HashMap::values_mut)
            .flat_map(BTreeMap::values_mut);
        for slot in slots {
            slot.sort_unstable();
        }
        tally
    }

    /// Every owner's open slots the cap counts: red work and work in review are not among them
    /// ([`Ledger::reds`], [`Ledger::reviews`]).
    pub fn holdings(&self) -> HashMap<&'a str, Held<'a>> {
        let [counted, ..] = self.tally();
        counted
    }

    /// Every owner's open red work ([`Ledger::red`]), which the cap does not count, by the task
    /// whose work each is.
    pub fn reds(&self) -> HashMap<&'a str, Held<'a>> {
        let [_, red, _] = self.tally();
        red
    }

    /// Every owner's work in review that is no red work, which the cap does not count, by the task
    /// whose work each is.
    pub fn reviews(&self) -> HashMap<&'a str, Held<'a>> {
        let [.., review] = self.tally();
        review
    }

    /// The open slots `handle` holds: each slot's key, and its open tasks with their status.
    pub fn held(&self, handle: &str) -> Held<'a> {
        self.holdings().remove(handle).unwrap_or_default()
    }

    /// The number of open slots each session holds, for every session holding one.
    pub fn counts(&self) -> HashMap<&'a str, usize> {
        (self.holdings().into_iter())
            .map(|(owner, held)| (owner, held.len()))
            .collect()
    }
}

/// An owner a start would carry past the cap, and the open slots it holds before it.
#[derive(Debug, Clone, PartialEq)]
pub struct Full<'a> {
    pub owner: &'a str,
    pub held: Held<'a>,
    /// The open red work it holds before it, which the cap does not count ([`Ledger::reds`]).
    pub red: Held<'a>,
    /// The work in review it holds before it, which the cap does not count ([`Ledger::reviews`]).
    pub review: Held<'a>,
}

/// An owner's open slots as a refusal names them, one entry a slot: the task whose work it is
/// first, then any mirror or instruction sharing it, `with` it, each with its status.
pub fn named(held: &Held<'_>) -> String {
    let mut text = String::new();
    for (at, (key, slot)) in held.iter().enumerate() {
        if at > 0 {
            text.push_str(", ");
        }
        let first = slot.iter().position(|(slug, _)| slug == key).unwrap_or(0);
        let lead = slot.get(first).into_iter();
        let rest = (slot.iter().enumerate())
            .filter(|(i, _)| *i != first)
            .map(|(_, task)| task);
        for (i, (slug, status)) in lead.chain(rest).enumerate() {
            if i > 0 {
                text.push_str(" with ");
            }
            let _ = write!(text, "{slug} ({status})");
        }
    }
    text
}

/// What a claim refused at the cap adds, so an agent that never read the work skill still knows:
/// the executor holding the reservation works under it, renewing it, and retries the claim.
/// Other starts (a move, a role) add nothing.
pub(crate) fn claim_hint(slug: &str, what: &str) -> String {
    if what != format!("{slug} cannot start") {
        return String::new();
    }
    format!(
        "; if the task is reserved for you, start work under the reservation now, renew it with `just tasks reserve {slug}` within each hour and retry the claim after each renewal"
    )
}

/// What a start refused at the cap says: `what` cannot happen, because `full`'s owner holds `cap`
/// open slots or more, as many as it names. When it stands in for an orchestrator as the parent of
/// the sessions starting a task no orchestrator holds, those `children` are named. Its red work and
/// its work in review are named apart, last, as not counted.
pub fn refusal(what: &str, full: &Full<'_>, children: &[&str], cap: usize) -> String {
    let owner = full.owner;
    let standing = if children.is_empty() {
        String::new()
    } else {
        format!(
            " (no orchestrator holds it: {owner} is the parent of {})",
            children.join(", ")
        )
    };
    let apart = |held: &Held<'_>, why: &str| {
        if held.is_empty() {
            String::new()
        } else {
            format!("; not counted, as {why} takes no slot: {}", named(held))
        }
    };
    let red = apart(&full.red, "red work");
    let review = apart(&full.review, "work in review");
    format!(
        "{what}: {owner}{standing} holds {} open tasks, the cap of {cap} an orchestrator holds: {}; a slot frees when one of them goes to review or is done or cancelled{red}{review}",
        full.held.len(),
        named(&full.held)
    )
}

#[cfg(test)]
mod tests;
