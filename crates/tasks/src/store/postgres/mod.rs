//! Every statement the store sends to a board on PostgreSQL: the twin of each SurrealQL statement
//! of [`super::surreal`] that has one so far, by the same name in a module of the same name, and
//! the board's tables ([`schema`]).
//!
//! A statement here is one of the shapes of [`Pg`]. Its variables are the one `jsonb` the store
//! hands it, `v` in its text, a field of it read as `v ->> 'name'` (text) or `v -> 'name'`
//! (`jsonb`), and nothing a caller wrote is ever spliced into one. A read is an expression over
//! `v`, its value the answer, or where that value would hold long lists, a statement for each
//! list, an item a row ([`Pg::Listed`]). A write is a PL/pgSQL block that reads `v` the same way, after the
//! schema's guard when it is one ([`GUARD`]), and gives its answer as `board_answer`, or is
//! followed by the read whose value is the answer. A request's statements, and the guard before
//! them, run in one transaction on the client's connection ([`crate::pg`]), REPEATABLE READ: a
//! write that meets another's committed since its snapshot fails at once, a conflict read and
//! decided again as on SurrealDB. A block raises `stale` where SurrealQL throws it, and a refusal
//! in its words where SurrealQL throws one.
//!
//! The marker's triggers and each sender's last word are the schema's ([`schema`]), as SurrealDB's
//! events are its schema's.

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
pub(in crate::store) mod schema;
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
/// written (`blocked`), the move's `reason` posted after its line, the task's `needs` and its
/// `gates` written, its mark of an approve not landed set or lifted (`unlanded`), a change of
/// status (`moved`, whether to cancelled) settling its hold and its dependants, and the slot
/// counters it `holds` guarded.
pub(in crate::store) fn rule(
    reason: bool,
    (needs, gates): (bool, bool),
    unlanded: bool,
    blocked: bool,
    moved: Option<bool>,
    holds: bool,
    placed: bool,
) -> Pg<'static> {
    // Twin of `super::surreal::rule`'s placement first.
    let mut body = if placed {
        format!("{}\n", workflow::REOPEN_PLACED)
    } else {
        String::new()
    };
    body.push_str(&write::write());
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
    if gates {
        add(&work::field_set("_board,gates", "v -> 'gates'"));
    }
    if unlanded {
        add(&work::field_set("_board,unlanded", work::UNLANDED));
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
    /// A read of long lists, each list a statement of its own that answers an item a row, all in
    /// the request's one transaction and so of one snapshot ([`Listed`]). One value of the same
    /// lists is built whole in the backend that answers it, in several times its size of memory,
    /// which the backend keeps until its connection ends, at its command's end: thirty commands
    /// reading a board at once then hold thirty boards.
    Listed(Listed),
    /// A PL/pgSQL block over `v`, then the expression whose value is the answer, or none for the
    /// block's own `board_answer`.
    Script(Cow<'a, str>, Option<Cow<'a, str>>),
    /// Statements that take no variables, sent as one simple query: a migration's.
    Simple(Cow<'a, str>),
    /// A PL/pgSQL block over `v` answering its own `board_answer`, which the server ends once it
    /// has run this many milliseconds (`statement_timeout`, for its transaction alone): a block
    /// that may outlast its caller's wait, and must not commit after the caller gave up on it.
    Within(u64, Cow<'a, str>),
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
    /// transaction, and which of them give the answer.
    Steps {
        steps: Vec<(String, bool)>,
        answer: Gives,
    },
    /// One simple query, which answers nothing.
    Simple(String),
}

/// Which of a request's statements give its answer.
#[derive(Debug, PartialEq, Eq)]
pub(in crate::store) enum Gives {
    /// The value of the statement at this place.
    Value(usize),
    /// Each list's key and the statement whose rows are its items, and the statement whose value
    /// is the object's other fields ([`joined`]).
    Lists {
        lists: Vec<(&'static str, usize)>,
        rest: Option<usize>,
    },
}

/// The statements `statement` sends, behind the schema's guard when `guarded`; none for
/// [`Pg::Nothing`].
///
/// A request carries its variables once, in its first statement, and only its last answers
/// anything at length, or each statement of its lists: a read of one statement binds them, and
/// any other request sets them first ([`VARS`], which answers nothing), where every statement
/// after it reads them.
pub(in crate::store) fn request(statement: &Pg, guarded: bool) -> Option<Request> {
    let mut steps = Vec::new();
    let several = matches!(statement, Pg::Listed(listed) if listed.statements() > 1);
    let set = guarded || several || matches!(statement, Pg::Script(..) | Pg::Within(..));
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
        Pg::Listed(listed) => {
            let mut lists = Vec::new();
            for list in &listed.lists {
                lists.push((list.key, steps.len()));
                steps.push((list.rows(&listed.with, from), !set));
            }
            let rest = listed.rest.as_ref().map(|rest| {
                steps.push((answered(&listed.object(rest), from), !set));
                steps.len() - 1
            });
            let answer = Gives::Lists { lists, rest };
            return Some(Request::Steps { steps, answer });
        }
        Pg::Within(millis, block) => {
            steps.push((within(*millis), false));
            steps.push((script(block), false));
            steps.push((ANSWER.to_string(), false));
        }
        Pg::Script(block, then) => {
            steps.push((script(block), false));
            match then {
                Some(expression) => steps.push((answered(expression, SET), false)),
                None => steps.push((ANSWER.to_string(), false)),
            }
        }
    }
    let answer = Gives::Value(steps.len() - 1);
    Some(Request::Steps { steps, answer })
}

/// The variables, set for the transaction, where every later statement reads them ([`SET`]): a
/// row of no columns is all it answers.
const VARS: &str = "SELECT FROM set_config('board.vars', $1::text, true)";

/// The statement that bounds the one after it: the server cancels a statement of this
/// transaction that runs longer than `millis`, and its transaction with it. The server times
/// each statement of a request from its own start, so the bound is the next block's.
fn within(millis: u64) -> String {
    format!("SELECT FROM set_config('statement_timeout', '{millis}', true)")
}

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

/// A read of long lists ([`Pg::Listed`]): the common table expressions its statements start
/// with (none: empty), its lists, and the other fields of its object, the arguments of a
/// `jsonb_build_object`. With no other fields, its one list is the answer, and has no key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::store) struct Listed {
    pub with: String,
    pub lists: Vec<List>,
    pub rest: Option<String>,
}

/// One list of a [`Listed`] read: the field it is, each item's value (an expression over a row
/// of `from`), the rows it lists, with their condition, and the order they come in: plain values
/// of a row, each ascending or `DESC`, with `, ` between them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::store) struct List {
    pub key: &'static str,
    pub item: String,
    pub from: String,
    pub order: String,
}

