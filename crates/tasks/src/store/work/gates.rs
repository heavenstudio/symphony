//! A task's gate decisions ([`crate::gates`]): the board's own field `fields._board.gates`, every
//! decision kept, each a line on the task's thread, in one write.
//!
//! A decision is its orchestrator's: the session holding that role on the task when it is given.
//! It is refused to a session that executed the task or work under it, and to one that reviews it
//! or gave it a verdict, whatever role it holds besides, so neither decides its own gate. The
//! owner's word in chat is recorded by the orchestrator, its reason naming that word.

use crate::Held;
use crate::gates::{Decision, Kind, Limit, of};
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
    /// not let a decision waive ([`Catalogue::unwaivable`]); a task done or cancelled; and a
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
        let reviewed =
            (self.thread(slug)?.iter()).any(|line| line.kind == "verdict" && line.sender == handle);
        self.rule(slug, handle, |view| {
            view.actor(handle)?;
            let task = view.task(slug)?;
            if task.status == "done" || task.status == "cancelled" {
                return Err(refused(format!(
                    "{slug} is {}: a gate decision is for an open task's land",
                    task.status
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
}
