//! Every open task has an orchestrator: the board gives the role to a task filed with none, to the
//! work a leave would leave with none, to a task moved out from under its orchestrator's task (at
//! the move, to the one it had) or reopened with none, and to every open task nobody orchestrates
//! at the next read of the whole board that shows one (a look of `tasks wait`, the server's clock,
//! a detached dispatcher, the idle sweep), with no sweep needed. A give goes to the orchestrator the
//! task's own story names (the task above it, the filer's work, the filer's parent, the one it
//! had), else to the orchestrator that stays (live past the first live line, and listening or
//! holding a task) holding fewest; with none that stays, the task is marked waiting and the first
//! to come to stay is given it. A session that only registered is given nothing by the board's own
//! choice. A release that would leave open work with no orchestrator is refused.

use super::common::{Fixture, HOST, NOW, Seed, refused};
use super::orchestrators::go_quiet_but;
use symphony_board::cli::Env;
use symphony_board::health::LIVE_SECONDS;
use symphony_board::{NewSession, NewTask};

/// What one look of `tasks wait` printed for `handle`, line by line.
fn read(f: &Fixture, handle: &str) -> Vec<String> {
    let mut said: Vec<String> = Vec::new();
    let mut print = |text: &str| {
        said.extend(text.lines().map(str::to_owned));
        Ok(())
    };
    (f.store)
        .collect_callbacks(handle, &Env::of_process(), &mut print)
        .unwrap();
    said
}

/// Of `lines`, the role lines: not the PULL digest's.
fn roles(lines: Vec<String>) -> Vec<String> {
    (lines.into_iter())
        .filter(|line| line.starts_with("GIVEN ") || line.starts_with("ROLE "))
        .collect()
}

fn orchestrators_of(f: &Fixture, slug: &str) -> Vec<String> {
    let task = f.store.get(slug).unwrap();
    let held = task.holders("orchestrator");
    held.into_iter().map(str::to_owned).collect()
}

/// When `slug` was marked waiting for an orchestrator, if it is.
fn waiting(f: &Fixture, slug: &str) -> Option<f64> {
    symphony_board::orchestration::waiting(&f.store.get(slug).unwrap().fields)
}

/// Every one of `handles` past the first live line, each beating then: what the board's own choice
/// asks of an orchestrator besides listening or holding a task (`orchestration::Pool`). The
/// sessions not named go quiet for as long.
fn settle(f: &Fixture, handles: &[&str]) {
    f.advance(LIVE_SECONDS + 1.0);
    for handle in handles {
        f.store.touch(handle).unwrap();
    }
}

/// `handles` on the waiting list a look of `tasks wait` puts its reader on, as a waiting
/// orchestrator is, with no look of their own made (which would give what waits).
fn listen(f: &Fixture, handles: &[&str]) {
    let raw = f.store.raw();
    for handle in handles {
        let state = raw.row("callback_state", *handle);
        state.put(serde_json::json!({"handle": handle})).unwrap();
    }
    let chat = raw.row("callback_state", "chat");
    let mut waiting = match chat.field("waiting").unwrap() {
        serde_json::Value::Array(waiting) => waiting,
        _ => Vec::new(),
    };
    for handle in handles {
        if !waiting.iter().any(|was| was == handle) {
            waiting.push(serde_json::json!(handle));
        }
    }
    chat.put(serde_json::json!({"waiting": waiting})).unwrap();
}

/// Whether a reader of the whole board, as the server's clock reads it, gave anything.
fn tick(f: &Fixture) -> bool {
    f.store.give_if_due(&f.store.clock_board().unwrap())
}

/// `slug`, filed by `by` through the store, and the orchestrator the board gave it.
fn filed(f: &Fixture, slug: &str, by: &str) -> Option<String> {
    f.task_with(NewTask::new(slug, "Follow-up", by));
    f.store.give_filed(slug, by, None).unwrap()
}

