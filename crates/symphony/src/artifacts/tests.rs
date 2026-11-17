use super::*;
use crate::test_tree::Tree;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Barrier, Mutex};

#[derive(Default)]
struct Memory {
    rows: Mutex<BTreeMap<Scope, (u64, Vec<u8>)>>,
    fail_before: AtomicBool,
    fail_after: AtomicBool,
    conflicts: AtomicUsize,
}
impl Metadata for Memory {
    fn load(&self, scope: &Scope) -> Result<Snapshot, Error> {
        let rows = self.rows.lock().unwrap();
        Ok(match rows.get(scope) {
            Some((revision, bytes)) => Snapshot {
                revision: *revision,
                ledger: serde_json::from_slice(bytes).unwrap(),
            },
            None => Snapshot {
                revision: 0,
                ledger: Ledger::default(),
            },
        })
    }
    fn compare_exchange(&self, scope: &Scope, expected: u64, next: &Ledger) -> Result<bool, Error> {
        if self.fail_before.swap(false, Ordering::SeqCst) {
            return Err(Error::Metadata);
        }
        if self
            .conflicts
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Ok(false);
        }
        let mut rows = self.rows.lock().unwrap();
        if rows.get(scope).map_or(0, |(revision, _)| *revision) != expected {
            return Ok(false);
        }
        rows.insert(
            scope.clone(),
            (expected + 1, serde_json::to_vec(next).unwrap()),
        );
        if self.fail_after.swap(false, Ordering::SeqCst) {
            return Err(Error::Metadata);
        }
        Ok(true)
    }
}

type Objects = BTreeMap<(Scope, ArtifactId), (Generation, Vec<u8>)>;
#[derive(Default)]
struct Fake {
    objects: Mutex<Objects>,
    put_lost: AtomicBool,
    delete_lost: AtomicBool,
    read_failure: Mutex<Option<Failure>>,
    inspect_failure: Mutex<Option<Failure>>,
    put_failure: Mutex<Option<Failure>>,
    delete_failure: Mutex<Option<Failure>>,
    reads: AtomicUsize,
}
impl Provider for Fake {
    fn binding(&self) -> &str {
        "fake-v1"
    }
    fn put(&self, scope: &Scope, id: &ArtifactId, bytes: &[u8]) -> Result<Generation, Failure> {
        if let Some(failure) = self.put_failure.lock().unwrap().take() {
            return Err(failure);
        }
        let generation = self
            .objects
            .lock()
            .unwrap()
            .entry((scope.clone(), id.clone()))
            .or_insert_with(|| (Generation::new("generation-1").unwrap(), bytes.into()))
            .0
            .clone();
        if self.put_lost.swap(false, Ordering::SeqCst) {
            return Err(Failure::Unknown);
        }
        Ok(generation)
    }
    fn inspect(&self, scope: &Scope, id: &ArtifactId) -> Result<Generation, Failure> {
        if let Some(failure) = self.inspect_failure.lock().unwrap().take() {
            return Err(failure);
        }
        self.objects
            .lock()
            .unwrap()
            .get(&(scope.clone(), id.clone()))
            .map(|(g, _)| g.clone())
            .ok_or(Failure::Missing)
    }
    fn read(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<Vec<u8>, Failure> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        if let Some(failure) = self.read_failure.lock().unwrap().take() {
            return Err(failure);
        }
        let objects = self.objects.lock().unwrap();
        let (actual, bytes) = objects
            .get(&(scope.clone(), id.clone()))
            .ok_or(Failure::Missing)?;
        if actual != generation {
            return Err(Failure::GenerationMismatch);
        }
        Ok(bytes.clone())
    }
    fn delete(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<(), Failure> {
        if let Some(failure) = self.delete_failure.lock().unwrap().take() {
            return Err(failure);
        }
        let mut objects = self.objects.lock().unwrap();
        let (actual, _) = objects
            .get(&(scope.clone(), id.clone()))
            .ok_or(Failure::Missing)?;
        if actual != generation {
            return Err(Failure::GenerationMismatch);
        }
        objects.remove(&(scope.clone(), id.clone()));
        if self.delete_lost.swap(false, Ordering::SeqCst) {
            return Err(Failure::Unknown);
        }
        Ok(())
    }
}

type ReadAction<'a> = Box<dyn Fn(&Scope, &ArtifactId) + Send + Sync + 'a>;

/// Interleave another actor after verified byte delivery, without a scheduler or timed wait.
struct OnRead<'a> {
    inner: &'a Fake,
    then: ReadAction<'a>,
}

impl Provider for OnRead<'_> {
    fn binding(&self) -> &str {
        self.inner.binding()
    }
    fn put(&self, scope: &Scope, id: &ArtifactId, bytes: &[u8]) -> Result<Generation, Failure> {
        self.inner.put(scope, id, bytes)
    }
    fn inspect(&self, scope: &Scope, id: &ArtifactId) -> Result<Generation, Failure> {
        self.inner.inspect(scope, id)
    }
    fn read(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<Vec<u8>, Failure> {
        let bytes = self.inner.read(scope, id, generation)?;
        (self.then)(scope, id);
        Ok(bytes)
    }
    fn delete(
        &self,
        scope: &Scope,
        id: &ArtifactId,
        generation: &Generation,
    ) -> Result<(), Failure> {
        self.inner.delete(scope, id, generation)
    }
}

