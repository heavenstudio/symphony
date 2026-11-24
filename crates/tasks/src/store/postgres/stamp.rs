//! The statement of `store/stamp.rs` on PostgreSQL.

use super::{Pg, row};

/// The task `slug`'s row, its links, its roles, the branches its executors' sessions recorded,
/// the tasks right under it and its thread's newest line.
pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object( \
    'task', (SELECT {} FROM task t WHERE t.slug = v ->> 'slug'), \
    'links', coalesce((SELECT jsonb_agg(jsonb_build_object('kind', kind, 'target', target) \
        ORDER BY kind, target) FROM link WHERE slug = v ->> 'slug'), '[]'), \
    'roles', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object('handle', handle, \
            'role', role, 'since', since, 'expires', expires)) ORDER BY since, handle, role) \
        FROM role WHERE slug = v ->> 'slug'), '[]'), \
    'branches', coalesce((SELECT jsonb_agg(jsonb_build_object('branch', branch, 'handle', handle, \
            'started', started) ORDER BY started, handle) \
        FROM task_session WHERE slug = v ->> 'slug' AND role = 'executor' AND branch <> ''), '[]'), \
    'under', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', slug, 'sid', sid) ORDER BY sid) \
        FROM task WHERE parent = v ->> 'slug'), '[]'), \
    'newest', (SELECT max(sid) FROM message WHERE slug = v ->> 'slug'))",
        row("t")
    ))
}