#[test]
fn a_due_give_reports_errors_to_retrying_callers_and_preserves_the_boolean_wrapper() {
    let f = Fixture::new(Seed::default().task("tools/pending", "ready"));
    let old = symphony_board::Store::opening_on(
        f.other_database(),
        Box::new(|| NOW),
        Box::new(|_| false),
        HOST,
        &symphony_board::Opening::read().at(symphony_board::Schema::this_build().before()),
    )
    .unwrap();
    let snapshot = old.clock_board().unwrap();
    let error = old.try_give_if_due(&snapshot).unwrap_err().to_string();
    assert!(error.contains("nothing was written"), "{error}");
    assert!(
        !old.give_if_due(&snapshot),
        "existing callers still receive false on error"
    );
    assert!(
        f.store.try_give_if_due(&snapshot).unwrap(),
        "a healthy pass marks the unowned task waiting"
    );
    assert!(
        !f.store
            .try_give_if_due(&f.store.clock_board().unwrap())
            .unwrap(),
        "no more give is due"
    );
    let empty = Fixture::new(Seed::default());
    assert!(
        !empty
            .store
            .try_give_if_due(&empty.store.clock_board().unwrap())
            .unwrap()
    );
}

#[test]
fn a_workers_follow_up_goes_to_the_orchestrator_of_the_task_it_works_on() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    let worker = f.session("worker");
    f.task("tools/feature");
    f.task("tools/other");
    f.store
        .take_role("tools/feature", &amber, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/other", &amber, "orchestrator")
        .unwrap();
    f.store.claim("tools/feature", &worker).unwrap();
    // Rule 4 alone would pick cobalt, who holds none: the work the filer is on decides first.
    assert_eq!(filed(&f, "tools/follow-up", &worker), Some(amber.clone()));
    assert_eq!(orchestrators_of(&f, "tools/follow-up"), [amber.as_str()]);
    assert_eq!(
        f.said("tools/follow-up"),
        [
            format!("created by {worker}"),
            format!("{amber} was given orchestrator (filed by {worker})"),
        ]
    );
    // A reviewer's follow-up the same, and one filed under a held task is the tree's: no line.
    let reviewer = f.session("reviewer");
    f.store.move_to("tools/feature", "review", &worker).unwrap();
    f.store
        .take_role("tools/feature", &reviewer, "reviewer")
        .unwrap();
    assert_eq!(filed(&f, "tools/found", &reviewer), Some(amber.clone()));
    f.task_with(NewTask {
        parent: Some("tools/feature".into()),
        ..NewTask::new("tools/feature/part", "Part", &worker)
    });
    let got = f
        .store
        .give_filed("tools/feature/part", &worker, None)
        .unwrap();
    assert_eq!(got, Some(amber.clone()));
    assert_eq!(
        f.said("tools/feature/part"),
        [format!("created by {worker}")]
    );
    assert!(orchestrators_of(&f, "tools/feature/part").is_empty());
    assert!(
        orchestrators_of(&f, "tools/follow-up")
            .iter()
            .all(|h| *h != cobalt)
    );
}

#[test]
fn an_orchestrator_filing_takes_the_role_and_one_named_for_takes_it_instead() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    assert_eq!(filed(&f, "tools/mine", &amber), Some(amber.clone()));
    assert_eq!(
        f.said("tools/mine").last().unwrap(),
        &format!("{amber} joined as orchestrator")
    );
    f.task_with(NewTask::new("tools/theirs", "Theirs", &amber));
    let got = f
        .store
        .give_filed("tools/theirs", &amber, Some(&cobalt))
        .unwrap();
    assert_eq!(got, Some(cobalt.clone()));
    assert_eq!(orchestrators_of(&f, "tools/theirs"), [cobalt.as_str()]);
    assert_eq!(
        f.said("tools/theirs").last().unwrap(),
        &format!("{cobalt} was given orchestrator (filed by {amber})")
    );
}

#[test]
fn a_release_under_a_task_an_orchestrator_holds_above_it_goes() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    f.task("tools/a");
    f.task_with(NewTask {
        parent: Some("tools/a".into()),
        ..NewTask::new("tools/a/part", "Part", "owner")
    });
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    // cobalt orchestrating the part below amber's task: amber holds it above, so it goes, and the
    // part is amber's by the tree.
    let cobalt = f.session("orchestrator");
    f.store
        .take_role("tools/a/part", &cobalt, "orchestrator")
        .unwrap();
    let part = f
        .store
        .release("tools/a/part", &cobalt, Some("orchestrator"))
        .unwrap();
    assert!(part.holders("orchestrator").is_empty());
    assert!(
        f.store.give_unowned(None).unwrap().is_empty(),
        "nothing to give"
    );
}