fn scope() -> Scope {
    Scope::new("tenant-a", "project-a").unwrap()
}
fn binding() -> Binding {
    Binding {
        source: Revision::new("repository-a", &"a".repeat(40)).unwrap(),
        producer: Producer::Human {
            actor: "owner".into(),
        },
        operation: "upload-1".into(),
    }
}
fn core<'a>(metadata: &'a Memory, provider: &'a Fake) -> Core<'a, Memory, Fake> {
    Core { metadata, provider }
}
fn admitted(core: &Core<'_, Memory, Fake>) -> Permit {
    core.fence(&scope(), "operation-owner", 0, Some(binding()))
        .unwrap()
}
fn begin(core: &Core<'_, Memory, Fake>, permit: &Permit) -> ArtifactId {
    core.begin(&scope(), "retry-1", permit, "evidence.txt", b"evidence")
        .unwrap()
}
fn publication(permit: &Permit, id: &ArtifactId) -> Publication {
    Publication {
        owner: "task:one".into(),
        permit: permit.clone(),
        artifacts: BTreeSet::from([id.clone()]),
        delivered: Some(Revision::new("repository-a", &"b".repeat(40)).unwrap()),
    }
}

#[test]
fn intent_is_durable_before_upload_and_retry_checks_bytes_metadata_and_provenance() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    assert_eq!(c.status(&scope(), &id), Ok(Status::Pending));
    assert!(p.objects.lock().unwrap().is_empty());
    assert_eq!(
        c.reconcile(&scope(), &id),
        Err(Error::Provider(Failure::Missing))
    );
    assert_eq!(c.status(&scope(), &id), Ok(Status::Pending));
    assert_eq!(begin(&core(&m, &p), &permit), id);
    assert_eq!(
        c.begin(&scope(), "retry-1", &permit, "evidence.txt", b"different"),
        Err(Error::Conflict)
    );
    assert_eq!(
        c.begin(&scope(), "retry-1", &permit, "renamed.txt", b"evidence"),
        Err(Error::Conflict)
    );
    let next = c
        .fence(&scope(), &permit.owner, permit.revision, Some(binding()))
        .unwrap();
    assert_eq!(
        c.begin(&scope(), "retry-1", &next, "evidence.txt", b"evidence"),
        Err(Error::Conflict)
    );
    assert_eq!(m.load(&scope()).unwrap().ledger.records.len(), 1);
}

#[test]
fn verified_availability_precedes_atomic_idempotent_publication() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    let publication = publication(&permit, &id);
    assert_eq!(
        c.publish(&scope(), "result-1", &publication),
        Err(Error::NotAvailable)
    );
    assert_eq!(c.read(&scope(), &id, None), Err(Error::NotAvailable));
    let provider = OnRead {
        inner: &p,
        then: Box::new(|scope, id| {
            assert_eq!(c.status(scope, id), Ok(Status::Pending));
            assert_eq!(
                c.publish(scope, "during-verification", &publication),
                Err(Error::NotAvailable)
            );
        }),
    };
    let sealed = (Core {
        metadata: &m,
        provider: &provider,
    })
    .upload(&scope(), &id, b"evidence")
    .unwrap();
    assert_eq!(sealed.sha256, model::digest(b"evidence"));
    assert_eq!(sealed.media_type, "text/plain; charset=utf-8");
    assert_eq!(sealed.bytes, 8);
    assert_eq!(p.reads.load(Ordering::SeqCst), 1);
    c.publish(&scope(), "result-1", &publication).unwrap();
    c.publish(&scope(), "result-1", &publication).unwrap();
    let mut different = publication.clone();
    different.owner = "task:two".into();
    assert_eq!(
        c.publish(&scope(), "result-1", &different),
        Err(Error::Conflict)
    );
    let ledger = m.load(&scope()).unwrap().ledger;
    assert_eq!(ledger.publications.len(), 1);
    assert_eq!(ledger.references.len(), 1);
    assert_eq!(ledger.records[&id].binding.source, binding().source);
    assert_ne!(
        ledger.publications["result-1"].delivered,
        Some(binding().source)
    );
}

#[test]
fn crash_before_or_after_intent_commit_converges_without_provider_work() {
    for after in [false, true] {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        if after {
            m.fail_after.store(true, Ordering::SeqCst);
        } else {
            m.fail_before.store(true, Ordering::SeqCst);
        }
        assert_eq!(
            c.begin(&scope(), "retry-1", &permit, "evidence.txt", b"evidence"),
            Err(Error::Metadata)
        );
        let id = begin(&core(&m, &p), &permit);
        assert_eq!(m.load(&scope()).unwrap().ledger.records.len(), 1);
        assert_eq!(c.status(&scope(), &id), Ok(Status::Pending));
        assert!(p.objects.lock().unwrap().is_empty());
    }
}

