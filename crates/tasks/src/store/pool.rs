//! The messages one dispatch pass reads every recipient's inbox from ([`Pool`]), and the test of
//! whether any of them could reach a recipient, made on the client from those rows
//! ([`Store::could_hear`]), so a write's observation reads no inbox that holds nothing for it.

use crate::lock::Recovered as _;

use super::messages::DELIVERED;
use super::surreal::pool as sql;
use super::work::OWNER;
use super::{Error, Message, Store};
use serde_json::{Value, json};

/// How far behind the newest message a recipient's cursor may be for a dispatch pass to read its
/// inbox from the pass's [`Pool`]: one further behind (a session long unobserved) reads the table, so
/// no pool grows to carry it for every recipient.
const POOL_SPAN: i64 = 2000;

/// The delivered messages and the moves of the owner's and a person's ([`OWNER_MOVES`]) after
/// `floor`, as of message
/// `latest`, that one dispatch pass reads every recipient's inbox from: one scan of the message
/// table a pass, where each recipient's own read scanned it, and the deliveries, twice. Only the
/// fields the inbox's `could_reach` reads and the row's id.
pub(super) struct Pool {
    floor: i64,
    latest: i64,
    rows: Value,
}

/// The pass's [`Pool`], held for as long as the store it borrows: [`Store::pool_through`] made
/// sure there is one.
struct PoolGuard<'store>(std::sync::MutexGuard<'store, Option<Pool>>);

impl std::ops::Deref for PoolGuard<'_> {
    type Target = Pool;

    fn deref(&self) -> &Pool {
        self.0
            .as_ref()
            .expect("pool_through sets the pool before it answers")
    }
}

/// Whether `row`, a [`Pool`]'s row, could reach `handle`'s inbox after `since`: the inbox's
/// `could_reach` on the client, but for the session's connection time and the kind, which the
/// pool's read already took, so it never says no of a message the inbox would hold. A row without a
/// number for its `sid` counts as after `since`.
fn could_reach_row(row: &Value, handle: &str, since: i64) -> bool {
    let text = |field: &str| row.get(field).and_then(Value::as_str);
    let recipient = text("recipient");
    (row.get("sid").and_then(Value::as_i64)).is_none_or(|sid| sid > since)
        && text("sender") != Some(handle)
        && (text("sender") == Some(OWNER)
            || row.get("person") == Some(&Value::Bool(true))
            || recipient == Some(handle)
            || recipient.is_some_and(|to| to.starts_with('@'))
            || text("kind") == Some("verdict"))
}

impl Store {
    /// Native dispatch has already copied older messages to its durable outbox. Newly acquired
    /// roles must not backfill that history; manual inbox reads still use the complete history.
    /// A dispatch pass reads it from the pass's [`Pool`], made or widened here when it does not
    /// reach back to `since` or forward to `latest`, the newest message of the board the pass
    /// dispatches from: every message that board holds is in the pool, and none is read twice.
    pub(super) fn inbox_pooled(
        &self,
        handle: &str,
        since: i64,
        latest: i64,
    ) -> Result<Vec<Message>, Error> {
        if latest - since > POOL_SPAN {
            return self.inbox_read(handle, Some(since), None);
        }
        let rows = self.pool_through(since, latest)?.rows.clone();
        self.inbox_read(handle, Some(since), Some(rows))
    }

    /// Whether a message after `since`, as of message `latest`, could reach `handle`'s inbox, read
    /// from the pass's [`Pool`] as [`Store::inbox_pooled`] reads it: false means the inbox holds
    /// nothing after `since`, with no read of its own; true, that it may. A span wider than a pool
    /// reaches answers true.
    pub(super) fn could_hear(&self, handle: &str, since: i64, latest: i64) -> Result<bool, Error> {
        if latest - since > POOL_SPAN {
            return Ok(true);
        }
        let pool = self.pool_through(since, latest)?;
        Ok(pool
            .rows
            .as_array()
            .is_none_or(|rows| rows.iter().any(|row| could_reach_row(row, handle, since))))
    }