#[test]
fn a_task_moved_out_from_under_its_orchestrators_task_stays_with_that_orchestrator() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/a");
    f.task_with(NewTask {
        parent: Some("tools/a".into()),
        ..NewTask::new("tools/a/part", "Part", "owner")
    });
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    // Moved to the top by anyone, it stays amber's, given at the move in the same write: rule 4
    // would have picked cobalt, holding none.
    let moved = f.store.set_parent("tools/a/part", None, &cobalt).unwrap();
    assert_eq!(moved.holders("orchestrator"), [amber.as_str()]);
    let thread = f.store.thread("tools/a/part").unwrap();
    let said: Vec<(String, String)> = (thread.into_iter().rev().take(2).rev())
        .map(|m| (m.sender, m.text))
        .collect();
    let moved_out =
        format!("{amber} was given orchestrator (tools/a/part moved out from under tools/a)");
    assert_eq!(said[1], (cobalt.clone(), moved_out), "{said:?}");
    assert!(said[0].1.starts_with(&cobalt), "{said:?}");
    assert!(f.store.give_unowned(None).unwrap().is_empty());
    // Moved back under amber's task, it holds its own role: the move is the one line.
    let lines = f.said("tools/a/part").len();
    f.store
        .set_parent("tools/a/part", Some("tools/a"), &amber)
        .unwrap();
    assert_eq!(f.said("tools/a/part").len(), lines + 1);
    // Moved under another held task, the tree gives it: no give.
    f.task("tools/b");
    f.store
        .take_role("tools/b", &cobalt, "orchestrator")
        .unwrap();
    f.task_with(NewTask {
        parent: Some("tools/b".into()),
        ..NewTask::new("tools/b/part", "Part", "owner")
    });
    let lines = f.said("tools/b/part").len();
    f.store
        .set_parent("tools/b/part", Some("tools/a"), "owner")
        .unwrap();
    assert!(orchestrators_of(&f, "tools/b/part").is_empty());
    assert_eq!(f.said("tools/b/part").len(), lines + 1);
    // The owner's move out from under a held task gives it too, as the board.
    f.store.set_parent("tools/b/part", None, "owner").unwrap();
    assert_eq!(orchestrators_of(&f, "tools/b/part"), [amber.as_str()]);
    let line = f.store.thread("tools/b/part").unwrap().pop().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.text),
        (
            "board",
            format!("{amber} was given orchestrator (tools/b/part moved out from under tools/a)")
        )
    );
}

#[test]
fn a_finished_task_moved_out_with_open_work_below_keeps_that_work_with_its_orchestrator() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    f.task("tools/a");
    f.task_with(NewTask {
        parent: Some("tools/a".into()),
        ..NewTask::new("tools/a/done", "Done", "owner")
    });
    f.task_with(NewTask {
        parent: Some("tools/a/done".into()),
        ..NewTask::new("tools/a/done/open", "Open", "owner")
    });
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    f.store
        .move_to("tools/a/done", "cancelled", &amber)
        .unwrap();
    // The role goes on the moved task, the top of what moved, and the open part below is amber's
    // by the tree.
    f.store.set_parent("tools/a/done", None, &amber).unwrap();
    assert_eq!(orchestrators_of(&f, "tools/a/done"), [amber.as_str()]);
    assert!(orchestrators_of(&f, "tools/a/done/open").is_empty());
    assert!(f.store.give_unowned(None).unwrap().is_empty());
    // With nothing open at or below it, a task moves out with no give.
    f.task_with(NewTask {
        parent: Some("tools/a".into()),
        ..NewTask::new("tools/a/closed", "Closed", "owner")
    });
    f.store
        .move_to("tools/a/closed", "cancelled", &amber)
        .unwrap();
    f.store.set_parent("tools/a/closed", None, &amber).unwrap();
    assert!(orchestrators_of(&f, "tools/a/closed").is_empty());
}

