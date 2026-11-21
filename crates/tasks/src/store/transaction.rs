//! Transaction execution: the schema guard a write starts with, answer decoding and durable
//! post-commit dispatch.
//!
//! The one place the store sends anything to its database: every statement the store's modules
//! name ([`super::surreal`], and its twin in [`super::postgres`], [`super::statement`]) goes out
//! through [`Store::run_transaction`], [`Store::read`], [`Store::bounded_read`] or, for the open's
//! own and a writer's line, [`Store::request`] and [`Store::define`], to the database the board is
//! in ([`Database`]). On PostgreSQL a statement with no twin yet is refused here, before anything
//! is sent, naming the line that sent it.

use super::database::{Board, Postgres, Reach};
use super::postgres::{self, Pg};
use super::statement::Statement;
use super::surreal as sql;
use super::{Error, Failure, REFUSED, Store};
use crate::pg;
use crate::rpc::DbError;
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
    /// `body`'s statements in one transaction; the last of them is a `RETURN` of the answer. A null
    /// among `vars` is left out, so it reads as NONE: SurrealDB's NULL is a value an `option` field
    /// refuses, and that is never equal to NONE.
    #[track_caller]
    pub(super) fn transact<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        self.transact_dispatch(body, vars, true)
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

    /// A task write returns recipient presence in its snapshot and dispatches after decoding it.
    /// Other writes keep the normal post-commit dispatch path.
    #[track_caller]
    pub(super) fn transact_dispatch<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
        dispatch: bool,
    ) -> Result<T, Failure> {
        let body = body.into();
        let run = if schema::mutates(&body.surreal) {
            Run::Guarded { dispatch }
        } else {
            Run::Unguarded
        };
        self.run_transaction(body, vars, run)
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
            if dispatch && sql::posts(&body.surreal) {
                self.after_write();
            }
        };
        match self.db.reach() {
            Reach::SurrealDb(client) => {
                let results = client.query(&sql::transaction(guard, &body.surreal), vars)?;
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
    /// least the highest step whose table its statement, as it is sent, names on either engine
    /// ([`schema::needs`]). Whatever a word misses the database refuses itself, before anything
    /// of the transaction is written: an undefined table on a STRICT board, an undefined field.
    fn steps_taken(&self, body: &Statement, run: Run) -> schema::Steps {
        let board = &self.refusal.board;
        match run {
            Run::Staging | Run::Own => schema::Steps::exactly(self.additions, board, self.schema),
            Run::Guarded { .. } | Run::Unguarded => {
                let texts = std::iter::once(body.surreal.as_ref()).chain(body.postgres_text());
                let needed = texts
                    .filter_map(|text| schema::needs(text, &self.added))
                    .max_by_key(|(_, step)| *step);
                match needed {
                    Some((table, step)) => schema::Steps::naming(table, step, board, self.schema),
                    None => schema::Steps::any(),
                }
            }
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
            Reach::SurrealDb(client) => client.query(&sql.surreal, vars),
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
/// answer, null for a statement that sends nothing or answers nothing.
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
        Reach::SurrealDb(client) => client.query(&body.surreal, vars)?.pop(),
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
            "SurrealDB: an answer not in the board's shape: missing field `at`"
        );
        assert_eq!(
            says(json!({"name": "a", "at": 1, "since": 0})),
            "SurrealDB: an answer not in the board's shape: unknown field `since`, expected one \
             of `name`, `at`, `until`"
        );
        assert_eq!(
            says(json!({"name": 1, "at": 1})),
            "SurrealDB: an answer not in the board's shape: invalid type: number, expected a string"
        );
        // A struct's fields in an array's order are no answer the board's database gives.
        assert_eq!(
            says(json!(["a", 1])),
            "SurrealDB: an answer not in the board's shape: invalid type: sequence, expected \
             struct Answer"
        );
    }
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod guard_tests;
