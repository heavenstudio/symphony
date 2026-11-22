//! The agent sessions `just tasks launch` started for a task: what restoring one needs.

use super::postgres::{Pg, launches as pg};
use super::statement::both;
use super::surreal::launches as sql;
use super::work::no_task;
use super::{Error, REFUSED, Store, refused, repr};
use crate::rows::TaskSessionRow;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use std::fmt::Write as _;

/// A launch as it is recorded (`Store.record_launch`'s arguments).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct NewLaunch {
    pub slug: String,
    pub handle: String,
    /// `executor` or `reviewer`.
    pub role: String,
    /// The Claude Code session id: `claude --resume` with it brings the conversation back.
    pub claude_session: String,
    pub worktree: String,
    pub branch: String,
    /// The hc terminal it runs in (a herdr pane id, `w1:p3`, in a record from before hc).
    pub pane: String,
    pub model: String,
    /// `launched`, or how else it came to be recorded (`found`: cleanup found it running).
    pub how: String,
    /// Whether the record says so on the task's thread: a session found only to be closed (the idle
    /// sweep, `crate::launch::reap`) says so in its closing line instead.
    pub log: bool,
}

impl Default for NewLaunch {
    fn default() -> NewLaunch {
        NewLaunch {
            slug: String::new(),
            handle: String::new(),
            role: String::new(),
            claude_session: String::new(),
            worktree: String::new(),
            branch: String::new(),
            pane: String::new(),
            model: String::new(),
            how: "launched".into(),
            log: true,
        }
    }
}

impl NewLaunch {
    pub fn new(slug: &str, handle: &str, role: &str) -> NewLaunch {
        NewLaunch {
            slug: slug.into(),
            handle: handle.into(),
            role: role.into(),
            ..NewLaunch::default()
        }
    }
}

/// What a launch's own record line says about it, around the agent session it records (which the
/// record's transaction settles: `SETTLED`): the words before it and after it.
fn said(launch: &NewLaunch) -> (String, String) {
    let h = &launch.handle;
    let who = if launch.how == "launched" {
        format!("launched {h}")
    } else {
        format!("found {h} running")
    };
    let before = format!(
        "{who} as {}: pane {}, agent session ",
        launch.role, launch.pane
    );
    let mut after = format!(", worktree {}", launch.worktree);
    if !launch.branch.is_empty() {
        let _ = write!(after, ", branch {}", launch.branch);
    }
    (before, after)
}

/// The variables [`sql::HOLDS`] reads, for the hold `revision` of `handle`'s session.
pub(super) fn held(vars: &mut Value, handle: &str, revision: u64) {
    if let Value::Object(vars) = vars {
        let taken =
            format!("{REFUSED}another close or a restore of {handle} took its session over");
        vars.insert("hold".into(), json!(revision));
        vars.insert("taken".into(), json!(taken));
    }
}

/// How long a close or a restore holds a session ([`Store::take_session`]) without renewing it:
/// past it, the next close or restore of that session takes it over. A close renews it at its last
/// look, right before its kill; a restore right before it starts the terminal.
pub const SESSION_LEASE_SECONDS: f64 = 300.0;

/// A session's lease as [`Store::take_session`] answers: the revision taken, or who holds it until
/// when.
#[derive(Deserialize)]
struct Lease {
    revision: Option<u64>,
    doing: Option<String>,
    until: Option<f64>,
}

/// What `CLOSURE_LINE` says of the closure of `handle`'s pane on `slug`, and why.
fn closure_vars(slug: &str, handle: &str, why: &str, restore: &str) -> Value {
    json!({
        "why": why, "restore": restore,
        "said": format!("closed the pane of {handle} ({why}); restore: {restore}"),
        "lost": format!(
            ". Its Codex thread id never arrived, so restore cannot resume its conversation: start \
             it again with just tasks launch {slug}"
        ),
    })
}

impl Store {
    /// Write down that `handle` runs `slug` as `role` in `pane`, so it can be restored: its Claude
    /// session id and worktree. One record per task and handle; recording again replaces it (a
    /// relaunch).
    pub fn record_launch(&self, launch: &NewLaunch) -> Result<TaskSessionRow, Error> {
        if launch.role != "executor" && launch.role != "reviewer" {
            return Err(refused(format!(
                "a task session is an executor or a reviewer, not {}",
                repr(&launch.role)
            )));
        }
        let row = TaskSessionRow {
            slug: launch.slug.clone(),
            handle: launch.handle.clone(),
            role: launch.role.clone(),
            claude_session: launch.claude_session.clone(),
            worktree: launch.worktree.clone(),
            branch: launch.branch.clone(),
            pane: launch.pane.clone(),
            model: launch.model.clone(),
            started: (self.clock)(),
            ..TaskSessionRow::default()
        };
        let body = sql::record_launch(launch.log);
        let (before, after) = said(launch);
        let vars = json!({
            "slug": launch.slug, "handle": launch.handle, "row": row, "now": row.started,
            "given": launch.claude_session, "said_before": before, "said_after": after,
            "no_task": format!("{REFUSED}{}", no_task(&launch.slug)),
            "no_session": format!("{REFUSED}no session {}", repr(&launch.handle)),
        });
        let claude_session = self.retrying(|| {
            self.transact::<String>(
                both(&body, pg::record_launch(launch.log)).posting_when(launch.log),
                vars.clone(),
            )
        })?;
        Ok(TaskSessionRow {
            claude_session,
            ..row
        })
    }

