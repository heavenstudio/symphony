//! The limit on the open tasks an orchestrator holds the orchestrator role on (`[limits]
//! orchestrator_tasks`, 16 unwritten): a take of the role and a hand-over to one at the limit are
//! refused, naming the setting; the board's give, at a filing, a leave, a reopen and every read
//! of the whole board, passes over one at the limit, and with none under it the task waits, a
//! well state any orchestrator under the limit takes with `role`; a red is held to none of it. One
//! already past the limit keeps every task and lets unstarted ones go with `release`.

use super::common::{Fixture, Seed, refused};
use super::orchestrators::go_quiet_but;
use symphony_board::health::LIVE_SECONDS;
use symphony_board::limits::{Limits, ORCHESTRATOR_TASKS};
use symphony_board::red::FILER;
use symphony_board::{NewTask, Store};

/// A store on `f`'s board holding orchestrators to `most` open tasks.
fn limited(f: &Fixture, most: usize) -> Store {
    f.other().limiting(Limits {
        orchestrator_tasks: most,
        ..Limits::default()
    })
}

/// What a refusal at the limit says of `handle` holding `held` against `limit`.
fn at_limit(handle: &str, held: usize, limit: usize) -> String {
    format!(
        "{handle} orchestrates {held} open tasks, the limit of {limit} an orchestrator holds \
         ([limits] orchestrator_tasks): it gains none until one of them is done or cancelled or \
         it lets one go"
    )
}

/// `n` tasks `prefix/0`.. that `handle` takes the orchestrator role on through `store`.
fn orchestrating(f: &Fixture, store: &Store, handle: &str, prefix: &str, n: usize) {
    for i in 0..n {
        let slug = format!("{prefix}/{i}");
        f.task(&slug);
        store.take_role(&slug, handle, "orchestrator").unwrap();
    }
}

fn orchestrators_of(f: &Fixture, slug: &str) -> Vec<String> {
    let task = f.store.get(slug).unwrap();
    task.holders("orchestrator")
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// Every one of `handles` past the first live line, beating: orchestrators that stay, as each
/// holds a task.
fn settle(f: &Fixture, handles: &[&str]) {
    f.advance(LIVE_SECONDS + 1.0);
    for handle in handles {
        f.store.touch(handle).unwrap();
    }
}

/// A red `slug`, filed by a red's filer.
fn red(f: &Fixture, slug: &str) {
    let filer = f.session(FILER);
    f.task_with(NewTask {
        red: Some("main".into()),
        ..NewTask::new(slug, "A red", &filer)
    });
}

#[test]
fn the_default_limit_refuses_the_seventeenth_take_naming_the_setting() {
    let f = Fixture::new(Seed::default());
    let amber = f.session("orchestrator");
    assert_eq!(f.store.limits().orchestrator_tasks, ORCHESTRATOR_TASKS);
    orchestrating(&f, &f.store, &amber, "tools/t", ORCHESTRATOR_TASKS);
    f.task("tools/more");
    assert_eq!(
        refused(f.store.take_role("tools/more", &amber, "orchestrator")),
        format!(
            "{amber} cannot take orchestrator on tools/more: {}",
            at_limit(&amber, 16, 16)
        )
    );
    assert!(orchestrators_of(&f, "tools/more").is_empty());
    // Taking again one it holds gains nothing, and is never refused.
    f.store
        .take_role("tools/t/0", &amber, "orchestrator")
        .unwrap();
    // One done frees a place.
    let worker = f.session("worker");
    f.store.claim("tools/t/0", &worker).unwrap();
    f.store.move_to("tools/t/0", "cancelled", &amber).unwrap();
    f.store
        .take_role("tools/more", &amber, "orchestrator")
        .unwrap();
}

#[test]
fn every_open_task_held_counts_review_and_blocked_too_but_not_the_tree_below_nor_a_red() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 3);
    let (amber, worker) = (f.session("orchestrator"), f.session("worker"));
    orchestrating(&f, &store, &amber, "tools/t", 2);
    store.claim("tools/t/0", &worker).unwrap();
    store.move_to("tools/t/0", "review", &worker).unwrap();
    store.move_to("tools/t/1", "blocked", &amber).unwrap();
    // Work under its tasks is its own by the tree: no place taken, and the third goes.
    for part in ["tools/t/0/part", "tools/t/1/part"] {
        let parent = part.rsplit_once('/').unwrap().0;
        f.task_with(NewTask {
            parent: Some(parent.into()),
            ..NewTask::new(part, "Part", "owner")
        });
    }
    // A red it orchestrates takes none, and is taken at any count.
    red(&f, "tests/main-red-a");
    store
        .take_role("tests/main-red-a", &amber, "orchestrator")
        .unwrap();
    f.task("tools/third");
    store
        .take_role("tools/third", &amber, "orchestrator")
        .unwrap();
    f.task("tools/fourth");
    assert_eq!(
        refused(store.take_role("tools/fourth", &amber, "orchestrator")),
        format!(
            "{amber} cannot take orchestrator on tools/fourth: {}",
            at_limit(&amber, 3, 3)
        )
    );
    red(&f, "tests/main-red-b");
    store
        .take_role("tests/main-red-b", &amber, "orchestrator")
        .unwrap();
}

