//! The cap where work moves back into an open status: from blocked, through a changed dependency,
//! from a landing's hold; where a change of parent or a hand-over brings open work under an
//! orchestrator; on a task two orchestrators hold; a start racing the close of the slot it joins;
//! and a red's close, never refused, which counts the work under it from then, a start that read
//! the board before it decided again.

use super::{
    EIGHT, F, HOLDS_EIGHT, K, O, P, W, board, capped, eight_and, open, red, reserve_hint, sessions,
    with,
};
use crate::common::{Fixture, refused, task};
use serde_json::json;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;
use symphony_board::rows::TaskRow;
use symphony_board::{Error, NewTask, Store, Task};

/// `slugs` as `change_depends` takes them.
fn owned(slugs: &[&str]) -> Vec<String> {
    slugs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn work_blocked_from_in_progress_goes_to_review_at_the_cap_and_back_to_work_only_below_it() {
    let f = eight_and(&[("work/held", "ready")]);
    let free = f.other();
    free.claim("work/held", K).unwrap();
    free.move_to("work/held", "blocked", K).unwrap();
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.move_to("work/held", "in_progress", K)),
        format!("work/held cannot go from blocked to in_progress: {HOLDS_EIGHT}")
    );
    assert_eq!(store.get("work/held").unwrap().status, "blocked");
    assert_eq!(
        store.move_to("work/held", "review", K).unwrap().status,
        "review",
        "work in review takes no slot"
    );
}

/// O holding [`EIGHT`] with work/h's place taken by work/next, in progress.
fn holds_next() -> String {
    HOLDS_EIGHT.replace("work/h (in_progress)", "work/next (in_progress)")
}

#[test]
fn work_blocked_from_review_by_a_move_goes_back_to_review_at_the_cap_and_to_work_only_below_it() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    store.move_to("work/h", "review", O).unwrap();
    store.move_to("work/h", "blocked", O).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.move_to("work/h", "in_progress", O)),
        format!(
            "work/h cannot go from blocked to in_progress: {}",
            holds_next()
        )
    );
    assert_eq!(store.get("work/h").unwrap().status, "blocked");
    assert_eq!(
        store.move_to("work/h", "review", O).unwrap().status,
        "review"
    );
    assert_eq!(open(&store).len(), 9);
}

#[test]
fn a_landings_hold_keeps_its_tasks_slot_and_returns_it_to_work_at_the_cap() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    // land-stamps holds work/h while nobody makes a stamp it lacks: its slot stays taken meanwhile.
    assert_eq!(store.hold_blocked("work/h", O).unwrap().status, "blocked");
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {}{}",
            HOLDS_EIGHT.replace("work/h (in_progress)", "work/h (blocked, held)"),
            reserve_hint("work/next")
        )
    );
    // Back to work, at the cap.
    assert_eq!(
        store.move_to("work/h", "in_progress", O).unwrap().status,
        "in_progress"
    );
    assert_eq!(open(&store).len(), 8);
    // That move ended the hold: blocked again by a move, the slot is free and the way back capped.
    store.move_to("work/h", "blocked", O).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.move_to("work/h", "in_progress", O)),
        format!(
            "work/h cannot go from blocked to in_progress: {}",
            holds_next()
        )
    );
    assert_eq!(
        refused(store.hold_blocked("work/h", O)),
        "work/h is blocked: only work in progress or in review is held"
    );
}

/// Whether the board keeps a hold on `slug`.
fn hold_kept(store: &Store, slug: &str) -> bool {
    let hold = store.raw().row("counter", json!(["hold", slug]));
    hold.exists().unwrap()
}

#[test]
fn a_landing_of_held_work_deletes_its_hold_as_every_status_write_does() {
    let f = eight_and(&[]);
    let store = capped(&f, 8);
    store.hold_blocked("work/h", O).unwrap();
    store.hold_blocked("work/f", O).unwrap();
    assert!(hold_kept(&store, "work/h") && hold_kept(&store, "work/f"));
    let landed = store
        .landed("work/h", O, "landed", false, &["review", "blocked"])
        .unwrap();
    assert_eq!(
        (landed.task.status.as_str(), landed.from.as_str()),
        ("done", "blocked")
    );
    assert!(
        !hold_kept(&store, "work/h"),
        "the landing left its hold behind"
    );
    store.move_to("work/f", "cancelled", "owner").unwrap();
    assert!(
        !hold_kept(&store, "work/f"),
        "the owner's cancel left its hold behind"
    );
}

