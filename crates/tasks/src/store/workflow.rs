//! A custom delivery workflow's records on the board (docs/WORKFLOW.md), from schema 24: the
//! active generation, the immutable definitions, activations, transitions and receipts in the
//! `workflow` table, and the position of each task the workflow holds in its
//! `fields._board.workflow`.
//!
//! Nothing but this module writes them, and the schema's fences hold every writer to that: a
//! task's position, or the status of a task that has one, changes only in the write that records
//! its transition, and a recorded definition, activation, transition or receipt is never changed
//! (`crate::schema`'s `workflow_position`, `workflow_history` and `workflow_active`, and their
//! PostgreSQL twins). `Store::raw` refuses the table, and `Store::set_field` the board's own
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

/// The exit of a holder a recovery answered (`Store::recover_holder`): an agent's own release of
/// its role, or a holder lost, by the position's configured action (docs/WORKFLOW.md, "Recovery").
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recovered {
    pub trigger: Trigger,
    pub holder: String,
    /// The role of the position's staffing the holder held there.
    pub role: String,
    /// Whether the action asks a person to recover the task by hand.
    pub manual: bool,
}

/// What a recovery answers: the position's `release` action, or its `lost_holder` one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    Release,
    LostHolder,
}

impl Trigger {
    /// Its name in the definition and on the record.
    pub fn as_str(self) -> &'static str {
        match self {
            Trigger::Release => "release",
            Trigger::LostHolder => "lost_holder",
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
    /// The idempotency key of the request that made it.
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
/// none and never activated one; nothing, as what it commits is active already; or activated it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Followed {
    Legacy,
    Unchanged(Active),
    Activated(Active),
}

/// How many times an activation is planned again after the board changed under it.
const PLANS: usize = 5;

/// What an activation reads ([`sql::STANDINGS`]).
#[derive(Deserialize)]
struct Standings {
    active: Option<Active>,
    tasks: Vec<Read>,
}

#[derive(Deserialize)]
struct Read {
    slug: String,
    version: i64,
    status: String,
    position: Option<TaskPosition>,
    blocked_from: Option<String>,
    accepted: Option<String>,
}

impl Store {
    /// The workflow `committed` sets made active on this board, as automatic activation B runs
    /// it (docs/DECISIONS.md#configurable-workflow-activation): a file with no `[workflow]` on a
    /// board that never activated one writes nothing; one that drops the workflow a board
    /// activated is refused, as a return to the board's own writers is not supported yet; a
    /// custom definition is activated unless it is the active one ([`Store::activate_workflow`]).
    pub fn follow_committed(
        &self,
        committed: &crate::workflow::Committed,
        host: &crate::workflow::Host,
    ) -> Result<Followed, Error> {
        match &committed.custom {
            Some(custom) => self.activate_workflow(custom, host),
            None => match self.workflow_active()? {
                None => Ok(Followed::Legacy),
                Some(active) => Err(Error::Refused(format!(
                    "workflow: commit {} sets no [workflow], and generation {} is active; a \
                     return to the board's own writers is not supported yet \
                     (tools/workflow-return-to-default), so the active workflow stays",
                    committed.commit, active.generation
                ))),
            },
        }
    }

    /// `custom` activated as the next generation, in one write: its definition recorded under its
    /// digest, every task a workflow holds or the board's writers keep open placed on it
    /// ([`crate::workflow::activate::plan`]), each move recorded, and the activation recorded with
    /// the mappings it applied. Unchanged when its digest is active already. Refused whole, the
    /// previous generation left active and nothing written, when the host does not support it
    /// ([`crate::workflow::activate::supported`]) or a task cannot be placed; planned again when
    /// the board changed between the read and the write (a task it read, an executor or reviewer
    /// it read as lost beating, an open task with no position filed or reopened), or another
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
        activate::supported(&custom.definition, host).map_err(refused_workflow)?;
        let by = crate::workflow::board_service("activation");
        for _ in 0..PLANS {
            // The versions first, the holders after: a claim between the two is seen by the
            // second, or fences the write by the version it moved.
            let read: Standings =
                self.retrying(|| self.transact(both(sql::STANDINGS, pg::standings()), json!({})))?;
            if let Some(active) = &read.active
                && self.activated_as(active, &Value::Null, custom)?
            {
                return Ok(Followed::Unchanged(active.clone()));
            }
            let old = match &read.active {
                None => None,
                Some(active) => Some(
                    self.workflow_definition(&active.digest)?
                        .ok_or_else(|| {
                            Error::Db(format!(
                                "the active workflow's definition {} is not recorded",
                                active.digest
                            ))
                        })?
                        .definition,
                ),
            };
            let board = self.board()?;
            // Each executor or reviewer the plan reads as no longer holding its role, fenced on its
            // session as read: one that beats before the write makes it stale, and the next plan
            // reads it live.
            let read_slug = |slug: &str| read.tasks.iter().any(|t| t.slug == slug);
            let seen: Vec<Value> = (board.tasks.iter())
                .flat_map(|t| t.roles.iter().map(move |r| (t, r)))
                .filter(|(_, r)| matches!(r.role.as_str(), "executor" | "reviewer"))
                .filter(|(t, r)| !r.health.holds() && read_slug(&t.slug))
                .filter_map(|(_, r)| board.sessions.iter().find(|s| s.row.handle == r.handle))
                .map(|s| json!({"handle": s.row.handle, "last_seen": s.row.last_seen}))
                .collect();
            let tasks: Vec<activate::Standing> = (read.tasks.into_iter())
                .map(|task| {
                    let roles = (board.tasks.iter())
                        .find(|t| t.slug == task.slug)
                        .map(|t| {
                            (t.roles.iter())
                                .map(|r| (r.handle.clone(), r.role.clone(), r.health.holds()))
                                .collect()
                        })
                        .unwrap_or_default();
                    activate::Standing {
                        slug: task.slug,
                        version: task.version,
                        status: task.status,
                        position: task.position,
                        blocked_from: task.blocked_from,
                        roles,
                        accepted: task.accepted,
                    }
                })
                .collect();
            let expected = read.active.as_ref().map_or(0, |a| a.generation);
            let generation = expected + 1;
            let at = self.now();
            let plan = activate::plan(custom, old.as_ref(), &tasks, generation, &by, at)
                .map_err(refused_workflow)?;
            let moved: Vec<Value> = (plan.moved.iter())
                .map(|m| {
                    json!({
                        "slug": m.transition.slug, "version": m.version, "expected": m.expected,
                        "transition": m.transition, "position": m.transition.position(),
                    })
                })
                .collect();
            let kept: Vec<Value> = (plan.kept.iter())
                .map(|(slug, version)| json!({"slug": slug, "version": version}))
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
                    "moved": plan.moved.len(),
                    "principal": by,
                    "at": at,
                },
                "moved": moved,
                "kept": kept,
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
        Err(Error::Refused(format!(
            "workflow: the board changed under each of {PLANS} plans of its activation; nothing \
             was activated, and the previous workflow stays"
        )))
    }

    /// The active custom workflow, or none on a board that never activated one.
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
        // Each maps a pair once (`Validated::check_mappings`), so as maps they are equal or not.
        Ok(crate::workflow::mapped(&mappings) == crate::workflow::mapped(&custom.mappings))
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
    serde_json::from_value(body)
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