#[test]
fn a_subagents_task_goes_to_the_orchestrator_that_registered_it() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/held");
    f.store
        .take_role("tools/held", &amber, "orchestrator")
        .unwrap();
    let child = f.session_with(NewSession {
        parent: Some(amber.clone()),
        ..NewSession::new("worker")
    });
    // Working on nothing, the parent decides, though cobalt holds fewer.
    assert_eq!(filed(&f, "tools/finding", &child), Some(amber.clone()));
    // A parent that is not live, or no orchestrator, falls to rule 4, among those that stay.
    settle(&f, &[&amber, &cobalt]);
    listen(&f, &[&cobalt]);
    let chat = f.session("chat");
    let grandchild = f.session_with(NewSession {
        parent: Some(chat),
        ..NewSession::new("worker")
    });
    assert_eq!(
        filed(&f, "tools/elsewhere", &grandchild),
        Some(cobalt.clone())
    );
}

#[test]
fn a_task_filed_by_no_orchestrators_session_goes_to_the_orchestrator_that_stays_holding_fewest() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/a");
    f.task("tools/b");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/b", &amber, "orchestrator")
        .unwrap();
    settle(&f, &[&amber, &cobalt]);
    listen(&f, &[&cobalt]);
    // The owner's task: cobalt holds none, then one, so the second goes to it too (1 < 2). The
    // give is the board's, not a word of the owner's.
    assert_eq!(filed(&f, "tools/c", "owner"), Some(cobalt.clone()));
    assert_eq!(filed(&f, "tools/d", "owner"), Some(cobalt.clone()));
    assert_eq!(
        f.said("tools/c").last().unwrap(),
        &format!("{cobalt} was given orchestrator (filed by owner)")
    );
    assert_eq!(f.store.thread("tools/c").unwrap()[1].sender, "board");
    // Then a tie, two each: the lesser handle.
    let first = amber.clone().min(cobalt.clone());
    assert_eq!(filed(&f, "tools/e", "owner"), Some(first));
    // A stale orchestrator is given nothing, and a filer gone (a red's, which leaves) is the board's.
    f.advance(symphony_board::health::LIVE_SECONDS + 1.0);
    f.store.touch(&amber).unwrap();
    let filer = f.session("filer");
    f.task_with(NewTask::new("tools/red-ish", "Red", &filer));
    f.store.leave(&filer).unwrap();
    let got = f.store.give_filed("tools/red-ish", &filer, None).unwrap();
    assert_eq!(got, Some(amber.clone()));
    assert_eq!(f.store.thread("tools/red-ish").unwrap()[1].sender, "board");
}

#[test]
fn with_no_orchestrator_that_stays_a_task_waits_and_a_lone_new_one_is_given_it_once_it_waits() {
    let f = Fixture::new(Seed::default());
    f.session("orchestrator");
    go_quiet_but(&f, &[]);
    assert_eq!(filed(&f, "tools/loose", "owner"), None);
    let marked = waiting(&f, "tools/loose").expect("marked waiting");
    assert_eq!(marked, f.now());
    // No line on its thread: the mark is the record.
    assert_eq!(f.said("tools/loose"), ["created by owner"]);
    // A sweep with none that stays marks nothing again.
    f.advance(60.0);
    let swept = f.store.give_unowned(None).unwrap();
    assert!(swept.gave.is_empty());
    assert_eq!(swept.waiting, ["tools/loose"]);
    assert!(swept.marked.is_empty(), "marked once");
    assert_eq!(waiting(&f, "tools/loose"), Some(marked));
    // A new orchestrator waiting at once is given nothing yet: it has not stayed.
    let next = f.session("orchestrator");
    assert!(roles(read(&f, &next)).is_empty());
    assert!(!tick(&f));
    assert!(orchestrators_of(&f, "tools/loose").is_empty());
    // Waiting past the first live line, its look gives it what waited: one line on the thread,
    // the board's, the mark gone, and one GIVEN line to it.
    settle(&f, &[&next]);
    assert_eq!(
        roles(read(&f, &next)),
        ["GIVEN 1 open task is yours to orchestrate: tools/loose"]
    );
    assert_eq!(orchestrators_of(&f, "tools/loose"), [next.as_str()]);
    assert_eq!(waiting(&f, "tools/loose"), None);
    let line = f.store.thread("tools/loose").unwrap().pop().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.text),
        (
            "board",
            format!("{next} was given orchestrator (it waited for an orchestrator)")
        )
    );
    // The next look, and the clock, have nothing more to give or say.
    assert!(roles(read(&f, &next)).is_empty());
    assert!(!tick(&f));
    assert_eq!(f.said("tools/loose").len(), 2);
}

