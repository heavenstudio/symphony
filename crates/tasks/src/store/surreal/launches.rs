//! The statements of `store/launches.rs`.

use super::logged;

/// A launch record `$row` written by its named fields, so a field of an additive step this build
/// does not know is kept on the row ([`crate::schema::OPEN`]).
const LAUNCH_ROW: &str = "slug = $row.slug, handle = $row.handle, role = $row.role, \
     claude_session = $row.claude_session, worktree = $row.worktree, branch = $row.branch, \
     pane = $row.pane, model = $row.model, started = $row.started, closed = $row.closed, \
     closed_why = $row.closed_why, restore = $row.restore";

/// The conversation a write of a record of `$handle` keeps, as `$conversation` (`$session` is
/// SurrealDB's own): the one `$given`, but a Codex one whose thread is not known yet (`codex:`) takes
/// the thread the handle's callback registered (`codex:THREAD`), so the thread is kept whichever was
/// written first, the callback or the record.
const SETTLED: &str = "LET $thread = type::record('callback_state', $handle).address ?? '';
LET $conversation = IF $given = 'codex:' AND string::starts_with($thread, 'codex:') AND $thread != 'codex:' { $thread } ELSE { $given };";

/// The closure's line, as `$line`: `$said`, and `$lost` after it when the conversation it keeps is
/// a Codex one whose thread never arrived.
const CLOSURE_LINE: &str =
    "LET $line = IF $conversation = 'codex:' { string::concat($said, $lost) } ELSE { $said };";

/// Stops a change of a record that is no longer the launch `$started` in terminal `$pane`.
const SAME_LAUNCH: &str = "LET $at = type::record('task_session', [$slug, $handle]);
IF $at.pane != $pane OR $at.started != $started { RETURN false };";

/// A change to a recorded task session, refused if there is none, said on the thread as `said`
/// (an expression). `guard` comes first, once the record is known to exist: a statement that may
/// end the change (`RETURN false`) when the record is no longer the one the change is about.
fn recorded(guard: &str, change: &str, said: &str) -> String {
    format!(
        "IF !record::exists(type::record('task_session', [$slug, $handle])) {{ THROW $unrecorded }};
{guard}
UPDATE type::record('task_session', [$slug, $handle]) {change} RETURN NONE;
{}
RETURN true;",
        logged("$slug", "$handle", "'system'", said, "NONE")
    )
}

/// The closure of the record of `$slug` and `$handle` (as `Store::close_task_session` says it),
/// once `guard` lets it through.
fn closing(guard: &str) -> String {
    format!(
        "LET $given = type::record('task_session', [$slug, $handle]).claude_session;
{SETTLED}
{CLOSURE_LINE}
{}",
        recorded(
            guard,
            "SET closed = $now, closed_why = $why, restore = $restore, claude_session = $conversation",
            "$line"
        )
    )
}

/// Stops a write of a close or a restore once its hold of `$handle`'s session (`$hold`, the revision
/// `Store::take_session` gave it) is no longer the session's, lapsed or not: another close or
/// restore took it over. Nothing is written, and the write is refused (`held`). While the hold is
/// its own, the write also writes the hold's row, as a takeover does: the board's transactions are
/// optimistic and conflict only on what both write, so a takeover that commits while the write is
/// under way makes it fail, and tried again (`retrying`) it is refused.
pub(in crate::store) const HOLDS: &str =
    "IF type::record('callback_lease', ['session', $handle]).revision != $hold { THROW $taken };
UPDATE type::record('callback_lease', ['session', $handle]) SET writes = (writes ?? 0) + 1 RETURN NONE;";

/// `$handle` is recorded as running `$slug` (`$row`, with the conversation [`SETTLED`] keeps),
/// replacing any record of them, said on the thread when `log`: the conversation kept. Needs
/// `$given`, `$now`, `$said_before`, `$said_after`, `$no_task` and `$no_session`.
pub(in crate::store) fn record_launch(log: bool) -> String {
    let line = if log {
        logged(
            "$slug",
            "$handle",
            "'system'",
            "string::concat($said_before, $said_conversation, $said_after)",
            "NONE",
        )
    } else {
        String::new()
    };
    format!(
        "IF !record::exists(type::record('task', $slug)) {{ THROW $no_task }};
IF !record::exists(type::record('session', $handle)) {{ THROW $no_session }};
{SETTLED}
LET $said_conversation = IF $conversation = '' {{ 'unknown' }} ELSE {{ $conversation }};
UPSERT type::record('task_session', [$slug, $handle]) SET {LAUNCH_ROW} RETURN NONE;
UPDATE type::record('task_session', [$slug, $handle]) SET claude_session = $conversation RETURN NONE;
{line}
RETURN $conversation;"
    )
}

/// The task sessions `filter` (conditions and order) keeps.
fn task_sessions(filter: &str) -> String {
    format!("RETURN SELECT * OMIT id FROM task_session {filter};")
}

