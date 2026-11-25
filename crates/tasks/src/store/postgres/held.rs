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

/// The key of the held comment on a document numbered `n` (a SQL expression): `["document", N]`,
/// never a task's held message's, whose key is a number.
fn document_key(n: &str) -> String {
    format!("jsonb_build_array('document', ({n})::bigint)")
}

/// `v ->> 'author'`'s comment `v ->> 'text'` on the document place `v -> 'document'`, held: its
/// row, numbered by the counter `held_document`, its answer.
pub(in crate::store) fn hold_on_document() -> Pg<'static> {
    let key = document_key("sid_");
    Pg::script(format!(
        "INSERT INTO counter (id, last, ts) VALUES ('\"held_document\"', 1, (v ->> 'now')::double precision)
    ON CONFLICT (id) DO UPDATE SET last = counter.last + 1 RETURNING last INTO sid_;
INSERT INTO held_message (id, doc) VALUES ({key}, jsonb_build_object(
    'id', sid_, 'document', v -> 'document', 'author', v ->> 'author', 'text', v ->> 'text',
    'at', (v ->> 'now')::double precision));
board_answer := (SELECT doc FROM held_message WHERE id = {key});"
    ))
}

/// The held comment `v ->> 'id'`, passed on by `v ->> 'by'`: refused where none is held by it or it
/// was passed already, naming who passed it; else the row marked passed, its answer, and kept
/// under the checkout `v -> 'into'` names (`{"scope": SCOPE}`, `{}` the project's own) where `v`
/// has one.
pub(in crate::store) fn pass_on_document() -> Pg<'static> {
    let key = document_key("v ->> 'id'");
    Pg::script(format!(
        "DECLARE held_ jsonb; BEGIN
SELECT doc INTO held_ FROM held_message WHERE id = {key} FOR UPDATE;
IF held_ IS NULL THEN RAISE EXCEPTION '%', v ->> 'none_held'; END IF;
IF held_ ? 'passed' THEN
    RAISE EXCEPTION '%', format(v ->> 'passed_already', held_ #>> '{{passed,by}}');
END IF;
IF v ? 'into' THEN
    held_ := jsonb_set(held_, '{{document}}', (held_ -> 'document') - 'scope' || (v -> 'into'));
END IF;
UPDATE held_message SET doc = held_ || jsonb_build_object('passed',
        jsonb_build_object('by', v ->> 'by', 'at', (v ->> 'now')::double precision))
    WHERE id = {key};
board_answer := (SELECT doc FROM held_message WHERE id = {key});
END;"
    ))
}

/// The comments held on documents that `filter` (a condition on `doc`) keeps, oldest first: none on
/// a board without the step.
fn on_documents(filter: &str) -> Pg<'static> {
    Pg::script(format!(
        "board_answer := '[]'::jsonb;
IF to_regclass('held_message') IS NOT NULL THEN
    board_answer := coalesce((SELECT jsonb_agg(doc ORDER BY (doc ->> 'id')::bigint)
        FROM held_message WHERE doc ? 'document' AND ({filter})), '[]'::jsonb);
END IF;"
    ))
}

/// The comments held on the document `v ->> 'path'` of the checkout `v ->> 'scope'` (null: the
/// project's own), passed on or not.
pub(in crate::store) fn on_document() -> Pg<'static> {
    on_documents(
        "doc #>> '{document,path}' = v ->> 'path' \
         AND (doc #>> '{document,scope}') IS NOT DISTINCT FROM (v ->> 'scope')",
    )
}

/// The held comment `v ->> 'id'`, in a list of one or none.
pub(in crate::store) fn comment() -> Pg<'static> {
    on_documents("(doc ->> 'id')::bigint = (v ->> 'id')::bigint")
}

/// The comments held on documents and not passed on.
pub(in crate::store) fn unpassed() -> Pg<'static> {
    on_documents("NOT doc ? 'passed'")
}