#[test]
fn a_just_registered_orchestrator_beside_one_that_stays_is_given_nothing() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    f.task("tools/a");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    settle(&f, &[&amber]);
    // cobalt only registered: it holds none, so rule 4 alone would pick it, but it has not stayed.
    let cobalt = f.session("orchestrator");
    listen(&f, &[&cobalt]);
    assert_eq!(filed(&f, "tools/b", "owner"), Some(amber.clone()));
    // A leaver's work goes to amber too, not to the fresh one, and the clock finds nothing left.
    let leaver = f.session("orchestrator");
    f.task("tools/left");
    f.store
        .take_role("tools/left", &leaver, "orchestrator")
        .unwrap();
    f.store.leave(&leaver).unwrap();
    assert_eq!(orchestrators_of(&f, "tools/left"), [amber.as_str()]);
    assert!(!tick(&f));
    // cobalt, beating and listening, has stayed once past the first live line: holding none, it
    // is the next.
    settle(&f, &[&amber, &cobalt]);
    assert_eq!(filed(&f, "tools/c", "owner"), Some(cobalt.clone()));
    assert_eq!(orchestrators_of(&f, "tools/b"), [amber.as_str()]);
}

#[test]
fn a_throwaway_orchestrator_that_registers_and_vanishes_moves_nothing() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    for slug in ["tools/a", "tools/b"] {
        f.task(slug);
        f.store.take_role(slug, &amber, "orchestrator").unwrap();
    }
    go_quiet_but(&f, &[]);
    // amber lost, the clock marks its work waiting: no line on either thread.
    assert!(tick(&f));
    let marks = (waiting(&f, "tools/a"), waiting(&f, "tools/b"));
    assert!(marks.0.is_some() && marks.1.is_some(), "{marks:?}");
    let lines = (f.said("tools/a").len(), f.said("tools/b").len());
    // A session registers as an orchestrator, beats a while, and goes.
    let throwaway = f.session("orchestrator");
    assert!(!tick(&f));
    f.advance(30.0);
    f.store.touch(&throwaway).unwrap();
    assert!(!tick(&f));
    go_quiet_but(&f, &[]);
    assert!(!tick(&f));
    // Nothing moved: the work waits as it did, marked when it was, and nobody else holds it.
    assert_eq!((waiting(&f, "tools/a"), waiting(&f, "tools/b")), marks);
    assert_eq!((f.said("tools/a").len(), f.said("tools/b").len()), lines);
    for slug in ["tools/a", "tools/b"] {
        assert_eq!(orchestrators_of(&f, slug), [amber.as_str()], "{slug}");
    }
}

#[test]
fn a_lost_holders_open_work_is_given_at_the_next_wait_look_with_no_sweep_run() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/mine");
    f.task("tools/quiet");
    f.store
        .take_role("tools/mine", &cobalt, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/quiet", &amber, "orchestrator")
        .unwrap();
    // amber executes it too, so gone ten minutes it is lost, holding claimable work.
    f.store.claim("tools/quiet", &amber).unwrap();
    read(&f, &cobalt);
    go_quiet_but(&f, &[&cobalt]);
    let health = f.health(&amber);
    assert_eq!(health.as_str(), "lost");
    // No `tasks reap`, no `give_unowned`: cobalt's own look gives it, from the gone holder.
    assert_eq!(
        roles(read(&f, &cobalt)),
        ["GIVEN 1 open task is yours to orchestrate: tools/quiet"]
    );
    assert_eq!(orchestrators_of(&f, "tools/quiet"), [cobalt.as_str()]);
    let line = f.store.thread("tools/quiet").unwrap().pop().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.text),
        (
            "board",
            format!(
                "{cobalt} was given orchestrator from {amber} ({})",
                health.as_str()
            )
        )
    );
    // Once given, every reader finds nothing to do: one line per give.
    let lines = f.said("tools/quiet").len();
    assert!(!tick(&f));
    assert!(roles(read(&f, &cobalt)).is_empty());
    assert_eq!(f.said("tools/quiet").len(), lines);
}