#[test]
fn a_hold_ends_with_a_change_of_parent_and_frees_the_slot() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    store.hold_blocked("work/h", O).unwrap();
    store.set_parent("work/h", None, O).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.move_to("work/h", "in_progress", O)),
        format!(
            "work/h cannot go from blocked to in_progress: {}",
            holds_next()
        )
    );
}

#[test]
fn a_dependency_change_that_would_start_blocked_work_past_the_cap_is_refused() {
    let mut seed = sessions();
    for (slug, status) in EIGHT {
        seed = with(seed, slug, status, None, Some(O));
    }
    seed = with(seed, "work/held", "in_progress", None, Some(O));
    seed = with(seed, "work/r", "review", None, Some(O));
    for dep in ["dep/old", "dep/two"] {
        seed = with(seed, dep, "ready", None, None);
    }
    let seed = seed
        .role("work/held", K, "executor")
        .link("work/held", "dep", "dep/old")
        .link("work/r", "dep", "dep/two");
    let f = Fixture::new(seed);
    let store = capped(&f, 8);
    // A cancelled prerequisite blocks work/held (was in_progress): eight open again.
    store.move_to("dep/old", "cancelled", "owner").unwrap();
    assert_eq!(store.get("work/held").unwrap().status, "blocked");
    let back = || store.change_depends("work/held", &[], &owned(&["dep/old"]), K);
    assert_eq!(
        refused(back()),
        format!(
            "work/held cannot go from blocked back to in_progress: {HOLDS_EIGHT}; not counted, as work in review takes no slot: work/r (review)"
        )
    );
    assert_eq!(store.get("work/held").unwrap().status, "blocked");
    store.move_to("work/a", "cancelled", "owner").unwrap();
    assert_eq!(back().unwrap().status, "in_progress");
    // Work a cancelled prerequisite blocked in review goes back to review at the cap: it takes no
    // slot there.
    store.move_to("dep/two", "cancelled", "owner").unwrap();
    assert_eq!(store.get("work/r").unwrap().status, "blocked");
    let back = || store.change_depends("work/r", &[], &owned(&["dep/two"]), K);
    assert_eq!(back().unwrap().status, "review");
}

#[test]
fn open_work_moved_out_of_its_feature_keeps_a_slot_and_rejoining_one_consumes_no_extra_slot() {
    let mut seed = sessions();
    for (slug, status) in &EIGHT[..7] {
        seed = with(seed, slug, status, None, Some(O));
    }
    seed = with(seed, "work/a/part", "in_progress", Some("work/a"), None);
    seed = with(seed, "work/next", "ready", None, Some(O));
    seed = with(seed, "loose", "review", None, None);
    let f = Fixture::new(seed.role("work/a/part", K, "executor"));
    let store = capped(&f, 8);
    // A child leaving its feature stays its orchestrator's: the move gives O the part, which
    // takes a slot of its own, decided by the move's own cap read. O holds 8 then, the cap, so a
    // start waits until the part rejoins.
    store.set_parent("work/a/part", None, K).unwrap();
    let part = store.get("work/a/part").unwrap();
    assert_eq!(part.holders("orchestrator"), [O]);
    let start = refused(store.claim("work/next", K));
    assert!(
        start.starts_with("work/next cannot start: olive-orch holds 8 open tasks"),
        "{start}"
    );
    // Rejoining an already counted feature consumes no slot of its own, and frees the one it took.
    store.set_parent("work/a/part", Some("work/a"), K).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    // At the cap, the move out is refused, as any change that would raise O's count past it is.
    let out = refused(store.set_parent("work/a/part", None, K));
    assert!(
        out.starts_with("work/a/part cannot go under nothing: olive-orch holds 8 open tasks"),
        "{out}"
    );
    // Joining a counted feature after another started consumes no slot either.
    store.set_parent("loose", Some("work/b"), K).unwrap();
    assert_eq!(
        store.get("work/a/part").unwrap().parent.as_deref(),
        Some("work/a")
    );
    // A change that leaves every count as it was is not refused, nor is the owner's.
    store.set_parent("work/next", Some("work/a"), O).unwrap();
    store.set_parent("loose", Some("work/b"), "owner").unwrap();
}