#[test]
fn upload_acknowledgement_and_availability_commit_crashes_reconcile_exact_generation() {
    for boundary in 0..3 {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        let id = begin(&c, &permit);
        match boundary {
            0 => p.put_lost.store(true, Ordering::SeqCst),
            1 => m.fail_before.store(true, Ordering::SeqCst),
            _ => m.fail_after.store(true, Ordering::SeqCst),
        }
        assert!(c.upload(&scope(), &id, b"evidence").is_err());
        let sealed = core(&m, &p).reconcile(&scope(), &id).unwrap();
        assert_eq!(c.upload(&scope(), &id, b"evidence").unwrap(), sealed);
        assert_eq!(p.objects.lock().unwrap().len(), 1);
        assert_eq!(c.status(&scope(), &id), Ok(Status::Available));
    }
}

#[test]
fn reference_commit_crashes_leave_one_publication_and_no_partial_edges() {
    for after in [false, true] {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        let id = begin(&c, &permit);
        c.upload(&scope(), &id, b"evidence").unwrap();
        let publication = publication(&permit, &id);
        if after {
            m.fail_after.store(true, Ordering::SeqCst);
        } else {
            m.fail_before.store(true, Ordering::SeqCst);
        }
        assert_eq!(
            c.publish(&scope(), "result", &publication),
            Err(Error::Metadata)
        );
        let ledger = m.load(&scope()).unwrap().ledger;
        assert_eq!(ledger.publications.is_empty(), ledger.references.is_empty());
        c.publish(&scope(), "result", &publication).unwrap();
        assert_eq!(m.load(&scope()).unwrap().ledger.publications.len(), 1);
    }
}

