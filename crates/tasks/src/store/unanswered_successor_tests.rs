//! Reservation eligibility and stale observations on both stores, without starting an agent.

use super::*;
use crate::store_fixture::HOST;
use std::cell::Cell;

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
            let (f, pending) = setup(backend);
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
            let (f, pending) = setup(backend);
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
            let (f, pending) = setup(backend);
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
        let (f, pending) = setup(backend);
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
