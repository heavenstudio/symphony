//! Transaction execution: the schema guard a write starts with, answer decoding and durable
//! post-commit dispatch.
//!
//! The one place the store sends anything to its database: every statement the store's modules
//! name ([`super::surreal`], and its twin in [`super::postgres`], [`super::statement`]) goes out
//! through [`Store::run_transaction`], [`Store::read`], [`Store::bounded_read`] or, for the open's
//! own and a writer's line, [`Store::request`] and [`Store::define`], to the database the board is
//! in ([`Database`]). On PostgreSQL a statement with no twin yet is refused here, before anything
//! is sent, naming the line that sent it, and on SurrealDB one written for PostgreSQL alone.
//!
//! What a transaction does is what its call and its statement say, never what either text reads
//! as: [`Store::transact`] sends a write behind the guard, [`Store::read_together`] a read, and
//! the statement says whether it posts a line, which dispatches its callbacks, and which tables of
//! an additive step it names ([`Statement`]). A debug build refuses, before anything is sent, a
//! statement whose SurrealQL does other than it says ([`Statement::disagreement`]).

use super::database::{Board, Postgres, Reach};
use super::postgres::{self, Pg};
use super::statement::Statement;
use super::surreal as sql;
use super::{Error, Failure, REFUSED, Store};
use crate::db::DbError;
use crate::pg;
use crate::schema;
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::panic::Location;

/// How a transaction runs: a write behind the schema guard, its committed messages dispatched to
/// their callbacks when `dispatch`; staging preserves raw row values without dispatch; a write held
/// to the store's own schema whoever opened it, with no dispatch (`Own`: a recovery rebind and a
/// whole-board copy's swap, which a reader's store never makes on a board one schema ahead); or
/// with no guard, as a read, or the migration that checks the version itself. Staging, a
/// whole-board copy's, is held to the store's own schema too.
#[derive(Clone, Copy)]
pub(super) enum Run {
    Guarded { dispatch: bool },
    Staging,
    Own,
    Unguarded,
}

impl Store {
    /// `body`'s statements in one write, behind the schema's guard, its committed line dispatched to
    /// its callbacks when it posts one; the last of them is a `RETURN` of the answer. A null among
    /// `vars` is left out, so it reads as NONE: SurrealDB's NULL is a value an `option` field
    /// refuses, and that is never equal to NONE.
    #[track_caller]
    pub(super) fn transact<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        self.transact_dispatch(body, vars, true)
    }

    /// `body`'s statements, which read and write nothing, in one transaction behind no guard, so
    /// all of them read one snapshot of the board; the last of them is a `RETURN` of the answer,
    /// and `vars` are read as [`Store::transact`] reads them.
    #[track_caller]
    pub(super) fn read_together<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        let body = body.into();
        said_as(&body, false, &self.added)?;
        self.run_transaction(body, vars, Run::Unguarded)
    }

    /// `body`, one statement that reads and writes nothing, answered outside an explicit
    /// transaction: a statement reads one snapshot of the board on its own, and a transaction's
    /// commit syncs the store even when nothing was written (about 5 ms on the board's server, paid
    /// for each recipient of each write that reads this way).
    #[track_caller]
    pub(super) fn read<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        decoded(read_on(&self.db, &body.into(), vars)?)
    }

    /// [`Store::read`] through the unanswered-post lookup's client when one is set, which waits a
    /// bounded time and sends a read that ran it out no second time
    /// ([`Store::looking_up_within`]).
    #[track_caller]
    pub(super) fn bounded_read<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        decoded(read_on(
            self.lookup.as_ref().unwrap_or(&self.db),
            &body.into(),
            vars,
        )?)
    }

    /// [`Store::transact`], its line dispatched after it commits only when `dispatch`: a task
    /// write returns recipient presence in its snapshot and dispatches after decoding it.
    #[track_caller]
    pub(super) fn transact_dispatch<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
        dispatch: bool,
    ) -> Result<T, Failure> {
        let body = body.into();
        said_as(&body, true, &self.added)?;
        self.run_transaction(body, vars, Run::Guarded { dispatch })
    }

    /// `body` in one transaction, as `run` says: a write starts with the schema guard, so on a
    /// board at another version than this store's it is refused before any statement of its own,
    /// but for a reader's ordinary write on a board one ahead whose record lets it
    /// ([`schema::GUARD`]).
    #[track_caller]
    pub(super) fn run_transaction<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
        run: Run,
    ) -> Result<T, Failure> {
        decoded(self.run_statement(&body.into(), vars, run)?)
    }

    /// [`Store::run_transaction`]'s work, its answer as the database gave it: one body for every
    /// statement and answer, so no caller's types make a copy of it.
    #[track_caller]
    fn run_statement(&self, body: &Statement, mut vars: Value, run: Run) -> Result<Value, Failure> {
        if !matches!(run, Run::Staging) {
            unset_nulls(&mut vars);
        }
        let (guard, dispatch) = match run {
            Run::Guarded { .. } | Run::Staging | Run::Own => {
                // Only a reader's ordinary write takes a board one schema ahead that lets it: a
                // whole-board copy (its staging and its swap) and a rebind are held to the store's
                // own schema, a reader's too.
                let ahead = self.reader && matches!(run, Run::Guarded { .. });
                schema::guard_vars(
                    &mut vars,
                    self.schema,
                    ahead,
                    &self.refusal,
                    &self.steps_taken(body, run),
                    REFUSED,
                );
                (
                    schema::GUARD,
                    matches!(run, Run::Guarded { dispatch: true }),
                )
            }
            Run::Unguarded => ("", false),
        };
        // Once the write has committed, before its answer is read: delivery failure must never turn
        // it into a failed write that a caller might repeat. The shared server also drains this
        // durable outbox after a crash.
        let committed = || {
            if dispatch && body.posts {
                self.after_write();
            }
        };
        match self.db.reach() {
            Reach::SurrealDb(client) => {
                let surreal = body.surreal_at(Location::caller()).map_err(Failure::Stop)?;
                let results = client.query(&sql::transaction(guard, surreal), vars)?;
                committed();
                let answer = results
                    .len()
                    .checked_sub(2)
                    .and_then(|last| results.into_iter().nth(last));
                answer
                    .ok_or_else(|| Failure::Stop(Error::Db("a transaction gave no answer".into())))
            }
            Reach::PostgreSql(client) => {
                let twin = body
                    .postgres_at(Location::caller())
                    .map_err(Failure::Stop)?;
                let answer = on_postgres(client, twin, &vars, !guard.is_empty())?;
                committed();
                Ok(answer)
            }
        }
    }
}

