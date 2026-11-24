//! A write that hands its delivery to a detached dispatcher still queues what each recipient hears
//! before it answers, read against the board as the write left it: a later write before the
//! dispatcher runs (a hand-over, a move) neither reroutes nor folds it, a callback registered
//! before a write hears that write, and a recipient a later write's observation reached first is
//! never told the older board back. Each case is the same with the delivery made in the write. A
//! write takes no recipient's lease to read an inbox no message since could reach.

use super::common::Fixture;
use super::dispatch_later::{deferring, ids, listen};
use serde_json::{Value, json};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use symphony_board::Store;

/// The store whose writes the case makes: one that delivers in the write, or one that leaves the
/// delivery to a dispatcher the case runs after its last write.
fn writing(f: &Fixture, deferred: bool) -> Store {
    if deferred { deferring(f) } else { f.other() }
}

#[test]
fn a_post_to_a_role_reaches_its_holder_at_the_post_though_it_was_handed_over_before_delivery() {
    for deferred in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let (former, successor) = (f.session("orchestrator"), f.session("orchestrator"));
        let sender = f.session("worker");
        f.task("tools/target");
        f.task("tools/other");
        f.store
            .take_role("tools/target", &former, "orchestrator")
            .unwrap();
        f.store
            .take_role("tools/other", &successor, "orchestrator")
            .unwrap();
        let database = f.store.database().database().to_string();
        let to_former = listen(&f.store, &former, &format!("{database}-former"));
        let to_successor = listen(&f.store, &successor, &format!("{database}-successor"));
        let _ = to_former.drain(&f, &sender, &former);
        let _ = to_successor.drain(&f, &sender, &successor);
        let board = writing(&f, deferred);
        let id = board
            .say(
                "tools/target",
                &sender,
                "for the holder now",
                Some("@orchestrator"),
            )
            .unwrap()
            .id;
        board
            .hand_over("tools/target", &former, &successor)
            .unwrap();
        f.other().dispatch_detached().unwrap();
        let (heard, heard_after) = (
            to_former.drain(&f, &sender, &former),
            to_successor.drain(&f, &sender, &successor),
        );
        assert_eq!(
            ids(&heard).get(&id),
            Some(&1),
            "deferred={deferred}: the holder at the post hears it once: {heard:?}"
        );
        assert!(
            !ids(&heard_after).contains_key(&id),
            "deferred={deferred}: its successor does not: {heard_after:?}"
        );
    }
}

#[test]
fn a_claim_and_a_move_before_delivery_are_each_heard_in_their_order() {
    for deferred in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let (observer, worker, sender) = (
            f.session("orchestrator"),
            f.session("worker"),
            f.session("worker"),
        );
        f.task("tools/history");
        f.store
            .take_role("tools/history", &observer, "orchestrator")
            .unwrap();
        let listener = listen(
            &f.store,
            &observer,
            &format!("{}-history", f.store.database().database()),
        );
        let _ = listener.drain(&f, &sender, &observer);
        let board = writing(&f, deferred);
        board.claim("tools/history", &worker).unwrap();
        board.move_to("tools/history", "review", &worker).unwrap();
        f.other().dispatch_detached().unwrap();
        let heard = listener.drain(&f, &sender, &observer);
        let claimed = heard.iter().position(|line| {
            line.starts_with("ROLE tools/history ") && line.ends_with(", in_progress")
        });
        let moved = heard.iter().position(|line| {
            line.starts_with("STATUS tools/history ") && line.ends_with("→ review")
        });
        assert!(
            claimed.is_some() && moved.is_some() && claimed < moved,
            "deferred={deferred}: the claim, then the move: {heard:?}"
        );
        f.other().dispatch_detached().unwrap();
        let again = listener.drain(&f, &sender, &observer);
        assert!(
            again.is_empty(),
            "deferred={deferred}: said once: {again:?}"
        );
    }
}

#[test]
fn a_verdict_after_a_registration_reaches_it_though_no_dispatcher_ran_between() {
    for deferred in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let (executor, reviewer) = (f.session("worker"), f.session("reviewer"));
        f.task("tools/verdict");
        f.store.claim("tools/verdict", &executor).unwrap();
        f.store
            .take_role("tools/verdict", &reviewer, "reviewer")
            .unwrap();
        f.store
            .move_to("tools/verdict", "review", &executor)
            .unwrap();
        let board = writing(&f, deferred);
        // Registered through the deferring store too: its first look at the board is taken before
        // the registration answers, so what comes after it is news, not history.
        let listener = listen(
            &board,
            &executor,
            &format!("{}-verdict", f.store.database().database()),
        );
        board
            .verdict("tools/verdict", &reviewer, "changes", "once more")
            .unwrap();
        let thread = f.store.thread("tools/verdict").unwrap();
        let id = (thread.iter())
            .filter(|message| message.kind == "verdict")
            .map(|message| message.id)
            .max()
            .unwrap_or_else(|| panic!("the thread holds the verdict: {thread:?}"));
        f.other().dispatch_detached().unwrap();
        let heard = listener.drain(&f, &reviewer, &executor);
        assert_eq!(
            ids(&heard).get(&id),
            Some(&1),
            "deferred={deferred}: the verdict is heard once: {heard:?}"
        );
    }
}

