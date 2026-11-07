//! The cap on each orchestrator's open work: its slots. The owner, 2026-11-07: "a given maximum
//! slot per orchestrator, so that orchestrators are incentivized to push work to done (as new work
//! won't be able to start until other tasks truly finish)", and "this max slot can disconsider
//! tasks that are blocked and not started (before in progress)".
//!
//! A task is **open** while it is in progress or in review ([`OPEN`]); blocked work and work not
//! started hold no slot, and a slot frees only when its task is done or cancelled (or leaves the
//! open statuses some other way). An open task holds a slot of its orchestrator's: the session
//! holding the role on it, or on the nearest task above it that has a holder still there, as
//! [`crate::orchestration`] reads whose a task is. One piece of work holds one slot however it is
//! split:
//!
//! - a review mirror counts with the task it reviews: `review/BRANCH` (`review-N/BRANCH`) with the
//!   task `BRANCH`, and `SLUG/review` with `SLUG`, when the board holds that task;
//! - a subtask counts with the task above it that its orchestrator holds the role on, as far up
//!   as the same orchestrator holds the work, so a task and its subtasks open at once take one slot
//!   ([`Place::key`]).
//!
//! A task no orchestrator holds counts against its executor's parent session (`register
//! --parent`). The cap is the project's (`[board] orchestrator_slots` in the main checkout's
//! `symphony.toml`, [`of_env`]); a board with none configured caps nothing. It bites where work
//! starts ([`crate::Store::claim`], a move into in_progress from ready or blocked, and taking the
//! orchestrator role on an open task): the start is refused when it would open a new slot for an
//! orchestrator already holding the cap, naming the open tasks it holds. It is not retroactive: an
//! orchestrator above the cap keeps every task, and starts nothing new until it is below it.

use crate::Board;
use crate::cli::Env;
use crate::on_review::{MANIFEST, branch_of};
use crate::sweep::trigger::main_checkout;
use serde::Deserialize;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;
use std::path::Path;

/// The statuses a task holds a slot in.
pub const OPEN: [&str; 2] = ["in_progress", "review"];
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
    /// The sessions holding the orchestrator role on it that are still there
    /// ([`crate::Health::holds`]).
    pub orchestrators: Vec<&'a str>,
    pub executors: Vec<&'a str>,
}

/// Whose slot a task takes, and which.
#[derive(Debug, Clone, PartialEq)]
pub struct Place<'a> {
    /// The orchestrators it counts against: its own, or, for a task no orchestrator holds, its
    /// executors' parents. Empty when there is no one to count it against.
    pub owners: Vec<&'a str>,
    /// The slot: the task the work is held under, the highest at or above it (or above the task it
    /// reviews) that its orchestrator holds.
    pub key: &'a str,
    /// Whether the owners are parents standing in for an orchestrator.
    pub by_parent: bool,
}

/// The open tasks of one slot, each with its status, by slug.
pub type Slot<'a> = Vec<(&'a str, &'a str)>;

