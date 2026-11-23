//! Launch records, closure and restoration, and the session lease that fences their writes.

use super::{Pg, listed};

pub(in crate::store) fn sessions_of_task() -> Pg<'static> {
    listed(
        "l",
        "task_session l WHERE slug = v ->> 'slug'",
        "l.started, l.handle",
    )
}

const KEY: &str = "jsonb_build_array(v -> 'slug', v -> 'handle')";
const KNOWN: &str = "IF NOT EXISTS (SELECT 1 FROM task WHERE slug = v ->> 'slug') THEN RAISE EXCEPTION '%', v ->> 'no_task'; END IF;
IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'handle') THEN RAISE EXCEPTION '%', v ->> 'no_session'; END IF;";
const SETTLED: &str = "thread_ := (SELECT doc ->> 'address' FROM callback_state WHERE id = v -> 'handle');
IF conversation_ = 'codex:' AND starts_with(thread_, 'codex:') AND thread_ <> 'codex:' THEN conversation_ := thread_; END IF;";
const CLOSURE: &str =
    "line_ := (v ->> 'said') || CASE WHEN conversation_ = 'codex:' THEN v ->> 'lost' ELSE '' END;";

/// A holder writes the lease row as a takeover does, so an overlapping takeover conflicts.
pub(in crate::store) const HOLDS: &str = "IF (SELECT doc -> 'revision' FROM callback_lease WHERE id = jsonb_build_array('session', v -> 'handle')) IS DISTINCT FROM v -> 'hold' THEN RAISE EXCEPTION '%', v ->> 'taken'; END IF;
UPDATE callback_lease SET doc = doc || jsonb_build_object('writes', coalesce((doc ->> 'writes')::bigint, 0) + 1) WHERE id = jsonb_build_array('session', v -> 'handle');";

fn put() -> String {
    format!("INSERT INTO task_session SELECT r.* FROM jsonb_populate_record(NULL::task_session, (v -> 'row') || jsonb_build_object('id', {KEY}, 'claude_session', conversation_)) r
ON CONFLICT (id) DO UPDATE SET slug = EXCLUDED.slug, handle = EXCLUDED.handle, role = EXCLUDED.role, claude_session = EXCLUDED.claude_session, worktree = EXCLUDED.worktree, branch = EXCLUDED.branch, pane = EXCLUDED.pane, model = EXCLUDED.model, started = EXCLUDED.started, closed = EXCLUDED.closed, closed_why = EXCLUDED.closed_why, restore = EXCLUDED.restore;")
}

pub(in crate::store) fn record_launch(log: bool) -> Pg<'static> {
    let line = if log {
        super::logged(
            "v ->> 'slug'",
            "v ->> 'handle'",
            "'system'",
            "(v ->> 'said_before') || CASE WHEN conversation_ = '' THEN 'unknown' ELSE conversation_ END || (v ->> 'said_after')",
            "NULL",
        )
    } else {
        String::new()
    };
    Pg::script(format!(
        "DECLARE conversation_ text := v ->> 'given'; thread_ text;
BEGIN
{KNOWN}
{SETTLED}
{}
{line}
board_answer := to_jsonb(conversation_);
END;",
        put()
    ))
}

pub(in crate::store) fn sessions_of_handle() -> Pg<'static> {
    listed(
        "l",
        "task_session l WHERE handle = v ->> 'handle'",
        "l.started, l.slug",
    )
}
pub(in crate::store) fn all_sessions() -> Pg<'static> {
    listed("l", "task_session l", "l.started, l.slug, l.handle")
}
pub(in crate::store) fn open_sessions() -> Pg<'static> {
    listed(
        "l",
        "task_session l WHERE closed IS NULL",
        "l.started, l.slug, l.handle",
    )
}

pub(in crate::store) fn codex_launches() -> Pg<'static> {
    super::list_of(
        "jsonb_build_object('claude_session', l.claude_session, 'pane', l.pane, 'open', l.closed IS NULL)".into(),
        "task_session l WHERE starts_with(l.claude_session, v ->> 'prefix') AND l.claude_session <> v ->> 'prefix'",
        "l.slug, l.handle",
    )
}