#[test]
fn a_detached_dispatchers_pass_gives_a_lost_holders_work_to_an_orchestrator_on_a_channel() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    let worker = f.session("worker");
    f.task("tools/mine");
    f.task("tools/quiet");
    f.store
        .take_role("tools/mine", &cobalt, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/quiet", &amber, "orchestrator")
        .unwrap();
    f.store.claim("tools/quiet", &amber).unwrap();
    // cobalt listens on a channel alone: no wait, no monitor.
    let ear = crate::quiet_callbacks::listen(&f, &cobalt);
    go_quiet_but(&f, &[&cobalt, &worker]);
    // A plain store's delivering pass (a library's write, the server's own dispatch) gives nothing.
    f.store.dispatch_callbacks().unwrap();
    assert_eq!(orchestrators_of(&f, "tools/quiet"), [amber.as_str()]);
    // A detached dispatcher's pass, which a command-line write starts, gives it, and cobalt hears
    // the one GIVEN line on its channel.
    f.other().giving().dispatch_callbacks().unwrap();
    assert_eq!(orchestrators_of(&f, "tools/quiet"), [cobalt.as_str()]);
    let heard = ear.drain(&f, &worker, &cobalt);
    let told = "GIVEN 1 open task is yours to orchestrate: tools/quiet".to_string();
    assert!(heard.contains(&told), "{heard:?}");
    assert_eq!(heard.iter().filter(|l| l.starts_with("GIVEN ")).count(), 1);
}

#[test]
fn the_clocks_read_gives_a_lost_holders_work_to_one_that_holds_a_task() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/mine");
    f.task("tools/quiet");
    f.store
        .take_role("tools/mine", &cobalt, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/quiet", &amber, "orchestrator")
        .unwrap();
    // cobalt never waits: holding a task while it beats is staying.
    go_quiet_but(&f, &[&cobalt]);
    assert!(tick(&f));
    assert_eq!(orchestrators_of(&f, "tools/quiet"), [cobalt.as_str()]);
    assert!(!tick(&f));
}

#[test]
fn when_one_of_two_orchestrators_leaves_the_other_is_given_its_open_work_in_the_same_write() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    let worker = f.session("worker");
    for slug in ["tools/a", "tools/b", "tools/done", "tools/cobalts"] {
        f.task(slug);
    }
    f.task_with(NewTask {
        parent: Some("tools/b".into()),
        ..NewTask::new("tools/b/part", "Part", "owner")
    });
    for slug in ["tools/a", "tools/b", "tools/done"] {
        f.store.take_role(slug, &amber, "orchestrator").unwrap();
    }
    f.store
        .take_role("tools/cobalts", &cobalt, "orchestrator")
        .unwrap();
    f.store.claim("tools/a", &worker).unwrap();
    f.store.move_to("tools/done", "cancelled", &amber).unwrap();
    settle(&f, &[&amber, &cobalt, &worker]);
    read(&f, &cobalt);
    f.store.leave(&amber).unwrap();
    for slug in ["tools/a", "tools/b"] {
        assert_eq!(orchestrators_of(&f, slug), [cobalt.as_str()], "{slug}");
        assert_eq!(
            f.said(slug).last().unwrap(),
            &format!("{cobalt} was given orchestrator from {amber} (left)"),
            "{slug}"
        );
    }
    // The part below is cobalt's by the tree, and the cancelled task is given nothing.
    assert!(orchestrators_of(&f, "tools/b/part").is_empty());
    assert!(orchestrators_of(&f, "tools/done").is_empty());
    // Work in progress keeps its executor and status.
    let a = f.store.get("tools/a").unwrap();
    assert_eq!(
        (a.status.as_str(), a.holders("executor")),
        ("in_progress", vec![worker.as_str()])
    );
    // cobalt hears one GIVEN line, not a ROLE line a task.
    assert_eq!(
        roles(read(&f, &cobalt)),
        ["GIVEN 2 open tasks are yours to orchestrate: tools/a, tools/b"]
    );
}

