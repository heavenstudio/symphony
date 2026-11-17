//! Every statement the store sends to a board on PostgreSQL: the twin of each SurrealQL statement
//! of [`super::surreal`] that has one so far, by the same name in a module of the same name, and
//! the board's tables ([`schema`]).
//!
//! A statement here is one of the shapes of [`Pg`]. Its variables are the one `jsonb` the store
//! hands it, `v` in its text, a field of it read as `v ->> 'name'` (text) or `v -> 'name'`
//! (`jsonb`), and nothing a caller wrote is ever spliced into one. A read is an expression over
//! `v`, its value the answer. A write is a PL/pgSQL block that reads `v` the same way, after the
//! schema's guard when it is one ([`GUARD`]), and gives its answer as `board_answer`, or is
//! followed by the read whose value is the answer. A request's statements, and the guard before
//! them, run in one transaction on the client's connection ([`crate::pg`]), REPEATABLE READ: a
//! write that meets another's committed since its snapshot fails at once, a conflict read and
//! decided again as on SurrealDB. A block raises `stale` where SurrealQL throws it, and a refusal
//! in its words where SurrealQL throws one.
//!
//! The marker's triggers and each sender's last word are the schema's ([`schema`]), as SurrealDB's
//! events are its schema's.

pub(in crate::store) mod asks;
pub(in crate::store) mod branches;
pub(in crate::store) mod callbacks;
pub(in crate::store) mod cap;
pub(in crate::store) mod copy;
pub(in crate::store) mod depends;
pub(in crate::store) mod dispatcher;
pub(in crate::store) mod fields;
pub(in crate::store) mod history;
pub(in crate::store) mod launches;
pub(in crate::store) mod messages;
pub(in crate::store) mod opening;
pub(in crate::store) mod pane_watch;
pub(in crate::store) mod pool;
pub(in crate::store) mod raw;
pub(in crate::store) mod reading;
pub(in crate::store) mod report_lines;
pub(in crate::store) mod resume;
pub(in crate::store) mod schema;
pub(in crate::store) mod session_urls;
pub(in crate::store) mod sessions;
pub(in crate::store) mod snapshot;
pub(in crate::store) mod spending;
pub(in crate::store) mod stage_lines;
pub(in crate::store) mod stamp;
pub(in crate::store) mod thread_window;
pub(in crate::store) mod unanswered;
pub(in crate::store) mod waits;
pub(in crate::store) mod work;
pub(in crate::store) mod write;
pub(in crate::store) mod writers;

use std::borrow::Cow;

/// The slugs of every task, or of those in `v ->> 'status'` when `by_status`
/// (`super::surreal::task_slugs`).
pub(in crate::store) fn task_slugs(by_status: bool) -> Pg<'static> {
    Pg::read(if by_status {
        "coalesce((SELECT jsonb_agg(slug ORDER BY slug) FROM task WHERE status = v ->> 'status'), '[]')"
    } else {
        "coalesce((SELECT jsonb_agg(slug ORDER BY slug) FROM task), '[]')"
    })
}

/// The tasks `v ->> 'handle'` holds that another could take over, as a read of its own
/// (`super::surreal::claimable`).
pub(in crate::store) fn claimable() -> Pg<'static> {
    Pg::read(sessions::CLAIMABLE)
}

/// A rule's write ([`write::write`]) with what its change adds, as `super::surreal::rule` adds
/// it: what a block waited on removed by a change of status out of blocked, what it waits on
/// written (`blocked`), the move's `reason` posted after its line, the task's `needs` written, a
/// change of status (`moved`, whether to cancelled) settling its hold and its dependants, and the
/// slot counters it `holds` guarded.
pub(in crate::store) fn rule(
    reason: bool,
    needs: bool,
    blocked: bool,
    moved: Option<bool>,
    holds: bool,
) -> Pg<'static> {
    let mut body = write::write();
    let mut add = |part: &str| {
        body.push('\n');
        body.push_str(part);
    };
    if moved.is_some() {
        add(work::UNBLOCKED);
    }
    if blocked {
        add(&work::field_set("_board,blocked", "v -> 'blocked'"));
    }
    if reason {
        add(&work::reason_written());
    }
    if needs {
        add(&work::field_set("_board,needs", "v -> 'needs'"));
    }
    if let Some(cancelled) = moved {
        add(cap::HOLDING);
        add(&work::block_cancelled_dependencies(cancelled));
    }
    if holds {
        add(cap::GUARD);
    }
    Pg::script(body)
}

