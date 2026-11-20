//! The statements of `store/messages.rs`.

use super::logged;
use super::reading::NATIVE_PRESENCE;
use super::write::{ACT, AUTHORIZE};
use crate::stages::MOVED;
use crate::store::messages::Poster;
use crate::store::work::OWNER;
use std::sync::LazyLock;

/// The owner's moves, the one record line an inbox carries: `owner moved it FROM → TO`
/// ([`crate::stages::moved_line`]), the owner's word as its posts are. A table's row: its text is
/// read; a [`crate::store::pool::Pool`] holds no other record line ([`POOLED_MOVES`]).
pub(in crate::store) static OWNER_MOVES: LazyLock<String> = LazyLock::new(|| {
    format!(
        "(kind = 'system' AND sender = '{OWNER}' AND string::starts_with(text, '{OWNER}{MOVED}'))"
    )
});

/// [`OWNER_MOVES`] for a [`crate::store::pool::Pool`]'s row, `$m`: the pool reads no text, and
/// holds no record line but the owner's moves.
static POOLED_MOVES: LazyLock<String> =
    LazyLock::new(|| format!("($m.kind = 'system' AND $m.sender = '{OWNER}')"));

/// Whether a message could reach `$handle`'s inbox: delivered, or the owner's move (`moves`, its
/// test for the rows read), sent since the session connected, not its own, and from the owner, to
/// it, to a role or a verdict (not addressed to another handle or the owner). Its fields are read
/// under `m`: `""` for a row of the table, `"$m."` for a row of a [`crate::store::pool::Pool`].
/// The roles and the tree decide the rest ([`crate::store::Store::inbox_read`]).
fn could_reach(m: &str, moves: &str) -> String {
    format!(
        "{m}ts >= $connected AND ({m}kind IN $delivered OR {moves}) AND {m}sender != $handle
        AND ({m}sender = 'owner' OR {m}recipient = $handle OR ({m}recipient != NONE AND string::starts_with({m}recipient, '@')) OR {m}kind = 'verdict')"
    )
}

/// What an inbox is worked out from: the session's roles, the tree, and the messages since it
/// connected (after `$since`, when it is given) that are not its own, that it has not acknowledged,
/// and that could reach it. Read from the message table, through `message_ts` from the session's
/// connection or `message_sid` from `$since`, or from `$pool`, a dispatch pass's rows of it
/// ([`crate::store::pool::Pool`]), when one is given: then acknowledgements are looked up for those
/// rows alone, not read through `delivery_handle`. The connection time is a parameter of its own:
/// the server takes an index for a parameter's value, never for a field of one (`$me.connected`
/// read every message). Acknowledgement markers
/// overwrite decimal integer keys during tree construction, then one marker filter selects the
/// unread record references. Their bodies are fetched within the same transaction snapshot, and the
/// roles and the tree only when one is wanted. The orchestrators' sessions are fetched by id from
/// their roles: a `session WHERE handle IN (SELECT ... FROM role)` runs the role scan again for each
/// session, seconds on a board of thousands of each, for every inbox a message could reach.
pub(in crate::store) static INBOX: LazyLock<String> = LazyLock::new(|| {
    format!(
        "
LET $me = (SELECT connected, kind FROM ONLY type::record('session', $handle));
LET $connected = $me.connected;
LET $candidates = IF $me = NONE {{ [] }} ELSE IF $pool != NONE {{
    $pool.filter(|$m| $m.sid > $since AND {pooled}).map(|$m| [<string>$m.sid, type::record('message', [$m.slug, $m.sid])])
}} ELSE IF $since = NONE {{
    (SELECT VALUE [<string>sid, id] FROM message WHERE {scan})
}} ELSE {{
    (SELECT VALUE [<string>sid, id] FROM message WHERE sid > $since AND {scan})
}};
LET $acked = IF $pool != NONE {{
    $candidates.filter(|$c| record::exists(type::record('delivery', [<int>$c[0], $handle]))).map(|$c| [$c[0], NULL])
}} ELSE IF $since = NONE {{
    (SELECT VALUE [<string>message, NULL] FROM delivery WHERE handle = $handle)
}} ELSE {{
    (SELECT VALUE [<string>message, NULL] FROM delivery WHERE handle = $handle AND message > $since)
}};
{unread}
LET $placing = array::len($wanted) > 0;
LET $orchestrators = IF $placing {{ (SELECT VALUE [slug, handle] FROM role WHERE role = 'orchestrator') }} ELSE {{ [] }};
RETURN {{
    orchestrators: $orchestrators,
    orchestrator_sessions: (SELECT * OMIT id FROM array::distinct($orchestrators.map(|$held| type::record('session', $held[1])))),
    native_presence: IF $placing {{ {NATIVE_PRESENCE} }} ELSE {{ [] }},
    known: $me != NONE,
    orchestrating: $me.kind = 'orchestrator',
    roles: IF $placing {{ (SELECT slug, role FROM role WHERE handle = $handle) }} ELSE {{ [] }},
    tree: IF $placing {{ (SELECT VALUE [slug, parent] FROM task) }} ELSE {{ [] }},
    messages: (SELECT * OMIT id FROM $wanted ORDER BY sid),
}};",
        scan = could_reach("", &OWNER_MOVES),
        pooled = could_reach("$m.", &POOLED_MOVES),
        unread = include_str!("inbox_unread.surql"),
    )
});

