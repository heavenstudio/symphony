//! A custom delivery workflow's records on the board (docs/WORKFLOW.md), from schema 24: the
//! active generation, the immutable definitions, activations, transitions and receipts in the
//! `workflow` table, and the position of each task the workflow holds in its
//! `fields._board.workflow`.
//!
//! Nothing but this module writes them, and the schema's fences hold every writer to that: a
//! task's position, or the status of a task that has one, changes only in the write that records
//! its transition, a position goes only in the return that retires it, and a recorded
//! definition, activation, transition or receipt is never changed (`crate::schema`'s
//! `workflow_position`, `workflow_history` and `workflow_active`, and their PostgreSQL twins).
//! No fence reads a deletion, which the whole-board copy makes of the table: that the active
//! generation goes only in a return's write ([`Store::return_to_default`]) is this module's and
//! `Store::raw`'s refusal of the table to hold. `Store::set_field` refuses the board's own
//! fields. A board with no custom workflow holds none of this, and every other write meets the
//! fences as it always ran.

use super::postgres::workflow as pg;
use super::statement::both;
use super::surreal::workflow as sql;
use super::{Error, Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::cmp::Ordering;
use symphony_api::workflow::{Definition, Effect, Operation, Pair, Revision};
use symphony_plugin::execution::Principal;

/// Where a task a custom workflow holds stands (`fields._board.workflow`): its pair, the pair a
/// blocked task resumes to, the activation generation and digest it was last moved under, and its
/// workflow revision, which only its own transitions move (a post on its thread or an edit of its
/// title moves none).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskPosition {
    pub phase: String,
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Pair>,
    pub generation: u64,
    pub revision: u64,
    pub digest: String,
    /// The recovery that left it waiting on a person's manual recovery, until its next
    /// transition: none for a task no holder's exit stopped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manual: Option<Recovered>,
}

/// What a recovery answered (docs/WORKFLOW.md, "Recovery"): a holder's exit
/// (`Store::recover_holder`), an agent's own release of its role or a holder lost, or an execution
/// attempt's ending (`Store::observe_attempt`), each by the position's configured action; or the
/// automatic edge the board took on an attempt's failure. A transition answering an attempt is
/// keyed `attempt/ID` ([`Transition::key`]), which names the attempt.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovered {
    pub trigger: Trigger,
    /// The holder that exited; for an attempt, the handle it ran as, or the attempt where it ran
    /// as none.
    pub holder: String,
    /// The role of the position's staffing the holder held there; for an attempt, the role it ran
    /// as.
    pub role: String,
    /// Whether the action asks a person to recover the task by hand.
    pub manual: bool,
}

/// What a recovery answers: the position's `release` action, its `lost_holder` one, or, on an
/// execution attempt's ending, its `attempt_failure` or `cancellation_acknowledged` one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Release,
    LostHolder,
    /// An attempt that ended failed or rejected, no cancel asked of it.
    AttemptFailure,
    /// An attempt whose cancel is known to have taken: it ended after one was asked.
    CancellationAcknowledged,
}

impl Trigger {
    /// Its name in the definition and on the record.
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Release => "release",
            Trigger::LostHolder => "lost_holder",
            Trigger::AttemptFailure => "attempt_failure",
            Trigger::CancellationAcknowledged => "cancellation_acknowledged",
        }
    }
}

impl TaskPosition {
    /// The position `value` holds, read by field as its decoder reads it: none where a field is
    /// missing, of another type, or one it does not know. No decoder of its own: every program
    /// that lists a PULL reads a held task's position, and links what it reads
    /// (tools/size-budget.toml).
    pub fn of(value: &Value) -> Option<TaskPosition> {
        const FIELDS: [&str; 7] = [
            "phase",
            "state",
            "resume",
            "generation",
            "revision",
            "digest",
            "manual",
        ];
        let fields = known(value, &FIELDS)?;
        let resume = match fields.get("resume") {
            None | Some(Value::Null) => None,
            Some(pair) => {
                let pair = known(pair, &["phase", "state"])?;
                Some(Pair {
                    phase: text(pair, "phase")?,
                    state: text(pair, "state")?,
                })
            }
        };
        let manual = match fields.get("manual") {
            None | Some(Value::Null) => None,
            Some(recovered) => {
                let recovered = known(recovered, &["trigger", "holder", "role", "manual"])?;
                Some(Recovered {
                    trigger: match recovered.get("trigger")?.as_str()? {
                        "release" => Trigger::Release,
                        "lost_holder" => Trigger::LostHolder,
                        "attempt_failure" => Trigger::AttemptFailure,
                        "cancellation_acknowledged" => Trigger::CancellationAcknowledged,
                        _ => return None,
                    },
                    holder: text(recovered, "holder")?,
                    role: text(recovered, "role")?,
                    manual: recovered.get("manual")?.as_bool()?,
                })
            }
        };
        Some(TaskPosition {
            phase: text(fields, "phase")?,
            state: text(fields, "state")?,
            resume,
            generation: fields.get("generation")?.as_u64()?,
            revision: fields.get("revision")?.as_u64()?,
            digest: text(fields, "digest")?,
            manual,
        })
    }