/// A statement on PostgreSQL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::store) enum Pg<'a> {
    /// Nothing to send: what SurrealDB needs that PostgreSQL does not (an analyzer).
    Nothing,
    /// An expression over `v` whose value is the answer.
    Read(Cow<'a, str>),
    /// A PL/pgSQL block over `v`, then the expression whose value is the answer, or none for the
    /// block's own `board_answer`.
    Script(Cow<'a, str>, Option<Cow<'a, str>>),
    /// Statements that take no variables, sent as one simple query: a migration's.
    Simple(Cow<'a, str>),
}

impl<'a> Pg<'a> {
    pub(in crate::store) fn read(expression: impl Into<Cow<'a, str>>) -> Pg<'a> {
        Pg::Read(expression.into())
    }

    pub(in crate::store) fn script(block: impl Into<Cow<'a, str>>) -> Pg<'a> {
        Pg::Script(block.into(), None)
    }

    /// `block`, then the read `expression`, whose value is the answer.
    pub(in crate::store) fn script_then(
        block: impl Into<Cow<'a, str>>,
        expression: impl Into<Cow<'a, str>>,
    ) -> Pg<'a> {
        Pg::Script(block.into(), Some(expression.into()))
    }
}

/// What a [`Pg`] statement sends.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::store) enum Request {
    /// Each statement's text and whether it takes the variables as its `$1`, all in one
    /// transaction; the answer is the value of the one at `answer`.
    Steps {
        steps: Vec<(String, bool)>,
        answer: usize,
    },
    /// One simple query, which answers nothing.
    Simple(String),
}

/// The statements `statement` sends, behind the schema's guard when `guarded`; none for
/// [`Pg::Nothing`].
///
/// A request carries its variables once, in its first statement, and only its last answers
/// anything at length: a read alone binds them, and any other request sets them first ([`VARS`],
/// which answers nothing), where every statement after it reads them.
pub(in crate::store) fn request(statement: &Pg, guarded: bool) -> Option<Request> {
    let mut steps = Vec::new();
    let set = guarded || matches!(statement, Pg::Script(..));
    if set {
        steps.push((VARS.to_string(), true));
    }
    if guarded {
        steps.push((GUARD.to_string(), false));
    }
    let from = if set { SET } else { BOUND };
    match statement {
        Pg::Nothing => return None,
        Pg::Simple(sql) => return Some(Request::Simple(sql.to_string())),
        Pg::Read(expression) => steps.push((answered(expression, from), !set)),
        Pg::Script(block, then) => {
            steps.push((script(block), false));
            match then {
                Some(expression) => steps.push((answered(expression, SET), false)),
                None => steps.push((ANSWER.to_string(), false)),
            }
        }
    }
    let answer = steps.len() - 1;
    Some(Request::Steps { steps, answer })
}

/// The variables, set for the transaction, where every later statement reads them ([`SET`]): a
/// row of no columns is all it answers.
const VARS: &str = "SELECT FROM set_config('board.vars', $1::text, true)";

/// The variables as a statement binds them, its `$1`: a read sent alone.
const BOUND: &str = "$1::jsonb";

/// The variables as [`VARS`] set them, for a statement after it.
const SET: &str = "current_setting('board.vars')::jsonb";

/// A block's `board_answer`, which it set for the transaction.
const ANSWER: &str = "SELECT nullif(current_setting('board.answer', true), '')";

/// `expression`'s value, as `jsonb` text, `v` the variables `from` gives ([`BOUND`], [`SET`]).
fn answered(expression: &str, from: &str) -> String {
    format!("SELECT to_jsonb(({expression}))::text FROM (SELECT {from} AS v) AS board_vars")
}

/// `block` as a statement of its own: `v` the variables, `sid_` and `ts_` the line [`logged`]
/// numbered and dated, `board_answer` what it answers.
fn script(block: &str) -> String {
    format!(
        "DO $board$
DECLARE
    v jsonb := current_setting('board.vars')::jsonb;
    board_answer jsonb;
    sid_ bigint;
    ts_ double precision;
BEGIN
{block}
PERFORM set_config('board.answer', coalesce(board_answer::text, ''), true);
END
$board$"
    )
}

/// The statement every write starts with: it raises the refusal (`board_schema_*`,
/// [`crate::schema::guard_vars`]) unless the board is at this store's version.
pub(in crate::store) const GUARD: &str = "DO $guard$
DECLARE
    v jsonb := current_setting('board.vars')::jsonb;
    found_ bigint;
BEGIN
    SELECT version INTO found_ FROM meta WHERE id = '\"schema\"';
    IF found_ IS DISTINCT FROM (v ->> 'board_schema')::bigint THEN
        RAISE EXCEPTION '%', (v ->> 'board_schema_before') || coalesce(found_::text, 'NONE')
            || (v ->> 'board_schema_after');
    END IF;
END
$guard$";

/// `handle` (a text expression) fenced: its session's revision written, so a write that decided on
/// it meets every other that did ([`super::surreal::write`]).
pub(in crate::store) fn fence(handle: &str) -> String {
    format!(
        "INSERT INTO session_revision (id, last, writes) VALUES (to_jsonb(({handle})::text), 0, 1)
    ON CONFLICT (id) DO UPDATE SET writes = coalesce(session_revision.writes, 0) + 1;"
    )
}

/// The actor (`v ->> 'actor'`) a session that has not left, else refused in its words.
fn authorized() -> &'static str {
    "IF NOT EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'actor') THEN
    RAISE EXCEPTION '%', v ->> 'unknown_actor';
END IF;
IF EXISTS (SELECT 1 FROM session WHERE handle = v ->> 'actor' AND left_at IS NOT NULL) THEN
    RAISE EXCEPTION '%', v ->> 'left_actor';
END IF;"
}

