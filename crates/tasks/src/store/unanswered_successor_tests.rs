//! Reservation eligibility and stale observations on both stores, without starting an agent.

use super::*;
use crate::store_fixture::HOST;
use std::cell::Cell;

const THREAD: &str = "00000000-0000-4000-8000-000000000001";

fn replacement(f: &Fixture, pending: &Pending) -> String {
    f.advance(31.0 * 60.0);
    let next = f.session("worker");
    f.store.claim(&pending.slug, &next).unwrap();
    next
}

#[test]
fn a_same_task_active_successor_blocks_restore_without_answering_or_reserving_the_post() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        for state in ["live", "stale", "native", "unknown", "busy"] {
            let (f, pending) = setup();
            let next = replacement(&f, &pending);
            match state {
                "stale" => f.advance(crate::health::LIVE_SECONDS + 1.0),
                "native" | "unknown" | "busy" => f.advance(31.0 * 60.0),
                _ => {}
            }
            if matches!(state, "native" | "unknown") {
                f.store
                    .raw()
                    .row("callback_state", next.as_str())
                    .put(json!({
                        "handle":next, "address":"codex:next", "host":HOST, "confirmed_until":
                            f.now() + if state == "native" { 390.0 } else { -1.0 }
                    }))
                    .unwrap();
            }
            let busy = || {
                Ok(if state == "busy" {
                    BTreeSet::from([next.clone()])
                } else {
                    BTreeSet::new()
                })
            };
            assert!(
                !f.store
                    .claim_unanswered_restore_checking(&pending, &busy)
                    .unwrap(),
                "{backend:?} {state}"
            );
            assert!(!reserved(&f, &pending), "{state}");
            assert!(
                owed_by(&f, &pending.handle)
                    .iter()
                    .any(|p| p.post.id == pending.post.id),
                "{state}"
            );
            // Once the same successor really is gone, the original request still has its try.
            f.advance(31.0 * 60.0);
            assert!(
                f.store.claim_unanswered_restore(&pending).unwrap(),
                "{state}"
            );
        }
    }
}

#[test]
fn unrelated_roles_and_gone_successors_do_not_block_and_own_running_work_does() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        for state in [
            "other-task",
            "reviewer",
            "lost",
            "released",
            "left",
            "own-busy",
            "other-busy",
        ] {
            let (f, pending) = setup();
            f.advance(31.0 * 60.0);
            let next = f.session("worker");
            match state {
                "other-task" => {
                    f.task("other");
                    f.store.claim("other", &next).unwrap();
                }
                "reviewer" => {
                    f.store.take_role(&pending.slug, &next, "reviewer").unwrap();
                }
                "lost" | "released" | "left" | "other-busy" => {
                    f.store.claim(&pending.slug, &next).unwrap();
                    f.store.move_to(&pending.slug, "blocked", &next).unwrap();
                    if state == "released" {
                        f.store
                            .release(&pending.slug, &next, Some("executor"))
                            .unwrap();
                    } else if state == "left" {
                        f.store.leave(&next).unwrap();
                    } else {
                        f.advance(31.0 * 60.0);
                    }
                }
                _ => {}
            }
            let busy = || {
                Ok(BTreeSet::from([if state == "own-busy" {
                    pending.handle.clone()
                } else {
                    "unrelated".into()
                }]))
            };
            assert_eq!(
                f.store
                    .claim_unanswered_restore_checking(&pending, &busy)
                    .unwrap(),
                state != "own-busy",
                "{backend:?} {state}"
            );
            assert_eq!(reserved(&f, &pending), state != "own-busy");
        }
    }
}

