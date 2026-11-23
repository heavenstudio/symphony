//! Owned scratch controls for the final recovery reservation and the sender-index bounds.

use super::*;
use crate::NewTask;
use crate::store_fixture::{Backend, Fixture, Seed};

#[path = "unanswered_reads.rs"]
mod reads;
#[path = "unanswered_successor_tests.rs"]
mod successor_tests;

/// Every post `handle` owes on the board as it is now.
fn owed_by(f: &Fixture, handle: &str) -> Vec<Pending> {
    let board = f.store.board().unwrap();
    let asked = BTreeSet::from([handle.to_string()]);
    f.store
        .pending_posts(&board, Asked::Handles(&asked))
        .unwrap()
}

fn setup() -> (Fixture, Pending) {
    let f = Fixture::new(Seed::default());
    f.task("work");
    let worker = f.session("worker");
    f.store.claim("work", &worker).unwrap();
    f.task_with(NewTask {
        parent: Some("work".into()),
        ..NewTask::new("different-name", "child", "owner")
    });
    f.store
        .say("work", "owner", "continue on child", Some(&worker))
        .unwrap();
    let pending = owed_by(&f, &worker).remove(0);
    (f, pending)
}

fn reserved(f: &Fixture, pending: &Pending) -> bool {
    f.store
        .raw()
        .row(
            "unanswered_restore",
            json!([pending.slug, pending.handle, pending.post.id]),
        )
        .exists()
        .unwrap()
}

#[test]
fn final_reservation_rechecks_late_descendant_answers_and_preserves_one_attempt() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        for reply_task in ["work", "different-name"] {
            let (f, pending) = setup();
            let rows = answers::rows(&f.store, &pending.handle, pending.post.id).unwrap();
            let parents = answers::parents(&f.store, &rows).unwrap();
            assert!(rows.is_empty());
            // A second store commits after the decision, immediately before the reservation transaction.
            f.other()
                .say(reply_task, &pending.handle, "done", None)
                .unwrap();
            assert_eq!(
                f.store.session(&pending.handle).unwrap().row.last_seen,
                pending.seen
            );
            assert!(
                !f.store
                    .reserve_unanswered_restore(&pending, &rows, &parents)
                    .unwrap()
            );
            assert!(!f.store.claim_unanswered_restore(&pending).unwrap());
            assert!(!reserved(&f, &pending));
            let request = f
                .store
                .say("work", "owner", "new request", Some(&pending.handle))
                .unwrap();
            // The reply answered the first post, on the task or under it.
            let owed = owed_by(&f, &pending.handle);
            assert_eq!(owed.len(), 1, "{reply_task}: {owed:?}");
            let next = owed.into_iter().next().unwrap();
            assert_eq!(next.post.id, request.id);
            assert!(f.store.claim_unanswered_restore(&next).unwrap());
            assert!(!f.other().claim_unanswered_restore(&next).unwrap());
        }
    }
}

#[test]
fn final_reservation_rechecks_ancestry_and_heartbeat_without_consuming_an_attempt() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        for change in ["parent", "heartbeat", "status"] {
            let (f, pending) = setup();
            f.task("unrelated");
            f.store
                .set_parent("different-name", Some("unrelated"), "owner")
                .unwrap();
            f.store
                .say("different-name", &pending.handle, "unrelated reply", None)
                .unwrap();
            let rows = answers::rows(&f.store, &pending.handle, pending.post.id).unwrap();
            let parents = answers::parents(&f.store, &rows).unwrap();
            assert!(!answers::answered(
                &rows,
                &pending.slug,
                &parents.iter().cloned().collect()
            ));
            match change {
                "parent" => {
                    f.other()
                        .set_parent("different-name", Some("work"), "owner")
                        .unwrap();
                }
                "heartbeat" => {
                    f.advance(1.0);
                    // The session-writing slice owns touch; this test changes its observed input.
                    f.other()
                        .raw()
                        .row("session", pending.handle.as_str())
                        .set(json!({"last_seen": f.now()}))
                        .unwrap();
                }
                "status" => {
                    f.other()
                        .raw()
                        .row("task", "work")
                        .set(json!({"status": "done"}))
                        .unwrap();
                }
                _ => unreachable!(),
            }
            assert!(
                !f.store
                    .reserve_unanswered_restore(&pending, &rows, &parents)
                    .unwrap(),
                "{change}"
            );
            assert!(!reserved(&f, &pending));
        }
    }
}

