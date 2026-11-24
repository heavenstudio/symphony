//! The statements of `store/branches.rs` on PostgreSQL.

use super::Pg;

/// The branches recorded for the tasks `scope` keeps (a condition on `b`, a `task_session` row),
/// as a list.
pub(in crate::store) fn of_tasks(scope: &str) -> String {
    format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('slug', b.slug, 'branch', b.branch, \
            'handle', b.handle, 'started', b.started)) \
        FROM task_session b WHERE {scope} AND b.role = 'executor' AND b.branch <> ''), '[]')"
    )
}

/// The tasks recorded on the branches `v -> 'slot_wanted'` names, as rows of `slug`.
pub(in crate::store) const ON_BRANCHES: &str = "SELECT s.slug FROM task_session s \
    WHERE s.branch IN (SELECT jsonb_array_elements_text(v -> 'slot_wanted')) AND s.role = 'executor'";

/// A branch and the tasks recorded on it: `v ->> 'branch'` first, then the others.
pub(in crate::store) fn on_branch() -> Pg<'static> {
    Pg::read(format!(
        "(SELECT jsonb_agg(o.slug ORDER BY o.at, o.slug) FROM (SELECT v ->> 'branch' AS slug, 0 AS at \
    UNION SELECT r.slug, 1 FROM ({ON_BRANCHES}) AS r WHERE r.slug <> v ->> 'branch') AS o)"
    ))
}
