//! The statements of `store/artifacts.rs` on PostgreSQL: the scoped private artifact ledger's load
//! and compare-and-set.

use super::Pg;

pub(in crate::store) fn load() -> Pg<'static> {
    Pg::read(
        "(SELECT jsonb_build_object('revision', revision, 'body', body) FROM artifact_ledger WHERE id = v -> 'key')",
    )
}

pub(in crate::store) fn cas() -> Pg<'static> {
    Pg::script(
        "
IF (v ->> 'expected')::bigint = 0 THEN
    WITH inserted AS (
        INSERT INTO artifact_ledger (id, revision, body) VALUES (v -> 'key', 1, v -> 'body')
        ON CONFLICT (id) DO NOTHING RETURNING 1
    ) SELECT to_jsonb(EXISTS(SELECT 1 FROM inserted)) INTO board_answer;
ELSE
    WITH updated AS (
        UPDATE artifact_ledger SET revision = revision + 1, body = v -> 'body'
        WHERE id = v -> 'key' AND revision = (v ->> 'expected')::bigint RETURNING 1
    ) SELECT to_jsonb(EXISTS(SELECT 1 FROM updated)) INTO board_answer;
END IF;
",
    )
}
