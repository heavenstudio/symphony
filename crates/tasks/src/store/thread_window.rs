//! A bounded recorded range for role history and message metadata. This is separate from the
//! full thread read used by existing consumers. Non-system bodies never enter this projection.

use super::postgres::thread_window as pg;
use super::statement::Statement;
use super::{Error, Message, Store, message};
use crate::rows::MessageRow;
use serde::Deserialize;
use serde_json::json;

/// Maximum lines returned by one history read, plus one probe row inside the query.
pub const THREAD_WINDOW_LIMIT: usize = 256;
/// Maximum system event text considered. A cut event makes the window uncertain.
pub(super) const EVENT_TEXT: usize = 2048;

/// A thread range, oldest id first. Full post text stays in `Store::thread`. `more` means an
/// older range exists; pass the first id as `before` to continue, exclusively, without overlap.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ThreadWindow {
    /// Task facts read in the same snapshot as the range, never role/session rows.
    pub parent: Option<String>,
    pub status: String,
    pub lines: Vec<Message>,
    pub more: bool,
    pub uncertain: bool,
    /// The ids of its lines a person a board named wrote, by the board's mark of each
    /// ([`MessageRow::person`]): their senders are people, never sessions of the task's.
    pub by_person: std::collections::BTreeSet<i64>,
}

impl Store {
    /// Newest bounded recorded lines before `before` (exclusive), or newest at all. Read through
    /// the task's message key range, never through a timestamp filter or a whole-history fetch.
    pub fn thread_window(&self, slug: &str, before: Option<i64>) -> Result<ThreadWindow, Error> {
        #[derive(Deserialize)]
        struct Row {
            #[serde(flatten)]
            message: MessageRow,
            cut: bool,
        }
        #[derive(Deserialize)]
        struct Read {
            known: bool,
            parent: Option<String>,
            status: Option<String>,
            messages: Vec<Row>,
        }
        let read: Read = self.retrying(|| {
            self.read(
                Statement::from(pg::read()),
                json!({"slug": slug, "before": before.unwrap_or(i64::MAX)}),
            )
        })?;
        if !read.known {
            return Err(super::work::no_task(slug));
        }
        let more = read.messages.len() > THREAD_WINDOW_LIMIT;
        let rows: Vec<_> = read
            .messages
            .into_iter()
            .take(THREAD_WINDOW_LIMIT)
            .collect();
        let uncertain = rows.iter().any(|row| row.cut);
        let by_person = (rows.iter())
            .filter(|row| row.message.person)
            .map(|row| row.message.id)
            .collect();
        let mut lines: Vec<_> = rows.into_iter().map(|row| message(row.message)).collect();
        lines.reverse();
        Ok(ThreadWindow {
            parent: read.parent,
            status: read.status.unwrap_or_default(),
            lines,
            more,
            uncertain,
            by_person,
        })
    }
}
