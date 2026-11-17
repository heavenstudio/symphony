//! The statements of `store/callbacks.rs` on PostgreSQL.

use super::Pg;

/// Every channel of a session that has not left, with its session and whether its agent's
/// binding is an in-process one (`super::super::surreal::callbacks::REGISTRATIONS`).
pub(in crate::store) fn registrations() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object(
    'handle', c.doc -> 'handle', 'address', c.doc -> 'address', 'host', c.doc -> 'host',
    'terminal', c.doc -> 'terminal',
    'registration_revision', coalesce(c.doc -> 'registration_revision', '0'::jsonb),
    'registration_nonce', c.doc -> 'registration_nonce',
    'chat', coalesce(c.doc -> 'chat', 'false'::jsonb),
    'chat_fallback', coalesce(c.doc -> 'chat_fallback', 'false'::jsonb),
    'owner', c.doc -> 'owner',
    'confirmed_until', coalesce(c.doc -> 'confirmed_until', '0'::jsonb),
    'session', {},
    'chat_resume', coalesce(c.doc -> 'chat_resume', 'false'::jsonb),
    'pane_watch', coalesce(c.doc -> 'pane_watch', '{{}}'::jsonb),
    'in_process', CASE WHEN s.agent IS NULL THEN 'false'::jsonb ELSE coalesce(
        (SELECT b.doc -> 'in_process' FROM callback_state b WHERE b.id = jsonb_build_array('binding', s.agent)),
        'true'::jsonb) END)))
    FROM callback_state c LEFT JOIN session s ON s.handle = c.doc ->> 'handle'
    WHERE c.doc ? 'address' AND s.left_at IS NULL), '[]'::jsonb)",
        super::row("s")
    ))
}
