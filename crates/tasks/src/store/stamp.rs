//! What tells a task from the one a reader read before ([`Store::task_stamp`]), in one small read:
//! a reader that keeps what it made of a task and its thread (the Symphony's gates,
//! `symphony::gates`) asks for it on each of its reads, and reads the task and its thread again
//! only once it moved.

use super::postgres::stamp as pg;
use super::statement::Statement;
use super::{Error, Store};
use serde_json::{Value, json};

/// A task as one read found it ([`Store::task_stamp`]): equal to another while nothing
/// [`Store::get`] and [`Store::thread`] read of the task has changed between them, nor the tasks
/// right under it, nor the board's people.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskStamp(Value);

impl Store {
    /// What tells the task `slug` from the one read before: everything [`Store::get`] makes its
    /// [`super::Task`] from (its row, links, roles and its executors' recorded branches), the
    /// tasks right under it, and its thread's newest line, which stands for [`Store::thread`]; and
    /// the board's people, whom a reader of its `owner` stamp asks after ([`TaskStamp::deciders`]).
    /// One read of the task's own rows and the people's, none of the board's sessions. `None` for a
    /// slug no task has.
    ///
    /// A reservation lapses by the clock, with no row written, so a stamp does not tell a task
    /// whose reservation lapsed from the one before.
    pub fn task_stamp(&self, slug: &str) -> Result<Option<TaskStamp>, Error> {
        let read: Value =
            self.retrying(|| self.read(Statement::from(pg::read()), json!({"slug": slug})))?;
        let known = read.get("task").is_some_and(|task| !task.is_null());
        Ok(known.then_some(TaskStamp(read)))
    }
}

impl TaskStamp {
    /// The people who held decide on the board when it was read ([`Store::deciders`]): none on a
    /// board without the step that keeps them, and none where its people cannot be read, so an
    /// approve it would count is not counted.
    pub fn deciders(&self) -> Vec<String> {
        (self.0.get("people"))
            .and_then(|people| super::people::answered(people).ok())
            .map(|people| people.holding(crate::people::Power::Decide))
            .unwrap_or_default()
    }
}