#[test]
fn a_hand_over_to_one_at_the_limit_is_refused_but_the_owners_and_a_reds() {
    // The owner's session, which holds the role on a task as the board's seed wrote it.
    let seed = (Seed::default().session("owner"))
        .task("tools/owned", "ready")
        .role("tools/owned", "owner", "orchestrator");
    let f = Fixture::new(seed);
    let store = limited(&f, 1);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    orchestrating(&f, &store, &cobalt, "tools/c", 1);
    orchestrating(&f, &limited(&f, 2), &amber, "tools/a", 2);
    assert_eq!(
        refused(store.hand_over("tools/a/1", &amber, &cobalt)),
        format!(
            "{amber} cannot hand tools/a/1 to {cobalt}: {}",
            at_limit(&cobalt, 1, 1)
        )
    );
    assert_eq!(orchestrators_of(&f, "tools/a/1"), [amber.as_str()]);
    // The owner's is not held to it, as its moves are not held to the cap.
    store.hand_over("tools/owned", "owner", &cobalt).unwrap();
    assert_eq!(orchestrators_of(&f, "tools/owned"), [cobalt.as_str()]);
    red(&f, "tests/main-red-c");
    store
        .take_role("tests/main-red-c", &amber, "orchestrator")
        .unwrap();
    store
        .hand_over("tests/main-red-c", &amber, &cobalt)
        .unwrap();
    assert_eq!(orchestrators_of(&f, "tests/main-red-c"), [cobalt.as_str()]);
}

#[test]
fn an_orchestrator_filing_at_the_limit_is_passed_over_for_one_under_it_or_the_task_waits() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 2);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    orchestrating(&f, &store, &amber, "tools/a", 2);
    orchestrating(&f, &store, &cobalt, "tools/c", 1);
    settle(&f, &[&amber, &cobalt]);
    f.task_with(NewTask::new("tools/filed", "Filed", &amber));
    let filed = store
        .give_filed_saying("tools/filed", &amber, None)
        .unwrap();
    assert_eq!(filed.orchestrator.as_deref(), Some(cobalt.as_str()));
    assert_eq!(
        filed.passed,
        Some(format!(
            "{amber} cannot take orchestrator on tools/filed: {}",
            at_limit(&amber, 2, 2)
        ))
    );
    assert_eq!(
        f.said("tools/filed").last().unwrap(),
        &format!("{cobalt} was given orchestrator (filed by {amber})")
    );
    // Both at the limit now: the next waits, marked, and the filer is told why.
    f.task_with(NewTask::new("tools/waits", "Waits", &amber));
    let filed = store
        .give_filed_saying("tools/waits", &amber, None)
        .unwrap();
    assert_eq!(filed.orchestrator, None);
    assert!(filed.passed.is_some());
    let task = f.store.get("tools/waits").unwrap();
    assert!(symphony_board::orchestration::waiting(&task.fields).is_some());
    // `--for` one at the limit passes it over the same way, naming it.
    f.task_with(NewTask::new("tools/for", "For", &amber));
    let filed = store
        .give_filed_saying("tools/for", &amber, Some(&cobalt))
        .unwrap();
    assert_eq!(filed.orchestrator, None);
    assert_eq!(filed.passed, Some(at_limit(&cobalt, 2, 2)));
    // A filer under the limit takes it, saying nothing more.
    let teal = f.session("orchestrator");
    f.task_with(NewTask::new("tools/teals", "Teal's", &teal));
    let filed = store.give_filed_saying("tools/teals", &teal, None).unwrap();
    assert_eq!(
        (filed.orchestrator.as_deref(), filed.passed),
        (Some(teal.as_str()), None)
    );
}

