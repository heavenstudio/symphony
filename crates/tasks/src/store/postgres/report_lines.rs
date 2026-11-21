//! The report window, its tasks and their event lines, on PostgreSQL.

use super::Pg;

pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(
        "(WITH picked AS (SELECT sid, slug, status, created, updated, parent, instruction, \
        fields #> '{_board,red}' AS red, CASE WHEN (fields #> '{_board,red}') NOT IN ('null', 'false', '0', '\"\"', '[]', '{}') THEN body END AS body \
        FROM task WHERE status NOT IN ('done', 'cancelled') OR updated >= (v ->> 'since')::double precision \
            OR slug IN (SELECT slug FROM message WHERE ts >= (v ->> 'since')::double precision)) \
        , landed AS (SELECT slug FROM picked WHERE status = 'done' AND parent IS NULL) \
        SELECT jsonb_build_object( \
            'usage', coalesce((SELECT jsonb_agg(to_jsonb(u)) FROM (SELECT role, model, speed, \
                sum(input) AS input, sum(cache_write_5m) AS cache_write_5m, sum(cache_write_1h) AS cache_write_1h, \
                sum(cache_read) AS cache_read, sum(output) AS output, sum(calls) AS calls \
                FROM usage GROUP BY role, model, speed) u), '[]'), \
            'spent', coalesce((SELECT jsonb_agg(jsonb_build_object( \
                'slug', CASE WHEN t.parent IN (SELECT slug FROM landed) THEN t.parent ELSE u.slug END, \
                'role', u.role, 'model', u.model, 'speed', u.speed, 'round', u.round, 'input', u.input, \
                'cache_write_5m', u.cache_write_5m, 'cache_write_1h', u.cache_write_1h, \
                'cache_read', u.cache_read, 'output', u.output, 'calls', u.calls)) \
                FROM usage u JOIN task t ON t.slug = u.slug \
                WHERE t.slug IN (SELECT slug FROM landed) OR t.parent IN (SELECT slug FROM landed)), '[]'), \
            'tasks', coalesce((SELECT jsonb_agg(jsonb_strip_nulls(to_jsonb(p)) ORDER BY sid) FROM picked p), '[]'), \
            'workflow', (SELECT body FROM workflow WHERE id = '[\"active\"]'::jsonb), \
            'lines', coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg(jsonb_build_object( \
                'sid', m.sid, 'kind', m.kind, 'ts', m.ts, 'sender', m.sender, \
                'text', CASE WHEN m.kind = 'system' THEN m.text WHEN m.kind = 'verdict' THEN left(m.text, 16) ELSE '' END) \
                ORDER BY m.sid) FROM message m WHERE m.slug = p.slug), '[]') ORDER BY p.sid) FROM picked p), '[]')))",
    )
}
