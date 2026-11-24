//! The cap on an orchestrator's open tasks: where it refuses a start, what it counts (red work it
//! does not), where a slot frees, and that it takes nothing from an orchestrator already above it.
//! Moves, dependency
//! changes, the tree reshaped and races are [`moves`]'; a red's mark against writes that read
//! before it changed, [`red_marks`]'; what the cap costs, [`cost`]'s.

mod cost;
mod features;
mod holdings;
mod moves;
mod red_marks;
mod reservations;

use crate::common::{Fixture, Seed, refused, session, task};
use serde_json::json;
use symphony_board::rows::{SessionRow, TaskRow};
use symphony_board::{NewTask, Store};

/// The orchestrator every test's open work is under.
const O: &str = "olive-orch";
/// A worker O registered (`--parent olive-orch`).
const W: &str = "wren";
/// A worker with no parent.
const K: &str = "kite";
/// Another orchestrator, holding nothing.
const P: &str = "pine-orch";
/// A red's filer, a session as the tools that file reds register ([`symphony_board::red::FILER`]).
const F: &str = "flint";

/// Eight tasks in progress, the open tasks the cap counts (work in review it does not).
const EIGHT: [(&str, &str); 8] = [
    ("work/a", "in_progress"),
    ("work/b", "in_progress"),
    ("work/c", "in_progress"),
    ("work/d", "in_progress"),
    ("work/e", "in_progress"),
    ("work/f", "in_progress"),
    ("work/g", "in_progress"),
    ("work/h", "in_progress"),
];

/// What a claim refused at the cap adds for the executor holding the reservation.
fn reserve_hint(slug: &str) -> String {
    format!(
        "; if the task is reserved for you, start work under the reservation now, renew it with `just tasks reserve {slug}` within each hour and retry the claim after each renewal"
    )
}

/// What O holding [`EIGHT`] is said as.
const HOLDS_EIGHT: &str = "olive-orch holds 8 open tasks, the cap of 8 an orchestrator holds: work/a (in_progress), work/b (in_progress), work/c (in_progress), work/d (in_progress), work/e (in_progress), work/f (in_progress), work/g (in_progress), work/h (in_progress); a slot frees when one of them goes to review or is done or cancelled";

/// A board to seed: O and P orchestrators, W (O's child), K and the filer F.
fn sessions() -> Seed {
    let kind = |handle: &str, kind: &str| SessionRow {
        kind: kind.into(),
        ..session(handle)
    };
    Seed::default()
        .session_row(kind(O, "orchestrator"))
        .session_row(kind(P, "orchestrator"))
        .session_row(SessionRow {
            parent: Some(O.into()),
            ..session(W)
        })
        .session(K)
        .session_row(kind(F, symphony_board::red::FILER))
}

/// `seed` with a task `slug` in `status` under `parent`, its orchestrator `by` if any.
fn with(seed: Seed, slug: &str, status: &str, parent: Option<&str>, by: Option<&str>) -> Seed {
    let row = TaskRow {
        id: seed.dump.tasks.len() as i64 + 1,
        parent: parent.map(String::from),
        ..task(slug, status)
    };
    let seed = seed.task_row(row);
    match by {
        Some(by) => seed.role(slug, by, "orchestrator"),
        None => seed,
    }
}

/// O orchestrating each of `led`, beside the tasks `free` nobody orchestrates.
fn board(led: &[(&str, &str)], free: &[(&str, &str)]) -> Fixture {
    let mut seed = sessions();
    for (slug, status) in led {
        seed = with(seed, slug, status, None, Some(O));
    }
    for (slug, status) in free {
        seed = with(seed, slug, status, None, None);
    }
    Fixture::new(seed)
}

/// The seed of [`EIGHT`] under O, and `more` it orchestrates.
fn eight_and_seed(more: &[(&str, &str)]) -> Seed {
    let mut seed = sessions();
    for (slug, status) in EIGHT.iter().chain(more) {
        seed = with(seed, slug, status, None, Some(O));
    }
    seed
}

