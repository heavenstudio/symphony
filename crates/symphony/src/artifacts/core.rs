use super::model::{Record, digest, plain, random};
use super::{
    ArtifactId, Binding, Descriptor, Error, Failure, Ledger, Metadata, Operation, Permit, Producer,
    Provider, Publication, Scope, Status,
};
use crate::uploads;
use std::collections::BTreeSet;

/// Explicit, resumable operations over durable metadata and immutable bytes. No background work,
/// clocks, automatic retention policy or workflow transition is installed by constructing this.
pub struct Core<'a, M: ?Sized, P: ?Sized> {
    pub metadata: &'a M,
    pub provider: &'a P,
}

impl<M: Metadata + ?Sized, P: Provider + ?Sized> Core<'_, M, P> {
    fn change<T>(
        &self,
        scope: &Scope,
        mut apply: impl FnMut(&mut Ledger) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if !scope.valid() {
            return Err(Error::Invalid);
        }
        for _ in 0..32 {
            let mut snapshot = self.metadata.load(scope)?;
            if snapshot.ledger.format != 1 {
                return Err(Error::Metadata);
            }
            let answer = apply(&mut snapshot.ledger)?;
            if self
                .metadata
                .compare_exchange(scope, snapshot.revision, &snapshot.ledger)?
            {
                return Ok(answer);
            }
        }
        Err(Error::Busy)
    }

    /// Admit, replace or cancel an operation using its expected monotonic revision. The host must
    /// make this change with its authoritative cancellation/restore fence before admitting reads
    /// or results; this method does not implement a task-attempt lease or accept a result.
    pub fn fence(
        &self,
        scope: &Scope,
        owner: &str,
        expected: u64,
        binding: Option<Binding>,
    ) -> Result<Permit, Error> {
        if !plain(owner) || binding.as_ref().is_some_and(|b| !valid_binding(b)) {
            return Err(Error::Invalid);
        }
        self.change(scope, |ledger| {
            let previous = ledger.operations.get(owner).cloned().unwrap_or_default();
            if previous.revision != expected {
                return Err(Error::Stale);
            }
            let revision = expected.checked_add(1).ok_or(Error::Invalid)?;
            ledger.operations.insert(
                owner.into(),
                Operation {
                    revision,
                    binding: binding.clone(),
                },
            );
            Ok(Permit {
                owner: owner.into(),
                revision,
            })
        })
    }

    /// Save a durable pending intent before any provider call. Same retry, bytes, provenance and
    /// safe metadata converge; changing any of them conflicts. Upload validation is the existing
    /// attachment validator, including the size cap and active-content refusal.
    pub fn begin(
        &self,
        scope: &Scope,
        retry: &str,
        permit: &Permit,
        name: &str,
        bytes: &[u8],
    ) -> Result<ArtifactId, Error> {
        let kind = uploads::sniff(name, bytes).map_err(|_refusal| Error::Invalid)?;
        if !plain(retry) || !plain(self.provider.binding()) {
            return Err(Error::Invalid);
        }
        let name = uploads::display_name(name, kind);
        let sha256 = digest(bytes);
        let id = ArtifactId(random()?);
        self.change(scope, |ledger| {
            let binding = admitted(ledger, permit)?.clone();
            let proposed = Record {
                id: id.clone(),
                permit: permit.clone(),
                binding,
                sha256: sha256.clone(),
                bytes: bytes.len() as u64,
                media_type: kind.mime.into(),
                name: name.clone(),
                provider: self.provider.binding().into(),
                status: Status::Pending,
                intent_open: true,
                sealed: None,
                failure: None,
            };
            if let Some(existing) = ledger.retries.get(retry) {
                let record = ledger.records.get(existing).ok_or(Error::Metadata)?;
                if record.permit != proposed.permit
                    || record.binding != proposed.binding
                    || record.sha256 != proposed.sha256
                    || record.bytes != proposed.bytes
                    || record.media_type != proposed.media_type
                    || record.name != proposed.name
                    || record.provider != proposed.provider
                {
                    return Err(Error::Conflict);
                }
                return Ok(existing.clone());
            }
            if ledger.records.contains_key(&id) {
                return Err(Error::Conflict);
            }
            ledger.retries.insert(retry.into(), id.clone());
            ledger.records.insert(id.clone(), proposed);
            Ok(id.clone())
        })
    }

    /// Upload once and verify the exact acknowledged generation by reading and hashing it. A lost
    /// put acknowledgement leaves pending metadata; `reconcile` inspects the recorded identity.
    pub fn upload(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        bytes: &[u8],
    ) -> Result<Descriptor, Error> {
        let record = self.record(scope, id)?;
        if record.bytes != bytes.len() as u64 || record.sha256 != digest(bytes) {
            return Err(Error::Conflict);
        }
        if record.status == Status::Available {
            return self.verified(scope, &record);
        }
        if record.status != Status::Pending {
            return Err(Error::NotAvailable);
        }
        self.current(scope, &record.permit)?;
        let generation = self.provider.put(scope, id, bytes)?;
        self.finish(scope, &record, generation)
    }

    /// Recover an intent after worker/app shutdown or an uncertain provider/metadata outcome.
    /// Missing/unavailable/incomplete results remain pending and explicit; retries never overwrite.
    pub fn reconcile(&self, scope: &Scope, id: &ArtifactId) -> Result<Descriptor, Error> {
        let record = self.record(scope, id)?;
        if record.status == Status::Available {
            return self.verified(scope, &record);
        }
        if record.status != Status::Pending {
            return Err(Error::NotAvailable);
        }
        self.current(scope, &record.permit)?;
        let generation = self.provider.inspect(scope, id)?;
        self.finish(scope, &record, generation)
    }

    fn finish(
        &self,
        scope: &Scope,
        record: &Record,
        generation: super::Generation,
    ) -> Result<Descriptor, Error> {
        if !plain(generation.as_str()) {
            return Err(Error::Provider(Failure::Incomplete));
        }
        let sealed = Descriptor {
            sha256: record.sha256.clone(),
            bytes: record.bytes,
            media_type: record.media_type.clone(),
            name: record.name.clone(),
            provider: record.provider.clone(),
            generation,
        };
        self.bytes(scope, record, &sealed)?;
        self.change(scope, |ledger| {
            admitted(ledger, &record.permit)?;
            let current = ledger.records.get_mut(&record.id).ok_or(Error::Missing)?;
            if current.status == Status::Available && current.sealed.as_ref() == Some(&sealed) {
                return Ok(sealed.clone());
            }
            if current.status != Status::Pending {
                return Err(Error::NotAvailable);
            }
            current.sealed = Some(sealed.clone());
            current.status = Status::Available;
            Ok(sealed.clone())
        })
    }

    fn bytes(&self, scope: &Scope, record: &Record, sealed: &Descriptor) -> Result<Vec<u8>, Error> {
        let result = self
            .provider
            .read(scope, &record.id, &sealed.generation)
            .and_then(|bytes| {
                if bytes.len() as u64 != sealed.bytes || digest(&bytes) != sealed.sha256 {
                    Err(Failure::Integrity)
                } else {
                    Ok(bytes)
                }
            });
        match result {
            Ok(bytes) => Ok(bytes),
            Err(failure) => {
                // Uncertain availability is not proof of lost bytes. Integrity/missing failures
                // retain provenance and historical references while making publication impossible.
                if matches!(
                    failure,
                    Failure::Integrity | Failure::Missing | Failure::GenerationMismatch
                ) {
                    self.change(scope, |ledger| {
                        let current = ledger.records.get_mut(&record.id).ok_or(Error::Missing)?;
                        if matches!(current.status, Status::Pending | Status::Available) {
                            current.status = Status::Failed;
                            current.failure = Some(failure);
                        }
                        Ok(())
                    })?;
                }
                Err(failure.into())
            }
        }
    }

    fn verified(&self, scope: &Scope, record: &Record) -> Result<Descriptor, Error> {
        let sealed = record.sealed.as_ref().ok_or(Error::Metadata)?;
        self.bytes(scope, record, sealed)?;
        Ok(sealed.clone())
    }

    /// Read only sealed bytes, verifying the full object before returning any range. Hosted HTTP
    /// authorization, HEAD/304/cache and bounded revocation belong to the separate delivery layer.
    pub fn read(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        range: Option<std::ops::Range<usize>>,
    ) -> Result<Vec<u8>, Error> {
        let record = self.record(scope, id)?;
        if record.status != Status::Available {
            return Err(Error::NotAvailable);
        }
        let sealed = record.sealed.as_ref().ok_or(Error::Metadata)?;
        let bytes = self.bytes(scope, &record, sealed)?;
        match range {
            Some(range) => bytes.get(range).map(<[u8]>::to_vec).ok_or(Error::Invalid),
            None => Ok(bytes),
        }
    }

    pub fn status(&self, scope: &Scope, id: &ArtifactId) -> Result<Status, Error> {
        Ok(self.record(scope, id)?.status)
    }

    /// Append an immutable publication and its owned reference edges in one CAS. This records
    /// storage prerequisites only: a task result must be accepted by its authoritative transaction.
    pub fn publish(
        &self,
        scope: &Scope,
        key: &str,
        publication: &Publication,
    ) -> Result<(), Error> {
        if !plain(key)
            || !plain(&publication.owner)
            || publication.artifacts.is_empty()
            || publication
                .delivered
                .as_ref()
                .is_some_and(|revision| !revision.valid())
        {
            return Err(Error::Invalid);
        }
        self.change(scope, |ledger| {
            if let Some(previous) = ledger.publications.get(key) {
                return if previous == publication {
                    Ok(())
                } else {
                    Err(Error::Conflict)
                };
            }
            admitted(ledger, &publication.permit)?;
            for id in &publication.artifacts {
                let record = ledger.records.get(id).ok_or(Error::Missing)?;
                if record.status != Status::Available {
                    return Err(Error::NotAvailable);
                }
                if record.permit != publication.permit {
                    return Err(Error::Stale);
                }
            }
            for id in &publication.artifacts {
                ledger
                    .records
                    .get_mut(id)
                    .ok_or(Error::Missing)?
                    .intent_open = false;
            }
            ledger
                .references
                .entry(publication.owner.clone())
                .or_default()
                .extend(publication.artifacts.clone());
            ledger.publications.insert(key.into(), publication.clone());
            Ok(())
        })
    }

    /// Explicit authorized release; no worker termination or age policy calls this automatically.
    pub fn release_owner(&self, scope: &Scope, owner: &str) -> Result<(), Error> {
        self.change(scope, |ledger| {
            ledger.references.remove(owner);
            Ok(())
        })
    }

    /// Retained exports are separate roots, including exports whose producing worker disappeared.
    /// Existing roots are immutable; release one explicitly before reusing its identifier.
    pub fn retain_export(
        &self,
        scope: &Scope,
        export: &str,
        ids: &BTreeSet<ArtifactId>,
    ) -> Result<(), Error> {
        if !plain(export) || ids.is_empty() {
            return Err(Error::Invalid);
        }
        self.change(scope, |ledger| {
            if let Some(previous) = ledger.exports.get(export) {
                return if previous == ids {
                    Ok(())
                } else {
                    Err(Error::Conflict)
                };
            }
            for id in ids {
                if ledger
                    .records
                    .get(id)
                    .is_none_or(|r| r.status != Status::Available)
                {
                    return Err(Error::NotAvailable);
                }
            }
            ledger.exports.insert(export.into(), ids.clone());
            Ok(())
        })
    }

    pub fn release_export(&self, scope: &Scope, export: &str) -> Result<(), Error> {
        self.change(scope, |ledger| {
            ledger.exports.remove(export);
            Ok(())
        })
    }

    /// Only a trusted reference inventory covering retained history and restore roots can assert
    /// completeness. The default is unknown, which retains every generation.
    pub fn reference_inventory(&self, scope: &Scope, complete: bool) -> Result<(), Error> {
        self.change(scope, |ledger| {
            ledger.references_complete = complete;
            Ok(())
        })
    }

    pub fn release_intent(&self, scope: &Scope, id: &ArtifactId) -> Result<(), Error> {
        self.change(scope, |ledger| {
            ledger
                .records
                .get_mut(id)
                .ok_or(Error::Missing)?
                .intent_open = false;
            Ok(())
        })
    }

    /// Reserve an exact generation while atomically excluding every known reference and intent.
    /// New publication/export roots refuse a reserved object. No provider deletion precedes this.
    pub fn reserve_delete(&self, scope: &Scope, id: &ArtifactId) -> Result<(), Error> {
        self.change(scope, |ledger| {
            if !ledger.references_complete
                || ledger
                    .references
                    .values()
                    .chain(ledger.exports.values())
                    .any(|ids| ids.contains(id))
            {
                return Err(Error::Protected);
            }
            let record = ledger.records.get_mut(id).ok_or(Error::Missing)?;
            if record.intent_open {
                return Err(Error::Protected);
            }
            if record.status == Status::Deleting {
                return Ok(());
            }
            if !matches!(record.status, Status::Available | Status::Failed)
                || record.sealed.is_none()
            {
                return Err(Error::NotAvailable);
            }
            record.status = Status::Deleting;
            Ok(())
        })
    }

    /// Retry a reserved generation's conditional delete. Unknown outcomes remain reserved; a
    /// missing exact generation completes it. No retry ever deletes a replacement generation.
    pub fn delete_reserved(&self, scope: &Scope, id: &ArtifactId) -> Result<(), Error> {
        let record = self.record(scope, id)?;
        if record.status == Status::Deleted {
            return Ok(());
        }
        if record.status != Status::Deleting {
            return Err(Error::Protected);
        }
        self.reserve_delete(scope, id)?;
        let sealed = record.sealed.as_ref().ok_or(Error::Metadata)?;
        match self.provider.delete(scope, id, &sealed.generation) {
            Ok(()) | Err(Failure::Missing) => {}
            Err(error) => return Err(error.into()),
        }
        self.change(scope, |ledger| {
            let current = ledger.records.get_mut(id).ok_or(Error::Missing)?;
            if !matches!(current.status, Status::Deleting | Status::Deleted) {
                return Err(Error::Conflict);
            }
            current.status = Status::Deleted;
            Ok(())
        })
    }

    fn current(&self, scope: &Scope, permit: &Permit) -> Result<(), Error> {
        if !scope.valid() {
            return Err(Error::Invalid);
        }
        let snapshot = self.metadata.load(scope)?;
        if snapshot.ledger.format != 1 {
            return Err(Error::Metadata);
        }
        admitted(&snapshot.ledger, permit).map(|_| ())
    }

    fn record(&self, scope: &Scope, id: &ArtifactId) -> Result<Record, Error> {
        if !scope.valid() {
            return Err(Error::Invalid);
        }
        let snapshot = self.metadata.load(scope)?;
        if snapshot.ledger.format != 1 {
            return Err(Error::Metadata);
        }
        let record = snapshot
            .ledger
            .records
            .get(id)
            .cloned()
            .ok_or(Error::Missing)?;
        if record.provider != self.provider.binding() {
            return Err(Error::Conflict);
        }
        Ok(record)
    }
}

fn admitted<'a>(ledger: &'a Ledger, permit: &Permit) -> Result<&'a Binding, Error> {
    let operation = ledger.operations.get(&permit.owner).ok_or(Error::Stale)?;
    if operation.revision != permit.revision {
        return Err(Error::Stale);
    }
    operation.binding.as_ref().ok_or(Error::Stale)
}

fn valid_binding(binding: &Binding) -> bool {
    binding.source.valid()
        && plain(&binding.operation)
        && match &binding.producer {
            Producer::Human { actor } => plain(actor),
            Producer::Attempt {
                id,
                task,
                role,
                handle,
                epoch: _,
                fence,
            } => [id, task, role, handle, fence]
                .into_iter()
                .all(|v| plain(v)),
        }
}
