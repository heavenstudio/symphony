//! What each open task the listings send waits on ([`symphony_board::waits_on`]), since when, and
//! where a blocked one stopped; the board's orchestrators and the ready tasks waiting to start.
//! A block that names nothing is sent in the stage it stopped in (docs/workflow/TASKS.md, "What a task
//! waits on"): the board itself still holds it blocked.
//!
//! When a task took its status is its thread's replay ([`symphony_board::stages::in_status`]),
//! read for many tasks in one round trip, and kept ([`Stood`]), so a board read moved by a beat
//! reads no thread: only tasks whose status or write moved since are read again. Closed tasks
//! keep their review-round count as well.

use std::collections::BTreeMap;

use symphony_api::{Orchestrator, Task, WaitsOn};
use symphony_board::health::FINISHED;
use symphony_board::stages::InStatus;
use symphony_board::waits_on::{self, Blocked, Landing, Look};

/// Each task's review-round count, status entry and previous status, as last read, by slug,
/// beside the status and write time it was read for.
#[derive(Debug, Default)]
pub struct Stood {
    by_slug: BTreeMap<String, (String, u64, InStatus)>,
}

impl Stood {
    /// Read again what moved on `board`: tasks this has not read at the status and write time
    /// they hold, in one round trip ([`symphony_board::Store::in_status`]). Closed tasks are
    /// retained for their review counts; only removed tasks are dropped. None to read reads nothing.
    pub fn read(
        &mut self,
        store: &symphony_board::Store,
        board: &symphony_board::Board,
    ) -> Result<(), symphony_board::Error> {
        let moved: Vec<String> = (board.tasks.iter())
            .filter(|t| {
                self.by_slug
                    .get(&t.slug)
                    .is_none_or(|(status, updated, _)| {
                        *status != t.status || *updated != t.updated.to_bits()
                    })
            })
            .map(|t| t.slug.clone())
            .collect();
        let mut read = store.in_status(&moved)?;
        for task in &board.tasks {
            if let Some(stood) = read.remove(&task.slug) {
                let key = (task.status.clone(), task.updated.to_bits(), stood);
                self.by_slug.insert(task.slug.clone(), key);
            }
        }
        let mut kept = std::collections::BTreeSet::new();
        kept.extend(board.tasks.iter().map(|t| t.slug.as_str()));
        self.by_slug.retain(|slug, _| kept.contains(slug.as_str()));
        Ok(())
    }

    /// What was read of `task` at the status and write time it holds; none for one not read so.
    pub fn of(&self, task: &symphony_board::Task) -> Option<&InStatus> {
        let (status, updated, stood) = self.by_slug.get(&task.slug)?;
        (*status == task.status && *updated == task.updated.to_bits()).then_some(stood)
    }
}

/// `wire`, the listing's `task`, with what it waits on and since when, by the board `look` sees and
/// what `stood` read of it; a blocked one with the status it was blocked from, sent in that status
/// when its block names nothing.
pub fn fill(wire: &mut Task, task: &symphony_board::Task, look: &Look<'_>, stood: &Stood) {
    let read = stood.of(task);
    wire.review_rounds = read.map(|r| r.review_rounds);
    let Some(line) = waits_on::line(task, look, Landing::of(task, None).as_ref()) else {
        return;
    };
    if task.status == "blocked" {
        let recorded = Blocked::of(task).and_then(|record| record.from);
        wire.from = recorded.or_else(|| read.and_then(|r| r.from.clone()));
        let stage =
            (wire.from.as_deref()).filter(|from| *from != "blocked" && !FINISHED.contains(from));
        if line.unnamed
            && let Some(stage) = stage
        {
            wire.status = stage.to_string();
        }
    }
    wire.waits_on = Some(WaitsOn {
        kind: line.kind.into(),
        who: line.who,
        why: line.why,
        since: read.map_or(task.updated, |r| r.since),
    });
}

/// Each session holding the orchestrator role on an open task of `board`, by handle, with its
/// health.
pub fn orchestrators(board: &symphony_board::Board) -> Vec<Orchestrator> {
    (waits_on::orchestrators(&board.tasks).into_iter())
        .map(|(handle, health)| Orchestrator {
            handle,
            health: health.as_str().into(),
        })
        .collect()
}

/// What a task in the owner's sign-off lane that names no card waits on: the project's owner gate.
pub const SIGN_OFF: &str = "sign-off";

/// `tasks` with each in the owner's sign-off lane (`lane`: its slug, then the cards it posted that
/// wait on the owner) waiting on a decision, never a person: those cards, or the sign-off a
/// project's owner gate holds it for with none ([`SIGN_OFF`]). The lane is read beside the board,
/// so the snapshot says it where it sends the board.
pub fn decided(tasks: &mut [Task], lane: &BTreeMap<String, Vec<String>>) {
    for task in tasks {
        let Some(cards) = lane.get(&task.slug) else {
            continue;
        };
        let since = task.waits_on.as_ref().map_or(task.updated, |w| w.since);
        let (who, why) = if cards.is_empty() {
            (SIGN_OFF.to_string(), "its owner gate holds it")
        } else {
            (cards.join(", "), "the owner's sign-off")
        };
        task.waits_on = Some(WaitsOn {
            kind: waits_on::DECISION.into(),
            who,
            why: why.into(),
            since,
        });
    }
}

#[cfg(test)]
#[path = "waiting_tests.rs"]
mod tests;