#[test]
fn reservation_rechecks_replacement_answer_session_and_native_changes_without_consuming_a_try() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        for change in [
            "replacement",
            "answer",
            "heartbeat",
            "native",
            "release",
            "unrelated",
        ] {
            let (f, pending) = setup();
            let next = replacement(&f, &pending);
            f.advance(31.0 * 60.0);
            let rows = answers::rows(&f.store, &pending.handle, pending.post.id).unwrap();
            let parents = answers::parents(&f.store, &rows).unwrap();
            let observed = successors::read(&f.store, &pending).unwrap().unwrap();
            assert!(!observed.active(&f.store, &pending, &BTreeSet::new()));
            let other = f.other();
            match change {
                "replacement" => {
                    let third = f.session("worker");
                    other.claim(&pending.slug, &third).unwrap();
                }
                "answer" => {
                    other
                        .say(&pending.slug, &pending.handle, "late original answer", None)
                        .unwrap();
                }
                "heartbeat" => other.touch(&next).unwrap(),
                "native" => {
                    other.raw().row("callback_state", next.as_str()).put(json!({"handle":next,"address":"codex:next","host":HOST,"confirmed_until":f.now()+390.0})).unwrap();
                }
                "release" => {
                    other
                        .release(&pending.slug, &next, Some("executor"))
                        .unwrap();
                }
                "unrelated" => {
                    f.task("unrelated");
                    let worker = f.session("worker");
                    other.claim("unrelated", &worker).unwrap();
                }
                _ => unreachable!(),
            }
            assert_eq!(
                f.store
                    .reserve_unanswered_restore_observed(&pending, &rows, &parents, Some(&observed))
                    .unwrap(),
                change == "unrelated",
                "{backend:?} {change}"
            );
            assert_eq!(reserved(&f, &pending), change == "unrelated", "{change}");
        }
    }
}

#[test]
fn a_replacement_arriving_during_the_last_probe_is_revalidated_by_the_reservation() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        let (f, pending) = setup();
        f.advance(31.0 * 60.0);
        let next = f.session("worker");
        let probed = Cell::new(false);
        let verifying = || {
            f.other().claim(&pending.slug, &next).unwrap();
            probed.set(true);
            Ok(BTreeSet::new())
        };
        assert!(
            !f.store
                .claim_unanswered_restore_checking(&pending, &verifying)
                .unwrap()
        );
        assert!(probed.get());
        assert!(!reserved(&f, &pending));
        f.advance(31.0 * 60.0);
        assert!(f.store.claim_unanswered_restore(&pending).unwrap());
    }
}

#[test]
fn successor_roles_are_read_through_the_task_index() {
    let (f, pending) = setup(Backend::SurrealDb);
    let plan = f
        .store
        .client()
        .query(
            &format!("EXPLAIN {};", sql::successor_roles()),
            json!({"slug":pending.slug}),
        )
        .unwrap();
    let plan = plan[0].as_str().unwrap();
    assert!(plan.contains("role_slug"), "{plan}");
    assert!(!plan.contains("TableScan"), "{plan}");
}

#[test]
fn native_registration_from_an_absent_row_and_confirmation_of_an_existing_row_refuse_stale_reservations()
 {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        for existing in [false, true] {
            let (f, pending) = setup();
            let next = replacement(&f, &pending);
            let elsewhere = f.elsewhere();
            let address = format!("codex:{THREAD}");
            if existing {
                elsewhere
                    .callback(&next, Some(&address), false, false)
                    .unwrap();
            }
            f.advance(31.0 * 60.0);
            let rows = answers::rows(&f.store, &pending.handle, pending.post.id).unwrap();
            let parents = answers::parents(&f.store, &rows).unwrap();
            let observed = successors::read(&f.store, &pending).unwrap().unwrap();
            assert!(!observed.active(&f.store, &pending, &BTreeSet::new()));
            assert_eq!(
                f.store
                    .raw()
                    .row("callback_state", next.as_str())
                    .exists()
                    .unwrap(),
                existing
            );
            if existing {
                let seen = elsewhere
                    .callback_presence(
                        Some(&crate::Presence {
                            threads: BTreeSet::from([THREAD.into()]),
                            ..Default::default()
                        }),
                        true,
                    )
                    .unwrap();
                assert!(seen.contains(&next));
            } else {
                elsewhere
                    .callback(&next, Some(&address), false, false)
                    .unwrap();
            }
            assert!(
                !f.store
                    .reserve_unanswered_restore_observed(&pending, &rows, &parents, Some(&observed))
                    .unwrap()
            );
            assert!(!reserved(&f, &pending));
            f.advance(31.0 * 60.0);
            assert!(f.store.claim_unanswered_restore(&pending).unwrap());
        }
    }
}

