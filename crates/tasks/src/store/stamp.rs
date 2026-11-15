//! What tells a task from the one a reader read before ([`Store::task_stamp`]), in one small read:
//! a reader that keeps what it made of a task and its thread (the Symphony's gates,
//! `symphony::gates`) asks for it on each of its reads, and reads the task and its thread again
//! only once it moved.

use super::{Error, Store};
use serde_json::{Value, json};

/// The task `$slug`'s row, its links, its roles, the branches its executors' sessions recorded,
/// the tasks right under it and its thread's newest line: each by its own key or index, as the
/// board's snapshot reads them, and one statement, so it is answered outside a transaction. A
/// thread's lines are never rewritten or removed, so its newest line's id says what it holds.
const READ: &str = "RETURN {
    task: (SELECT * OMIT id FROM ONLY type::record('task', $slug)),
    links: (SELECT kind, target FROM link WHERE slug IN [$slug] ORDER BY kind, target),
    roles: (SELECT handle, role, since, expires FROM role WHERE slug IN [$slug] ORDER BY since, handle),
    branches: (SELECT branch, handle, started FROM task_session
        WHERE slug IN [$slug] AND role = 'executor' AND branch != '' ORDER BY started, handle),
    under: (SELECT slug, sid FROM task WHERE parent = $slug ORDER BY sid),
    newest: array::last(SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..]),
};";

/// A task as one read found it ([`Store::task_stamp`]): equal to another while nothing
/// [`Store::get`] and [`Store::thread`] read of the task has changed between them, nor the tasks
/// right under it.
#[derive(Debug, Clone, PartialEq)]
pub struct TaskStamp(Value);

impl Store {
    /// What tells the task `slug` from the one read before: everything [`Store::get`] makes its
    /// [`super::Task`] from (its row, links, roles and its executors' recorded branches), the
    /// tasks right under it, and its thread's newest line, which stands for [`Store::thread`]. One
    /// read of the task's own rows, none of the board's sessions. `None` for a slug no task has.
    ///
    /// A reservation lapses by the clock, with no row written, so a stamp does not tell a task
    /// whose reservation lapsed from the one before.
    pub fn task_stamp(&self, slug: &str) -> Result<Option<TaskStamp>, Error> {
        let read: Value = self.retrying(|| self.read(READ, json!({"slug": slug})))?;
        let known = read.get("task").is_some_and(|task| !task.is_null());
        Ok(known.then_some(TaskStamp(read)))
    }
}
