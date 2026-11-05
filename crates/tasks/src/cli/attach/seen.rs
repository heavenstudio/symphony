//! What a session's monitor sees of the board at one poll ([`Seen`]), scoped to what it hears: its
//! own tasks, and for an orchestrator the ones it orchestrates and the ones nobody does
//! ([`crate::orchestration`]).

use crate::health::{EXECUTING, FINISHED, Health};
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
    #[serde(default)]
    pub unclaimed: BTreeSet<String>,
    /// For an orchestrator, the ready tasks, slug -> title.
    pub ready: BTreeMap<String, String>,
    /// The health of each session it is the parent of.
    pub children: BTreeMap<String, Health>,
}

/// The session's view of the board, in one read however many tasks it orchestrates.
pub fn observe(store: &Store, handle: &str, kind: &str) -> Result<Seen, Error> {
    Ok(seen(&store.board()?, handle, kind))
}

pub(crate) fn seen(board: &Board, handle: &str, kind: &str) -> Seen {
    let mine: Vec<&Task> = board
        .tasks
        .iter()
        .filter(|t| t.roles.iter().any(|r| r.handle == handle))
        .collect();
    let owners = Owners::new(&board.tasks);
    let orchestrator = kind == "orchestrator";
    let Followed {
        watched,
        below,
        unclaimed,
    } = followed(board, &owners, &mine, handle, orchestrator);
    // An orchestrator hears its own ready work and nobody's, never another orchestrator's.
    let ready = if orchestrator {
        (board.tasks.iter())
            .filter(|t| t.status == "ready" && owners.reaches(&t.slug, handle))
            .map(|t| (t.slug.clone(), t.title.clone()))
            .collect()
    } else {
        BTreeMap::new()
    };
    let staffing = if orchestrator {
        staffing(board, &owners, handle)
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
    let work = orchestrator.then(|| claimable_work(board));
    let children = (board.sessions.iter())
        .filter(|s| s.row.handle != handle)
        .filter(|s| told_of(&owners, work.as_ref(), &s.row, handle))
        .map(|s| (s.row.handle.clone(), s.health))
        .collect();
    Seen {
        latest_message: Some(board.latest_message),
        staffing,
        verdict_tasks,
        roles,
        statuses: (watched.iter())
            .map(|(slug, task)| (slug.to_string(), task.status.clone()))
            .collect(),
        unclaimed: unclaimed.into_iter().map(str::to_owned).collect(),
        ready,
        children,
    }
}

/// The tasks whose status a session follows, those of them it follows only from above, and those
/// it follows only because nobody orchestrates them.
struct Followed<'b> {
    watched: BTreeMap<&'b str, &'b Task>,
    below: HashSet<&'b str>,
    unclaimed: BTreeSet<&'b str>,
}

/// Its own tasks; for each it orchestrates, everything below but what another orchestrator holds;
/// for an orchestrator, every unfinished task nobody orchestrates that it holds no role on.
fn followed<'b>(
    board: &'b Board,
    owners: &Owners,
    mine: &[&'b Task],
    handle: &str,
    orchestrator: bool,
) -> Followed<'b> {
    let mut kids: HashMap<Option<&str>, Vec<&Task>> = HashMap::new();
    for task in &board.tasks {
        kids.entry(task.parent.as_deref()).or_default().push(task);
    }
    let mut watched: BTreeMap<&str, &Task> = mine.iter().map(|t| (t.slug.as_str(), *t)).collect();
    let mut below: HashSet<&str> = HashSet::new();
    let orchestrated =
        |t: &&&Task| (t.roles.iter()).any(|r| r.handle == handle && r.role == "orchestrator");
    for task in mine.iter().filter(orchestrated) {
        let mut todo = kids
            .get(&Some(task.slug.as_str()))
            .cloned()
            .unwrap_or_default();
        while let Some(under) = todo.pop() {
            if owners.elsewhere(&under.slug, handle) || !below.insert(&under.slug) {
                continue;
            }
            watched.insert(&under.slug, under);
            todo.extend(kids.get(&Some(under.slug.as_str())).into_iter().flatten());
        }
    }
    let mut unclaimed: BTreeSet<&str> = BTreeSet::new();
    if orchestrator {
        let open = (board.tasks.iter()).filter(|t| !FINISHED.contains(&t.status.as_str()));
        for task in open.filter(|t| owners.of(&t.slug).is_empty()) {
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

/// The staffing of the work an orchestrator hears, as one canonical string: a change in it is a
/// reason to compute the PULL digest again.
fn staffing(board: &Board, owners: &Owners, handle: &str) -> String {
    let active = |t: &&Task| {
        (matches!(t.status.as_str(), "ready" | "review") || EXECUTING.contains(&t.status.as_str()))
            && owners.reaches(&t.slug, handle)
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