impl List {
    /// The list as one value.
    fn whole(&self) -> String {
        let List {
            item, from, order, ..
        } = self;
        format!("coalesce((SELECT jsonb_agg({item} ORDER BY {order}) FROM {from}), '[]')")
    }

    /// The list's items, a row each in its order, after `with`, `v` the variables `vars` gives.
    /// The order is the statement's own, by the list's ordering values, each a column beside
    /// the item: no order of a subquery's is relied on.
    fn rows(&self, with: &str, vars: &str) -> String {
        let List {
            item, from, order, ..
        } = self;
        let (mut places, mut by) = (String::new(), Vec::new());
        for (at, term) in order.split(", ").enumerate() {
            let (value, down) = match term.strip_suffix(" DESC") {
                Some(value) => (value, " DESC"),
                None => (term, ""),
            };
            places.push_str(&format!(", {value} AS place{at}"));
            by.push(format!("n.place{at}{down}"));
        }
        format!(
            "SELECT n.item FROM (SELECT {vars} AS v) AS board_vars, \
             LATERAL ({}SELECT ({item})::text AS item{places} FROM {from}) AS n ORDER BY {}",
            with_clause(with),
            by.join(", ")
        )
    }
}

/// `with` as a query's WITH clause, or nothing for none.
fn with_clause(with: &str) -> String {
    match with {
        "" => String::new(),
        with => format!("WITH {with} "),
    }
}

impl Listed {
    /// How many statements answer it: one for each list, and one for its other fields.
    fn statements(&self) -> usize {
        self.lists.len() + usize::from(self.rest.is_some())
    }

    /// The object of `fields` (the arguments of a `jsonb_build_object`), as an expression.
    fn object(&self, fields: &str) -> String {
        format!(
            "({}SELECT jsonb_build_object({fields}))",
            with_clause(&self.with)
        )
    }

