//! PostgreSQL twin of `store/workflow.rs`'s statements (`super::super::surreal::workflow`).

use super::Pg;

pub(in crate::store) fn active() -> Pg<'static> {
    Pg::read("(SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb)")
}

pub(in crate::store) fn definition() -> Pg<'static> {
    Pg::read(
        "(SELECT body FROM workflow WHERE id = jsonb_build_array('definition', v ->> 'digest'))",
    )
}

pub(in crate::store) fn position() -> Pg<'static> {
    Pg::script(
        "
SELECT fields #> '{_board,workflow}' INTO board_answer FROM task WHERE slug = v ->> 'slug';
IF NOT FOUND THEN
    RAISE EXCEPTION '%', 'refused: no task ' || (v ->> 'slug');
END IF;
",
    )
}

pub(in crate::store) fn record() -> Pg<'static> {
    Pg::script(
        "
DECLARE
    w_ jsonb;
BEGIN
    SELECT fields #> '{_board,workflow}' INTO w_ FROM task WHERE slug = v ->> 'slug';
    IF NOT FOUND THEN
        RAISE EXCEPTION '%', 'refused: no task ' || (v ->> 'slug');
    END IF;
    IF coalesce((w_ ->> 'revision')::bigint, 0) <> (v ->> 'expected')::bigint THEN
        RAISE EXCEPTION '%', 'refused: stale_task: ' || (v ->> 'slug') || ' is at workflow revision '
            || coalesce(w_ ->> 'revision', '0') || ', not ' || (v ->> 'expected');
    END IF;
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('event', v ->> 'slug', v -> 'transition' -> 'revision'), 'event',
        v -> 'transition', (v -> 'transition' ->> 'at')::double precision);
    UPDATE task SET status = v -> 'transition' ->> 'status',
        fields = board_fields(id, fields, '{_board,workflow}', v -> 'position'),
        updated = (v -> 'transition' ->> 'at')::double precision, version = version + 1
        WHERE slug = v ->> 'slug';
END;
",
    )
}
