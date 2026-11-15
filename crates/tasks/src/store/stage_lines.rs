//! What [`crate::stages`] replays, read for many tasks at once: `tasks stats` asks for a window of
//! the board in one round trip, never a thread at a time, and `tasks list --json`, `search --json`
//! and `tasks api` for when each task they name took the status it holds
//! ([`Store::in_status_since`]).

use super::report_lines::line_of;
use super::{Error, Store, refused, repr};
use crate::stages::{self, Facts, Line};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashSet};

/// A top-level task and the lines of its thread that can change its status.
#[derive(Debug, Clone, PartialEq)]
pub struct StageRecord {
    pub slug: String,
    pub status: String,
    pub created: f64,
    pub updated: f64,
    /// Whether a session holds its executor role.
    pub executing: bool,
    /// The arm it was filed on ([`crate::arm`]).
    pub arm: Option<String>,
    /// Its `system` and `verdict` messages, a verdict cut to the word that names it.
    pub lines: Vec<Line>,
}

/// The top-level tasks made since `$since` and those done that were written since (a task closed
/// since then is one of them: its close wrote it), who executes them, and each one's thread read
/// by its own range of keys without its posts. A verdict's text is only read for its first word. The
/// roles are read by their tasks' slugs, through `role_slug`, and kept by role after, as the
/// board's snapshot reads them: one condition over both would read every role.
const READ: &str = "
LET $tasks = (SELECT sid, slug, status, created, updated, fields._board.arm AS arm FROM task
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

/// The status and times of each task `$slugs` names (none for a slug no task has) and the lines of
/// its thread that can change its status, read by its own range of keys: a verdict's text only for
/// its first words, the posts and the board's notices not at all. Nothing is written.
const SINCE_READ: &str = "RETURN {
    tasks: $slugs.map(|$slug| (SELECT status, created, updated FROM type::record('task', $slug))),
    lines: $slugs.map(|$slug| (SELECT sid, kind, ts,
        IF kind = 'system' { text } ELSE { string::slice(text, 0, 16) } AS text
        FROM message:[$slug, NONE]..[$slug, ..] WHERE kind IN ['system', 'verdict'])),
};";

impl Store {
    /// When each task `slugs` names took the status it holds, by slug: its thread replayed
    /// ([`stages::in_status_since`]) against its status and times as the same read found them, so
    /// the two agree. One round trip however many there are; a slug no task has is refused, as
    /// [`Store::threads`] refuses it.
    pub fn in_status_since(&self, slugs: &[String]) -> Result<BTreeMap<String, f64>, Error> {
        if slugs.is_empty() {
            return Ok(BTreeMap::new());
        }
        let answer: Value = self.retrying(|| self.transact(SINCE_READ, json!({"slugs": slugs})))?;
        let read = since_taken_apart(slugs, &answer).ok_or_else(|| {
            Error::Db("the read of when tasks took their status is not in the board's shape".into())
        })?;
        let mut since = BTreeMap::new();
        for (slug, found) in slugs.iter().zip(read) {
            let Some(task) = found else {
                return Err(refused(format!("no task {}", repr(slug))));
            };
            let facts = Facts {
                status: &task.status,
                created: task.created,
                updated: task.updated,
            };
            since.insert(slug.clone(), stages::in_status_since(facts, &task.lines));
        }
        Ok(since)
    }

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
            arm: Option<Value>,
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
                // Read as the task's own fields are ([`crate::arm::marked`]): a name alone.
                arm: (task.arm.as_ref().and_then(Value::as_str))
                    .filter(|arm| crate::arm::named(arm).is_ok())
                    .map(str::to_string),
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
                        sender: String::new(),
                    })
                    .collect(),
            })
            .collect())
    }
}

/// A task as [`SINCE_READ`] finds it.
#[derive(Debug, PartialEq)]
struct Found {
    status: String,
    created: f64,
    updated: f64,
    lines: Vec<Line>,
}

/// [`SINCE_READ`]'s `answer` for `slugs`, in their order: each one's task and lines, taken apart by
/// hand as the roll-up's read is ([`line_of`]), so no reader of its own shape is added to the
/// server's binary (docs/CHECKS.md, size budgets); none for a slug no task has. None for the whole
/// when a list does not pair with the slugs or a row lacks a field it must have.
fn since_taken_apart(slugs: &[String], answer: &Value) -> Option<Vec<Option<Found>>> {
    let tasks = answer.get("tasks")?.as_array()?;
    let threads = answer.get("lines")?.as_array()?;
    if tasks.len() != slugs.len() || threads.len() != slugs.len() {
        return None;
    }
    let mut found = Vec::new();
    for (task, thread) in tasks.iter().zip(threads) {
        let Some(task) = task.as_array()?.first() else {
            found.push(None);
            continue;
        };
        let lines = (thread.as_array()?.iter())
            .map(line_of)
            .collect::<Option<Vec<Line>>>()?;
        found.push(Some(Found {
            status: task.get("status")?.as_str()?.to_owned(),
            created: task.get("created")?.as_f64()?,
            updated: task.get("updated")?.as_f64()?,
            lines,
        }));
    }
    Some(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slugs(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_string()).collect()
    }

    #[test]
    fn each_slug_takes_its_task_and_lines_and_one_no_task_has_takes_none() {
        let task = json!({"status": "review", "created": 1.0, "updated": 7});
        let line = json!({"sid": 4, "kind": "verdict", "ts": 5.0, "text": "changes: a test"});
        let read = since_taken_apart(
            &slugs(&["a", "gone"]),
            &json!({"tasks": [[task], []], "lines": [[line], []]}),
        )
        .unwrap();
        assert_eq!(
            read,
            [
                Some(Found {
                    status: "review".into(),
                    created: 1.0,
                    updated: 7.0,
                    lines: vec![Line {
                        id: 4,
                        kind: "verdict".into(),
                        text: "changes: a test".into(),
                        ts: 5.0,
                        sender: String::new(),
                    }],
                }),
                None,
            ],
            "a whole number reads as a time, and a slug with no task finds none"
        );
    }

    #[test]
    fn a_read_not_in_its_shape_reads_as_none() {
        let task = json!({"status": "ready", "created": 1.0, "updated": 2.0});
        let two = slugs(&["a", "b"]);
        for (bad, why) in [
            (
                json!({"tasks": [[task]], "lines": [[]]}),
                "fewer than the slugs",
            ),
            (json!({"lines": [[], []]}), "no tasks"),
            (json!({"tasks": [[task], [task]]}), "no lines"),
            (
                json!({"tasks": [[task], [{"status": "ready"}]], "lines": [[], []]}),
                "a task with no times",
            ),
            (
                json!({"tasks": [[task], [task]], "lines": [[], [{"kind": "system", "ts": 1.0}]]}),
                "a line with no key",
            ),
            (
                json!({"tasks": [task, [task]], "lines": [[], []]}),
                "a task that is no list",
            ),
            (
                json!({"tasks": [[task], [task]], "lines": [[], {}]}),
                "a thread that is no list",
            ),
        ] {
            assert_eq!(since_taken_apart(&two, &bad), None, "{why}");
        }
    }
}
