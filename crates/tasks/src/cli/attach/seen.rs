//! What a session's monitor sees of the board at one poll ([`Seen`]), scoped to what it hears: its
//! own tasks, and for an orchestrator the ones it orchestrates and the ones nobody does
//! ([`crate::orchestration`]). Dispatchers built before that scope keep the whole board's look
//! ([`legacy_seen`]) in the callback state they share with newer ones, which keep their scoped look
//! beside it as a [`Narrowing`] of it.

use crate::health::{EXECUTING, Health};
use crate::maps::Collect as _;
use crate::orchestration::{Owners, claimable_work, told_of};
use crate::{Board, Error, Store, Task};
use std::collections::btree_map::Entry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// What a session's monitor sees of the board at one poll.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Seen {
    #[serde(default)]
    pub latest_message: Option<i64>,
    /// Verdicts on these tasks reach this recipient, so their status need not be repeated.
    #[serde(default)]
    pub verdict_tasks: BTreeSet<String>,
    /// Canonical active staffing inputs invalidate PULL without widening ROLE notifications.
    #[serde(default)]
    pub staffing: String,
    /// (slug, handle, role) on each task the session holds a role on.
    pub roles: BTreeSet<(String, String, String)>,
    /// The status of each task it holds a role on, and, for one it orchestrates, of every task below.
    pub statuses: BTreeMap<String, String>,
    /// Of `statuses`, the tasks an orchestrator follows only because nobody orchestrates them: it
    /// hears their move to `blocked`, which no PULL row shows, and leaves the rest to its digest.
    /// Never in a legacy look, whose record stays in the shape older dispatchers write.
    #[serde(default, skip_serializing_if = "BTreeSet::is_empty")]
    pub unclaimed: BTreeSet<String>,
    /// For an orchestrator, the ready tasks, slug -> title.
    pub ready: BTreeMap<String, String>,
    /// The health of each session it is the parent of.
    pub children: BTreeMap<String, Health>,
}

/// Whom a look's lines go to. A dispatch says a line to a recipient only where its look hears the
/// line's subject, and judges a line queued for it by the same test again at delivery.
impl Seen {
    /// Older staffing records have no canonical count; they need a fresh observation once.
    pub(crate) fn open_slots(&self) -> Option<usize> {
        serde_json::from_slice::<(String, usize)>(self.staffing.as_bytes())
            .ok()
            .map(|(_, count)| count)
    }

    /// Use the same successful canonical observation as this pass's PULL account.
    pub(crate) fn with_holdings(
        mut self,
        holdings: &crate::cli::pull::Holdings,
        handle: &str,
    ) -> Self {
        let count = holdings.get(handle).map_or(0, BTreeSet::len);
        self.staffing =
            serde_json::to_string(&(self.staffing, count)).expect("strings and counts serialize");
        self
    }
    /// Whether a STATUS move of `slug` to `status` is said: a task it follows, but one nobody
    /// orchestrates only as it blocks.
    pub fn hears_move(&self, slug: &str, status: &str) -> bool {
        self.statuses.contains_key(slug) && (!self.unclaimed.contains(slug) || status == "blocked")
    }

    /// Whether a ROLE change on `slug` is said: a task the session holds a role on.
    pub fn hears_roles(&self, slug: &str) -> bool {
        (self.roles.iter()).any(|(on, _, _)| on == slug)
    }

    /// Whether `session` going quiet is said: a child it is told of ([`told_of`]).
    pub fn hears_quiet(&self, session: &str) -> bool {
        self.children.contains_key(session)
    }
}

/// One board view, plus a canonical holdings observation for an orchestrator.
pub fn observe(store: &Store, handle: &str, kind: &str) -> Result<Seen, Error> {
    let observed = seen(&store.board()?, handle, kind);
    Ok(if kind == "orchestrator" {
        observed.with_holdings(&store.slot_holdings()?, handle)
    } else {
        observed
    })
}

/// What the session hears of the board.
pub(crate) fn seen(board: &Board, handle: &str, kind: &str) -> Seen {
    look(board, handle, kind, Scope::Heard)
}

/// The session's look as dispatchers built before one orchestrator per task took it, and still
/// read it from the callback state: for an orchestrator, every ready task, every task below the
/// ones it orchestrates and the staffing of the whole board; for anyone, its children by parent.
pub(crate) fn legacy_seen(board: &Board, handle: &str, kind: &str) -> Seen {
    look(board, handle, kind, Scope::Board)
}

/// How much of the board a look takes in.
#[derive(Clone, Copy, PartialEq)]
enum Scope {
    /// What the session hears ([`crate::orchestration`]).
    Heard,
    /// The whole board, as before an orchestrator heard only its own tasks and the unclaimed ones.
    Board,
}

