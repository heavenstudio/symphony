//! The bounded message pool shared by one dispatch pass.
use super::Pg;

/// The newest message's number, and the delivered messages and the moves of the owner's and a
/// person's after `v ->> 'floor'`, only the fields the inbox's reach test reads
/// (`super::super::surreal::pool::pool`): a person's mark read through the row
/// ([`super::person`]).
pub(in crate::store) fn pool() -> Pg<'static> {
    let person = super::person("m");
    Pg::read(format!(
        "jsonb_build_array(coalesce((SELECT last FROM counter WHERE id = '\"message\"'), 0), coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object('sid', sid, 'slug', slug, 'sender', sender, 'recipient', recipient, 'kind', kind, 'ts', ts, 'person', to_jsonb(m) -> 'person'))) FROM message m WHERE sid > (v ->> 'floor')::bigint AND (kind IN (SELECT jsonb_array_elements_text(v -> 'delivered')) OR (kind = 'system' AND ((sender = 'owner' AND starts_with(text, 'owner moved it ')) OR ({person} AND starts_with(text, sender || ' moved it ')))))), '[]'))",
    ))
}
