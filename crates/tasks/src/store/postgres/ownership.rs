//! The statements of `store/ownership.rs` on PostgreSQL: the ownership log read whole, a line of
//! it written fenced on the newest it read, and a task handed with its line.

use super::Pg;
use std::borrow::Cow;

/// The newest line's number of the ownership log, as an expression: 0 where it has none.
macro_rules! newest {
    () => {
        "coalesce((SELECT (id #>> '{}')::bigint FROM ownership ORDER BY id DESC LIMIT 1), 0)"
    };
}

/// The fence a write of a line meets: the log's newest line is still `expected`, and where
/// `roster` is given, the board's people revision is still it. A line is a hand-over's decision on
/// who is in a group, read with the people.
const FENCED: &str = concat!(
    newest!(),
    " = (v ->> 'expected')::bigint AND (v -> 'roster' IS NULL OR v -> 'roster' = 'null'::jsonb \
     OR coalesce((SELECT (id #>> '{}')::bigint FROM people_log ORDER BY id DESC LIMIT 1), 0) \
     = (v ->> 'roster')::bigint)"
);

/// Every line of the ownership log, oldest first: none where the board lacks the step.
pub(in crate::store) const LOG: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "board_answer := '[]'::jsonb;
IF to_regclass('ownership') IS NOT NULL THEN
    board_answer := coalesce((SELECT jsonb_agg(doc ORDER BY id) FROM ownership), '[]'::jsonb);
END IF;",
    ),
    None,
);

/// The line `line` written only while [`FENCED`] holds: whether it was. Two writes that read the
/// same newest line both write the one numbered after it, and the second to commit fails to
/// serialize and is tried again from a new read.
pub(in crate::store) fn line() -> Pg<'static> {
    Pg::script(format!(
        "
IF NOT ({FENCED}) THEN
    board_answer := 'false'::jsonb;
ELSE
    INSERT INTO ownership (id, doc) VALUES (v -> 'line' -> 'n', v -> 'line')
        ON CONFLICT (id) DO NOTHING;
    board_answer := to_jsonb(FOUND);
END IF;
"
    ))
}

/// The hand-over line `v -> 'line'` removed while it is still the line written: whether it was.
pub(in crate::store) const TAKEN_BACK: Pg<'static> = Pg::Script(
    Cow::Borrowed(
        "DELETE FROM ownership WHERE id = v -> 'line' -> 'n' AND doc = v -> 'line'
    AND doc ->> 'did' = 'hand';
board_answer := to_jsonb(FOUND);",
    ),
    None,
);

/// The task `v ->> 'slug'` handed to `v ->> 'name'`, its line `v -> 'line'` and the board's line on
/// its thread (`said`), while what the hand-over was decided on stands: the name written on it is
/// still `v ->> 'was'` and [`FENCED`] holds; else refused as `v ->> 'moved'`, which is read again.
/// A missing task, or a closed one, is refused. As in [`line`], a line another write took the
/// number of since this one read writes nothing: under the store's snapshot the insert fails to
/// serialize and is tried again from a new read, which [`FENCED`] then refuses as moved, and the
/// whole write, the task's name with it, is undone.
pub(in crate::store) fn hand_task(said: &str) -> String {
    format!(
        "
DECLARE
    task_ record;
BEGIN
    SELECT status, fields #>> '{{_board,group}}' AS named INTO task_ FROM task
        WHERE slug = v ->> 'slug';
    IF NOT FOUND THEN
        RAISE EXCEPTION '%', v ->> 'missing';
    END IF;
    IF task_.status IN ('done', 'cancelled') THEN
        RAISE EXCEPTION '%', (v ->> 'closed') || task_.status || ': only an open task is handed on';
    END IF;
    IF task_.named IS DISTINCT FROM v ->> 'was' OR NOT ({FENCED}) THEN
        RAISE EXCEPTION '%', v ->> 'moved';
    END IF;
    UPDATE task SET fields = board_fields(id, fields, '{{_board,group}}', v -> 'name'),
        version = version + 1 WHERE slug = v ->> 'slug';
    INSERT INTO ownership (id, doc) VALUES (v -> 'line' -> 'n', v -> 'line')
        ON CONFLICT (id) DO NOTHING;
    IF NOT FOUND THEN
        RAISE EXCEPTION '%', v ->> 'moved';
    END IF;
    {said}
END;"
    )
}
