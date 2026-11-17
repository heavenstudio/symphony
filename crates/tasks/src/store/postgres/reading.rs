//! The snapshot a read of tasks gives on PostgreSQL (`super::super::surreal::reading`): one
//! expression over `v`, the tasks picked, then what the view reads beside them.

use super::row;
use crate::store::{Sessions, Which};

/// Each part of a `Snapshot` a read does not take from what it picked: `callback_sequence` (the
/// whole board's read, [`super::snapshot`]) and `has_callbacks` (a write's, [`written`]).
#[derive(Clone, Copy)]
pub(in crate::store) struct Beside {
    pub callback_sequence: &'static str,
    pub has_callbacks: &'static str,
}

const NOTHING_BESIDE: Beside = Beside {
    callback_sequence: "0",
    has_callbacks: "false",
};

/// Whether any session that has not left keeps a callback address.
const HAS_CALLBACKS: &str = "EXISTS (SELECT 1 FROM callback_state c
    WHERE c.doc ? 'address'
    AND (SELECT s.left_at FROM session s WHERE s.handle = c.doc ->> 'handle') IS NULL)";

/// The read of `which` with the sessions `sessions` asks for, as an expression; none where it has
/// no twin yet (the slot holdings' ledger, and the reads of many tasks but the whole board's).
pub(in crate::store) fn reading(which: &Which, sessions: Sessions) -> Option<String> {
    reading_with(which, sessions, NOTHING_BESIDE)
}

/// [`reading`], with what `beside` gives.
pub(in crate::store) fn reading_with(
    which: &Which,
    sessions: Sessions,
    beside: Beside,
) -> Option<String> {
    let (picked, known, deps) = match which {
        Which::All => ("SELECT t.* FROM task t", "true", false),
        Which::Slug(_) => (
            "SELECT t.* FROM task t WHERE t.slug = v ->> 'slug'",
            "EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug')",
            true,
        ),
        _ => return None,
    };
    let scoped = |alias: &str| match which {
        Which::All => "true".to_string(),
        _ => format!("{alias}.slug IN (SELECT slug FROM picked)"),
    };
    let open_deps = if deps {
        "coalesce((SELECT jsonb_agg(jsonb_build_object('slug', l.slug, 'kind', l.kind, 'target', l.target)
            ORDER BY l.target)
        FROM link l JOIN task t ON t.slug = l.target
        WHERE l.slug IN (SELECT slug FROM picked) AND l.kind = 'dep' AND t.status <> 'done'), '[]'::jsonb)"
    } else {
        "'[]'::jsonb"
    };
    let sessions = match sessions {
        Sessions::Whole => "session s".to_string(),
        Sessions::Shown => {
            "session s WHERE s.handle IN (SELECT handle FROM view_roles)".to_string()
        }
    };
    let Beside {
        callback_sequence,
        has_callbacks,
    } = beside;
    Some(format!(
        "(WITH picked AS ({picked}),
    view_roles AS (SELECT r.slug, r.handle, r.role, r.since, r.expires FROM role r WHERE {}),
    open_slugs AS (SELECT slug FROM task WHERE status IN (SELECT jsonb_array_elements_text(v -> 'executing'))),
    held AS (SELECT DISTINCT r.handle FROM role r
        WHERE (r.role = 'executor' AND r.slug IN (SELECT slug FROM open_slugs))
            OR (r.role = 'reviewer' AND r.slug IN (SELECT slug FROM task WHERE status = 'review')))
SELECT jsonb_build_object(
    'tasks', coalesce((SELECT jsonb_agg({} ORDER BY p.priority, p.sid) FROM picked p), '[]'::jsonb),
    'branches', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', b.slug, 'branch', b.branch,
            'handle', b.handle, 'started', b.started))
        FROM task_session b WHERE {} AND b.role = 'executor' AND b.branch <> ''), '[]'::jsonb),
    'links', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', l.slug, 'kind', l.kind, 'target', l.target)
            ORDER BY l.target)
        FROM link l WHERE {}), '[]'::jsonb),
    'roles', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(to_jsonb(r)) ORDER BY r.since, r.handle)
        FROM view_roles r), '[]'::jsonb),
    'sessions', coalesce((SELECT jsonb_agg({} ORDER BY s.connected, s.handle) FROM {sessions}), '[]'::jsonb),
    'held', coalesce((SELECT jsonb_agg(handle) FROM held), '[]'::jsonb),
    'latest', coalesce((SELECT to_jsonb(last) FROM counter WHERE id = '\"message\"'), '0'::jsonb),
    'callback_sequence', {callback_sequence},
    'has_callbacks', {has_callbacks},
    'native_presence', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', c.doc -> 'handle',
            'until', coalesce(c.doc -> 'confirmed_until', '0'::jsonb)))
        FROM callback_state c WHERE starts_with(c.doc ->> 'address', 'codex:')), '[]'::jsonb),
    'open_deps', {open_deps},
    'known', {known},
    'mine', '[]'::jsonb))",
        scoped("r"),
        row("p"),
        scoped("b"),
        scoped("l"),
        row("s"),
    ))
}

/// `slug` read as `get` reads it after a write, with whether any session keeps a callback address
/// (`Store::written`).
pub(in crate::store) fn written(which: &Which) -> Option<String> {
    reading_with(
        which,
        Sessions::Shown,
        Beside {
            callback_sequence: "0",
            has_callbacks: HAS_CALLBACKS,
        },
    )
}