/// [`EIGHT`] under O, and `more` it orchestrates.
fn eight_and(more: &[(&str, &str)]) -> Fixture {
    Fixture::new(eight_and_seed(more))
}

/// A store on `f`'s board holding starts to `cap` open tasks an orchestrator.
fn capped(f: &Fixture, cap: usize) -> Store {
    f.other().capping(Some(cap))
}

/// The open tasks on the board, by slug.
fn open(store: &Store) -> Vec<String> {
    let mut open: Vec<String> = (store.tasks(None).unwrap().into_iter())
        .filter(|t| t.status == "in_progress" || t.status == "review")
        .map(|t| t.slug)
        .collect();
    open.sort();
    open
}

#[test]
fn the_ninth_claim_under_one_orchestrator_is_refused_naming_the_eight_and_their_status() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {HOLDS_EIGHT}{}",
            reserve_hint("work/next")
        )
    );
    let next = store.get("work/next").unwrap();
    assert_eq!(
        (next.status.as_str(), next.holders("executor")),
        ("ready", vec![])
    );
    assert!(f.said("work/next").is_empty(), "nothing written");
    assert!(
        next.holders("reserved").is_empty(),
        "a cap refusal reserves nothing: {next:?}"
    );
}

#[test]
fn a_store_with_no_cap_refuses_no_start() {
    let f = eight_and(&[("work/next", "ready")]);
    assert_eq!(f.store.slots(), None);
    assert_eq!(f.store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(open(&f.store).len(), 9);
}

#[test]
fn up_to_the_cap_each_claim_starts_and_blocked_or_unstarted_work_holds_no_slot() {
    let led: Vec<(&str, &str)> = EIGHT[..7]
        .iter()
        .copied()
        .chain([
            ("held/blocked", "blocked"),
            ("held/ready", "ready"),
            ("held/backlog", "backlog"),
            ("held/concept", "concept"),
            ("held/design", "design"),
            ("work/next", "ready"),
            ("work/later", "ready"),
        ])
        .collect();
    let f = board(&led, &[]);
    let store = capped(&f, 8);
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let said = refused(store.claim("work/later", K));
    assert!(
        said.starts_with("work/later cannot start: olive-orch holds 8 open tasks"),
        "{said}"
    );
    assert!(said.contains("work/next (in_progress)"), "{said}");
    assert!(!said.contains("held/"), "{said}");
}

#[test]
fn work_in_review_takes_no_slot_and_a_refusal_names_it_apart() {
    let led: Vec<(&str, &str)> = EIGHT
        .iter()
        .map(|(slug, _)| (*slug, "review"))
        .chain([("work/next", "ready")])
        .collect();
    let f = board(&led, &[]);
    assert_eq!(
        capped(&f, 1).claim("work/next", K).unwrap().status,
        "in_progress",
        "eight in review take none of a cap of one"
    );
    let f = eight_and(&[
        ("work/i", "review"),
        ("work/j", "review"),
        ("work/next", "ready"),
    ]);
    assert_eq!(
        refused(capped(&f, 8).claim("work/next", K)),
        format!(
            "work/next cannot start: {HOLDS_EIGHT}; not counted, as work in review takes no slot: work/i (review), work/j (review){}",
            reserve_hint("work/next")
        )
    );
}

#[test]
fn a_reviewers_instruction_and_mirror_under_a_task_in_review_are_claimed_at_the_cap() {
    let seed = eight_and_seed(&[("work/r", "review"), ("work/next", "ready")]);
    let id = seed.dump.tasks.len() as i64 + 1;
    let seed = seed.task_row(TaskRow {
        id,
        parent: Some("work/r".into()),
        instruction: true,
        ..task("work/r/review", "ready")
    });
    let f = Fixture::new(with(seed, "review/work/r", "ready", None, None));
    let store = capped(&f, 8);
    // The reviewer's instruction subtask, as the skills staff one, and a mirror, at the cap.
    assert_eq!(
        store.claim("work/r/review", K).unwrap().status,
        "in_progress"
    );
    assert_eq!(
        store.claim("review/work/r", W).unwrap().status,
        "in_progress"
    );
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {HOLDS_EIGHT}; not counted, as work in review takes no slot: work/r (review) with review/work/r (in_progress) with work/r/review (in_progress){}",
            reserve_hint("work/next")
        )
    );
}

