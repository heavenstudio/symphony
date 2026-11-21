//! PostgreSQL twin of `store/workflow.rs`'s statements (`super::super::surreal::workflow`).

use super::Pg;

pub(in crate::store) fn active() -> Pg<'static> {
    Pg::read("(SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb)")
}

/// Twin of `super::super::surreal::workflow::ACTIVE_SOURCE`.
pub(in crate::store) fn active_source() -> Pg<'static> {
    Pg::read(
        "(SELECT jsonb_build_object('active', a.body, 'revision', d.body -> 'revision')
    FROM workflow a LEFT JOIN workflow d ON d.id = jsonb_build_array('definition', a.body ->> 'digest')
    WHERE a.id = '[\"active\"]'::jsonb)",
    )
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
    'last', coalesce((SELECT max((body ->> 'generation')::bigint) FROM workflow
        WHERE id >= '[\"activation\", 0]'::jsonb AND id <= '[\"activation\", 9223372036854775807]'::jsonb), 0),
    'tasks', coalesce((SELECT jsonb_agg(jsonb_build_object(
        'slug', slug, 'version', version, 'status', status,
        'position', fields #> '{_board,workflow}',
        'blocked_from', fields #> '{_board,blocked,from}', 'blocked', fields #> '{_board,blocked}',
        'retired', coalesce((fields #>> '{_board,workflow_retired,revision}')::bigint, 0),
        'accepted', (SELECT w.body -> 'accepted' FROM workflow w
            WHERE w.id = jsonb_build_array('event', task.slug, task.fields #> '{_board,workflow,revision}')),
        'attempts', coalesce((SELECT jsonb_agg(a.doc -> 'attempt'
            ORDER BY (a.doc ->> 'created')::bigint, a.doc ->> 'attempt' COLLATE \"C\")
            FROM execution_attempt a WHERE a.doc ->> 'task' = task.slug AND (a.doc ->> 'live')::boolean
                AND coalesce(a.doc #>> '{state,cancellation}', 'none') = 'none'), '[]'::jsonb))
        ORDER BY slug)
        FROM task WHERE status NOT IN ('done', 'cancelled') OR fields #> '{_board,workflow}' IS NOT NULL),
        '[]'::jsonb))",
    )
}

/// Twin of `super::super::surreal::workflow::ACTIVATE`, the board's marker moved by
/// `board_bump`, once in its transaction as every trigger's.
pub(in crate::store) fn activate() -> Pg<'static> {
    Pg::script(
        "
DECLARE
    active_ jsonb;
    t_ jsonb;
    stale_ text;
    at_ double precision := (v ->> 'at')::double precision;
BEGIN
    SELECT body INTO active_ FROM workflow WHERE id = '[\"active\"]'::jsonb FOR UPDATE;
    IF coalesce((active_ ->> 'generation')::bigint, 0) <> (v ->> 'expected')::bigint THEN
        RAISE EXCEPTION '%', 'refused: stale_workflow: the active generation is '
            || coalesce(active_ ->> 'generation', '0') || ', not ' || (v ->> 'expected');
    END IF;
    IF EXISTS (SELECT 1 FROM workflow WHERE id = jsonb_build_array('activation', v -> 'generation')) THEN
        RAISE EXCEPTION '%', 'refused: stale_workflow: generation ' || (v ->> 'generation')
            || ' is activated already';
    END IF;
    PERFORM 1 FROM session WHERE handle IN (SELECT s ->> 'handle' FROM jsonb_array_elements(v -> 'seen') AS s)
        FOR SHARE;
    SELECT s ->> 'handle' INTO stale_ FROM jsonb_array_elements(v -> 'seen') AS s
        WHERE NOT EXISTS (SELECT 1 FROM session WHERE handle = s ->> 'handle'
            AND last_seen = (s ->> 'last_seen')::double precision)
        LIMIT 1;
    IF stale_ IS NOT NULL THEN
        RAISE EXCEPTION '%', 'refused: stale_task: ' || stale_ || ' was seen while its workflow activated';
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
    SELECT slug INTO stale_ FROM task
        WHERE (status NOT IN ('done', 'cancelled') OR fields #> '{_board,workflow}' IS NOT NULL)
            AND slug NOT IN (SELECT p ->> 'slug' FROM jsonb_array_elements((v -> 'kept') || (v -> 'moved')) AS p)
        ORDER BY slug LIMIT 1;
    IF stale_ IS NOT NULL THEN
        RAISE EXCEPTION '%', 'refused: stale_task: ' || stale_ || ' was filed or reopened while its workflow activated';
    END IF;
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('definition', v ->> 'digest'), 'definition', v -> 'definition', at_)
        ON CONFLICT (id) DO NOTHING;
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('activation', v -> 'generation'), 'activation', v -> 'activation', at_);
    INSERT INTO workflow (id, kind, body, at) VALUES ('[\"active\"]', 'active',
        jsonb_build_object('generation', v -> 'generation', 'digest', v ->> 'digest'), at_)
        ON CONFLICT (id) DO UPDATE SET body = EXCLUDED.body, at = EXCLUDED.at;
    PERFORM board_bump();
END;
",
    )
}

/// Twin of `super::super::surreal::workflow::RETURN_DEFAULT`.
pub(in crate::store) fn return_default() -> Pg<'static> {
    Pg::script(
        "
DECLARE
    active_ jsonb;
    t_ jsonb;
    stale_ text;
    at_ double precision := (v ->> 'at')::double precision;
BEGIN
    SELECT body INTO active_ FROM workflow WHERE id = '[\"active\"]'::jsonb FOR UPDATE;
    IF coalesce((active_ ->> 'generation')::bigint, 0) <> (v ->> 'expected')::bigint THEN
        RAISE EXCEPTION '%', 'refused: stale_workflow: the active generation is '
            || coalesce(active_ ->> 'generation', '0') || ', not ' || (v ->> 'expected');
    END IF;
    PERFORM 1 FROM session WHERE handle IN (SELECT s ->> 'handle' FROM jsonb_array_elements(v -> 'seen') AS s)
        FOR SHARE;
    SELECT s ->> 'handle' INTO stale_ FROM jsonb_array_elements(v -> 'seen') AS s
        WHERE NOT EXISTS (SELECT 1 FROM session WHERE handle = s ->> 'handle'
            AND last_seen = (s ->> 'last_seen')::double precision)
        LIMIT 1;
    IF stale_ IS NOT NULL THEN
        RAISE EXCEPTION '%', 'refused: stale_task: ' || stale_ || ' was seen while its workflow returned';
    END IF;
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('activation', v -> 'generation'), 'activation', v -> 'activation', at_);
    FOR t_ IN SELECT value FROM jsonb_array_elements(v -> 'kept') LOOP
        UPDATE task SET version = version + 1
            WHERE slug = t_ ->> 'slug' AND version = (t_ ->> 'version')::bigint;
        IF NOT FOUND THEN
            RAISE EXCEPTION '%', 'refused: stale_task: ' || (t_ ->> 'slug') || ' changed while its workflow returned';
        END IF;
    END LOOP;
    FOR t_ IN SELECT value FROM jsonb_array_elements(v -> 'moved') LOOP
        PERFORM 1 FROM task WHERE slug = t_ ->> 'slug' AND version = (t_ ->> 'version')::bigint
            AND coalesce((fields #>> '{_board,workflow,revision}')::bigint, 0) = (t_ ->> 'expected')::bigint
            FOR UPDATE;
        IF NOT FOUND THEN
            RAISE EXCEPTION '%', 'refused: stale_task: ' || (t_ ->> 'slug') || ' changed while its workflow returned';
        END IF;
        INSERT INTO workflow (id, kind, body, at) VALUES (
            jsonb_build_array('event', t_ ->> 'slug', t_ -> 'transition' -> 'revision'), 'event',
            t_ -> 'transition', at_);
        UPDATE task SET status = t_ -> 'transition' ->> 'status',
            fields = board_fields(id, board_fields(id, board_fields(id, fields,
                '{_board,workflow}', NULL),
                '{_board,workflow_retired}', t_ -> 'retired'),
                '{_board,blocked}', nullif(t_ -> 'blocked', 'null'::jsonb)),
            updated = at_, version = version + 1
            WHERE slug = t_ ->> 'slug';
    END LOOP;
    SELECT slug INTO stale_ FROM task
        WHERE (status NOT IN ('done', 'cancelled') OR fields #> '{_board,workflow}' IS NOT NULL)
            AND slug NOT IN (SELECT p ->> 'slug' FROM jsonb_array_elements((v -> 'kept') || (v -> 'moved')) AS p)
        ORDER BY slug LIMIT 1;
    IF stale_ IS NOT NULL THEN
        RAISE EXCEPTION '%', 'refused: stale_task: ' || stale_ || ' was filed or reopened while its workflow returned';
    END IF;
    DELETE FROM workflow WHERE id = '[\"active\"]'::jsonb;
END;
",
    )
}

pub(in crate::store) fn admission() -> Pg<'static> {
    Pg::read(
        "jsonb_build_object(
    'active', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb),
    'task', (SELECT jsonb_build_object('slug', slug, 'version', version, 'status', status,
        'created_by', created_by, 'position', fields #> '{_board,workflow}',
        'accepted', (SELECT w.body -> 'accepted' FROM workflow w
            WHERE w.id = jsonb_build_array('event', task.slug, task.fields #> '{_board,workflow,revision}')))
        FROM task WHERE slug = v ->> 'slug'),
    'roles', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', handle, 'role', role,
        'expires', expires)) FROM role WHERE slug = v ->> 'slug'), '[]'::jsonb),
    'dependencies', coalesce((SELECT jsonb_agg(target) FROM link
        WHERE slug = v ->> 'slug' AND kind = 'dep'), '[]'::jsonb),
    'found', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', t.slug, 'version', t.version,
        'status', t.status, 'position', t.fields #> '{_board,workflow}',
        'accepted', (SELECT w.body -> 'accepted' FROM workflow w
            WHERE w.id = jsonb_build_array('event', t.slug, t.fields #> '{_board,workflow,revision}'))))
        FROM task t JOIN link l ON l.target = t.slug
        WHERE l.slug = v ->> 'slug' AND l.kind = 'dep'), '[]'::jsonb),
    'receipt', (SELECT body FROM workflow WHERE id = v -> 'receipt'),
    'present', coalesce((SELECT left_at IS NULL FROM session WHERE handle = v ->> 'actor'), false),
    'project', (SELECT body -> 'project' FROM workflow
        WHERE id = jsonb_build_array('activation',
            (SELECT body -> 'generation' FROM workflow WHERE id = '[\"active\"]'::jsonb))),
    'attesters', ATTESTERS,
    'accepted', coalesce((SELECT jsonb_agg(w.body ORDER BY w.id) FROM workflow w
        WHERE w.id IN (SELECT jsonb_build_array('authority', a ->> 'adapter')
            FROM jsonb_array_elements(ATTESTERS) AS a)), '[]'::jsonb))"
            .replace("ATTESTERS", ATTESTERS),
    )
}

/// The approval authorities the active activation commits, an empty array where none.
pub(in crate::store) const ATTESTERS: &str = "coalesce((SELECT body -> 'authorities' FROM workflow
        WHERE id = jsonb_build_array('activation',
            (SELECT body -> 'generation' FROM workflow WHERE id = '[\"active\"]'::jsonb))), '[]'::jsonb)";

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
    PERFORM 1 FROM workflow WHERE id IN (SELECT jsonb_build_array('authority', a ->> 'adapter')
        FROM jsonb_array_elements(v -> 'authorities') AS a) FOR UPDATE;
    IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'authorities') AS a
            WHERE NOT EXISTS (SELECT 1 FROM workflow w
                WHERE w.id = jsonb_build_array('authority', a ->> 'adapter')
                AND w.body ->> 'epoch' = a ->> 'epoch'
                AND (w.body ->> 'generation')::bigint = (a ->> 'generation')::bigint
                AND w.body ->> 'state' = a ->> 'state'
                AND NOT coalesce((w.body ->> 'conflicted')::boolean, false))) THEN
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
    UPDATE workflow SET body = jsonb_set(body, '{{admitted}}',
            to_jsonb(coalesce((body ->> 'admitted')::bigint, 0) + 1))
        WHERE id IN (SELECT jsonb_build_array('authority', a ->> 'adapter')
            FROM jsonb_array_elements(v -> 'authorities') AS a);
    IF v ->> 'admission' IS NOT NULL THEN
        INSERT INTO workflow (id, kind, body, at) VALUES (
            jsonb_build_array('admission', v ->> 'slug', v -> 'transition' -> 'revision'), 'admission',
            v -> 'admission', (v -> 'transition' ->> 'at')::double precision);
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

