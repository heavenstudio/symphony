//! The report window, its tasks and their event lines, on PostgreSQL.

use super::Pg;

pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(
        "(WITH picked AS (SELECT sid, slug, status, created, updated, parent, instruction, \
        fields #> '{_board,red}' AS red, CASE WHEN (fields #> '{_board,red}') NOT IN ('null', 'false', '0', '\"\"', '[]', '{}') THEN body END AS body \
        FROM task WHERE status NOT IN ('done', 'cancelled') OR updated >= (v ->> 'since')::double precision \
            OR slug IN (SELECT slug FROM message WHERE ts >= (v ->> 'since')::double precision)) \
        SELECT jsonb_build_object( \
            'tasks', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(to_jsonb(p)) ORDER BY sid) FROM picked p), '[]'), \
            'workflow', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb), \
            'lines', coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg(jsonb_build_object( \
                'sid', m.sid, 'kind', m.kind, 'ts', m.ts, 'sender', m.sender, \
                'text', CASE WHEN m.kind = 'system' THEN m.text WHEN m.kind = 'verdict' THEN left(m.text, 16) ELSE '' END) \
                ORDER BY m.sid) FROM message m WHERE m.slug = p.slug), '[]') ORDER BY p.sid) FROM picked p), '[]')))",
    )
}