#[test]
fn one_filed_for_at_the_limit_is_named_as_passed_over_though_the_filer_is_given_the_task() {
    let f = Fixture::new(crate::common::Seed::default());
    let store = limited(&f, 2);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    orchestrating(&f, &store, &amber, "tools/a", 1);
    orchestrating(&f, &store, &cobalt, "tools/c", 2);
    settle(&f, &[&amber, &cobalt]);
    // `--for` cobalt, at its limit: the board gives it to the one under it that stays, the filer,
    // and the filer is still told why cobalt was passed over.
    f.task_with(NewTask::new("tools/for", "For", &amber));
    let filed = store
        .give_filed_saying("tools/for", &amber, Some(&cobalt))
        .unwrap();
    assert_eq!(
        (filed.orchestrator.as_deref(), filed.passed),
        (Some(amber.as_str()), Some(at_limit(&cobalt, 2, 2)))
    );
    assert_eq!(orchestrators_of(&f, "tools/for"), [amber.as_str()]);
}

#[test]
fn the_boards_give_fills_none_past_the_limit_and_a_waiting_task_is_any_takers() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 2);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    orchestrating(&f, &store, &amber, "tools/a", 1);
    orchestrating(&f, &store, &cobalt, "tools/c", 2);
    for slug in ["tools/free/1", "tools/free/2", "tools/free/3"] {
        f.task(slug);
    }
    settle(&f, &[&amber, &cobalt]);
    let given = store.give_unowned(None).unwrap();
    // amber takes one, to its limit; cobalt, at its limit, none; the rest wait.
    assert_eq!(given.gave, [("tools/free/1".to_string(), amber.clone())]);
    assert_eq!(given.waiting, ["tools/free/2", "tools/free/3"]);
    // Nobody under the limit: a read of the whole board has nothing to give, and gives nothing.
    assert!(!store.give_if_due(&store.board().unwrap()));
    assert!(store.give_unowned(None).unwrap().gave.is_empty());
    // A waiting task is any orchestrator's to take under the limit: one just registered too.
    let teal = f.session("orchestrator");
    store
        .take_role("tools/free/3", &teal, "orchestrator")
        .unwrap();
    // One of amber's done, the next read gives it the next that waits.
    store.move_to("tools/a/0", "cancelled", &amber).unwrap();
    assert!(store.give_if_due(&store.board().unwrap()));
    assert_eq!(orchestrators_of(&f, "tools/free/2"), [amber.as_str()]);
}

#[test]
fn a_place_under_the_limit_goes_to_a_task_that_waits_on_nothing_before_one_that_still_waits() {
    // In the board's order: one that waits on open work, one on work that is done, one on none.
    let seed = Seed::default()
        .task("tools/landed", "done")
        .task("tools/open", "ready")
        .task("tools/waits", "ready")
        .link("tools/waits", "dep", "tools/open")
        .task("tools/after", "ready")
        .link("tools/after", "dep", "tools/landed")
        .task("tools/free", "ready");
    let f = Fixture::new(seed);
    let store = limited(&f, 3);
    let amber = f.session("orchestrator");
    store
        .take_role("tools/open", &amber, "orchestrator")
        .unwrap();
    settle(&f, &[&amber]);
    // Two places: they go to the two that can be worked on now, and the one nobody can start waits.
    let given = store.give_unowned(None).unwrap();
    assert_eq!(
        given.gave,
        [
            ("tools/after".to_string(), amber.clone()),
            ("tools/free".to_string(), amber.clone())
        ]
    );
    assert_eq!(given.waiting, ["tools/waits"]);
}

