//! The cap on each orchestrator's open work: its slots. The owner, 2026-11-07: "a given maximum
//! slot per orchestrator, so that orchestrators are incentivized to push work to done (as new work
//! won't be able to start until other tasks truly finish)", and "this max slot can disconsider
//! tasks that are blocked and not started (before in progress)".
//!
//! A task is **open** while it is in progress or in review ([`OPEN`]), or blocked under a hold that
//! keeps its slot (`Store::hold_blocked`, a landing waiting on its stamps: [`Entry::held`]); other blocked
//! work and work not started hold no slot, and a slot frees only when its task is done or cancelled
//! (or leaves the open statuses some other way). Each open task holds a slot of its own, of its
//! orchestrator's:
//! the session holding the role on it, or on the nearest task above it that has a holder still
//! there, as [`crate::orchestration`] reads whose a task is. Two kinds of task are no work of their
//! own and share the slot of the task they serve ([`Ledger::work`]):
//!
//! - a review mirror counts with the task it reviews: `review/BRANCH` (`review-N/BRANCH`) with the
//!   task `BRANCH`, and `SLUG/review` with `SLUG`, when the board holds that task;
//! - an instruction subtask (`--instruction`, an orchestrator's word to an agent, which never goes
//!   to review or lands) counts with the task it is an instruction for, its nearest ancestor that
//!   is not one.
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
//! --parent`). The cap is the project's (`[board] orchestrator_slots` in the main checkout's
//! `symphony.toml`, [`of_env`]); a board with none configured caps nothing. It bites where work
//! starts: a change that would raise an orchestrator's open count past the cap is refused, naming
//! the open tasks it holds ([`started`]). It is not retroactive: an orchestrator above the cap
//! keeps every task, and starts nothing new until it is below it.

use crate::Board;
use crate::cli::Env;
use crate::on_review::{MANIFEST, branch_of};
use crate::sweep::trigger::main_checkout;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

/// The statuses a task holds a slot in.
pub const OPEN: [&str; 2] = ["in_progress", "review"];
/// How a refusal names the status of blocked work a hold keeps open.
pub const HELD: &str = "blocked, held";
/// The `[board]` key of the project file naming the cap.
pub const KEY: &str = "orchestrator_slots";

#[derive(Default, Deserialize)]
struct Manifest {
    #[serde(default)]
    board: ManifestBoard,
}

#[derive(Default, Deserialize)]
struct ManifestBoard {
    orchestrator_slots: Option<i64>,
}

/// The cap `[board] orchestrator_slots` names in the manifest at `root`: none when the file, the
/// table or the key is missing. A count below one, or a file that does not parse, is an error naming
/// the file. Only that key is read; the rest of the file is the Symphony's.
pub fn configured(root: &Path) -> Result<Option<usize>, String> {
    let path = root.join(MANIFEST);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("cannot read {}: {error}", path.display())),
    };
    let manifest: Manifest = toml::from_str(&text)
        .map_err(|error| format!("{}: [board] {KEY}: {error}", path.display()))?;
    match manifest.board.orchestrator_slots {
        None => Ok(None),
        Some(cap) if cap >= 1 => Ok(usize::try_from(cap).ok()),
        Some(cap) => Err(format!(
            "{}: [board] {KEY} is {cap}; an orchestrator needs at least one slot",
            path.display()
        )),
    }
}

/// The cap of the board `env` runs against: the one the main checkout of the repository holding its
/// working directory configures ([`configured`]), as `[board] on_review` is read there; none outside
/// a repository. The repository is found from its `.git` files alone, with no `git` process, since
/// every command reads it.
pub fn of_env(env: &Env) -> Result<Option<usize>, String> {
    let Some(common) = crate::git::read(&env.cwd) else {
        return Ok(None);
    };
    match main_checkout(&common) {
        Some(root) => configured(root),
        None => Ok(None),
    }
}

/// One task as the ledger reads it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry<'a> {
    pub slug: &'a str,
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
}

