//! Durable execution attempts ([docs/engineering/EXECUTION_ATTEMPTS.md](../../../docs/engineering/EXECUTION_ATTEMPTS.md)):
//! the board's record of each time a task's role ran on a provider, and the rules that recover it
//! and decide which of its results may count. A project supplies the provider, as a kit
//! [`Adapter`], and the host that runs the [`Coordinator`]; the board keeps the history.
//!
//! An attempt is admitted in one transaction with its first operation recorded as owed, its lane
//! taken (one live attempt per task and lane), a request key that admits the same attempt again,
//! and a reservation in the host's capacity scope, which holds until it is known that nothing of
//! the attempt runs. Its identity, the task's input pinned among it, is never written again; a
//! retry or a resume is another attempt, linked to it.
//!
//! A worker acts for an attempt under a lease the database's clock times, each taking one at the
//! next epoch, and records an operation as sent before it sends it; its answer is written only
//! while that lease still holds, else kept as a stale fact that changes nothing. An answer that
//! settles nothing is reconciled by its key, never sent anew, and after a bound the attempt waits
//! on a person, who alone may admit a retry. A cancel asked for bars every result of the attempt
//! from the moment it is written. A provider's answer completes no task, grants no stamp and moves
//! no workflow: [`Coordinator::eligibility`] says only whether an attempt's result may be put
//! forward as evidence, by the rules that already decide that.
//!
//! Authority is the host's: every call takes a [`Grant`], which the host builds after it
//! authenticated its caller and which is never read from a request. A [`Principal`] in a grant or
//! a record is who acted, never what they may do.

mod coordinator;
mod evidence;
mod keys;
mod reader;
mod record;
mod rules;
mod view;

pub use coordinator::Coordinator;
pub use reader::Reader;
/// Named by the JSON reader's tests, which write an attempt's row whole ([`crate::objects`]).
#[cfg(test)]
pub(crate) use record::Pin;
pub(crate) use record::Row;
pub use record::{
    Artifact, Asked, Binding, Cancellation, Cleanup, Collection, Continuation, Dispatch, Ending,
    Fact, Identity, Lease, Pending, Relation, State,
};
pub use symphony_plugin::execution::{
    Adapter, AttemptRef, Capabilities, ContentRef, ExternalLink, Input, Operation, Outcome,
    Principal, PrincipalKind, Profile, ResumeMode, Scope, StopMode,
};
pub use view::{board_scope, principal_word, resume_mode, resume_word, stop_mode, stop_word};

use crate::Error;
use std::fmt;
use std::time::Duration;

/// What a caller may ask of a [`Coordinator`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum Act {
    /// Admit an attempt.
    Start,
    Cancel,
    Stop,
    /// Admit an attempt that continues a paused or stopped one.
    Resume,
    /// Admit a new execution after a held or ended one: a person's decision alone.
    Retry,
    /// Read attempts, their facts and artifacts.
    Read,
    /// Resolve a view of an attempt's resource.
    Link,
    /// Report an attempt's collected artifacts.
    Collect,
    /// Report an attempt's resource cleaned up.
    Cleanup,
    /// Act as the worker that sends an attempt's operations.
    Work,
}

/// What a host established for one caller: the principal it authenticated, the board it may act
/// on, the tasks and acts its policy grants, and that policy's revision.
///
/// Built only by the host, after authentication, with [`Grant::from_host`]: it is never
/// deserialized, so no request's payload, query or claimed handle can become one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Grant {
    principal: Principal,
    scope: Scope,
    tasks: Option<Vec<String>>,
    acts: Vec<Act>,
    policy_revision: String,
}

impl Grant {
    /// `principal` may do `acts` on every task of `scope`'s board, under `policy_revision`.
    pub fn from_host(
        principal: Principal,
        scope: Scope,
        acts: &[Act],
        policy_revision: impl Into<String>,
    ) -> Grant {
        Grant {
            principal,
            scope,
            tasks: None,
            acts: acts.to_vec(),
            policy_revision: policy_revision.into(),
        }
    }

    /// The same grant, on `tasks` alone.
    #[must_use]
    pub fn on_tasks(mut self, tasks: &[&str]) -> Grant {
        self.tasks = Some(tasks.iter().map(|task| (*task).to_string()).collect());
        self
    }

    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    pub fn policy_revision(&self) -> &str {
        &self.policy_revision
    }

