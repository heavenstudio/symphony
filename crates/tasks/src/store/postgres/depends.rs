//! The statements of `store/depends.rs` on PostgreSQL.

use super::cap::{GUARD, UNHOLD};
use super::work::{UNBLOCKED, said};
use super::{Pg, act};

/// Every task's status, every dependency, the count of changes to them, and the task's own version,
/// thread and executor, as `change_depends` decides on them.
pub(in crate::store) const READ: Pg<'static> = Pg::Read(std::borrow::Cow::Borrowed(
    "jsonb_build_object( \
    'tasks', coalesce((SELECT jsonb_agg(jsonb_build_array(slug, status) ORDER BY slug) FROM task), '[]'), \
    'deps', coalesce((SELECT jsonb_agg(jsonb_build_array(slug, target) ORDER BY slug, target) FROM link \
        WHERE kind = 'dep'), '[]'), \
    'edits', coalesce((SELECT last FROM counter WHERE id = '\"depends\"'), 0), \
    'version', (SELECT version FROM task WHERE slug = v ->> 'slug'), \
    'lines', coalesce((SELECT jsonb_agg(text ORDER BY sid) FROM message \
        WHERE slug = v ->> 'slug' AND kind = 'system'), '[]'), \
    'executing', EXISTS (SELECT 1 FROM role WHERE slug = v ->> 'slug' AND role = 'executor'))",
));

/// Stale unless no dependency changed anywhere, the task is the version read and what it still
/// waits on has the status read; then the links change, and the task with them.
const CHANGE: &str = "IF coalesce((SELECT last FROM counter WHERE id = '\"depends\"'), 0) \
        <> (v ->> 'edits')::bigint \
    OR (SELECT version FROM task WHERE slug = v ->> 'slug') IS DISTINCT FROM (v ->> 'version')::bigint \
    OR EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'watch') AS d \
        WHERE (SELECT status FROM task WHERE slug = d ->> 'slug') IS DISTINCT FROM d ->> 'status') THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
INSERT INTO counter (id, last) VALUES ('\"depends\"', 1) \
    ON CONFLICT (id) DO UPDATE SET last = counter.last + 1; \
DELETE FROM link WHERE id IN (SELECT jsonb_build_array(v ->> 'slug', 'dep', t) \
    FROM jsonb_array_elements_text(v -> 'drop') AS t); \
INSERT INTO link (id, slug, kind, target) \
    SELECT DISTINCT jsonb_build_array(v ->> 'slug', 'dep', t), v ->> 'slug', 'dep', t \
    FROM jsonb_array_elements_text(v -> 'add') AS t \
    ON CONFLICT (id) DO NOTHING; \
UPDATE task SET status = coalesce(v ->> 'status', status), updated = (v ->> 'now')::double precision, \
    version = version + 1 WHERE slug = v ->> 'slug';";

/// [`CHANGE`] with its line on the thread (`said`) and, when the task goes back to a status
/// (`back`), that line too, by the actor, and what its block waited on removed ([`UNBLOCKED`]): a
/// session's write when `acts`, else the owner's; with the slot counters it holds (`slots`) when
/// `guarded`.
pub(in crate::store) fn change(acts: bool, guarded: bool) -> Pg<'static> {
    let act = if acts { act() } else { String::new() };
    let guard = if guarded { GUARD } else { "" };
    Pg::script(format!(
        "{act}\n{CHANGE}\n{UNHOLD}\n{UNBLOCKED}\n{}\nIF v ? 'back' THEN\n{}\nEND IF;\n{guard}",
        said("v ->> 'said'"),
        said("v ->> 'back'")
    ))
}
