//! Optional artifact metadata's scoped CAS aggregate. These calls admit no task result, attempt,
//! approval or restore. The artifact core owns the versioned private body; board schema guards and
//! database transactions own durable compare-and-set. No route or local upload selects this table.

use super::postgres::artifacts as pg;
use super::statement::Statement;
use super::{Error, Store};
use serde_json::{Value, json};

impl Store {
    /// Private artifact ledger `{revision, body}` for stable trusted tenant/project identities.
    /// Absence is revision zero. Names, paths and aliases are not authorization.
    pub fn artifact_ledger(&self, tenant: &str, project: &str) -> Result<Option<Value>, Error> {
        check(tenant, project)?;
        let row: Option<Value> = self.retrying(|| {
            self.read(
                Statement::from(pg::load()),
                json!({"key": [tenant, project]}),
            )
        })?;
        row.map(|mut row| {
            let encoded = row["body"]["encoded"]
                .as_str()
                .ok_or_else(|| Error::Db("invalid artifact ledger envelope".into()))?;
            row["body"] = serde_json::from_str(encoded)
                .map_err(|error| Error::Db(format!("invalid artifact ledger JSON: {error}")))?;
            Ok(row)
        })
        .transpose()
    }

    /// Persist a private versioned artifact ledger only at `expected`, incrementing once. A
    /// false result is a competing committed revision. Unknown commit outcomes must be reloaded;
    /// no consumer may infer task success or reset operation tombstones from this CAS.
    pub fn artifact_ledger_cas(
        &self,
        tenant: &str,
        project: &str,
        expected: u64,
        body: &Value,
    ) -> Result<bool, Error> {
        check(tenant, project)?;
        if expected >= i64::MAX as u64 || !body.is_object() {
            return Err(Error::Refused(
                "invalid artifact ledger revision or body".into(),
            ));
        }
        self.retrying(|| self.transact_dispatch(Statement::from(pg::cas()),
            // Preserve nested nulls through the board's optional-field normalization and both
            // backends' projections. The opaque envelope is never inspected by a SQL statement.
            json!({"key": [tenant, project], "expected": expected, "body": {"encoded": body.to_string()}}), false))
    }
}

fn check(tenant: &str, project: &str) -> Result<(), Error> {
    if [tenant, project]
        .into_iter()
        .any(|v| v.is_empty() || v.len() > 512 || v.chars().any(char::is_control))
    {
        return Err(Error::Refused("invalid artifact ledger scope".into()));
    }
    Ok(())
}