impl Store {
    /// What a write run as `run` takes of the board's additive steps ([`schema::Steps`]): a
    /// whole-board copy, its staging and a rebind exactly the store's own; an ordinary write at
    /// least the highest step whose table its statement says it names ([`Statement::names`]), the
    /// first of this store's steps of that number. Whatever it does not say the database refuses
    /// itself, before anything of the transaction is written: an undefined table on a STRICT
    /// board, an undefined field.
    fn steps_taken(&self, body: &Statement, run: Run) -> schema::Steps {
        let board = &self.refusal.board;
        match run {
            Run::Staging | Run::Own => schema::Steps::exactly(self.additions, board, self.schema),
            Run::Guarded { .. } | Run::Unguarded => {
                let named =
                    (self.added.iter()).filter(|(table, _)| body.names.contains(&table.as_str()));
                let needed = named.fold(None, |most: Option<&(String, i64)>, added| match most {
                    Some(most) if most.1 >= added.1 => Some(most),
                    _ => Some(added),
                });
                match needed.map(|(table, step)| (table.as_str(), *step)) {
                    Some((table, step)) => schema::Steps::naming(table, step, board, self.schema),
                    None => schema::Steps::any(),
                }
            }
        }
    }
}

/// `body`, sent as a write when `writes` by a store whose additive steps define `added`, refused
/// before anything is sent where what it says it does disagrees with what its SurrealQL does
/// ([`Statement::disagreement`]), naming the line it was sent from: in a debug build alone, every
/// test's, as a release build sends what a statement says and reads no SurrealQL for it.
#[track_caller]
fn said_as(body: &Statement, writes: bool, added: &[(String, i64)]) -> Result<(), Failure> {
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    match body.disagreement(writes, added) {
        None => Ok(()),
        Some(why) => {
            let at = Location::caller();
            Err(Failure::Stop(Error::Db(format!(
                "the statement sent from {}:{} {why}: nothing was sent",
                at.file(),
                at.line()
            ))))
        }
    }
}

/// `answer` as the caller's type, each struct of it read from an object ([`crate::objects`]).
fn decoded<T: DeserializeOwned>(answer: Value) -> Result<T, Failure> {
    crate::objects::read(&answer).map_err(|e| {
        Failure::Stop(Error::Db(format!(
            "an answer not in the board's shape: {e}"
        )))
    })
}

impl Store {
    /// `sql`'s statements as they are, in no transaction of the store's and behind no guard, each
    /// statement's answer in order: the open's reads of a board that may not be made yet, the
    /// analyzers an index needs committed first, and a writer's line, which builds of other
    /// schemas write too ([`super::writers`]).
    ///
    /// On PostgreSQL, the one answer of its statements, which run in one transaction of their own.
    #[track_caller]
    pub(super) fn request<'a>(
        &self,
        sql: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<Vec<Value>, DbError> {
        self.request_statement(&sql.into(), vars)
    }