fn look(board: &Board, handle: &str, kind: &str, scope: Scope) -> Seen {
    let mine: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|t| t.roles.iter().any(|r| r.handle == handle))
        .collect();
    let owners = Owners::new(&board.tasks);
    let reaches = |slug: &str| scope == Scope::Board || owners.reaches(slug, handle);
    let orchestrator = kind == "orchestrator";
    let Followed {
        watched,
        below,
        unclaimed,
    } = followed(board, &owners, &mine, handle, orchestrator, scope);
    // An orchestrator hears its own ready work and nobody's, never another orchestrator's.
    let ready = if orchestrator {
        (board.tasks.iter())
            .filter(|t| t.status == "ready" && reaches(&t.slug))
            .map(|t| (t.slug.clone(), t.title.clone()))
            .collect_map()
    } else {
        BTreeMap::new()
    };
    let staffing = if orchestrator {
        staffing(board, &reaches)
    } else {
        String::new()
    };
    let decides = |t: &&&Task| {
        (t.roles.iter())
            .any(|r| r.handle == handle && matches!(r.role.as_str(), "executor" | "orchestrator"))
    };
    let verdict_tasks = (mine.iter().filter(decides).map(|t| t.slug.clone()))
        .chain(below.into_iter().map(str::to_owned))
        .collect();
    let roles = (mine.iter())
        .flat_map(|t| (t.roles.iter()).map(|r| (t.slug.clone(), r.handle.clone(), r.role.clone())))
        .collect();
    // A session going quiet is said to its parent; to an orchestrator, only when it is the
    // orchestrator of the work that session holds, or that work is nobody's.
    let work = (orchestrator && scope == Scope::Heard).then(|| claimable_work(board));
    let children = (board.sessions.iter())
        .filter(|s| s.row.handle != handle)
        .filter(|s| told_of(&owners, work.as_ref(), &s.row, handle))
        .map(|s| (s.row.handle.clone(), s.health))
        .collect_map();
    Seen {
        latest_message: Some(board.latest_message),
        staffing,
        verdict_tasks,
        roles,
        statuses: (watched.iter())
            .map(|(slug, task)| (slug.to_string(), task.status.clone()))
            .collect_map(),
        unclaimed: unclaimed.into_iter().map(str::to_owned).collect(),
        ready,
        children,
    }
}

/// The tasks whose status a session follows, those of them it follows only from above, and those
/// it follows only because nobody orchestrates them.
struct Followed<'board> {
    watched: BTreeMap<&'board str, &'board Task>,
    below: HashSet<&'board str>,
    unclaimed: BTreeSet<&'board str>,
}

/// Its own tasks; for each it orchestrates, everything below but what another orchestrator holds;
/// for an orchestrator, every unfinished task nobody orchestrates that it holds no role on. A look
/// at the whole board follows everything below, and nothing for being nobody's.
fn followed<'board>(
    board: &'board Board,
    owners: &Owners,
    mine: &[&'board Task],
    handle: &str,
    orchestrator: bool,
    scope: Scope,
) -> Followed<'board> {
    let mut kids: HashMap<Option<&str>, Vec<&Task>> = HashMap::new();
    for task in &board.tasks {
        kids.entry(task.parent.as_deref()).or_default().push(task);
    }
    let mut watched = mine.iter().map(|t| (t.slug.as_str(), *t)).collect_map();
    let mut below: HashSet<&str> = HashSet::new();
    let orchestrated =
        |t: &&&Task| (t.roles.iter()).any(|r| r.handle == handle && r.role == "orchestrator");
    let elsewhere = |slug: &str| scope == Scope::Heard && owners.elsewhere(slug, handle);
    let orchestrating = mine.iter().filter(orchestrated);
    for task in orchestrating {
        let mut todo = kids
            .get(&Some(task.slug.as_str()))
            .cloned()
            .unwrap_or_default();
        while let Some(under) = todo.pop() {
            if elsewhere(&under.slug) || !below.insert(&under.slug) {
                continue;
            }
            watched.insert(&under.slug, under);
            todo.extend(kids.get(&Some(under.slug.as_str())).into_iter().flatten());
        }
    }
    let mut unclaimed: BTreeSet<&str> = BTreeSet::new();
    if orchestrator && scope == Scope::Heard {
        for task in crate::orchestration::unowned(owners, &board.tasks) {
            if let Entry::Vacant(entry) = watched.entry(&task.slug) {
                entry.insert(task);
                unclaimed.insert(&task.slug);
            }
        }
    }
    Followed {
        watched,
        below,
        unclaimed,
    }
}