/// A machine-authored notice's authorization: [`authorized`], and the actor fenced.
pub(in crate::store) fn authorize() -> String {
    format!("{}\n{}", authorized(), fence("v ->> 'actor'"))
}

/// A session's act: [`authorize`], and the actor seen at `now`.
pub(in crate::store) fn act() -> String {
    format!(
        "{}
UPDATE session SET last_seen = greatest(last_seen, (v ->> 'now')::double precision)
    WHERE handle = v ->> 'actor';",
        authorize()
    )
}

/// A line on a thread at `now`, each argument a text expression, numbered `sid_` and dated `ts_`
/// by the message counter as [`super::surreal::logged`] numbers and dates it: never dated before
/// the line numbered ahead of it when that one is less than a minute ahead.
pub(in crate::store) fn logged(
    slug: &str,
    sender: &str,
    kind: &str,
    text: &str,
    recipient: &str,
) -> String {
    format!(
        "INSERT INTO counter (id, last, ts) VALUES ('\"message\"', 1, (v ->> 'now')::double precision)
    ON CONFLICT (id) DO UPDATE SET last = counter.last + 1,
        ts = CASE WHEN counter.ts IS NOT NULL AND counter.ts > EXCLUDED.ts
            AND counter.ts <= EXCLUDED.ts + 60 THEN counter.ts ELSE EXCLUDED.ts END
    RETURNING last, ts INTO sid_, ts_;
INSERT INTO message (id, sid, slug, sender, recipient, kind, text, ts)
    VALUES (jsonb_build_array(({slug})::text, sid_), sid_, {slug}, {sender}, {recipient}, {kind}, {text}, ts_);"
    )
}

/// A row read as SurrealDB gives it, `row` a table's row or a query's: its unset fields left out,
/// and its id.
pub(in crate::store) fn row(row: &str) -> String {
    format!("(jsonb_strip_nulls(to_jsonb({row})) - 'id')")
}

