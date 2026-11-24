//! The statements of `store/held.rs` on PostgreSQL: a message held for people with the board's line
//! that says so, a held message passed on as its author's line, and the reads of a task's held
//! messages, which find none on a board without the step ([`crate::schema::HELD`]).

use super::messages::ON_THREAD;
use super::{Pg, act, logged};
use crate::store::messages::Poster;

/// The table the step defines, which a write of a held message names.
pub(in crate::store) const TABLES: [&str; 1] = ["held_message"];

/// The checks a line on `v ->> 'slug'`'s thread to `v ->> 'to'` passes first, as a post's: the
/// task is there, a role named is one, and a session named is on the board.
const ADDRESSED: &str = "IF NOT EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN \
        RAISE EXCEPTION '%', v ->> 'no_task'; END IF; \
    IF v ? 'no_role' THEN RAISE EXCEPTION '%', v ->> 'no_role'; END IF; \
    IF v ? 'handle' AND NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN \
        RAISE EXCEPTION '%', v ->> 'nobody'; END IF;";

/// `v ->> 'author'`'s message `v ->> 'text'` to `v ->> 'to'`, held: the board's line to `owner`
/// saying so (`v ->> 'marker'` and its own number) and the held row keyed by that number, its
/// answer.
pub(in crate::store) fn hold() -> Pg<'static> {
    let marker = logged(
        "v ->> 'slug'",
        "v ->> 'sender'",
        "'msg'",
        "(v ->> 'marker') || '#' || sid_",
        "'owner'",
    );
    Pg::script(format!(
        "{ADDRESSED}
{marker}
INSERT INTO held_message (id, doc) VALUES (to_jsonb(sid_), jsonb_strip_nulls(jsonb_build_object(
    'id', sid_, 'slug', v ->> 'slug', 'author', v ->> 'author', 'text', v ->> 'text',
    'to', v -> 'to', 'at', ts_)));
board_answer := (SELECT doc FROM held_message WHERE id = to_jsonb(sid_));"
    ))
}

/// The held message `v ->> 'id'` on `v ->> 'slug'`, passed on by `v ->> 'by'` (authorized as a
/// session's write for a session, `poster`): refused where none is held there or it was passed
/// already, naming who passed it; else its line, `v ->> 'text'` under `v ->> 'sender'` (its
/// author, marked a person's) to `v ->> 'to'`, moving the task's ask as a person's line does, and
/// the row marked passed with that line's number, its answer.
pub(in crate::store) fn pass(poster: Poster) -> Pg<'static> {
    let act = match poster {
        Poster::Session => act(),
        Poster::Owner | Poster::Board => String::new(),
    };
    let line = logged(
        "v ->> 'slug'",
        "v ->> 'sender'",
        "'msg'",
        "v ->> 'text'",
        "v ->> 'to'",
    );
    let on_thread = ON_THREAD.as_str();
    Pg::script(format!(
        "{act}
DECLARE held_ jsonb; BEGIN
SELECT doc INTO held_ FROM held_message
    WHERE id = to_jsonb((v ->> 'id')::bigint) AND doc ->> 'slug' = v ->> 'slug' FOR UPDATE;
IF held_ IS NULL THEN RAISE EXCEPTION '%', v ->> 'none_held'; END IF;
IF held_ ? 'passed' THEN
    RAISE EXCEPTION '%', format(v ->> 'passed_already', held_ #>> '{{passed,by}}');
END IF;
{ADDRESSED}
{line} {on_thread}
UPDATE held_message SET doc = doc || jsonb_build_object('passed',
        jsonb_build_object('by', v ->> 'by', 'at', ts_, 'line', sid_))
    WHERE id = to_jsonb((v ->> 'id')::bigint);
board_answer := (SELECT doc FROM held_message WHERE id = to_jsonb((v ->> 'id')::bigint));
END;"
    ))
}

/// The held messages on `v ->> 'slug'`, oldest first: none on a board without the step.
pub(in crate::store) fn on_task() -> Pg<'static> {
    Pg::script(
        "board_answer := '[]'::jsonb;
IF to_regclass('held_message') IS NOT NULL THEN
    board_answer := coalesce((SELECT jsonb_agg(doc ORDER BY (doc ->> 'id')::bigint)
        FROM held_message WHERE doc ->> 'slug' = v ->> 'slug'), '[]'::jsonb);
END IF;",
    )
}

/// How many messages on `v ->> 'slug'` are held and not passed on: none on a board without the
/// step.
pub(in crate::store) fn count() -> Pg<'static> {
    Pg::script(
        "board_answer := '0'::jsonb;
IF to_regclass('held_message') IS NOT NULL THEN
    board_answer := to_jsonb((SELECT count(*) FROM held_message
        WHERE doc ->> 'slug' = v ->> 'slug' AND NOT doc ? 'passed'));
END IF;",
    )
}
