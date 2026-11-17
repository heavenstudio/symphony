//! Transaction execution: the schema guard a write starts with, answer decoding and durable
//! post-commit dispatch.
//!
//! The one place the store sends anything to its database: every statement the store's modules
//! name ([`super::surreal`], and its twin in [`super::postgres`], [`super::statement`]) goes out
//! through [`Store::run_transaction`], [`Store::read`], [`Store::bounded_read`] or, for the open's
//! own and a writer's line, [`Store::request`] and [`Store::define`], to the database the board is
//! in ([`Database`]). On PostgreSQL a statement with no twin yet is refused here, before anything
//! is sent, naming the line that sent it.

use super::database::Database;
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
/// their callbacks when `dispatch`; staging preserves raw row values without dispatch; or with no
/// guard, as a read, or the migration that checks the version itself.
#[derive(Clone, Copy)]
pub(super) enum Run {
    Guarded { dispatch: bool },
    Staging,
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
        read_on(&self.db, &body.into(), vars)
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
        read_on(self.lookup.as_ref().unwrap_or(&self.db), &body.into(), vars)
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
    /// board at another version than this store's it is refused before any statement of its own.
    #[track_caller]
    pub(super) fn run_transaction<'a, T: DeserializeOwned>(
        &self,
        body: impl Into<Statement<'a>>,
        mut vars: Value,
        run: Run,
    ) -> Result<T, Failure> {
        let body = body.into();
        if !matches!(run, Run::Staging) {
            unset_nulls(&mut vars);
        }
        let (guard, dispatch) = match run {
            Run::Guarded { .. } | Run::Staging => {
                schema::guard_vars(&mut vars, self.schema, &self.refusal, REFUSED);
                (
                    schema::GUARD,
                    matches!(run, Run::Guarded { dispatch: true }),
                )
            }
            Run::Unguarded => ("", false),
        };
        let answer = match &self.db {
            Database::SurrealDb(client) => {
                let results = client.query(&sql::transaction(guard, &body.surreal), vars)?;
                let answer = results
                    .len()
                    .checked_sub(2)
                    .and_then(|last| results.into_iter().nth(last));
                answer.ok_or_else(|| {
                    Failure::Stop(Error::Db("a transaction gave no answer".into()))
                })?
            }
            Database::PostgreSql(client) => {
                let twin = body
                    .postgres_at(Location::caller())
                    .map_err(Failure::Stop)?;
                on_postgres(client, twin, &vars, !guard.is_empty())?
            }
        };
        // The write has committed. Delivery failure must never turn it into a failed write that a
        // caller might repeat. The shared server also drains this durable outbox after a crash.
        if dispatch && sql::posts(&body.surreal) {
            self.after_write();
        }
        serde_json::from_value(answer).map_err(|e| {
            Failure::Stop(Error::Db(format!(
                "an answer not in the board's shape: {e}"
            )))
        })
    }
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
        let sql = sql.into();
        match &self.db {
            Database::SurrealDb(client) => client.query(&sql.surreal, vars),
            Database::PostgreSql(client) => {
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
    client: &pg::Client,
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
    let Some(text) = answers.get(answer).and_then(pg::Answer::value) else {
        return Ok(Value::Null);
    };
    serde_json::from_str(text).map_err(|e| {
        DbError::Failed(format!(
            "PostgreSQL answered what is not JSON ({e}): {text}"
        ))
    })
}

/// `body`, one read, answered by the board in `db` ([`Store::read`]).
#[track_caller]
fn read_on<T: DeserializeOwned>(
    db: &Database,
    body: &Statement,
    mut vars: Value,
) -> Result<T, Failure> {
    unset_nulls(&mut vars);
    let answer = match db {
        Database::SurrealDb(client) => client.query(&body.surreal, vars)?.pop(),
        Database::PostgreSql(client) => {
            let twin = body
                .postgres_at(Location::caller())
                .map_err(Failure::Stop)?;
            Some(on_postgres(client, twin, &vars, false)?)
        }
    };
    let answer = answer.ok_or_else(|| Failure::Stop(Error::Db("a read gave no answer".into())))?;
    serde_json::from_value(answer).map_err(|e| {
        Failure::Stop(Error::Db(format!(
            "an answer not in the board's shape: {e}"
        )))
    })
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
