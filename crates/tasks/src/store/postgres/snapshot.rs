//! The statements of `store/snapshot.rs` on PostgreSQL.

use super::Pg;
use super::reading::{Beside, reading_with};
use super::schema::MARKER;
use crate::store::{Sessions, Which};

/// The whole board with every session (a `Snapshot`), read with the board's marker as
/// `callback_sequence`, in one statement and so one snapshot.
pub(in crate::store) fn board() -> Pg<'static> {
    let read = reading_with(
        &Which::All,
        Sessions::Whole,
        Beside {
            callback_sequence: MARKER,
            has_callbacks: "false",
        },
    );
    Pg::read(read)
}

/// The board's marker alone.
pub(in crate::store) fn marker() -> Pg<'static> {
    Pg::read(MARKER)
}
