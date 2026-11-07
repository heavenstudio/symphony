//! The cap where work moves back into an open status: from blocked, through a changed dependency,
//! from a landing's hold; where a change of parent or a hand-over brings open work under an
//! orchestrator; on a task two orchestrators hold; and a start racing the close of the slot it
//! joins.

use super::{EIGHT, HOLDS_EIGHT, K, O, P, W, board, capped, eight_and, open, sessions, with};
use crate::common::{Fixture, refused, task};
use serde_json::json;
use symphony_board::Store;
use symphony_board::rows::TaskRow;

/// `slugs` as `change_depends` takes them.
fn owned(slugs: &[&str]) -> Vec<String> {
    slugs.iter().map(|s| (*s).to_string()).collect()
}

#[test]
fn work_blocked_from_in_progress_cannot_go_round_the_cap_through_review() {
    let f = eight_and(&[("work/held", "ready")]);
    let free = f.other();
    free.claim("work/held", K).unwrap();
    free.move_to("work/held", "blocked", K).unwrap();
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.move_to("work/held", "review", K)),
        format!("work/held cannot go from blocked to review: {HOLDS_EIGHT}")
    );
    assert_eq!(store.get("work/held").unwrap().status, "blocked");
    assert_eq!(
        refused(store.move_to("work/held", "in_progress", K)),
        format!("work/held cannot go from blocked to in_progress: {HOLDS_EIGHT}")
    );
}

/// O holding [`EIGHT`] with work/h's place taken by work/next, in progress.
fn holds_next() -> String {
    HOLDS_EIGHT.replace("work/h (review)", "work/next (in_progress)")
}

#[test]
fn work_blocked_from_review_by_a_move_takes_a_slot_to_come_back() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    // The way round the cap a review found: block a task in review, start new work in its slot,
    // and move it back.
    store.move_to("work/h", "blocked", O).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.move_to("work/h", "review", O)),
        format!("work/h cannot go from blocked to review: {}", holds_next())
    );
    assert_eq!(store.get("work/h").unwrap().status, "blocked");
    assert_eq!(open(&store).len(), 8);
}

#[test]
fn a_landings_hold_keeps_its_tasks_slot_and_returns_it_to_review_at_the_cap() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    // land-stamps holds work/h, in review, while nobody makes a stamp it lacks: its slot stays
    // taken meanwhile.
    assert_eq!(store.hold_blocked("work/h", O).unwrap().status, "blocked");
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {}",
            HOLDS_EIGHT.replace("work/h (review)", "work/h (blocked, held)")
        )
    );
    // The stamp arrives: back to review, at the cap.
    assert_eq!(
        store.move_to("work/h", "review", O).unwrap().status,
        "review"
    );
    assert_eq!(open(&store).len(), 8);
    // That move ended the hold: blocked again by a move, the slot is free and the way back capped.
    store.move_to("work/h", "blocked", O).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.move_to("work/h", "review", O)),
        format!("work/h cannot go from blocked to review: {}", holds_next())
    );
    assert_eq!(
        refused(store.hold_blocked("work/h", O)),
        "work/h is blocked: only work in progress or in review is held"
    );
}

/// Whether the board keeps a hold on `slug`.
fn hold_kept(store: &Store, slug: &str) -> bool {
    let sql = "RETURN record::exists(type::record('counter', ['hold', $slug]));";
    let answer = store.client().query(sql, json!({"slug": slug})).unwrap();
    answer == [json!(true)]
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
    store.set_parent("work/h", Some("work/a"), O).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.move_to("work/h", "review", O)),
        format!("work/h cannot go from blocked to review: {}", holds_next())
    );
}

#[test]
fn a_dependency_change_that_would_start_blocked_work_past_the_cap_is_refused() {
    let mut seed = sessions();
    for (slug, status) in EIGHT {
        seed = with(seed, slug, status, None, Some(O));
    }
    seed = with(seed, "work/held", "in_progress", None, Some(O));
    seed = with(seed, "work/next", "ready", None, Some(O));
    for dep in ["dep/old", "dep/two"] {
        seed = with(seed, dep, "ready", None, None);
    }
    let seed = seed
        .role("work/held", K, "executor")
        .link("work/held", "dep", "dep/old")
        .link("work/b", "dep", "dep/two");
    let f = Fixture::new(seed);
    let store = capped(&f, 8);
    // A cancelled prerequisite blocks work/held (was in_progress): eight open again.
    store.move_to("dep/old", "cancelled", "owner").unwrap();
    assert_eq!(store.get("work/held").unwrap().status, "blocked");
    let back = || store.change_depends("work/held", &[], &owned(&["dep/old"]), K);
    assert_eq!(
        refused(back()),
        format!("work/held cannot go from blocked back to in_progress: {HOLDS_EIGHT}")
    );
    assert_eq!(store.get("work/held").unwrap().status, "blocked");
    store.move_to("work/a", "cancelled", "owner").unwrap();
    assert_eq!(back().unwrap().status, "in_progress");
    // Work a cancelled prerequisite blocked in review takes a slot to go back to review.
    store.move_to("dep/two", "cancelled", "owner").unwrap();
    assert_eq!(store.get("work/b").unwrap().status, "blocked");
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let back = || store.change_depends("work/b", &[], &owned(&["dep/two"]), K);
    assert_eq!(
        refused(back()),
        "work/b cannot go from blocked back to review: olive-orch holds 8 open tasks, the cap of 8 an orchestrator holds: work/c (in_progress), work/d (review), work/e (in_progress), work/f (review), work/g (in_progress), work/h (review), work/held (in_progress), work/next (in_progress); a slot frees when one of them is done or cancelled"
    );
    assert_eq!(store.get("work/b").unwrap().status, "blocked");
    store.move_to("work/c", "cancelled", "owner").unwrap();
    assert_eq!(back().unwrap().status, "review");
}