    /// Its pair.
    pub fn at(&self) -> Pair {
        Pair {
            phase: self.phase.clone(),
            state: self.state.clone(),
        }
    }
}

/// One move of a task's position, as recorded (`workflow:['event', slug, revision]`): an
/// admitted operation on an edge, or an activation moving the task to its mapped pair.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    /// Its stable identity, `SLUG@REVISION`.
    pub id: String,
    pub slug: String,
    /// The task's workflow revision it made.
    pub revision: u64,
    pub generation: u64,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
    /// The activation that moved it, for a move no edge made.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activation: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<Pair>,
    pub to: Pair,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume: Option<Pair>,
    /// The board status the position projects to, which the task's row holds.
    pub status: String,
    pub principal: Principal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<Effect>,
    /// The idempotency key of the request that made it: `attempt/ID` for the board's answer to
    /// execution attempt ID's ending, by an edge or a recovery, which answers it once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    /// The holder's exit it answered, for a recovery's move or stay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovered>,
    /// The task's accepted output standing after it (docs/WORKFLOW.md, "Accepted outputs"): this
    /// transition's own id where its edge accepted the output, the one before it carried by a move
    /// between two positions that satisfy dependants, none once the task stands where none does.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub accepted: Option<String>,
    pub at: f64,
}

/// Of a recorded [`Transition`], what the pipeline alarm reads: the status it put its task at, and
/// when. Read by field from the record's value ([`Moved::of`]), with no decoder of its own: every
/// program that delivers a PULL (`agent-send` and `symphony` among them) composes the alarm, and
/// links what it reads (tools/size-budget.toml).
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub slug: String,
    pub revision: u64,
    pub status: String,
    pub at: f64,
}

impl Moved {
    /// The four fields of a recorded transition's `body`; none where one is missing.
    pub fn of(body: &Value) -> Option<Moved> {
        Some(Moved {
            slug: body.get("slug")?.as_str()?.to_string(),
            revision: body.get("revision")?.as_u64()?,
            status: body.get("status")?.as_str()?.to_string(),
            at: body.get("at")?.as_f64()?,
        })
    }
}

impl Transition {
    /// The identity of the transition that made `revision` of `slug`.
    pub fn id_of(slug: &str, revision: u64) -> String {
        format!("{slug}@{revision}")
    }

    /// The position it puts its task at.
    pub fn position(&self) -> TaskPosition {
        TaskPosition {
            phase: self.to.phase.clone(),
            state: self.to.state.clone(),
            resume: self.resume.clone(),
            generation: self.generation,
            revision: self.revision,
            digest: self.digest.clone(),
            manual: self.recovery.clone().filter(|r| r.manual),
        }
    }
}

/// The active custom workflow: its activation generation and its definition's digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Active {
    pub generation: u64,
    pub digest: String,
}

/// A definition as recorded under its digest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recorded {
    pub definition: Definition,
    pub revision: Revision,
}

/// What following the default branch's workflow did: nothing, on a board whose project commits
/// none and has none active; nothing, as what it commits is active already; activated it; or
/// returned the board to its own writers, as the activation of that generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Followed {
    Legacy,
    Unchanged(Active),
    Activated(Active),
    Returned { generation: u64 },
}

/// How many times an activation is planned again after the board changed under it.
const PLANS: usize = 5;

/// What an activation reads ([`sql::STANDINGS`]).
#[derive(Deserialize)]
struct Standings {
    active: Option<Active>,
    /// The generation of the last activation recorded, 0 on a board that never activated one:
    /// the active one's, or a return's, which leaves none active.
    last: u64,
    tasks: Vec<Read>,
}

#[derive(Deserialize)]
struct Read {
    slug: String,
    version: i64,
    status: String,
    position: Option<TaskPosition>,
    blocked_from: Option<String>,
    /// The block record the task carries, whatever its status: a workflow leaves it as it was.
    blocked: Option<Value>,
    retired: u64,
    accepted: Option<String>,
    /// Its live execution attempts no cancel was asked of, oldest first.
    #[serde(default)]
    attempts: Vec<String>,
}

