//! Recovery of a holder's exit on a task a custom workflow holds (docs/workflow/WORKFLOW.md, "Recovery"),
//! decided by [`crate::workflow::recover`] on admission's one read and written as an admitted
//! operation is (`pg::workflow::apply`): fenced on the active workflow, the task's version, each role it
//! relies on or drops and, for a lost holder, the holder's session as it was read, so a holder that
//! comes back, or moves the task first, wins and the recovery decides again. Its principal is the
//! board's recovery service, which the host must grant `recovery`; the session that asked is the
//! line's sender on the thread.
//!
//! Three exits come here: an agent's own `release` of its role (the position's `release` action),
//! a session that leaves (its `lost_holder` action, never refused, as a leave is not), and a lost
//! holder's recovery asked by any session on the board (`just tasks workflow recover`, its
//! `lost_holder` action, for a holder whose session holds nothing now).

use super::{
    Decided, Error, Failure, Host, Refusal, RefusalCode, Store, Taking, Transition, Value, context,
    pg, refusal,
};
use crate::store::statement::Statement;
use crate::workflow::recover::{self as plan, Recovery};
use crate::workflow::{Recovered, Trigger, board_service};
use serde_json::json;
use symphony_api::workflow::Pair;
use symphony_plugin::execution::Principal;

/// Which exit a recovery answers, and so which action and which check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Exit {
    /// The holder's own word: the position's `release` action.
    Release,
    /// A holder whose session holds nothing now: its `lost_holder` action, fenced on its session.
    Lost,
    /// A session leaving: its `lost_holder` action, never refused for the role it keeps.
    Leaving,
}

/// What `Store::release` does after a custom workflow had its say on the roles it holds there.
pub(in crate::store) enum Released {
    /// Every role it let go of was the workflow's: the task as it is now.
    Done(Box<crate::Task>),
    /// The board's own release goes on, for the orchestrator's role alone.
    Orchestrator,
    /// The board's own release, as it always ran: no workflow holds the task, or the release
    /// names no role of its.
    Board,
}

impl Store {
    /// `holder`'s lost work on `slug` recovered at `by`'s word, by the position's `lost_holder`
    /// action, as the board's recovery service, which `host` must grant `recovery`: every role
    /// it holds there but the orchestrator's let go (or kept where the action says a person
    /// recovers it), the task staying or moving to the action's target, never a target the board
    /// invents. Refused while `holder` is live, stale or awaiting a sweep: a holder that can still
    /// act lets go of its own. Answers the transition written, none when only roles the position
    /// does not staff went.
    pub fn recover_holder(
        &self,
        slug: &str,
        holder: &str,
        by: &str,
        host: &Host,
    ) -> Result<Option<Transition>, Error> {
        self.recover_exit(slug, holder, None, Exit::Lost, by, host)
    }

    /// `Store::release` on a task a custom workflow holds: each role but the orchestrator's goes by
    /// the position's `release` action ([`Store::recover_holder`]'s write), and what is left for
    /// the board's own release, if anything.
    pub(in crate::store) fn released_by_workflow(
        &self,
        slug: &str,
        handle: &str,
        role: Option<&str>,
    ) -> Result<Released, Error> {
        // A board that never activated a workflow holds no position: one small read decides it.
        if role == Some("orchestrator") || self.workflow_active()?.is_none() {
            return Ok(Released::Board);
        }
        let task = match self.get(slug) {
            Ok(task) => task,
            Err(Error::Refused(_)) => return Ok(Released::Board),
            Err(error) => return Err(error),
        };
        if !crate::store::workflow::position_held(&task.fields) {
            return Ok(Released::Board);
        }
        let mine = || task.roles.iter().filter(|r| r.handle == handle);
        let workflows =
            mine().any(|r| r.role != "orchestrator" && role.is_none_or(|x| r.role == x));
        if !workflows {
            return Ok(Released::Board);
        }
        let orchestrating = role.is_none() && mine().any(|r| r.role == "orchestrator");
        let host = Host::local();
        self.recover_exit(slug, handle, role, Exit::Release, handle, &host)?;
        Ok(if orchestrating {
            Released::Orchestrator
        } else {
            Released::Done(Box::new(self.get(slug)?))
        })
    }

