//! The statements of `store/messages.rs` and the thread reads on PostgreSQL.

use super::{Pg, act, authorize, listed, logged, rows};
use crate::store::messages::Poster;
use std::sync::LazyLock;

/// Whether `v ->> 'slug'` is a task, and its thread in order.
pub(in crate::store) fn thread() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('known', EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug'),
    'messages', {})",
        rows("m", "message m WHERE m.slug = v ->> 'slug'", "m.sid")
    ))
}

/// The threads of `v -> 'slugs'`, each in order, and those of them no task has.
pub(in crate::store) fn threads() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object(
    'unknown', coalesce((SELECT jsonb_agg(w.slug ORDER BY w.at)
        FROM jsonb_array_elements_text(v -> 'slugs') WITH ORDINALITY AS w (slug, at)
        WHERE NOT EXISTS (SELECT 1 FROM task WHERE slug = w.slug)), '[]'::jsonb),
    'threads', coalesce((SELECT jsonb_agg({} ORDER BY w.at)
        FROM jsonb_array_elements_text(v -> 'slugs') WITH ORDINALITY AS w (slug, at)), '[]'::jsonb))",
        rows("m", "message m WHERE m.slug = w.slug", "m.sid")
    ))
}

/// The task's ask moves with a line just put on its thread (`sid_`, `ts_`): the owner's or a
/// person's, when it asks, opens it or joins the open one; a line of a session holding a role on
/// the task, or of the session the last line of the owner's or a person's named, answers it.
pub(in crate::store) static ON_THREAD: LazyLock<String> = LazyLock::new(|| {
    format!("DECLARE
    task_ask_ bigint;
    holds_ boolean;
    named_ text;
    opened_ bigint;
BEGIN
    SELECT sid INTO task_ask_ FROM ask WHERE about = v ->> 'task_about' AND open LIMIT 1;
    IF (v ->> 'sender' = 'owner' OR v ->> 'sender' = v ->> 'by_person')
        AND coalesce((v ->> 'asking')::boolean, false) THEN
        IF task_ask_ IS NOT NULL THEN
            UPDATE ask SET lines = lines + 1, last = greatest(last, ts_) WHERE sid = task_ask_;
        ELSE
            INSERT INTO counter (id, last) VALUES ('\"ask\"', 1)
                ON CONFLICT (id) DO UPDATE SET last = counter.last + 1
                RETURNING last INTO opened_;
            INSERT INTO ask (id, sid, kind, about, task, words, asked, lines, last, before, open, closed)
                VALUES (to_jsonb(opened_), opened_, 'task', v ->> 'task_about', v ->> 'slug',
                    v ->> 'task_words', ts_, 1, ts_, '[]'::jsonb, true, false);
        END IF;
    ELSIF task_ask_ IS NOT NULL THEN
        holds_ := EXISTS (SELECT 1 FROM role WHERE slug = v ->> 'slug' AND handle = v ->> 'sender');
        IF NOT holds_ THEN
            SELECT recipient INTO named_ FROM message
                WHERE slug = v ->> 'slug' AND (sender = 'owner' OR {}) ORDER BY sid DESC LIMIT 1;
        END IF;
        IF holds_ OR named_ = v ->> 'sender' THEN
            UPDATE ask SET open = false, answered = ts_, answered_by = v ->> 'sender',
                answer = '#' || sid_ WHERE sid = task_ask_;
        END IF;
    END IF;
END;", super::person("message"))
});

/// A line made once per key, optionally adopting the earliest matching old line.
fn once(line: &str, same: Option<&str>) -> String {
    let adopt = same.map_or(String::new(), |same| format!(
        "SELECT sid INTO made_ FROM message WHERE slug = v ->> 'slug' AND {same} ORDER BY sid LIMIT 1;"));
    format!(
        "DECLARE made_ bigint; BEGIN \
        SELECT message INTO made_ FROM delivery_key WHERE id = jsonb_build_array(v -> 'slug', v -> 'key'); \
        IF made_ IS NULL THEN {adopt} \
            IF made_ IS NULL THEN {line} made_ := sid_; END IF; \
            INSERT INTO delivery_key (id, slug, key, message, ts) VALUES (jsonb_build_array(v -> 'slug', v -> 'key'), \
                v ->> 'slug', v ->> 'key', made_, coalesce(ts_, (v ->> 'now')::double precision)) \
                ON CONFLICT (id) DO UPDATE SET message = delivery_key.message RETURNING message INTO made_; \
        END IF; \
        board_answer := (SELECT {} FROM message m WHERE slug = v ->> 'slug' AND sid = made_); END;",
        super::row("m")
    )
}

pub(in crate::store) fn post(poster: Poster, keyed: Option<bool>) -> Pg<'static> {
    let act = match poster {
        Poster::Owner => String::new(),
        Poster::Board => authorize(),
        Poster::Session => act(),
    };
    let line = format!(
        "{} {}",
        logged(
            "v ->> 'slug'",
            "v ->> 'sender'",
            "'msg'",
            "v ->> 'text'",
            "v ->> 'to'"
        ),
        ON_THREAD.as_str()
    );
    let write = match keyed {
        None => format!("{line} board_answer := (SELECT {} FROM message m WHERE m.id = jsonb_build_array(v -> 'slug', sid_));", super::row("m")),
        Some(adopt) => once(&line, adopt.then_some("kind = 'msg' AND text = v ->> 'text' AND recipient IS NOT DISTINCT FROM v ->> 'to'")),
    };
    Pg::script(format!(
        "{act} \
        IF NOT EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN RAISE EXCEPTION '%', v ->> 'no_task'; END IF; \
        IF v ? 'no_role' THEN RAISE EXCEPTION '%', v ->> 'no_role'; END IF; \
        IF v ? 'handle' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN \
            RAISE EXCEPTION '%', v ->> 'nobody'; END IF; {write}"
    ))
}