    /// The read as one expression, each list whole: for a read of few rows that follows a block.
    pub(in crate::store) fn whole(&self) -> String {
        let mut fields: Vec<String> = (self.lists.iter())
            .map(|list| format!("'{}', {}", list.key, list.whole()))
            .collect();
        fields.extend(self.rest.clone());
        self.object(&fields.join(", "))
    }
}

/// The JSON text of a [`Listed`] read's answer: each list's items under its key, then the other
/// fields of `rest`, an object's text; or the one list alone, where there is no `rest`.
pub(in crate::store) fn joined(lists: &[(&str, Vec<&str>)], rest: Option<&str>) -> String {
    let list = |items: &[&str]| format!("[{}]", items.join(","));
    let Some(rest) = rest else {
        return lists
            .first()
            .map_or_else(|| "[]".into(), |(_, items)| list(items));
    };
    let keyed: Vec<String> = (lists.iter())
        .map(|(key, items)| format!("\"{key}\":{}", list(items)))
        .collect();
    // The other fields, after their object's opening brace: none of them is an object that
    // closes at once, which takes no comma before it.
    let fields = rest.trim_start().strip_prefix('{').unwrap_or(rest);
    let between = if keyed.is_empty() || fields.trim_start().starts_with('}') {
        ""
    } else {
        ","
    };
    format!("{{{}{between}{fields}", keyed.join(","))
}

/// The rows of `from` read as [`row`] reads one, in `order`, as a list an item a row: [`rows`]
/// as a statement of its own, `alias` naming a row.
pub(in crate::store) fn listed(alias: &str, from: &str, order: &str) -> Pg<'static> {
    list_of(row(alias), from, order)
}

/// The list of `item` (an expression over a row of `from`) for each row of `from`, in `order`,
/// an item a row.
pub(in crate::store) fn list_of(item: String, from: &str, order: &str) -> Pg<'static> {
    Pg::Listed(Listed {
        with: String::new(),
        lists: vec![List {
            key: "",
            item,
            from: from.into(),
            order: order.into(),
        }],
        rest: None,
    })
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
/// [`crate::schema::guard_vars`]) unless the board is at this store's version, or, for a write
/// that may (`board_schema_ahead`), one version ahead with a `writable_from` that is a number no
/// greater than this store's version ([`crate::schema::writes`]). The record is read from the row
/// as JSON, so a board whose `meta` has no such column yet (one a build before it made) reads as
/// none. The shared row lock meets a migration's version update, including from an older
/// repeatable-read snapshot: every migration writes that row, a move of additive steps alone too.
/// On a board of the store's own contract, it then raises (`board_additions_*`) unless the board's
/// additive steps are within what the write takes ([`crate::schema::Steps`]).
pub(in crate::store) const GUARD: &str = "DO $guard$
DECLARE
    v jsonb := current_setting('board.vars')::jsonb;
    own_ bigint := (v ->> 'board_schema')::bigint;
    found_ bigint;
    from_ jsonb;
    additions_ bigint;
