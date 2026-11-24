//! The bounded thread metadata read through the `(slug, sid)` index.

use super::{Pg, person};
use crate::store::thread_window::{EVENT_TEXT, THREAD_WINDOW_LIMIT};

pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object(
        'known', EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug'),
        'parent', (SELECT parent FROM task WHERE slug = v ->> 'slug'),
        'status', (SELECT status FROM task WHERE slug = v ->> 'slug'),
        'messages', coalesce((SELECT jsonb_agg(jsonb_build_object(
            'sid', m.sid, 'slug', m.slug, 'sender', m.sender, 'recipient', m.recipient,
            'kind', m.kind, 'ts', m.ts, 'person', {person},
            'text', CASE WHEN m.kind = 'system' THEN left(m.text, {EVENT_TEXT})
                WHEN m.kind = 'verdict' THEN left(m.text, 16) ELSE '' END,
            'cut', m.kind = 'system' AND length(m.text) > {EVENT_TEXT}) ORDER BY m.sid DESC)
        FROM (SELECT message.* FROM message WHERE slug = v ->> 'slug'
            AND sid < (v ->> 'before')::bigint ORDER BY sid DESC LIMIT {}) m), '[]'))",
        THREAD_WINDOW_LIMIT + 1,
        person = person("m"),
    ))
}
