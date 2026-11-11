//! What [`crate::stages`] replays, read for many tasks at once: `tasks stats` asks for a window of
//! the board in one round trip, never a thread at a time.

use super::{Error, Store};
use crate::stages::Line;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashSet;

/// A top-level task and the lines of its thread that can change its status.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRecord {
    pub slug: String,
    pub status: String,
    pub created: f64,
    pub updated: f64,
    /// Whether a session holds its executor role.
    pub executing: bool,
    /// Its `system` and `verdict` messages, a verdict cut to the word that names it.
    pub lines: Vec<Line>,
}

/// The top-level tasks made since `$since` and those done that were written since (a task closed
/// since then is one of them: its close wrote it), who executes them, and each one's thread read
/// by its own range of keys without its posts. A verdict's text is only read for its first word. The
/// roles are read by their tasks' slugs, through `role_slug`, and kept by role after, as the
/// board's snapshot reads them: one condition over both would read every role.
const READ: &str = "
LET $tasks = (SELECT sid, slug, status, created, updated FROM task
    WHERE parent = NONE AND (created >= $since OR (status = 'done' AND updated >= $since))
    ORDER BY sid);
RETURN {
    tasks: $tasks,
    executing: (SELECT slug, role FROM role WHERE slug IN $tasks.slug)
        .filter(|$r| $r.role = 'executor').map(|$r| $r.slug),
    lines: $tasks.map(|$task| (SELECT sid, kind, ts,
        IF kind = 'system' { text } ELSE { string::slice(text, 0, 16) } AS text
        FROM message:[$task.slug, NONE]..[$task.slug, ..] WHERE kind != 'msg')),
};";

impl Store {
    /// Every top-level task made at or after `since`, and every done one written at or after it
    /// (so every one closed since, whenever it was made), in the order they were made, with what
    /// [`crate::stages::stages`] reads of each: one round trip however many there are.
    pub fn stage_records(&self, since: f64) -> Result<Vec<StageRecord>, Error> {
        #[derive(Deserialize)]
        struct TaskFacts {
            slug: String,
            status: String,
            created: f64,
            updated: f64,
        }
        #[derive(Deserialize)]
        struct LineRow {
            sid: i64,
            kind: String,
            ts: f64,
            text: String,
        }
        #[derive(Deserialize)]
        struct Read {
            tasks: Vec<TaskFacts>,
            executing: HashSet<String>,
            lines: Vec<Vec<LineRow>>,
        }
        let read: Read = self.retrying(|| self.transact(READ, json!({"since": since})))?;
        Ok(read
            .tasks
            .into_iter()
            .zip(read.lines)
            .map(|(task, lines)| StageRecord {
                executing: read.executing.contains(&task.slug),
                slug: task.slug,
                status: task.status,
                created: task.created,
                updated: task.updated,
                lines: lines
                    .into_iter()
                    .map(|l| Line {
                        id: l.sid,
                        kind: l.kind,
                        text: l.text,
                        ts: l.ts,
                    })
                    .collect(),
            })
            .collect())
    }
}