/// Twin of `super::super::surreal::workflow::TRANSITIONS`, by the primary key, in the order the
/// ids are given.
pub(in crate::store) fn transitions() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(w.body ORDER BY i.n) \
            FROM jsonb_array_elements(v -> 'ids') WITH ORDINALITY i(id, n) \
            JOIN workflow w ON w.id = i.id), '[]')",
    )
}

/// Twin of `super::super::surreal::workflow::MAPPINGS`.
pub(in crate::store) fn mappings() -> Pg<'static> {
    Pg::read(
        "(SELECT body -> 'mappings' FROM workflow WHERE id = jsonb_build_array('activation', v -> 'generation'))",
    )
}

/// Twin of `super::super::surreal::workflow::AUTHORITIES`.
pub(in crate::store) fn authorities() -> Pg<'static> {
    Pg::read(
        "(SELECT jsonb_build_object('project', body -> 'project', 'authorities', body -> 'authorities')
        FROM workflow WHERE id = jsonb_build_array('activation', v -> 'generation'))",
    )
}

/// Twin of `super::super::surreal::workflow::REOPEN_PLACED`.
pub(in crate::store) const REOPEN_PLACED: &str = "IF coalesce((SELECT (body ->> 'generation')::bigint FROM workflow
        WHERE id = '[\"active\"]'::jsonb FOR SHARE), 0) <> (v -> 'placing' ->> 'generation')::bigint THEN
    RAISE EXCEPTION 'stale';
END IF;
INSERT INTO workflow (id, kind, body, at) VALUES (
    jsonb_build_array('event', v ->> 'slug', v -> 'placing' -> 'revision'), 'event',
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
