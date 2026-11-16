//! The statements of `store/snapshot.rs`.

use super::reading::reading;
use crate::schema::BOARD_MARKER;
use crate::store::{Sessions, Which};
use serde_json::Value;

/// The whole board with every session (a `Snapshot`), read in one transaction with the board's
/// marker as `callback_sequence`, and the read's variables.
pub(in crate::store) fn board() -> (String, Value) {
    let (read, vars) = reading(&Which::All, Sessions::Whole);
    (
        format!("LET $callback_sequence = {BOARD_MARKER};\n{read}"),
        vars,
    )
}

/// The board's marker alone.
pub(in crate::store) fn marker() -> String {
    format!("RETURN {BOARD_MARKER};")
}
