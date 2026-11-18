//! The statements of `store/opening.rs` on PostgreSQL.

use super::Pg;
use crate::store::{REFUSED, STALE};

/// The board's schema version, or none before its tables are made: one read, which makes nothing.
pub(in crate::store) const BOARD_SCHEMA: Pg<'static> = Pg::Script(
    std::borrow::Cow::Borrowed(
        "IF to_regclass('meta') IS NOT NULL THEN
    board_answer := (SELECT to_jsonb(version) FROM meta WHERE id = '\"schema\"');
END IF;",
    ),
    None,
);

/// How many tables, views, sequences and functions the database's own schema holds: what an
/// empty database a board may be made in has none of.
const FOREIGN: &str =
    "(SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
        WHERE n.nspname = 'public')
    + (SELECT count(*) FROM pg_proc p JOIN pg_namespace n ON n.oid = p.pronamespace
        WHERE n.nspname = 'public')";

/// The server's version as a number: 160004 is 16.4.
#[cfg(feature = "hosted-postgresql")]
pub(in crate::store) const SERVER_VERSION: Pg<'static> = Pg::Read(std::borrow::Cow::Borrowed(
    "current_setting('server_version_num')::bigint",
));

/// The key of the lock a migration holds while it runs, so two at once run one after the other.
const MIGRATING: i64 = 0x0053_594d_504d_4947; // "SYMPMIG"

/// A migration's one transaction, read committed so each statement reads what committed before it:
/// the migrations' lock taken, then stale unless the board is still at `was`, then `steps`, and the
/// board's record of `version`. A migration another finished while this one waited for the lock
/// finds the board moved, and is read again as stale. The board's table is looked for in the
/// catalogue as this statement reads it, never by `to_regclass`: a session that read no board
/// before it waited keeps that answer in its catalogue cache, and would take the board another
/// migration made meanwhile for none. With `empty`, a board that is made
/// (`was` none) is made only in a database whose schema holds nothing, or the migration is
/// refused in those words: judged here, under the lock and after the board was looked for, so
/// what another migration made a moment before is a board and never another's objects.
pub(in crate::store) fn migration(
    steps: &str,
    was: Option<i64>,
    version: i64,
    empty: Option<&str>,
) -> String {
    let alone = match (was, empty) {
        (None, Some(refusal)) => format!(
            "
    IF ({FOREIGN}) <> 0 THEN
        RAISE EXCEPTION USING MESSAGE = '{REFUSED}{}';
    END IF;",
            refusal.replace('\'', "''")
        ),
        _ => String::new(),
    };
    let was = was.map_or_else(|| "NULL".to_string(), |was| was.to_string());
    format!(
        "BEGIN ISOLATION LEVEL READ COMMITTED;
SELECT pg_advisory_xact_lock({MIGRATING});
DO $migration$
DECLARE
    found_ bigint;
BEGIN
    IF EXISTS (SELECT 1 FROM pg_class c
            WHERE c.relname = 'meta' AND c.relkind = 'r' AND pg_table_is_visible(c.oid)) THEN
        SELECT version INTO found_ FROM meta WHERE id = '\"schema\"';
    END IF;
    IF found_ IS DISTINCT FROM {was} THEN
        RAISE EXCEPTION '{STALE}';
    END IF;{alone}
END
$migration$;
{steps}
INSERT INTO meta (id, version) VALUES ('\"schema\"', {version})
    ON CONFLICT (id) DO UPDATE SET version = EXCLUDED.version;
COMMIT;"
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_migration_names_the_version_it_moves_from_and_to_as_numbers_alone() {
        let made = super::migration("CREATE TABLE t ();", None, 19, None);
        assert!(!made.contains("pg_class"), "{made}");
        assert!(made.contains("IS DISTINCT FROM NULL THEN"), "{made}");
        assert!(made.contains("VALUES ('\"schema\"', 19)"), "{made}");
        let moved = super::migration("", Some(19), 20, Some("another's"));
        assert!(
            !moved.contains("pg_class"),
            "a board there is moved: {moved}"
        );
        assert!(moved.contains("IS DISTINCT FROM 19 THEN"), "{moved}");
        assert!(moved.starts_with("BEGIN ISOLATION LEVEL READ COMMITTED;"));
        assert!(moved.ends_with("COMMIT;"));
        assert!(!moved.contains("to_regclass"), "{moved}");
        // A board made where nothing of another's may be: judged after the board was looked
        // for, in the same block, and refused in the words given, quoted as SQL takes them.
        let alone = super::migration("CREATE TABLE t ();", None, 19, Some("another's, 100%"));
        let (stale, foreign) = (
            alone.find("RAISE EXCEPTION 'stale'").unwrap(),
            alone.find("FROM pg_class").unwrap(),
        );
        assert!(stale < foreign && foreign < alone.find("CREATE TABLE t").unwrap());
        assert!(
            alone.contains("RAISE EXCEPTION USING MESSAGE = 'refused: another''s, 100%';"),
            "{alone}"
        );
    }
}