    #[track_caller]
    fn request_statement(&self, sql: &Statement, vars: Value) -> Result<Vec<Value>, DbError> {
        match self.db.reach() {
            Reach::SurrealDb(client) => {
                let surreal = sql
                    .surreal_at(Location::caller())
                    .map_err(|refused| DbError::Failed(refused.to_string()))?;
                client.query(surreal, vars)
            }
            Reach::PostgreSql(client) => {
                let twin = sql
                    .postgres_at(Location::caller())
                    .map_err(|refused| DbError::Failed(refused.to_string()))?;
                Ok(vec![on_postgres(client, twin, &vars, false)?])
            }
        }
    }

    /// The board's namespace and database, made if they are not there yet.
    #[track_caller]
    pub(super) fn define(&self) -> Result<(), DbError> {
        self.db.define(sql::NOTHING)
    }
}

/// `statement` on PostgreSQL, behind the schema's guard when `guarded`, `vars` its variables: its
/// answer, null for a statement that sends nothing or answers nothing. The request is the line's
/// that sent the statement, as the board's health records it ([`crate::board_health::Query`]),
/// never this one's.
#[track_caller]
pub(super) fn on_postgres(
    client: Postgres,
    statement: &Pg,
    vars: &Value,
    guarded: bool,
) -> Result<Value, DbError> {
    let Some(request) = postgres::request(statement, guarded) else {
        return Ok(Value::Null);
    };
    let (steps, answer) = match request {
        postgres::Request::Simple(sql) => {
            client.simple(&sql)?;
            return Ok(Value::Null);
        }
        postgres::Request::Steps { steps, answer } => (steps, answer),
    };
    let vars = vars.to_string();
    let steps: Vec<pg::Step> = (steps.iter())
        .map(|(sql, takes)| {
            if *takes {
                pg::Step::with(sql, &vars)
            } else {
                pg::Step::bare(sql)
            }
        })
        .collect();
    let answers = client.request(&steps)?;
    let text: std::borrow::Cow<str> = match &answer {
        postgres::Gives::Value(at) => match answers.get(*at).and_then(pg::Answer::value) {
            Some(text) => text.into(),
            None => return Ok(Value::Null),
        },
        postgres::Gives::Lists { lists, rest } => {
            let column = |at: &usize| answers.get(*at).map(pg::Answer::column);
            let lists: Vec<(&str, Vec<&str>)> = (lists.iter())
                .map(|(key, at)| (*key, column(at).unwrap_or_default()))
                .collect();
            let rest = match rest {
                Some(at) => Some(answers.get(*at).and_then(pg::Answer::value).ok_or_else(
                    || {
                        DbError::Failed(
                            "PostgreSQL answered a read's lists without its fields".into(),
                        )
                    },
                )?),
                None => None,
            };
            postgres::joined(&lists, rest).into()
        }
    };
    serde_json::from_str(&text).map_err(|e| {
        DbError::Failed(format!(
            "PostgreSQL answered what is not JSON ({e}): {text}"
        ))
    })
}

/// `body`, one read, answered by the board in `db` ([`Store::read`]).
#[track_caller]
fn read_on(db: &Board, body: &Statement, mut vars: Value) -> Result<Value, Failure> {
    unset_nulls(&mut vars);
    let answer = match db.reach() {
        Reach::SurrealDb(client) => {
            let surreal = body.surreal_at(Location::caller()).map_err(Failure::Stop)?;
            client.query(surreal, vars)?.pop()
        }
        Reach::PostgreSql(client) => {
            let twin = body
                .postgres_at(Location::caller())
                .map_err(Failure::Stop)?;
            Some(on_postgres(client, twin, &vars, false)?)
        }
    };
    answer.ok_or_else(|| Failure::Stop(Error::Db("a read gave no answer".into())))
}