#[test]
fn recovery_fences_touch_only_the_observed_task_sessions_and_existing_native_row() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        let (f, pending) = setup();
        let next = replacement(&f, &pending);
        let address = format!("codex:{THREAD}");
        f.elsewhere()
            .callback(&next, Some(&address), false, false)
            .unwrap();
        f.advance(31.0 * 60.0);
        let observed = successors::read(&f.store, &pending).unwrap().unwrap();
        assert_eq!(observed.value["holders"].as_array().unwrap().len(), 1);
        assert_eq!(observed.value["holders"][0][0], next);
        let revision = |h: &str| {
            f.store.raw().row("session_revision", h).get().unwrap()["writes"]
                .as_i64()
                .unwrap_or(0)
        };
        let task_version = || {
            f.store
                .raw()
                .row("task", pending.slug.as_str())
                .get()
                .unwrap()["version"]
                .as_i64()
                .unwrap()
        };
        let native_revision = || {
            f.store
                .raw()
                .row("callback_state", next.as_str())
                .get()
                .unwrap()["recovery_revision"]
                .as_i64()
                .unwrap_or(0)
        };
        let before = (
            revision(&pending.handle),
            revision(&next),
            task_version(),
            native_revision(),
        );
        assert!(f.store.claim_unanswered_restore(&pending).unwrap());
        let after = (
            revision(&pending.handle),
            revision(&next),
            task_version(),
            native_revision(),
        );
        assert_eq!((after.0 - before.0, after.1 - before.1), (1, 1));
        let (task_writes, native_writes) = match backend {
            Backend::SurrealDb => (1, 1),
            Backend::PostgreSql => (0, 0),
        };
        assert_eq!(
            (after.2 - before.2, after.3 - before.3),
            (task_writes, native_writes)
        );
        eprintln!(
            "{backend:?}: successor observation: 1 indexed read, 1 holder; reservation: 2 session revision writes, {task_writes} task version write, {native_writes} native revision write (PostgreSQL locks task/native rows)"
        );
        // Existing registration writers retain the new, optional bookkeeping value, and callback
        // readers still decode the registration without knowing it.
        f.elsewhere()
            .callback(&next, Some(&address), false, false)
            .unwrap();
        assert_eq!(native_revision(), native_writes);
        assert!(f.store.callbacks().unwrap().contains_key(&next));
    }
}

#[test]
fn a_released_live_successors_projection_agrees_with_reservation_eligibility() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        let (f, pending) = setup();
        let next = replacement(&f, &pending);
        f.store.move_to(&pending.slug, "blocked", &next).unwrap();
        let busy = BTreeSet::from([next.clone()]);
        let before = owed_by(&f, &pending.handle).remove(0);
        assert!(before.active_successor(&busy));
        f.store
            .release(&pending.slug, &next, Some("executor"))
            .unwrap();
        f.task("other-work");
        f.store.claim("other-work", &next).unwrap();
        let after = owed_by(&f, &pending.handle).remove(0);
        let executor = after.executor.as_ref().unwrap();
        assert_eq!(executor.handle, next);
        assert_eq!(executor.health, Health::Live);
        assert!(!executor.held);
        assert!(!after.active_successor(&busy));
        assert_eq!(
            (after.post.id, &after.post.sender, &after.post.recipient),
            (
                pending.post.id,
                &pending.post.sender,
                &pending.post.recipient
            )
        );
        assert!(
            f.store
                .claim_unanswered_restore_checking(&after, &|| Ok(busy.clone()))
                .unwrap()
        );
    }
}
