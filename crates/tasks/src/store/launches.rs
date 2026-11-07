//! The agent sessions `just tasks launch` started for a task: what restoring one needs.

use super::work::no_task;
use super::{Error, REFUSED, Store, logged, refused, repr};
use crate::rows::TaskSessionRow;
use serde::Deserialize;
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
/// record's transaction settles: [`SETTLED`]): the words before it and after it.
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

/// The conversation a write of a record of `$handle` keeps, as `$conversation` (`$session` is
/// SurrealDB's own): the one `$given`, but a Codex one whose thread is not known yet (`codex:`) takes
/// the thread the handle's callback registered (`codex:THREAD`), so the thread is kept whichever was
/// written first, the callback or the record.
const SETTLED: &str = "LET $thread = type::record('callback_state', $handle).address ?? '';
LET $conversation = IF $given = 'codex:' AND string::starts_with($thread, 'codex:') AND $thread != 'codex:' { $thread } ELSE { $given };";

/// A change to a recorded task session, refused if there is none, said on the thread as `said`
/// (an expression).
fn recorded(change: &str, said: &str) -> String {
    format!(
        "IF !record::exists(type::record('task_session', [$slug, $handle])) {{ THROW $unrecorded }};
UPDATE type::record('task_session', [$slug, $handle]) {change} RETURN NONE;
{}
RETURN NONE;",
        logged("$slug", "$handle", "'system'", said, "NONE")
    )
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
        let line = if launch.log {
            logged(
                "$slug",
                "$handle",
                "'system'",
                "string::concat($said_before, $said_conversation, $said_after)",
                "NONE",
            )
        } else {
            String::new()
        };
        let body = format!(
            "IF !record::exists(type::record('task', $slug)) {{ THROW $no_task }};
IF !record::exists(type::record('session', $handle)) {{ THROW $no_session }};
{SETTLED}
LET $said_conversation = IF $conversation = '' {{ 'unknown' }} ELSE {{ $conversation }};
UPSERT type::record('task_session', [$slug, $handle]) CONTENT $row RETURN NONE;
UPDATE type::record('task_session', [$slug, $handle]) SET claude_session = $conversation RETURN NONE;
{line}
RETURN $conversation;"
        );
        let (before, after) = said(launch);
        let vars = json!({
            "slug": launch.slug, "handle": launch.handle, "row": row, "now": row.started,
            "given": launch.claude_session, "said_before": before, "said_after": after,
            "no_task": format!("{REFUSED}{}", no_task(&launch.slug)),
            "no_session": format!("{REFUSED}no session {}", repr(&launch.handle)),
        });
        let claude_session = self.retrying(|| self.transact::<String>(&body, vars.clone()))?;
        Ok(TaskSessionRow {
            claude_session,
            ..row
        })
    }

    /// The sessions launched for `slug`, by when they started.
    pub fn task_sessions(&self, slug: &str) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where(
            "WHERE slug = $slug ORDER BY started, handle",
            json!({"slug": slug}),
        )
    }

    /// The tasks `handle` was launched for, by when it started on each.
    pub fn sessions_of(&self, handle: &str) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where(
            "WHERE handle = $handle ORDER BY started, slug",
            json!({"handle": handle}),
        )
    }

    pub fn all_task_sessions(&self) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where("ORDER BY started, slug, handle", json!({}))
    }

    /// Those whose pane is not closed.
    pub fn open_task_sessions(&self) -> Result<Vec<TaskSessionRow>, Error> {
        self.task_sessions_where(
            "WHERE closed = NONE ORDER BY started, slug, handle",
            json!({}),
        )
    }

    fn task_sessions_where(&self, filter: &str, vars: Value) -> Result<Vec<TaskSessionRow>, Error> {
        let read = format!("RETURN SELECT * OMIT id FROM task_session {filter};");
        self.retrying(|| self.transact(&read, vars.clone()))
    }

    /// Its pane is closed. The record stays, with the line that brings it back. A Codex record whose
    /// thread is still unknown takes the one the handle's callback registered, if any ([`SETTLED`]);
    /// one that closes with none says on the thread, after its restore line, that no thread id ever
    /// arrived, so restore cannot resume it.
    pub fn close_task_session(
        &self,
        slug: &str,
        handle: &str,
        why: &str,
        restore: &str,
    ) -> Result<(), Error> {
        let body = format!(
            "LET $given = type::record('task_session', [$slug, $handle]).claude_session;
{SETTLED}
LET $line = IF $conversation = 'codex:' {{ string::concat($said, $lost) }} ELSE {{ $said }};
{}",
            recorded(
                "SET closed = $now, closed_why = $why, restore = $restore, claude_session = $conversation",
                "$line"
            )
        );
        let said = format!("closed the pane of {handle} ({why}); restore: {restore}");
        let lost = format!(
            ". Its Codex thread id never arrived, so restore cannot resume its conversation: start \
             it again with just tasks launch {slug}"
        );
        self.change_task_session(
            slug,
            handle,
            &body,
            json!({"why": why, "restore": restore, "said": said, "lost": lost}),
        )
    }

    /// It runs again, in `pane`.
    pub fn reopen_task_session(&self, slug: &str, handle: &str, pane: &str) -> Result<(), Error> {
        let body = recorded("SET pane = $pane, closed = NONE, closed_why = ''", "$said");
        let said = format!("restored {handle} in pane {pane}");
        self.change_task_session(slug, handle, &body, json!({"pane": pane, "said": said}))
    }

    fn change_task_session(
        &self,
        slug: &str,
        handle: &str,
        body: &str,
        more: Value,
    ) -> Result<(), Error> {
        let mut vars = json!({
            "slug": slug, "handle": handle, "now": (self.clock)(),
            "unrecorded": format!("{REFUSED}{handle} has no recorded session on {slug}"),
        });
        if let (Some(vars), Value::Object(more)) = (vars.as_object_mut(), more) {
            vars.extend(more);
        }
        self.retrying(|| self.transact::<Value>(body, vars.clone()))
            .map(|_| ())
    }
}
