//! Dependency completion resumes only explicit task blocks, on both stores and native delivery; a
//! block a server judged over ([`symphony_board::Store::resume_judged`]) ends the same way, once, and
//! a move in between wins.

use crate::common::{Backend, Fixture, Seed};
use serde_json::json;
use symphony_board::{On, waits_on::Blocked};

const TARGET: &str = "sim/after";
const FIRST: &str = "sim/first";

fn fixture(backend: Backend, status: &str) -> Fixture {
    with_executor(backend, status, false)
}

fn with_executor(backend: Backend, status: &str, executor: bool) -> Fixture {
    let seed = Seed::default()
        .session_row(symphony_board::rows::SessionRow {
            kind: "orchestrator".into(),
            ..crate::common::session("orch")
        })
        .session("worker")
        .session("reviewer")
        .task(TARGET, status)
        .task(FIRST, "review")
        .role(TARGET, "orch", "orchestrator")
        .role(FIRST, "worker", "executor")
        .role(FIRST, "reviewer", "reviewer")
        .link(TARGET, "dep", FIRST);
    Fixture::on(
        backend,
        if executor {
            seed.role(TARGET, "worker", "executor")
        } else {
            seed
        },
    )
}

fn block(f: &Fixture, on: &str) {
    f.store
        .move_on(
            TARGET,
            "blocked",
            "orch",
            None,
            Some(On {
                thing: on,
                why: None,
            }),
        )
        .unwrap();
}

fn complete(f: &Fixture, slug: &str) {
    f.store
        .landed(slug, "worker", "landed on main", true, &["review", "done"])
        .unwrap();
}

fn status(f: &Fixture) -> String {
    f.store.get(TARGET).unwrap().status
}

/// The block the target holds, as a server reads it before it judges it over.
fn blocked(f: &Fixture) -> Blocked {
    Blocked::of(&f.store.get(TARGET).unwrap()).expect("the target is blocked")
}