    /// Whether it lets its principal do `act` on `task` of `scope`, or on the whole board with no
    /// task named: a grant on some tasks alone gives nothing board-wide.
    pub(crate) fn allows(
        &self,
        scope: &Scope,
        task: Option<&str>,
        act: Act,
    ) -> Result<(), Refusal> {
        if self.scope != *scope {
            return Err(Refusal::Scope);
        }
        let unnamed = self.principal.issuer.is_empty()
            || self.principal.subject.is_empty()
            || self.policy_revision.is_empty();
        let on_task = match (&self.tasks, task) {
            (None, _) => true,
            (Some(tasks), Some(task)) => tasks.iter().any(|allowed| allowed == task),
            (Some(_), None) => false,
        };
        if unnamed || !on_task || !self.acts.contains(&act) {
            return Err(Refusal::Unauthorized(format!(
                "{} may not {act:?} {}",
                self.principal.subject,
                task.map_or_else(|| "on this board".to_string(), |task| format!("on {task}"))
            )));
        }
        Ok(())
    }
}

/// Why the coordinator did not do what it was asked. Nothing was written, but where it says so.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Refusal {
    /// The grant does not let its principal do it.
    Unauthorized(String),
    /// Another project's or board's: no grant or attempt crosses them.
    Scope,
    /// There is no such attempt on this board.
    Unknown(String),
    /// The lane holds this live attempt (or not the predecessor named).
    Busy(Option<String>),
    /// The capacity scope is full of these attempts.
    Full(Vec<String>),
    /// A key or a name already stands for something else.
    Conflict(String),
    /// What was asked does not hold for this attempt, or is malformed.
    Invalid(String),
    /// The provider gave no usable answer to a call made at once (a link).
    Unavailable(String),
    /// The board could not be read or written.
    Board(Error),
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::Unauthorized(why) => write!(f, "not authorized: {why}"),
            Refusal::Scope => f.write_str("another project's or board's attempt"),
            Refusal::Unknown(attempt) => write!(f, "there is no execution attempt {attempt}"),
            Refusal::Busy(Some(attempt)) => write!(f, "the lane holds attempt {attempt}"),
            Refusal::Busy(None) => f.write_str("the lane holds no attempt to follow"),
            Refusal::Full(held) => write!(f, "capacity is full: {}", held.join(", ")),
            Refusal::Conflict(why) => write!(f, "conflict: {why}"),
            Refusal::Invalid(why) => write!(f, "refused: {why}"),
            Refusal::Unavailable(why) => write!(f, "unavailable: {why}"),
            Refusal::Board(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for Refusal {}

impl From<Error> for Refusal {
    fn from(error: Error) -> Refusal {
        Refusal::Board(error)
    }
}

/// How a coordinator paces its worker: its lease, each call's budget, how often it observes a
/// running attempt, and how far recovery goes before the attempt waits on a person.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Pacing {
    /// How long a worker holds an attempt without writing: longer than `call`.
    pub lease: Duration,
    /// Each provider call's budget (at most the kit's 30 s).
    pub call: Duration,
    pub observe: Duration,
    /// The first wait after an answer that settled nothing, doubled after each up to `backoff_cap`.
    pub backoff: Duration,
    pub backoff_cap: Duration,
    /// Answers in a row that settle nothing, after which the attempt is held.
    pub unknowns: u32,
}

impl Default for Pacing {
    fn default() -> Pacing {
        let bounds = rules::Bounds::default();
        Pacing {
            lease: Duration::from_secs(60),
            call: Duration::from_secs(10),
            observe: Duration::from_millis(bounds.observe_ms),
            backoff: Duration::from_millis(bounds.backoff_ms),
            backoff_cap: Duration::from_millis(bounds.backoff_cap_ms),
            unknowns: bounds.unknowns,
        }
    }
}

/// An attempt asked for: the task and lane it runs in, its role and handle, the task's input it
/// runs on and the caller's request key.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Admission {
    pub task: String,
    pub lane: String,
    pub role: String,
    pub handle: Option<String>,
    pub input: Input,
    pub request_key: Option<String>,
}

