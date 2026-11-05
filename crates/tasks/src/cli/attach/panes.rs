//! The pane watch ([`pane_watch`]): BLOCKED once for a session it oversees ([`overseen`]) whose herdr
//! pane waits on an approval or a question, and WATCH once while herdr cannot be asked.

use crate::cli::agents::{Answer, Pane, find_pane};
use crate::orchestration::{Owners, claimable_work, told_of};
use crate::rows::TaskSessionRow;
use crate::{Board, Task};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

/// A session whose herdr pane waits on an approval or a question: the task it was launched for,
/// and the pane.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Blocked {
    pub task: String,
    pub pane: String,
}

/// What the pane watch last told a recipient: the sessions it said were blocked, and whether it
/// said herdr could not be asked. Neither is said again until it ends.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct PaneWatch {
    pub blocked: BTreeMap<String, Blocked>,
    pub unanswered: bool,
}

/// The sessions `handle` oversees: its children as LOST reaches them (an orchestrator's only when
/// their work is its own or nobody's), and whoever holds a role on a task it orchestrates or on one
/// below it that no other orchestrator holds. Never itself, and none that left.
pub fn overseen<'b>(board: &'b Board, handle: &str) -> BTreeSet<&'b str> {
    let mut kids: HashMap<&str, Vec<&Task>> = HashMap::new();
    for task in &board.tasks {
        if let Some(parent) = &task.parent {
            kids.entry(parent.as_str()).or_default().push(task);
        }
    }
    let orchestrated =
        |t: &&Task| (t.roles.iter()).any(|r| r.handle == handle && r.role == "orchestrator");
    let mut todo: Vec<&Task> = board.tasks.iter().filter(orchestrated).collect();
    let owners = Owners::new(&board.tasks);
    let mut tasks: HashSet<&str> = HashSet::new();
    let mut holders: HashSet<&str> = HashSet::new();
    while let Some(task) = todo.pop() {
        if tasks.insert(&task.slug) {
            holders.extend(task.roles.iter().map(|r| r.handle.as_str()));
            let below = kids.get(task.slug.as_str()).into_iter().flatten();
            todo.extend(below.filter(|kid| !owners.elsewhere(&kid.slug, handle)));
        }
    }
    let orchestrator =
        (board.sessions.iter()).any(|s| s.row.handle == handle && s.row.kind == "orchestrator");
    let work = orchestrator.then(|| claimable_work(board));
    (board.sessions.iter())
        .map(|s| &s.row)
        .filter(|s| s.left_at.is_none() && s.handle != handle)
        .filter(|s| {
            told_of(&owners, work.as_ref(), s, handle) || holders.contains(s.handle.as_str())
        })
        .map(|s| s.handle.as_str())
        .collect()
}

/// One look at the panes of the `overseen` sessions launched into one still open (`open`, oldest
/// first): BLOCKED for each whose pane herdr reports `blocked` and did not at the last look, and
/// WATCH once when herdr cannot be asked. `panes` is asked only when there is a pane to look at.
/// Returns what the watch remembers for the next look.
pub fn pane_watch(
    board: &Board,
    overseen: &BTreeSet<&str>,
    open: &[TaskSessionRow],
    panes: &dyn Fn() -> Answer<Vec<Pane>>,
    before: &PaneWatch,
    emit: &mut dyn FnMut(&str),
) -> PaneWatch {
    // Each session's latest open launch: the pane it was launched into.
    let launched: BTreeMap<&str, &TaskSessionRow> = open
        .iter()
        .filter(|r| overseen.contains(r.handle.as_str()))
        .map(|r| (r.handle.as_str(), r))
        .collect();
    if launched.is_empty() {
        return PaneWatch::default();
    }
    let panes = match panes() {
        Ok(panes) => panes,
        Err(why) => {
            if !before.unanswered {
                emit(&format!(
                    "WATCH herdr unanswered: {why} — blocked panes go unreported until it answers"
                ));
            }
            let mut blocked = before.blocked.clone();
            blocked.retain(|who, _| launched.contains_key(who.as_str()));
            return PaneWatch {
                blocked,
                unanswered: true,
            };
        }
    };
    let bound: HashMap<&str, &str> = board
        .sessions
        .iter()
        .filter_map(|s| Some((s.row.handle.as_str(), s.row.agent.as_deref()?)))
        .collect();
    let blocked: BTreeMap<String, Blocked> = launched
        .iter()
        .filter_map(|(&who, &record)| {
            let pane = find_pane(&panes, Some(record), bound.get(who).copied(), who)?;
            (pane.status.as_deref() == Some("blocked")).then(|| {
                let blocked = Blocked {
                    task: record.slug.clone(),
                    pane: pane.id.clone(),
                };
                (who.to_string(), blocked)
            })
        })
        .collect();
    for (who, b) in &blocked {
        if !before.blocked.contains_key(who) {
            emit(&format!(
                "BLOCKED {who} {} {} — its pane waits on an approval or a question",
                b.task, b.pane
            ));
        }
    }
    PaneWatch {
        blocked,
        unanswered: false,
    }
}
