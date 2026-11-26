//! The statement of `store/stamp.rs` on PostgreSQL.

use super::people::{people_held, people_object};
use super::{Pg, row};

/// The task `slug`'s row, its links, its roles, the branches its executors' sessions recorded,
/// the tasks right under it and its thread's newest line; and, on a board with the steps that keep
/// them, the board's people (`people`) and the lines of its map of places (`map`, its maps and
/// unmaps: a hand-over of the task changes its row), from which a reader of the task's `owner`
/// stamp says who decides it: one request, a block that names the people's and the map's tables
/// only where the board has them.
pub(in crate::store) fn read() -> Pg<'static> {
    Pg::script(format!(
        "board_answer := jsonb_build_object( \
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
    'newest', (SELECT max(sid) FROM message WHERE slug = v ->> 'slug'));
IF {held} THEN
    board_answer := board_answer || jsonb_build_object('people', {people});
END IF;
IF to_regclass('ownership') IS NOT NULL THEN
    board_answer := board_answer || jsonb_build_object('map', coalesce((SELECT jsonb_agg(doc \
        ORDER BY id) FROM ownership WHERE doc ->> 'did' <> 'hand'), '[]'::jsonb));
END IF;",
        row("t"),
        held = people_held!(),
        people = people_object!(),
    ))
}
