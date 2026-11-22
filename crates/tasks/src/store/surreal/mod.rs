//! Every statement the store sends, in SurrealQL: the one place the board's code says anything in
//! the database's language.
//!
//! A module here stands beside the store module of the same name and holds its statements as that
//! module asks for them: a whole request by name, a constant or a function of the few values its
//! text turns on. The store's own modules hold the rules (what is read, what is decided, the
//! variables a statement is handed and the shape of its answer) and no SurrealQL: a statement's
//! name, its variables and its answer are what another database would have to give, and its text
//! here is how this one does. `fence_tests.rs` holds the line.
//!
//! What stays outside, each for its reason: `crate::schema` (the tables, the indexes and the
//! migration steps are the database's own definition), `crate::rpc` (the client, which tells a
//! read from a write by a statement's words) and `crate::server` (the keeper of the SurrealDB
//! server, which asks it of itself).

pub(in crate::store) mod artifacts;
pub(in crate::store) mod asks;
pub(in crate::store) mod branches;
pub(in crate::store) mod callbacks;
pub(in crate::store) mod cap;
pub(in crate::store) mod copy;
pub(in crate::store) mod depends;
pub(in crate::store) mod dispatcher;
pub(in crate::store) mod effects;
pub(in crate::store) mod execution;
pub(in crate::store) mod fields;
pub(in crate::store) mod guard;
pub(in crate::store) mod history;
pub(in crate::store) mod launches;
pub(in crate::store) mod messages;
pub(in crate::store) mod opening;
pub(in crate::store) mod pane_watch;
pub(in crate::store) mod pool;
pub(in crate::store) mod raw;
pub(in crate::store) mod reading;
pub(in crate::store) mod recovery;
pub(in crate::store) mod report_lines;
pub(in crate::store) mod resume;
pub(in crate::store) mod search;
pub(in crate::store) mod session_urls;
pub(in crate::store) mod sessions;
pub(in crate::store) mod snapshot;
pub(in crate::store) mod spending;
pub(in crate::store) mod stage_lines;
pub(in crate::store) mod stamp;
pub(in crate::store) mod stored_settings;
pub(in crate::store) mod thread_window;
pub(in crate::store) mod unanswered;
pub(in crate::store) mod waits;
pub(in crate::store) mod work;
pub(in crate::store) mod workflow;
pub(in crate::store) mod workflow_events;
pub(in crate::store) mod write;
pub(in crate::store) mod writers;

use super::RuleParts;
use write::WRITE;

#[cfg(test)]
mod fence_tests;

/// The slugs of every task, or of those in `$status` when `by_status`.
pub(in crate::store) fn task_slugs(by_status: bool) -> &'static str {
    if by_status {
        "RETURN SELECT VALUE slug FROM task WHERE status = $status;"
    } else {
        "RETURN SELECT VALUE slug FROM task;"
    }
}

/// The tasks `$handle` holds that another could take over ([`CLAIMABLE`]), as a read of its own.
pub(in crate::store) fn claimable() -> String {
    format!("RETURN {CLAIMABLE};")
}

/// Whether `$slug` is a task, and its thread in order.
pub(in crate::store) const THREAD: &str = "RETURN { known: record::exists(type::record('task', $slug)),
                            messages: (SELECT * OMIT id FROM message:[$slug, NONE]..[$slug, ..]) };";

/// The threads of `$slugs`, each in order, and those of them no task has.
pub(in crate::store) const THREADS: &str = "RETURN {
    unknown: $slugs.filter(|$slug| !record::exists(type::record('task', $slug))),
    threads: $slugs.map(|$slug| (SELECT * OMIT id FROM message:[$slug, NONE]..[$slug, ..])),
};";

