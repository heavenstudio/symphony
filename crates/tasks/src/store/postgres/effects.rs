//! PostgreSQL twin of `store/admission/effects.rs`'s statements (`super::super::surreal::effects`).

use super::Pg;

pub(in crate::store) fn intent() -> Pg<'static> {
    Pg::read(
        "(SELECT body FROM workflow WHERE id = jsonb_build_array('intent', v ->> 'slug', v -> 'revision'))",
    )
}

pub(in crate::store) fn transition() -> Pg<'static> {
    Pg::read(
        "(SELECT body FROM workflow WHERE id = jsonb_build_array('event', v ->> 'slug', v -> 'revision'))",
    )
}

pub(in crate::store) fn standing() -> Pg<'static> {
    Pg::read(
        "jsonb_build_object(
    'intent', (SELECT body FROM workflow WHERE id = jsonb_build_array('intent', v ->> 'slug', v -> 'revision')),
    'active', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb),
    'task', (SELECT jsonb_build_object('version', version, 'position', fields #> '{_board,workflow}')
        FROM task WHERE slug = v ->> 'slug'))",
    )
}

/// Twin of `super::super::surreal::effects::PREPARED`, spliced into `super::workflow::apply`.
pub(in crate::store) const PREPARED: &str = "INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('intent', v ->> 'slug', v -> 'transition' -> 'revision'), 'intent',
        v -> 'intent', (v -> 'transition' ->> 'at')::double precision);";

/// Twin of `super::super::surreal::effects::SETTLED`.
pub(in crate::store) const SETTLED: &str = "UPDATE workflow SET body = v -> 'intent'
        WHERE id = jsonb_build_array('intent', v ->> 'slug', v -> 'intent' -> 'revision')
        AND body ->> 'stage' = v ->> 'stage_was' AND body ->> 'finalized' IS NULL;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'stale';
    END IF;";

/// Twin of `super::super::surreal::effects::moved`.
pub(in crate::store) fn moved(logged: &str) -> Pg<'static> {
    Pg::script(format!(
        "
BEGIN
    PERFORM 1 FROM task WHERE slug = v ->> 'slug' AND version = (v ->> 'version')::bigint FOR SHARE;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'stale';
    END IF;
    IF coalesce((SELECT (body ->> 'generation')::bigint FROM workflow
            WHERE id = '[\"active\"]'::jsonb FOR SHARE), 0) <> (v ->> 'generation')::bigint
        OR coalesce((SELECT body ->> 'digest' FROM workflow WHERE id = '[\"active\"]'::jsonb), '')
            <> v ->> 'digest' THEN
        RAISE EXCEPTION 'stale';
    END IF;
    {SETTLED}
    {logged}
END;
"
    ))
}