pub(in crate::store) fn unread(keyed: bool) -> Pg<'static> {
    let line = logged(
        "v ->> 'slug'",
        "v ->> 'sender'",
        "'msg'",
        "v ->> 'text'",
        "v ->> 'to'",
    );
    let write = if keyed {
        once(
            &line,
            Some("sender = v ->> 'sender' AND starts_with(text, v ->> 'said')"),
        )
    } else {
        line
    };
    Pg::script(format!("{write} board_answer := 'null';"))
}

/// `v ->> 'said'`, a record line `v ->> 'sender'` puts on `v ->> 'slug'`'s thread, to no one, made
/// once per `v ->> 'key'` ([`once`]).
pub(in crate::store) fn record_once() -> Pg<'static> {
    let line = logged(
        "v ->> 'slug'",
        "v ->> 'sender'",
        "'system'",
        "v ->> 'said'",
        "NULL",
    );
    Pg::script(format!("{} board_answer := 'null';", once(&line, None)))
}

/// Whether a line was made under `v ->> 'key'` on `v ->> 'slug'`'s thread ([`once`]).
pub(in crate::store) fn keyed() -> Pg<'static> {
    Pg::read(
        "to_jsonb(EXISTS (SELECT 1 FROM delivery_key \
         WHERE id = jsonb_build_array(v -> 'slug', v -> 'key')))",
    )
}

pub(in crate::store) fn ack() -> Pg<'static> {
    Pg::script(format!(
        "{} \
        INSERT INTO delivery (id, message, handle, acked) \
        SELECT jsonb_build_array(m, v -> 'actor'), m, v ->> 'actor', (v ->> 'now')::double precision \
        FROM (SELECT DISTINCT value::bigint AS m FROM jsonb_array_elements_text(v -> 'ids')) ids \
        ON CONFLICT (id) DO NOTHING;",
        act()
    ))
}

