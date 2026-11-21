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

/// How many additive steps on its contract the board has (`meta:additions`): none before its
/// tables are made, or where no record says.
macro_rules! board_additions {
    () => {
        "IF (INFO FOR DB).tables.meta = NONE { 0 } ELSE { (SELECT VALUE version FROM ONLY meta:additions) ?? 0 }"
    };
}
pub(in crate::store) const BOARD_ADDITIONS_VALUE: &str = board_additions!();
/// [`BOARD_ADDITIONS_VALUE`] as a read of its own ([`crate::Store::additions`]).
pub(in crate::store) const BOARD_ADDITIONS: &str = concat!("RETURN ", board_additions!(), ";");

/// The board's record of its schema, its version and `writable_from`, or NONE before its tables
/// are made: one read ([`crate::Store::board_standing`]).
pub(in crate::store) const BOARD_STANDING: &str = "RETURN IF (INFO FOR DB).tables.meta = NONE { NONE } ELSE { (SELECT version, writable_from FROM ONLY meta:schema) };";

/// The other half of a statement only PostgreSQL is asked: what its server's version is and
/// what a hosted database already holds, which no SurrealDB board is read for.
#[cfg(feature = "hosted-postgresql")]
pub(in crate::store) const POSTGRESQL_ALONE: &str = "RETURN NONE;";

/// A migration's one transaction: stale unless the board is still at `$was` with `$was_additions`
/// additive steps, then the database `database` made STRICT (a write to a table it does not define
/// is refused, by every build), `steps`, the version's own, the board's record of `$version` with
/// the oldest schema whose Symphony may write it, `$writable_from`
/// ([`crate::Schema::writable_from`]), and its count of additive steps, `$additions`.
pub(in crate::store) fn migration(steps: &str, database: &str) -> String {
    format!(
        "LET $found = {BOARD_SCHEMA_VALUE};\nLET $found_additions = {BOARD_ADDITIONS_VALUE};\n\
         IF $found != $was OR $found_additions != $was_additions {{ THROW '{STALE}' }};\n\
         {strict}\n{steps}\n\
         UPSERT meta:schema SET version = $version, writable_from = $writable_from RETURN NONE;\n\
         UPSERT meta:additions SET version = $additions RETURN NONE;\nRETURN NONE;",
        strict = strict(database)
    )
}

/// The database `database` made STRICT, keeping everything it holds: no write, of this build or
/// any other, makes a table it does not define. Only a definition of the database that is not
/// STRICT undoes it, and none is sent.
pub(in crate::store) fn strict(database: &str) -> String {
    format!("DEFINE DATABASE OVERWRITE `{database}` STRICT;")
}
