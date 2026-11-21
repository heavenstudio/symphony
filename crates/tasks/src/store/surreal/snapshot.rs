//! The statements of `store/snapshot.rs`.

use super::reading::reading;
use crate::schema::BOARD_MARKER;
use crate::store::{Sessions, Which};

/// The whole board with every session (a `Snapshot`), read in one transaction with the board's
/// marker as `callback_sequence`.
pub(in crate::store) fn board() -> String {
    let read = reading(&Which::All, Sessions::Whole);
    format!("LET $callback_sequence = {BOARD_MARKER};\n{read}")
}

/// The board's marker alone.
pub(in crate::store) fn marker() -> String {
    format!("RETURN {BOARD_MARKER};")
}
