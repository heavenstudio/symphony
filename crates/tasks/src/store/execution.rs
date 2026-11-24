//! The store's half of execution attempts ([`crate::execution`]): an attempt's row (its immutable
//! identity beside its revisioned state), its append-only facts, the fences admission writes and
//! its collected artifacts. The rules are `crate::execution`'s, which decides in the process on
//! what it read; these calls send what it decided, fenced on what it read, and compare and write
//! every time by the database's own clock.
//!
//! A write never repeats a decision made on a row that moved: [`Store::execution_update`] reads
//! the row again and asks its caller again, as every write of the board's does.

use super::postgres::execution as pg;
use super::statement::Statement;
use super::{Error, Failure, Store};
use serde::Deserialize;
use serde_json::{Value, json};

/// What an admission found ([`pg::admit`]).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(crate) enum Admission {
    /// The attempt was written.
    Admitted,
    /// The request key was admitted before, as this attempt, of this admission digest.
    Existing {
        attempt: String,
        digest: Option<String>,
    },
    /// The lane holds this open attempt, or not the predecessor named.
    Busy { attempt: Option<String> },
    /// The capacity scope is full of these attempts.
    Full { reserved: Vec<String> },
}

/// What a claim of an attempt's lease found ([`pg::claim`]).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(crate) enum Claim {
    Missing,
    /// Another worker holds it unexpired.
    Held {
        lease: Value,
        now: i64,
    },
    /// Claimed: the row as it now is, its lease this worker's.
    Claimed {
        row: Value,
        now: i64,
    },
}

/// What a fenced write of an attempt did ([`pg::commit`]).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub(crate) enum Commit {
    Missing,
    /// The lease it was written under is no longer the writer's.
    Fenced {
        now: i64,
    },
    Committed {
        revision: i64,
        now: i64,
    },
}

/// What a caller decided on the row it read: a write, or nothing, each with what it answers.
pub(crate) enum Decision<T> {
    Write(Box<Write>, T),
    Nothing(T),
}

impl<T> Decision<T> {
    /// `write`, answering `answer`.
    pub(crate) fn write(write: Write, answer: T) -> Decision<T> {
        Decision::Write(Box::new(write), answer)
    }
}

/// One write of an attempt, as its caller decided on the row it read.
#[derive(Debug, Clone, Default)]
pub(crate) struct Write {
    /// The lease (`{owner, epoch}`) it must still hold, if it acts under one.
    pub lease: Option<Value>,
    /// The new state, `open`, `live` and when its next work is due (ms from now), or none for a
    /// write of facts alone.
    pub state: Option<(Value, bool, bool, Option<u64>)>,
    /// The lease let go with the state.
    pub release_lease: bool,
    /// The facts appended, in order.
    pub facts: Vec<Value>,
    /// The capacity scope whose reservation is given back.
    pub release: Option<String>,
    /// The artifacts made, each `{name, artifact}`.
    pub artifacts: Vec<Value>,
}

/// What a caller decides on the row it read, less what it answers: the write, or none.
type Decide<'a> = dyn FnMut(&Value) -> Result<Option<Box<Write>>, Error> + 'a;

impl Store {
    /// Admit attempt `vars["attempt"]` as [`pg::admit`] says, tried again while the board moves
    /// under it. A retry or a resume names its `predecessor`, whose row `follow` is asked about on
    /// each try: the write it answers (none once it was followed) is made in the admission's own
    /// transaction, fenced on that row, so the new attempt is never admitted with its predecessor
    /// left live, nor the predecessor ended without it.
    pub(crate) fn execution_admit(
        &self,
        vars: Value,
        predecessor: Option<&str>,
        mut follow: impl FnMut(&Value) -> Result<Option<Write>, Error>,
    ) -> Result<Admission, Error> {
        self.retrying(|| {
            let mut vars = vars.clone();
            if let Some(attempt) = predecessor {
                let row: Option<Value> =
                    self.read(Statement::from(pg::attempt()), json!({"attempt": attempt}))?;
                let Some(row) = row else {
                    return Err(Failure::Stop(Error::Refused(format!(
                        "there is no execution attempt {attempt}"
                    ))));
                };
                if let Some(write) = follow(&row).map_err(Failure::Stop)? {
                    let first = row["facts"].as_i64().unwrap_or(0) + 1;
                    let (state, open, live, due_in) = write.state.unwrap_or_default();
                    vars["follow"] = json!({
                        "attempt": attempt,
                        "revision": row["revision"],
                        "facts_from": row["facts"],
                        "state": state,
                        "open": open,
                        "live": live,
                        "due_in": due_in,
                        "facts": (write.facts.into_iter().zip(first..))
                            .map(|(fact, seq)| json!({"seq": seq, "fact": fact}))
                            .collect::<Vec<_>>(),
                        "release": write.release,
                    });
                }
            }
            self.transact(Statement::from(pg::admit()), vars)
        })
    }

    /// Attempt `attempt`'s lease, taken or renewed for `owner` for `lease_ms`.
    pub(crate) fn execution_claim(
        &self,
        attempt: &str,
        owner: &str,
        lease_ms: u64,
    ) -> Result<Claim, Error> {
        let vars = json!({"attempt": attempt, "owner": owner, "lease_ms": lease_ms});
        self.retrying(|| self.transact(Statement::from(pg::claim()), vars.clone()))
    }