#[test]
fn a_move_into_review_frees_a_slot_and_a_changes_verdict_returns_the_work_past_the_cap() {
    let f = Fixture::new(
        eight_and_seed(&[("work/next", "ready"), ("work/later", "ready")]).session("lark"),
    );
    let store = capped(&f, 8);
    refused(store.claim("work/next", K));
    assert_eq!(
        store.move_to("work/h", "review", O).unwrap().status,
        "review"
    );
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    // The reviewer asks for changes: work/h was being worked already, so it goes back past the cap.
    store.take_role("work/h", "lark", "reviewer").unwrap();
    let back = store
        .verdict("work/h", "lark", "changes", "one more case")
        .unwrap();
    assert_eq!(back.status, "in_progress");
    // So does a move back from review, and both count from then.
    store.move_to("work/g", "review", O).unwrap();
    assert_eq!(
        store.move_to("work/g", "in_progress", O).unwrap().status,
        "in_progress"
    );
    let said = refused(store.claim("work/later", K));
    assert!(
        said.starts_with("work/later cannot start: olive-orch holds 9 open tasks"),
        "{said}"
    );
    assert!(said.contains("work/h (in_progress)"), "{said}");
    assert!(!said.contains("not counted"), "{said}");
}

#[test]
fn a_review_mirror_counts_once_with_the_task_it_reviews() {
    let mut seed = sessions();
    for (slug, status) in &EIGHT[..7] {
        seed = with(seed, slug, status, None, Some(O));
    }
    // Mirrors a reviewer works in progress; in review they take no slot, as all work there.
    seed = with(seed, "review/work/a", "in_progress", None, None);
    seed = with(seed, "work/b/review", "in_progress", Some("work/b"), None);
    seed = with(seed, "review-2/work/c", "in_progress", None, Some(O));
    seed = with(seed, "work/next", "ready", None, Some(O));
    seed = with(seed, "work/later", "ready", None, Some(O));
    let f = Fixture::new(seed);
    let store = capped(&f, 8);
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    let said = refused(store.claim("work/later", K));
    assert!(said.contains("holds 8 open tasks"), "{said}");
    for slot in [
        "work/a (in_progress) with review/work/a (in_progress), ",
        "work/b (in_progress) with work/b/review (in_progress), ",
        "work/c (in_progress) with review-2/work/c (in_progress), ",
    ] {
        assert!(said.contains(slot), "{slot}: {said}");
    }
}

#[test]
fn done_or_cancelled_frees_a_slot() {
    let f = eight_and(&[("work/next", "ready"), ("work/later", "ready")]);
    let store = capped(&f, 8);
    refused(store.claim("work/next", K));
    let landed = store
        .landed("work/b", O, "landed on main", false, &["in_progress"])
        .unwrap();
    assert_eq!(landed.task.status, "done");
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    refused(store.claim("work/later", K));
    store.move_to("work/a", "cancelled", "owner").unwrap();
    assert_eq!(store.claim("work/later", K).unwrap().status, "in_progress");
    assert_eq!(open(&store).len(), 8);
}

/// A task closed as covered by another's landing is done: its slot frees as a landing's does.
#[test]
fn a_covered_close_frees_a_slot() {
    let f = eight_and(&[("work/next", "ready"), ("work/landed", "review")]);
    let store = capped(&f, 8);
    store
        .landed(
            "work/landed",
            O,
            "landed work/landed on main at 0123456789ab",
            false,
            &["review"],
        )
        .unwrap();
    refused(store.claim("work/next", K));
    let closed = store
        .close_covered(
            "work/c",
            O,
            "work/landed",
            "0123456789ab",
            "its fix landed there",
        )
        .unwrap();
    assert_eq!(closed.status, "done");
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
    assert_eq!(open(&store).len(), 8);
}

