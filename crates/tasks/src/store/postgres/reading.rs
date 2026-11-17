//! The snapshot a read of tasks gives on PostgreSQL (`super::super::surreal::reading`): one
//! expression over `v`, the tasks picked, then what the view reads beside them.

use super::branches::of_tasks;
use super::{cap, row};
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

/// Whether the task `v ->> 'slug'` is there.
const KNOWN: &str = "EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug')";

/// The read of `which` with the sessions `sessions` asks for, as an expression.
pub(in crate::store) fn reading(which: &Which, sessions: Sessions) -> String {
    reading_with(which, sessions, NOTHING_BESIDE)
}

/// What `which` picks: its tasks (rows `t` of `task`), the order they come in (`p` a row of
/// them), and when the one it is about is there.
fn picking(which: &Which) -> (&'static str, &'static str, &'static str) {
    const BY_PRIORITY: &str = "p.priority, p.sid";
    match which {
        Which::All | Which::SlotHoldings => ("", BY_PRIORITY, "true"),
        Which::Status(_) => ("WHERE t.status = v ->> 'status'", BY_PRIORITY, "true"),
        Which::Slug(_) | Which::Starting(_) => ("WHERE t.slug = v ->> 'slug'", "p.sid", KNOWN),
        Which::Slugs(_) => (
            "WHERE t.slug IN (SELECT jsonb_array_elements_text(v -> 'wanted'))",
            BY_PRIORITY,
            "true",
        ),
        Which::Children(_) => ("WHERE t.parent = v ->> 'slug'", "p.sid", KNOWN),
        Which::Pullable(_) => (
            "WHERE t.status IN (SELECT jsonb_array_elements_text(v -> 'pullable'))",
            "p.sid",
            "EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle')",
        ),
        Which::Linked(_) => (
            "WHERE t.slug IN (SELECT l.slug FROM link l WHERE l.kind = 'doc'
        AND (l.target = v ->> 'doc' OR starts_with(l.target, (v ->> 'doc') || '#')))",
            "p.priority, p.slug",
            "true",
        ),
    }
}

/// [`reading`], with what `beside` gives.
pub(in crate::store) fn reading_with(which: &Which, sessions: Sessions, beside: Beside) -> String {
    let (picked, order, known) = picking(which);
    let whole = matches!(which, Which::All | Which::SlotHoldings);
    let scoped = |alias: &str| match whole {
        true => "true".to_string(),
        false => format!("{alias}.slug IN (SELECT slug FROM picked)"),
    };
    let one = matches!(which, Which::Slug(_) | Which::Starting(_));
    let open_deps = if one || matches!(which, Which::Pullable(_)) {
        "coalesce((SELECT jsonb_agg(jsonb_build_object('slug', l.slug, 'kind', l.kind, 'target', l.target)
            ORDER BY l.target, l.slug)
        FROM link l JOIN task t ON t.slug = l.target
        WHERE l.slug IN (SELECT slug FROM picked) AND l.kind = 'dep' AND t.status <> 'done'), '[]'::jsonb)"
    } else {
        "'[]'::jsonb"
    };
    let (mine, asker) = match which {
        Which::Pullable(_) => (
            "coalesce((SELECT jsonb_agg(slug ORDER BY slug, role) FROM role WHERE handle = v ->> 'handle'),
        '[]'::jsonb)",
            " OR s.handle = v ->> 'handle'",
        ),
        _ => ("'[]'::jsonb", ""),
    };
    let sessions = match sessions {
        Sessions::Whole => "session s".to_string(),
        Sessions::Shown => {
            format!("session s WHERE s.handle IN (SELECT handle FROM view_roles){asker}")
        }
    };
    // The cap's read beside a start's task and the whole board's holdings (`super::cap`), and the
    // branches of every task it reads; else those of the tasks picked.
    let (slot_lets, slots, wanted) = match which {
        Which::Starting(_) => (
            format!(",\n    {}", cap::lets("SELECT v ->> 'slug'")),
            cap::field(),
            "slot_wanted",
        ),
        Which::SlotHoldings => (
            format!(",\n    {}", cap::lets("SELECT slug FROM task")),
            cap::field(),
            "slot_wanted",
        ),
        _ => (String::new(), String::new(), "picked"),
    };
    let branches = of_tasks(&format!("b.slug IN (SELECT slug FROM {wanted})"));
    let Beside {
        callback_sequence,
        has_callbacks,
    } = beside;
    format!(
        "(WITH picked AS (SELECT t.* FROM task t {picked}),
    view_roles AS (SELECT r.slug, r.handle, r.role, r.since, r.expires FROM role r WHERE {}),
    open_slugs AS (SELECT slug FROM task WHERE status IN (SELECT jsonb_array_elements_text(v -> 'executing'))),
    held AS (SELECT DISTINCT r.handle FROM role r
        WHERE (r.role = 'executor' AND r.slug IN (SELECT slug FROM open_slugs))
            OR (r.role = 'reviewer' AND r.slug IN (SELECT slug FROM task WHERE status = 'review'))){slot_lets}
SELECT jsonb_build_object(
    'tasks', coalesce((SELECT jsonb_agg({} ORDER BY {order}) FROM picked p), '[]'::jsonb),
    'branches', {branches},
    'links', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', l.slug, 'kind', l.kind, 'target', l.target)
            ORDER BY l.target, l.slug, l.kind)
        FROM link l WHERE {}), '[]'::jsonb),
    'roles', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(to_jsonb(r)) ORDER BY r.since, r.handle, r.slug, r.role)
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
    {slots}
    'known', {known},
    'mine', {mine}))",
        scoped("r"),
        row("p"),
        scoped("l"),
        row("s"),
    )
}

/// `slug` read as `get` reads it after a write, with whether any session keeps a callback address
/// (`Store::written`).
pub(in crate::store) fn written(which: &Which) -> String {
    reading_with(
        which,
        Sessions::Shown,
        Beside {
            callback_sequence: "0",
            has_callbacks: HAS_CALLBACKS,
        },
    )
}