/// What [`Store::standings`] answers.
struct Planned {
    /// The read, its tasks taken out into `tasks`.
    read: Standings,
    /// The tasks as the planner takes them.
    tasks: Vec<crate::workflow::activate::Standing>,
    /// The block record each task carries, by its slug.
    blocks: std::collections::BTreeMap<String, Value>,
    /// Each executor or reviewer the read holds as no longer holding its role, with its session's
    /// last beat as read: one that beats before the write makes it stale, and the next plan reads
    /// it live.
    seen: Vec<Value>,
}

/// What a return leaves on a task in its position's place (`fields._board.workflow_retired`):
/// the workflow revision its return made, which its next transition continues from, and the
/// generation that retired it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Retired {
    pub revision: u64,
    pub generation: u64,
}

/// The last workflow revision of a task whose board fields are `fields`, where a return retired
/// its position; 0 for a task no return retired.
pub(crate) fn retired(
    fields: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>,
) -> u64 {
    (fields.get("_board"))
        .and_then(|board| board.get("workflow_retired"))
        .and_then(|retired| retired.get("revision"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

impl Store {
    /// The workflow `committed` sets made active on this board, as automatic activation B runs
    /// it (docs/DECISIONS.md#configurable-workflow-activation): a file with no `[workflow]` on a
    /// board with none active writes nothing; one that drops the workflow a board activated
    /// returns the board to its own writers ([`Store::return_to_default`]); a custom definition
    /// is activated unless it is the active one ([`Store::activate_workflow`]).
    pub fn follow_committed(
        &self,
        committed: &crate::workflow::Committed,
        host: &crate::workflow::Host,
    ) -> Result<Followed, Error> {
        match &committed.custom {
            Some(custom) => self.activate_workflow(custom, host),
            None => match self.workflow_active()? {
                None => Ok(Followed::Legacy),
                Some(_) => self.return_to_default(committed),
            },
        }
    }

    /// The board returned to its own writers, as `committed`, a project file with no
    /// `[workflow]`, asks of a board with a workflow active: the next generation's activation, in
    /// one write. Every task a workflow holds, closed ones too, takes the status (and a blocked
    /// one the blocked source) of the built-in v1 pair it goes to
    /// ([`crate::workflow::activate::returning`]) and loses its position, its last workflow
    /// revision kept beside it; each move is recorded as a transition, the activation recorded
    /// with the commit and the mappings it applied, and no generation is left active, so every
    /// writer finds a board with no custom workflow. The definitions, activations and
    /// transitions before it stay. A task returning blocked keeps the block record it carried
    /// (what it waits on and why) where it has stood blocked since that record was written, and
    /// takes one waiting on `parked` where it carried none or left blocked since.
    /// Nothing on a board with none active. Refused whole, the active generation and every task
    /// left as they were, where a task cannot be placed; planned again when the board changed
    /// between the read and the write.
    pub fn return_to_default(
        &self,
        committed: &crate::workflow::Committed,
    ) -> Result<Followed, Error> {
        use crate::workflow::activate;
        let by = crate::workflow::board_service("activation");
        let mut refusal = None;
        for _ in 0..PLANS {
            let Planned {
                read,
                tasks,
                blocks,
                seen,
            } = self.standings()?;
            let Some(active) = read.active else {
                return Ok(Followed::Legacy);
            };
            let old = self.active_definition(&active)?;
            let generation = active.generation + 1;
            let at = self.now();
            let plan = match activate::returning(
                &old,
                &committed.returning,
                &tasks,
                generation,
                &by,
                at,
            ) {
                Ok(plan) => plan,
                Err(why) if self.moved_since(&tasks)? => {
                    refusal = Some(why);
                    continue;
                }
                Err(why) => return Err(refused_workflow(why)),
            };
            let blocks = self.standing_blocks(blocks, &plan)?;
            let moved: Vec<Value> = (plan.moved.iter())
                .map(|m| {
                    let transition = &m.transition;
                    let mut moved = json!({
                        "slug": transition.slug, "version": m.version, "expected": m.expected,
                        "transition": transition,
                        "retired": Retired { revision: transition.revision, generation },
                    });
                    if transition.status == "blocked" {
                        let from = transition.resume.as_ref().map(|pair| pair.phase.clone());
                        moved["blocked"] = json!(blocked(blocks.get(&transition.slug), from, at));
                    }
                    moved
                })
                .collect();
            let vars = json!({
                "expected": active.generation,
                "generation": generation,
                "activation": {
                    "generation": generation,
                    "digest": crate::workflow::DEFAULT,
                    "returned": true,
                    "commit": committed.commit,
                    "previous": active,
                    "mappings": activate::recorded_mappings(&committed.returning),
                    "moved": plan.moved.len(),
                    "principal": by,
                    "at": at,
                },
                "moved": moved,
                "kept": kept_vars(&plan),
                "seen": seen,
                "at": at,
            });
            match self.retrying(|| {
                self.transact::<Value>(
                    both(sql::RETURN_DEFAULT, pg::return_default()),
                    vars.clone(),
                )
            }) {
                Ok(_) => return Ok(Followed::Returned { generation }),
                Err(Error::Refused(why))
                    if why.starts_with("stale_task:") || why.starts_with("stale_workflow:") => {}
                Err(other) => return Err(other),
            }
        }
        Err(refusal.map_or_else(
            || {
                Error::Refused(format!(
                    "workflow: the board changed under each of {PLANS} plans of its return to its \
                     own writers; nothing was written, and the active workflow stays"
                ))
            },
            refused_workflow,
        ))
    }

    /// What an activation plans from: the active generation and the last one recorded, each
    /// task a workflow holds or the board's writers keep open, with the roles held on it, the
    /// block record each carries, by its slug, and each executor or reviewer read as lost.
    fn standings(&self) -> Result<Planned, Error> {
        // The versions first, the holders after: a claim between the two is seen by the
        // second, or fences the write by the version it moved.
        let mut read: Standings =
            self.retrying(|| self.transact(both(sql::STANDINGS, pg::standings()), json!({})))?;
        let board = self.board()?;
        let read_slug = |slug: &str| read.tasks.iter().any(|t| t.slug == slug);
        let seen: Vec<Value> = (board.tasks.iter())
            .flat_map(|t| t.roles.iter().map(move |r| (t, r)))
            .filter(|(_, r)| matches!(r.role.as_str(), "executor" | "reviewer"))
            .filter(|(t, r)| !r.health.holds() && read_slug(&t.slug))
            .filter_map(|(_, r)| board.sessions.iter().find(|s| s.row.handle == r.handle))
            .map(|s| json!({"handle": s.row.handle, "last_seen": s.row.last_seen}))
            .collect();
        let mut blocks = std::collections::BTreeMap::new();
        let tasks = (std::mem::take(&mut read.tasks).into_iter())
            .map(|task| {
                if let Some(record) = task.blocked {
                    blocks.insert(task.slug.clone(), record);
                }
                let roles = (board.tasks.iter())
                    .find(|t| t.slug == task.slug)
                    .map(|t| {
                        (t.roles.iter())
                            .map(|r| (r.handle.clone(), r.role.clone(), r.health.holds()))
                            .collect()
                    })
                    .unwrap_or_default();
                crate::workflow::activate::Standing {
                    slug: task.slug,
                    version: task.version,
                    status: task.status,
                    position: task.position,
                    blocked_from: task.blocked_from,
                    roles,
                    accepted: task.accepted,
                    attempts: task.attempts,
                    retired: task.retired,
                }
            })
            .collect();
        Ok(Planned {
            read,
            tasks,
            blocks,
            seen,
        })
    }

    /// Whether the board moved on from the read `tasks` came from: a task's version moved, or one
    /// was filed or reopened. Its tasks are read before their holders ([`Store::standings`]), so a
    /// write between the two (a claim) shows a task as no write left it; a plan refused on that
    /// read is planned again rather than refused, and its refusal is said only where every plan
    /// was.
    fn moved_since(&self, tasks: &[crate::workflow::activate::Standing]) -> Result<bool, Error> {
        let now: Standings =
            self.retrying(|| self.transact(both(sql::STANDINGS, pg::standings()), json!({})))?;
        Ok(now.tasks.len() != tasks.len()
            || (now.tasks.iter().zip(tasks))
                .any(|(now, then)| now.slug != then.slug || now.version != then.version))
    }

    /// Of `blocks`, the block records still standing on the tasks `plan` returns blocked: a record
    /// the task has stood blocked under since it was written, every transition recorded at or
    /// after its time leaving the task blocked. A workflow's edges never write the record, so one
    /// a task carried out of blocked and back in by them says why it waited before, not now.
    fn standing_blocks(
        &self,
        mut blocks: std::collections::BTreeMap<String, Value>,
        plan: &crate::workflow::activate::Plan,
    ) -> Result<std::collections::BTreeMap<String, Value>, Error> {
        let returning: Vec<(&str, u64)> = (plan.moved.iter())
            .filter(|m| m.transition.status == "blocked")
            .filter(|m| blocks.contains_key(&m.transition.slug))
            .map(|m| (m.transition.slug.as_str(), m.expected))
            .collect();
        blocks.retain(|slug, _| returning.iter().any(|(s, _)| s == slug));
        let ids = (returning.iter())
            .flat_map(|&(slug, revision)| (1..=revision).map(move |r| event(slug, r)))
            .collect();
        let read = self.events(ids)?;
        for body in read.as_array().into_iter().flatten() {
            let Some(slug) = body.get("slug").and_then(Value::as_str) else {
                continue;
            };
            let since = |record: &Value| {
                (record.get("at").and_then(Value::as_f64))
                    .is_none_or(|written| body["at"].as_f64().is_none_or(|at| at >= written))
            };
            if body["status"] != "blocked" && blocks.get(slug).is_some_and(since) {
                blocks.remove(slug);
            }
        }
        Ok(blocks)
    }

    /// The definition `active` names, which an activation recorded.
    fn active_definition(&self, active: &Active) -> Result<Definition, Error> {
        (self.workflow_definition(&active.digest)?)
            .map(|recorded| recorded.definition)
            .ok_or_else(|| {
                Error::Db(format!(
                    "the active workflow's definition {} is not recorded",
                    active.digest
                ))
            })
    }

    /// `custom` activated as the next generation, in one write: its definition recorded under its
    /// digest, every task a workflow holds or the board's writers keep open placed on it
    /// ([`crate::workflow::activate::plan`]), each move recorded, and the activation recorded with
    /// the mappings it applied. Unchanged when its digest is active already. Refused whole, the
    /// previous generation left active and nothing written, when the host does not support it
    /// ([`crate::workflow::activate::supported`]) or a task cannot be placed; planned again when
    /// the board changed between the read and the write (a task it read, an executor or reviewer
    /// it read as lost beating, a task filed or reopened that it did not plan), or another
    /// activation won.
    pub fn activate_workflow(
        &self,
        custom: &crate::workflow::Custom,
        host: &crate::workflow::Host,
    ) -> Result<Followed, Error> {
        use crate::workflow::activate;
        let digest = &custom.revision.digest;
        // What each write opening asks first, in one small read: is it the active one already. A
        // commit that changed something else names another revision of the same workflow, which
        // stays active as it was: only a commit that changes the workflow activates.
        // The active generation and the revision its definition was recorded with.
        let mut body: Value =
            self.retrying(|| self.read(both(sql::ACTIVE_SOURCE, pg::active_source()), json!({})))?;
        if let Some(active) = decoded::<Active>(body["active"].take(), "active workflow")?
            && self.activated_as(&active, &body["revision"], custom)?
        {
            return Ok(Followed::Unchanged(active));
        }
        activate::supported_with(&custom.definition, host, &custom.authorities)
            .and_then(|()| activate::bound(custom))
            .map_err(refused_workflow)?;
        let by = crate::workflow::board_service("activation");
        let mut refusal = None;
        for _ in 0..PLANS {
            let Planned {
                read, tasks, seen, ..
            } = self.standings()?;
            if let Some(active) = &read.active
                && self.activated_as(active, &Value::Null, custom)?
            {
                return Ok(Followed::Unchanged(active.clone()));
            }
            let old = (read.active.as_ref())
                .map(|active| self.active_definition(active))
                .transpose()?;
            let expected = read.active.as_ref().map_or(0, |a| a.generation);
            // After a return none is active, and the generations go on from its own.
            let generation = read.last + 1;
            let at = self.now();
            let plan = match activate::plan(custom, old.as_ref(), &tasks, generation, &by, at) {
                Ok(plan) => plan,
                Err(why) if self.moved_since(&tasks)? => {
                    refusal = Some(why);
                    continue;
                }
                Err(why) => return Err(refused_workflow(why)),
            };
            let moved: Vec<Value> = (plan.moved.iter())
                .map(|m| {
                    json!({
                        "slug": m.transition.slug, "version": m.version, "expected": m.expected,
                        "transition": m.transition, "position": m.transition.position(),
                    })
                })
                .collect();
            let vars = json!({
                "expected": expected,
                "generation": generation,
                "digest": digest,
                "definition": Recorded {
                    definition: custom.definition.definition().clone(),
                    revision: custom.revision.clone(),
                },
                "activation": {
                    "generation": generation,
                    "digest": digest,
                    "revision": custom.revision,
                    "previous": read.active,
                    "mappings": activate::recorded_mappings(&custom.mappings),
                    "authorities": custom.authorities,
                    "project": custom.project,
                    "moved": plan.moved.len(),
                    "principal": by,
                    "at": at,
                },
                "moved": moved,
                "kept": kept_vars(&plan),
                "seen": seen,
                "at": at,
            });
            match self.retrying(|| {
                self.transact::<Value>(both(sql::ACTIVATE, pg::activate()), vars.clone())
            }) {
                Ok(_) => {
                    return Ok(Followed::Activated(Active {
                        generation,
                        digest: digest.clone(),
                    }));
                }
                Err(Error::Refused(why))
                    if why.starts_with("stale_task:") || why.starts_with("stale_workflow:") => {}
                Err(other) => return Err(other),
            }
        }
        Err(refusal.map_or_else(
            || {
                Error::Refused(format!(
                    "workflow: the board changed under each of {PLANS} plans of its activation; \
                     nothing was activated, and the previous workflow stays"
                ))
            },
            refused_workflow,
        ))
    }

    /// The active custom workflow, or none on a board that never activated one or returned to its
    /// own writers.
    pub fn workflow_active(&self) -> Result<Option<Active>, Error> {
        let body: Value =
            self.retrying(|| self.read(both(sql::ACTIVE, pg::active()), json!({})))?;
        decoded(body, "active workflow")
    }

    /// Whether `active` is `custom` already: its revision, the project file it was read from in
    /// the same format (`recorded`, its definition's revision as recorded, null where it was not
    /// read), or a definition and mappings that are `custom`'s, recorded from another commit. The
    /// digest names the commit as well as the workflow, and the file holds the mappings as well as
    /// the definition, so the definition and the mappings are read only where the file's bytes
    /// differ.
    fn activated_as(
        &self,
        active: &Active,
        recorded: &Value,
        custom: &crate::workflow::Custom,
    ) -> Result<bool, Error> {
        let theirs = &custom.revision;
        if active.digest == theirs.digest
            || (recorded["format"] == theirs.format
                && recorded["source"]["project_blob"] == theirs.source.project_blob.as_str())
        {
            return Ok(true);
        }
        let Some(recorded) = self.workflow_definition(&active.digest)? else {
            return Ok(false);
        };
        if recorded.revision.format != custom.revision.format
            || recorded.definition != *custom.definition.definition()
        {
            return Ok(false);
        }
        let mappings = self
            .workflow_mappings(active.generation)?
            .unwrap_or_default();
        let (project, authorities) = self.workflow_authorities_of(active.generation)?;
        // Each maps a pair once (`Validated::check_mappings`), so as maps they are equal or not.
        Ok(
            crate::workflow::mapped(&mappings) == crate::workflow::mapped(&custom.mappings)
                && authorities == custom.authorities
                && project == custom.project,
        )
    }

    /// The project and the approval authorities, sorted by adapter, activation `generation`
    /// recorded: none for an activation that committed none, or one not recorded.
    pub fn workflow_authorities_of(
        &self,
        generation: u64,
    ) -> Result<(Option<String>, Vec<crate::workflow::Attester>), Error> {
        #[derive(Deserialize)]
        struct Recorded {
            project: Option<String>,
            #[serde(default)]
            authorities: Vec<crate::workflow::Attester>,
        }
        let body: Value = self.retrying(|| {
            self.read(
                both(sql::AUTHORITIES, pg::authorities()),
                json!({"generation": generation}),
            )
        })?;
        Ok(decoded::<Recorded>(body, "workflow authorities")?
            .map_or((None, Vec::new()), |r| (r.project, r.authorities)))
    }

    /// The mappings activation `generation` recorded, or none for an activation not recorded.
    pub fn workflow_mappings(
        &self,
        generation: u64,
    ) -> Result<Option<Vec<crate::workflow::Mapping>>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(sql::MAPPINGS, pg::mappings()),
                json!({"generation": generation}),
            )
        })?;
        decoded(body, "workflow mappings")
    }

    /// The active workflow as a write that reopens a task with no position places it
    /// ([`crate::workflow::activate::Placing`]), or none on a board that never activated one.
    pub(in crate::store) fn placing_context(
        &self,
    ) -> Result<Option<crate::workflow::activate::Placing>, Error> {
        let Some(active) = self.workflow_active()? else {
            return Ok(None);
        };
        let unrecorded = |what: &str| {
            Error::Db(format!(
                "the active workflow's {what} (generation {}) is not recorded",
                active.generation
            ))
        };
        let recorded =
            (self.workflow_definition(&active.digest)?).ok_or_else(|| unrecorded("definition"))?;
        let mappings =
            (self.workflow_mappings(active.generation)?).ok_or_else(|| unrecorded("activation"))?;
        Ok(Some(crate::workflow::activate::Placing {
            definition: recorded.definition,
            mappings,
            generation: active.generation,
            digest: active.digest,
        }))
    }

    /// The definition recorded under `digest`, or none.
    pub fn workflow_definition(&self, digest: &str) -> Result<Option<Recorded>, Error> {
        let body: Value = self.retrying(|| {
            self.read(
                both(sql::DEFINITION, pg::definition()),
                json!({"digest": digest}),
            )
        })?;
        decoded(body, "workflow definition")
    }

    /// The transitions recorded for each of `tasks`, a slug with the workflow revision its
    /// position stands at, as the pipeline alarm reads them ([`Moved`]): in the order the tasks
    /// are given, each task's oldest first, every one from its first revision, each read by its
    /// id, never a scan of the record.
    pub fn transitions(&self, tasks: &[(&str, u64)]) -> Result<Vec<Moved>, Error> {
        let ids = (tasks.iter())
            .flat_map(|&(slug, revision)| (1..=revision).map(move |r| event(slug, r)))
            .collect();
        let read = self.events(ids)?;
        let unread = || {
            Error::Db(format!(
                "recorded transitions the alarm cannot read: {read}"
            ))
        };
        let bodies = read.as_array().ok_or_else(unread)?;
        bodies
            .iter()
            .map(|body| Moved::of(body).ok_or_else(unread))
            .collect()
    }

    /// The accepted output standing for each of `tasks`, a slug with the workflow revision its
    /// position stands at: the `accepted` the transition that made that revision left
    /// (docs/WORKFLOW.md, "Accepted outputs"), with its slug and revision, in the order the tasks
    /// are given, none for a task whose transition left none, each read by its id.
    pub fn accepted_standing(
        &self,
        tasks: &[(&str, u64)],
    ) -> Result<Vec<(String, u64, String)>, Error> {
        let ids = (tasks.iter())
            .filter(|&&(_, revision)| revision > 0)
            .map(|&(slug, revision)| event(slug, revision))
            .collect();
        let read = self.events(ids)?;
        Ok((read.as_array().into_iter().flatten())
            .filter_map(|body| {
                Some((
                    body.get("slug")?.as_str()?.to_string(),
                    body.get("revision")?.as_u64()?,
                    body.get("accepted")?.as_str()?.to_string(),
                ))
            })
            .collect())
    }

    /// The bodies of the transitions `ids` name, in their order, one no transition has left out:
    /// each read by its id, never a scan of the record. Built as plain values, and read as one, so
    /// no serializer or list reader of their own links where a PULL is composed
    /// (tools/size-budget.toml).
    fn events(&self, ids: Vec<Value>) -> Result<Value, Error> {
        if ids.is_empty() {
            return Ok(Value::Array(Vec::new()));
        }
        let vars = Value::Object(
            [("ids".to_string(), Value::Array(ids))]
                .into_iter()
                .collect(),
        );
        self.retrying(|| self.read(both(sql::TRANSITIONS, pg::transitions()), vars.clone()))
    }

    /// `slug`'s workflow position, or none for a task the board's own writers keep; refused for
    /// a task that is not there.
    pub fn workflow_position(&self, slug: &str) -> Result<Option<TaskPosition>, Error> {
        let body: Value = self.retrying(|| {
            self.transact(both(sql::POSITION, pg::position()), json!({"slug": slug}))
        })?;
        decoded(body, "workflow position")
    }

    /// `transition` recorded and its task put where it goes, in one write, when the task is at
    /// workflow revision `expected` (0 for a task no workflow held yet); refused as `stale_task`
    /// at any other: the fences' tests' own write of a position. The board writes one only in an
    /// activation (`Store::activate_workflow`), an admitted operation (`Store::apply_operation`)
    /// and a filing (`Store::placing`).
    #[cfg(test)]
    pub(crate) fn record_transition(
        &self,
        transition: &Transition,
        expected: u64,
    ) -> Result<(), Error> {
        let vars = json!({
            "slug": transition.slug,
            "expected": expected,
            "transition": transition,
            "position": transition.position(),
        });
        self.retrying(|| self.transact::<Value>(both(sql::RECORD, pg::record()), vars.clone()))
            .map(drop)
    }
}