#[test]
fn a_holder_above_the_cap_keeps_its_tasks_works_them_and_starts_nothing_new() {
    let f = eight_and(&[
        ("work/i", "in_progress"),
        ("work/j", "review"),
        ("work/next", "ready"),
    ]);
    let store = capped(&f, 8);
    let said = refused(store.claim("work/next", K));
    assert!(said.contains("olive-orch holds 9 open tasks"), "{said}");
    assert_eq!(open(&store).len(), 10, "not retroactive: every task kept");
    assert!(
        (store.tasks(None).unwrap().iter())
            .filter(|t| t.slug.starts_with("work/") && t.slug != "work/next")
            .all(|t| t.holders("orchestrator") == [O]),
        "still its own"
    );
    // Its open work goes on: a claim of a task already open, review and back.
    store.claim("work/i", K).unwrap();
    store.move_to("work/i", "review", K).unwrap();
    store.move_to("work/i", "in_progress", K).unwrap();
    // Below the cap again, it starts.
    for slug in ["work/b", "work/d", "work/f"] {
        store
            .landed(slug, O, "landed", false, &["in_progress"])
            .unwrap();
    }
    assert_eq!(store.claim("work/next", K).unwrap().status, "in_progress");
}

#[test]
fn a_move_into_in_progress_from_blocked_or_ready_is_refused_at_the_cap() {
    let f = eight_and(&[("work/held", "ready"), ("work/next", "ready")]);
    let store = f.other();
    store.claim("work/held", K).unwrap();
    store.move_to("work/held", "blocked", K).unwrap();
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.move_to("work/held", "in_progress", K)),
        format!("work/held cannot go from blocked to in_progress: {HOLDS_EIGHT}")
    );
    assert_eq!(
        refused(store.move_to("work/next", "in_progress", K)),
        format!("work/next cannot go from ready to in_progress: {HOLDS_EIGHT}")
    );
    // A slot freed lets it go on.
    store
        .landed("work/h", O, "landed", false, &["in_progress"])
        .unwrap();
    assert_eq!(
        store.move_to("work/held", "in_progress", K).unwrap().status,
        "in_progress"
    );
}

#[test]
fn taking_orchestrator_on_open_work_is_refused_at_the_cap_and_on_its_own_or_unstarted_work_is_not()
{
    let mut seed = sessions();
    for (slug, status) in &EIGHT {
        seed = with(seed, slug, status, None, Some(O));
    }
    // A regular child shares work/a's slot, including when O takes its local role.
    seed = with(seed, "work/a/part", "in_progress", Some("work/a"), None);
    seed = with(seed, "loose", "in_progress", None, None);
    seed = with(seed, "later", "ready", None, None);
    seed = with(seed, "later/one", "in_progress", Some("later"), None);
    let f = Fixture::new(seed);
    let store = capped(&f, 8);
    let holds = HOLDS_EIGHT.replace(
        "work/a (in_progress)",
        "work/a (in_progress) with work/a/part (in_progress)",
    );
    assert_eq!(
        refused(store.take_role("loose", O, "orchestrator")),
        format!("olive-orch cannot take orchestrator on loose, which is in_progress: {holds}")
    );
    assert_eq!(
        store.get("loose").unwrap().holders("orchestrator"),
        Vec::<&str>::new()
    );
    // later is not open, but the work below it is, and would be O's.
    assert!(
        refused(store.take_role("later", O, "orchestrator")).starts_with(
            "olive-orch cannot take orchestrator on later, which is ready: olive-orch holds 8"
        ),
    );
    store.take_role("work/a/part", O, "orchestrator").unwrap();
    store.take_role("loose", P, "orchestrator").unwrap();
    store.take_role("later", P, "orchestrator").unwrap();
    // A reviewer takes no slot.
    store.take_role("work/b", K, "reviewer").unwrap();
}