    /// `Store::leave`'s first step: each task a custom workflow holds on which `handle` holds a
    /// role but the orchestrator's, recovered by its position's `lost_holder` action, before the
    /// leave lets go of the rest. Nothing to read on a board with no workflow active. A session
    /// that left already holds only what a manual recovery kept for a person, and leaves no
    /// more: refused as every write refuses a session that left, before any recovery.
    pub(in crate::store) fn recover_leaving(&self, handle: &str) -> Result<(), Error> {
        if self.workflow_active()?.is_none() {
            return Ok(());
        }
        let slugs: Vec<String> = self.retrying(|| {
            self.read(
                Statement::from(pg::workflow::leaving()),
                json!({"actor": handle}),
            )
        })?;
        if !slugs.is_empty() && self.session(handle)?.row.left_at.is_some() {
            return Err(Error::Refused(crate::store::left_actor(handle)));
        }
        let host = Host::local();
        for slug in slugs {
            self.recover_exit(&slug, handle, None, Exit::Leaving, handle, &host)?;
        }
        Ok(())
    }

    fn recover_exit(
        &self,
        slug: &str,
        holder: &str,
        role: Option<&str>,
        exit: Exit,
        by: &str,
        host: &Host,
    ) -> Result<Option<Transition>, Error> {
        let principal = board_service("recovery");
        if !host.grants.holds(&principal, "recovery") {
            return Err(Error::Refused(
                Refusal::new(
                    RefusalCode::Unauthorized,
                    "this host grants the board's recovery service no `recovery`: nobody \
                     recovers a holder here",
                )
                .to_string(),
            ));
        }
        let trigger = match exit {
            Exit::Release => Trigger::Release,
            Exit::Lost | Exit::Leaving => Trigger::LostHolder,
        };
        self.retrying(|| {
            let decided = self.decided(slug, &principal, None)?;
            let seen = match exit {
                Exit::Lost => self.unseen(slug, holder)?,
                Exit::Release | Exit::Leaving => Vec::new(),
            };
            let at = self.now();
            let recovery = plan::recover(
                context(&decided),
                &decided.facts,
                holder,
                role,
                trigger,
                &principal,
                at,
            )
            .map_err(refusal)?;
            let slots = self.recovery_slots(&decided, &recovery, &principal)?;
            let vars = vars(&decided, &recovery, holder, role, seen, slots, by, at);
            let guarded = !vars["slots"].as_array().is_none_or(Vec::is_empty);
            self.written(applied(guarded), vars, slug)?;
            Ok(recovery.transition)
        })
    }

    /// The slot counters `recovery`'s move holds under the cap, as `principal` takes it on what
    /// `decided` read ([`Store::taken`]); none for a stay or a role let go alone.
    pub(super) fn recovery_slots(
        &self,
        decided: &Decided,
        recovery: &Recovery,
        principal: &Principal,
    ) -> Result<Vec<Value>, Failure> {
        let Some(t) = &recovery.transition else {
            return Ok(Vec::new());
        };
        let taking = Taking {
            status: &t.status,
            acquires: recovery.acquires,
            held: decided.holds_now(),
            holds: decided.holds_at(&t.to),
            add: &[],
            what: format!(
                "{} cannot move to {} by its recovery",
                decided.task.slug,
                named(&t.to)
            ),
        };
        self.taken(decided, taking, principal)
    }

    /// The fence on a lost holder's session, as read: refused while it holds (live, stale or
    /// awaiting a sweep); nothing to fence for a session the board no longer has.
    fn unseen(&self, slug: &str, holder: &str) -> Result<Vec<Value>, Failure> {
        let sessions = self.sessions().map_err(Failure::Stop)?;
        match sessions.iter().find(|s| s.row.handle == holder) {
            None => Ok(Vec::new()),
            Some(session) if session.health.holds() => Err(refusal(Refusal::new(
                RefusalCode::ExplicitActionRequired,
                format!(
                    "{holder} is {} on the board: only a lost holder's work is recovered, and \
                     {holder} lets go of its own (`just tasks release {slug}`)",
                    session.health.as_str()
                ),
            ))),
            Some(session) => Ok(vec![
                json!({"handle": holder, "last_seen": session.row.last_seen}),
            ]),
        }
    }
}

/// The write of a recovery (`pg::workflow::apply`), its line on the thread by the system, and the
/// slot counters it holds guarded after it when `guarded`.
pub(super) fn applied(guarded: bool) -> Statement<'static> {
    Statement::from(pg::workflow::apply(
        &pg::logged(
            "v ->> 'slug'",
            "v ->> 'sender'",
            "'system'",
            "v ->> 'said'",
            "NULL",
        ),
        guarded,
    ))
    .posting()
}