/// The staffing of the work an orchestrator takes in (`reaches`), as one canonical string: a change
/// in it is a reason to compute the PULL digest again.
fn staffing(board: &Board, reaches: &dyn Fn(&str) -> bool) -> String {
    let active = |t: &&Task| {
        (matches!(t.status.as_str(), "ready" | "review") || EXECUTING.contains(&t.status.as_str()))
            && reaches(&t.slug)
    };
    let roles: BTreeSet<(&str, &str, &str, bool)> = (board.tasks.iter())
        .filter(active)
        .flat_map(|t| {
            (t.roles.iter())
                .filter(|r| matches!(r.role.as_str(), "executor" | "reviewer" | "reserved"))
                .map(|r| {
                    (
                        t.slug.as_str(),
                        r.handle.as_str(),
                        r.role.as_str(),
                        r.health.holds(),
                    )
                })
        })
        .collect();
    serde_json::to_string(&roles).expect("strings and booleans serialize")
}

pub(crate) fn pull_changed(before: Option<&Seen>, now: &Seen) -> bool {
    before.is_none_or(|before| {
        before.roles != now.roles
            || before.statuses != now.statuses
            || before.children != now.children
            || before.staffing != now.staffing
    })
}

/// A session's heard look ([`seen`]) as what it leaves out of and adds to its legacy look
/// ([`legacy_seen`]) of the same board. Dispatchers built before the narrowing read and write only
/// the legacy record and never this, so the heard baseline is always the legacy record as it now
/// stands with this on it ([`Narrowing::over`]): what an older writer said since counts as said for
/// both, and neither repeats a line for the other.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Narrowing {
    statuses: Delta<String>,
    ready: Delta<String>,
    children: Delta<Health>,
    /// The verdict tasks the legacy look holds that the heard one does not, and the reverse.
    verdicts_left: BTreeSet<String>,
    verdicts_added: BTreeSet<String>,
    unclaimed: BTreeSet<String>,
    staffing: String,
}

/// How one map of the heard look differs from the legacy look's. The entries both hold agree, as
/// both looks read one board, so the legacy record's own value stands for each key it holds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Delta<V> {
    /// Keys of the legacy map the heard one leaves out.
    left: BTreeSet<String>,
    /// Entries of the heard map the legacy one lacks.
    added: BTreeMap<String, V>,
}

impl<V> Default for Delta<V> {
    fn default() -> Self {
        Delta {
            left: BTreeSet::new(),
            added: BTreeMap::new(),
        }
    }
}

impl<V: Clone> Delta<V> {
    fn between(legacy: &BTreeMap<String, V>, heard: &BTreeMap<String, V>) -> Delta<V> {
        Delta {
            left: (legacy.keys())
                .filter(|key| !heard.contains_key(*key))
                .cloned()
                .collect(),
            added: (heard.iter())
                .filter(|(key, _)| !legacy.contains_key(*key))
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect_map(),
        }
    }

    fn over(&self, legacy: &BTreeMap<String, V>) -> BTreeMap<String, V> {
        let mut heard = (legacy.iter())
            .filter(|(key, _)| !self.left.contains(*key))
            .map(|(key, value)| (key.clone(), value.clone()))
            .collect_map();
        for (key, value) in &self.added {
            heard.entry(key.clone()).or_insert_with(|| value.clone());
        }
        heard
    }
}

impl Narrowing {
    /// The heard look `heard` as a narrowing of `legacy`, both of one board.
    pub fn between(legacy: &Seen, heard: &Seen) -> Narrowing {
        Narrowing {
            statuses: Delta::between(&legacy.statuses, &heard.statuses),
            ready: Delta::between(&legacy.ready, &heard.ready),
            children: Delta::between(&legacy.children, &heard.children),
            verdicts_left: (legacy.verdict_tasks)
                .difference(&heard.verdict_tasks)
                .cloned()
                .collect(),
            verdicts_added: (heard.verdict_tasks)
                .difference(&legacy.verdict_tasks)
                .cloned()
                .collect(),
            unclaimed: heard.unclaimed.clone(),
            staffing: heard.staffing.clone(),
        }
    }

    /// The heard look this narrowing makes of `legacy`. Its roles and messages are the legacy
    /// look's, which both scopes share.
    pub fn over(&self, legacy: &Seen) -> Seen {
        let mut verdict_tasks: BTreeSet<String> = legacy
            .verdict_tasks
            .difference(&self.verdicts_left)
            .cloned()
            .collect();
        verdict_tasks.extend(self.verdicts_added.iter().cloned());
        Seen {
            latest_message: legacy.latest_message,
            verdict_tasks,
            staffing: self.staffing.clone(),
            roles: legacy.roles.clone(),
            statuses: self.statuses.over(&legacy.statuses),
            unclaimed: self.unclaimed.clone(),
            ready: self.ready.over(&legacy.ready),
            children: self.children.over(&legacy.children),
        }
    }
}