    /// The sessions launched for `slug`, by when they started.
    pub fn task_sessions(&self, slug: &str) -> Result<Vec<TaskSessionRow>, Error> {
        self.retrying(|| {
            self.read_together(
                super::statement::both(
                    sql::sessions_of_task(),
                    super::postgres::launches::sessions_of_task(),
                ),
                json!({"slug": slug}),
            )
        })
    }

    /// The tasks `handle` was launched for, by when it started on each.
    pub fn sessions_of(&self, handle: &str) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where(
            &sql::sessions_of_handle(),
            pg::sessions_of_handle(),
            json!({"handle": handle}),
        )
    }

    pub fn all_task_sessions(&self) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where(&sql::all_sessions(), pg::all_sessions(), json!({}))
    }

    /// Those whose pane is not closed.
    pub fn open_task_sessions(&self) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where(&sql::open_sessions(), pg::open_sessions(), json!({}))
    }

    fn task_sessions_where(
        &self,
        read: &str,
        pg: Pg<'static>,
        vars: Value,
    ) -> Result<Vec<TaskSessionRow>, Error> {
        self.retrying(|| self.read_together(both(read, pg.clone()), vars.clone()))
    }

    /// Its pane is closed. The record stays, with the line that brings it back. A Codex record whose
    /// thread is still unknown takes the one the handle's callback registered, if any (`SETTLED`);
    /// one that closes with none says on the thread, after its restore line, that no thread id ever
    /// arrived, so restore cannot resume it.
    pub fn close_task_session(
        &self,
        slug: &str,
        handle: &str,
        why: &str,
        restore: &str,
    ) -> Result<(), Error> {
        let vars = closure_vars(slug, handle, why, restore);
        self.change_task_session::<Value>(slug, handle, &sql::close(), pg::close(), vars)
            .map(|_| ())
    }

    /// [`Store::close_task_session`] of the session `record` was read as, by the close or restore
    /// holding it as `hold` ([`Store::take_session`]), only while it is still that launch, open, in
    /// that terminal: never a session restored into another terminal since, nor one another close
    /// has closed already. Whether it closed it; refused once the hold is another's ([`sql::HOLDS`]).
    pub fn close_task_session_as(
        &self,
        record: &TaskSessionRow,
        why: &str,
        restore: &str,
        hold: u64,
    ) -> Result<bool, Error> {
        let mut vars = closure_vars(&record.slug, &record.handle, why, restore);
        same_launch(&mut vars, record);
        held(&mut vars, &record.handle, hold);
        self.change_task_session(
            &record.slug,
            &record.handle,
            &sql::close_as(),
            pg::close_as(),
            vars,
        )
    }

    /// Record `launch` as a session found running in its pane and closed there at once, said in one
    /// line: the closure's ([`Store::close_task_session`]), by the close holding the session as
    /// `hold` (refused once it is another's, [`sql::HOLDS`]). Nothing is written when the record of its
    /// task and handle already names that terminal: its closure is recorded already, or it is open
    /// there. The record written, if any.
    pub fn record_found_closed(
        &self,
        launch: &NewLaunch,
        why: &str,
        restore: &str,
        hold: u64,
    ) -> Result<Option<TaskSessionRow>, Error> {
        let now = (self.clock)();
        let row = TaskSessionRow {
            slug: launch.slug.clone(),
            handle: launch.handle.clone(),
            role: launch.role.clone(),
            claude_session: launch.claude_session.clone(),
            worktree: launch.worktree.clone(),
            branch: launch.branch.clone(),
            pane: launch.pane.clone(),
            model: launch.model.clone(),
            started: now,
            closed: Some(now),
            closed_why: why.into(),
            restore: restore.into(),
        };
        let body = sql::record_found_closed();
        let mut vars = closure_vars(&launch.slug, &launch.handle, why, restore);
        if let Value::Object(more) = &mut vars {
            more.extend([
                ("slug".into(), json!(launch.slug)),
                ("handle".into(), json!(launch.handle)),
                ("row".into(), json!(row)),
                ("now".into(), json!(now)),
                (
                    "no_task".into(),
                    json!(format!("{REFUSED}{}", no_task(&launch.slug))),
                ),
                (
                    "no_session".into(),
                    json!(format!("{REFUSED}no session {}", repr(&launch.handle))),
                ),
            ]);
        }
        held(&mut vars, &launch.handle, hold);
        let kept: Option<String> = self.retrying(|| {
            self.transact(
                both(&body, pg::record_found_closed()).posting(),
                vars.clone(),
            )
        })?;
        Ok(kept.map(|claude_session| TaskSessionRow {
            claude_session,
            ..row
        }))
    }

    /// It runs again, in `pane`.
    pub fn reopen_task_session(&self, slug: &str, handle: &str, pane: &str) -> Result<(), Error> {
        let body = sql::reopen();
        let said = format!("restored {handle} in pane {pane}");
        self.change_task_session::<Value>(
            slug,
            handle,
            &body,
            pg::reopen(),
            json!({"pane": pane, "said": said}),
        )
        .map(|_| ())
    }

    /// [`Store::reopen_task_session`] in `pane` of the session `record` was read as, by the restore
    /// holding it as `hold` ([`Store::take_session`]), only while it is still that launch, closed,
    /// in the terminal it was read in: never one a close recorded in another terminal since (the
    /// new one, killed as it started). The session is revived with it ([`Store::revive`]): cleanup
    /// leaves handles that closed, and the restored agent must be able to use the board. Whether it
    /// reopened it; refused once the hold is another's ([`sql::HOLDS`]).
    pub fn reopen_task_session_as(
        &self,
        record: &TaskSessionRow,
        pane: &str,
        hold: u64,
    ) -> Result<bool, Error> {
        let handle = &record.handle;
        let mut vars = json!({"into": pane, "said": format!("restored {handle} in pane {pane}")});
        same_launch(&mut vars, record);
        held(&mut vars, handle, hold);
        let body = sql::reopen_as();
        self.change_task_session(&record.slug, handle, &body, pg::reopen_as(), vars)
    }

    /// Take `handle`'s session for `doing` (`a close by ACTOR`, `a restore`): the lease a close and a
    /// restore of one session each hold for their whole run, the row
    /// `callback_lease:['session', HANDLE]`. Refused while another holds it and it has not lapsed
    /// ([`SESSION_LEASE_SECONDS`]), so a close or a restore stopped part way holds it no longer than
    /// that. The revision that names this hold: [`Store::renew_session`],
    /// [`Store::release_session`], and the fencing token every write of its holder carries, so none
    /// lands once another took the session over ([`sql::HOLDS`]).
    pub fn take_session(&self, handle: &str, doing: &str) -> Result<u64, Error> {
        let now = self.now();
        let taken: Lease = self.retrying(|| {
            self.transact(
                both(sql::TAKE_SESSION, pg::take_session()),
                json!({"handle": handle, "doing": doing, "now": now, "seconds": SESSION_LEASE_SECONDS}),
            )
        })?;
        match taken {
            Lease {
                revision: Some(revision),
                ..
            } => Ok(revision),
            Lease { doing, until, .. } => Err(refused(format!(
                "{} of {handle} is under way: its hold lapses in {:.0} s",
                doing.unwrap_or_default(),
                until.unwrap_or(now) - now
            ))),
        }
    }

    /// Hold `handle`'s session another [`SESSION_LEASE_SECONDS`] from now, while the hold
    /// `revision` is still its lease, lapsed or not: false once another close or restore took it.
    pub fn renew_session(&self, handle: &str, revision: u64) -> Result<bool, Error> {
        self.retrying(|| {
            self.transact(
                both(sql::RENEW_SESSION, pg::renew_session()),
                json!({"handle": handle, "revision": revision, "now": self.now(), "seconds": SESSION_LEASE_SECONDS}),
            )
        })
    }

    /// End the hold `revision` of `handle`'s session; a hold another took since is left as it is.
    pub fn release_session(&self, handle: &str, revision: u64) -> Result<(), Error> {
        self.retrying(|| {
            self.transact::<Value>(
                both(sql::RELEASE_SESSION, pg::release_session()),
                json!({"handle": handle, "revision": revision}),
            )
        })
        .map(|_| ())
    }

    fn change_task_session<T: DeserializeOwned>(
        &self,
        slug: &str,
        handle: &str,
        body: &str,
        pg: Pg<'static>,
        more: Value,
    ) -> Result<T, Error> {
        let mut vars = json!({
            "slug": slug, "handle": handle, "now": (self.clock)(),
            "unrecorded": format!("{REFUSED}{handle} has no recorded session on {slug}"),
        });
        if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), more) {
            vars.extend(more);
        }
        self.retrying(|| self.transact(both(body, pg.clone()).posting(), vars.clone()))
    }
}

/// The launch and terminal `SAME_LAUNCH` holds a change to: `record`'s.
fn same_launch(vars: &mut Value, record: &TaskSessionRow) {
    if let Value::Object(vars) = vars {
        vars.insert("pane".into(), json!(record.pane));
        vars.insert("started".into(), json!(record.started));
    }
}
