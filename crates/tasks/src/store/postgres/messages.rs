//! The statements of `store/messages.rs` and the thread reads on PostgreSQL.

use super::{Pg, act, authorize, logged, rows};
use crate::store::messages::Poster;

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

/// The task's ask moves with a line just put on its thread (`sid_`, `ts_`): the owner's, when it
/// asks, opens it or joins the open one; a line of a session holding a role on the task, or of the
/// session the owner's last line named, answers it (`super::super::surreal::asks::ON_THREAD`).
const ON_THREAD: &str = "DECLARE
    task_ask_ bigint;
    holds_ boolean;
    named_ text;
    opened_ bigint;
BEGIN
    SELECT sid INTO task_ask_ FROM ask WHERE about = v ->> 'task_about' AND open LIMIT 1;
    IF v ->> 'sender' = 'owner' AND coalesce((v ->> 'asking')::boolean, false) THEN
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
                WHERE slug = v ->> 'slug' AND sender = 'owner' ORDER BY sid DESC LIMIT 1;
        END IF;
        IF holds_ OR named_ = v ->> 'sender' THEN
            UPDATE ask SET open = false, answered = ts_, answered_by = v ->> 'sender',
                answer = '#' || sid_ WHERE sid = task_ask_;
        END IF;
    END IF;
END;";

/// A post by `poster`, its line the answer (`super::super::surreal::messages::post`); a post made
/// once per key has no twin yet.
pub(in crate::store) fn post(poster: Poster, keyed: Option<bool>) -> Option<Pg<'static>> {
    if keyed.is_some() {
        return None;
    }
    let act = match poster {
        Poster::Owner => String::new(),
        Poster::Board => authorize(),
        Poster::Session => act(),
    };
    Some(Pg::script(format!(
        "{act}
IF NOT EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN
    RAISE EXCEPTION '%', v ->> 'no_task';
END IF;
IF v ? 'no_role' THEN
    RAISE EXCEPTION '%', v ->> 'no_role';
END IF;
IF v ? 'handle' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN
    RAISE EXCEPTION '%', v ->> 'nobody';
END IF;
{}
{ON_THREAD}
board_answer := (SELECT {} FROM message m WHERE m.id = jsonb_build_array(v -> 'slug', sid_));",
        logged(
            "v ->> 'slug'",
            "v ->> 'sender'",
            "'msg'",
            "v ->> 'text'",
            "v ->> 'to'"
        ),
        super::row("m")
    )))
}
