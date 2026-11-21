//! Resume explicit task blocks once their named task and every prerequisite are done. Completion
//! has the same meaning as claim/PULL: a done task satisfies it, including an approve awaiting its
//! landing. Cancellation never satisfies it. Unknown and independent blockers stay as recorded.
//!
//! Task writes reconcile before observing delivery; dispatch reconciles again after an interrupted
//! writer. The task version fences a changed blocker, dependencies and roles. Each prerequisite
//! is also held by a write, so a concurrent reopening cannot pass the completion check. The move
//! and its thread line commit together; native STATUS/PULL delivery uses its existing accounting.
//!
//! A block the board cannot judge alone, on the owner's decision or the owner's approve of a tip,
//! is ended by [`Store::resume_judged`], which the one server calls once it has read the owner's
//! word where the owner gave it (`symphony::decided`): the same move, back to the same status,
//! under the same fence, said as `unblocked: WHY; back to STATUS`
//! ([`crate::stages::unblocked_line`]).

use super::postgres::resume as pg;
use super::statement::both;
use super::surreal::resume as sql;
use super::{Error, Failure, Opens, Store, Which};
use crate::slots::OPEN;
use crate::waits_on::{Blocked, Thing};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct Prerequisite {
    slug: String,
    status: String,
    version: i64,
}

impl Store {
    pub(super) fn resume_after_write(&self) -> bool {
        match self.resume_task_blocks() {
            Ok(changed) => changed,
            Err(error) => {
                eprintln!("Task-block reconciliation deferred: {error}");
                false
            }
        }
    }

    pub(super) fn resume_task_blocks(&self) -> Result<bool, Error> {
        let candidates: Vec<String> =
            self.retrying(|| self.read(both(sql::CANDIDATES, pg::candidates()), json!({})))?;
        let mut changed = false;
        for slug in candidates {
            changed |= self.resume_task_block(&slug)?;
        }
        Ok(changed)
    }

    fn resume_task_block(&self, slug: &str) -> Result<bool, Error> {
        let moved = self.resume_if(slug, |task, block| {
            let Some(Thing::Task(named)) = block.thing() else {
                return Ok(None);
            };
            let mut required = task.depends.clone();
            required.push(named.into());
            required.sort();
            required.dedup();
            let prerequisites: Vec<Prerequisite> = self.read(
                both(sql::PREREQUISITES, pg::prerequisites()),
                json!({"required": required}),
            )?;
            if prerequisites.len() != required.len()
                || prerequisites.iter().any(|p| p.status != "done")
            {
                return Ok(None);
            }
            Ok(Some(Over {
                watch: prerequisites
                    .iter()
                    .map(|p| json!({"slug": p.slug, "version": p.version}))
                    .collect(),
                why: None,
            }))
        })?;
        Ok(moved.is_some())
    }

    /// End `slug`'s block when it is still the one `judged` records (what it is on and when it was
    /// recorded, as the caller read it), for `why`, a few words without `; back to` in them: back
    /// to the status the block recorded, as a `task:` block's end is, in one write fenced on the
    /// task's version with the line `unblocked: WHY; back to STATUS`, so a move or a block anew
    /// since the caller read it (the owner's, a session's) wins and nothing is said twice. The
    /// status it went back to; none when the task is not blocked so any more, its block names a
    /// status no block returns to, or the slot cap holds it until a slot frees (ask again then).
    pub fn resume_judged(
        &self,
        slug: &str,
        judged: &Blocked,
        why: &str,
    ) -> Result<Option<String>, Error> {
        self.resume_if(slug, |_, block| {
            Ok(
                (block.on == judged.on && block.at == judged.at).then(|| Over {
                    watch: Vec::new(),
                    why: Some(why.to_string()),
                }),
            )
        })
    }

    /// `slug` moved out of its block, back to the status its record names, once `over` says the
    /// block is over (with the tasks whose versions fence the write and why), in one write with its
    /// line. The status it went back to, or none.
    fn resume_if(
        &self,
        slug: &str,
        over: impl Fn(&crate::Task, &Blocked) -> Result<Option<Over>, Failure>,
    ) -> Result<Option<String>, Error> {
        let cap = self.slots();
        self.retrying(|| {
            let which = if cap.is_some() {
                Which::Starting(slug)
            } else {
                Which::Slug(slug)
            };
            let view = self.view(&which)?;
            let Some(task) = view.tasks().into_iter().find(|t| t.slug == slug) else {
                return Ok(None);
            };
            let Some(block) = Blocked::of(&task) else {
                return Ok(None);
            };
            let Some(Over { watch, why }) = over(&task, &block)? else {
                return Ok(None);
            };
            let executing: Vec<String> =
                view.holders("executor").map(|r| r.handle.clone()).collect();
            let Some(back) = back_to(&block, !executing.is_empty()) else {
                return Ok(None);
            };
            let slots = if let Some(cap) = cap.filter(|_| OPEN.contains(&back)) {
                let opens = Opens {
                    status: Some(back.into()),
                    by: executing,
                    what: format!("{slug} cannot resume from its block"),
                    ..Opens::default()
                };
                match self.slots_taken(&view, slug, &opens, cap) {
                    Ok(slots) => slots,
                    // The next dispatch tries again when the slot is free.
                    Err(Failure::Stop(Error::Refused(_))) => return Ok(None),
                    Err(error) => return Err(error),
                }
            } else {
                Vec::new()
            };
            let version = view
                .snap
                .tasks
                .iter()
                .find(|t| t.row.slug == slug)
                .unwrap()
                .version;
            let said = match &why {
                Some(why) => crate::stages::unblocked_line(why, back),
                None => crate::stages::moved_line("board", "blocked", back),
            };
            let vars = json!({
                "slug": slug, "version": version, "status": back, "now": (self.clock)(),
                "watch": watch, "slots": slots, "said": said,
            });
            let _: serde_json::Value =
                self.transact_dispatch(both(sql::resume(), pg::resume()), vars, false)?;
            Ok(Some(back.to_string()))
        })
    }
}

/// What ends a block, once its caller judged it over: the tasks whose versions fence the write
/// (a `task:` block's prerequisites), and why it ended, for `unblocked: WHY; back to STATUS`
/// (none says the move's own line, `board moved it blocked → STATUS`).
struct Over {
    watch: Vec<serde_json::Value>,
    why: Option<String>,
}

/// The status a task blocked as `block` goes back to, `executing` whether an executor holds it:
/// the one its record names, but work in progress with no executor goes back to ready, and a block
/// that recorded none to in progress with one and ready without; none for any other status.
fn back_to(block: &Blocked, executing: bool) -> Option<&'static str> {
    Some(match block.from.as_deref() {
        Some("concept") => "concept",
        Some("design") => "design",
        Some("ready") => "ready",
        Some("review") => "review",
        Some("in_progress") | None if executing => "in_progress",
        Some("in_progress") | None => "ready",
        _ => return None,
    })
}
