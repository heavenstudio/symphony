//! What the hourly roll-up of a project's reports reads of its board ([`crate::reports`]): every
//! task open now and every one written since the window opened, top-level and subtasks alike,
//! each with its thread's lines, and the sessions that registered since, in one round trip.

use super::{Error, Store};
use crate::stages::Line;
use serde::Deserialize;
use serde_json::{Value, json};

/// A task of the window and its thread.
#[derive(Debug, Clone, PartialEq)]
pub struct ReportTask {
    pub slug: String,
    pub status: String,
    pub created: f64,
    pub updated: f64,
    /// The task it is a subtask of; none for a top-level task.
    pub parent: Option<String>,
    /// Whether it is an instruction subtask (a review's, say), which executes no work of its own.
    pub instruction: bool,
    /// Whether its filer marked it red ([`crate::red::marked`]).
    pub marked_red: bool,
    /// Every line of its thread: a `system` line whole, a `verdict` cut to the words that name it,
    /// and a post (`msg`) with no text, its sender and time alone.
    pub lines: Vec<Line>,
}

/// A session, as far as the roll-up counts it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct ReportSession {
    pub kind: String,
    #[serde(default)]
    pub model: String,
    /// When it registered.
    pub connected: f64,
}

/// The one read the roll-up makes of the board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReportRead {
    /// Each task open now or written since the window opened, in the order they were made.
    pub tasks: Vec<ReportTask>,
    /// The sessions that registered since the window opened.
    pub sessions: Vec<ReportSession>,
}

/// The tasks open now, whenever they were made, and those closed that were written since `$since`
/// (a task closed or made since then is one of them), each one's thread read by its own range of
/// keys: a verdict's text only for its first words, a post's not at all. The sessions are read
/// whole and kept by when they registered, which no index orders; a board holds a few thousand.
/// Nothing is written.
pub(crate) const READ: &str = "
LET $tasks = (SELECT sid, slug, status, created, updated, parent, instruction,
        fields._board.red AS red FROM task
    WHERE status NOT IN ['done', 'cancelled'] OR updated >= $since
    ORDER BY sid);
RETURN {
    tasks: $tasks,
    lines: $tasks.map(|$task| (SELECT sid, kind, ts, sender,
        IF kind = 'system' { text } ELSE IF kind = 'verdict' { string::slice(text, 0, 16) }
            ELSE { '' } AS text
        FROM message:[$task.slug, NONE]..[$task.slug, ..])),
    sessions: (SELECT kind, model, connected FROM session)
        .filter(|$s| $s.connected >= $since),
};";

impl Store {
    /// Every task open now and every one written at or after `since`, with its thread, and every
    /// session that registered at or after `since`: one round trip, writing nothing.
    pub fn report_records(&self, since: f64) -> Result<ReportRead, Error> {
        #[derive(Deserialize)]
        struct TaskFacts {
            slug: String,
            status: String,
            created: f64,
            updated: f64,
            #[serde(default)]
            parent: Option<String>,
            #[serde(default)]
            instruction: bool,
            #[serde(default)]
            red: Option<Value>,
        }
        #[derive(Deserialize)]
        struct LineRow {
            sid: i64,
            kind: String,
            ts: f64,
            #[serde(default)]
            sender: String,
            #[serde(default)]
            text: String,
        }
        #[derive(Deserialize)]
        struct Read {
            tasks: Vec<TaskFacts>,
            lines: Vec<Vec<LineRow>>,
            sessions: Vec<ReportSession>,
        }
        let read: Read = self.retrying(|| self.transact(READ, json!({"since": since})))?;
        let tasks = read
            .tasks
            .into_iter()
            .zip(read.lines)
            .map(|(task, lines)| ReportTask {
                slug: task.slug,
                status: task.status,
                created: task.created,
                updated: task.updated,
                parent: task.parent,
                instruction: task.instruction,
                marked_red: task.red.is_some_and(|red| !red.is_null()),
                lines: lines
                    .into_iter()
                    .map(|l| Line {
                        id: l.sid,
                        kind: l.kind,
                        text: l.text,
                        ts: l.ts,
                        sender: l.sender,
                    })
                    .collect(),
            })
            .collect();
        Ok(ReportRead {
            tasks,
            sessions: read.sessions,
        })
    }
}
