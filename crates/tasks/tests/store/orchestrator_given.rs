//! Every open task has an orchestrator: the board gives the role to a task filed with none, to the
//! work a leave would leave with none, to a task reopened with none, and, by the idle sweep and as
//! an orchestrator arrives, to every open task nobody orchestrates (a task moved out from under
//! its orchestrator's task among them). A give goes
//! to the orchestrator the task's own story names (the task above it, the filer's work, the filer's
//! parent, the one it had), else to the live orchestrator holding fewest; with none live, the task
//! is marked waiting and the next orchestrator to arrive is given it. A release that would leave
//! open work with no orchestrator is refused.

use super::common::{Fixture, RawWrite as _, refused};
use super::orchestrators::go_quiet_but;
use symphony_board::cli::Env;
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

/// `slug`, filed by `by` through the store, and the orchestrator the board gave it.
fn filed(f: &Fixture, slug: &str, by: &str) -> Option<String> {
    f.task_with(NewTask::new(slug, "Follow-up", by));
    f.store.give_filed(slug, by, None).unwrap()
}

#[test]
fn a_workers_follow_up_goes_to_the_orchestrator_of_the_task_it_works_on() {
    let f = Fixture::empty();
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
fn a_subagents_task_goes_to_the_orchestrator_that_registered_it() {
    let f = Fixture::empty();
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
    // A parent that is not live, or no orchestrator, falls to rule 4.
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
fn a_task_filed_by_no_orchestrators_session_goes_to_the_live_orchestrator_holding_fewest() {
    let f = Fixture::empty();
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/a");
    f.task("tools/b");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    f.store
        .take_role("tools/b", &amber, "orchestrator")
        .unwrap();
    // The owner's task: cobalt holds none, then one, so the second goes to it too (1 < 2).
    assert_eq!(filed(&f, "tools/c", "owner"), Some(cobalt.clone()));
    assert_eq!(filed(&f, "tools/d", "owner"), Some(cobalt.clone()));
    assert_eq!(
        f.said("tools/c").last().unwrap(),
        &format!("{cobalt} was given orchestrator (filed by owner)")
    );
    assert_eq!(f.store.thread("tools/c").unwrap()[1].sender, "owner");
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
fn an_orchestrator_filing_takes_the_role_and_one_named_for_takes_it_instead() {
    let f = Fixture::empty();
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
fn with_no_orchestrator_live_a_task_waits_and_the_next_to_register_is_given_it() {
    let f = Fixture::empty();
    let gone = f.session("orchestrator");
    go_quiet_but(&f, &[]);
    assert_eq!(filed(&f, "tools/loose", "owner"), None);
    let marked = waiting(&f, "tools/loose").expect("marked waiting");
    assert_eq!(marked, f.now());
    // No line on its thread: the mark is the record.
    assert_eq!(f.said("tools/loose"), ["created by owner"]);
    // A sweep with none live marks nothing again.
    f.advance(60.0);
    let swept = f.store.give_unowned(None).unwrap();
    assert!(swept.gave.is_empty());
    assert_eq!(swept.waiting, ["tools/loose"]);
    assert!(swept.marked.is_empty(), "marked once");
    assert_eq!(waiting(&f, "tools/loose"), Some(marked));
    // The next orchestrator arriving takes it, one line on the thread, the mark gone.
    let next = f.session("orchestrator");
    let given = f.store.give_unowned(Some(&next)).unwrap();
    assert_eq!(given.gave, [("tools/loose".to_string(), next.clone())]);
    assert_eq!(
        given.told(&next).as_deref(),
        Some("GIVEN 1 open task is yours to orchestrate: tools/loose")
    );
    assert_eq!(given.told(&gone), None);
    assert_eq!(waiting(&f, "tools/loose"), None);
    assert_eq!(
        f.said("tools/loose").last().unwrap(),
        &format!("{next} was given orchestrator (it waited for an orchestrator)")
    );
    // Its own give is no news to it.
    assert!(roles(read(&f, &next)).is_empty());
}

#[test]
fn when_one_of_two_orchestrators_leaves_the_other_is_given_its_open_work_in_the_same_write() {
    let f = Fixture::empty();
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
    let f = Fixture::empty();
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
fn the_idle_sweep_gives_a_quiet_orchestrators_open_work_to_a_live_one() {
    let f = Fixture::empty();
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
fn an_orchestrator_quiet_then_back_is_given_what_waited_while_none_was_live() {
    let f = Fixture::empty();
    let amber = f.session("orchestrator");
    f.task("tools/a");
    f.task("tools/b");
    go_quiet_but(&f, &[]);
    let swept = f.store.give_unowned(None).unwrap();
    assert_eq!(swept.marked, ["tools/a", "tools/b"]);
    f.store.touch(&amber).unwrap();
    let given = f.store.give_unowned(Some(&amber)).unwrap();
    assert_eq!(
        given.told(&amber).as_deref(),
        Some("GIVEN 2 open tasks are yours to orchestrate: tools/a, tools/b")
    );
    assert!(waiting(&f, "tools/a").is_none() && waiting(&f, "tools/b").is_none());
    assert_eq!(
        f.store.thread("tools/a").unwrap().pop().unwrap().sender,
        amber
    );
}

#[test]
fn letting_the_orchestrator_role_go_is_refused_while_it_leaves_open_work_with_none() {
    let f = Fixture::empty();
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    let board = f.store.board_command();
    f.task("tools/a");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
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
    // With no other orchestrator live, it says so.
    go_quiet_but(&f, &[&amber]);
    assert_eq!(
        refused(f.store.release("tools/a", &amber, Some("orchestrator"))),
        format!(
            "{amber} cannot let go of orchestrator on tools/a: tools/a would have no orchestrator, \
             and no other orchestrator is live to take it: it stays {amber}'s (`{board} leave` \
             leaves it waiting for the next orchestrator)"
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
             orchestrator, and no other orchestrator is live to take it: it stays {amber}'s \
             (`{board} leave` leaves it waiting for the next orchestrator)"
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
fn a_release_under_a_task_an_orchestrator_holds_above_it_goes() {
    let f = Fixture::empty();
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
fn a_task_moved_out_from_under_its_orchestrators_task_is_given_by_the_idle_sweep() {
    let f = Fixture::empty();
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    f.task("tools/a");
    f.task_with(NewTask {
        parent: Some("tools/a".into()),
        ..NewTask::new("tools/a/part", "Part", "owner")
    });
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    // The move stays one write and gives nothing: the part is nobody's for that interval.
    let moved = f.store.set_parent("tools/a/part", None, &cobalt).unwrap();
    assert!(moved.holders("orchestrator").is_empty());
    let lines = f.said("tools/a/part").len();
    // The sweep gives it by rule 4: with no task above it now, to the live orchestrator holding
    // fewest, cobalt, holding none against amber's one.
    let given = f.store.give_unowned(None).unwrap();
    assert_eq!(given.gave, [("tools/a/part".to_string(), cobalt.clone())]);
    assert_eq!(orchestrators_of(&f, "tools/a/part"), [cobalt.as_str()]);
    let said = f.said("tools/a/part");
    assert_eq!(said.len(), lines + 1);
    assert_eq!(
        said.last().unwrap(),
        &format!("{cobalt} was given orchestrator (nobody orchestrated it)")
    );
    // Moved under another held task, the tree gives it: the sweep has nothing to give.
    f.task("tools/b");
    f.store
        .take_role("tools/b", &cobalt, "orchestrator")
        .unwrap();
    f.task_with(NewTask {
        parent: Some("tools/b".into()),
        ..NewTask::new("tools/b/part", "Part", "owner")
    });
    f.store
        .set_parent("tools/b/part", Some("tools/a"), "owner")
        .unwrap();
    assert!(orchestrators_of(&f, "tools/b/part").is_empty());
    assert!(f.store.give_unowned(None).unwrap().is_empty());
}

#[test]
fn an_approved_task_reopened_after_its_orchestrator_left_is_given_one_again() {
    let f = Fixture::empty();
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
    let f = Fixture::empty();
    let amber = f.session("orchestrator");
    let worker = f.session("worker");
    f.task("tools/a");
    f.store
        .take_role("tools/a", &amber, "orchestrator")
        .unwrap();
    f.store.claim("tools/a", &worker).unwrap();
    f.store.leave(&amber).unwrap();
    let cobalt = f.session("orchestrator");
    let given = f.store.give_unowned(Some(&cobalt)).unwrap();
    assert_eq!(given.gave, [("tools/a".to_string(), cobalt.clone())]);
    // The role row gone, the line alone says cobalt orchestrated it.
    f.store
        .client()
        .write(
            "DELETE role WHERE slug = 'tools/a' AND handle = $handle",
            serde_json::json!({"handle": cobalt}),
        )
        .unwrap();
    let held = f.store.ever_held("tools/a").unwrap();
    let thread = f.said("tools/a");
    assert!(
        held.orchestrated.contains(&cobalt),
        "{held:?} from {thread:?}"
    );
}
