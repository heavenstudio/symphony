//! The statements of `store/history.rs` on PostgreSQL.

use super::Pg;
use crate::store::history::PAGE;

/// A line `m` of a thread as the alarm reads it ([`crate::alarm::Line`]).
const LINE: &str =
    "jsonb_build_object('slug', m.slug, 'sid', m.sid, 'ts', m.ts, 'kind', m.kind, 'text', m.text)";

/// Whether the text of `m` holds the text the variable `name` names.
fn holds(name: &str) -> String {
    format!("strpos(m.text, v ->> '{name}') > 0")
}

/// Whether the text of `m` starts with the text the variable `name` names.
fn starts(name: &str) -> String {
    format!("starts_with(m.text, v ->> '{name}')")
}

/// A line `m` that may say a status changed: a verdict, or the system line of a move, a claim, a
/// block by a cancelled dependency or the end of a block, and `more`.
fn step(more: &str) -> String {
    format!(
        "m.kind IN ('system', 'verdict') AND (m.kind = 'verdict' OR {} OR {} OR {} OR {}{more})",
        holds("moved"),
        holds("claimed"),
        starts("block"),
        starts("unblock")
    )
}

/// The lines since `since` that may say a status changed, a change to what a task's landing waits
/// on and a move in the tree among them (`super::super::surreal::history::STATUS_LINES`).
pub(in crate::store) fn status_lines() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg({LINE} ORDER BY m.slug, m.sid) FROM message m \
    WHERE m.ts >= (v ->> 'since')::double precision AND {}), '[]')",
        step(&format!(" OR {} OR {}", holds("waits"), holds("under")))
    ))
}

/// The read, of each thread `from` names, of its newest [`PAGE`] lines `filter` matches before the
/// line of its `sid`, in the order `from` names them.
fn page_query(filter: &str) -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg({LINE} ORDER BY m.sid DESC) \
        FROM (SELECT m.* FROM message m WHERE m.slug = f.line ->> 'slug' \
            AND m.sid < (f.line ->> 'sid')::bigint AND {filter} \
            ORDER BY m.sid DESC LIMIT {PAGE}) AS m), '[]') ORDER BY f.at) \
    FROM jsonb_array_elements(v -> 'from') WITH ORDINALITY AS f (line, at)), '[]')"
    ))
}

/// A line `m` before `since`.
const BEFORE: &str = "m.ts < (v ->> 'since')::double precision";

/// A page of each thread's lines before `since` that may say a status changed.
pub(in crate::store) fn steps_before() -> Pg<'static> {
    page_query(&format!("{BEFORE} AND {}", step("")))
}

/// A page of each thread's lines before `since` that may record what its landing waits on.
pub(in crate::store) fn waits_before() -> Pg<'static> {
    page_query(&format!(
        "{BEFORE} AND m.kind = 'system' AND {}",
        holds("waits")
    ))
}

/// A page of each thread's lines that may have taken it into review: a move (`moved_in`), or the
/// end of a block that returns it there (`unblock`, holding `back_in`).
pub(in crate::store) fn into_review() -> Pg<'static> {
    page_query(&format!(
        "m.kind = 'system' AND (right(m.text, length(v ->> 'moved_in')) = v ->> 'moved_in' \
        OR ({} AND {}))",
        starts("unblock"),
        holds("back_in")
    ))
}

/// The lines of `slug`'s thread written at `at` that may record what its landing waits on.
pub(in crate::store) fn waits_at() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg({LINE} ORDER BY m.sid) FROM message m WHERE m.slug = v ->> 'slug' \
    AND m.ts = (v ->> 'at')::double precision AND m.kind = 'system' AND {}), '[]')",
        holds("waits")
    ))
}