#[test]
fn a_task_no_orchestrator_holds_counts_against_the_claimers_parent() {
    let f = board(&EIGHT, &[("loose/next", "ready")]);
    let store = capped(&f, 8);
    assert_eq!(
        refused(store.claim("loose/next", W)),
        format!(
            "loose/next cannot start: olive-orch (no orchestrator holds it: olive-orch is the parent of wren) {}{}",
            HOLDS_EIGHT.trim_start_matches("olive-orch "),
            reserve_hint("loose/next")
        )
    );
    // A worker with no parent counts it against no one.
    assert_eq!(store.claim("loose/next", K).unwrap().status, "in_progress");
}

#[test]
fn open_work_no_orchestrator_holds_takes_a_slot_of_its_executors_parent() {
    let led: Vec<(&str, &str)> = EIGHT[..7]
        .iter()
        .copied()
        .chain([("work/next", "ready")])
        .collect();
    let f = board(&led, &[("loose/one", "ready")]);
    f.other().claim("loose/one", W).unwrap();
    let said = refused(capped(&f, 8).claim("work/next", K));
    assert!(said.contains("holds 8 open tasks"), "{said}");
    assert!(said.contains("loose/one (in_progress)"), "{said}");
}

/// `slug`, filed for a red of `what` by its filer F, most urgent, as land files one.
fn red(store: &Store, slug: &str, what: &str) {
    let task = store
        .create(&NewTask {
            priority: 0,
            red: Some(what.into()),
            ..NewTask::new(slug, &format!("Red: {slug}"), F)
        })
        .unwrap();
    assert!(task.red(), "{slug} is marked");
}

#[test]
fn an_orchestrator_at_the_cap_still_starts_main_and_nightly_red_work_and_names_it_apart() {
    let seed = eight_and_seed(&[("work/next", "ready")]).session("lark");
    let f = Fixture::new(seed);
    let store = capped(&f, 8);
    red(&store, "tests/main-red-abc", "main");
    red(&store, "tests/nightly-red-sim", "nightly");
    (store.create(&NewTask {
        parent: Some("tests/main-red-abc".into()),
        ..NewTask::new("fix/static", "Fix static", "owner")
    }))
    .unwrap();
    store
        .take_role("tests/nightly-red-sim", O, "orchestrator")
        .unwrap();
    // A red no orchestrator holds counts against its claimer's parent: O, at the cap.
    assert_eq!(
        store.claim("tests/main-red-abc", W).unwrap().status,
        "in_progress"
    );
    assert_eq!(
        store.claim("tests/nightly-red-sim", K).unwrap().status,
        "in_progress"
    );
    // Taking the role on open red work, and starting the work under a red, are not capped either.
    store
        .take_role("tests/main-red-abc", O, "orchestrator")
        .unwrap();
    assert_eq!(
        store.claim("fix/static", "lark").unwrap().status,
        "in_progress"
    );
    // Historical reviewed red children remain excluded from the feature cap.
    f.historical_review("fix/static");
    assert_eq!(open(&store).len(), 11);
    // A feature at the cap is refused as before, the red work named apart as not counted.
    assert_eq!(
        refused(store.claim("work/next", K)),
        format!(
            "work/next cannot start: {HOLDS_EIGHT}; not counted, as red work takes no slot: tests/main-red-abc (in_progress) with fix/static (review), tests/nightly-red-sim (in_progress){}",
            reserve_hint("work/next")
        )
    );
    assert_eq!(store.get("work/next").unwrap().status, "ready");
}

