//! The checked entry's atomic fence and original receipts. API and production-entry acceptance
//! exercise the transport separately; these native cases protect the mutation's own races.

use super::*;

const SLUG: &str = "sim/jump";
const RECORD: &str = "aaa: changes";
const ACTOR: &str = "brisk-finch";

#[path = "source_retry.rs"]
mod source_retry;

#[test]
fn an_external_move_after_observation_is_not_serialized_by_the_board_transaction() {
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    let lines = f.said(SLUG).len();
    let current = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let moved = current.clone();
    // The clock runs after the read-only source callback and before the query is sent.
    let late = f.late(move || moved.store(false, std::sync::atomic::Ordering::SeqCst));
    let mut observations = Vec::new();
    let result = late.record_waits_guarded(SLUG, RECORD, "observed", ACTOR, &expected, &mut || {
        let observed = current.load(std::sync::atomic::Ordering::SeqCst);
        observations.push(observed);
        Ok(observed)
    });
    assert!(result.is_ok(), "{observations:?}: {result:?}");
    assert_eq!(observations, [true]);
    assert!(!current.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(f.said(SLUG).len(), lines + 1);
}

#[test]
fn a_snapshot_writes_nothing_and_a_late_competing_write_refuses_the_fresh_assessment() {
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let before = f.said(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    assert_eq!((&*expected.slug, &*expected.status), (SLUG, "ready"));
    assert_eq!(f.store.assessment_state(SLUG).unwrap(), expected);
    assert_eq!(f.said(SLUG), before);
    assert!(refused(f.store.assessment_state("missing")).contains("no task 'missing'"));
    let competitor = f.other();
    let late = f.late(move || {
        competitor
            .set_field(SLUG, "game", "effort", &json!(7), ACTOR)
            .unwrap();
    });
    let why = refused(late.record_waits_checked(SLUG, RECORD, "old-decision", ACTOR, &expected));
    assert!(why.contains("changed since its assessment"), "{why}");
    let task = f.store.get(SLUG).unwrap();
    assert_eq!(task.fields["game"]["effort"], json!(7));
    assert!(!task.fields.contains_key("_board"));
    assert_eq!(
        f.said(SLUG).len(),
        before.len() + 1,
        "only the competitor wrote"
    );
    assert_eq!(
        f.store.assessment_state(SLUG).unwrap().version,
        expected.version + 1
    );
}

#[test]
fn status_version_and_actor_fences_leave_protected_fields_unchanged() {
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    let before = f.said(SLUG);
    let mut wrong = expected.clone();
    wrong.status = "review".into();
    let why = refused(
        f.store
            .record_waits_checked(SLUG, RECORD, "status", ACTOR, &wrong),
    );
    assert!(
        why.contains("changed since its assessment"),
        "{wrong:?}: {why}"
    );
    wrong = expected.clone();
    wrong.version += 1;
    let why = refused(
        f.store
            .record_waits_checked(SLUG, RECORD, "version", ACTOR, &wrong),
    );
    assert!(
        why.contains("changed since its assessment"),
        "{wrong:?}: {why}"
    );
    for (slug, version) in [("other", expected.version), (SLUG, -1)] {
        wrong = expected.clone();
        wrong.slug = slug.into();
        wrong.version = version;
        let why = refused(
            f.store
                .record_waits_checked(SLUG, RECORD, "invalid", ACTOR, &wrong),
        );
        assert!(why.contains("invalid assessment state"), "{wrong:?}: {why}");
    }
    let denied = refused(
        f.store
            .record_waits_checked(SLUG, RECORD, "actor", "absent", &expected),
    );
    assert!(denied.contains("absent"), "{denied}");
    for field in ["waits", "assessed", "assessments"] {
        let why = refused(
            f.store
                .set_field(SLUG, "_board", field, &json!("forged"), ACTOR),
        );
        assert!(why.contains("board's own"), "{field}: {why}");
    }
    assert_eq!(f.said(SLUG), before);
    assert!(f.store.get(SLUG).unwrap().fields.is_empty());
}

#[test]
fn concurrent_known_duplicates_share_one_receipt_but_distinct_decisions_cannot_share_a_stale_fence()
{
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    let before = f.said(SLUG).len();
    let stores: Vec<_> = (0..4).map(|_| f.other()).collect();
    let start = std::sync::Barrier::new(stores.len());
    std::thread::scope(|scope| {
        let calls: Vec<_> = stores
            .iter()
            .map(|store| {
                let (start, expected) = (&start, &expected);
                scope.spawn(move || {
                    start.wait();
                    store
                        .record_waits_checked(SLUG, RECORD, "same", ACTOR, expected)
                        .unwrap()
                })
            })
            .collect();
        let receipts: Vec<_> = calls.into_iter().map(|call| call.join().unwrap()).collect();
        assert_eq!(receipts, vec![receipts[0].clone(); 4]);
    });
    assert_eq!(f.said(SLUG).len(), before + 1);
    let next = f.store.assessment_state(SLUG).unwrap();
    let start = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let calls: Vec<_> = stores[..2]
            .iter()
            .enumerate()
            .map(|(i, store)| {
                let (start, next) = (&start, &next);
                scope.spawn(move || {
                    start.wait();
                    store.record_waits_checked(SLUG, RECORD, &format!("fresh-{i}"), ACTOR, next)
                })
            })
            .collect();
        let results: Vec<_> = calls.into_iter().map(|call| call.join().unwrap()).collect();
        assert_eq!(
            results.iter().filter(|result| result.is_ok()).count(),
            1,
            "{results:?}"
        );
        let rejected = results.into_iter().find(Result::is_err).unwrap();
        let why = refused(rejected);
        assert!(
            why.contains("changed since its assessment"),
            "{next:?}: {why}"
        );
    });
    assert_eq!(f.said(SLUG).len(), before + 2);
}

