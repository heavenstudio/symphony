//! The statements of `store/admission/effects.rs` on PostgreSQL.

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

/// A prepare edge's intent recorded at its transition's revision, spliced into
/// `super::workflow::apply`.
pub(in crate::store) const PREPARED: &str = "INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('intent', v ->> 'slug', v -> 'transition' -> 'revision'), 'intent',
        v -> 'intent', (v -> 'transition' ->> 'at')::double precision);";

/// The intent written over the one read at `stage_was`, not yet finalized: `stale` where it moved.
pub(in crate::store) const SETTLED: &str = "UPDATE workflow SET body = v -> 'intent'
        WHERE id = jsonb_build_array('intent', v ->> 'slug', v -> 'intent' -> 'revision')
        AND body ->> 'stage' = v ->> 'stage_was' AND body ->> 'finalized' IS NULL;
    IF NOT FOUND THEN
        RAISE EXCEPTION 'stale';
    END IF;";

/// The intent moved on ([`SETTLED`]) with no transition of its task, fenced on the task's version
/// and the active workflow, then the line `logged`: `stale` where either moved.
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