#[test]
fn a_cancelled_reds_children_share_one_counted_feature_without_exempting_other_work() {
    let f = Fixture::new(eight_and_seed(&[]));
    let store = capped(&f, 8);
    red(&store, "tests/main-red-old", "main");
    store
        .create(&NewTask {
            parent: Some("tests/main-red-old".into()),
            ..NewTask::new("fix/early", "Early", "owner")
        })
        .unwrap();
    store
        .take_role("tests/main-red-old", O, "orchestrator")
        .unwrap();
    // Started under the open red at the cap: red work, taking no slot.
    store.claim("fix/early", W).unwrap();
    store.move_to("tests/main-red-old", "cancelled", O).unwrap();
    // Once the red is cancelled its active children hold one counted feature slot. More work in
    // that same feature can join it, while an unrelated feature is still capped.
    store
        .create(&NewTask {
            parent: Some("tests/main-red-old".into()),
            ..NewTask::new("fix/late", "Late", "owner")
        })
        .unwrap();
    assert_eq!(store.claim("fix/late", W).unwrap().status, "in_progress");
    store
        .create(&NewTask::new("unrelated", "Unrelated", "owner"))
        .unwrap();
    store.take_role("unrelated", O, "orchestrator").unwrap();
    let said = refused(store.claim("unrelated", K));
    assert!(said.contains("holds 9 open tasks"), "{said}");
    assert!(
        said.contains("fix/early (in_progress) with fix/late (in_progress)"),
        "{said}"
    );
    assert!(!said.contains("not counted"), "{said}");
    assert!(said.ends_with(&reserve_hint("unrelated")), "{said}");
}

#[test]
fn a_filer_marks_no_feature_red_so_the_cap_still_refuses_its_claim() {
    let f = eight_and(&[("work/next", "ready")]);
    let store = capped(&f, 8);
    let full = format!(
        "work/next cannot start: {HOLDS_EIGHT}{}",
        reserve_hint("work/next")
    );
    assert_eq!(refused(store.claim("work/next", K)), full);
    // A filer, which any worker may register, marks the feature the cap refused: no red series'.
    let said = refused(store.mark_red("work/next", "main", F));
    assert!(
        said.starts_with("work/next is no red series' own task"),
        "{said}"
    );
    assert!(!store.get("work/next").unwrap().red());
    assert_eq!(refused(store.claim("work/next", K)), full);
    assert_eq!(store.get("work/next").unwrap().status, "ready");
}

#[test]
fn a_task_only_named_like_a_red_takes_a_slot() {
    let f = eight_and(&[("tests/main-red-old", "ready")]);
    assert_eq!(
        refused(capped(&f, 8).claim("tests/main-red-old", K)),
        format!(
            "tests/main-red-old cannot start: {HOLDS_EIGHT}{}",
            reserve_hint("tests/main-red-old")
        )
    );
}

#[test]
fn a_red_with_nothing_named_red_is_refused() {
    let f = Fixture::new(sessions());
    assert_eq!(
        refused(f.store.create(&NewTask {
            red: Some(" ".into()),
            ..NewTask::new("tests/main-red-blank", "Red", F)
        })),
        "a red task names what is red, such as main"
    );
    assert!(
        !f.store
            .create(&NewTask::new("plain/x", "X", "owner"))
            .unwrap()
            .red()
    );
}

#[test]
fn two_starts_at_once_under_one_orchestrator_take_one_slot() {
    let f = board(&seven_and_two(), &[]);
    let first = capped(&f, 8);
    // The second reads the board with one slot free, then the first takes it before it writes.
    let second = f
        .slow(move || {
            first.claim("work/other", K).unwrap();
        })
        .capping(Some(8));
    let said = refused(second.claim("work/next", K));
    assert!(
        said.starts_with("work/next cannot start: olive-orch holds 8"),
        "{said}"
    );
    assert!(said.contains("work/other (in_progress)"), "{said}");
    assert_eq!(f.store.get("work/next").unwrap().status, "ready");
    assert_eq!(open(&f.store).len(), 8);
}