#[test]
fn open_work_handed_back_and_forth_between_orchestrators_is_refused_at_the_cap() {
    let mut seed = sessions().session("owner");
    for slug in ["work/a", "work/b"] {
        seed = with(seed, slug, "in_progress", None, Some(O));
    }
    seed = with(seed, "work/next", "ready", None, Some(O));
    seed = with(seed, "pine/x", "in_progress", None, Some(P));
    seed = with(seed, "owned", "review", None, Some("owner"));
    let f = Fixture::new(seed);
    let store = capped(&f, 2);
    let full = |tasks: &str| {
        format!(
            "olive-orch holds 2 open tasks, the cap of 2 an orchestrator holds: {tasks}; a slot frees when one of them goes to review or is done or cancelled"
        )
    };
    assert_eq!(
        refused(store.hand_over("pine/x", P, O)),
        format!(
            "pine-orch cannot hand pine/x, which is in_progress, to olive-orch: {}",
            full("work/a (in_progress), work/b (in_progress)")
        )
    );
    assert_eq!(store.get("pine/x").unwrap().holders("orchestrator"), [P]);
    // The way round the cap a review found: hand open work to another orchestrator, start new work
    // in the slot that frees, and have it handed back.
    store.hand_over("work/a", O, P).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let holds = full("work/b (in_progress), work/next (in_progress)");
    assert_eq!(
        refused(store.hand_over("work/a", P, O)),
        format!("pine-orch cannot hand work/a, which is in_progress, to olive-orch: {holds}")
    );
    assert_eq!(store.get("work/a").unwrap().holders("orchestrator"), [P]);
    // Taking it is refused before the cap, pointing to a hand-over the cap decides the same way.
    assert_eq!(
        refused(store.take_role("work/a", O, "orchestrator")),
        format!(
            "work/a is orchestrated by pine-orch (live); pine-orch hands it over with: {} \
                 handover work/a olive-orch, which olive-orch's cap decides as it does a take",
            store.board_command()
        )
    );
    // The owner's hand-over is not capped, as the owner's moves are not.
    store.hand_over("owned", "owner", O).unwrap();
    assert_eq!(store.get("owned").unwrap().holders("orchestrator"), [O]);
}

#[test]
fn open_work_moved_under_one_feature_shares_its_slot_regardless_of_instruction_flag() {
    let seed = super::eight_and_seed(&[("work/next", "ready")]);
    let id = seed.dump.tasks.len() as i64 + 1;
    let seed = seed.task_row(TaskRow {
        id,
        parent: Some("work/a".into()),
        instruction: true,
        ..task("work/a/build", "ready")
    });
    let f = Fixture::new(seed);
    // One parent delivery owns all its implementation slices.
    for (slug, _) in &EIGHT[1..] {
        f.store.set_parent(slug, Some("work/a"), O).unwrap();
    }
    let store = capped(&f, 1);
    let shared = HOLDS_EIGHT
        .replace(
            "holds 8 open tasks, the cap of 8",
            "holds 1 open tasks, the cap of 1",
        )
        .replace("), work/", ") with work/");
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {shared}{}",
            reserve_hint("work/next")
        )
    );
    assert_eq!(
        store.claim("work/a/build", K).unwrap().status,
        "in_progress"
    );
    assert_eq!(refused(store.claim("work/next", K)), {
        let shared = shared.replace(
            "work/a (in_progress)",
            "work/a (in_progress) with work/a/build (in_progress)",
        );
        format!(
            "work/next cannot start: {shared}{}",
            reserve_hint("work/next")
        )
    });
}

#[test]
fn a_task_two_orchestrators_hold_counts_against_both_and_is_refused_if_either_is_full() {
    let seed = super::eight_and_seed(&[])
        .task_row(TaskRow {
            id: 100,
            ..task("both", "ready")
        })
        .role("both", O, "orchestrator")
        .role("both", P, "orchestrator");
    let seed = with(seed, "pine/next", "ready", None, Some(P));
    let f = Fixture::new(seed);
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.claim("both", K)),
        format!("both cannot start: {HOLDS_EIGHT}{}", reserve_hint("both"))
    );
    store.move_to("work/a", "cancelled", "owner").unwrap();
    assert_eq!(store.claim("both", K).unwrap().status, "in_progress");
    // It is P's too: at a cap of one, P starts nothing more.
    assert_eq!(
        refused(capped(&f, 1).claim("pine/next", K)),
        format!(
            "pine/next cannot start: {}{}",
            "pine-orch holds 1 open tasks, the cap of 1 an orchestrator holds: both (in_progress); a slot frees when one of them goes to review or is done or cancelled",
            reserve_hint("pine/next")
        )
    );
}