    /// Attempt `attempt` written as `decide` says of the row it read, with the value it answered:
    /// `decide` is asked again, on the row read again, each time the row moved under the write. A
    /// [`Decision::Nothing`] writes nothing. A write whose lease is gone is not repeated: it
    /// answers [`Commit::Fenced`].
    pub(crate) fn execution_update<T>(
        &self,
        attempt: &str,
        mut decide: impl FnMut(&Value) -> Result<Decision<T>, Error>,
    ) -> Result<(Option<Commit>, T), Error> {
        let mut answer = None;
        let committed = self.execution_written(attempt, &mut |row| {
            let (write, said) = match decide(row)? {
                Decision::Nothing(said) => (None, said),
                Decision::Write(write, said) => (Some(write), said),
            };
            answer = Some(said);
            Ok(write)
        })?;
        match answer {
            Some(answer) => Ok((committed, answer)),
            None => Err(Error::Db(format!(
                "the write of execution attempt {attempt} ended with nothing decided"
            ))),
        }
    }

    /// The one body of [`Store::execution_update`], whatever its caller answers: the row read, the
    /// write `decide` gives for it fenced on that row, none where it gives none.
    fn execution_written(
        &self,
        attempt: &str,
        decide: &mut Decide<'_>,
    ) -> Result<Option<Commit>, Error> {
        self.retrying(|| {
            let row: Option<Value> =
                self.read(Statement::from(pg::attempt()), json!({"attempt": attempt}))?;
            let Some(row) = row else {
                return Err(Failure::Stop(Error::Refused(format!(
                    "there is no execution attempt {attempt}"
                ))));
            };
            let Some(write) = decide(&row).map_err(Failure::Stop)? else {
                return Ok(None);
            };
            let write = *write;
            let mut vars = json!({
                "attempt": attempt,
                "revision": row["revision"],
                "facts_from": row["facts"],
                "lease": write.lease,
                "release_lease": write.release_lease,
                "release": write.release,
                "artifacts": write.artifacts,
            });
            let first = row["facts"].as_i64().unwrap_or(0) + 1;
            vars["facts"] = (write.facts.into_iter().zip(first..))
                .map(|(fact, seq)| json!({"seq": seq, "fact": fact}))
                .collect();
            if let Some((state, open, live, due_in)) = write.state {
                vars["state"] = state;
                vars["open"] = json!(open);
                vars["live"] = json!(live);
                vars["due_in"] = json!(due_in);
            }
            self.transact(Statement::from(pg::commit()), vars).map(Some)
        })
    }

    /// Attempt `attempt`'s row, if there is one.
    pub(crate) fn execution_attempt(&self, attempt: &str) -> Result<Option<Value>, Error> {
        self.retrying(|| self.read(Statement::from(pg::attempt()), json!({"attempt": attempt})))
    }

    /// Every attempt of task `slug`, oldest first.
    pub(crate) fn execution_of_task(&self, slug: &str) -> Result<Vec<Value>, Error> {
        self.retrying(|| self.read(Statement::from(pg::of_task()), json!({"task": slug})))
    }

    /// How many execution attempts task `slug` has had, ended ones included: whether its page has a
    /// history of them to show. A task has few, so its rows are read and counted.
    pub fn attempt_count(&self, slug: &str) -> Result<usize, Error> {
        self.execution_of_task(slug).map(|rows| rows.len())
    }

    /// The attempts of task `slug` whose runtime may still be alive (dispatched and not known to
    /// have ended, or held for a person to settle), oldest first: what a workflow change waits on
    /// before it moves the task.
    pub fn live_attempts(&self, slug: &str) -> Result<Vec<String>, Error> {
        self.retrying(|| self.read(Statement::from(pg::live()), json!({"task": slug})))
    }

    /// Up to `limit` open attempts under `configuration` whose work is due by the database's clock,
    /// longest due first.
    pub(crate) fn execution_due(
        &self,
        configuration: &str,
        limit: usize,
    ) -> Result<Vec<String>, Error> {
        let vars = json!({"configuration": configuration, "limit": limit});
        self.retrying(|| self.read(Statement::from(pg::due()), vars.clone()))
    }

    /// Attempt `attempt`'s facts, each `{seq, at, fact}`, in order.
    pub(crate) fn execution_facts(&self, attempt: &str) -> Result<Vec<Value>, Error> {
        self.retrying(|| self.read(Statement::from(pg::facts()), json!({"attempt": attempt})))
    }

    /// Attempt `attempt`'s artifacts, each `{name, at, artifact}`, by name.
    pub(crate) fn execution_artifacts(&self, attempt: &str) -> Result<Vec<Value>, Error> {
        self.retrying(|| {
            self.read(
                Statement::from(pg::artifacts()),
                json!({"attempt": attempt}),
            )
        })
    }

    /// The fence `key` (`['request', KEY]`, `['lane', TASK, LANE]` or `['capacity', SCOPE]`).
    pub(crate) fn execution_fence(&self, key: Value) -> Result<Option<Value>, Error> {
        self.retrying(|| self.read(Statement::from(pg::fence()), json!({"key": key})))
    }
}