impl Admission {
    pub fn new(task: &str, lane: &str, role: &str, input: Input) -> Admission {
        Admission {
            task: task.into(),
            lane: lane.into(),
            role: role.into(),
            handle: None,
            input,
            request_key: None,
        }
    }

    #[must_use]
    pub fn handle(mut self, handle: &str) -> Admission {
        self.handle = Some(handle.into());
        self
    }

    /// The same request key admits the same attempt again, however many times it is sent.
    #[must_use]
    pub fn request_key(mut self, key: &str) -> Admission {
        self.request_key = Some(key.into());
        self
    }
}

/// An attempt as a reader sees it.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Attempt {
    /// Its UUID, 32 hex digits.
    pub id: String,
    /// When it was admitted, by the database's clock (ms).
    pub created: i64,
    pub identity: Identity,
    pub state: State,
    pub revision: i64,
    /// Whether it is its lane's current attempt: a later one supersedes it.
    pub current: bool,
}

impl Attempt {
    /// Whether a resume in `mode` may continue it now, and why not: it must be known paused or
    /// ended with its resource bound, owe nothing, have no cancel whose outcome is uncertain and no
    /// successor, and a resume that reuses its resource needs that resource's cleanup not begun.
    /// [`Coordinator::resume`] decides again on the row it writes.
    pub fn resumable(&self, mode: ResumeMode) -> Result<(), String> {
        rules::resumable(&self.state, mode == ResumeMode::ReuseWithNewGeneration).map(|_| ())
    }
}

/// A fact as it was appended.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Recorded {
    pub seq: u64,
    /// When, by the database's clock (ms).
    pub at: i64,
    pub fact: Fact,
}

/// Whether an attempt's result may be put forward as the task's evidence, and why not.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub struct Eligibility {
    pub eligible: bool,
    pub reasons: Vec<String>,
}

/// What a worker's step did for one attempt.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Worked {
    /// Another worker holds its lease.
    Leased,
    /// Nothing is owed: it ended, or waits on a person or the host.
    Waited,
    /// It was held for a person, for this reason, with no I/O.
    Held(String),
    /// An operation was sent (or reconciled, or the resource observed) and its answer recorded.
    Answered {
        operation: Operation,
        outcome: Outcome,
    },
    /// The answer came after this worker's lease was gone: kept as a stale fact, acted on by no
    /// one.
    Fenced {
        operation: Operation,
        outcome: Outcome,
    },
}

/// One attempt [`Coordinator::run_due`] stepped, and what its step did.
pub type Stepped = (String, Result<Worked, Refusal>);

/// What reporting an artifact did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Collected {
    /// Kept as the attempt's.
    Kept,
    /// The same artifact was kept already.
    Already,
    /// Its producer was not the attempt's current generation or runtime: kept as a stale fact
    /// alone, never as the attempt's artifact.
    Stale,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_refusal_says_what_was_refused_and_carries_a_board_error_as_it_was() {
        let said = |refusal: Refusal| refusal.to_string();
        assert_eq!(
            said(Refusal::Unauthorized("no grant".into())),
            "not authorized: no grant"
        );
        assert_eq!(said(Refusal::Scope), "another project's or board's attempt");
        assert_eq!(
            said(Refusal::Unknown("ab".into())),
            "there is no execution attempt ab"
        );
        assert_eq!(
            said(Refusal::Busy(Some("ab".into()))),
            "the lane holds attempt ab"
        );
        assert_eq!(
            said(Refusal::Busy(None)),
            "the lane holds no attempt to follow"
        );
        assert_eq!(
            said(Refusal::Full(vec!["a".into(), "b".into()])),
            "capacity is full: a, b"
        );
        assert_eq!(said(Refusal::Conflict("taken".into())), "conflict: taken");
        assert_eq!(said(Refusal::Invalid("blank".into())), "refused: blank");
        assert_eq!(
            said(Refusal::Unavailable("down".into())),
            "unavailable: down"
        );
        let error = Error::Db("lost".into());
        assert_eq!(said(Refusal::from(error.clone())), error.to_string());
        assert_eq!(Refusal::from(error.clone()), Refusal::Board(error));
    }
}