/// The write's variables: the roles the holder held that it relies on (none for an attempt's
/// ending, which names no holder, `""`), those it drops, its transition, if any, and its line, said
/// by `by`, the session the write checks is still there unless it is the owner or the board.
#[expect(
    clippy::too_many_arguments,
    reason = "one write's variables, each read once"
)]
pub(super) fn vars(
    decided: &Decided,
    recovery: &Recovery,
    holder: &str,
    role: Option<&str>,
    seen: Vec<Value>,
    slots: Vec<Value>,
    by: &str,
    at: f64,
) -> Value {
    let pair = |(h, r): &(String, String)| json!({"handle": h, "role": r});
    let mut holding: Vec<(String, String)> = (decided.facts.roles.iter())
        .filter(|(h, r)| h == holder && r != "orchestrator" && role.is_none_or(|x| r == x))
        .cloned()
        .collect();
    holding.extend(recovery.drop.iter().cloned());
    crate::order::sort_by(&mut holding, &Ord::cmp);
    holding.dedup();
    let status = (recovery.transition.as_ref()).map_or(&decided.task.status, |t| &t.status);
    json!({
        "slug": decided.task.slug,
        "version": decided.task.version,
        "generation": decided.active.generation,
        "digest": decided.active.digest,
        "dependencies": [],
        "holding": holding.iter().map(pair).collect::<Vec<_>>(),
        "add": [],
        "drop": recovery.drop.iter().map(pair).collect::<Vec<_>>(),
        "seen": seen,
        "receipt": null,
        "receipt_body": null,
        // The owner and the board itself are no sessions the write could check is still there.
        "actor": (by != crate::store::work::OWNER && by != crate::store::messages::BOARD)
            .then_some(by),
        "transition": recovery.transition,
        "position": recovery.transition.as_ref().map(Transition::position),
        "moved": *status != decided.task.status,
        "sender": by,
        "said": said(recovery, holder),
        "now": at,
        "slots": slots,
    })
}

/// The recovery's line on the task's thread: the holder's exit, or the attempt's ending, it
/// answered, and where the task went.
pub(super) fn said(recovery: &Recovery, holder: &str) -> String {
    let let_go: Vec<String> = (recovery.drop.iter())
        .map(|(h, r)| format!("{h} {r}"))
        .collect();
    let Some(transition) = &recovery.transition else {
        return format!(
            "{holder} let go of {}, which its workflow position does not staff",
            let_go.join(", ")
        );
    };
    let recovered = transition
        .recovery
        .as_ref()
        .expect("a recovery's transition says what it recovered");
    let what = match (transition.key.as_deref()).and_then(|k| k.strip_prefix("attempt/")) {
        Some(attempt) => ended(recovered, attempt),
        None => format!("{holder}'s {} {}", recovered.role, exit(recovered.trigger)),
    };
    let from = transition.from.as_ref().unwrap_or(&transition.to);
    let went = if *from == transition.to {
        format!("stays at {}", named(from))
    } else {
        format!("{} → {}", named(from), named(&transition.to))
    };
    let mut line = format!(
        "the board's recovery ({}): {what}; {went}, workflow revision {}",
        transition.id, transition.revision
    );
    if !let_go.is_empty() {
        line.push_str(&format!("; let go: {}", let_go.join(", ")));
    }
    if recovered.manual {
        line.push_str(&format!(
            "; it waits on a person's recovery (`just tasks workflow actions {}`)",
            transition.slug
        ));
    }
    line
}

/// Attempt `attempt`'s ending, as `recovered` records it: an attempt no session holds is named by
/// its id alone, its holder.
pub(super) fn ended(recovered: &Recovered, attempt: &str) -> String {
    let (role, exit) = (&recovered.role, exit(recovered.trigger));
    if attempt == recovered.holder {
        format!("{role} attempt {attempt} {exit}")
    } else {
        format!("attempt {attempt} of {}'s {role} {exit}", recovered.holder)
    }
}

fn exit(trigger: Trigger) -> &'static str {
    match trigger {
        Trigger::Release => "released",
        Trigger::LostHolder => "lost",
        Trigger::AttemptFailure => "failed",
        Trigger::CancellationAcknowledged => "ended cancelled",
    }
}

fn named(pair: &Pair) -> String {
    format!("{}/{}", pair.phase, pair.state)
}

#[cfg(test)]
#[path = "recovery_tests.rs"]
mod tests;
