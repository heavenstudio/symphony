//! The statements of `store/opening.rs`.

use super::super::STALE;

/// The board's schema version, or NONE before its tables are made.
macro_rules! board_schema {
    () => {
        "IF (INFO FOR DB).tables.meta = NONE { NONE } ELSE { (SELECT VALUE version FROM ONLY meta:schema) }"
    };
}
pub(in crate::store) const BOARD_SCHEMA_VALUE: &str = board_schema!();
/// [`BOARD_SCHEMA_VALUE`] as a read of its own.
pub(in crate::store) const BOARD_SCHEMA: &str = concat!("RETURN ", board_schema!(), ";");

/// A migration's one transaction: stale unless the board is still at `$was`, then `steps`, the
/// version's own, and the board's record of `$version`.
pub(in crate::store) fn migration(steps: &str) -> String {
    format!(
        "LET $found = {BOARD_SCHEMA_VALUE};\nIF $found != $was {{ THROW '{STALE}' }};\n{steps}\n\
             UPSERT meta:schema SET version = $version RETURN NONE;\nRETURN NONE;"
    )
}