#[test]
fn a_red_with_none_under_the_limit_goes_to_one_that_stays_whatever_it_holds() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 1);
    let amber = f.session("orchestrator");
    orchestrating(&f, &store, &amber, "tools/a", 1);
    f.task("tools/ordinary");
    red(&f, "tests/main-red-d");
    settle(&f, &[&amber]);
    let given = store.give_unowned(None).unwrap();
    assert_eq!(
        given.gave,
        [("tests/main-red-d".to_string(), amber.clone())]
    );
    assert_eq!(given.waiting, ["tools/ordinary"]);
}

#[test]
fn a_leavers_work_goes_only_as_far_as_the_others_limits_and_the_rest_waits() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 2);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    orchestrating(&f, &store, &cobalt, "tools/c", 1);
    orchestrating(&f, &store, &amber, "tools/a", 2);
    settle(&f, &[&amber, &cobalt]);
    store.leave(&amber).unwrap();
    assert_eq!(orchestrators_of(&f, "tools/a/0"), [cobalt.as_str()]);
    assert!(orchestrators_of(&f, "tools/a/1").is_empty());
    let task = f.store.get("tools/a/1").unwrap();
    assert!(symphony_board::orchestration::waiting(&task.fields).is_some());
}

#[test]
fn a_leavers_task_that_waits_on_nothing_takes_the_place_before_one_that_still_waits() {
    // In the board's order, the one that waits on open work comes first, by its slug and its filing.
    let seed = Seed::default()
        .task("tools/open", "ready")
        .task("tools/a-waits", "ready")
        .link("tools/a-waits", "dep", "tools/open")
        .task("tools/b-free", "ready");
    let f = Fixture::new(seed);
    let store = limited(&f, 2);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    store
        .take_role("tools/open", &cobalt, "orchestrator")
        .unwrap();
    for slug in ["tools/a-waits", "tools/b-free"] {
        store.take_role(slug, &amber, "orchestrator").unwrap();
    }
    settle(&f, &[&amber, &cobalt]);
    // Cobalt has one place: the task that can be worked on now takes it, and the other waits.
    store.leave(&amber).unwrap();
    assert_eq!(orchestrators_of(&f, "tools/b-free"), [cobalt.as_str()]);
    assert!(orchestrators_of(&f, "tools/a-waits").is_empty());
    let task = f.store.get("tools/a-waits").unwrap();
    assert!(symphony_board::orchestration::waiting(&task.fields).is_some());
}

#[test]
fn a_reopened_task_is_given_only_to_one_under_the_limit() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 1);
    let (amber, exe, rev) = (
        f.session("orchestrator"),
        f.session("worker"),
        f.session("reviewer"),
    );
    f.task("sim/jump");
    store.take_role("sim/jump", &amber, "orchestrator").unwrap();
    store.claim("sim/jump", &exe).unwrap();
    store.move_to("sim/jump", "review", &exe).unwrap();
    store.take_role("sim/jump", &rev, "reviewer").unwrap();
    store.verdict("sim/jump", &rev, "approve", "good").unwrap();
    store.leave(&amber).unwrap();
    let cobalt = f.session("orchestrator");
    orchestrating(&f, &store, &cobalt, "tools/c", 1);
    settle(&f, &[&cobalt, &exe]);
    let task = store.move_to("sim/jump", "review", &exe).unwrap();
    assert!(task.holders("orchestrator").is_empty(), "{task:?}");
    assert!(symphony_board::orchestration::waiting(&task.fields).is_some());
}

