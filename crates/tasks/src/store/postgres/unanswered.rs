//! Indexed unanswered-post reads and the single recovery reservation on PostgreSQL.

use super::{Pg, logged};

const ANSWER: &str = "(m.kind IN ('msg', 'verdict') OR (m.kind = 'system' AND \
    (starts_with(m.text, m.sender || ' moved it ') OR starts_with(m.text, m.sender || ' claimed it') \
    OR (starts_with(m.text, m.sender || ' released ') AND strpos(m.text, ' (released)') > 0) \
    OR m.text = m.sender || ' joined as reviewer' OR m.text = m.sender || ' joined as executor')))";

pub(in crate::store) fn naming() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg(slug ORDER BY slug) FROM \
        (SELECT slug FROM message WHERE recipient = h.handle UNION \
         SELECT slug FROM message WHERE sender = h.handle AND kind = 'system') named), '[]') ORDER BY h.at) \
        FROM jsonb_array_elements_text(v -> 'handles') WITH ORDINALITY h (handle, at)), '[]')",
    )
}

pub(in crate::store) fn reviewer_launches() -> Pg<'static> {
    Pg::read(super::rows(
        "l",
        "task_session l WHERE handle IN (SELECT jsonb_array_elements_text(v -> 'handles')) AND role = 'reviewer'",
        "l.started, l.slug, l.handle",
    ))
}

pub(in crate::store) fn threads() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('threads', \
        coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg(jsonb_strip_nulls(jsonb_build_object( \
            'sid', m.sid, 'slug', m.slug, 'sender', m.sender, 'recipient', m.recipient, 'kind', m.kind, \
            'ts', m.ts, 'text', CASE WHEN m.kind = 'system' THEN m.text ELSE '' END)) ORDER BY m.sid) \
            FROM message m WHERE m.slug = s.slug AND ({ANSWER} OR \
                (m.kind = 'system' AND starts_with(m.text, m.sender || ' released ')))), '[]') ORDER BY s.at) \
            FROM jsonb_array_elements_text(v -> 'slugs') WITH ORDINALITY s (slug, at)), '[]'), \
        'launches', {})",
        super::rows(
            "l",
            "jsonb_array_elements_text(v -> 'launched') WITH ORDINALITY w (slug, at) JOIN task_session l ON l.slug = w.slug",
            "w.at, l.handle"
        )
    ))
}

pub(in crate::store) fn below() -> Pg<'static> {
    Pg::read(format!(
        "coalesce((SELECT jsonb_agg(coalesce((SELECT jsonb_agg(m.sid ORDER BY b.at, m.sid) \
        FROM jsonb_array_elements_text(a.ask -> 'below') WITH ORDINALITY b (slug, at) \
        JOIN message m ON m.slug = b.slug WHERE m.sender = a.ask ->> 'handle' AND m.sid > (a.ask ->> 'after')::bigint AND {ANSWER}), '[]') ORDER BY a.at) \
        FROM jsonb_array_elements(v -> 'asks') WITH ORDINALITY a (ask, at)), '[]')"
    ))
}

fn answers() -> String {
    format!(
        "coalesce((SELECT jsonb_agg(jsonb_build_array(m.sid, m.slug) ORDER BY m.sid, m.slug) FROM message m \
        WHERE m.sender = v ->> 'handle' AND m.sid > (v ->> 'post')::bigint AND {ANSWER}), '[]')"
    )
}

pub(in crate::store) fn answer_rows() -> Pg<'static> {
    Pg::read(answers())
}

pub(in crate::store) fn parents() -> Pg<'static> {
    Pg::read(
        "coalesce((SELECT jsonb_agg(jsonb_build_array(s.slug, (SELECT parent FROM task WHERE slug = s.slug)) ORDER BY s.at) \
        FROM jsonb_array_elements_text(v -> 'slugs') WITH ORDINALITY s (slug, at)), '[]')",
    )
}

/// Fence the recipient and lock the observed task edges: PostgreSQL repeatable-read alone
/// would let a concurrent reply or parent move commit after the check without a write conflict.
pub(in crate::store) fn reserve() -> Pg<'static> {
    Pg::script(format!(
        "DECLARE key_ jsonb := jsonb_build_array(v -> 'slug', v -> 'handle', v -> 'post'); status_ text; BEGIN \
        board_answer := 'false'; \
        {} \
        PERFORM 1 FROM task WHERE slug = v ->> 'slug' OR slug IN \
            (SELECT p ->> 0 FROM jsonb_array_elements(v -> 'parents') p) ORDER BY slug FOR SHARE; \
        IF NOT EXISTS (SELECT 1 FROM unanswered_restore WHERE id = key_) \
            AND (SELECT last_seen FROM session WHERE handle = v ->> 'handle') = (v ->> 'seen')::double precision \
            AND {} = v -> 'answers' \
            AND NOT EXISTS (SELECT 1 FROM jsonb_array_elements(v -> 'parents') p \
                WHERE (SELECT parent FROM task WHERE slug = p ->> 0) IS DISTINCT FROM p ->> 1) THEN \
            SELECT status INTO status_ FROM task WHERE slug = v ->> 'slug'; \
            IF NOT ((v ->> 'role' = 'executor' AND status_ NOT IN ('in_progress', 'blocked', 'review')) \
                OR (v ->> 'role' = 'reviewer' AND status_ <> 'review')) THEN \
                INSERT INTO unanswered_restore (id, doc) VALUES (key_, jsonb_build_object( \
                    'slug', v -> 'slug', 'handle', v -> 'handle', 'post', v -> 'post')) ON CONFLICT (id) DO NOTHING; \
                board_answer := to_jsonb(FOUND); \
            END IF; \
        END IF; END;",
        super::fence("v ->> 'handle'"),
        answers()
    ))
}

pub(in crate::store) fn restore_said() -> Pg<'static> {
    Pg::script(logged(
        "v ->> 'slug'",
        "'board'",
        "'msg'",
        "v ->> 'text'",
        "'@orchestrator'",
    ))
}
