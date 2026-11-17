//! The statements of `store/sessions.rs` on PostgreSQL.

use super::Pg;

/// Every session's handle.
pub(in crate::store) const HANDLES: Pg<'static> = Pg::Read(std::borrow::Cow::Borrowed(
    "coalesce((SELECT jsonb_agg(handle) FROM session), '[]'::jsonb)",
));

/// A session, its parent there when it names one, and a handle nobody took meanwhile.
const REGISTER: &str = "IF v ? 'parent' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'parent') THEN
    RAISE EXCEPTION '%', v ->> 'orphan';
END IF;
IF EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN
    RAISE EXCEPTION 'stale';
END IF;
INSERT INTO session
    SELECT r.* FROM jsonb_populate_record(NULL::session, (v -> 'row') || jsonb_build_object('id', v -> 'handle')) AS r
    ON CONFLICT DO NOTHING;
IF NOT FOUND THEN
    RAISE EXCEPTION 'stale';
END IF;";

/// The tasks `v ->> 'handle'` holds that another could take over (`super::super::surreal::CLAIMABLE`).
/// Needs `executing`.
pub(in crate::store) const CLAIMABLE: &str =
    "coalesce((SELECT jsonb_agg(r.slug) FROM role r JOIN task t ON t.slug = r.slug
    WHERE r.handle = v ->> 'handle'
    AND ((r.role = 'executor' AND t.status IN (SELECT jsonb_array_elements_text(v -> 'executing')))
        OR (r.role = 'reviewer' AND t.status = 'review'))), '[]'::jsonb)";

/// One session, `v ->> 'handle'`, and whether it holds work another could take over (a `Found`).
/// Needs `executing`.
pub(in crate::store) fn session_read() -> String {
    format!(
        "jsonb_build_object(
    'session', (SELECT {} FROM session s WHERE s.handle = v ->> 'handle'),
    'held', {CLAIMABLE},
    'native_until', (SELECT CASE WHEN starts_with(coalesce(c.doc ->> 'address', ''), 'codex:')
        THEN coalesce(c.doc -> 'confirmed_until', '0'::jsonb) END
        FROM callback_state c WHERE c.id = v -> 'handle'))",
        super::row("s")
    )
}

/// [`REGISTER`], then the session as a read of one gives it ([`session_read`]).
pub(in crate::store) fn register() -> Pg<'static> {
    Pg::script_then(REGISTER, session_read())
}
