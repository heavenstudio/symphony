//! Owned scratch controls for the final recovery reservation and the sender-index bounds.

use super::*;
use crate::NewTask;
use crate::store_fixture::Fixture;

#[path = "unanswered_batch.rs"]
mod batch;

fn setup() -> (Fixture, Pending) {
    let f = Fixture::empty();
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
    let pending = f
        .store
        .pending_posts(&[f.store.get("work").unwrap()])
        .unwrap()
        .remove(0);
    (f, pending)
}

fn reserved(f: &Fixture, pending: &Pending) -> bool {
    f.store
        .client()
        .query(
            "RETURN record::exists(type::record('unanswered_restore', [$slug, $handle, $post]));",
            json!({"slug":pending.slug, "handle":pending.handle, "post":pending.post.id}),
        )
        .unwrap()[0]
        .as_bool()
        .unwrap()
}

#[test]
fn final_reservation_rechecks_late_descendant_answers_and_preserves_one_attempt() {
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
        f.store
            .say("work", "owner", "new request", Some(&pending.handle))
            .unwrap();
        let next = f
            .store
            .pending_posts(&[f.store.get("work").unwrap()])
            .unwrap()
            .remove(0);
        assert!(f.store.claim_unanswered_restore(&next).unwrap());
        assert!(!f.other().claim_unanswered_restore(&next).unwrap());
    }
}

#[test]
fn final_reservation_rechecks_ancestry_and_heartbeat_without_consuming_an_attempt() {
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
                f.other().touch(&pending.handle).unwrap();
            }
            "status" => {
                f.other()
                    .client()
                    .query("UPDATE task:work SET status = 'done';", json!({}))
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

#[test]
fn recipient_suffix_uses_sender_kind_and_sequence_index_bounds() {
    let (f, pending) = setup();
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
        let query = answers::select();
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