pub(in crate::store) fn inbox() -> Pg<'static> {
    // `person`: whether the row is a person's line, read through the table's row
    // (`super::person`) or a pool's.
    let eligible = |moves: &str, person: &str| {
        format!(
            "m.ts >= (SELECT connected FROM me) \
        AND (m.kind IN (SELECT jsonb_array_elements_text(v -> 'delivered')) OR {moves}) \
        AND m.sender <> v ->> 'handle' AND (m.sender = 'owner' OR m.recipient = v ->> 'handle' \
            OR starts_with(m.recipient, '@') OR m.kind = 'verdict' OR {person})"
        )
    };
    let person = super::person("m");
    let scan = eligible(
        &format!(
            "(m.kind = 'system' AND ((m.sender = 'owner' AND starts_with(m.text, 'owner moved it ')) \
            OR ({person} AND starts_with(m.text, m.sender || ' moved it '))))"
        ),
        &person,
    );
    let pooled = eligible(
        "(m.kind = 'system' AND (m.sender = 'owner' OR coalesce(m.person, false)))",
        "coalesce(m.person, false)",
    );
    Pg::read(format!(
        "(WITH me AS (SELECT connected, kind FROM session WHERE handle = v ->> 'handle'), \
        candidates AS (SELECT m.slug, m.sid FROM message m WHERE NOT (v ? 'pool') \
            AND (NOT (v ? 'since') OR m.sid > (v ->> 'since')::bigint) AND {scan} \
        UNION ALL SELECT m.slug, m.sid FROM jsonb_to_recordset(coalesce(v -> 'pool', '[]')) \
            AS m (slug text, sid bigint, sender text, recipient text, kind text, ts double precision, person boolean) \
            WHERE v ? 'pool' AND m.sid > (v ->> 'since')::bigint AND {pooled}), \
        wanted AS (SELECT m.* FROM candidates c JOIN message m ON m.slug = c.slug AND m.sid = c.sid \
            WHERE NOT EXISTS (SELECT 1 FROM delivery d WHERE d.message = c.sid AND d.handle = v ->> 'handle')), \
        orchestrators AS (SELECT slug, handle FROM role WHERE role = 'orchestrator' AND EXISTS (SELECT 1 FROM wanted)) \
        SELECT jsonb_build_object( \
            'known', EXISTS (SELECT 1 FROM me), 'orchestrating', EXISTS (SELECT 1 FROM me WHERE kind = 'orchestrator'), \
            'orchestrators', coalesce((SELECT jsonb_agg(jsonb_build_array(slug, handle) ORDER BY slug, handle) FROM orchestrators), '[]'), \
            'orchestrator_sessions', {}, \
            'native_presence', coalesce((SELECT jsonb_agg(jsonb_build_object('handle', doc -> 'handle', \
                'until', coalesce(doc -> 'confirmed_until', '0'))) FROM callback_state \
                WHERE doc ->> 'address' >= v ->> 'codex' AND doc ->> 'address' < v ->> 'codex_after' \
                AND EXISTS (SELECT 1 FROM wanted)), '[]'), \
            'roles', coalesce((SELECT jsonb_agg(jsonb_build_object('slug', slug, 'role', role)) FROM role \
                WHERE handle = v ->> 'handle' AND EXISTS (SELECT 1 FROM wanted)), '[]'), \
            'tree', coalesce((SELECT jsonb_agg(jsonb_build_array(slug, parent)) FROM task WHERE EXISTS (SELECT 1 FROM wanted)), '[]'), \
            'messages', {}))",
        rows(
            "s",
            "session s WHERE handle IN (SELECT handle FROM orchestrators)",
            "s.handle"
        ),
        rows("m", "wanted m", "m.sid")
    ))
}

pub(in crate::store) fn to_owner() -> Pg<'static> {
    listed(
        "m",
        "(SELECT m.* FROM message m WHERE recipient = 'owner' ORDER BY sid DESC LIMIT (v ->> 'limit')::bigint) m",
        "m.sid DESC",
    )
}

pub(in crate::store) fn questions_waiting() -> Pg<'static> {
    listed(
        "m",
        &format!(
            "message m WHERE recipient = v ->> 'person' AND kind = 'msg' AND sender <> v ->> 'person' \
            AND slug IN (SELECT jsonb_array_elements_text(v -> 'open')) \
            AND NOT EXISTS (SELECT 1 FROM message a WHERE a.slug = m.slug AND a.sid > m.sid \
                AND (a.sender = v ->> 'person' OR (v ->> 'person' = 'owner' AND {})))",
            super::person("a")
        ),
        "m.sid",
    )
}

pub(in crate::store) fn newest_verdict() -> Pg<'static> {
    listed(
        "m",
        "(SELECT m.* FROM message m WHERE kind = 'verdict' AND (text IN ('approve', 'changes') \
        OR starts_with(text, 'approve:') OR starts_with(text, 'changes:')) ORDER BY sid DESC LIMIT 1) m",
        "m.sid",
    )
}

pub(in crate::store) fn messages_after() -> Pg<'static> {
    listed(
        "m",
        "message m WHERE sid > (v ->> 'since')::bigint AND sid <= (v ->> 'through')::bigint",
        "m.sid",
    )
}

pub(in crate::store) fn last_messages() -> Pg<'static> {
    listed(
        "m",
        "said s JOIN message m ON m.slug = s.slug AND m.sid = s.sid",
        "m.sid",
    )
}

/// The landings no status change follows: the lines holding the word `landed` found by `words`'
/// index of each line's terms, each by its key, then each slug's thread after its newest.
pub(in crate::store) fn landings_posted() -> Pg<'static> {
    Pg::read(
        "(WITH landed AS (SELECT m.slug, max(m.sid) AS sid FROM words w JOIN message m ON m.id = w.id -> 1 \
        WHERE w.terms ? 'landed' AND w.source = 'text' \
            AND m.kind IN (SELECT jsonb_array_elements_text(v -> 'delivered')) \
            AND strpos(m.text, 'landed ' || m.slug || ' on main') > 0 GROUP BY m.slug) \
        SELECT coalesce(jsonb_agg(l.slug ORDER BY l.slug), '[]') FROM landed l \
        WHERE NOT EXISTS (SELECT 1 FROM message m WHERE m.slug = l.slug AND m.sid > l.sid \
            AND (m.kind = 'verdict' OR (m.kind = 'system' \
                AND (strpos(m.text, ' moved it ') > 0 OR strpos(m.text, ' claimed it') > 0)))))",
    )
}