/// `line`, a message's write, made once per `$key` on `$slug`'s thread: the first write under the
/// key makes the line and `delivery_key:[slug, key]`, which names that line to every later one,
/// and a later write makes nothing. Given `same`, a condition on the thread's lines, the first
/// write under the key instead takes the earliest line meeting it when the thread holds one (a
/// line made under no key), binding the key to that line and writing nothing. `$had` is the key's
/// record before this write.
fn once(line: &str, same: Option<&str>) -> String {
    let make = format!(
        "{line}
    CREATE type::record('delivery_key', [$slug, $key]) SET slug = $slug, key = $key, message = $sid, ts = $ts RETURN NONE;"
    );
    let write = match same {
        None => make,
        Some(same) => format!(
            "LET $same = array::first((SELECT VALUE sid FROM message:[$slug, NONE]..[$slug, ..] WHERE {same} LIMIT 1));
    IF $same != NONE {{
        CREATE type::record('delivery_key', [$slug, $key]) SET slug = $slug, key = $key, message = $same, ts = $now RETURN NONE;
    }} ELSE {{
        {make}
    }};"
        ),
    };
    format!(
        "LET $had = (SELECT message FROM ONLY type::record('delivery_key', [$slug, $key]));
IF $had = NONE {{
    {write}
}};"
    )
}

/// A post of `$text` by `$sender` to `$to` on `$slug`'s thread, authorized as `poster`, answering
/// the line on the record: this post's, or with `keyed` the one `$key` names, made once per key
/// ([`once`]), which with `Some(true)` takes the thread's earliest line reading the same made
/// under no key. The task's ask moves with the line ([`super::asks::ON_THREAD`], its variables
/// `$task_about`, `$task_words` and `$asking`). Refuses with `$no_task`, `$no_role` when it is
/// given, and `$nobody` when `$handle` names no session.
pub(in crate::store) fn post(poster: Poster, keyed: Option<bool>) -> String {
    let act = match poster {
        Poster::Owner => "",
        // A machine event is no tool call or heartbeat from the waiting session.
        Poster::Board => AUTHORIZE,
        Poster::Session => ACT,
    };
    // The task's ask moves with the line: the owner's opens or joins it, an answer closes it.
    let line = format!(
        "{}\n{}",
        logged("$slug", "$sender", "'msg'", "$text", "$to"),
        super::asks::ON_THREAD
    );
    // The answer is the line on the record: this post's, or the one its key names.
    let said = match keyed {
        None => format!(
            "{line}
RETURN (SELECT * OMIT id FROM ONLY type::record('message', [$slug, $sid]));"
        ),
        Some(adopt) => format!(
            "{}
LET $made = (SELECT message FROM ONLY type::record('delivery_key', [$slug, $key]));
RETURN (SELECT * OMIT id FROM ONLY type::record('message', [$slug, $made.message]));",
            once(
                &line,
                adopt.then_some("kind = 'msg' AND text = $text AND recipient = $to")
            )
        ),
    };
    format!(
        "{act}
IF !record::exists(type::record('task', $slug)) {{ THROW $no_task }};
IF $no_role != NONE {{ THROW $no_role }};
IF $handle != NONE AND !record::exists(type::record('session', $handle)) {{ THROW $nobody }};
{said}"
    )
}

