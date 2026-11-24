//! The statements of `store/stored_settings.rs` on PostgreSQL: the stored settings' read and
//! write.

use super::Pg;
use std::borrow::Cow;

/// Every stored entry, by key, or none where the board lacks the step that defines the table.
pub(in crate::store) const ENTRIES: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := '[]'::jsonb;
IF to_regclass('setting') IS NOT NULL THEN
    board_answer := coalesce((SELECT jsonb_agg(doc ORDER BY doc ->> 'key') FROM setting), '[]'::jsonb);
END IF;",
    ),
    None,
);

/// The entry `key` written as `doc`, only while its revision is still `expected` (0: no row):
/// whether it was.
pub(in crate::store) fn write() -> Pg<'static> {
    Pg::script(
        "
IF (v ->> 'expected')::bigint = 0 THEN
    WITH inserted AS (
        INSERT INTO setting (id, doc) VALUES (v -> 'key', v -> 'doc')
        ON CONFLICT (id) DO NOTHING RETURNING 1
    ) SELECT to_jsonb(EXISTS(SELECT 1 FROM inserted)) INTO board_answer;
ELSE
    WITH updated AS (
        UPDATE setting SET doc = v -> 'doc'
        WHERE id = v -> 'key' AND (doc ->> 'revision')::bigint = (v ->> 'expected')::bigint RETURNING 1
    ) SELECT to_jsonb(EXISTS(SELECT 1 FROM updated)) INTO board_answer;
END IF;
",
    )
}
