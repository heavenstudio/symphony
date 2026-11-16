//! What the hourly roll-up of a project's reports reads of its board ([`crate::reports`]): every
//! task open now, every one written since the window opened and every one with a line since then,
//! top-level and subtasks alike, each with its thread's lines, in one round trip.

use super::surreal::report_lines as sql;
use super::{Error, Store};
use crate::stages::Line;
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
    /// The commits a red's body names it came between ([`crate::red::between`]); none for a task
    /// not marked red, whose body is not read.
    pub between: Option<String>,
    /// Every line of its thread, once each and in the order written (by its key): a `system` line
    /// whole, a `verdict` cut to the words that name it, and a post (`msg`) with no text, its sender
    /// and time alone.
    pub lines: Vec<Line>,
}

/// The one read the roll-up makes of the board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ReportRead {
    /// Each task open now, written since the window opened or with a line since then, in the order
    /// they were made.
    pub tasks: Vec<ReportTask>,
}

impl Store {
    /// Every task open now, every one written at or after `since` and every one with a line at or
    /// after `since`, each with its thread: one round trip, writing nothing.
    pub fn report_records(&self, since: f64) -> Result<ReportRead, Error> {
        let answer: Value = self.retrying(|| self.transact(sql::READ, json!({"since": since})))?;
        taken_apart(&answer)
            .ok_or_else(|| Error::Db("the reports' read is not in the board's shape".into()))
    }
}

/// The read's `answer`, its rows taken apart by hand: a reader derived for each row's shape would
/// cost the server's binary a deserializer of its own for each (docs/CHECKS.md, size budgets).
/// None when a row lacks a field it must have, or the two lists do not pair.
fn taken_apart(answer: &Value) -> Option<ReportRead> {
    let text = |row: &Value, name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    let number = |row: &Value, name: &str| row.get(name).and_then(Value::as_f64);
    let tasks = answer.get("tasks")?.as_array()?;
    let threads = answer.get("lines")?.as_array()?;
    if tasks.len() != threads.len() {
        return None;
    }
    let mut read = ReportRead::default();
    for (task, thread) in tasks.iter().zip(threads) {
        let mut lines = Vec::new();
        for line in thread.as_array()? {
            lines.push(line_of(line)?);
        }
        let marked_red = task.get("red").is_some_and(|red| !red.is_null());
        let body = task
            .get("body")
            .and_then(Value::as_str)
            .filter(|_| marked_red);
        read.tasks.push(ReportTask {
            slug: text(task, "slug")?,
            status: text(task, "status")?,
            created: number(task, "created")?,
            updated: number(task, "updated")?,
            parent: text(task, "parent"),
            instruction: task.get("instruction").and_then(Value::as_bool) == Some(true),
            marked_red,
            between: body.and_then(crate::red::between).map(str::to_owned),
            lines,
        });
    }
    Some(read)
}

/// A thread's line as a read's `row` gives it, taken apart by hand as [`taken_apart`] takes its
/// rows: its key (`sid`), kind and time must be there, its text and sender may be missing. None
/// when one it must have is not.
pub(super) fn line_of(row: &Value) -> Option<Line> {
    let text = |name: &str| row.get(name).and_then(Value::as_str).map(str::to_owned);
    Some(Line {
        id: row.get("sid")?.as_i64()?,
        kind: text("kind")?,
        text: text("text").unwrap_or_default(),
        ts: row.get("ts")?.as_f64()?,
        sender: text("sender").unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_without_what_it_must_have_or_lists_that_do_not_pair_read_as_none() {
        let line = json!({"sid": 3, "kind": "system", "ts": 5.0});
        let task = json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0});
        let read = taken_apart(&json!({"tasks": [task], "lines": [[line]]})).unwrap();
        assert_eq!(
            read.tasks,
            [ReportTask {
                slug: "a".into(),
                status: "ready".into(),
                created: 1.0,
                updated: 2.0,
                parent: None,
                instruction: false,
                marked_red: false,
                between: None,
                lines: vec![Line {
                    id: 3,
                    kind: "system".into(),
                    text: String::new(),
                    ts: 5.0,
                    sender: String::new(),
                }],
            }],
            "a line's text and sender and a task's parent, instruction and red may be missing"
        );
        let marked = json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0,
            "parent": null, "instruction": true, "red": {"series": "main"}});
        let read = taken_apart(&json!({"tasks": [marked], "lines": [[]]})).unwrap();
        assert!(read.tasks[0].instruction && read.tasks[0].marked_red);
        let null_red = json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0,
            "red": null});
        let read = taken_apart(&json!({"tasks": [null_red], "lines": [[]]})).unwrap();
        assert!(
            !read.tasks[0].marked_red,
            "a red cleared to null marks nothing"
        );
        let named = "Red.\n\nbetween: 0123456..89abcde\nkey: verify";
        let red_body = json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0,
            "red": "release", "body": named});
        let read = taken_apart(&json!({"tasks": [red_body], "lines": [[]]})).unwrap();
        assert_eq!(read.tasks[0].between.as_deref(), Some("0123456..89abcde"));
        for (task, why) in [
            (
                json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0,
                    "body": named}),
                "a body of a task not marked red",
            ),
            (
                json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0,
                    "red": "release"}),
                "a red with no body",
            ),
            (
                json!({"slug": "a", "status": "ready", "created": 1.0, "updated": 2.0,
                    "red": "release", "body": "Red.\nkey: verify"}),
                "a red whose body names no commits",
            ),
        ] {
            let read = taken_apart(&json!({"tasks": [task], "lines": [[]]})).unwrap();
            assert_eq!(read.tasks[0].between, None, "{why}");
        }
        for (bad, why) in [
            (
                json!({"tasks": [task], "lines": []}),
                "lists that do not pair",
            ),
            (json!({"lines": [[]]}), "no tasks"),
            (
                json!({"tasks": [{"slug": "a"}], "lines": [[]]}),
                "a task with no times",
            ),
            (
                json!({"tasks": [task], "lines": [[{"kind": "msg", "ts": 1.0}]]}),
                "a line with no key",
            ),
            (
                json!({"tasks": [task], "lines": [{}]}),
                "a thread that is no list",
            ),
        ] {
            assert_eq!(taken_apart(&bad), None, "{why}");
        }
    }
}
