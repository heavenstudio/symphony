use super::{Error, Ledger, Metadata, Scope, Snapshot};
use symphony_board::Store;

/// Optional artifact metadata in the board's durable store, on either database. Constructing this
/// adapter does not enable hosted artifacts or alter legacy upload reads/writes.
pub struct BoardMetadata<'a>(pub &'a Store);

impl Metadata for BoardMetadata<'_> {
    fn load(&self, scope: &Scope) -> Result<Snapshot, Error> {
        let (tenant, project) = scope.parts();
        let row = self
            .0
            .artifact_ledger(tenant, project)
            .map_err(|_error| Error::Metadata)?;
        let Some(row) = row else {
            return Ok(Snapshot {
                revision: 0,
                ledger: Ledger::default(),
            });
        };
        let revision = row
            .get("revision")
            .and_then(serde_json::Value::as_u64)
            .filter(|v| *v > 0)
            .ok_or(Error::Metadata)?;
        let ledger = serde_json::from_value(row.get("body").cloned().ok_or(Error::Metadata)?)
            .map_err(|_error| Error::Metadata)?;
        Ok(Snapshot { revision, ledger })
    }

    fn compare_exchange(&self, scope: &Scope, expected: u64, next: &Ledger) -> Result<bool, Error> {
        let (tenant, project) = scope.parts();
        let body = serde_json::to_value(next).map_err(|_error| Error::Metadata)?;
        self.0
            .artifact_ledger_cas(tenant, project, expected, &body)
            .map_err(|_error| Error::Metadata)
    }
}