BEGIN
    SELECT m.version, to_jsonb(m) -> 'writable_from' INTO found_, from_
        FROM meta m WHERE m.id = '\"schema\"' FOR SHARE;
    IF found_ IS DISTINCT FROM own_ AND NOT (
        coalesce(v -> 'board_schema_ahead' = 'true'::jsonb, false)
        AND found_ IS NOT DISTINCT FROM own_ + 1
        AND CASE WHEN jsonb_typeof(from_) = 'number' AND (from_ #>> '{}') ~ '^-?[0-9]+$'
            THEN (from_ #>> '{}')::numeric <= own_ ELSE false END
    ) THEN
        RAISE EXCEPTION '%', (v ->> 'board_schema_before') || coalesce(found_::text, 'NONE')
            || (v ->> 'board_schema_after');
    END IF;
    SELECT m.version INTO additions_ FROM meta m WHERE m.id = '\"additions\"';
    additions_ := coalesce(additions_, 0);
    IF found_ IS NOT DISTINCT FROM own_ AND (
        additions_ < coalesce((v ->> 'board_additions_least')::numeric, 0)
        OR additions_ > coalesce((v ->> 'board_additions_most')::numeric, 0)
    ) THEN
        RAISE EXCEPTION '%', (v ->> 'board_additions_before') || additions_::text
            || (v ->> 'board_additions_after');
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

/// A row read as SurrealDB gives it, `row` a table's row or a query's: its unset columns and id
/// left out. JSON values within a column keep their nulls.
pub(in crate::store) fn row(row: &str) -> String {
    format!(
        "(SELECT coalesce(jsonb_object_agg(key, value), '{{}}'::jsonb) \
         FROM jsonb_each(to_jsonb({row}) - 'id') WHERE value <> 'null'::jsonb)"
    )
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
                answer: Gives::Value(0)
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
        assert_eq!(answer, Gives::Value(3));
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
        assert_eq!(answer, Gives::Value(2));
        let Some(Request::Steps { steps, .. }) = request(&Pg::read("v"), true) else {
            panic!("a read is steps");
        };
        assert_eq!(steps.len(), 3);
        assert_eq!(binding(&steps), [0], "a guarded read binds them once too");
        assert!(steps[2].0.contains(SET), "{}", steps[2].0);
        let Some(Request::Steps { steps, answer }) =
            request(&Pg::Within(270_000, "NULL;".into()), true)
        else {
            panic!("a bounded block is steps");
        };
        assert_eq!(steps.len(), 5);
        assert_eq!(binding(&steps), [0]);
        assert_eq!(
            steps[2].0, "SELECT FROM set_config('statement_timeout', '270000', true)",
            "the bound is set after the guard and right before the block it bounds"
        );
        assert!(steps[3].0.contains("\nNULL;\n"), "{}", steps[3].0);
        assert_eq!((steps[4].0.as_str(), answer), (ANSWER, Gives::Value(4)));
        assert_eq!(request(&Pg::Nothing, true), None);
        assert_eq!(
            request(&Pg::Simple("CREATE TABLE t ();".into()), false),
            Some(Request::Simple("CREATE TABLE t ();".into()))
        );
    }

    /// A read of the lists `keys`, each of table `t`'s rows by `id`, with `rest` beside them.
    fn listing(keys: &[&'static str], rest: Option<&str>) -> Listed {
        let list = |key| List {
            key,
            item: "to_jsonb(t)".into(),
            from: "t WHERE t.at > (v ->> 'since')::bigint".into(),
            order: "t.at, t.id".into(),
        };
        Listed {
            with: String::new(),
            lists: keys.iter().copied().map(list).collect(),
            rest: rest.map(str::to_string),
        }
    }

    #[test]
    fn a_list_alone_is_one_statement_that_binds_the_variables_and_orders_its_own_rows() {
        let Pg::Listed(read) = listed("t", "t WHERE t.at > 1", "t.at, t.id") else {
            panic!("a list is listed");
        };
        assert_eq!((read.with.as_str(), read.rest.as_deref()), ("", None));
        assert_eq!(read.lists.len(), 1);
        assert_eq!(read.lists[0].item, row("t"));
        assert_eq!(
            request(&Pg::Listed(listing(&[""], None)), false),
            Some(Request::Steps {
                steps: vec![(
                    "SELECT n.item FROM (SELECT $1::jsonb AS v) AS board_vars, \
                     LATERAL (SELECT (to_jsonb(t))::text AS item, t.at AS place0, t.id AS place1 \
                     FROM t WHERE t.at > (v ->> 'since')::bigint) AS n \
                     ORDER BY n.place0, n.place1"
                        .into(),
                    true
                )],
                answer: Gives::Lists {
                    lists: vec![("", 0)],
                    rest: None
                }
            })
        );
    }

    #[test]
    fn a_list_orders_by_each_of_its_values_in_turn_ascending_or_descending() {
        let mut list = listing(&[""], None).lists.remove(0);
        list.order = "t.at DESC, t.id".into();
        let rows = list.rows("", BOUND);
        assert!(
            rows.contains("AS item, t.at AS place0, t.id AS place1 FROM t "),
            "{rows}"
        );
        assert!(
            rows.ends_with(" AS n ORDER BY n.place0 DESC, n.place1"),
            "{rows}"
        );
        list.order = "t.at".into();
        let rows = list.rows("", BOUND);
        assert!(rows.ends_with(", t.at AS place0 FROM t WHERE t.at > (v ->> 'since')::bigint) AS n ORDER BY n.place0"), "{rows}");
    }

    #[test]
    fn a_read_of_lists_sets_its_variables_once_and_answers_a_statement_a_list_then_the_rest() {
        let mut read = listing(&["tasks", "roles"], Some("'latest', 7"));
        read.with = "picked AS (SELECT 1)".into();
        for guarded in [false, true] {
            let Some(Request::Steps { steps, answer }) =
                request(&Pg::Listed(read.clone()), guarded)
            else {
                panic!("a read of lists is steps");
            };
            let before = 1 + usize::from(guarded);
            assert_eq!(steps.len(), before + 3, "{guarded}");
            assert_eq!(binding(&steps), [0], "the variables are sent once");
            assert_eq!(steps[0].0, VARS);
            assert_eq!(steps[before - 1].0 == GUARD, guarded);
            for list in &steps[before..before + 2] {
                assert!(
                    list.0.starts_with(&format!(
                        "SELECT n.item FROM (SELECT {SET} AS v) AS board_vars, \
                         LATERAL (WITH picked AS (SELECT 1) SELECT (to_jsonb(t))::text AS item,"
                    )),
                    "{}",
                    list.0
                );
                assert!(
                    list.0.ends_with(" AS n ORDER BY n.place0, n.place1"),
                    "{}",
                    list.0
                );
            }
            assert_eq!(
                steps[before + 2].0,
                answered(
                    "(WITH picked AS (SELECT 1) SELECT jsonb_build_object('latest', 7))",
                    SET
                )
            );
            assert_eq!(
                answer,
                Gives::Lists {
                    lists: vec![("tasks", before), ("roles", before + 1)],
                    rest: Some(before + 2)
                }
            );
        }
    }

    #[test]
    fn a_read_of_lists_as_one_value_is_its_object_with_each_list_whole() {
        let mut read = listing(&["tasks"], Some("'latest', 7"));
        let tasks = "'tasks', coalesce((SELECT jsonb_agg(to_jsonb(t) ORDER BY t.at, t.id) \
                     FROM t WHERE t.at > (v ->> 'since')::bigint), '[]')";
        assert_eq!(
            read.whole(),
            format!("(SELECT jsonb_build_object({tasks}, 'latest', 7))")
        );
        read.with = "picked AS (SELECT 1)".into();
        read.rest = None;
        assert_eq!(
            read.whole(),
            format!("(WITH picked AS (SELECT 1) SELECT jsonb_build_object({tasks}))")
        );
    }

    #[test]
    fn an_answer_in_lists_joins_to_the_text_of_its_one_value() {
        let parsed = |text: String| -> serde_json::Value {
            serde_json::from_str(&text).unwrap_or_else(|why| panic!("{why}: {text}"))
        };
        let tasks = vec![r#"{"slug": "a"}"#, r#"{"slug": "b, c"}"#];
        assert_eq!(
            parsed(joined(
                &[("tasks", tasks.clone()), ("roles", vec![])],
                Some(r#"{"known": true, "latest": 7}"#)
            )),
            serde_json::json!({
                "tasks": [{"slug": "a"}, {"slug": "b, c"}],
                "roles": [],
                "known": true,
                "latest": 7
            })
        );
        assert_eq!(
            parsed(joined(&[("", tasks)], None)),
            serde_json::json!([{"slug": "a"}, {"slug": "b, c"}])
        );
        assert_eq!(joined(&[("", vec![])], None), "[]");
        assert_eq!(joined(&[], None), "[]");
        assert_eq!(
            joined(&[("tasks", vec!["1"])], Some(r#"{"a": []}"#)),
            r#"{"tasks":[1],"a": []}"#
        );
        // An object of no other fields, and one of no lists, are still one object.
        assert_eq!(
            joined(&[("tasks", vec![]), ("roles", vec!["2"])], Some("{}")),
            r#"{"tasks":[],"roles":[2]}"#
        );
        assert_eq!(joined(&[], Some(r#"{"a": 1}"#)), r#"{"a": 1}"#);
        assert_eq!(joined(&[], Some("{}")), "{}");
    }

    /// `rule`'s block, which a rule's write is.
    fn ruled(reason: bool, needs: bool, blocked: bool, moved: Option<bool>, holds: bool) -> String {
        match rule(reason, (needs, needs), needs, blocked, moved, holds, false) {
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
            work::field_set("_board,gates", "v -> 'gates'"),
            work::field_set("_board,unlanded", work::UNLANDED),
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
