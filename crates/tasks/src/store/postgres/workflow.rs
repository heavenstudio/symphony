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

#[cfg(test)]
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

pub(in crate::store) fn admission() -> Pg<'static> {
    Pg::read(
        "jsonb_build_object(
    'active', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb),
    'task', (SELECT jsonb_build_object('slug', slug, 'version', version, 'status', status,
        'created_by', created_by, 'position', fields #> '{_board,workflow}')
        FROM task WHERE slug = v ->> 'slug'),
    'roles', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', handle, 'role', role,
        'expires', expires)) FROM role WHERE slug = v ->> 'slug'), '[]'::jsonb),
    'dependencies', coalesce((SELECT jsonb_agg(target) FROM link
        WHERE slug = v ->> 'slug' AND kind = 'dep'), '[]'::jsonb),
    'found', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', t.slug, 'version', t.version,
        'status', t.status, 'position', t.fields #> '{_board,workflow}'))
        FROM task t JOIN link l ON l.target = t.slug
        WHERE l.slug = v ->> 'slug' AND l.kind = 'dep'), '[]'::jsonb),
    'receipt', (SELECT body FROM workflow WHERE id = v -> 'receipt'),
    'present', coalesce((SELECT left_at IS NULL FROM session WHERE handle = v ->> 'actor'), false))",
    )
}

/// `body`, the write of an admitted operation (`super::super::surreal::workflow::APPLY`), with
/// the slot counters it holds guarded after it when `guarded` and its line `logged` on the
/// thread.
pub(in crate::store) fn apply(logged: &str, guarded: bool) -> Pg<'static> {
    let guard = if guarded { super::cap::GUARD } else { "" };
    Pg::script(format!(
        "
DECLARE
    active_ jsonb;
BEGIN
    SELECT body INTO active_ FROM workflow WHERE id = '[\"active\"]'::jsonb FOR SHARE;
    IF active_ IS NULL OR (active_ ->> 'generation')::bigint <> (v ->> 'generation')::bigint
        OR active_ ->> 'digest' <> v ->> 'digest' THEN
        RAISE EXCEPTION 'stale';
    END IF;
    PERFORM 1 FROM task WHERE slug = v ->> 'slug' AND version = (v ->> 'version')::bigint FOR UPDATE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'stale';
    END IF;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'dependencies') AS d
            WHERE NOT EXISTS (SELECT 1 FROM task t WHERE t.slug = d ->> 'slug'
                AND t.version = (d ->> 'version')::bigint)) THEN
        RAISE EXCEPTION 'stale';
    END IF;
    PERFORM 1 FROM task WHERE slug IN (SELECT d ->> 'slug' FROM jsonb_array_elements(v -> 'dependencies') AS d)
        FOR SHARE;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'holding') AS r
            WHERE NOT EXISTS (SELECT 1 FROM role WHERE id = jsonb_build_array(v ->> 'slug', r ->> 'handle', r ->> 'role'))) THEN
        RAISE EXCEPTION 'stale';
    END IF;
    PERFORM 1 FROM session WHERE handle IN (SELECT s ->> 'handle' FROM jsonb_array_elements(v -> 'seen') AS s)
        FOR SHARE;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'seen') AS s
            WHERE NOT EXISTS (SELECT 1 FROM session WHERE handle = s ->> 'handle'
                AND last_seen = (s ->> 'last_seen')::double precision)) THEN
        RAISE EXCEPTION 'stale';
    END IF;
    IF v ->> 'receipt' IS NOT NULL AND EXISTS (SELECT 1 FROM workflow WHERE id = v -> 'receipt') THEN
        RAISE EXCEPTION 'stale';
    END IF;
    IF v ->> 'actor' IS NOT NULL AND NOT EXISTS (SELECT 1 FROM session
            WHERE handle = v ->> 'actor' AND left_at IS NULL) THEN
        RAISE EXCEPTION 'stale';
    END IF;
    DELETE FROM role WHERE id IN (SELECT jsonb_build_array(v ->> 'slug', d ->> 'handle', d ->> 'role')
        FROM jsonb_array_elements(v -> 'drop') AS d);
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'add') AS a JOIN role ON role.slug = v ->> 'slug'
            AND role.role IN (a ->> 'role', 'reserved') AND role.handle <> a ->> 'handle') THEN
        RAISE EXCEPTION 'stale';
    END IF;
    INSERT INTO role (id, slug, handle, role, since, expires)
        SELECT jsonb_build_array(v ->> 'slug', a ->> 'handle', a ->> 'role'), a ->> 'slug',
            a ->> 'handle', a ->> 'role', (a ->> 'since')::double precision, NULL
        FROM jsonb_array_elements(v -> 'add') AS a
        ON CONFLICT (id) DO UPDATE SET (slug, handle, role, since, expires)
            = (EXCLUDED.slug, EXCLUDED.handle, EXCLUDED.role, EXCLUDED.since, EXCLUDED.expires);
    IF v ->> 'transition' IS NOT NULL THEN
        INSERT INTO workflow (id, kind, body, at) VALUES (
            jsonb_build_array('event', v ->> 'slug', v -> 'transition' -> 'revision'), 'event',
            v -> 'transition', (v -> 'transition' ->> 'at')::double precision);
        UPDATE task SET status = v -> 'transition' ->> 'status',
            fields = board_fields(id, fields, '{{_board,workflow}}', v -> 'position'),
            updated = (v -> 'transition' ->> 'at')::double precision, version = version + 1
            WHERE slug = v ->> 'slug';
    ELSE
        UPDATE task SET updated = (v ->> 'now')::double precision, version = version + 1
            WHERE slug = v ->> 'slug';
    END IF;
    IF v ->> 'receipt' IS NOT NULL THEN
        INSERT INTO workflow (id, kind, body, at) VALUES (v -> 'receipt', 'receipt', v -> 'receipt_body',
            (v -> 'transition' ->> 'at')::double precision);
    END IF;
    IF (v ->> 'moved')::boolean THEN
        DELETE FROM counter WHERE id = jsonb_build_array('hold', v ->> 'slug');
    END IF;
    {logged}
    {guard}