#[test]
fn the_last_orchestrator_leaving_leaves_its_open_work_waiting() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    f.task("tools/a");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    f.store.leave(&amber).unwrap();
    assert!(orchestrators_of(&f, "tools/a").is_empty());
    assert_eq!(waiting(&f, "tools/a"), Some(f.now()));
    assert_eq!(
        f.said("tools/a").last().unwrap(),
        &format!("{amber} released every role (left)")
    );
}

#[test]
fn the_idle_sweep_gives_a_quiet_orchestrators_open_work_to_one_that_stays() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/a");
    f.task("tools/quiet");
    f.store
        .take_role("tools/a", &cobalt, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/quiet", &amber, "orchestrator")
        .unwrap();
    read(&f, &cobalt);
    go_quiet_but(&f, &[&cobalt]);
    let health = f.health(&amber);
    assert!(!health.holds(), "{health:?}");
    let would = f.store.would_give_unowned().unwrap();
    assert_eq!(would.gave, [("tools/quiet".to_string(), cobalt.clone())]);
    assert_eq!(
        orchestrators_of(&f, "tools/quiet"),
        [amber.as_str()],
        "a dry run writes nothing"
    );
    let given = f.store.give_unowned(None).unwrap();
    assert_eq!(given, would);
    // The quiet holder's role goes with the give.
    assert_eq!(orchestrators_of(&f, "tools/quiet"), [cobalt.as_str()]);
    let line = f.store.thread("tools/quiet").unwrap().pop().unwrap();
    assert_eq!(
        (line.sender.as_str(), line.text),
        (
            "board",
            format!(
                "{cobalt} was given orchestrator from {amber} ({})",
                health.as_str()
            )
        )
    );
    assert_eq!(
        roles(read(&f, &cobalt)),
        ["GIVEN 1 open task is yours to orchestrate: tools/quiet"]
    );
    // Nothing is left to give: a second sweep writes nothing.
    let lines = f.said("tools/quiet").len();
    assert!(f.store.give_unowned(None).unwrap().is_empty());
    assert_eq!(f.said("tools/quiet").len(), lines);
}

#[test]
fn an_orchestrator_quiet_then_back_is_given_what_waited_once_it_waits() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    f.task("tools/a");
    f.task("tools/b");
    go_quiet_but(&f, &[]);
    let swept = f.store.give_unowned(None).unwrap();
    assert_eq!(swept.marked, ["tools/a", "tools/b"]);
    // Back, beating but neither waiting nor holding a task, it is not one that stays yet.
    f.store.touch(&amber).unwrap();
    assert!(!tick(&f));
    // Its first look waits, so it stays: the look gives it both, the board's lines.
    read(&f, &amber);
    assert!(waiting(&f, "tools/a").is_none() && waiting(&f, "tools/b").is_none());
    assert_eq!(orchestrators_of(&f, "tools/a"), [amber.as_str()]);
    assert_eq!(orchestrators_of(&f, "tools/b"), [amber.as_str()]);
    assert_eq!(
        f.store.thread("tools/a").unwrap().pop().unwrap().sender,
        "board"
    );
}