/// The rows of `from` read as [`row`] reads one, in `order`, as a list: `alias` names a row.
pub(in crate::store) fn rows(alias: &str, from: &str, order: &str) -> String {
    format!(
        "coalesce((SELECT jsonb_agg({} ORDER BY {order}) FROM {from}), '[]')",
        row(alias)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Which of `steps` bind the variables: a request carries them once.
    fn binding(steps: &[(String, bool)]) -> Vec<usize> {
        let at = steps.iter().enumerate();
        at.filter(|(_, (_, takes))| *takes)
            .map(|(at, _)| at)
            .collect()
    }

    #[test]
    fn a_read_is_one_statement_and_a_write_sets_its_variables_runs_its_block_and_answers() {
        assert_eq!(
            request(&Pg::read("v -> 'a'"), false),
            Some(Request::Steps {
                steps: vec![(
                    "SELECT to_jsonb((v -> 'a'))::text FROM (SELECT $1::jsonb AS v) AS board_vars"
                        .into(),
                    true
                )],
                answer: 0
            })
        );
        let Some(Request::Steps { steps, answer }) = request(&Pg::script("NULL;"), true) else {
            panic!("a script is steps");
        };
        let kinds: Vec<(&str, bool)> = steps
            .iter()
            .map(|(sql, takes)| (sql.split_whitespace().next().unwrap(), *takes))
            .collect();
        assert_eq!(
            kinds,
            [
                ("SELECT", true),
                ("DO", false),
                ("DO", false),
                ("SELECT", false)
            ]
        );
        assert_eq!(steps[0].0, VARS);
        assert_eq!(steps[1].0, GUARD);
        assert!(steps[2].0.contains("\nNULL;\n"), "{}", steps[2].0);
        assert_eq!(steps[3].0, ANSWER);
        assert_eq!(answer, 3);
        let Some(Request::Steps { steps, answer }) = request(&Pg::script_then("NULL;", "v"), false)
        else {
            panic!("a script is steps");
        };
        assert_eq!(steps.len(), 3);
        assert_eq!(binding(&steps), [0], "only the first binds the variables");
        assert_eq!(
            steps[2].0,
            "SELECT to_jsonb((v))::text FROM (SELECT current_setting('board.vars')::jsonb AS v) \
             AS board_vars",
            "the read after the block takes the variables its first statement set"
        );
        assert_eq!(answer, 2);
        let Some(Request::Steps { steps, .. }) = request(&Pg::read("v"), true) else {
            panic!("a read is steps");
        };
        assert_eq!(steps.len(), 3);
        assert_eq!(binding(&steps), [0], "a guarded read binds them once too");
        assert!(steps[2].0.contains(SET), "{}", steps[2].0);
        assert_eq!(request(&Pg::Nothing, true), None);
        assert_eq!(
            request(&Pg::Simple("CREATE TABLE t ();".into()), false),
            Some(Request::Simple("CREATE TABLE t ();".into()))
        );
    }

    /// `rule`'s block, which a rule's write is.
    fn ruled(reason: bool, needs: bool, blocked: bool, moved: Option<bool>, holds: bool) -> String {
        match rule(reason, needs, blocked, moved, holds) {
            Pg::Script(block, None) => block.into_owned(),
            other => panic!("a rule is a block answering its own: {other:?}"),
        }
    }

    #[test]
    fn a_rule_adds_to_its_write_each_part_its_change_asks_for_in_surrealqls_order() {
        assert_eq!(ruled(false, false, false, None, false), write::write());
        let whole = ruled(true, true, true, Some(true), true);
        let parts = [
            write::write(),
            work::UNBLOCKED.to_string(),
            work::field_set("_board,blocked", "v -> 'blocked'"),
            work::reason_written(),
            work::field_set("_board,needs", "v -> 'needs'"),
            cap::HOLDING.to_string(),
            work::block_cancelled_dependencies(true),
            cap::GUARD.to_string(),
        ];
        let at: Vec<usize> = (parts.iter())
            .map(|part| {
                whole
                    .find(part.as_str())
                    .unwrap_or_else(|| panic!("{part}"))
            })
            .collect();
        assert!(at.windows(2).all(|pair| pair[0] < pair[1]), "{at:?}");
        assert_eq!(
            whole.len(),
            parts.iter().map(|part| part.len() + 1).sum::<usize>() - 1
        );

        let moved = ruled(false, false, false, Some(false), false);
        assert!(moved.contains(&work::block_cancelled_dependencies(false)));
        assert!(!moved.contains(&work::block_cancelled_dependencies(true)));
        assert!(moved.contains(work::UNBLOCKED) && moved.contains(cap::HOLDING));
        assert!(!moved.contains(cap::GUARD) && !moved.contains("move_reason"));
        let held = ruled(false, false, false, None, true);
        assert_eq!(held, format!("{}\n{}", write::write(), cap::GUARD));
    }

    #[test]
    fn the_slugs_read_names_a_status_only_when_asked_by_status() {
        let (Pg::Read(by_status), Pg::Read(all)) = (task_slugs(true), task_slugs(false)) else {
            panic!("the slugs are a read");
        };
        assert!(
            by_status.contains("WHERE status = v ->> 'status'"),
            "{by_status}"
        );
        assert!(!all.contains("WHERE"), "{all}");
        assert_eq!(claimable(), Pg::read(sessions::CLAIMABLE));
    }
}