#[test]
fn concurrent_retry_and_reference_insertion_converge() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let barrier = Barrier::new(6);
    let ids = std::thread::scope(|threads| {
        let handles: Vec<_> = (0..6)
            .map(|_| {
                threads.spawn(|| {
                    barrier.wait();
                    let id = begin(&c, &permit);
                    c.upload(&scope(), &id, b"evidence").unwrap();
                    c.publish(&scope(), "result", &publication(&permit, &id))
                        .unwrap();
                    id
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(ids.iter().all(|id| id == &ids[0]));
    assert_eq!(p.objects.lock().unwrap().len(), 1);
    assert_eq!(m.load(&scope()).unwrap().ledger.publications.len(), 1);
}

#[test]
fn mismatched_concurrent_retry_has_one_winner() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let barrier = Barrier::new(2);
    let results = std::thread::scope(|threads| {
        [b"one".as_slice(), b"two".as_slice()]
            .map(|bytes| {
                threads.spawn(|| {
                    barrier.wait();
                    c.begin(&scope(), "same-key", &permit, "evidence.txt", bytes)
                })
            })
            .map(|h| h.join().unwrap())
    });
    assert_eq!(results.iter().filter(|r| r.is_ok()).count(), 1);
    assert!(results.contains(&Err(Error::Conflict)));
    assert_eq!(m.load(&scope()).unwrap().ledger.records.len(), 1);
}

#[test]
fn same_size_corruption_missing_and_replaced_generations_fail_closed() {
    for failure in [
        Failure::Integrity,
        Failure::Missing,
        Failure::GenerationMismatch,
    ] {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        let id = begin(&c, &permit);
        c.upload(&scope(), &id, b"evidence").unwrap();
        c.publish(&scope(), "result", &publication(&permit, &id))
            .unwrap();
        let key = (scope(), id.clone());
        match failure {
            Failure::Integrity => {
                p.objects.lock().unwrap().get_mut(&key).unwrap().1 = b"corrupt!".to_vec()
            }
            Failure::Missing => {
                p.objects.lock().unwrap().remove(&key);
            }
            _ => {
                p.objects.lock().unwrap().get_mut(&key).unwrap().0 =
                    Generation::new("generation-2").unwrap()
            }
        }
        assert_eq!(c.read(&scope(), &id, None), Err(Error::Provider(failure)));
        assert_eq!(c.status(&scope(), &id), Ok(Status::Failed));
        let ledger = m.load(&scope()).unwrap().ledger;
        assert_eq!(ledger.publications.len(), 1);
        assert!(ledger.references["task:one"].contains(&id));
    }
}

#[test]
fn corruption_during_initial_verification_never_seals_or_publishes() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    p.put(&scope(), &id, b"corrupt!").unwrap();
    assert_eq!(
        c.reconcile(&scope(), &id),
        Err(Error::Provider(Failure::Integrity))
    );
    assert_eq!(c.status(&scope(), &id), Ok(Status::Failed));
    assert_eq!(
        c.publish(&scope(), "result", &publication(&permit, &id)),
        Err(Error::NotAvailable)
    );
}

#[test]
fn transient_incomplete_and_forbidden_provider_results_are_explicit() {
    for failure in [
        Failure::Unknown,
        Failure::Unavailable,
        Failure::Incomplete,
        Failure::Forbidden,
    ] {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        let id = begin(&c, &permit);
        *p.put_failure.lock().unwrap() = Some(failure);
        assert_eq!(
            c.upload(&scope(), &id, b"evidence"),
            Err(Error::Provider(failure))
        );
        *p.inspect_failure.lock().unwrap() = Some(failure);
        assert_eq!(c.reconcile(&scope(), &id), Err(Error::Provider(failure)));
        assert_eq!(c.status(&scope(), &id), Ok(Status::Pending));
        c.upload(&scope(), &id, b"evidence").unwrap();
        *p.read_failure.lock().unwrap() = Some(failure);
        assert_eq!(c.read(&scope(), &id, None), Err(Error::Provider(failure)));
        assert_eq!(c.status(&scope(), &id), Ok(Status::Available));
    }
}

#[test]
fn project_guesses_same_digests_and_legacy_aliases_confer_no_authority() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    let descriptor = c.upload(&scope(), &id, b"evidence").unwrap();
    for other in [
        Scope::new("tenant-b", "project-a").unwrap(),
        Scope::new("tenant-a", "project-b").unwrap(),
    ] {
        assert_eq!(c.read(&other, &id, None), Err(Error::Missing));
        let other_permit = c
            .fence(&other, "operation-owner", 0, Some(binding()))
            .unwrap();
        let other_id = c
            .begin(
                &other,
                "retry-1",
                &other_permit,
                "evidence.txt",
                b"evidence",
            )
            .unwrap();
        assert_ne!(other_id, id);
        assert_eq!(
            c.upload(&other, &other_id, b"evidence").unwrap().sha256,
            descriptor.sha256
        );
        assert_eq!(c.read(&other, &id, None), Err(Error::Missing));
    }
    assert!(ArtifactId::parse("/artifacts/uploads/../file").is_err());
    assert_eq!(
        c.read(
            &scope(),
            &ArtifactId::parse(&descriptor.sha256).unwrap(),
            None
        ),
        Err(Error::Missing)
    );
}

#[test]
fn cancellation_replacement_and_fake_restore_fences_reject_stale_completion() {
    for next in [
        None,
        Some(binding()),
        Some(Binding {
            operation: "new-epoch".into(),
            ..binding()
        }),
    ] {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        let id = begin(&c, &permit);
        p.put(&scope(), &id, b"evidence").unwrap();
        c.fence(&scope(), &permit.owner, permit.revision, next)
            .unwrap();
        assert_eq!(c.reconcile(&scope(), &id), Err(Error::Stale));
        assert_eq!(c.upload(&scope(), &id, b"evidence"), Err(Error::Stale));
        assert_eq!(
            c.publish(&scope(), "result", &publication(&permit, &id)),
            Err(Error::Stale)
        );
        assert_eq!(
            c.fence(&scope(), &permit.owner, permit.revision, None),
            Err(Error::Stale)
        );
        assert_eq!(m.load(&scope()).unwrap().ledger.publications.len(), 0);
    }
}

#[test]
fn cancellation_between_verified_bytes_and_metadata_cas_is_rechecked() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    let provider = OnRead {
        inner: &p,
        then: Box::new(|scope, _id| {
            c.fence(scope, &permit.owner, permit.revision, None)
                .unwrap();
        }),
    };
    assert_eq!(
        (Core {
            metadata: &m,
            provider: &provider
        })
        .upload(&scope(), &id, b"evidence"),
        Err(Error::Stale)
    );
    assert_eq!(c.status(&scope(), &id), Ok(Status::Pending));
    assert_eq!(p.objects.lock().unwrap().len(), 1);
}

#[test]
fn interleaved_finalizers_converge_after_both_verify_the_same_generation() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    let provider = OnRead {
        inner: &p,
        then: Box::new(|scope, id| {
            c.upload(scope, id, b"evidence").unwrap();
        }),
    };
    let descriptor = (Core {
        metadata: &m,
        provider: &provider,
    })
    .upload(&scope(), &id, b"evidence")
    .unwrap();
    assert_eq!(descriptor.generation, p.inspect(&scope(), &id).unwrap());
    assert_eq!(p.reads.load(Ordering::SeqCst), 2);
    assert_eq!(c.status(&scope(), &id), Ok(Status::Available));
    assert_eq!(m.load(&scope()).unwrap().ledger.records.len(), 1);
    assert_eq!(p.objects.lock().unwrap().len(), 1);
}

