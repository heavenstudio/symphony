//! A transaction conflict proves the guarded callback runs again with the original expected state.
//! The conflict is the server's own: a trigger of the test's raises `serialization_failure` at the
//! guarded write's first update of the task, once, and the retry reaches the board as it is.

use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

/// The task's first update from now on fails as a conflict does; every later one goes through. The
/// sequence counts the updates tried, as no rollback takes a sequence's value back.
fn one_conflict(f: &Fixture) {
    f.postgresql()
        .simple(
            "CREATE SEQUENCE fixture_conflicts; \
             CREATE FUNCTION fixture_conflict() RETURNS trigger LANGUAGE plpgsql AS $$ \
             BEGIN \
                 IF nextval('fixture_conflicts') = 1 THEN \
                     RAISE EXCEPTION USING ERRCODE = 'serialization_failure', \
                         MESSAGE = 'a conflict of the test''s own'; \
                 END IF; \
                 RETURN NEW; \
             END $$; \
             CREATE TRIGGER fixture_conflict BEFORE UPDATE ON task \
                 FOR EACH ROW EXECUTE FUNCTION fixture_conflict();",
        )
        .unwrap();
}

/// How many updates of the task the trigger has seen.
fn updates_tried(f: &Fixture) -> i64 {
    let answer = f
        .postgresql()
        .request(&[symphony_board::pg::Step::bare(
            "SELECT CASE WHEN is_called THEN last_value ELSE 0 END::text FROM fixture_conflicts",
        )])
        .unwrap();
    answer[0].value().unwrap().parse().unwrap()
}

/// An unknown pair: the first try observes its source current and meets the conflict at its
/// update; the retry observes the source again, now changed, and is refused against the same
/// expected state, writing nothing.
#[test]
fn a_real_retry_reobserves_source_without_refreshing_the_frozen_task_fence() {
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    let before = f.store.get(SLUG).unwrap();
    let lines = f.said(SLUG);
    one_conflict(&f);
    let current = AtomicBool::new(true);
    let mut observed = Vec::new();
    let result =
        f.store
            .record_waits_guarded(SLUG, RECORD, "original", ACTOR, &expected, &mut || {
                // The source changes once it has been observed.
                let value = current.swap(false, Ordering::SeqCst);
                observed.push(value);
                Ok(value)
            });
    assert_eq!(observed, [true, false], "{result:?}");
    assert_eq!(
        updates_tried(&f),
        1,
        "the conflict was met once, at the first try"
    );
    assert_eq!(
        refused(result),
        "assessment source changed before publication"
    );
    let after = f.store.get(SLUG).unwrap();
    assert_eq!(after.fields, before.fields);
    assert_eq!(after.updated, before.updated);
    assert_eq!(f.said(SLUG), lines);
}

/// A known committed pair returns its original receipt, the source unreadable and its actor gone,
/// and writes nothing.
#[test]
fn a_known_pair_returns_its_original_receipt_whatever_its_source_says() {
    let f = Fixture::new(Seed::default().session(ACTOR));
    f.task(SLUG);
    let expected = f.store.assessment_state(SLUG).unwrap();
    let original = f
        .store
        .record_waits_checked(SLUG, RECORD, "original", ACTOR, &expected)
        .unwrap();
    let newer = f.store.assessment_state(SLUG).unwrap();
    f.store
        .record_waits_checked(SLUG, RECORD, "newer", ACTOR, &newer)
        .unwrap();
    f.store.leave(ACTOR).unwrap();
    let before = f.store.get(SLUG).unwrap();
    let lines = f.said(SLUG);
    one_conflict(&f);
    let mut observed = 0;
    let result =
        f.store
            .record_waits_guarded(SLUG, RECORD, "original", ACTOR, &expected, &mut || {
                observed += 1;
                Err(symphony_board::Error::Db("source became unreadable".into()))
            });
    let mut kept = original;
    kept.current = false;
    assert_eq!(result, Ok(kept));
    assert_eq!(observed, 1);
    assert_eq!(updates_tried(&f), 0, "nothing was updated");
    let after = f.store.get(SLUG).unwrap();
    assert_eq!(after.fields, before.fields);
    assert_eq!(after.updated, before.updated);
    assert_eq!(f.said(SLUG), lines);
}