    /// The pass's [`Pool`], made or widened when it does not reach back to `since` or forward to
    /// `latest`. A pool is set whole, after its read, or not at all: a panic while the lock was held
    /// left the pool that was there before it, which is still a true read of the messages it covers.
    fn pool_through(&self, since: i64, latest: i64) -> Result<PoolGuard<'_>, Error> {
        let mut pool = self.inbox_pool.lock().recover();
        if !pool
            .as_ref()
            .is_some_and(|pool| pool.floor <= since && pool.latest >= latest)
        {
            let floor = pool.as_ref().map_or(since, |pool| pool.floor.min(since));
            let read = sql::pool();
            let (latest, rows): (i64, Value) = self.retrying(|| {
                self.read(
                    super::statement::both(&read, super::postgres::pool::pool()),
                    json!({"floor": floor, "delivered": DELIVERED}),
                )
            })?;
            *pool = Some(Pool {
                floor,
                latest,
                rows,
            });
        }
        Ok(PoolGuard(pool))
    }
}

#[cfg(test)]
mod tests {
    use super::could_reach_row;
    use serde_json::{Value, json};

    /// The recipient every case asks about, and its cursor.
    const ME: &str = "amber-wren";
    const CURSOR: i64 = 4;

    /// A pool row as the pool's read gives one.
    struct Row<'case> {
        sid: i64,
        sender: &'case str,
        recipient: Option<&'case str>,
        kind: &'case str,
    }

    impl Row<'_> {
        /// A post after the cursor, from another session, to `recipient`.
        fn after(recipient: Option<&str>) -> Row<'_> {
            Row {
                sid: CURSOR + 1,
                sender: "solar-fox",
                recipient,
                kind: "msg",
            }
        }

        fn json(&self) -> Value {
            json!({"sid": self.sid, "slug": "tools/x", "sender": self.sender, "recipient": self.recipient, "kind": self.kind, "ts": 1.0})
        }
    }

    #[test]
    fn a_pool_row_could_reach_the_handle_it_is_to_a_role_the_owner_and_a_verdict() {
        for row in [
            Row::after(Some(ME)),
            Row::after(Some("@orchestrator")),
            Row::after(Some("@executor")),
            Row {
                sender: "owner",
                ..Row::after(None)
            },
            Row {
                sender: "owner",
                ..Row::after(Some("solar-fox"))
            },
            Row {
                kind: "verdict",
                ..Row::after(None)
            },
        ] {
            let row = row.json();
            assert!(could_reach_row(&row, ME, CURSOR), "{row}");
        }
    }

    #[test]
    fn a_pool_row_reaches_no_other_handle_nor_its_sender_nor_one_at_or_before_the_cursor() {
        for row in [
            Row::after(Some("vivid-owl")),
            Row::after(None),
            Row {
                sender: ME,
                ..Row::after(Some(ME))
            },
            Row {
                sender: ME,
                kind: "verdict",
                ..Row::after(Some("@orchestrator"))
            },
            Row {
                sid: CURSOR,
                ..Row::after(Some(ME))
            },
            Row {
                sid: CURSOR - 1,
                sender: "owner",
                kind: "verdict",
                ..Row::after(None)
            },
        ] {
            let row = row.json();
            assert!(!could_reach_row(&row, ME, CURSOR), "{row}");
        }
    }

    #[test]
    fn a_pool_row_without_a_number_for_its_sid_counts_as_after_the_cursor() {
        let mut unnumbered = Row::after(Some(ME)).json();
        unnumbered["sid"] = json!("5");
        assert!(could_reach_row(&unnumbered, ME, CURSOR), "{unnumbered}");
        unnumbered
            .as_object_mut()
            .expect("a row is an object")
            .remove("sid");
        assert!(could_reach_row(&unnumbered, ME, CURSOR), "{unnumbered}");
    }

    #[test]
    fn a_persons_pool_row_could_reach_as_the_owners_does_and_an_unmarked_one_of_the_name_not() {
        let mut person = Row {
            sender: "ana",
            ..Row::after(None)
        }
        .json();
        assert!(
            !could_reach_row(&person, ME, CURSOR),
            "a session named ana: {person}"
        );
        person["person"] = json!(true);
        assert!(could_reach_row(&person, ME, CURSOR), "{person}");
        person["sid"] = json!(CURSOR);
        assert!(
            !could_reach_row(&person, ME, CURSOR),
            "at the cursor: {person}"
        );
    }
}