#[test]
fn unknown_inventory_open_intents_references_and_exports_protect_generation() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    c.upload(&scope(), &id, b"evidence").unwrap();
    assert_eq!(c.reserve_delete(&scope(), &id), Err(Error::Protected));
    c.reference_inventory(&scope(), true).unwrap();
    assert_eq!(c.reserve_delete(&scope(), &id), Err(Error::Protected));
    c.publish(&scope(), "result", &publication(&permit, &id))
        .unwrap();
    c.retain_export(&scope(), "backup-1", &BTreeSet::from([id.clone()]))
        .unwrap();
    assert_eq!(c.reserve_delete(&scope(), &id), Err(Error::Protected));
    c.release_owner(&scope(), "task:one").unwrap();
    assert_eq!(c.reserve_delete(&scope(), &id), Err(Error::Protected));
    c.release_export(&scope(), "backup-1").unwrap();
    c.reserve_delete(&scope(), &id).unwrap();
    c.reserve_delete(&scope(), &id).unwrap();
    assert_eq!(
        c.publish(&scope(), "new-result", &publication(&permit, &id)),
        Err(Error::NotAvailable)
    );
    assert_eq!(
        c.retain_export(&scope(), "backup-2", &BTreeSet::from([id.clone()])),
        Err(Error::NotAvailable)
    );
    assert_eq!(c.read(&scope(), &id, None), Err(Error::NotAvailable));
    c.delete_reserved(&scope(), &id).unwrap();
    c.delete_reserved(&scope(), &id).unwrap();
    assert_eq!(c.status(&scope(), &id), Ok(Status::Deleted));
    assert!(p.objects.lock().unwrap().is_empty());
}

#[test]
fn ambiguous_delete_reconciles_without_deleting_a_replacement() {
    for replace in [false, true] {
        let m = Memory::default();
        let p = Fake::default();
        let c = core(&m, &p);
        let permit = admitted(&c);
        let id = begin(&c, &permit);
        c.upload(&scope(), &id, b"evidence").unwrap();
        c.release_intent(&scope(), &id).unwrap();
        c.reference_inventory(&scope(), true).unwrap();
        c.reserve_delete(&scope(), &id).unwrap();
        p.delete_lost.store(true, Ordering::SeqCst);
        assert_eq!(
            c.delete_reserved(&scope(), &id),
            Err(Error::Provider(Failure::Unknown))
        );
        assert_eq!(c.status(&scope(), &id), Ok(Status::Deleting));
        if replace {
            p.objects.lock().unwrap().insert(
                (scope(), id.clone()),
                (
                    Generation::new("replacement").unwrap(),
                    b"evidence".to_vec(),
                ),
            );
        }
        if replace {
            assert_eq!(
                c.delete_reserved(&scope(), &id),
                Err(Error::Provider(Failure::GenerationMismatch))
            );
            assert_eq!(p.objects.lock().unwrap().len(), 1);
        } else {
            c.delete_reserved(&scope(), &id).unwrap();
            assert_eq!(c.status(&scope(), &id), Ok(Status::Deleted));
        }
    }
}

#[test]
fn publication_and_cleanup_race_is_arbitrated_by_one_cas() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    c.upload(&scope(), &id, b"evidence").unwrap();
    c.release_intent(&scope(), &id).unwrap();
    c.reference_inventory(&scope(), true).unwrap();
    let barrier = Barrier::new(2);
    let (published, deleted) = std::thread::scope(|threads| {
        let publish = threads.spawn(|| {
            barrier.wait();
            c.publish(&scope(), "result", &publication(&permit, &id))
        });
        let delete = threads.spawn(|| {
            barrier.wait();
            c.reserve_delete(&scope(), &id)
        });
        (publish.join().unwrap(), delete.join().unwrap())
    });
    assert_ne!(published.is_ok(), deleted.is_ok());
    if published.is_ok() {
        assert_eq!(deleted, Err(Error::Protected));
    } else {
        assert_eq!(published, Err(Error::NotAvailable));
    }
}

#[test]
fn bounded_ranges_upload_guards_and_safe_names_use_legacy_validation() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    for (name, bytes) in [
        ("bad.txt", b"<html>bad".as_slice()),
        ("bad.svg", b"<svg/>"),
        ("empty.txt", b""),
    ] {
        assert_eq!(
            c.begin(&scope(), "bad", &permit, name, bytes),
            Err(Error::Invalid)
        );
    }
    assert_eq!(
        c.begin(
            &scope(),
            "bad",
            &permit,
            "big.txt",
            &vec![b'x'; crate::uploads::MAX_BYTES + 1]
        ),
        Err(Error::Invalid)
    );
    let id = c
        .begin(&scope(), "good", &permit, "../[evidence].txt", b"evidence")
        .unwrap();
    assert_eq!(c.upload(&scope(), &id, b"wrong"), Err(Error::Conflict));
    let sealed = c.upload(&scope(), &id, b"evidence").unwrap();
    assert_eq!(sealed.name, "_evidence_.txt");
    assert_eq!(c.read(&scope(), &id, Some(2..5)), Ok(b"ide".to_vec()));
    assert_eq!(c.read(&scope(), &id, Some(8..8)), Ok(vec![]));
    assert_eq!(c.read(&scope(), &id, Some(0..9)), Err(Error::Invalid));
}