fn unset_nulls(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.retain(|_, field| !field.is_null());
            fields.values_mut().for_each(unset_nulls);
        }
        Value::Array(items) => items.iter_mut().for_each(unset_nulls),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::execution::Commit;
    use serde::Deserialize;
    use serde_json::json;

    #[derive(Debug, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Answer {
        name: String,
        at: f64,
        #[serde(default)]
        until: Option<f64>,
    }

    /// What `answer` decodes to as `T`, or the words of its refusal.
    fn decode<T: DeserializeOwned>(answer: Value) -> Result<T, String> {
        decoded(answer).map_err(|failure| match failure {
            Failure::Stop(error) => error.to_string(),
            Failure::Again => "again".into(),
        })
    }

    /// A request on PostgreSQL is recorded at the line that sent its statement, through the
    /// store's read and the one sender beneath it ([`on_postgres`]), as the board's health names
    /// an hour's slowest; one that got no answer is recorded all the same.
    #[test]
    fn a_postgresql_request_is_recorded_at_the_line_that_sent_its_statement() {
        let socket = std::env::temp_dir().join(format!("sent-from-{}", std::process::id()));
        // Nothing listens there: the request fails at its connection, and is counted.
        let client = pg::Client::new(&socket, crate::DATABASE);
        let log = client.queries();
        let store = Store::unchecked_on(
            client.into(),
            Box::new(crate::store::now),
            Box::new(|_| true),
            "",
            &crate::Opening::write(),
        );
        let statement = super::super::statement::both("RETURN 1;", Pg::read("1"));
        let sent = line!() + 1;
        let read = store.read::<Value>(statement, json!({}));
        assert!(read.is_err(), "no server answers");
        let (report, _) = log.report(crate::sweep::trigger::now());
        assert_eq!(report.count, 1);
        assert_eq!(
            report.slowest.map(|slowest| slowest.source),
            Some(format!("{}:{sent}", file!()))
        );
    }

    #[test]
    fn an_answer_reads_its_structs_from_objects_as_from_value_reads_them() {
        let read: Answer = decode(json!({"name": "a", "at": 1.50})).unwrap();
        assert_eq!(
            read,
            Answer {
                name: "a".into(),
                at: 1.5,
                until: None,
            }
        );
        let rows: Vec<Answer> = decode(json!([{"name": "b", "at": 2, "until": 3.25}])).unwrap();
        assert_eq!(rows[0].until, Some(3.25));
        let missing: Option<Answer> = decode(Value::Null).unwrap();
        assert_eq!(missing, None);
        let committed: Commit =
            decode(json!({"outcome": "committed", "revision": 4, "now": 9})).unwrap();
        assert_eq!(
            committed,
            Commit::Committed {
                revision: 4,
                now: 9,
            }
        );
    }

    #[test]
    fn an_answer_not_in_the_boards_shape_is_refused_in_the_words_it_was() {
        let says = |answer: Value| decode::<Answer>(answer).unwrap_err();
        assert_eq!(
            says(json!({"name": "a"})),
            "the board's database: an answer not in the board's shape: missing field `at`"
        );
        assert_eq!(
            says(json!({"name": "a", "at": 1, "since": 0})),
            "the board's database: an answer not in the board's shape: unknown field `since`, expected one \
             of `name`, `at`, `until`"
        );
        assert_eq!(
            says(json!({"name": 1, "at": 1})),
            "the board's database: an answer not in the board's shape: invalid type: number, expected a string"
        );
        // A struct's fields in an array's order are no answer the board's database gives.
        assert_eq!(
            says(json!(["a", 1])),
            "the board's database: an answer not in the board's shape: invalid type: sequence, expected \
             struct Answer"
        );
    }

    /// A debug build refuses, on either database, a statement whose SurrealQL does other than its
    /// call and it say, naming the line it was sent from, before anything of it is sent: none of
    /// the refused ones is a statement either database could parse. The same read sent as a read
    /// is answered.
    #[test]
    #[cfg(debug_assertions)]
    fn a_statement_saying_other_than_its_surrealql_does_is_refused_before_it_is_sent() {
        use crate::store::statement::both;
        use crate::store_fixture::{Backend, Fixture, Seed};
        let read = || both("RETURN 1;", Pg::read("to_jsonb(1)"));
        let unparsed = "and this is no statement";
        let write = || {
            both(
                format!("UPSERT counter:walk SET last = 1; {unparsed}"),
                Pg::script(unparsed),
            )
        };
        let line = sql::logged("'sim/walk'", "'board'", "'msg'", "'said'", "NONE");
        let posting = || both(format!("{line} {unparsed}"), Pg::script(unparsed));
        for backend in [Backend::SurrealDb, Backend::PostgreSql] {
            let f = Fixture::on(backend, Seed::default());
            // Each send on a line of its own, the line its refusal names.
            let store = &f.store;
            let refused = |sent: Result<Value, Failure>, at: u32, why: &str| match sent {
                Err(Failure::Stop(error)) => {
                    let said = error.to_string();
                    let expected = format!("{}:{at} {why}: nothing was sent", file!());
                    assert!(said.contains(&expected), "{backend:?}: {said}");
                }
                other => panic!("{backend:?}: {other:?}"),
            };
            let at = line!() + 1;
            let sent = store.transact::<Value>(read(), json!({}));
            refused(
                sent,
                at,
                "is sent as a write, and its SurrealQL changes nothing",
            );
            let at = line!() + 1;
            let sent = store.read_together::<Value>(write(), json!({}));
            refused(sent, at, "is sent as a read, and its SurrealQL writes");
            let at = line!() + 1;
            let sent = store.transact_dispatch::<Value>(posting(), json!({}), false);
            refused(
                sent,
                at,
                "says it posts no line, and its SurrealQL posts one",
            );
            let answer: i64 = store.read_together(read(), json!({})).unwrap();
            assert_eq!(answer, 1, "{backend:?}");
        }
    }
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod guard_tests;