#[test]
fn a_move_into_in_progress_of_work_nobody_orchestrates_counts_against_its_executors_parent() {
    let led: Vec<(&str, &str)> = (EIGHT[..7].iter().copied())
        .chain([("work/next", "ready")])
        .collect();
    let f = board(&led, &[("loose", "ready")]);
    let free = f.other();
    free.claim("loose", W).unwrap();
    free.move_to("loose", "blocked", W).unwrap();
    let store = capped(&f, 8);
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let said = refused(store.move_to("loose", "in_progress", W));
    assert!(
            said.starts_with("loose cannot go from blocked to in_progress: olive-orch (no orchestrator holds it: olive-orch is the parent of wren) holds 8 open tasks"),
            "{said}"
        );
    assert!(said.contains("work/next (in_progress)"), "{said}");
}

#[test]
fn a_start_into_an_open_slot_racing_its_close_and_another_start_is_decided_again() {
    let mut seed = sessions();
    seed = with(seed, "work/a", "in_progress", None, Some(O));
    seed = with(seed, "work/b", "in_progress", None, Some(O));
    seed = with(seed, "work/c", "ready", None, Some(O));
    seed = with(seed, "review/work/a", "ready", None, None);
    let f = Fixture::new(seed);
    let other = capped(&f, 2);
    // The mirror's claim reads work/a's slot counted, so it opens none; before it writes, work/a is
    // cancelled and work/c takes the slot that freed.
    let racing = f
        .slow(move || {
            other.move_to("work/a", "cancelled", "owner").unwrap();
            other.claim("work/c", K).unwrap();
        })
        .capping(Some(2));
    assert_eq!(
        refused(racing.claim("review/work/a", K)),
        format!(
            "review/work/a cannot start: {}{}",
            "olive-orch holds 2 open tasks, the cap of 2 an orchestrator holds: work/b (in_progress), work/c (in_progress); a slot frees when one of them goes to review or is done or cancelled",
            reserve_hint("review/work/a")
        )
    );
    assert_eq!(open(&f.store), ["work/b", "work/c"]);
}

/// The red the race tests close, and the work under it.
pub(super) const RED: &str = "tests/main-red-race";
pub(super) const UNDER: &str = "fix/under";

/// O holding the first `led` of [`EIGHT`] and `work/next` to claim, beside [`RED`] in review, which
/// O orchestrates and lark reviews, with [`UNDER`] under it, in progress when `started`: red work,
/// which takes no slot while the red is open.
pub(super) fn beside_a_red(led: usize, started: bool) -> Fixture {
    let mut seed = sessions().session("lark");
    for (slug, status) in &EIGHT[..led] {
        seed = with(seed, slug, status, None, Some(O));
    }
    let f = Fixture::new(with(seed, "work/next", "ready", None, Some(O)));
    let store = &f.store;
    red(store, RED, "main");
    store.take_role(RED, O, "orchestrator").unwrap();
    store.take_role(RED, "lark", "reviewer").unwrap();
    store.claim(RED, W).unwrap();
    store.move_to(RED, "review", W).unwrap();
    let under = NewTask {
        parent: Some(RED.into()),
        ..NewTask::new(UNDER, "Under the red", "owner")
    };
    store.create(&under).unwrap();
    if started {
        store.claim(UNDER, W).unwrap();
    }
    f
}

/// A write that closes [`RED`].
type Close = fn(&Store) -> Result<Task, Error>;

