//! Transaction execution: the schema guard a write starts with, answer decoding and durable
//! post-commit dispatch.

use super::{Error, Failure, REFUSED, Store};
use crate::schema;
use serde::de::DeserializeOwned;
use serde_json::Value;

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
    pub(super) fn transact<T: DeserializeOwned>(
        &self,
        body: &str,
        vars: Value,
    ) -> Result<T, Failure> {
        self.transact_dispatch(body, vars, true)
    }

    /// `body`, one statement that reads and writes nothing, answered outside an explicit
    /// transaction: a statement reads one snapshot of the board on its own, and a transaction's
    /// commit syncs the store even when nothing was written (about 5 ms on the board's server, paid
    /// for each recipient of each write that reads this way).
    #[track_caller]
    pub(super) fn read<T: DeserializeOwned>(&self, body: &str, vars: Value) -> Result<T, Failure> {
        read_on(&self.client, body, vars)
    }

    /// [`Store::read`] through the unanswered-post lookup's client, which waits a bounded time
    /// and sends a read that ran it out no second time.
    #[track_caller]
    pub(super) fn bounded_read<T: DeserializeOwned>(
        &self,
        body: &str,
        vars: Value,
    ) -> Result<T, Failure> {
        read_on(&self.lookup, body, vars)
    }

    /// A task write returns recipient presence in its snapshot and dispatches after decoding it.
    /// Other writes keep the normal post-commit dispatch path.
    #[track_caller]
    pub(super) fn transact_dispatch<T: DeserializeOwned>(
        &self,
        body: &str,
        vars: Value,
        dispatch: bool,
    ) -> Result<T, Failure> {
        let run = if schema::mutates(body) {
            Run::Guarded { dispatch }
        } else {
            Run::Unguarded
        };
        self.run_transaction(body, vars, run)
    }

    /// `body` in one transaction, as `run` says: a write starts with the schema guard, so on a
    /// board at another version than this store's it is refused before any statement of its own.
    #[track_caller]
    pub(super) fn run_transaction<T: DeserializeOwned>(
        &self,
        body: &str,
        mut vars: Value,
        run: Run,
    ) -> Result<T, Failure> {
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
        let results = self.client.query(
            &format!("BEGIN TRANSACTION;\n{guard}\n{body}\nCOMMIT TRANSACTION;"),
            vars,
        )?;
        // The write has committed. Delivery failure must never turn it into a failed write that a
        // caller might repeat. The shared server also drains this durable outbox after a crash.
        if dispatch && body.contains("CREATE type::record('message',") {
            self.after_write();
        }
        let answer = results
            .len()
            .checked_sub(2)
            .and_then(|last| results.into_iter().nth(last));
        let answer = answer
            .ok_or_else(|| Failure::Stop(Error::Db("a transaction gave no answer".into())))?;
        serde_json::from_value(answer).map_err(|e| {
            Failure::Stop(Error::Db(format!(
                "an answer not in the board's shape: {e}"
            )))
        })
    }
}

/// `body`, one read, answered by `client` ([`Store::read`]).
#[track_caller]
fn read_on<T: DeserializeOwned>(
    client: &crate::rpc::Client,
    body: &str,
    mut vars: Value,
) -> Result<T, Failure> {
    unset_nulls(&mut vars);
    let answer = client.query(body, vars)?.pop();
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