/// The sessions launched for `$slug`, by when they started.
pub(in crate::store) fn sessions_of_task() -> String {
    task_sessions("WHERE slug = $slug ORDER BY started, handle")
}

/// The tasks `$handle` was launched for, by when it started on each.
pub(in crate::store) fn sessions_of_handle() -> String {
    task_sessions("WHERE handle = $handle ORDER BY started, slug")
}

/// Every task session, by when it started.
pub(in crate::store) fn all_sessions() -> String {
    task_sessions("ORDER BY started, slug, handle")
}

/// The task sessions whose pane is not closed, by when they started.
pub(in crate::store) fn open_sessions() -> String {
    task_sessions("WHERE closed = NONE ORDER BY started, slug, handle")
}

/// The record of `$slug` and `$handle` closed at `$now` ([`closing`], unguarded): true. Needs
/// `$why`, `$restore`, `$said`, `$lost` and `$unrecorded`.
pub(in crate::store) fn close() -> String {
    closing("")
}

/// [`close`] by the close or restore holding the session ([`HOLDS`]), only while the record is
/// still that launch ([`SAME_LAUNCH`]) and open: whether it closed it. Needs [`close`]'s
/// variables and `$hold`, `$taken`, `$pane` and `$started`.
pub(in crate::store) fn close_as() -> String {
    let guard = format!("{HOLDS}\n{SAME_LAUNCH}\nIF $at.closed != NONE {{ RETURN false }};");
    closing(&guard)
}

/// `$row` recorded as a session found running and closed at once by the close holding it
/// ([`HOLDS`]), said in the closure's line: the conversation kept, or none when the record already
/// names that terminal. Needs `$slug`, `$handle`, `$now`, `$said`, `$lost`, `$hold`, `$taken`,
/// `$no_task` and `$no_session`.
pub(in crate::store) fn record_found_closed() -> String {
    format!(
        "IF !record::exists(type::record('task', $slug)) {{ THROW $no_task }};
IF !record::exists(type::record('session', $handle)) {{ THROW $no_session }};
{HOLDS}
IF type::record('task_session', [$slug, $handle]).pane = $row.pane {{ RETURN NONE }};
LET $given = $row.claude_session;
{SETTLED}
{CLOSURE_LINE}
UPSERT type::record('task_session', [$slug, $handle]) SET {LAUNCH_ROW} RETURN NONE;
UPDATE type::record('task_session', [$slug, $handle]) SET claude_session = $conversation RETURN NONE;
{}
RETURN $conversation;",
        logged("$slug", "$handle", "'system'", "$line", "NONE")
    )
}

/// The record of `$slug` and `$handle` runs again, in `$pane`, said as `$said`: true. Needs `$now`
/// and `$unrecorded`.
pub(in crate::store) fn reopen() -> String {
    recorded(
        "",
        "SET pane = $pane, closed = NONE, closed_why = ''",
        "$said",
    )
}

/// [`reopen`] in `$into` by the restore holding the session ([`HOLDS`]), only while the record is
/// still that launch ([`SAME_LAUNCH`], `$pane` and `$started`) and closed, its session revived
/// with it: whether it reopened it. Needs `$said`, `$now`, `$hold`, `$taken` and `$unrecorded`.
pub(in crate::store) fn reopen_as() -> String {
    let guard = format!(
        "{HOLDS}\n{SAME_LAUNCH}\nIF $at.closed = NONE {{ RETURN false }};
UPDATE type::record('session', $handle) SET left_at = NONE, last_seen = math::max([last_seen, $now]) RETURN NONE;"
    );
    recorded(
        &guard,
        "SET pane = $into, closed = NONE, closed_why = ''",
        "$said",
    )
}

/// The lease of `$handle`'s session taken for `$doing` until `$now + $seconds`: its new revision,
/// or who holds it until when while another's has not lapsed.
pub(in crate::store) const TAKE_SESSION: &str = "LET $lease = (SELECT until, revision, doing FROM ONLY type::record('callback_lease', ['session', $handle]));
                 IF ($lease.until ?? 0) > $now { RETURN { doing: $lease.doing, until: $lease.until } };
                 LET $revision = ($lease.revision ?? 0) + 1;
                 UPSERT type::record('callback_lease', ['session', $handle]) SET until = $now + $seconds, revision = $revision, doing = $doing RETURN NONE;
                 RETURN { revision: $revision };";

/// The hold `$revision` of `$handle`'s session held until `$now + $seconds`: false once the lease
/// is another's.
pub(in crate::store) const RENEW_SESSION: &str = "IF (SELECT VALUE revision FROM ONLY type::record('callback_lease', ['session', $handle])) != $revision { RETURN false };
                 UPDATE type::record('callback_lease', ['session', $handle]) SET until = $now + $seconds RETURN NONE;
                 RETURN true;";

/// The hold `$revision` of `$handle`'s session ended; one another took since is left as it is.
pub(in crate::store) const RELEASE_SESSION: &str = "UPDATE type::record('callback_lease', ['session', $handle]) SET until = 0 WHERE revision = $revision RETURN NONE; RETURN NONE;";