on_both! {
    fn completion_resumes_the_recorded_stage_and_clears_its_block_once(backend: Backend) {
        for (from, executor, back) in [
            ("concept", false, "concept"), ("design", false, "design"),
            ("ready", false, "ready"), ("in_progress", false, "ready"),
            ("in_progress", true, "in_progress"), ("review", false, "review"),
        ] {
            let f = with_executor(backend, from, executor);
            block(&f, "task:sim/first");
            complete(&f, FIRST);
            let task = f.store.get(TARGET).unwrap();
            assert_eq!(task.status, back, "from {from}, executor {executor}");
            assert!(Blocked::of(&task).is_none());
            assert!(task.fields.get("_board").is_none_or(|fields| !fields.contains_key("blocked")));
            let expected = format!("board moved it blocked → {back}");
            assert_eq!(f.said(TARGET).iter().filter(|line| **line == expected).count(), 1);
            complete(&f, FIRST);
            f.store.dispatch_callbacks().unwrap();
            assert_eq!(f.said(TARGET).iter().filter(|line| **line == expected).count(), 1);
        }
    }

    fn every_prerequisite_and_the_named_task_must_be_done(backend: Backend) {
        let f = fixture(backend, "ready");
        f.task("sim/second");
        f.store.raw().row("task", "sim/second").set(json!({"status":"review"})).unwrap();
        f.store.change_depends(TARGET, &["sim/second".into()], &[], "orch").unwrap();
        block(&f, "task:sim/first");
        complete(&f, FIRST);
        assert_eq!(status(&f), "blocked");
        complete(&f, "sim/second");
        assert_eq!(status(&f), "ready");
        // A task block also works without a dependency link.
        let f = fixture(backend, "ready");
        f.store.change_depends(TARGET, &[], &[FIRST.into()], "orch").unwrap();
        block(&f, "task:sim/first");
        complete(&f, FIRST);
        assert_eq!(status(&f), "ready");
    }

    fn independent_unknown_and_cancelled_blocks_stay_blocked(backend: Backend) {
        for on in [Some("decision:layout"), Some("check:tests"), Some("parked"), None] {
            let f = fixture(backend, "ready");
            if let Some(on) = on { block(&f, on); }
            else { f.store.move_to(TARGET, "blocked", "orch").unwrap(); }
            complete(&f, FIRST);
            assert_eq!(status(&f), "blocked", "{on:?}");
        }
        let f = fixture(backend, "ready");
        block(&f, "task:sim/first");
        f.store.move_to(FIRST, "cancelled", "owner").unwrap();
        assert_eq!(status(&f), "blocked");
    }

    fn an_approve_satisfies_the_existing_done_contract_before_landing(backend: Backend) {
        let f = fixture(backend, "ready");
        block(&f, "task:sim/first");
        f.store.verdict(FIRST, "reviewer", "approve", "tip").unwrap();
        assert_eq!(status(&f), "ready");
    }

    fn dispatch_recovers_a_committed_completion_and_notifies_the_current_orchestrator_once(backend: Backend) {
        let f = fixture(backend, "ready");
        let successor = f.session("orchestrator");
        block(&f, "task:sim/first");
        let former = crate::dispatch_later::listen(&f.store, "orch", &format!("{}-old", f.store.database().database()));
        let next = crate::dispatch_later::listen(&f.store, &successor, &format!("{}-next", f.store.database().database()));
        f.store.hand_over(TARGET, "orch", &successor).unwrap();
        let _ = former.drain(&f, "worker", "orch");
        let _ = next.drain(&f, "worker", &successor);
        // A writer exited after committing the completion, before reconciliation/delivery.
        f.store.raw().row("task", FIRST).set(json!({"status":"done"})).unwrap();
        f.other().dispatch_callbacks().unwrap();
        assert_eq!(status(&f), "ready");
        let heard = next.drain(&f, "worker", &successor);
        assert_eq!(heard.iter().filter(|line| line.as_str() == "STATUS sim/after blocked → ready").count(), 1, "{heard:?}");
        assert!(heard.iter().any(|line| line.as_str() == "ready_unstaffed p2 sim/after — Task sim/after"), "{heard:?}");
        assert!(!former.drain(&f, "worker", "orch").iter().any(|line| line.starts_with("STATUS sim/after")));
        f.other().dispatch_callbacks().unwrap();
        assert!(next.drain(&f, "worker", &successor).is_empty());
    }

    fn missing_tasks_invalid_stages_and_stage_less_blocks_are_handled_conservatively(backend: Backend) {
        for (from, executor, back) in [
            (None, false, "ready"), (None, true, "in_progress"),
            (Some("blocked"), false, "blocked"), (Some("done"), false, "blocked"),
        ] {
            let f = with_executor(backend, "ready", executor);
            block(&f, "task:sim/first");
            f.store.raw().row("task", TARGET).set(json!({"fields._board.blocked.from":from})).unwrap();
            complete(&f, FIRST);
            assert_eq!(status(&f), back, "{from:?}, executor {executor}");
        }
        let f = fixture(backend, "ready");
        block(&f, "task:sim/first");
        f.store.raw().row("task", FIRST).delete().unwrap();
        f.store.dispatch_callbacks().unwrap();
        assert_eq!(status(&f), "blocked");
    }

    fn a_completion_already_done_when_the_block_is_named_is_reflected_in_the_answer(backend: Backend) {
        let f = fixture(backend, "ready");
        complete(&f, FIRST);
        let answer = f.store.move_on(TARGET, "blocked", "orch", None,
            Some(On { thing:"task:sim/first", why:None })).unwrap();
        assert_eq!(answer.status, "ready");
        assert!(Blocked::of(&answer).is_none());
    }

    fn resuming_waits_for_a_free_slot_and_a_hold_keeps_its_slot(backend: Backend) {
        for held in [false, true] {
            let f = with_executor(backend, "in_progress", true);
            let capped = f.other().capping(Some(1));
            if held {
                capped.hold_blocked_on(TARGET, "orch", Some(On { thing:"task:sim/first", why:None })).unwrap();
            } else {
                block(&f, "task:sim/first");
                f.task("sim/other");
                f.store.take_role("sim/other", "orch", "orchestrator").unwrap();
                f.store.claim("sim/other", "worker").unwrap();
            }
            // Simulate the interrupted writer so only the capped store reconciles.
            f.store.raw().row("task", FIRST).set(json!({"status":"done"})).unwrap();
            capped.dispatch_callbacks().unwrap();
            assert_eq!(status(&f), if held {"in_progress"} else {"blocked"});
            if !held {
                f.store.move_to("sim/other", "review", "worker").unwrap();
                capped.dispatch_callbacks().unwrap();
                assert_eq!(status(&f), "in_progress");
            }
        }
    }

    fn a_block_changed_after_the_read_is_decided_again(backend: Backend) {
        let f = fixture(backend, "ready");
        block(&f, "task:sim/first");
        f.store.raw().row("task", FIRST).set(json!({"status":"done"})).unwrap();
        let other = f.other();
        let late = f.late(move || {
            other.move_on(TARGET, "blocked", "orch", None,
                Some(On { thing:"decision:layout", why:None })).unwrap();
        });
        late.dispatch_callbacks().unwrap();
        assert_eq!(status(&f), "blocked");
        assert_eq!(Blocked::of(&f.store.get(TARGET).unwrap()).unwrap().on, "decision:layout");
        assert!(!f.said(TARGET).iter().any(|line| line == "board moved it blocked → ready"));
    }

    fn a_dependency_reopened_after_its_completion_read_is_decided_again(backend: Backend) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let f = fixture(backend, "ready");
        block(&f, "decision:layout");
        f.store.verdict(FIRST, "reviewer", "approve", "tip").unwrap();
        // Retarget without invoking the writer's reconciliation, then race the completion read.
        f.store.raw().row("task", TARGET).set(json!({"fields._board.blocked.on":"task:sim/first"})).unwrap();
        let calls = AtomicUsize::new(0);
        let other = f.other();
        let late = symphony_board::Store::opening_on(
            f.other_database(), Box::new(move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 1 {
                    other.move_to(FIRST, "review", "reviewer").unwrap();
                }
                crate::common::NOW
            }), Box::new(|_| false), crate::common::HOST, &symphony_board::Opening::write(),
        ).unwrap();
        late.dispatch_callbacks().unwrap();
        assert_eq!(f.store.get(FIRST).unwrap().status, "review");
        assert_eq!(status(&f), "blocked");
        assert!(!f.said(TARGET).iter().any(|line| line == "board moved it blocked → ready"));
    }

    fn completion_queues_native_delivery_before_a_detached_dispatcher_runs(backend: Backend) {
        let f = fixture(backend, "ready");
        block(&f, "task:sim/first");
        let listener = crate::dispatch_later::listen(&f.store, "orch", &format!("{}-deferred", f.store.database().database()));
        let _ = listener.drain(&f, "worker", "orch");
        let deferred = crate::dispatch_later::deferring(&f);
        for _ in 0..2 {
            deferred.landed(FIRST, "worker", "landed on main", true, &["review", "done"]).unwrap();
        }
        assert_eq!(status(&f), "ready");
        f.other().dispatch_detached().unwrap();
        let heard = listener.drain(&f, "worker", "orch");
        assert_eq!(heard.iter().filter(|line| line.as_str() == "STATUS sim/after blocked → ready").count(), 1, "{heard:?}");
        assert_eq!(heard.iter().filter(|line| line.as_str() == "ready_unstaffed p2 sim/after — Task sim/after").count(), 1, "{heard:?}");
        f.other().dispatch_detached().unwrap();
        assert!(listener.drain(&f, "worker", "orch").is_empty());
    }

    fn a_judged_block_returns_to_its_recorded_stage_with_its_line_once(backend: Backend) {
        for (from, executor, back) in [
            ("ready", false, "ready"), ("in_progress", false, "ready"),
            ("in_progress", true, "in_progress"), ("review", false, "review"),
        ] {
            let f = with_executor(backend, from, executor);
            block(&f, "decision:layout");
            let judged = blocked(&f);
            let why = "the owner decided layout (approve)";
            let moved = f.store.resume_judged(TARGET, &judged, why).unwrap();
            assert_eq!(moved.as_deref(), Some(back), "from {from}, executor {executor}");
            let task = f.store.get(TARGET).unwrap();
            assert_eq!(task.status, back);
            assert!(Blocked::of(&task).is_none());
            assert!(task.fields.get("_board").is_none_or(|fields| !fields.contains_key("blocked")));
            let line = format!("unblocked: {why}; back to {back}");
            // Judged again, as the next tick would before it read the move: nothing moves or is said.
            assert_eq!(f.store.resume_judged(TARGET, &judged, why).unwrap(), None);
            f.store.dispatch_callbacks().unwrap();
            assert_eq!(f.said(TARGET).iter().filter(|said| **said == line).count(), 1, "{:?}", f.said(TARGET));
        }
    }

    fn a_judged_block_the_owner_moved_or_blocked_anew_since_the_read_stays_as_he_left_it(backend: Backend) {
        let why = "the owner decided layout (reject)";
        // Moved by the owner since: it stays where the owner put it.
        let f = fixture(backend, "ready");
        block(&f, "decision:layout");
        let judged = blocked(&f);
        f.store.move_to(TARGET, "review", "owner").unwrap();
        assert_eq!(f.store.resume_judged(TARGET, &judged, why).unwrap(), None);
        assert_eq!(status(&f), "review");
        // Blocked anew since, on the same card or another thing: the new block stands.
        for again in ["decision:layout", "check:owner", "parked"] {
            let f = fixture(backend, "ready");
            block(&f, "decision:layout");
            let judged = blocked(&f);
            f.advance(60.0);
            f.store.move_to(TARGET, "review", "owner").unwrap();
            block(&f, again);
            assert_eq!(f.store.resume_judged(TARGET, &judged, why).unwrap(), None, "{again}");
            assert_eq!(status(&f), "blocked", "{again}");
            assert_eq!(blocked(&f).on, again);
            assert!(!f.said(TARGET).iter().any(|line| line.starts_with("unblocked: ")), "{again}");
        }
        // Gone, or not blocked at all: nothing to end.
        let f = fixture(backend, "ready");
        assert_eq!(f.store.resume_judged("sim/missing", &Blocked {
            on: "decision:layout".into(), why: None, by: "orch".into(), at: crate::common::NOW, from: None,
        }, why).unwrap(), None);
    }

    fn a_judged_block_moved_in_the_middle_of_its_end_is_decided_again(backend: Backend) {
        let f = fixture(backend, "ready");
        block(&f, "decision:layout");
        let judged = blocked(&f);
        let other = f.other();
        let late = f.late(move || {
            other.move_to(TARGET, "review", "owner").unwrap();
        });
        assert_eq!(late.resume_judged(TARGET, &judged, "the owner decided layout (approve)").unwrap(), None);
        assert_eq!(status(&f), "review");
        assert!(!f.said(TARGET).iter().any(|line| line.starts_with("unblocked: ")));
    }

    fn a_judged_end_waits_for_a_free_slot(backend: Backend) {
        let f = with_executor(backend, "in_progress", true);
        let capped = f.other().capping(Some(1));
        block(&f, "decision:layout");
        f.task("sim/other");
        f.store.take_role("sim/other", "orch", "orchestrator").unwrap();
        f.store.claim("sim/other", "worker").unwrap();
        let judged = blocked(&f);
        let why = "the owner decided layout (approve)";
        assert_eq!(capped.resume_judged(TARGET, &judged, why).unwrap(), None);
        assert_eq!(status(&f), "blocked");
        f.store.move_to("sim/other", "review", "worker").unwrap();
        assert_eq!(capped.resume_judged(TARGET, &judged, why).unwrap().as_deref(), Some("in_progress"));
    }

    fn a_judged_end_reaches_the_orchestrator_as_a_status_line_and_the_replay_reads_it(backend: Backend) {
        let f = fixture(backend, "review");
        f.store.hold_blocked_on(TARGET, "orch", Some(On { thing: "check:owner", why: None })).unwrap();
        let listener = crate::dispatch_later::listen(&f.store, "orch", &format!("{}-judged", f.store.database().database()));
        let _ = listener.drain(&f, "worker", "orch");
        f.advance(30.0);
        let back = f.store.resume_judged(TARGET, &blocked(&f), "the owner approved 1a2b3c4").unwrap();
        assert_eq!(back.as_deref(), Some("review"));
        f.other().dispatch_callbacks().unwrap();
        let heard = listener.drain(&f, "worker", "orch");
        assert_eq!(heard.iter().filter(|line| line.as_str() == "STATUS sim/after blocked → review").count(), 1, "{heard:?}");
        // When it went into review is the end of its block, read back from its thread.
        let entered = f.store.entered_review(&[TARGET]).unwrap();
        assert_eq!(entered.get(TARGET), Some(&(crate::common::NOW + 30.0)));
    }

    fn the_board_tells_a_tasks_orchestrator_once_per_key(backend: Backend) {
        let f = fixture(backend, "ready");
        let line = "the owner on card layout [2026-11-20 04:20] (docs/DECISIONS.md#layout), deciding nothing, so it stays blocked on decision:layout: wider?";
        for _ in 0..2 {
            f.store.tell_orchestrator_once(TARGET, line, "decided:layout:a").unwrap();
        }
        let thread = f.store.thread(TARGET).unwrap();
        let told: Vec<_> = thread.iter().filter(|m| m.text == line).collect();
        assert_eq!(told.len(), 1, "{thread:?}");
        assert_eq!((told[0].sender.as_str(), told[0].recipient.as_deref()), ("board", Some("@orchestrator")));
        assert!(f.store.inbox("orch").unwrap().iter().any(|m| m.text == line));
        // Another key with the same words takes the line the board said already; other words are
        // a line of their own.
        f.store.tell_orchestrator_once(TARGET, line, "decided:layout:b").unwrap();
        assert_eq!(f.said(TARGET).iter().filter(|said| **said == line).count(), 1);
        f.store.tell_orchestrator_once(TARGET, "the owner on card layout: narrower", "decided:layout:b2").unwrap();
        assert_eq!(f.said(TARGET).iter().filter(|said| said.starts_with("the owner on card layout")).count(), 2);
        assert!(f.store.tell_orchestrator_once(TARGET, "  ", "decided:layout:c").is_err());
    }
}
