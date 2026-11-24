//! A task its reviewer approved and no landing has closed reads done and is still somebody's: the
//! approve marks it (`orchestration::unlanded`), the board gives it on when its orchestrator leaves
//! or is lost, as it gives open work, and the PULL digest lists it under `approved_unlanded`, once.
//! Its landing's line, or a move back to review, lifts the mark, and a task done any other way (a
//! landing from review, a close as covered by another's landing, a subtask's `done`) never carries
//! it.

use super::common::{Fixture, Seed, refused};
use super::orchestrators::go_quiet_but;
use symphony_board::cli::pull::{Digest, UNLANDED, UNLANDED_SECONDS, digest};
use symphony_board::health::LIVE_SECONDS;
use symphony_board::{NewTask, orchestration};

/// The task approved and never landed.
const SLUG: &str = "tools/approved";
/// Its landing's line, as `just land` writes it.
const LANDED: &str = "landed tools/approved on main at 3e0c66192471; pushed to origin";

/// A board where `amber` orchestrates [`SLUG`] and `cobalt` a task of its own.
struct Made {
    f: Fixture,
    amber: String,
    cobalt: String,
    worker: String,
    reviewer: String,
}

/// [`SLUG`] in review, executed by `worker` and read by `reviewer`; both orchestrators and they
/// past the first live line, so `cobalt`, holding a task, stays.
fn in_review() -> Made {
    let f = Fixture::new(Seed::default());
    let (amber, cobalt) = (f.session("orchestrator"), f.session("orchestrator"));
    let (worker, reviewer) = (f.session("worker"), f.session("reviewer"));
    f.task(SLUG);
    f.task("tools/cobalts");
    f.store.take_role(SLUG, &amber, "orchestrator").unwrap();
    f.store
        .take_role("tools/cobalts", &cobalt, "orchestrator")
        .unwrap();
    f.store.claim(SLUG, &worker).unwrap();
    f.store.move_to(SLUG, "review", &worker).unwrap();
    f.store.take_role(SLUG, &reviewer, "reviewer").unwrap();
    let made = Made {
        f,
        amber,
        cobalt,
        worker,
        reviewer,
    };
    made.settle();
    made
}

/// [`in_review`], then approved: done, and not landed.
fn approved() -> Made {
    let made = in_review();
    (made.f.store)
        .verdict(SLUG, &made.reviewer, "approve", "3e0c661: good")
        .unwrap();
    made
}

impl Made {
    /// Everyone past the first live line again, each beating now.
    fn settle(&self) {
        self.f.advance(LIVE_SECONDS + 1.0);
        for handle in [&self.amber, &self.cobalt, &self.worker, &self.reviewer] {
            self.f.store.touch(handle).unwrap();
        }
    }

    /// When [`SLUG`] was approved, while no landing has closed it.
    fn unlanded(&self) -> Option<f64> {
        orchestration::unlanded(&self.f.store.get(SLUG).unwrap())
    }

    fn orchestrators(&self) -> Vec<String> {
        let task = self.f.store.get(SLUG).unwrap();
        let held = task.holders("orchestrator");
        held.into_iter().map(str::to_owned).collect()
    }

    /// The digest as `handle` hears it.
    fn heard(&self, handle: &str) -> Digest {
        let whole = digest(&self.f.store, &[], None, self.f.now(), &[]).unwrap();
        whole.only_for(&self.f.store.board().unwrap(), handle)
    }

    /// The slugs `handle`'s digest lists approved and not landed.
    fn listed(&self, handle: &str) -> Vec<String> {
        let heard = self.heard(handle);
        (heard.items(UNLANDED).iter())
            .map(|t| t.slug.clone())
            .collect()
    }

    /// Whether a reader of the whole board, as the server's clock reads it, gave anything.
    fn tick(&self) -> bool {
        (self.f.store).give_if_due(&self.f.store.clock_board().unwrap())
    }

    fn land(&self, by: &str) {
        (self.f.store)
            .landed(SLUG, by, LANDED, false, &["done"])
            .unwrap();
    }
}

#[test]
fn an_approve_marks_the_task_until_its_landing_closes_it() {
    let made = in_review();
    assert_eq!(made.unlanded(), None, "in review, nothing is approved");
    made.f.advance(5.0);
    let at = made.f.now();
    let task = (made.f.store)
        .verdict(SLUG, &made.reviewer, "approve", "3e0c661: good")
        .unwrap();
    assert_eq!(task.status, "done");
    assert_eq!(
        orchestration::unlanded(&task),
        Some(at),
        "the write's own answer carries it"
    );
    assert_eq!(made.unlanded(), Some(at));
    assert!(orchestration::owed(&task), "done, and still somebody's");
    made.f.advance(5.0);
    made.land(&made.amber);
    let task = made.f.store.get(SLUG).unwrap();
    assert_eq!(
        (task.status.as_str(), orchestration::unlanded(&task)),
        ("done", None)
    );
    assert!(!orchestration::owed(&task));
    let board = task.fields.get("_board");
    assert!(
        board.is_none_or(|board| !board.contains_key(orchestration::UNLANDED)),
        "{board:?}"
    );
}