/// Whether a custom workflow holds `task`: its row carries a workflow position.
pub(super) fn held(task: &crate::rows::TaskRow) -> bool {
    position_held(&task.fields)
}

/// Whether a task's board fields hold a workflow position: a task a custom workflow holds, which
/// only its edges and its recovery move and staff.
pub(crate) fn position_held(
    fields: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>,
) -> bool {
    (fields.get("_board")).is_some_and(|board| board.contains_key("workflow"))
}

/// The position a task's board fields hold, none for a task the board's own writers keep (or one
/// whose position is not in the board's shape, which the database's fences never write).
pub(crate) fn task_position(
    fields: &std::collections::BTreeMap<String, std::collections::BTreeMap<String, Value>>,
) -> Option<TaskPosition> {
    TaskPosition::of(fields.get("_board")?.get("workflow")?)
}

/// The id of the transition that made workflow revision `revision` of `slug`.
fn event(slug: &str, revision: u64) -> Value {
    Value::from(vec![json!("event"), slug.into(), revision.into()])
}

/// `value`'s fields: none where it is no object, or names a field `allowed` does not.
fn known<'v>(value: &'v Value, allowed: &[&str]) -> Option<&'v serde_json::Map<String, Value>> {
    let fields = value.as_object()?;
    (fields.keys())
        .all(|key| allowed.contains(&key.as_str()))
        .then_some(fields)
}