/// Two starts meeting on O's slot counter (`counter:['slots', O]`), one written between the
/// other's read and its write: the first moves the counter once and takes the last slot; the
/// later one's write finds the counter moved from where its read held it, reads the board
/// again once, and is refused at the cap, writing nothing.
#[test]
fn two_starts_meeting_on_one_counter_move_it_once_and_the_later_reads_again_once() {
    let f = board(&seven_and_two(), &[]);
    let before = slot_counter(&f);
    let first = capped(&f, 8);
    let second = f
        .slow(move || {
            first.claim("work/other", K).unwrap();
        })
        .capping(Some(8));
    let tried = Store::tried_again_here();
    let said = refused(second.claim("work/next", K));
    assert!(
        said.starts_with("work/next cannot start: olive-orch holds 8"),
        "{said}"
    );
    assert_eq!(
        Store::tried_again_here() - tried,
        1,
        "the later start read the board again once"
    );
    assert_eq!(
        slot_counter(&f),
        before + 1,
        "the counter moved once, by the start that won"
    );
    let (next, other) = (
        f.store.get("work/next").unwrap(),
        f.store.get("work/other").unwrap(),
    );
    assert_eq!(
        (next.status.as_str(), next.holders("executor")),
        ("ready", vec![])
    );
    assert_eq!(
        (other.status.as_str(), other.holders("executor")),
        ("in_progress", vec![K])
    );
    assert!(
        f.said("work/next").is_empty(),
        "the refused start wrote nothing"
    );
    assert_eq!(open(&f.store).len(), 8);
}

/// Two starts sent at once from two threads for O's last slot, round after round, a slot
/// freed between rounds: each round one start wins and one is refused at the cap, whichever
/// commits first and however the two transactions overlap, and the counter moves once a win.
#[test]
fn two_starts_at_once_for_the_last_slot_win_once_each_round() {
    const ROUNDS: usize = 6;
    let mut led = EIGHT[..7].to_vec();
    let slugs: Vec<(String, String)> = (0..ROUNDS)
        .map(|round| (format!("race/{round}/a"), format!("race/{round}/b")))
        .collect();
    for (a, b) in &slugs {
        led.extend([(a.as_str(), "ready"), (b.as_str(), "ready")]);
    }
    let f = board(&led, &[]);
    let (one, two) = (capped(&f, 8), capped(&f, 8));
    let before = slot_counter(&f);
    let mut won = Vec::new();
    for (round, (a, b)) in slugs.iter().enumerate() {
        let start = std::sync::Barrier::new(2);
        let (left, right) = std::thread::scope(|scope| {
            let left = scope.spawn(|| {
                start.wait();
                one.claim(a, K)
            });
            let right = scope.spawn(|| {
                start.wait();
                two.claim(b, K)
            });
            (left.join().unwrap(), right.join().unwrap())
        });
        let winner = match (left, right) {
            (Ok(_), Err(symphony_board::Error::Refused(why))) => {
                assert!(
                    why.starts_with(&format!("{b} cannot start: olive-orch holds 8")),
                    "{why}"
                );
                a
            }
            (Err(symphony_board::Error::Refused(why)), Ok(_)) => {
                assert!(
                    why.starts_with(&format!("{a} cannot start: olive-orch holds 8")),
                    "{why}"
                );
                b
            }
            other => panic!("round {round}: one start wins and one is refused, not {other:?}"),
        };
        let working = (f.store.tasks(Some("in_progress")).unwrap()).len();
        assert_eq!(working, 8, "round {round}: the cap's eight in progress");
        assert_eq!(
            slot_counter(&f),
            before + won.len() as i64 + 1,
            "round {round}"
        );
        won.push(winner.clone());
        // The winner in review frees the slot for the next round.
        f.store.move_to(winner, "review", K).unwrap();
    }
    assert_eq!(won.len(), ROUNDS);
    for (a, b) in &slugs {
        let started = [a, b].map(|slug| f.store.get(slug).unwrap().holders("executor").len());
        assert_eq!(
            started.iter().sum::<usize>(),
            1,
            "one of {a} and {b} started: {started:?}"
        );
    }
}

/// Seven of [`EIGHT`] under O, and the two ready tasks two starts meet over, `work/next` and
/// `work/other`, both its.
fn seven_and_two() -> Vec<(&'static str, &'static str)> {
    (EIGHT[..7].iter().copied())
        .chain([("work/next", "ready"), ("work/other", "ready")])
        .collect()
}

/// O's slot counter, `counter:['slots', O]`, as the board holds it: none read as 0.
fn slot_counter(f: &Fixture) -> i64 {
    let last = f
        .store
        .raw()
        .row("counter", json!(["slots", O]))
        .field("last");
    last.unwrap().as_i64().unwrap_or(0)
}