#[test]
fn a_changes_verdict_marks_nothing_and_a_move_back_to_review_lifts_the_mark() {
    let made = in_review();
    (made.f.store)
        .verdict(SLUG, &made.reviewer, "changes", "no")
        .unwrap();
    assert_eq!(made.unlanded(), None);
    made.f.store.move_to(SLUG, "review", &made.worker).unwrap();
    (made.f.store)
        .verdict(SLUG, &made.reviewer, "approve", "3e0c661")
        .unwrap();
    assert!(made.unlanded().is_some());
    // Its executor sends it back for a fresh approve of a new tip: open work again.
    made.f.store.move_to(SLUG, "review", &made.worker).unwrap();
    let task = made.f.store.get(SLUG).unwrap();
    assert_eq!(
        (task.status.as_str(), orchestration::unlanded(&task)),
        ("review", None)
    );
    assert!(made.listed(&made.amber).is_empty());
    made.f.advance(7.0);
    (made.f.store)
        .verdict(SLUG, &made.reviewer, "approve", "4b5e7b0")
        .unwrap();
    assert_eq!(
        made.unlanded(),
        Some(made.f.now()),
        "the approve that stands dates it"
    );
}

#[test]
fn its_orchestrator_leaving_gives_it_on_and_it_is_listed_once_until_it_lands() {
    let made = approved();
    let approved_at = made.unlanded().unwrap();
    // Just approved, its orchestrator heard the verdict and is landing it: nothing is listed.
    made.f.advance(UNLANDED_SECONDS - 1.0);
    assert!(made.listed(&made.amber).is_empty());
    made.f.advance(1.0);
    made.settle();
    // Left unlanded, it is listed, and while amber is there it is amber's alone to hear of.
    assert_eq!(made.listed(&made.amber), [SLUG]);
    assert!(made.listed(&made.cobalt).is_empty());
    made.f.store.leave(&made.amber).unwrap();
    assert_eq!(made.orchestrators(), [made.cobalt.as_str()]);
    assert_eq!(
        made.f.said(SLUG).last().unwrap(),
        &format!(
            "{} was given orchestrator from {} (left)",
            made.cobalt, made.amber
        )
    );
    assert!(
        !made.tick(),
        "the leave gave it: nothing is left for the clock"
    );
    // Listed for the one given it, counted in the header, with when it was approved.
    let heard = made.heard(&made.cobalt);
    assert_eq!(made.listed(&made.cobalt), [SLUG]);
    let lines = heard.lines();
    assert!(lines[0].contains("approved_unlanded=1"), "{lines:?}");
    let rows: Vec<&String> = lines.iter().filter(|line| line.contains(SLUG)).collect();
    assert_eq!(rows.len(), 1, "{lines:?}");
    assert!(
        rows[0].starts_with("  approved_unlanded p2 tools/approved — "),
        "{rows:?}"
    );
    assert!(
        rows[0].ends_with(", not landed)") && rows[0].contains("(approved "),
        "{rows:?}"
    );
    // Said once: a recipient told the entry hears no row of it again, and the count has not moved.
    let entry = (UNLANDED.to_string(), SLUG.to_string());
    assert!(heard.entries().contains(&entry));
    let again = made
        .heard(&made.cobalt)
        .changes_since(&heard.said(), &heard.entries());
    assert_eq!(again, ["PULL counts unchanged"]);
    // Landed, it leaves the list, said as resolved, and the mark is gone.
    made.land(&made.cobalt);
    assert_eq!(
        made.unlanded(),
        None,
        "approved at {approved_at}, landed now"
    );
    let after = made.heard(&made.cobalt);
    assert!(after.items(UNLANDED).is_empty());
    assert!(!after.lines()[0].contains(UNLANDED), "{:?}", after.lines());
    let changed = after.changes_since(&heard.said(), &heard.entries());
    assert_eq!(
        changed,
        [
            "PULL approved_unlanded=0 (was 1)",
            "  resolved approved_unlanded tools/approved"
        ]
    );
    // Landed, it is given to nobody when its orchestrator goes in turn.
    let said = made.f.said(SLUG).len();
    made.f.store.leave(&made.cobalt).unwrap();
    assert!(!made.tick());
    assert_eq!(made.f.said(SLUG).len(), said + 1, "{:?}", made.f.said(SLUG));
    assert!(made.orchestrators().is_empty());
}

