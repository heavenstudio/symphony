//! A cancel by a role holder, with its reason (`move_with`): its executor or orchestrator cancels a
//! task in any open stage, the reason a post of its own right after the move's line, in the move's
//! write; a session holding nothing on started work is refused and writes nothing, and so is a
//! reason with no words.

use crate::common::{Fixture, Seed, refused};

const SLUG: &str = "sim/jump";

/// `SLUG` in `status`: `ochre-owl` orchestrates it, `amber-heron` executes it unless it is work not
/// started yet, and `calm-vole` holds nothing.
fn board(status: &str) -> Fixture {
    let mut seed = Seed::default()
        .session("ochre-owl")
        .session("amber-heron")
        .session("calm-vole")
        .task(SLUG, status)
        .role(SLUG, "ochre-owl", "orchestrator");
    if !symphony_board::BEFORE_WORK.contains(&status) {
        seed = seed.role(SLUG, "amber-heron", "executor");
    }
    Fixture::new(seed)
}

/// The kind, sender and text of `slug`'s last two lines.
fn last_two(f: &Fixture, slug: &str) -> Vec<(String, String, String)> {
    let thread = f.store.thread(slug).unwrap();
    thread[thread.len().saturating_sub(2)..]
        .iter()
        .map(|m| (m.kind.clone(), m.sender.clone(), m.text.clone()))
        .collect()
}

fn line(kind: &str, sender: &str, text: &str) -> (String, String, String) {
    (kind.into(), sender.into(), text.into())
}

#[test]
fn its_orchestrator_cancels_it_in_any_open_stage_saying_why() {
    for status in [
        "backlog",
        "concept",
        "design",
        "ready",
        "in_progress",
        "review",
        "blocked",
    ] {
        let f = board(status);
        let task = f
            .store
            .move_with(
                SLUG,
                "cancelled",
                "ochre-owl",
                Some("  superseded by sim/fall \n"),
            )
            .unwrap();
        assert_eq!(task.status, "cancelled", "{status}");
        assert_eq!(
            last_two(&f, SLUG),
            [
                line(
                    "system",
                    "ochre-owl",
                    &format!("ochre-owl moved it {status} → cancelled")
                ),
                line("msg", "ochre-owl", "cancelled: superseded by sim/fall"),
            ],
            "{status}"
        );
    }
}

#[test]
fn its_executor_cancels_started_work_saying_why() {
    for status in ["in_progress", "review", "blocked"] {
        let f = board(status);
        f.store
            .move_with(
                SLUG,
                "cancelled",
                "amber-heron",
                Some("its fix landed with sim/fall"),
            )
            .unwrap();
        assert_eq!(f.store.get(SLUG).unwrap().status, "cancelled", "{status}");
        assert_eq!(
            last_two(&f, SLUG)[1],
            line(
                "msg",
                "amber-heron",
                "cancelled: its fix landed with sim/fall"
            )
        );
    }
}

#[test]
fn a_session_holding_nothing_on_started_work_cancels_none_and_writes_nothing() {
    let f = board("in_progress");
    let before = f.rows();
    assert_eq!(
        refused(
            f.store
                .move_with(SLUG, "cancelled", "calm-vole", Some("not mine"))
        ),
        "calm-vole holds no role on sim/jump, which is in_progress"
    );
    assert_eq!(f.rows(), before);
}

#[test]
fn a_reason_needs_words_and_a_move_without_one_posts_nothing_more() {
    let f = board("in_progress");
    for empty in ["", "  \n"] {
        assert_eq!(
            refused(
                f.store
                    .move_with(SLUG, "cancelled", "amber-heron", Some(empty))
            ),
            "a move's reason says why it moves, in words"
        );
    }
    assert_eq!(f.store.get(SLUG).unwrap().status, "in_progress");
    let lines = f.store.thread(SLUG).unwrap().len();
    f.store.move_to(SLUG, "cancelled", "amber-heron").unwrap();
    let thread = f.store.thread(SLUG).unwrap();
    assert_eq!(thread.len(), lines + 1, "the move's own line alone");
    assert_eq!(
        thread.last().unwrap().text,
        "amber-heron moved it in_progress → cancelled"
    );
}

#[test]
fn any_move_says_its_reason_and_a_refused_one_says_none() {
    let f = board("in_progress");
    f.store
        .move_with(
            SLUG,
            "blocked",
            "amber-heron",
            Some("waits on the owner's card"),
        )
        .unwrap();
    assert_eq!(
        last_two(&f, SLUG)[1],
        line("msg", "amber-heron", "blocked: waits on the owner's card")
    );
    let lines = f.store.thread(SLUG).unwrap().len();
    assert!(
        f.store
            .move_with(SLUG, "done", "amber-heron", Some("finished"))
            .is_err()
    );
    assert_eq!(f.store.thread(SLUG).unwrap().len(), lines);
}

#[test]
fn the_owner_cancels_saying_why() {
    let f = board("review");
    f.store
        .move_with(SLUG, "cancelled", "owner", Some("not needed now"))
        .unwrap();
    assert_eq!(f.store.get(SLUG).unwrap().status, "cancelled");
    assert_eq!(
        last_two(&f, SLUG),
        [
            line("system", "owner", "owner moved it review → cancelled"),
            line("msg", "owner", "cancelled: not needed now"),
        ]
    );
    // A refused owner's move posts no reason either.
    let lines = f.store.thread(SLUG).unwrap().len();
    assert!(
        f.store
            .move_with(SLUG, "cancelled", "owner", Some("again"))
            .is_err()
    );
    assert_eq!(f.store.thread(SLUG).unwrap().len(), lines);
}