/// The board as the cap reads it: every task's place in the tree, its status, orchestrators and
/// executors, and each session's parent.
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

    /// The ledger of a whole board as one read gave it.
    pub fn of_board(board: &'a Board) -> Ledger<'a> {
        let tasks = board.tasks.iter().map(|t| Entry {
            slug: &t.slug,
            parent: t.parent.as_deref(),
            status: &t.status,
            orchestrators: (t.roles.iter())
                .filter(|r| r.role == "orchestrator" && r.health.holds())
                .map(|r| r.handle.as_str())
                .collect(),
            executors: (t.roles.iter())
                .filter(|r| r.role == "executor")
                .map(|r| r.handle.as_str())
                .collect(),
        });
        let parents = (board.sessions.iter())
            .filter_map(|s| Some((s.row.handle.as_str(), s.row.parent.as_deref()?)));
        Ledger::new(tasks, parents)
    }

    /// The ledger as it would be once `handle` alone held the orchestrator role on `slug`.
    pub fn orchestrated_by(&self, slug: &str, handle: &'a str) -> Ledger<'a> {
        let mut after = self.clone();
        if let Some(task) = after.tasks.get_mut(slug) {
            task.orchestrators = vec![handle];
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

    /// The task, its parent, and so on as far as the board holds them, once round a loop at most.
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
    /// when no orchestrator holds the task.
    pub fn place_by(&self, slug: &'a str, sessions: &[&'a str]) -> Place<'a> {
        let chain = self.chain(self.reviewed(slug));
        // The holders the board reads each task of the chain as orchestrated by: its own, else the
        // nearest above it.
        let mut nearest: Vec<Vec<&'a str>> = vec![Vec::new(); chain.len()];
        let mut above: Vec<&'a str> = Vec::new();
        for (at, slug) in chain.iter().enumerate().rev() {
            let own = &self.tasks[slug].orchestrators;
            if !own.is_empty() {
                above = own.clone();
                above.sort_unstable();
                above.dedup();
            }
            nearest[at].clone_from(&above);
        }
        let Some(first) = nearest.first().filter(|held| !held.is_empty()) else {
            let mut owners: Vec<&'a str> = (sessions.iter())
                .filter_map(|s| self.parents.get(s).copied())
                .collect();
            owners.sort_unstable();
            owners.dedup();
            return Place {
                owners,
                key: chain.last().copied().unwrap_or(slug),
                by_parent: true,
            };
        };
        let reach = nearest.iter().take_while(|held| *held == first).count();
        Place {
            owners: first.clone(),
            key: chain[reach - 1],
            by_parent: false,
        }
    }

    /// The open slots `handle` holds: each slot's key, and its open tasks with their status.
    pub fn held(&self, handle: &str) -> BTreeMap<&'a str, Slot<'a>> {
        let mut held: BTreeMap<&'a str, Slot<'a>> = BTreeMap::new();
        for task in self.tasks.values().filter(|t| OPEN.contains(&t.status)) {
            let place = self.place(task.slug);
            if place.owners.contains(&handle) {
                held.entry(place.key)
                    .or_default()
                    .push((task.slug, task.status));
            }
        }
        for slot in held.values_mut() {
            slot.sort_unstable();
        }
        held
    }

    /// The number of open slots each session holds, for every session holding one: one pass over
    /// the open tasks, where [`Ledger::held`] is one session's.
    pub fn counts(&self) -> HashMap<&'a str, usize> {
        let mut slots: HashMap<&'a str, HashSet<&'a str>> = HashMap::new();
        for task in self.tasks.values().filter(|t| OPEN.contains(&t.status)) {
            let place = self.place(task.slug);
            for owner in place.owners {
                slots.entry(owner).or_default().insert(place.key);
            }
        }
        slots
            .into_iter()
            .map(|(owner, keys)| (owner, keys.len()))
            .collect()
    }

    /// What taking `place`'s slot does under `cap`: the owners it opens a new slot for (none for an
    /// owner already holding that slot open), or the first owner it would carry past the cap, one
    /// already holding `cap` open slots or more, with the slots it holds.
    pub fn take(&self, place: &Place<'a>, cap: usize) -> Result<Vec<&'a str>, Full<'a>> {
        let mut opened = Vec::new();
        for &owner in &place.owners {
            let held = self.held(owner);
            if held.contains_key(place.key) {
                continue;
            }
            if held.len() >= cap {
                return Err(Full { owner, held });
            }
            opened.push(owner);
        }
        Ok(opened)
    }
}

/// An owner a start would carry past the cap, and the open slots it holds.
#[derive(Debug, Clone, PartialEq)]
pub struct Full<'a> {
    pub owner: &'a str,
    pub held: BTreeMap<&'a str, Slot<'a>>,
}

/// `owner`'s open tasks as a refusal names them: each slot's tasks with their status, a slot's
/// several tasks joined by `+`.
pub fn named(held: &BTreeMap<&str, Slot<'_>>) -> String {
    let mut text = String::new();
    for (at, slot) in held.values().enumerate() {
        if at > 0 {
            text.push_str(", ");
        }
        for (i, (slug, status)) in slot.iter().enumerate() {
            if i > 0 {
                text.push_str(" + ");
            }
            let _ = write!(text, "{slug} ({status})");
        }
    }
    text
}