/// Two orchestrators' sessions in their handles' order, the order a pass visits recipients in. The
/// first holds no role: the look older dispatchers keep of an orchestrator takes in the staffing of
/// the whole board, so a claim anywhere is news to its observation, which reads its inbox.
fn two_orchestrators(f: &Fixture) -> (String, String) {
    let (one, other) = (f.session("orchestrator"), f.session("orchestrator"));
    if one < other {
        (one, other)
    } else {
        (other, one)
    }
}

/// What `handle`'s recorded observation says its inbox was read up to.
fn inbox_cursor(store: &Store, handle: &str) -> Value {
    let state = store.raw().row("callback_state", handle);
    state.field("inbox_cursor").unwrap()
}

#[test]
fn a_write_whose_later_recipient_heard_a_newer_board_never_says_a_status_back() {
    for deferred in [false, true] {
        let f = Fixture::new(crate::common::Seed::default());
        let (first, orchestrator) = two_orchestrators(&f);
        let (worker, sender) = (f.session("worker"), f.session("worker"));
        f.task("tools/raced");
        f.store
            .take_role("tools/raced", &orchestrator, "orchestrator")
            .unwrap();
        let database = f.store.database().database().to_string();
        let _to_first = listen(&f.store, &first, &format!("{database}-first"));
        let listener = listen(&f.store, &orchestrator, &format!("{database}-raced"));
        let _ = listener.drain(&f, &sender, &orchestrator);
        // Once the claim's own observation has passed the first recipient, a move commits and its
        // write observes the orchestrator before the claim's observation reaches it.
        let before = inbox_cursor(&f.store, &first);
        let (mover, moved) = (f.other(), Arc::new(AtomicBool::new(false)));
        let (racing, mover_handle) = (Arc::clone(&moved), worker.clone());
        let busy = f.busy(move || {
            if !racing.load(Ordering::SeqCst)
                && inbox_cursor(&mover, &first) != before
                && !racing.swap(true, Ordering::SeqCst)
            {
                mover
                    .move_to("tools/raced", "review", &mover_handle)
                    .unwrap();
            }
        });
        let board = if deferred {
            busy.dispatching_by(Box::new(|| true))
        } else {
            busy
        };
        board.claim("tools/raced", &worker).unwrap();
        assert!(
            moved.load(Ordering::SeqCst),
            "deferred={deferred}: the move raced the claim's observation"
        );
        f.other().dispatch_detached().unwrap();
        let heard = listener.drain(&f, &sender, &orchestrator);
        // A role line ends with the task's status as it heard it, a status line with the new one.
        let about: Vec<&String> = (heard.iter())
            .filter(|line| {
                line.starts_with("ROLE tools/raced ") || line.starts_with("STATUS tools/raced ")
            })
            .collect();
        assert!(
            about.last().is_some_and(|line| line.ends_with("review"))
                && !about.iter().any(|line| line.ends_with("in_progress")),
            "deferred={deferred}: the move is the last word, never undone: {heard:?}"
        );
    }
}

/// `handle`'s lease revision, which each taking of its lease moves.
fn lease_revision(store: &Store, handle: &str) -> Value {
    let lease = store.raw().row("callback_lease", handle);
    lease.field("revision").unwrap()
}

/// Whether `handle`'s outbox holds message `id`'s line, delivered or not.
fn queued(store: &Store, handle: &str, id: i64) -> bool {
    let key = format!("message-{id}");
    let line = store.raw().row("callback_event", json!([handle, key]));
    line.exists().unwrap()
}

#[test]
fn a_write_queues_its_post_for_its_recipient_and_leaves_an_inbox_it_cannot_reach_to_the_dispatcher()
{
    let f = Fixture::new(crate::common::Seed::default());
    let (addressed, bystander, sender) = (
        f.session("worker"),
        f.session("worker"),
        f.session("worker"),
    );
    for (slug, holder) in [
        ("tools/addressed", &addressed),
        ("tools/bystander", &bystander),
    ] {
        f.task(slug);
        f.store.claim(slug, holder).unwrap();
    }
    let database = f.store.database().database().to_string();
    let to_addressed = listen(&f.store, &addressed, &format!("{database}-addressed"));
    let to_bystander = listen(&f.store, &bystander, &format!("{database}-bystander"));
    let _ = to_addressed.drain(&f, &sender, &addressed);
    let _ = to_bystander.drain(&f, &sender, &bystander);
    let (cursor, revision) = (
        inbox_cursor(&f.store, &bystander),
        lease_revision(&f.store, &bystander),
    );
    let id = deferring(&f)
        .say(
            "tools/addressed",
            &sender,
            "for you alone",
            Some(&addressed),
        )
        .unwrap()
        .id;
    assert!(
        queued(&f.store, &addressed, id),
        "the write queued its post for the handle it is to before it answered"
    );
    assert_eq!(
        (
            inbox_cursor(&f.store, &bystander),
            lease_revision(&f.store, &bystander)
        ),
        (cursor, revision),
        "the write took no lease to read an inbox its post cannot reach"
    );
    f.other().dispatch_detached().unwrap();
    assert_eq!(
        inbox_cursor(&f.store, &bystander),
        json!(id),
        "the dispatcher moved the bystander's cursor past the post"
    );
    let (heard, overheard) = (
        to_addressed.drain(&f, &sender, &addressed),
        to_bystander.drain(&f, &sender, &bystander),
    );
    assert_eq!(ids(&heard).get(&id), Some(&1), "heard once: {heard:?}");
    assert!(
        !ids(&overheard).contains_key(&id),
        "the bystander never hears it: {overheard:?}"
    );
}