#[test]
fn metadata_cas_conflicts_retry_with_a_bounded_explicit_busy_outcome() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    m.conflicts.store(3, Ordering::SeqCst);
    let permit = admitted(&c);
    m.conflicts.store(32, Ordering::SeqCst);
    assert_eq!(
        c.begin(&scope(), "retry", &permit, "evidence.txt", b"evidence"),
        Err(Error::Busy)
    );
    assert!(m.load(&scope()).unwrap().ledger.records.is_empty());
}

#[test]
fn local_adapter_reopens_exact_generation_without_old_core_or_provider_state() {
    let tree = Tree::new();
    let m = Memory::default();
    let p = Local::for_project(&tree.root);
    let c = Core {
        metadata: &m,
        provider: &p,
    };
    let permit = c.fence(&scope(), "owner", 0, Some(binding())).unwrap();
    let id = c
        .begin(&scope(), "retry", &permit, "evidence.txt", b"evidence")
        .unwrap();
    let descriptor = c.upload(&scope(), &id, b"evidence").unwrap();
    drop(p);
    let p = Local::for_project(&tree.root);
    let c = Core {
        metadata: &m,
        provider: &p,
    };
    assert_eq!(c.reconcile(&scope(), &id).unwrap(), descriptor);
    assert_eq!(c.read(&scope(), &id, None), Ok(b"evidence".to_vec()));
    let other = Scope::new("tenant-b", "project-a").unwrap();
    assert_eq!(p.inspect(&other, &id), Err(Failure::Missing));
    assert_eq!(
        p.delete(&scope(), &id, &Generation::new("wrong").unwrap()),
        Err(Failure::GenerationMismatch)
    );
    assert_eq!(c.read(&scope(), &id, None), Ok(b"evidence".to_vec()));
    c.release_intent(&scope(), &id).unwrap();
    c.reference_inventory(&scope(), true).unwrap();
    c.reserve_delete(&scope(), &id).unwrap();
    c.delete_reserved(&scope(), &id).unwrap();
    assert_eq!(p.inspect(&scope(), &id), Err(Failure::Missing));
}