impl<'a> Entry<'a> {
    /// Whether it holds a slot: in an open status, or blocked under a hold.
    pub fn open(&self) -> bool {
        OPEN.contains(&self.status) || (self.held && self.status == "blocked")
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
    /// keeps open is not counted.
    pub fn of_board(board: &'a Board) -> Ledger<'a> {
        let tasks = board.tasks.iter().map(|t| Entry {
            slug: &t.slug,
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

    /// The task a review mirror reviews, when the board holds it: `review/BRANCH` (`review-N/BRANCH`)
    /// reviews `BRANCH`, `SLUG/review` reviews `SLUG`. Any other task is its own.
    pub fn reviewed(&self, slug: &'a str) -> &'a str {
        let mut at = slug;
        for _ in 0..4 {
            let branch = branch_of(at);
            let next = if branch != at && self.tasks.contains_key(branch) {
                branch
            } else {
                match at.strip_suffix("/review") {
                    Some(base) if self.tasks.contains_key(base) => base,
                    _ => break,
                }
            };
            at = next;
        }
        at
    }

    /// The task whose work `slug` is, the key of its slot: the task a mirror reviews, and the task an
    /// instruction subtask is part of, its nearest ancestor that is no instruction; any other task
    /// is its own work.
    pub fn work(&self, slug: &'a str) -> &'a str {
        let mut at = self.reviewed(slug);
        let mut seen = HashSet::new();
        while seen.insert(at) {
            let parent = (self.tasks.get(at))
                .filter(|t| t.instruction)
                .and_then(|t| t.parent)
                .filter(|p| self.tasks.contains_key(p));
            match parent {
                Some(parent) => at = self.reviewed(parent),
                None => break,
            }
        }
        at
    }

    /// The task, its parent, and so on as far as the ledger holds them, once round a loop at most.
    fn chain(&self, slug: &'a str) -> Vec<&'a str> {
        let mut seen = HashSet::new();
        let mut found = Vec::new();
        let mut at = self.tasks.get(slug);
        while let Some(task) = at.filter(|t| seen.insert(t.slug)) {
            found.push(task.slug);
            at = task.parent.and_then(|p| self.tasks.get(p));
        }
        found
    }

    /// Whose slot `slug` takes and which ([`Place`]), its executors' parents standing in for an
    /// orchestrator when none holds it.
    pub fn place(&self, slug: &'a str) -> Place<'a> {
        let executors = self.tasks.get(slug).map_or(&[][..], |t| &t.executors[..]);
        self.place_by(slug, executors)
    }

    /// As [`Ledger::place`], `sessions` standing in for its executors: whose parents take the slot
    /// when no orchestrator holds the task. A mirror is its reviewed task's orchestrator's.
    pub fn place_by(&self, slug: &'a str, sessions: &[&'a str]) -> Place<'a> {
        let key = self.work(slug);
        let nearest = (self.chain(self.reviewed(slug)).into_iter())
            .map(|at| &self.tasks[at].orchestrators)
            .find(|held| !held.is_empty());
        let (mut owners, by_parent) = match nearest {
            Some(held) => (held.clone(), false),
            None => (
                (sessions.iter())
                    .filter_map(|s| self.parents.get(s).copied())
                    .collect(),
                true,
            ),
        };
        owners.sort_unstable();
        owners.dedup();
        Place {
            owners,
            key,
            by_parent,
        }
    }

    /// Whether `slug` is red work, which takes no slot: the task whose work it is
    /// ([`Ledger::work`]), or a task above that one, is an open red ([`crate::red`]). So a mirror
    /// and an instruction are red work with the task they serve, a slot is all red work or none,
    /// and a red done or cancelled exempts nothing under it.
    pub fn red(&self, slug: &'a str) -> bool {
        self.exempting(slug).next().is_some()
    }

    /// The open reds that make `slug` red work ([`Ledger::red`]), nearest first.
    fn exempting(&self, slug: &'a str) -> impl Iterator<Item = &'a str> + '_ {
        let chain = self.chain(self.work(slug)).into_iter();
        chain.filter(|at| self.tasks[at].red && crate::red::open(self.tasks[at].status))
    }

    /// Every owner's open slots, in one pass over the open tasks: those the cap counts, then the
    /// red work it does not.
    fn tally(&self) -> [HashMap<&'a str, Held<'a>>; 2] {
        let mut tally: [HashMap<&'a str, Held<'a>>; 2] = Default::default();
        let open = self.tasks.values().filter(|t| t.open());
        for task in open {
            let place = self.place(task.slug);
            let all = &mut tally[usize::from(self.red(task.slug))];
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

    /// Every owner's open slots the cap counts: red work is not among them ([`Ledger::reds`]).
    pub fn holdings(&self) -> HashMap<&'a str, Held<'a>> {
        let [counted, _] = self.tally();
        counted
    }

    /// Every owner's open red work ([`Ledger::red`]), which the cap does not count, by the task
    /// whose work each is.
    pub fn reds(&self) -> HashMap<&'a str, Held<'a>> {
        let [_, red] = self.tally();
        red
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
}

/// What a change from `before` to `after` does under `cap`: the owners whose counted open slots it
/// changes, each of which the write holds the slot counter of, by handle. Refused, naming the first
/// such owner and what it holds before, when it raises an owner's open slots past `cap`: one at the
/// cap, or above it, starts nothing new. Red work counts in neither, so it starts at any count.
pub fn started<'a>(
    before: &Ledger<'a>,
    after: &Ledger<'a>,
    cap: usize,
) -> Result<Vec<&'a str>, Full<'a>> {
    let (mut was, now) = (before.holdings(), after.holdings());
    let changed = differing(&was, &now);
    for &owner in &changed {
        let (held, next) = (was.get(owner), now.get(owner));
        let (held, next) = (held.map_or(0, BTreeMap::len), next.map_or(0, BTreeMap::len));
        if next > held && next > cap {
            let held = was.remove(owner).unwrap_or_default();
            let red = before.reds().remove(owner).unwrap_or_default();
            return Err(Full { owner, held, red });
        }
    }
    Ok(changed)
}

/// The owners whose counted open slots a change from `before` to `after` changes, by handle, at
/// any count: what [`started`] holds, for a change the cap never refuses, a red's close. Closing an
/// open red raises its owners' counts by the open work under it, which counts from then on, so
/// the close holds their slot counters, and a start decided on a count it read before the close is
/// decided again.
pub fn changed<'a>(before: &Ledger<'a>, after: &Ledger<'a>) -> Vec<&'a str> {
    differing(&before.holdings(), &after.holdings())
}

/// The owners whose open slots differ between `was` and `now`, in order.
fn differing<'a>(
    was: &HashMap<&'a str, Held<'a>>,
    now: &HashMap<&'a str, Held<'a>>,
) -> Vec<&'a str> {
    let mut owners: Vec<&'a str> = was.keys().chain(now.keys()).copied().collect();
    owners.sort_unstable();
    owners.dedup();
    owners.retain(|owner| was.get(owner) != now.get(owner));
    owners
}

/// The reds a change from `before` to `after` relies on or ends, in order: a red whose open status
/// it changes (its close), and each open red that, before or after it, makes red work of an open
/// task whose record, place or red work it changes. The write holds each one's counter
/// (`counter:['red', SLUG]`), so a red's close and a start that took no slot under it meet there,
/// as two starts under one orchestrator meet at its slot counter: the one that loses reads and
/// decides again, the start then counted.
pub fn reds<'a>(before: &Ledger<'a>, after: &Ledger<'a>) -> Vec<&'a str> {
    let mut reds = BTreeSet::new();
    for (&slug, now) in &after.tasks {
        let was = before.tasks.get(slug);
        let open = crate::red::open;
        if now.red && was.is_some_and(|was| open(was.status) != open(now.status)) {
            reds.insert(slug);
        }
        let same = was == Some(now)
            && before.place(slug) == after.place(slug)
            && before.red(slug) == after.red(slug);
        if !same && (now.open() || was.is_some_and(Entry::open)) {
            reds.extend(before.exempting(slug).chain(after.exempting(slug)));
        }
    }
    reds.into_iter().collect()
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

/// What a start refused at the cap says: `what` cannot happen, because `full`'s owner holds `cap`
/// open slots or more, as many as it names. When it stands in for an orchestrator as the parent of
/// the sessions starting a task no orchestrator holds, those `children` are named. Its red work is
/// named apart, last, as not counted.
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
    let red = if full.red.is_empty() {
        String::new()
    } else {
        format!(
            "; not counted, as red work takes no slot: {}",
            named(&full.red)
        )
    };
    format!(
        "{what}: {owner}{standing} holds {} open tasks, the cap of {cap} an orchestrator holds: {}; a slot frees when one of them is done or cancelled{red}",
        full.held.len(),
        named(&full.held)
    )
}

#[cfg(test)]
mod tests;
