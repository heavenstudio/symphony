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

pub(in crate::store) fn standings() -> Pg<'static> {
    Pg::read(
        "jsonb_build_object(
    'active', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb),
    'tasks', coalesce((SELECT jsonb_agg(jsonb_build_object(
        'slug', slug, 'version', version, 'status', status,
        'position', fields #> '{_board,workflow}',
        'blocked_from', fields #> '{_board,blocked,from}') ORDER BY slug)
        FROM task WHERE status NOT IN ('done', 'cancelled') OR fields #> '{_board,workflow}' IS NOT NULL),
        '[]'::jsonb))",
    )
}

pub(in crate::store) fn activate() -> Pg<'static> {
    Pg::script(
        "
DECLARE
    active_ jsonb;
    t_ jsonb;
    at_ double precision := (v ->> 'at')::double precision;
BEGIN
    SELECT body INTO active_ FROM workflow WHERE id = '[\"active\"]'::jsonb FOR UPDATE;
    IF coalesce((active_ ->> 'generation')::bigint, 0) <> (v ->> 'expected')::bigint THEN
        RAISE EXCEPTION '%', 'refused: stale_workflow: the active generation is '
            || coalesce(active_ ->> 'generation', '0') || ', not ' || (v ->> 'expected');
    END IF;
    FOR t_ IN SELECT value FROM jsonb_array_elements(v -> 'kept') LOOP
        UPDATE task SET version = version + 1
            WHERE slug = t_ ->> 'slug' AND version = (t_ ->> 'version')::bigint;
        IF NOT FOUND THEN
            RAISE EXCEPTION '%', 'refused: stale_task: ' || (t_ ->> 'slug') || ' changed while its workflow activated';
        END IF;
    END LOOP;
    FOR t_ IN SELECT value FROM jsonb_array_elements(v -> 'moved') LOOP
        PERFORM 1 FROM task WHERE slug = t_ ->> 'slug' AND version = (t_ ->> 'version')::bigint
            AND coalesce((fields #>> '{_board,workflow,revision}')::bigint, 0) = (t_ ->> 'expected')::bigint
            FOR UPDATE;
        IF NOT FOUND THEN
            RAISE EXCEPTION '%', 'refused: stale_task: ' || (t_ ->> 'slug') || ' changed while its workflow activated';
        END IF;
        INSERT INTO workflow (id, kind, body, at) VALUES (
            jsonb_build_array('event', t_ ->> 'slug', t_ -> 'transition' -> 'revision'), 'event',
            t_ -> 'transition', at_);
        UPDATE task SET status = t_ -> 'transition' ->> 'status',
            fields = board_fields(id, fields, '{_board,workflow}', t_ -> 'position'),
            updated = at_, version = version + 1
            WHERE slug = t_ ->> 'slug';
    END LOOP;
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('definition', v ->> 'digest'), 'definition', v -> 'definition', at_)
        ON CONFLICT (id) DO NOTHING;
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('activation', v -> 'generation'), 'activation', v -> 'activation', at_);
    INSERT INTO workflow (id, kind, body, at) VALUES ('[\"active\"]', 'active',
        jsonb_build_object('generation', v -> 'generation', 'digest', v ->> 'digest'), at_)
        ON CONFLICT (id) DO UPDATE SET body = EXCLUDED.body, at = EXCLUDED.at;
END;
",
    )
}
