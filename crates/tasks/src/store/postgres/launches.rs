//! The launch read needed to explain a thread's undeliverable executor notice.

use super::{Pg, rows};

pub(in crate::store) fn sessions_of_task() -> Pg<'static> {
    Pg::read(rows(
        "l",
        "task_session l WHERE slug = v ->> 'slug'",
        "l.started, l.handle",
    ))
}