/// The text `fields` holds under `key`.
fn text(fields: &serde_json::Map<String, Value>, key: &str) -> Option<String> {
    Some(fields.get(key)?.as_str()?.to_string())
}

/// The refusal of a legacy move or staffing of `slug`, a task a custom workflow holds.
pub(crate) fn explicit_action(slug: &str) -> String {
    format!(
        "explicit_action_required: a custom workflow holds {slug}, which moves and is staffed by \
         its edges alone; read them with `just tasks workflow actions {slug}` and take one with \
         `just tasks workflow apply {slug} EDGE`"
    )
}

/// How the database refuses a write that takes `slug`, a task with no workflow position, out of
/// done or cancelled while a custom workflow is active (`workflow_position`), up to the status it
/// left: [`crate::Store`]'s rule then places it in the write that moves it.
pub(crate) fn unplaced(slug: &str) -> String {
    format!(
        "explicit_action_required: {slug} has no workflow position, and a custom workflow is \
         active: it leaves "
    )
}

/// The block record of a task returning blocked from `from`, at `at`: the one it `carried` that
/// still stands ([`Store::standing_blocks`]), what it waits on, why and who blocked it kept, or one
/// waiting on `parked` where none does; its source is the status the return gives it either way.
fn blocked(carried: Option<&Value>, from: Option<String>, at: f64) -> crate::waits_on::Blocked {
    let kept =
        carried.and_then(|record| crate::objects::read::<crate::waits_on::Blocked>(record).ok());
    match kept {
        Some(kept) => crate::waits_on::Blocked { from, ..kept },
        None => crate::waits_on::Blocked {
            on: "parked".into(),
            why: Some("returned from a custom workflow".into()),
            by: "board".into(),
            at,
            from,
        },
    }
}

