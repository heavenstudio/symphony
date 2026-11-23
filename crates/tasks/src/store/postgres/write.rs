//! A rule's write on PostgreSQL: the actor, the gone witnesses and the incoming role holders fenced
//! before their task changes.

use super::logged;

/// `handles` (rows of one text) fenced in their order, each session's revision written once, so a
/// write that decided on them meets every other that did ([`super::fence`]).
pub(in crate::store) fn fenced(handles: &str) -> String {
    format!(
        "INSERT INTO session_revision (id, last, writes) \
    SELECT to_jsonb(w.handle), 0, 1 FROM ({handles}) AS w (handle) ORDER BY w.handle \
    ON CONFLICT (id) DO UPDATE SET writes = coalesce(session_revision.writes, 0) + 1;"
    )
}

/// Stale unless every session `v -> 'seen'` names is at the `last_seen` it was read at.
pub(in crate::store) const SEEN: &str = "IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'seen') AS g \
        WHERE (SELECT last_seen FROM session WHERE handle = g ->> 'handle') \
            IS DISTINCT FROM (g ->> 'last_seen')::double precision) THEN \
    RAISE EXCEPTION 'stale'; \
END IF;";

/// The rule's write: stale unless the actor is there and has not left, the task is at the version
/// read and every session it took to be gone is as it was read; then the actor, the witnesses
/// and the incoming holders fenced, the actor seen, the roles dropped and added, the task moved
/// on, and its line (`say`) on the thread.
pub(in crate::store) fn write() -> String {
    format!(
        "IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'actor' AND left_at IS NULL) THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
IF (SELECT version FROM task WHERE slug = v ->> 'slug') IS DISTINCT FROM (v ->> 'version')::bigint THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
{SEEN} \
{} \
UPDATE session SET last_seen = greatest(last_seen, (v ->> 'now')::double precision) \
    WHERE handle = v ->> 'actor'; \
DELETE FROM role WHERE id IN (SELECT jsonb_build_array(v ->> 'slug', d ->> 'handle', d ->> 'role') \
    FROM jsonb_array_elements(v -> 'drop') AS d); \
IF EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'add') AS a \
        JOIN session s ON s.handle = a ->> 'handle' WHERE s.left_at IS NOT NULL) THEN \
    RAISE EXCEPTION 'stale'; \
END IF; \
INSERT INTO role (id, slug, handle, role, since, expires) \
    SELECT jsonb_build_array(v ->> 'slug', a ->> 'handle', a ->> 'role'), a ->> 'slug', \
        a ->> 'handle', a ->> 'role', (a ->> 'since')::double precision, \
        (a ->> 'expires')::double precision \
    FROM jsonb_array_elements(v -> 'add') AS a \
    ON CONFLICT (id) DO UPDATE SET (slug, handle, role, since, expires) \
        = (EXCLUDED.slug, EXCLUDED.handle, EXCLUDED.role, EXCLUDED.since, EXCLUDED.expires); \
UPDATE task SET version = version + 1, status = coalesce(v ->> 'status', status), \
    updated = coalesce((v ->> 'updated')::double precision, updated) WHERE slug = v ->> 'slug'; \
IF v ? 'say' THEN \
    {} \
END IF;",
        fenced(
            "SELECT v ->> 'actor' UNION SELECT g ->> 'handle' FROM jsonb_array_elements(v -> 'seen') AS g \
        UNION SELECT a ->> 'handle' FROM jsonb_array_elements(v -> 'add') AS a"
        ),
        logged(
            "v ->> 'slug'",
            "v ->> 'actor'",
            "v -> 'say' ->> 'kind'",
            "v -> 'say' ->> 'text'",
            "v -> 'say' ->> 'recipient'"
        )
    )
}
