//! PostgreSQL twin of `store/guard.rs`'s statements (`super::super::surreal::guard`).

use super::Pg;
use super::workflow::ATTESTERS;

/// The states accepted from the authorities in `attesters`, an array.
fn accepted(attesters: &str) -> String {
    format!(
        "coalesce((SELECT jsonb_agg(w.body ORDER BY w.id) FROM workflow w
        WHERE w.id IN (SELECT jsonb_build_array('authority', a ->> 'adapter')
            FROM jsonb_array_elements({attesters}) AS a)), '[]'::jsonb)"
    )
}

pub(in crate::store) fn authorities() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('attesters', {ATTESTERS}, 'accepted', {})",
        accepted(ATTESTERS)
    ))
}

pub(in crate::store) fn authority() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object(
    'generation', (SELECT body -> 'generation' FROM workflow WHERE id = '[\"active\"]'::jsonb),
    'attesters', {ATTESTERS},
    'accepted', (SELECT body FROM workflow WHERE id = jsonb_build_array('authority', v ->> 'adapter')))"
    ))
}

pub(in crate::store) fn observe() -> Pg<'static> {
    Pg::script(
        "
DECLARE
    held_ jsonb;
BEGIN
    PERFORM 1 FROM workflow WHERE id = '[\"active\"]'::jsonb FOR SHARE;
    IF (SELECT (body ->> 'generation')::bigint FROM workflow WHERE id = '[\"active\"]'::jsonb)
            IS DISTINCT FROM (v ->> 'generation')::bigint THEN
        RAISE EXCEPTION 'stale';
    END IF;
    SELECT body INTO held_ FROM workflow
        WHERE id = jsonb_build_array('authority', v ->> 'adapter') FOR UPDATE;
    IF held_ IS DISTINCT FROM v -> 'expected' THEN
        RAISE EXCEPTION 'stale';
    END IF;
    -- A first observation locks no row above, so another first one may commit in between. At the
    -- store's repeatable read that update is a serialization failure, tried again; the update
    -- also takes the row only at the state this one read, so it holds at any isolation.
    INSERT INTO workflow (id, kind, body, at) VALUES (
        jsonb_build_array('authority', v ->> 'adapter'), 'authority', v -> 'body',
        (v ->> 'now')::double precision)
        ON CONFLICT (id) DO UPDATE SET body = EXCLUDED.body, at = EXCLUDED.at
        WHERE workflow.body IS NOT DISTINCT FROM v -> 'expected';
    IF NOT FOUND THEN
        RAISE EXCEPTION 'stale';
    END IF;
END;
",
    )
}

pub(in crate::store) fn admission_record() -> Pg<'static> {
    Pg::read(
        "(SELECT body FROM workflow
        WHERE id = jsonb_build_array('admission', v ->> 'slug', v -> 'revision'))",
    )
}

pub(in crate::store) fn dispatch() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object(
    'active', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb),
    'found', EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug'),
    'position', (SELECT fields #> '{{_board,workflow}}' FROM task WHERE slug = v ->> 'slug'),
    'transition', (SELECT body FROM workflow
        WHERE id = jsonb_build_array('event', v ->> 'slug', v -> 'revision')),
    'admission', (SELECT body FROM workflow
        WHERE id = jsonb_build_array('admission', v ->> 'slug', v -> 'revision')),
    'project', (SELECT body -> 'project' FROM workflow
        WHERE id = jsonb_build_array('activation',
            (SELECT body -> 'generation' FROM workflow WHERE id = '[\"active\"]'::jsonb))),
    'attesters', {ATTESTERS},
    'accepted', {})",
        accepted(ATTESTERS)
    ))
}