/// The tasks a plan only fences, as its write reads them.
fn kept_vars(plan: &crate::workflow::activate::Plan) -> Vec<Value> {
    (plan.kept.iter())
        .map(|(slug, version)| json!({"slug": slug, "version": version}))
        .collect()
}

/// An activation refused in the planner's words, which name the workflow where they do not.
fn refused_workflow(why: String) -> Error {
    Error::Refused(if why.starts_with("workflow") {
        why
    } else {
        format!("workflow: {why}")
    })
}

/// `body` as `T`, none for null; a body in another shape is the database's error, named `what`.
fn decoded<T: serde::de::DeserializeOwned>(body: Value, what: &str) -> Result<Option<T>, Error> {
    if body.is_null() {
        return Ok(None);
    }
    crate::objects::read(&body)
        .map(Some)
        .map_err(|e| Error::Db(format!("a {what} not in the board's shape: {e}")))
}

/// The order of two workflow record keys, the parts of their ids, as SurrealDB orders compound
/// ids: part by part, a number by its value and a text by its bytes, a shorter key first where
/// one is the start of the other. PostgreSQL's jsonb orders arrays by length first, so the dump
/// sorts by this, and a board's export is one text in either database.
pub(crate) fn key_order(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Array(a), Value::Array(b)) => {
            for (x, y) in a.iter().zip(b) {
                match key_order(x, y) {
                    Ordering::Equal => {}
                    other => return other,
                }
            }
            a.len().cmp(&b.len())
        }
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap_or(0.0), y.as_f64().unwrap_or(0.0));
            x.partial_cmp(&y).unwrap_or(Ordering::Equal)
        }
        (Value::String(x), Value::String(y)) => x.cmp(y),
        // Numbers before texts before anything else, as SurrealDB sorts kinds apart.
        (x, y) => rank(x).cmp(&rank(y)),
    }
}

fn rank(value: &Value) -> u8 {
    match value {
        Value::Null => 0,
        Value::Bool(_) => 1,
        Value::Number(_) => 2,
        Value::String(_) => 3,
        Value::Array(_) => 4,
        Value::Object(_) => 5,
    }
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "activation_tests.rs"]
mod activation_tests;
