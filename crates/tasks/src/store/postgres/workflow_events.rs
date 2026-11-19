//! PostgreSQL twin of `store/workflow_events.rs`'s statement
//! (`super::super::surreal::workflow_events`).

use super::Pg;

/// Twin of `TRANSITIONS`. A key is a jsonb array, which PostgreSQL orders by its length, then part
/// by part: a null below every number and `true` above, so the range holds every revision of the
/// slug's events, in order, and reads them from the key's index.
pub(in crate::store) fn transitions() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(body ORDER BY id) FROM workflow \
         WHERE id > jsonb_build_array('event', v ->> 'slug', null) \
         AND id < jsonb_build_array('event', v ->> 'slug', true)), '[]'::jsonb)",
    )
}