/// What a start refused at the cap says: `what` cannot happen, because `full`'s owner holds `cap`
/// open slots or more. When it stands in for an orchestrator as the parent of the sessions starting
/// a task no orchestrator holds, those `children` are named.
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
    format!(
        "{what}: {owner}{standing} holds {} open tasks, the cap of {cap} an orchestrator holds: {}; a slot frees when one of them is done or cancelled",
        full.held.len(),
        named(&full.held)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry<'a>(slug: &'a str, parent: Option<&'a str>, status: &'a str) -> Entry<'a> {
        Entry {
            slug,
            parent,
            status,
            orchestrators: Vec::new(),
            executors: Vec::new(),
        }
    }

    fn led<'a>(slug: &'a str, parent: Option<&'a str>, status: &'a str, by: &'a str) -> Entry<'a> {
        Entry {
            orchestrators: vec![by],
            ..entry(slug, parent, status)
        }
    }

    #[test]
    fn a_mirror_counts_with_the_task_it_reviews_when_the_board_holds_it() {
        let ledger = Ledger::new(
            [
                entry("sim/jump", None, "review"),
                entry("review/sim/jump", None, "review"),
                entry("review-2/sim/jump", None, "review"),
                entry("sim/jump/review", Some("sim/jump"), "review"),
                entry("review/gone", None, "review"),
                entry("docs/x/review", None, "review"),
            ],
            [],
        );
        for (slug, reviewed) in [
            ("sim/jump", "sim/jump"),
            ("review/sim/jump", "sim/jump"),
            ("review-2/sim/jump", "sim/jump"),
            ("sim/jump/review", "sim/jump"),
            ("review/gone", "review/gone"),
            ("docs/x/review", "docs/x/review"),
        ] {
            assert_eq!(ledger.reviewed(slug), reviewed, "{slug}");
        }
    }

    #[test]
    fn a_task_counts_against_its_nearest_orchestrator_under_the_highest_task_it_holds() {
        let ledger = Ledger::new(
            [
                led("a", None, "ready", "o"),
                entry("a/b", Some("a"), "in_progress"),
                led("a/b/c", Some("a/b"), "in_progress", "o"),
                led("a/d", Some("a"), "in_progress", "p"),
                entry("a/d/e", Some("a/d"), "review"),
                entry("loose", None, "in_progress"),
                led("loose/x", Some("loose"), "in_progress", "q"),
            ],
            [],
        );
        for (slug, owners, key) in [
            ("a", vec!["o"], "a"),
            ("a/b", vec!["o"], "a"),
            ("a/b/c", vec!["o"], "a"),
            ("a/d", vec!["p"], "a/d"),
            ("a/d/e", vec!["p"], "a/d"),
            ("loose/x", vec!["q"], "loose/x"),
        ] {
            let place = ledger.place(slug);
            assert_eq!(
                (place.owners, place.key, place.by_parent),
                (owners, key, false),
                "{slug}"
            );
        }
    }

    #[test]
    fn a_task_nobody_orchestrates_counts_against_its_executors_parents() {
        let mut loose = entry("loose", None, "in_progress");
        loose.executors = vec!["w1", "w2"];
        let mut orphan = entry("loose/sub", Some("loose"), "in_progress");
        orphan.executors = vec!["w3"];
        let ledger = Ledger::new(
            [loose, orphan, entry("idle", None, "in_progress")],
            [("w1", "o"), ("w2", "o"), ("w3", "p")],
        );
        let place = ledger.place("loose");
        assert_eq!(
            (place.owners, place.key, place.by_parent),
            (vec!["o"], "loose", true)
        );
        let place = ledger.place("loose/sub");
        assert_eq!((place.owners, place.key), (vec!["p"], "loose"));
        assert!(
            ledger.place("idle").owners.is_empty(),
            "no executor, no one to count against"
        );
        let place = ledger.place_by("idle", &["w3", "nobody"]);
        assert_eq!(place.owners, vec!["p"]);
        assert_eq!(ledger.held("o").len(), 1);
        assert_eq!(ledger.held("p").len(), 1);
    }

    #[test]
    fn only_in_progress_and_review_hold_a_slot_and_a_mirror_or_subtask_holds_its_tasks_one() {
        let ledger = Ledger::new(
            [
                led("a", None, "in_progress", "o"),
                entry("a/review", Some("a"), "review"),
                entry("review/a", None, "review"),
                led("b", None, "review", "o"),
                led("c", None, "blocked", "o"),
                led("d", None, "ready", "o"),
                led("e", None, "backlog", "o"),
                led("f", None, "concept", "o"),
                led("g", None, "design", "o"),
                led("h", None, "done", "o"),
                led("i", None, "cancelled", "o"),
                led("j", None, "in_progress", "other"),
            ],
            [],
        );
        let held = ledger.held("o");
        assert_eq!(
            named(&held),
            "a (in_progress) + a/review (review) + review/a (review), b (review)"
        );
        assert_eq!(held.len(), 2);
        let counts = ledger.counts();
        assert_eq!((counts["o"], counts["other"], counts.len()), (2, 1, 2));
    }

    #[test]
    fn the_cap_refuses_a_new_slot_only_and_never_one_already_held() {
        let mut tasks: Vec<Entry> = ["s1", "s2", "s3"]
            .into_iter()
            .map(|s| led(s, None, "in_progress", "o"))
            .collect();
        tasks.push(led("next", None, "ready", "o"));
        tasks.push(entry("s1/more", Some("s1"), "ready"));
        let ledger = Ledger::new(tasks, []);
        let full = ledger
            .take(&ledger.place("next"), 3)
            .expect_err("at the cap");
        assert_eq!((full.owner, full.held.len()), ("o", 3));
        assert_eq!(
            ledger.take(&ledger.place("next"), 4),
            Ok(vec!["o"]),
            "below the cap"
        );
        assert_eq!(
            ledger.take(&ledger.place("s1/more"), 3),
            Ok(vec![]),
            "s1's slot is open"
        );
        assert_eq!(
            ledger
                .take(&ledger.place("next"), 2)
                .unwrap_err()
                .held
                .len(),
            3,
            "above it"
        );
        let nobody = ledger.place_by("next/none", &[]);
        assert_eq!(
            ledger.take(&nobody, 1),
            Ok(vec![]),
            "no one to count it against"
        );
    }

    #[test]
    fn taking_the_role_moves_the_slot_to_the_taker() {
        let ledger = Ledger::new(
            [
                led("a", None, "in_progress", "o"),
                entry("b", None, "review"),
            ],
            [],
        );
        let after = ledger.orchestrated_by("b", "o");
        assert_eq!(after.place("b").owners, vec!["o"]);
        assert_eq!(ledger.take(&after.place("b"), 1).unwrap_err().owner, "o");
        let mine = ledger.orchestrated_by("a", "o");
        assert_eq!(
            ledger.take(&mine.place("a"), 1),
            Ok(vec![]),
            "its own slot already"
        );
    }

    #[test]
    fn a_loop_the_board_should_never_hold_ends_the_walk() {
        let ledger = Ledger::new(
            [
                entry("x", Some("y"), "in_progress"),
                entry("y", Some("x"), "in_progress"),
            ],
            [],
        );
        assert_eq!(ledger.place("x").key, "y");
        assert_eq!(ledger.place("unknown").key, "unknown");
    }

    #[test]
    fn the_refusal_names_the_holder_its_open_tasks_and_how_a_slot_frees() {
        let ledger = Ledger::new(
            [
                led("a", None, "in_progress", "o"),
                led("b", None, "review", "o"),
            ],
            [],
        );
        let full = Full {
            owner: "o",
            held: ledger.held("o"),
        };
        assert_eq!(
            refusal("c cannot start", &full, &[], 2),
            "c cannot start: o holds 2 open tasks, the cap of 2 an orchestrator holds: a (in_progress), b (review); a slot frees when one of them is done or cancelled"
        );
        assert!(refusal("c cannot start", &full, &["w"], 2).starts_with(
            "c cannot start: o (no orchestrator holds it: o is the parent of w) holds 2"
        ));
    }

    #[test]
    fn the_manifest_names_the_cap_or_none_and_a_broken_one_is_an_error() {
        let dir = std::env::temp_dir().join(format!("slots-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(configured(&dir), Ok(None), "no manifest");
        for (text, expected) in [
            ("[project]\nname = \"x\"\n", Ok(None)),
            ("[board]\non_review = [\"x\"]\n", Ok(None)),
            ("[board]\norchestrator_slots = 8\n", Ok(Some(8))),
            ("[board]\norchestrator_slots = 1\n", Ok(Some(1))),
        ] {
            std::fs::write(dir.join(MANIFEST), text).unwrap();
            assert_eq!(configured(&dir), expected, "{text}");
        }
        for (text, said) in [
            (
                "[board]\norchestrator_slots = 0\n",
                "an orchestrator needs at least one slot",
            ),
            (
                "[board]\norchestrator_slots = -2\n",
                "an orchestrator needs at least one slot",
            ),
            (
                "[board]\norchestrator_slots = \"eight\"\n",
                "[board] orchestrator_slots",
            ),
        ] {
            std::fs::write(dir.join(MANIFEST), text).unwrap();
            let error = configured(&dir).unwrap_err();
            assert!(error.contains(said), "{text}: {error}");
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