#[test]
fn local_provider_concurrent_puts_never_overwrite_and_replacement_is_detected() {
    let tree = Tree::new();
    let p = Local::for_project(&tree.root);
    let id = ArtifactId::parse(&"a".repeat(64)).unwrap();
    let barrier = Barrier::new(4);
    let generations = std::thread::scope(|threads| {
        let handles: Vec<_> = (0..4)
            .map(|_| {
                threads.spawn(|| {
                    barrier.wait();
                    p.put(&scope(), &id, b"evidence").unwrap()
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|h| h.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(generations.iter().all(|g| g == &generations[0]));
    assert_eq!(p.put(&scope(), &id, b"different").unwrap(), generations[0]);
    assert_eq!(
        p.read(&scope(), &id, &generations[0]),
        Ok(b"evidence".to_vec())
    );
    p.delete(&scope(), &id, &generations[0]).unwrap();
    let replacement = p.put(&scope(), &id, b"evidence").unwrap();
    assert_ne!(replacement, generations[0]);
    assert_eq!(
        p.read(&scope(), &id, &generations[0]),
        Err(Failure::GenerationMismatch)
    );
    assert_eq!(
        p.delete(&scope(), &id, &generations[0]),
        Err(Failure::GenerationMismatch)
    );
    assert_eq!(
        p.read(&scope(), &id, &replacement),
        Ok(b"evidence".to_vec())
    );
}

#[test]
fn opaque_input_validation_never_accepts_paths_as_ids_or_incomplete_generations() {
    for value in ["", "../escape", &"G".repeat(64), &"a".repeat(63)] {
        assert_eq!(ArtifactId::parse(value), Err(Error::Invalid));
    }
    assert_eq!(Generation::new(""), Err(Failure::Incomplete));
    assert_eq!(Scope::new("", "project"), Err(Error::Invalid));
    assert_ne!(
        Scope::new("a/b", "c").unwrap().key(),
        Scope::new("a", "b/c").unwrap().key()
    );
    for commit in ["main", "short", &"A".repeat(40)] {
        assert_eq!(Revision::new("repo", commit), Err(Error::Invalid));
    }
}

#[test]
fn fake_attempt_provenance_is_revision_bound_and_never_implies_success() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let binding = Binding {
        producer: Producer::Attempt {
            id: "fake-attempt".into(),
            task: "task-one".into(),
            role: "executor".into(),
            handle: "fake-worker".into(),
            epoch: 7,
            fence: "fake-fence".into(),
        },
        ..binding()
    };
    let permit = c
        .fence(&scope(), "attempt-owner", 0, Some(binding.clone()))
        .unwrap();
    let id = begin(&c, &permit);
    c.upload(&scope(), &id, b"evidence").unwrap();
    let ledger = m.load(&scope()).unwrap().ledger;
    assert_eq!(ledger.records[&id].binding, binding);
    assert!(ledger.publications.is_empty());
    assert_eq!(binding.source.repository(), "repository-a");
    assert_eq!(binding.source.commit(), "a".repeat(40));
    c.fence(&scope(), &permit.owner, permit.revision, None)
        .unwrap();
    assert_eq!(
        c.publish(&scope(), "result", &publication(&permit, &id)),
        Err(Error::Stale)
    );
}

#[test]
fn export_idempotency_missing_roots_and_changed_inventory_remain_explicit() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    assert_eq!(c.delete_reserved(&scope(), &id), Err(Error::Protected));
    c.reference_inventory(&scope(), true).unwrap();
    c.release_intent(&scope(), &id).unwrap();
    assert_eq!(c.reserve_delete(&scope(), &id), Err(Error::NotAvailable));
    c.upload(&scope(), &id, b"evidence").unwrap();
    let ids = BTreeSet::from([id.clone()]);
    c.retain_export(&scope(), "backup", &ids).unwrap();
    c.retain_export(&scope(), "backup", &ids).unwrap();
    let missing = ArtifactId::parse(&"f".repeat(64)).unwrap();
    assert_eq!(
        c.retain_export(&scope(), "backup", &BTreeSet::from([missing.clone()])),
        Err(Error::Conflict)
    );
    assert_eq!(
        c.retain_export(&scope(), "missing", &BTreeSet::from([missing])),
        Err(Error::NotAvailable)
    );
    c.release_export(&scope(), "backup").unwrap();
    c.reserve_delete(&scope(), &id).unwrap();
    c.reference_inventory(&scope(), false).unwrap();
    assert_eq!(c.delete_reserved(&scope(), &id), Err(Error::Protected));
    assert_eq!(p.objects.lock().unwrap().len(), 1);
    c.reference_inventory(&scope(), true).unwrap();
    *p.delete_failure.lock().unwrap() = Some(Failure::Unavailable);
    assert_eq!(
        c.delete_reserved(&scope(), &id),
        Err(Error::Provider(Failure::Unavailable))
    );
    assert_eq!(c.status(&scope(), &id), Ok(Status::Deleting));
    c.delete_reserved(&scope(), &id).unwrap();
}

#[test]
fn invalid_deserialized_scopes_revisions_and_unknown_metadata_fail_closed() {
    use serde_json::json;
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    let invalid: Scope =
        serde_json::from_value(json!({"tenant": "", "project": "project-a"})).unwrap();
    assert_eq!(c.read(&invalid, &id, None), Err(Error::Invalid));
    assert_eq!(c.reference_inventory(&invalid, true), Err(Error::Invalid));
    let mut invalid_binding = binding();
    invalid_binding.source =
        serde_json::from_value(json!({"repository": "repo", "commit": "main"})).unwrap();
    assert_eq!(
        c.fence(&scope(), "invalid", 0, Some(invalid_binding.clone())),
        Err(Error::Invalid)
    );
    let mut published = publication(&permit, &id);
    published.delivered = Some(invalid_binding.source);
    assert_eq!(
        c.publish(&scope(), "invalid", &published),
        Err(Error::Invalid)
    );
    let mut snapshot = m.load(&scope()).unwrap();
    snapshot.ledger.format = 99;
    m.compare_exchange(&scope(), snapshot.revision, &snapshot.ledger)
        .unwrap();
    assert_eq!(c.status(&scope(), &id), Err(Error::Metadata));
    assert_eq!(c.reference_inventory(&scope(), false), Err(Error::Metadata));
    assert_eq!(
        Error::Provider(Failure::Incomplete).to_string(),
        "artifact operation: Provider(Incomplete)"
    );
}

#[test]
fn invalid_requests_and_artifacts_from_another_operation_are_refused_atomically() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    c.upload(&scope(), &id, b"evidence").unwrap();
    assert_eq!(
        c.fence(&scope(), "", 0, Some(binding())),
        Err(Error::Invalid)
    );
    assert_eq!(
        c.begin(&scope(), "", &permit, "evidence.txt", b"evidence"),
        Err(Error::Invalid)
    );
    let other = c.fence(&scope(), "other", 0, Some(binding())).unwrap();
    let mut proposed = publication(&other, &id);
    assert_eq!(c.publish(&scope(), "result", &proposed), Err(Error::Stale));
    proposed.artifacts.clear();
    assert_eq!(c.publish(&scope(), "empty", &proposed), Err(Error::Invalid));
    assert_eq!(
        c.retain_export(&scope(), "empty", &BTreeSet::new()),
        Err(Error::Invalid)
    );
    assert!(m.load(&scope()).unwrap().ledger.references.is_empty());
}

#[test]
fn local_adapter_uses_configured_artifact_folder_and_detects_equal_size_corruption() {
    let tree = Tree::new();
    tree.write("symphony.toml", "[project]\nartifacts = 'evidence'\n");
    let m = Memory::default();
    let p = Local::for_project(&tree.root);
    let c = Core {
        metadata: &m,
        provider: &p,
    };
    let permit = c.fence(&scope(), "owner", 0, Some(binding())).unwrap();
    let id = c
        .begin(&scope(), "retry", &permit, "evidence.txt", b"evidence")
        .unwrap();
    let descriptor = c.upload(&scope(), &id, b"evidence").unwrap();
    let path = tree
        .path("evidence/durable")
        .join(scope().key())
        .join(id.as_str());
    assert!(path.exists());
    assert!(!tree.path("artifacts").exists());
    std::fs::write(
        &path,
        format!("{}\ncorrupt!", descriptor.generation.as_str()),
    )
    .unwrap();
    assert_eq!(
        c.read(&scope(), &id, None),
        Err(Error::Provider(Failure::Integrity))
    );
    assert_eq!(c.status(&scope(), &id), Ok(Status::Failed));
}

#[test]
fn local_provider_rejects_malformed_objects_symlinks_and_oversized_payloads() {
    let tree = Tree::new();
    let p = Local::for_project(&tree.root);
    let id = ArtifactId::parse(&"a".repeat(64)).unwrap();
    let generation = p.put(&scope(), &id, b"evidence").unwrap();
    let path = tree
        .root
        .join(crate::project::Project::of(&tree.root).artifact("durable"))
        .join(scope().key())
        .join(id.as_str());
    for bytes in [
        vec![],
        vec![b'a'; 64],
        format!("{}\nx", "G".repeat(64)).into_bytes(),
    ] {
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(p.inspect(&scope(), &id), Err(Failure::Integrity));
        assert_eq!(p.put(&scope(), &id, b"evidence"), Err(Failure::Integrity));
    }
    let mut huge = format!("{}\n", generation.as_str()).into_bytes();
    huge.extend(vec![b'x'; crate::uploads::MAX_BYTES + 1]);
    std::fs::write(&path, huge).unwrap();
    assert_eq!(p.read(&scope(), &id, &generation), Err(Failure::Integrity));
    std::fs::remove_file(&path).unwrap();
    std::fs::create_dir(&path).unwrap();
    assert_eq!(p.inspect(&scope(), &id), Err(Failure::Integrity));
    std::fs::remove_dir(&path).unwrap();
    let target = tree.write("foreign", "foreign bytes");
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert_eq!(p.inspect(&scope(), &id), Err(Failure::Unavailable));
    assert_eq!(std::fs::read(&target).unwrap(), b"foreign bytes");
    assert_eq!(p.put(&scope(), &id, b""), Err(Failure::Integrity));
    let invalid: ArtifactId = serde_json::from_str("\"../escape\"").unwrap();
    assert_eq!(p.inspect(&scope(), &invalid), Err(Failure::Forbidden));
}

#[test]
fn incomplete_generations_and_provider_rebinding_cannot_publish() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    p.put(&scope(), &id, b"evidence").unwrap();
    p.objects
        .lock()
        .unwrap()
        .get_mut(&(scope(), id.clone()))
        .unwrap()
        .0 = serde_json::from_str("\"\"").unwrap();
    assert_eq!(
        c.reconcile(&scope(), &id),
        Err(Error::Provider(Failure::Incomplete))
    );
    assert_eq!(c.status(&scope(), &id), Ok(Status::Pending));
    let tree = Tree::new();
    let replacement = Local::for_project(&tree.root);
    let other_core = Core {
        metadata: &m,
        provider: &replacement,
    };
    assert_eq!(other_core.status(&scope(), &id), Err(Error::Conflict));
    assert!(!tree.path("artifacts").exists());
}

#[test]
fn multiple_artifact_publication_is_atomic_when_one_prerequisite_is_missing() {
    let m = Memory::default();
    let p = Fake::default();
    let c = core(&m, &p);
    let permit = admitted(&c);
    let id = begin(&c, &permit);
    c.upload(&scope(), &id, b"evidence").unwrap();
    let mut proposed = publication(&permit, &id);
    proposed
        .artifacts
        .insert(ArtifactId::parse(&"f".repeat(64)).unwrap());
    assert_eq!(
        c.publish(&scope(), "result", &proposed),
        Err(Error::Missing)
    );
    let ledger = m.load(&scope()).unwrap().ledger;
    assert!(ledger.publications.is_empty());
    assert!(ledger.references.is_empty());
    assert!(ledger.records[&id].intent_open);
}

#[test]
fn host_selected_provider_and_metadata_trait_objects_use_the_same_contract() {
    let metadata: &dyn Metadata = &Memory::default();
    let provider: &dyn Provider = &Fake::default();
    let core = Core { metadata, provider };
    let permit = core.fence(&scope(), "owner", 0, Some(binding())).unwrap();
    let id = core
        .begin(&scope(), "retry", &permit, "evidence.txt", b"evidence")
        .unwrap();
    core.upload(&scope(), &id, b"evidence").unwrap();
    assert_eq!(core.read(&scope(), &id, None), Ok(b"evidence".to_vec()));
}