#[test]
fn letting_the_orchestrator_role_go_is_refused_while_it_leaves_open_work_with_none() {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    let board = f.store.board_command();
    f.task("tools/a");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    settle(&f, &[&amber, &cobalt]);
    listen(&f, &[&cobalt]);
    assert_eq!(
        refused(f.store.release("tools/a", &amber, Some("orchestrator"))),
        format!(
            "{amber} cannot let go of orchestrator on tools/a: tools/a would have no orchestrator; \
             hand it over instead: {board} handover tools/a {cobalt}"
        )
    );
    // Every role at once, holding another: the refusal names how to let that one go.
    f.store.claim("tools/a", &amber).unwrap();
    assert_eq!(
        refused(f.store.release("tools/a", &amber, None)),
        format!(
            "{amber} cannot let go of orchestrator on tools/a: tools/a would have no orchestrator; \
             hand it over instead: {board} handover tools/a {cobalt}; executor goes alone with: \
             {board} release tools/a --role executor"
        )
    );
    f.store
        .release("tools/a", &amber, Some("executor"))
        .unwrap();
    // With no other orchestrator that stays, it says so.
    go_quiet_but(&f, &[&amber]);
    assert_eq!(
        refused(f.store.release("tools/a", &amber, Some("orchestrator"))),
        format!(
            "{amber} cannot let go of orchestrator on tools/a: tools/a would have no orchestrator, \
             and no other orchestrator that stays and holds fewer open tasks than the limit is \
             there to take it: it stays {amber}'s (`{board} leave` leaves it waiting for the next \
             orchestrator)"
        )
    );
    // Finished, it still keeps the open work below it: that goes first.
    f.task_with(NewTask {
        parent: Some("tools/a".into()),
        ..NewTask::new("tools/a/part", "Part", "owner")
    });
    f.store.move_to("tools/a", "cancelled", &amber).unwrap();
    assert_eq!(
        refused(f.store.release("tools/a", &amber, Some("orchestrator"))),
        format!(
            "{amber} cannot let go of orchestrator on tools/a: tools/a/part would have no \
             orchestrator, and no other orchestrator that stays and holds fewer open tasks than \
             the limit is there to take it: it stays {amber}'s (`{board} leave` leaves it waiting \
             for the next orchestrator)"
        )
    );
    f.store
        .move_to("tools/a/part", "cancelled", &amber)
        .unwrap();
    let task = f
        .store
        .release("tools/a", &amber, Some("orchestrator"))
        .unwrap();
    assert!(task.holders("orchestrator").is_empty());
}

#[test]
fn an_approved_task_reopened_after_its_orchestrator_left_is_given_one_again() {
    let f = Fixture::new(Seed::default());
    let (amber, exe, rev) = (
        f.session("orchestrator"),
        f.session("worker"),
        f.session("reviewer"),
    );
    f.task("sim/jump");
    f.store
        .take_role("sim/jump", &amber, "orchestrator")
        .unwrap();
    f.store.claim("sim/jump", &exe).unwrap();
    f.store.move_to("sim/jump", "review", &exe).unwrap();
    f.store.take_role("sim/jump", &rev, "reviewer").unwrap();
    f.store
        .verdict("sim/jump", &rev, "approve", "good")
        .unwrap();
    f.store.leave(&amber).unwrap();
    let cobalt = f.session("orchestrator");
    settle(&f, &[&cobalt, &exe]);
    listen(&f, &[&cobalt]);
    let task = f.store.move_to("sim/jump", "review", &exe).unwrap();
    assert_eq!(task.holders("orchestrator"), [cobalt.as_str()]);
    let thread = f.said("sim/jump");
    assert!(
        thread.contains(&format!(
            "{cobalt} was given orchestrator (reopened with no orchestrator)"
        )),
        "{thread:?}"
    );
}

#[test]
fn a_given_orchestrator_counts_as_having_orchestrated_the_task_for_its_review() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tools/a");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    f.store.claim("tools/a", &worker).unwrap();
    f.store.leave(&amber).unwrap();
    let cobalt = f.session("orchestrator");
    settle(&f, &[&cobalt]);
    listen(&f, &[&cobalt]);
    let given = f.store.give_unowned(Some(&cobalt)).unwrap();
    assert_eq!(given.gave, [("tools/a".to_string(), cobalt.clone())]);
    // The role row gone, the line alone says cobalt orchestrated it.
    (f.store.raw().table("role"))
        .matching(serde_json::json!({"slug": "tools/a", "handle": cobalt}))
        .delete()
        .unwrap();
    let held = f.store.ever_held("tools/a").unwrap();
    let thread = f.said("tools/a");
    assert!(
        held.orchestrated.contains(&cobalt),
        "{held:?} from {thread:?}"
    );
}