#[test]
fn recipient_suffix_uses_sender_kind_and_sequence_index_bounds() {
    let (f, pending) = setup(Backend::SurrealDb);
    let old = f
        .store
        .say("different-name", &pending.handle, "before cursor", None)
        .unwrap();
    let cursor = f
        .store
        .say("work", "owner", "cursor", Some(&pending.handle))
        .unwrap();
    f.store
        .say("different-name", "owner", "other sender", None)
        .unwrap();
    let answer = f
        .store
        .say("different-name", &pending.handle, "after cursor", None)
        .unwrap();
    assert!(old.id < cursor.id && cursor.id < answer.id);
    assert_eq!(
        answers::rows(&f.store, &pending.handle, cursor.id).unwrap(),
        [(answer.id, "different-name".into())]
    );
    for kind in ["msg", "verdict", "system"] {
        let vars = json!({"handle":pending.handle, "kind":kind, "post":cursor.id});
        let plan = f
            .store
            .client()
            .query(&format!("EXPLAIN {query};"), vars)
            .unwrap();
        let plan = plan[0].as_str().unwrap();
        eprintln!("{kind}: {plan}");
        assert!(plan.contains("message_sender"), "{plan}");
        assert!(!plan.contains("TableScan"), "{plan}");
        assert!(
            plan.contains(&format!(
                "access: ['{}', '{kind}'] MoreThan {}",
                pending.handle, cursor.id
            )),
            "{plan}"
        );
    }
}

#[test]
fn concurrent_reservations_allow_one_attempt_and_keep_it_after_reopening() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        let (f, pending) = setup();
        let stores = [f.other(), f.other()];
        let barrier = std::sync::Barrier::new(2);
        let answers = std::thread::scope(|scope| {
            let joins: Vec<_> = stores
                .iter()
                .map(|store| {
                    scope.spawn(|| {
                        barrier.wait();
                        store.claim_unanswered_restore(&pending)
                    })
                })
                .collect();
            joins
                .into_iter()
                .map(|join| join.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(
            answers
                .iter()
                .filter(|answer| matches!(answer, Ok(true)))
                .count(),
            1,
            "{backend:?}: {answers:?}"
        );
        for answer in answers {
            if let Err(error) = answer {
                assert_eq!(
                    error,
                    Error::Db(
                        "board changed during unanswered recovery; automatic attempt not repeated"
                            .into()
                    )
                );
            }
        }
        assert_eq!(
            f.store.raw().table("unanswered_restore").count().unwrap(),
            1
        );
        assert!(!f.other().claim_unanswered_restore(&pending).unwrap());
        f.store
            .record_unanswered_restore("work", "restored once")
            .unwrap();
        let notice = f.store.thread("work").unwrap().pop().unwrap();
        assert_eq!(
            (
                notice.sender.as_str(),
                notice.recipient.as_deref(),
                notice.text.as_str()
            ),
            ("board", Some("@orchestrator"), "restored once")
        );
    }
}

#[test]
fn descendant_latest_maps_keep_two_parent_alignment_full_posts_and_equal_clock_cutoffs() {
    for backend in [Backend::SurrealDb, Backend::PostgreSql] {
        let (f, first) = setup();
        let handle = first.handle.clone();
        f.task("work-two");
        f.store.claim("work-two", &handle).unwrap();
        f.task_with(NewTask {
            parent: Some("work-two".into()),
            ..NewTask::new("other-child", "child", "owner")
        });
        let second = f
            .store
            .say("work-two", "owner", "Second request", Some(&handle))
            .unwrap();
        let check = |posts: Vec<(&str, Message)>| {
            let board = f.store.board().unwrap();
            let asked = BTreeSet::from([handle.clone()]);
            let actual = f
                .store
                .pending_posts(&board, Asked::Handles(&asked))
                .unwrap();
            // This bounded lookup deliberately returns Message metadata without body text.
            let expected: BTreeMap<_, _> = posts
                .into_iter()
                .map(|(slug, mut post)| {
                    post.text.clear();
                    (slug, post)
                })
                .collect();
            let ordered: Vec<_> = board
                .tasks
                .iter()
                .filter_map(|t| {
                    expected
                        .get(t.slug.as_str())
                        .map(|post| (t.slug.clone(), post.clone()))
                })
                .collect();
            assert_eq!(
                actual
                    .iter()
                    .map(|p| (p.slug.clone(), p.post.clone()))
                    .collect::<Vec<_>>(),
                ordered
            );
            for p in actual {
                assert_eq!(p.handle, handle);
                assert_eq!(p.role, first.role);
                assert_eq!(p.health, first.health);
                assert_eq!(p.seen, first.seen);
                assert_eq!(p.status, "in_progress");
                let executor = p.executor.unwrap();
                assert_eq!(executor.handle, handle);
                assert!(executor.held);
                assert_eq!(executor.health, first.health);
            }
        };
        check(vec![
            ("work", first.post.clone()),
            ("work-two", second.clone()),
        ]);
        let reply = f
            .store
            .say("different-name", &handle, "First child answer", None)
            .unwrap();
        assert!(reply.id > first.post.id);
        assert_eq!(reply.ts, first.post.ts);
        check(vec![("work-two", second.clone())]);
        let newer = f
            .store
            .say("work", "owner", "New parent request", Some(&handle))
            .unwrap();
        assert!(newer.id > reply.id);
        assert_eq!(newer.ts, reply.ts);
        check(vec![("work", newer.clone()), ("work-two", second.clone())]);
        f.store
            .say("other-child", &handle, "Second child answer", None)
            .unwrap();
        check(vec![("work", newer)]);
        f.store
            .say("different-name", &handle, "Newest child answer", None)
            .unwrap();
        check(vec![]);
    }
}