/// Each write that closes [`RED`]: its orchestrator's cancel, the owner's, its reviewer's approval
/// and its landing.
fn closes() -> [(&'static str, Close); 4] {
    [
        ("its orchestrator's cancel", |s| {
            s.move_to(RED, "cancelled", O)
        }),
        ("the owner's cancel", |s| {
            s.move_to(RED, "cancelled", "owner")
        }),
        ("its reviewer's approval", |s| {
            s.verdict(RED, "lark", "approve", "fixed")
        }),
        ("its landing", |s| {
            (s.landed(RED, W, "landed", false, &["review"])).map(|landed| landed.task)
        }),
    ]
}

/// A store closing [`RED`]: one capping starts as the cap's board does, or one capping none, as the
/// release run's and any board without a cap: a red's close holds its counters on either.
type Writer = fn(&Fixture) -> Store;

/// One way to close [`RED`]: a close ([`closes`]) on a closing store ([`Writer`]), and how it is
/// named.
struct Closer {
    how: String,
    writer: Writer,
    close: Close,
}

/// Each closing store with each close.
fn closers() -> Vec<Closer> {
    let writers: [(&str, Writer); 2] = [
        ("a capped store", |f| capped(f, 8)),
        ("an uncapped store", Fixture::other),
    ];
    let pairs = writers.into_iter().flat_map(|(by, writer)| {
        (closes().into_iter()).map(move |(how, close)| Closer {
            how: format!("{how} on {by}"),
            writer,
            close,
        })
    });
    pairs.collect()
}

#[test]
fn a_reds_close_past_the_cap_is_never_refused_and_the_work_under_it_counts_from_then() {
    for Closer { how, writer, close } in closers() {
        let f = beside_a_red(8, true);
        close(&writer(&f)).unwrap_or_else(|why| panic!("{how}: {why:?}"));
        let store = capped(&f, 8);
        assert_eq!(open(&store).len(), 9, "{how}");
        let said = refused(store.claim("work/next", K));
        assert!(
            said.starts_with("work/next cannot start: olive-orch holds 9 open tasks"),
            "{how}: {said}"
        );
        assert!(said.contains("fix/under (in_progress)"), "{how}: {said}");
    }
}

#[test]
fn a_start_decided_before_a_red_closed_is_decided_again_the_work_under_it_counted() {
    for Closer { how, writer, close } in closers() {
        let f = beside_a_red(7, true);
        let other = writer(&f);
        // The claim reads seven slots, the red's work taking none; before it writes, the red
        // closes, and the work under it takes the eighth.
        let racing = f
            .slow(move || {
                close(&other).unwrap();
            })
            .capping(Some(8));
        let said = refused(racing.claim("work/next", K));
        assert!(
            said.starts_with("work/next cannot start: olive-orch holds 8 open tasks"),
            "{how}: {said}"
        );
        assert!(said.contains("fix/under (in_progress)"), "{how}: {said}");
        assert_eq!(f.store.get("work/next").unwrap().status, "ready", "{how}");
    }
}

#[test]
fn work_started_under_a_red_as_it_closes_is_decided_again_and_takes_a_slot() {
    for Closer { how, writer, close } in closers() {
        let f = beside_a_red(8, false);
        let other = writer(&f);
        // The claim reads the red open, so the work under it takes no slot; before it writes, the
        // red closes.
        let racing = f
            .slow(move || {
                close(&other).unwrap();
            })
            .capping(Some(8));
        assert_eq!(
            refused(racing.claim(UNDER, W)),
            format!("{UNDER} cannot start: {HOLDS_EIGHT}{}", reserve_hint(UNDER)),
            "{how}"
        );
        assert_eq!(f.store.get(UNDER).unwrap().status, "ready", "{how}");
    }
}

/// How long a race's side waits for the other before the test fails rather than hangs: the backstop
/// of the minute it once had.
pub(super) const WAIT: Duration = test_time::backstop(Duration::from_secs(60));

#[test]
fn a_red_marked_as_the_owner_cancels_it_is_decided_again_and_a_claim_read_between_counts_it() {
    // A red filed before the mark, in review with work in progress under it, all O's: eight slots
    // counted, the red's by the work under it.
    let mut seed = sessions();
    let led = (EIGHT[..7].iter()).chain(&[("work/next", "ready"), (RED, "review")]);
    for (slug, status) in led {
        seed = with(seed, slug, status, None, Some(O));
    }
    let f = Fixture::new(with(seed, UNDER, "in_progress", Some(RED), None));
    let (read, owner_read) = crate::test_channel::channel();
    let (go, owner_go) = crate::test_channel::channel();
    let (finished, owner_finished) = crate::test_channel::channel();
    let owner_go = Mutex::new(owner_go);
    // The owner's cancel reads the red unmarked, then waits.
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
    // Its filer marks it, so O counts seven; a claim reads seven, and before it writes the cancel
    // commits, the work under the red counted again.
    f.store.mark_red(RED, "main", F).unwrap();
    let owner_finished = Mutex::new(owner_finished);
    let claiming = f
        .slow(move || {
            go.send(()).unwrap();
            owner_finished.lock().unwrap().recv_timeout(WAIT).unwrap();
        })
        .capping(Some(8));
    let said = refused(claiming.claim("work/next", K));
    let cancelled = cancel.join().unwrap().unwrap();
    assert_eq!(cancelled.status, "cancelled", "{cancelled:?}");
    assert!(
        said.starts_with("work/next cannot start: olive-orch holds 8 open tasks"),
        "{said}"
    );
    assert!(said.contains("fix/under (in_progress)"), "{said}");
    let next = f.store.get("work/next").unwrap();
    assert_eq!(next.status, "ready", "{next:?}");
}