#[test]
fn one_past_the_limit_keeps_every_task_and_lets_the_unstarted_ones_go() {
    let f = Fixture::new(Seed::default());
    let (amber, worker) = (f.session("orchestrator"), f.session("worker"));
    // Taken under a limit of 4, then held to 2: amber is 2 past it, and nothing is taken from it.
    orchestrating(&f, &limited(&f, 4), &amber, "tools/a", 4);
    let store = limited(&f, 2);
    store.claim("tools/a/0", &worker).unwrap();
    for i in 0..4 {
        assert_eq!(
            orchestrators_of(&f, &format!("tools/a/{i}")),
            [amber.as_str()]
        );
    }
    // Started work stays its own: a release of it is refused as ever.
    let refusal = refused(store.release("tools/a/0", &amber, Some("orchestrator")));
    assert!(
        refusal.starts_with(&format!(
            "{amber} cannot let go of orchestrator on tools/a/0: tools/a/0 would have no orchestrator"
        )),
        "{refusal}"
    );
    // Unstarted work goes, and waits with no orchestrator under the limit that stays.
    store
        .release("tools/a/1", &amber, Some("orchestrator"))
        .unwrap();
    assert!(orchestrators_of(&f, "tools/a/1").is_empty());
    store
        .release("tools/a/2", &amber, Some("orchestrator"))
        .unwrap();
    // At the limit, no more past it: the next release is refused, as below the limit.
    let refusal = refused(store.release("tools/a/3", &amber, Some("orchestrator")));
    assert!(refusal.contains("would have no orchestrator"), "{refusal}");
    // amber, at the limit, is given none of what it let go.
    settle(&f, &[&amber, &worker]);
    let given = store.give_unowned(None).unwrap();
    assert!(given.gave.is_empty(), "{given:?}");
    assert_eq!(given.waiting, ["tools/a/1", "tools/a/2"]);
}

#[test]
fn a_take_written_between_a_gives_read_and_its_write_has_it_decide_again() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 2);
    let amber = f.session("orchestrator");
    orchestrating(&f, &store, &amber, "tools/a", 1);
    f.task("tools/free");
    f.task("tools/raced");
    settle(&f, &[&amber]);
    // While the pass decides on the board it read, amber takes another, to its limit.
    let (taker, handle) = (limited(&f, 2), amber.clone());
    let late = f
        .late(move || {
            taker
                .take_role("tools/raced", &handle, "orchestrator")
                .unwrap();
        })
        .limiting(Limits {
            orchestrator_tasks: 2,
            ..Limits::default()
        });
    let given = late.give_unowned(None).unwrap();
    assert!(given.gave.is_empty(), "{given:?}");
    assert_eq!(given.waiting, ["tools/free"]);
    assert_eq!(orchestrators_of(&f, "tools/raced"), [amber.as_str()]);
    assert!(orchestrators_of(&f, "tools/free").is_empty());
}

#[test]
fn a_receiver_leaving_between_a_gives_read_and_its_write_has_it_decide_again() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 2);
    let amber = f.session("orchestrator");
    orchestrating(&f, &store, &amber, "tools/a", 1);
    f.task("tools/free");
    settle(&f, &[&amber]);
    // While the pass decides on the board it read, amber, whom it gives the task to, leaves.
    let (leaver, handle) = (f.other(), amber.clone());
    let late = f
        .late(move || leaver.leave(&handle).unwrap())
        .limiting(Limits {
            orchestrator_tasks: 2,
            ..Limits::default()
        });
    let given = late.give_unowned(None).unwrap();
    assert!(given.gave.is_empty(), "{given:?}");
    assert!(
        given.waiting.contains(&"tools/free".to_string()),
        "{given:?}"
    );
    assert!(orchestrators_of(&f, "tools/free").is_empty());
}

#[test]
fn a_quiet_orchestrators_work_waits_while_the_one_that_stays_is_full() {
    let f = Fixture::new(Seed::default());
    let store = limited(&f, 1);
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    orchestrating(&f, &store, &cobalt, "tools/c", 1);
    orchestrating(&f, &store, &amber, "tools/a", 1);
    go_quiet_but(&f, &[&cobalt]);
    let given = store.give_unowned(None).unwrap();
    assert!(given.gave.is_empty(), "{given:?}");
    assert_eq!(given.waiting, ["tools/a/0"]);
    // A give of its own is not due again while nothing changes.
    assert!(!store.give_if_due(&store.board().unwrap()));
}
