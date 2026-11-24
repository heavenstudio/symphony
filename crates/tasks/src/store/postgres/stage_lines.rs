//! The statements of `store/stage_lines.rs` on PostgreSQL.

use super::Pg;

/// The lines of the thread of the task whose slug is `slug` (a text expression) that `kinds` keeps
/// (a condition on `m`), in order: a verdict's text only for its first words.
fn lines(slug: &str, kinds: &str) -> String {
    format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_object('sid', m.sid, 'kind', m.kind, 'ts', m.ts, \
        'text', CASE WHEN m.kind = 'system' THEN m.text ELSE left(m.text, 16) END) ORDER BY m.sid) \
    FROM message m WHERE m.slug = {slug} AND {kinds}), '[]')"
    )
}

/// The top-level tasks made since `since` and those done that were written since, who executes
/// them, and each one's thread without its posts.
pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(format!(
        "(WITH picked AS (SELECT sid, slug, status, created, updated, fields #> '{{_board,arm}}' AS arm \
        FROM task WHERE parent IS NULL AND (created >= (v ->> 'since')::double precision \
            OR (status = 'done' AND updated >= (v ->> 'since')::double precision))) \
SELECT jsonb_build_object( \
    'tasks', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(to_jsonb(p)) ORDER BY p.sid) FROM picked p), '[]'), \
    'executing', coalesce((SELECT jsonb_agg(DISTINCT r.slug) FROM role r \
        WHERE r.slug IN (SELECT slug FROM picked) AND r.role = 'executor'), '[]'), \
    'lines', coalesce((SELECT jsonb_agg({} ORDER BY p.sid) FROM picked p), '[]')))",
        lines("p.slug", "m.kind <> 'msg'")
    ))
}

/// The status and times of each task `slugs` names (none for a slug no task has) and the lines of
/// its thread that can change its status, in the order `slugs` names them.
pub(in crate::store) fn since_read() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object( \
    'tasks', coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg(jsonb_build_object('status', t.status, \
            'created', t.created, 'updated', t.updated)) FROM task t WHERE t.slug = s.slug), '[]') \
        ORDER BY s.at) FROM jsonb_array_elements_text(v -> 'slugs') WITH ORDINALITY AS s (slug, at)), '[]'), \
    'lines', coalesce((SELECT jsonb_agg({} ORDER BY s.at) \
        FROM jsonb_array_elements_text(v -> 'slugs') WITH ORDINALITY AS s (slug, at)), '[]'))",
        lines("s.slug", "m.kind IN ('system', 'verdict')")
    ))
}