fn recorded(close: bool, held: bool) -> Pg<'static> {
    let guard = if held { HOLDS } else { "" };
    let same = if held {
        format!(
            "IF at_.pane IS DISTINCT FROM v ->> 'pane' OR at_.started IS DISTINCT FROM (v ->> 'started')::double precision OR at_.closed IS {}NULL THEN board_answer := 'false'; EXIT change_; END IF;",
            if close { "NOT " } else { "" }
        )
    } else {
        String::new()
    };
    let change = if close {
        format!("conversation_ := at_.claude_session; {SETTLED} {CLOSURE}
UPDATE task_session SET closed = (v ->> 'now')::double precision, closed_why = v ->> 'why', restore = v ->> 'restore', claude_session = conversation_ WHERE id = {KEY};")
    } else {
        let revive = if held {
            "UPDATE session SET left_at = NULL, last_seen = greatest(last_seen, (v ->> 'now')::double precision) WHERE handle = v ->> 'handle';"
        } else {
            ""
        };
        format!(
            "{revive} line_ := v ->> 'said'; UPDATE task_session SET pane = v ->> '{}', closed = NULL, closed_why = '' WHERE id = {KEY};",
            if held { "into" } else { "pane" }
        )
    };
    Pg::script(format!(
        "<<change_>>
DECLARE at_ task_session; conversation_ text; thread_ text; line_ text;
BEGIN
SELECT l.* INTO at_ FROM task_session l WHERE id = {KEY};
IF NOT FOUND THEN RAISE EXCEPTION '%', v ->> 'unrecorded'; END IF;
{guard}
{same}
{change}
{}
board_answer := 'true';
END change_;",
        super::logged(
            "v ->> 'slug'",
            "v ->> 'handle'",
            "'system'",
            "line_",
            "NULL"
        )
    ))
}
pub(in crate::store) fn close() -> Pg<'static> {
    recorded(true, false)
}
pub(in crate::store) fn close_as() -> Pg<'static> {
    recorded(true, true)
}
pub(in crate::store) fn reopen() -> Pg<'static> {
    recorded(false, false)
}
pub(in crate::store) fn reopen_as() -> Pg<'static> {
    recorded(false, true)
}

pub(in crate::store) fn record_found_closed() -> Pg<'static> {
    Pg::script(format!("<<change_>>
DECLARE conversation_ text := v #>> '{{row,claude_session}}'; thread_ text; line_ text;
BEGIN
{KNOWN}
{HOLDS}
IF (SELECT pane FROM task_session WHERE id = {KEY}) = v #>> '{{row,pane}}' THEN EXIT change_; END IF;
{SETTLED}
{CLOSURE}
{}
{}
board_answer := to_jsonb(conversation_);
END change_;", put(), super::logged("v ->> 'slug'", "v ->> 'handle'", "'system'", "line_", "NULL")))
}

pub(in crate::store) fn take_session() -> Pg<'static> {
    Pg::script("DECLARE lease_ jsonb; key_ jsonb := jsonb_build_array('session', v -> 'handle'); revision_ bigint;
BEGIN
INSERT INTO callback_lease (id, doc) VALUES (key_, '{}') ON CONFLICT (id) DO UPDATE SET doc = callback_lease.doc;
SELECT doc INTO lease_ FROM callback_lease WHERE id = key_;
IF coalesce((lease_ ->> 'until')::double precision, 0) > (v ->> 'now')::double precision THEN
    board_answer := jsonb_build_object('doing', lease_ -> 'doing', 'until', lease_ -> 'until');
ELSE
    revision_ := coalesce((lease_ ->> 'revision')::bigint, 0) + 1;
    UPDATE callback_lease SET doc = doc || jsonb_build_object('until', (v ->> 'now')::double precision + (v ->> 'seconds')::double precision, 'revision', revision_, 'doing', v -> 'doing') WHERE id = key_;
    board_answer := jsonb_build_object('revision', revision_);
END IF;
END;")
}
pub(in crate::store) fn renew_session() -> Pg<'static> {
    Pg::script(
        "UPDATE callback_lease SET doc = doc || jsonb_build_object('until', (v ->> 'now')::double precision + (v ->> 'seconds')::double precision) WHERE id = jsonb_build_array('session', v -> 'handle') AND doc -> 'revision' = v -> 'revision'; board_answer := to_jsonb(FOUND);",
    )
}
pub(in crate::store) fn release_session() -> Pg<'static> {
    Pg::script(
        "UPDATE callback_lease SET doc = doc || '{\"until\":0}' WHERE id = jsonb_build_array('session', v -> 'handle') AND doc -> 'revision' = v -> 'revision';",
    )
}