#[test]
fn a_lost_orchestrators_is_given_at_the_next_read_of_the_whole_board() {
    let made = approved();
    go_quiet_but(&made.f, &[&made.cobalt, &made.worker, &made.reviewer]);
    assert!(made.tick(), "amber is gone: the approved task is nobody's");
    assert_eq!(made.orchestrators(), [made.cobalt.as_str()]);
    let line = made.f.said(SLUG).pop().unwrap();
    let given = format!(
        "{} was given orchestrator from {} (",
        made.cobalt, made.amber
    );
    assert!(line.starts_with(&given), "{line}");
    assert!(!made.tick(), "given once");
    made.f.advance(UNLANDED_SECONDS);
    made.f.store.touch(&made.cobalt).unwrap();
    assert_eq!(made.listed(&made.cobalt), [SLUG]);
}

#[test]
fn with_no_orchestrator_that_stays_it_waits_listed_for_every_orchestrator_until_it_lands() {
    let made = approved();
    made.f.store.leave(&made.cobalt).unwrap();
    made.f.store.leave(&made.amber).unwrap();
    assert!(made.orchestrators().is_empty());
    let waiting = |made: &Made| orchestration::waiting(&made.f.store.get(SLUG).unwrap().fields);
    assert_eq!(
        waiting(&made),
        Some(made.f.now()),
        "marked waiting, as open work is"
    );
    // A newcomer that only registered is given nothing, and hears it as nobody's.
    made.f.advance(UNLANDED_SECONDS);
    let dune = made.f.session("orchestrator");
    assert_eq!(made.listed(&dune), [SLUG]);
    // Any orchestrator takes it, and hands it over, though it reads done; no other role is
    // taken on it. Landed, the waiting mark is lifted at the next read.
    let teal = made.f.session("orchestrator");
    made.f.store.take_role(SLUG, &teal, "orchestrator").unwrap();
    made.f.store.hand_over(SLUG, &teal, &dune).unwrap();
    assert_eq!(made.orchestrators(), [dune.as_str()]);
    assert_eq!(
        refused(made.f.store.take_role(SLUG, &teal, "reviewer")),
        "tools/approved is done"
    );
    made.land(&dune);
    assert_eq!(
        refused(made.f.store.take_role(SLUG, &teal, "orchestrator")),
        "tools/approved is done"
    );
    assert_eq!(
        refused(made.f.store.hand_over(SLUG, &dune, &teal)),
        "tools/approved is done"
    );
    made.f.store.leave(&dune).unwrap();
    made.tick();
    assert_eq!(waiting(&made), None);
    assert!(
        made.orchestrators().is_empty(),
        "landed: nobody's, and nobody's to give"
    );
}

#[test]
fn a_task_done_by_a_landing_a_covered_close_or_a_subtasks_done_is_never_marked_or_given() {
    let made = in_review();
    let f = &made.f;
    // Landed from review, with no approve: done by its landing.
    f.store
        .landed(SLUG, &made.amber, LANDED, false, &["review"])
        .unwrap();
    // Closed as covered by that landing.
    f.task("tools/covered");
    f.store
        .take_role("tools/covered", &made.amber, "orchestrator")
        .unwrap();
    f.store.claim("tools/covered", &made.worker).unwrap();
    (f.store)
        .close_covered(
            "tools/covered",
            &made.worker,
            SLUG,
            "3e0c66192471",
            "its fix is there",
        )
        .unwrap();
    // A slice closed by its executor, under a parent amber holds.
    f.task("tools/parent");
    f.store
        .take_role("tools/parent", &made.amber, "orchestrator")
        .unwrap();
    f.task_with(NewTask {
        parent: Some("tools/parent".into()),
        ..NewTask::new("tools/parent/slice", "Slice", "owner")
    });
    f.store
        .take_role("tools/parent/slice", &made.amber, "orchestrator")
        .unwrap();
    f.store.claim("tools/parent/slice", &made.worker).unwrap();
    f.store
        .done("tools/parent/slice", &made.worker, "integrated at 3e0c661")
        .unwrap();
    let closed = [SLUG, "tools/covered", "tools/parent/slice"];
    for slug in closed {
        let task = f.store.get(slug).unwrap();
        assert_eq!(task.status, "done", "{slug}");
        assert_eq!(orchestration::unlanded(&task), None, "{slug}");
        assert!(!orchestration::owed(&task), "{slug}");
    }
    assert!(made.listed(&made.amber).is_empty());
    made.settle();
    let said: Vec<usize> = closed.iter().map(|slug| f.said(slug).len()).collect();
    f.store.leave(&made.amber).unwrap();
    assert!(!made.tick());
    for (slug, said) in closed.iter().zip(said) {
        let task = f.store.get(slug).unwrap();
        assert!(
            task.holders("orchestrator").is_empty(),
            "{slug}: {:?}",
            f.said(slug)
        );
        assert_eq!(
            f.said(slug).len(),
            said + 1,
            "{slug}: only the leaver's release"
        );
        assert_eq!(orchestration::waiting(&task.fields), None, "{slug}");
    }
    // The open parent went on, as open work does.
    let parent = f.store.get("tools/parent").unwrap();
    assert_eq!(parent.holders("orchestrator"), [made.cobalt.as_str()]);
    assert!(made.listed(&made.cobalt).is_empty());
}
