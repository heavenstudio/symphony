//! Session web addresses, kept outside the session row and board marker.
use super::{Pg, act};

/// Every address of a session that has not left, by handle: the `session_url` rows by their ids'
/// range on the primary key (`super::super::surreal::session_urls::READ`), each id the name and a
/// handle, a string, which sorts above `null` and below `true`.
pub(in crate::store) fn read() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_array(c.id -> 1, c.doc -> 'url') ORDER BY c.id) FROM callback_state c LEFT JOIN session s ON s.handle = c.id ->> 1 WHERE c.id > '[\"session_url\", null]' AND c.id < '[\"session_url\", true]' AND c.doc ? 'url' AND s.left_at IS NULL), '[]')",
    )
}

pub(in crate::store) fn set() -> Pg<'static> {
    Pg::script(format!("{}
IF NOT v ? 'url' THEN
    DELETE FROM callback_state WHERE id = jsonb_build_array('session_url', v -> 'actor');
ELSE
    INSERT INTO callback_state (id, doc) VALUES (jsonb_build_array('session_url', v -> 'actor'), jsonb_build_object('url', v -> 'url'))
    ON CONFLICT (id) DO UPDATE SET doc = callback_state.doc || EXCLUDED.doc;
END IF;", act()))
}
