//! Resume explicit task blocks once their named task and every prerequisite are done. Completion
//! has the same meaning as claim/PULL: a done task satisfies it, including an approve awaiting its
//! landing. Cancellation never satisfies it. Unknown and independent blockers stay as recorded.
//!
//! Task writes reconcile before observing delivery; dispatch reconciles again after an interrupted
//! writer. The task version fences a changed blocker, dependencies and roles. Each prerequisite
//! is also held by a write, so a concurrent reopening cannot pass the completion check. The move
//! and its thread line commit together; native STATUS/PULL delivery uses its existing accounting.

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
        let cap = self.slots();
        self.retrying(|| {
            let which = if cap.is_some() { Which::Starting(slug) } else { Which::Slug(slug) };
            let view = self.view(&which)?;
            let Some(task) = view.tasks().into_iter().find(|t| t.slug == slug) else { return Ok(false) };
            let Some(block) = Blocked::of(&task) else { return Ok(false) };
            let Some(Thing::Task(named)) = block.thing() else { return Ok(false) };
            let mut required = task.depends.clone();
            required.push(named.into());
            required.sort();
            required.dedup();
            let prerequisites: Vec<Prerequisite> = self.read(
                both(sql::PREREQUISITES, pg::prerequisites()), json!({"required": required}),
            )?;
            if prerequisites.len() != required.len() || prerequisites.iter().any(|p| p.status != "done") {
                return Ok(false);
            }
            let executing: Vec<String> = view.holders("executor").map(|r| r.handle.clone()).collect();
            let back = match block.from.as_deref() {
                Some(was @ ("concept" | "design" | "ready" | "review")) => was,
                Some("in_progress") | None if !executing.is_empty() => "in_progress",
                Some("in_progress") | None => "ready",
                _ => return Ok(false),
            };
            let slots = if let Some(cap) = cap.filter(|_| OPEN.contains(&back)) {
                let opens = Opens {
                    status: Some(back.into()), by: executing,
                    what: format!("{slug} cannot resume from its task block"),
                    ..Opens::default()
                };
                match self.slots_taken(&view, slug, &opens, cap) {
                    Ok(slots) => slots,
                    // The next dispatch tries again when the slot is free.
                    Err(Failure::Stop(Error::Refused(_))) => return Ok(false),
                    Err(error) => return Err(error),
                }
            } else { Vec::new() };
            let version = view.snap.tasks.iter().find(|t| t.row.slug == slug).unwrap().version;
            let vars = json!({
                "slug": slug, "version": version, "status": back, "now": (self.clock)(),
                "watch": prerequisites.iter().map(|p| json!({"slug": p.slug, "version": p.version})).collect::<Vec<_>>(),
                "slots": slots,
                "said": crate::stages::moved_line("board", "blocked", back),
            });
            let _: serde_json::Value = self.transact_dispatch(
                both(sql::resume(), pg::resume()), vars, false,
            )?;
            Ok(true)
        })
    }
}