#[test]
fn source_observations_refuse_fresh_writes_but_never_hide_a_committed_receipt() {
    use symphony_board::Error;
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    let before = f.store.get(SLUG).unwrap();
    let lines = f.said(SLUG);
    for observation in [Ok(false), Err(Error::Db("unreadable source".into()))] {
        let result =
            f.store
                .record_waits_guarded(SLUG, RECORD, "fresh", ACTOR, &expected, &mut || {
                    observation.clone()
                });
        match &observation {
            Ok(false) => assert!(
                matches!(result, Err(Error::Refused(_))),
                "{observation:?}: {result:?}"
            ),
            Err(_) => assert!(
                matches!(result, Err(Error::Db(_))),
                "{observation:?}: {result:?}"
            ),
            _ => unreachable!(),
        }
        let after = f.store.get(SLUG).unwrap();
        assert_eq!(after.fields, before.fields, "{observation:?}");
        assert_eq!(after.updated, before.updated, "{observation:?}");
        assert_eq!(f.said(SLUG), lines, "{observation:?}");
    }
    let mut original = f
        .store
        .record_waits_guarded(SLUG, RECORD, "original", ACTOR, &expected, &mut || Ok(true))
        .unwrap();
    let next = f.store.assessment_state(SLUG).unwrap();
    f.store
        .record_waits_checked(SLUG, RECORD, "newer", ACTOR, &next)
        .unwrap();
    f.store.leave(ACTOR).unwrap();
    original.current = false;
    let before = f.store.get(SLUG).unwrap();
    let lines = f.said(SLUG);
    for observation in [Ok(false), Err(Error::Db("missing source".into()))] {
        let result =
            f.store
                .record_waits_guarded(SLUG, RECORD, "original", ACTOR, &expected, &mut || {
                    observation.clone()
                });
        assert_eq!(result, Ok(original.clone()), "{observation:?}");
        let after = f.store.get(SLUG).unwrap();
        assert_eq!(after.fields, before.fields, "{observation:?}");
        assert_eq!(after.updated, before.updated, "{observation:?}");
        assert_eq!(f.said(SLUG), lines, "{observation:?}");
    }
}

#[test]
fn a_known_older_receipt_survives_moved_state_and_a_departed_writer_without_replacing_the_newer() {
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    f.store
        .set_field(SLUG, "game", "effort", &json!(3), ACTOR)
        .unwrap();
    let expected = f.store.assessment_state(SLUG).unwrap();
    let first = f
        .store
        .record_waits_checked(SLUG, RECORD, "first", ACTOR, &expected)
        .unwrap();
    assert_eq!(
        (&*first.slug, &*first.id, &*first.record),
        (SLUG, "first", RECORD)
    );
    assert!(first.current);
    assert!(first.sid > 0);
    assert_eq!(first.at, f.now());
    let next_state = f.store.assessment_state(SLUG).unwrap();
    let second = f
        .store
        .record_waits_checked(SLUG, RECORD, "second", ACTOR, &next_state)
        .unwrap();
    assert_ne!(first.sid, second.sid);
    assert_eq!(
        first.at, second.at,
        "fresh occurrences may share the board time"
    );
    f.store.leave(ACTOR).unwrap();
    let before = f.store.get(SLUG).unwrap();
    let lines = f.said(SLUG);
    let mut original = first;
    original.current = false;
    let retried = f
        .store
        .record_waits_checked(SLUG, RECORD, "first", ACTOR, &expected)
        .unwrap();
    assert_eq!(retried, original);
    let after = f.store.get(SLUG).unwrap();
    assert_eq!(after.fields, before.fields);
    assert_eq!(after.updated, before.updated);
    assert_eq!(after.fields["game"]["effort"], json!(3));
    assert_eq!(after.fields["_board"]["assessed"]["id"], json!("second"));
    assert_eq!(f.said(SLUG), lines);
}