END;
"
    ))
}

/// Twin of `super::super::surreal::workflow::PLACED`.
pub(in crate::store) const PLACED: &str = "IF coalesce((SELECT (body ->> 'generation')::bigint FROM workflow
        WHERE id = '[\"active\"]'::jsonb FOR SHARE), 0) <> (v -> 'placing' ->> 'generation')::bigint THEN
    RAISE EXCEPTION '%', 'refused: stale_activation: the workflow activated again while '
        || (v ->> 'slug') || ' was filed; file it again';
END IF;
INSERT INTO workflow (id, kind, body, at) VALUES (jsonb_build_array('event', v ->> 'slug', 1), 'event',
    v -> 'placing', (v -> 'placing' ->> 'at')::double precision);
UPDATE task SET fields = board_fields(id, fields, '{_board,workflow}', v -> 'placed')
    WHERE slug = v ->> 'slug';";

/// Twin of `super::super::surreal::workflow::TRANSITIONS`, by the primary key.
pub(in crate::store) fn transitions() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(w.body) FROM workflow w \
            WHERE w.id IN (SELECT jsonb_array_elements(v -> 'ids'))), '[]')",
    )
}

/// Twin of `super::super::surreal::workflow::MAPPINGS`.
pub(in crate::store) fn mappings() -> Pg<'static> {
    Pg::read(
        "(SELECT body -> 'mappings' FROM workflow WHERE id = jsonb_build_array('activation', v -> 'generation'))",
    )
}

/// Twin of `super::super::surreal::workflow::REOPEN_PLACED`.
pub(in crate::store) const REOPEN_PLACED: &str = "IF coalesce((SELECT (body ->> 'generation')::bigint FROM workflow
        WHERE id = '[\"active\"]'::jsonb FOR SHARE), 0) <> (v -> 'placing' ->> 'generation')::bigint THEN
    RAISE EXCEPTION 'stale';
END IF;
INSERT INTO workflow (id, kind, body, at) VALUES (jsonb_build_array('event', v ->> 'slug', 1), 'event',
    v -> 'placing', (v -> 'placing' ->> 'at')::double precision);
UPDATE task SET status = v -> 'placing' ->> 'status',
    fields = board_fields(id, fields, '{_board,workflow}', v -> 'placed')
    WHERE slug = v ->> 'slug';";

pub(in crate::store) fn receipt() -> Pg<'static> {
    Pg::read("(SELECT body FROM workflow WHERE id = v -> 'receipt')")
}

pub(in crate::store) fn leaving() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(DISTINCT r.slug ORDER BY r.slug) FROM role r JOIN task t ON t.slug = r.slug \
         WHERE r.handle = v ->> 'actor' AND r.role <> 'orchestrator' \
         AND t.fields #> '{_board,workflow}' IS NOT NULL), '[]')",
    )
}
