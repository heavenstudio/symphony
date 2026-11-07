//! Transaction execution, answer decoding and durable post-commit dispatch.

use super::{Error, Failure, Store};
use serde::de::DeserializeOwned;
use serde_json::Value;

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
    pub(super) fn read<T: DeserializeOwned>(
        &self,
        body: &str,
        mut vars: Value,
    ) -> Result<T, Failure> {
        unset_nulls(&mut vars);
        let answer = self.client.query(body, vars)?.pop();
        let answer =
            answer.ok_or_else(|| Failure::Stop(Error::Db("a read gave no answer".into())))?;
        serde_json::from_value(answer).map_err(|e| {
            Failure::Stop(Error::Db(format!(
                "an answer not in the board's shape: {e}"
            )))
        })
    }

    /// A task write returns recipient presence in its snapshot and dispatches after decoding it.
    /// Other writes keep the normal post-commit dispatch path.
    #[track_caller]
    pub(super) fn transact_dispatch<T: DeserializeOwned>(
        &self,
        body: &str,
        mut vars: Value,
        dispatch: bool,
    ) -> Result<T, Failure> {
        unset_nulls(&mut vars);
        let results = self.client.query(
            &format!("BEGIN TRANSACTION;\n{body}\nCOMMIT TRANSACTION;"),
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