#[test]
fn a_change_of_parent_that_would_bring_open_work_under_a_full_orchestrator_is_refused() {
    let mut seed = sessions();
    for (slug, status) in &EIGHT[..7] {
        seed = with(seed, slug, status, None, Some(O));
    }
    seed = with(seed, "work/a/part", "in_progress", Some("work/a"), None);
    seed = with(seed, "work/next", "ready", None, Some(O));
    seed = with(seed, "loose", "review", None, None);
    let f = Fixture::new(seed.role("work/a/part", K, "executor"));
    let store = capped(&f, 8);
    // The way round the cap a review found: take open work from under the orchestrator's tree,
    // start new work in the slot that frees, and put it back.
    store.set_parent("work/a/part", None, K).unwrap();
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(
        refused(store.set_parent("work/a/part", Some("work/a"), K)),
        format!("work/a/part cannot go under work/a: {}", holds_next())
    );
    assert_eq!(store.get("work/a/part").unwrap().parent, None);
    assert_eq!(
        refused(store.set_parent("loose", Some("work/b"), K)),
        format!("loose cannot go under work/b: {}", holds_next()),
        "open work nobody orchestrated comes under O"
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
            "olive-orch holds 2 open tasks, the cap of 2 an orchestrator holds: {tasks}; a slot frees when one of them is done or cancelled"
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
        "work/a is orchestrated by pine-orch (live); pine-orch hands it over with: just tasks handover work/a olive-orch, which olive-orch's cap decides as it does a take"
    );
    // The owner's hand-over is not capped, as the owner's moves are not.
    store.hand_over("owned", "owner", O).unwrap();
    assert_eq!(store.get("owned").unwrap().holders("orchestrator"), [O]);
}

#[test]
fn open_work_moved_under_one_task_still_holds_a_slot_each_and_an_instruction_shares_its_tasks() {
    let seed = super::eight_and_seed(&[("work/next", "ready")]);
    let id = seed.dump.tasks.len() as i64 + 1;
    let seed = seed.task_row(TaskRow {
        id,
        parent: Some("work/a".into()),
        instruction: true,
        ..task("work/a/build", "ready")
    });
    let f = Fixture::new(seed);
    // The way round the cap a review found: gather the open work under one task.
    for (slug, _) in &EIGHT[1..] {
        f.store.set_parent(slug, Some("work/a"), O).unwrap();
    }
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!("work/next cannot start: {HOLDS_EIGHT}")
    );
    // An instruction for work/a is no work of its own.
    assert_eq!(
        store.claim("work/a/build", K).unwrap().status,
        "in_progress"
    );
    assert_eq!(refused(store.claim("work/next", K)), {
        let shared = HOLDS_EIGHT.replace(
            "work/a (in_progress)",
            "work/a (in_progress) with work/a/build (in_progress)",
        );
        format!("work/next cannot start: {shared}")
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
        format!("both cannot start: {HOLDS_EIGHT}")
    );
    store.move_to("work/a", "cancelled", "owner").unwrap();
    assert_eq!(store.claim("both", K).unwrap().status, "in_progress");
    // It is P's too: at a cap of one, P starts nothing more.
    assert_eq!(
        refused(capped(&f, 1).claim("pine/next", K)),
        "pine/next cannot start: pine-orch holds 1 open tasks, the cap of 1 an orchestrator holds: both (in_progress); a slot frees when one of them is done or cancelled"
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
    seed = with(seed, "work/a", "review", None, Some(O));
    seed = with(seed, "work/b", "in_progress", None, Some(O));
    seed = with(seed, "work/c", "ready", None, Some(O));
    seed = with(seed, "review/work/a", "ready", None, None);
    let f = Fixture::new(seed);
    let other = capped(&f, 2);
    // The mirror's claim reads work/a's slot open, so it opens none; before it writes, work/a is
    // cancelled and work/c takes the slot that freed.
    let racing = f
        .slow(move || {
            other.move_to("work/a", "cancelled", "owner").unwrap();
            other.claim("work/c", K).unwrap();
        })
        .capping(Some(2));
    assert_eq!(
        refused(racing.claim("review/work/a", K)),
        "review/work/a cannot start: olive-orch holds 2 open tasks, the cap of 2 an orchestrator holds: work/b (in_progress), work/c (in_progress); a slot frees when one of them is done or cancelled"
    );
    assert_eq!(open(&f.store), ["work/b", "work/c"]);
}
