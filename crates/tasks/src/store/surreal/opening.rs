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

/// The board's record of its schema, its version and `writable_from`, or NONE before its tables
/// are made: one read ([`crate::Store::board_standing`]).
pub(in crate::store) const BOARD_STANDING: &str = "RETURN IF (INFO FOR DB).tables.meta = NONE { NONE } ELSE { (SELECT version, writable_from FROM ONLY meta:schema) };";

/// The other half of a statement only PostgreSQL is asked: what its server's version is and
/// what a hosted database already holds, which no SurrealDB board is read for.
#[cfg(feature = "hosted-postgresql")]
pub(in crate::store) const POSTGRESQL_ALONE: &str = "RETURN NONE;";

/// A migration's one transaction: stale unless the board is still at `$was`, then `steps`, the
/// version's own, and the board's record of `$version` with the oldest schema whose Symphony may
/// write it, `$writable_from` ([`crate::Schema::writable_from`]).
pub(in crate::store) fn migration(steps: &str) -> String {
    format!(
        "LET $found = {BOARD_SCHEMA_VALUE};\nIF $found != $was {{ THROW '{STALE}' }};\n{steps}\n\
             UPSERT meta:schema SET version = $version, writable_from = $writable_from RETURN NONE;\nRETURN NONE;"
    )
}