/// The board's line `$text` from `$sender` to `$to` on `$slug`'s thread at `$now`; `keyed`, once
/// per `$key` ([`once`]), a key that names no line yet taking the thread's earliest line of
/// `$sender`'s that starts with `$said`.
pub(in crate::store) fn unread(keyed: bool) -> String {
    let line = logged("$slug", "$sender", "'msg'", "$text", "$to");
    format!(
        "{}\nRETURN NONE;",
        if keyed {
            once(
                &line,
                Some("sender = $sender AND string::starts_with(text, $said)"),
            )
        } else {
            line
        }
    )
}

/// `$said`, a record line `$sender` puts on `$slug`'s thread, to no one, made once per `$key`
/// ([`once`]).
pub(in crate::store) static RECORD_ONCE: LazyLock<String> = LazyLock::new(|| {
    format!(
        "{}\nRETURN NONE;",
        once(
            &logged("$slug", "$sender", "'system'", "$said", "NONE"),
            None
        )
    )
});

/// Whether a line was made under `$key` on `$slug`'s thread ([`once`]).
pub(in crate::store) const KEYED: &str =
    "RETURN record::exists(type::record('delivery_key', [$slug, $key]));";

/// `$actor` has read the messages `$ids` at `$now`: a delivery for each it has none of.
pub(in crate::store) fn ack() -> String {
    format!(
        "{ACT}
FOR $id IN $ids {{
    IF !record::exists(type::record('delivery', [$id, $actor])) {{
        CREATE type::record('delivery', [$id, $actor]) CONTENT {{ message: $id, handle: $actor, acked: $now }} RETURN NONE;
    }};
}};
RETURN NONE;"
    )
}

/// The newest `$limit` messages to the owner, newest first.
pub(in crate::store) const TO_OWNER: &str = "RETURN SELECT * OMIT id FROM message WHERE recipient = 'owner' ORDER BY sid DESC LIMIT $limit;";

/// The posts to `$person` from anyone else on the tasks `$open` that no line of `$person`'s on the
/// same thread follows, oldest first.
pub(in crate::store) const QUESTIONS_WAITING: &str = "
LET $asked = (SELECT * OMIT id FROM message
    WHERE recipient = $person AND kind = 'msg' AND sender != $person AND slug IN $open ORDER BY sid ASC);
RETURN $asked.filter(|$line| (SELECT VALUE sid FROM message:[$line.slug, $line.sid]>..[$line.slug, ..]
    WHERE sender = $person LIMIT 1) = []);";

/// The newest verdict line whose text is `approve` or `changes`, alone or before its `: text`.
pub(in crate::store) const NEWEST_VERDICT: &str =
    "RETURN SELECT * OMIT id FROM message WHERE kind = 'verdict'
    AND (text = 'approve' OR string::starts_with(text, 'approve:')
        OR text = 'changes' OR string::starts_with(text, 'changes:'))
    ORDER BY sid DESC LIMIT 1;";

/// The messages after `$since` up to `$through`, in the order made.
pub(in crate::store) const MESSAGES_AFTER: &str =
    "RETURN SELECT * OMIT id FROM message WHERE sid > $since AND sid <= $through ORDER BY sid ASC;";

/// Each sender's latest message or verdict, through `said`.
pub(in crate::store) const LAST_MESSAGES: &str =
    "LET $said = (SELECT VALUE type::record('message', [slug, sid]) FROM said);
RETURN SELECT * OMIT id FROM $said;";

/// The slugs whose newest `landed <slug> on main` line, of the kinds `$delivered`, no verdict,
/// move or claim on its thread follows.
pub(in crate::store) const LANDINGS_POSTED: &str = "
LET $landed = (SELECT slug, math::max(sid) AS sid FROM message
    WHERE text @@ 'landed' AND kind IN $delivered AND string::contains(text, 'landed ' + slug + ' on main')
    GROUP BY slug);
RETURN $landed.filter(|$line| (SELECT VALUE sid FROM message:[$line.slug, $line.sid]>..[$line.slug, ..]
    WHERE kind = 'verdict' OR (kind = 'system' AND (string::contains(text, ' moved it ') OR string::contains(text, ' claimed it')))
    LIMIT 1) = []).map(|$line| $line.slug);";
