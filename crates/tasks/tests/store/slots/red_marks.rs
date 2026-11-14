//! A red's mark against writes that read the board before it changed: an owner's cancel that read
//! no task, never closing a red filed after it; and a red's mark lifted by its filer, a red left
//! open as ordinary work, which counts it and the work under it from then as such work counts (in
//! review, none), never refused, a start that read it marked decided again.

use super::moves::{RED, UNDER, WAIT, beside_a_red};
use super::{EIGHT, F, K, O, W, capped, red, reserve_hint, sessions, with};
use crate::common::{Fixture, refused};
use std::sync::{Mutex, mpsc};
use std::thread;
use symphony_board::NewTask;

#[test]
fn an_owners_cancel_that_read_no_task_is_refused_there_never_closing_a_red_filed_after_it() {
    // O holds seven features, with two more to claim.
    let mut seed = sessions();
    let led = (EIGHT[..7].iter()).chain(&[("work/next", "ready"), ("work/more", "ready")]);
    for (slug, status) in led {
        seed = with(seed, slug, status, None, Some(O));
    }
    let f = Fixture::new(seed);
    let (read, owner_read) = mpsc::channel();
    let (go, owner_go) = mpsc::channel();
    let (finished, owner_finished) = mpsc::channel();
    let owner_go = Mutex::new(owner_go);
    // The owner's cancel of the red reads no such task, then waits.
    let owner = f.slow(move || {
        read.send(()).unwrap();
        owner_go.lock().unwrap().recv_timeout(WAIT).unwrap();
    });
    let cancel = thread::spawn(move || {
        let cancelled = owner.move_to(RED, "cancelled", "owner");
        finished.send(()).unwrap();
        cancelled
    });
    owner_read.recv_timeout(WAIT).unwrap();
    // Its filer files it, O orchestrating it, and work under it starts at the cap as red work. A
    // feature's claim reads seven, and before it writes the cancel goes on.
    let store = capped(&f, 8);
    red(&store, RED, "main");
    store.take_role(RED, O, "orchestrator").unwrap();
    let under = NewTask {
        parent: Some(RED.into()),
        ..NewTask::new(UNDER, "Under the red", "owner")
    };
    store.create(&under).unwrap();
    store.claim(UNDER, W).unwrap();
    let owner_finished = Mutex::new(owner_finished);
    let claiming = f
        .slow(move || {
            go.send(()).unwrap();
            owner_finished.lock().unwrap().recv_timeout(WAIT).unwrap();
        })
        .capping(Some(8));
    let claimed = claiming.claim("work/next", K).unwrap();
    // The cancel judged no task, so it closes none: the red stays open, the work under it red work,
    // and O holds eight, the claim among them.
    assert_eq!(refused(cancel.join().unwrap()), format!("no task '{RED}'"));
    assert_eq!(claimed.status, "in_progress", "{claimed:?}");
    let filed = store.get(RED).unwrap();
    assert!(filed.status == "ready" && filed.red(), "{filed:?}");
    let said = refused(store.claim("work/more", K));
    assert!(
        said.starts_with("work/more cannot start: olive-orch holds 8 open tasks")
            && said.ends_with(&format!(
                "not counted, as red work takes no slot: fix/under (in_progress){}",
                reserve_hint("work/more")
            )),
        "{said}"
    );
}

#[test]
fn a_red_whose_mark_its_filer_lifts_counts_with_the_work_under_it_never_refused_and_once() {
    // O at the cap of eight beside the red, back in progress on its reviewer's changes, with work in
    // progress under it.
    let f = beside_a_red(8, true);
    let store = capped(&f, 8);
    let back = store.verdict(RED, "lark", "changes", "not yet").unwrap();
    assert!(back.red() && back.status == "in_progress", "{back:?}");
    assert_eq!(
        refused(store.unmark_red(RED, O)),
        format!(
            "only a red's filer marks a task red, a session of kind filer as the tools that file reds register; '{O}' is not one"
        )
    );
    let kept = store.get(RED).unwrap();
    assert!(kept.red(), "{kept:?}");
    // Its filer lifts the mark past the cap, never refused: the red and the work under it count.
    let lifted = store.unmark_red(RED, F).unwrap();
    assert!(
        !lifted.red() && lifted.status == "in_progress",
        "{lifted:?}"
    );
    let said = refused(store.claim("work/next", K));
    assert!(
        said.starts_with("work/next cannot start: olive-orch holds 9 open tasks")
            && said.contains("tests/main-red-race (in_progress) with fix/under (in_progress)")
            && !said.contains("not counted"),
        "{said}"
    );
    // Lifted once: a red lifted already, and a task never marked, are left as they are.
    let thread = store.thread(RED).unwrap();
    store.unmark_red(RED, F).unwrap();
    let never = store.unmark_red("work/a", F).unwrap();
    assert!(!never.red(), "{never:?}");
    assert_eq!(store.thread(RED).unwrap().len(), thread.len());
    let lines = (thread.iter()).filter(|m| m.text.contains("lifted its red mark"));
    assert_eq!(lines.count(), 1, "{thread:?}");
    assert_eq!(
        refused(store.unmark_red("work/none", F)),
        "no task 'work/none'"
    );
}

#[test]
fn a_start_that_read_a_red_marked_before_its_mark_was_lifted_is_decided_again_counted() {
    // O holds seven beside the red, back in progress on its reviewer's changes, which takes no slot:
    // a claim reads seven, then waits.
    let f = beside_a_red(7, false);
    f.store.verdict(RED, "lark", "changes", "not yet").unwrap();
    let (read, claim_read) = mpsc::channel();
    let (go, claim_go) = mpsc::channel();
    let claim_go = Mutex::new(claim_go);
    let claiming = f
        .slow(move || {
            read.send(()).unwrap();
            claim_go.lock().unwrap().recv_timeout(WAIT).unwrap();
        })
        .capping(Some(8));
    let claim = thread::spawn(move || claiming.claim("work/next", K));
    claim_read.recv_timeout(WAIT).unwrap();
    // Its filer lifts the mark before the claim writes: the red counts, so O holds eight.
    f.store.unmark_red(RED, F).unwrap();
    go.send(()).unwrap();
    let said = refused(claim.join().unwrap());
    assert!(
        said.starts_with("work/next cannot start: olive-orch holds 8 open tasks")
            && said.contains("tests/main-red-race (in_progress)"),
        "{said}"
    );
    assert_eq!(f.store.get("work/next").unwrap().status, "ready");
}
