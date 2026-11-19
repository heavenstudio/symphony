//! A task's gate decisions ([`crate::gates`]): the board's own field `fields._board.gates`, every
//! decision kept, each a line on the task's thread, in one write.
//!
//! A decision is for a land still to come: refused on a task cancelled, or done with nothing of it
//! left to land. An approve leaves its task done until its land, as a verdict does by the board's
//! own writers (its `unlanded` mark) and a custom workflow at a pair it does not mark terminal.
//!
//! A decision is its orchestrator's: the session holding that role on the task when it is given.
//! It is refused to a session that executed the task or work under it, and to one that reviews it
//! or gave it a verdict, whatever role it holds besides, so neither decides its own gate. The
//! owner's word in chat is recorded by the orchestrator, its reason naming that word.

use crate::Held;
use crate::gates::{Decision, Kind, Limit, of};
use crate::orchestration::marked_unlanded;
use crate::stamps::Catalogue;
use crate::store::{Change, Error, Say, Store, Task, refused};

/// The refusal of `handle`'s decision on `slug`, for what it did there (`did`).
fn not_its(slug: &str, handle: &str, did: &str) -> String {
    format!(
        "{handle} {did} {slug}: a gate decision is its orchestrator's, never its executor's or its \
         reviewer's, so no session decides its own gate"
    )
}

impl Store {
    /// Records `decision` on `slug` (decided by `decision.by`, at the board's clock), every
    /// decision before it kept, with a line on its thread saying it ([`Decision::line`]). Refused:
    /// an empty reason; a raise that names no limit ([`Limit::parse`]), or whose number is not
    /// above the one it raises; a waiver naming no whole commit, or of a stamp `catalogue` does
    /// not let a decision waive ([`Catalogue::unwaivable`]); a task cancelled, or done with no land
    /// still to come ([`crate::orchestration::unlanded`], a custom workflow's pair); and a
    /// decider that executed the task or work under it ([`Held`]), reviews it or gave it a verdict,
    /// or does not orchestrate it.
    pub fn decide_gate(
        &self,
        slug: &str,
        catalogue: &Catalogue,
        decision: &Decision,
    ) -> Result<Task, Error> {
        let handle = decision.by.as_str();
        let why = decision.why.trim();
        if why.is_empty() {
            return Err(refused(
                "a gate decision says why (--why WHY): what was measured, and why the gate gives \
                 way"
                .into(),
            ));
        }
        let gate = decision.gate.trim();
        let mut written = Decision {
            gate: gate.into(),
            why: why.into(),
            ..decision.clone()
        };
        match decision.kind {
            Kind::Raise => {
                written.gate = Limit::parse(gate).map_err(refused)?.gate();
                written.tip = None;
                let (Some(from), Some(to)) = (decision.from, decision.to) else {
                    return Err(refused(
                        "a raise names the number it raises and the number it raises it to".into(),
                    ));
                };
                if to <= from {
                    return Err(refused(format!(
                        "a raise of {} goes above {}, its number now: {} is not, and a limit \
                         lowered needs no decision",
                        written.gate,
                        crate::gates::grouped(from),
                        crate::gates::grouped(to)
                    )));
                }
            }
            Kind::Waive => {
                if let Some(why) = catalogue.unwaivable(gate) {
                    return Err(refused(why));
                }
                (written.from, written.to) = (None, None);
                let tip = decision.tip.as_deref().map(str::trim).unwrap_or_default();
                if tip.len() != 40 || !tip.chars().all(|c| c.is_ascii_hexdigit()) {
                    return Err(refused(format!(
                        "a waiver names the tip it is for, whole: not {tip:?}"
                    )));
                }
                written.tip = Some(tip.to_ascii_lowercase());
            }
        }
        let held: Held = self.ever_held(slug)?;
        let held_open = self.held_short_of_terminal(slug)?;
        let reviewed =
            (self.thread(slug)?.iter()).any(|line| line.kind == "verdict" && line.sender == handle);
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.status == "cancelled" {
                return Err(refused(format!(
                    "{slug} is cancelled: a gate decision is for an open task's land"
                )));
            }
            if task.status == "done"
                && !(held_open || marked_unlanded(&task.status, &task.fields).is_some())
            {
                return Err(refused(format!(
                    "{slug} is done, and no land of it is still to come: a gate decision is for \
                     a task's land"
                )));
            }
            if held.executed.contains(handle) {
                return Err(refused(not_its(slug, handle, "executed")));
            }
            if reviewed || view.holds(handle, "reviewer") {
                return Err(refused(not_its(slug, handle, "reviews")));
            }
            if !view.holds(handle, "orchestrator") {
                return Err(refused(format!(
                    "{handle} does not orchestrate {slug}: a gate decision is its \
                     orchestrator's, who records the owner's word too"
                )));
            }
            let mut all = of(&task.fields);
            all.push(Decision {
                at: view.now,
                ..written.clone()
            });
            Ok(Change {
                gates: Some(serde_json::to_value(&all).expect("decisions are JSON")),
                say: Say::system(written.line()),
                ..Change::default()
            })
        })
    }

    /// Whether a custom workflow holds `slug` at a pair it does not mark terminal, its land still
    /// to come however its board status reads: false on a board with no workflow active, in one
    /// small read.
    fn held_short_of_terminal(&self, slug: &str) -> Result<bool, Error> {
        if self.workflow_active()?.is_none() {
            return Ok(false);
        }
        let (Some(position), Some(workflow)) =
            (self.workflow_position(slug)?, self.workflow_read()?)
        else {
            return Ok(false);
        };
        let at = position.at();
        Ok((workflow.definition.positions.iter()).any(|pair| pair.at == at && !pair.terminal))
    }
}
