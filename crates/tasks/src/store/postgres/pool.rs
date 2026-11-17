//! The bounded message pool shared by one dispatch pass.
use super::Pg;

pub(in crate::store) fn pool() -> Pg<'static> {
    Pg::read(
        "jsonb_build_array(coalesce((SELECT last FROM counter WHERE id = '\"message\"'), 0), coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object('sid', sid, 'slug', slug, 'sender', sender, 'recipient', recipient, 'kind', kind, 'ts', ts))) FROM message WHERE sid > (v ->> 'floor')::bigint AND (kind IN (SELECT jsonb_array_elements_text(v -> 'delivered')) OR (kind = 'system' AND sender = 'owner' AND starts_with(text, 'owner moved it ')))), '[]'))",
    )
}