/// A rule's write ([`WRITE`]) with what its change adds ([`RuleParts`]): what a block waited on
/// removed by a change of status out of blocked, what it waits on written (`blocked`), the move's
/// `reason` posted after its line, the task's `needs` and its `gates` written, its mark of an
/// approve not landed set or lifted (`unlanded`), a change of status (`moved`, whether to
/// cancelled) settling its hold and its dependants, and the slot counters it `holds` guarded.
pub(in crate::store) fn rule(parts: RuleParts) -> String {
    let RuleParts {
        placed,
        reason,
        blocked,
        needs,
        gates,
        unlanded,
        moved,
        holds,
    } = parts;
    // A task reopened under a custom workflow is placed first, so the rule's own write meets a
    // task the workflow holds at the status it moves to (`workflow::REOPEN_PLACED`).
    let mut body = String::from(if placed { workflow::REOPEN_PLACED } else { "" });
    body.push_str(WRITE);
    if moved.is_some() {
        body.push_str(work::UNBLOCKED);
    }
    if blocked {
        body.push_str(work::BLOCKED_WRITTEN);
    }
    if reason {
        body.push('\n');
        body.push_str(&work::reason_written());
    }
    if needs {
        body.push_str(work::NEEDS_WRITTEN);
    }
    if gates {
        body.push_str(work::GATES_WRITTEN);
    }
    if unlanded {
        body.push_str(work::UNLANDED_WRITTEN);
    }
    if let Some(cancelled) = moved {
        body.push_str(&format!(
            "{}\n{}",
            cap::HOLDING,
            work::block_cancelled_dependencies(cancelled)
        ));
    }
    if holds {
        body.push_str(cap::GUARD);
    }
    body
}

/// `body`'s writes, then whether any session that has not left keeps a callback address, then
/// `read`: a task write's one request (`Store::written`).
pub(in crate::store) fn written(body: &str, read: &str) -> String {
    format!(
        "{body}\nLET $has_callbacks = array::len(SELECT handle FROM callback_state WHERE address != NONE AND type::record('session', handle).left_at = NONE LIMIT 1) > 0;\n{read}"
    )
}

/// One session, `$handle`, and whether it holds work another could take over (a `Found`). Needs
/// `$executing`.
pub(in crate::store) fn session_read() -> String {
    format!(
        "LET $c = (SELECT address, confirmed_until FROM ONLY type::record('callback_state', $handle));
         RETURN {{ session: (SELECT * OMIT id FROM ONLY type::record('session', $handle)), held: {CLAIMABLE},
             native_until: IF string::starts_with($c.address ?? '', 'codex:') {{ $c.confirmed_until ?? 0 }} ELSE {{ NONE }} }};"
    )
}

/// A role row's condition that its work is one another session could take over
/// (`health::claimable`, in the database): its task executed and in `$executing`, or reviewed and in
/// review. A macro, so a query can join it in with `concat!`.
macro_rules! holding {
    () => {
        "((role = 'executor' AND type::record('task', slug).status IN $executing)
      OR (role = 'reviewer' AND type::record('task', slug).status = 'review'))"
    };
}
pub(in crate::store) use holding;

/// The tasks `$handle` holds that another could take over.
pub(in crate::store) const CLAIMABLE: &str = concat!(
    "(SELECT VALUE slug FROM role WHERE handle = $handle AND ",
    holding!(),
    ")"
);

/// Statements that put a line on a task's thread at `$now`, each argument a SurrealQL expression. A line is
/// never dated before the one numbered ahead of it when that one is less than a minute ahead (a write reads
/// its clock at most `PATIENCE` before it commits); one further ahead came from a wrong clock, and the
/// lines after it keep their own dates rather than all take its.
pub(in crate::store) fn logged(
    slug: &str,
    sender: &str,
    kind: &str,
    text: &str,
    recipient: &str,
) -> String {
    format!(
        "LET $counted = (UPSERT ONLY counter:message SET last += 1, ts = IF ts != NONE AND ts > $now AND ts <= $now + 60 {{ ts }} ELSE {{ $now }});
LET $sid = $counted.last;
LET $ts = $counted.ts;
CREATE type::record('message', [{slug}, $sid]) SET sid = $sid, slug = {slug}, sender = {sender}, recipient = {recipient},
    kind = {kind}, text = {text}, ts = $ts RETURN NONE;"
    )
}

/// `body` as one transaction, behind `guard` (the schema's, or nothing).
pub(in crate::store) fn transaction(guard: &str, body: &str) -> String {
    format!("BEGIN TRANSACTION;\n{guard}\n{body}\nCOMMIT TRANSACTION;")
}

/// Whether `body`'s statements put a line on a thread ([`logged`]), which its callbacks then hear.
pub(in crate::store) fn posts(body: &str) -> bool {
    body.contains("CREATE type::record('message',")
}

/// A request that asks nothing: what making a board's namespace and database is sent with.
pub(in crate::store) const NOTHING: &str = "RETURN NONE;";
