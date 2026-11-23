//! The statements of `store/asks.rs` on PostgreSQL: the owner's asks.

use super::{Pg, row};

const JOIN_OR_OPEN: &str = "DECLARE ask_ bigint; BEGIN \
SELECT sid INTO ask_ FROM ask WHERE about = v ->> 'about' AND open LIMIT 1; \
IF ask_ IS NOT NULL THEN \
    UPDATE ask SET lines = lines + 1, last = greatest(last, (v ->> 'at')::double precision) WHERE sid = ask_; \
ELSE \
    INSERT INTO counter (id, last) VALUES ('\"ask\"', 1) \
        ON CONFLICT (id) DO UPDATE SET last = counter.last + 1 RETURNING last INTO ask_; \
    INSERT INTO ask (id, sid, kind, about, path, card, block, tree, task, thread, words, asked, lines, last, before, open, closed) \
    VALUES (to_jsonb(ask_), ask_, v ->> 'kind', v ->> 'about', v ->> 'path', v ->> 'card', \
        v ->> 'block', v ->> 'tree', v ->> 'task', v ->> 'thread', v ->> 'words', \
        (v ->> 'at')::double precision, 1, (v ->> 'at')::double precision, '[]', true, false); \
END IF; \
board_answer := (SELECT jsonb_strip_nulls(to_jsonb(a)) - 'id' FROM ask a WHERE sid = ask_); END;";

pub(in crate::store) fn open() -> Pg<'static> {
    Pg::script(format!(
        "IF v ? 'task' AND NOT EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'task') THEN \
        RAISE EXCEPTION '%', v ->> 'no_task'; END IF; {JOIN_OR_OPEN}"
    ))
}

pub(in crate::store) fn owner_save() -> Pg<'static> {
    Pg::script(format!(
        "INSERT INTO chat_event (id, doc) VALUES (v -> 'key', jsonb_build_object( \
            'key', v -> 'key', 'text', v -> 'text', 'delivered', v ->> 'text' = '')) ON CONFLICT (id) DO NOTHING; \
        IF FOUND AND (v ->> 'asking')::boolean THEN {JOIN_OR_OPEN} END IF; \
        board_answer := 'null';"
    ))
}

/// Every mutation first reads the ask, then returns its final row.
fn change(body: &str) -> String {
    format!(
        "DECLARE a_ ask%ROWTYPE; BEGIN \
        SELECT a.* INTO a_ FROM ask a WHERE sid = (v ->> 'sid')::bigint; \
        IF NOT FOUND THEN RAISE EXCEPTION '%', v ->> 'no_ask'; END IF; \
        {body} \
        board_answer := (SELECT {} FROM ask a WHERE sid = a_.sid); END;",
        row("a")
    )
}

pub(in crate::store) fn hold() -> Pg<'static> {
    Pg::script(change(
        "IF NOT a_.open THEN RAISE EXCEPTION '%', v ->> 'answered'; END IF; \
    IF v ? 'holder' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'holder') THEN \
        RAISE EXCEPTION '%', v ->> 'unknown'; END IF; \
    IF a_.holder IS DISTINCT FROM v ->> 'holder' THEN \
        UPDATE ask SET before = before || CASE WHEN a_.holder IS NULL THEN '[]'::jsonb ELSE \
            jsonb_build_array(jsonb_build_object('handle', a_.holder, 'since', coalesce(a_.held, (v ->> 'now')::double precision), \
            'until', (v ->> 'now')::double precision, 'why', v -> 'why')) END, \
            holder = v ->> 'holder', held = CASE WHEN v ? 'holder' THEN (v ->> 'now')::double precision END \
        WHERE sid = a_.sid; END IF;",
    ))
}

pub(in crate::store) fn answer() -> Pg<'static> {
    Pg::script(change(
        "IF a_.open THEN UPDATE ask SET open = false, answered = (v ->> 'now')::double precision, \
        answered_by = v ->> 'by', answer = v ->> 'answer' WHERE sid = a_.sid; END IF;",
    ))
}

pub(in crate::store) fn answer_about() -> Pg<'static> {
    Pg::script(format!(
        "DECLARE ask_ bigint; BEGIN \
        SELECT sid INTO ask_ FROM ask WHERE about = v ->> 'about' AND open LIMIT 1; \
        IF ask_ IS NOT NULL THEN UPDATE ask SET open = false, answered = (v ->> 'now')::double precision, \
            answered_by = v ->> 'by', answer = v ->> 'answer' WHERE sid = ask_; END IF; \
        board_answer := (SELECT {} FROM ask a WHERE sid = ask_); END;",
        row("a")
    ))
}

pub(in crate::store) fn close() -> Pg<'static> {
    let body = change(
        "IF NOT a_.open THEN RAISE EXCEPTION '%', v ->> 'answered'; END IF; \
        UPDATE ask SET open = false, answered = (v ->> 'now')::double precision, answered_by = v ->> 'by', \
            answer = v ->> 'why', closed = true WHERE sid = a_.sid;",
    );
    Pg::script(format!(
        "IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'by') THEN \
        RAISE EXCEPTION '%', v ->> 'unknown'; END IF; {body}"
    ))
}

pub(in crate::store) fn gone() -> Pg<'static> {
    Pg::script(change(
        "IF a_.open AND a_.gone IS DISTINCT FROM v ->> 'why' THEN \
        UPDATE ask SET gone = v ->> 'why', gone_since = CASE WHEN v ? 'why' \
            THEN coalesce(a_.gone_since, (v ->> 'now')::double precision) END WHERE sid = a_.sid; END IF;",
    ))
}

pub(in crate::store) fn asks() -> Pg<'static> {
    Pg::read(format!(
        "jsonb_build_object('open', {}, 'answered', {})",
        super::rows("a", "ask a WHERE open", "a.asked, a.sid"),
        super::rows(
            "a",
            "ask a WHERE answered >= (v ->> 'since')::double precision",
            "a.answered DESC, a.sid DESC"
        )
    ))
}

pub(in crate::store) fn ask() -> Pg<'static> {
    Pg::read(format!(
        "(SELECT {} FROM ask a WHERE sid = (v ->> 'sid')::bigint)",
        row("a")
    ))
}
