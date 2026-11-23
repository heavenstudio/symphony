//! Transaction execution: the schema guard a write starts with, answer decoding and durable
//! post-commit dispatch.
//!
//! The one place the store sends anything to its database: every statement the store's modules
//! name ([`super::postgres`], [`super::statement`]) goes out through [`Store::run_transaction`],
//! [`Store::read`], [`Store::bounded_read`] or, for the open's own and a writer's line,
//! [`Store::request`] and [`Store::define`], to the database the board is in
//! ([`super::Database`]).
//!
//! What a transaction does is what its call and its statement say, never what its text reads as:
//! [`Store::transact`] sends a write behind the guard, [`Store::read_together`] a read, and the
//! statement says whether it posts a line, which dispatches its callbacks, and which tables of an
//! additive step it names ([`Statement`]). A debug build refuses, before anything is sent, a
//! statement whose text names other additive steps' tables than it says
//! ([`Statement::disagreement`]).

use super::database::{Board, Postgres};
use super::postgres::{self, Pg};
use super::statement::Statement;
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
    /// its callbacks when it posts one, its answer the statement's. A null among `vars` is left
    /// out, so a field it would set reads as absent.
    #[track_caller]
    pub(super) fn transact<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        self.transact_dispatch(body, vars, true)
    }

    /// `body`'s statements, which read and write nothing, in one transaction behind no guard, so
    /// all of them read one snapshot of the board, and `vars` are read as [`Store::transact`]
    /// reads them.
    #[track_caller]
    pub(super) fn read_together<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        vars: Value,
    ) -> Result<T, Failure> {
        let body = body.into();
        said_as(&body, &self.added)?;
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
        said_as(&body, &self.added)?;
        self.run_transaction(body, vars, Run::Guarded { dispatch })
    }

    /// `body` in one transaction, as `run` says: a write starts with the schema guard, so on a
    /// board at another version than this store's it is refused before any statement of its own,
    /// but for a reader's ordinary write on a board one ahead whose record lets it
    /// ([`postgres::GUARD`]).
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
        let (guarded, dispatch) = match run {
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
                (true, matches!(run, Run::Guarded { dispatch: true }))
            }
            Run::Unguarded => (false, false),
        };
        // Once the write has committed, before its answer is read: delivery failure must never turn
        // it into a failed write that a caller might repeat. The shared server also drains this
        // durable outbox after a crash.
        let committed = || {
            if dispatch && body.posts {
                self.after_write();
            }
        };
        let answer = on_postgres(self.db.reach(), &body.postgres, &vars, guarded)?;
        committed();
        Ok(answer)
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

/// `body`, sent by a store whose additive steps define `added`, refused before anything is sent
/// where the tables it says it names disagree with those its text names
/// ([`Statement::disagreement`]), naming the line it was sent from: in a debug build alone, every
/// test's, as a release build sends what a statement says and reads no text for it.
#[track_caller]
fn said_as(body: &Statement, added: &[(String, i64)]) -> Result<(), Failure> {
    if !cfg!(debug_assertions) {
        return Ok(());
    }
    match body.disagreement(added) {
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
    /// `sql`'s statements as they are, behind no guard, in one transaction of their own, and their
    /// one answer: the open's reads of a board that may not be made yet, and a writer's line,
    /// which builds of other schemas write too ([`super::writers`]).
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
        Ok(vec![on_postgres(
            self.db.reach(),
            &sql.postgres,
            &vars,
            false,
        )?])
    }

    /// The board's database, made if it is not there yet.
    #[track_caller]
    pub(super) fn define(&self) -> Result<(), DbError> {
        self.db.define()
    }
}

/// `statement` on PostgreSQL, behind the schema's guard when `guarded`, `vars` its variables: its
/// answer, null for a statement that answers nothing.
pub(super) fn on_postgres(
    client: Postgres,
    statement: &Pg,
    vars: &Value,
    guarded: bool,
) -> Result<Value, DbError> {
    let (steps, answer) = match postgres::request(statement, guarded) {
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
    Ok(on_postgres(db.reach(), &body.postgres, &vars, false)?)
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
}

#[cfg(test)]
#[path = "transaction_tests.rs"]
mod guard_tests;
