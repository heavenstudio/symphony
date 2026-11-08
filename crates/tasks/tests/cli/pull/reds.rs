//! PULL digest: a red task ([`symphony_board::red`]) nobody executes 10 minutes after it was filed
//! is named first, and nowhere else.

use super::{Case, slugs};
use symphony_board::NewTask;
use symphony_board::health::RESERVE_SECONDS;
use symphony_board::red::UNSTAFFED_SECONDS;

impl Case {
    /// `slug`, filed for a red of main, most urgent, as land files one.
    fn red(&self, slug: &str) {
        self.task_with(
            NewTask {
                priority: 0,
                red: Some("main".into()),
                ..NewTask::new(slug, &format!("Red {slug}"), "owner")
            },
            "ready",
        );
    }
}

#[test]
fn a_red_nobody_executes_ten_minutes_after_filing_is_named_first_and_only_there() {
    let c = Case::new();
    c.task_with(c.priority("sim/urgent", 0), "ready");
    c.red("tests/main-red-abc");
    let d = c.digest();
    assert_eq!(
        (slugs(&d, "red_unstaffed"), slugs(&d, "ready_unstaffed")),
        (
            vec![],
            vec!["sim/urgent".into(), "tests/main-red-abc".into()]
        ),
        "just filed, it is ready work like any"
    );
    assert!(!d.lines()[0].contains("red_unstaffed"), "{}", d.lines()[0]);
    c.f.advance(UNSTAFFED_SECONDS - 1.0);
    assert_eq!(c.digest().count("red_unstaffed"), 0, "not yet");
    c.f.advance(1.0);
    let d = c.digest();
    assert_eq!(slugs(&d, "red_unstaffed"), ["tests/main-red-abc"]);
    assert_eq!(slugs(&d, "ready_unstaffed"), ["sim/urgent"], "named once");
    assert!(
        d.entries()
            .contains(&("red_unstaffed".into(), "tests/main-red-abc".into()))
    );
    let lines = d.lines();
    assert!(
        lines[0].contains(" executing=0 red_unstaffed=1 ready_unstaffed=1 "),
        "{}",
        lines[0]
    );
    assert_eq!(
        lines[1..],
        [
            "  red_unstaffed p0 tests/main-red-abc — Red tests/main-red-abc",
            "  ready_unstaffed p0 sim/urgent — T sim/urgent",
        ],
        "first, before ready work of the same priority"
    );
    // An executor on it, or on work under it, staffs it.
    let who = c.worker();
    c.f.store.claim("tests/main-red-abc", &who).unwrap();
    let d = c.digest();
    assert_eq!((d.count("red_unstaffed"), d.count("executing")), (0, 1));
}

#[test]
fn a_red_whose_executor_is_gone_is_named_first_not_orphaned_and_one_in_review_is_not_named() {
    let c = Case::new();
    c.red("tests/main-red-abc");
    c.red("tests/nightly-red-sim");
    let (first, second) = (c.worker(), c.worker());
    c.f.store.claim("tests/main-red-abc", &first).unwrap();
    c.f.store.claim("tests/nightly-red-sim", &second).unwrap();
    (c.f.store
        .move_to("tests/nightly-red-sim", "review", &second))
    .unwrap();
    c.go_quiet();
    let d = c.digest();
    assert_eq!(slugs(&d, "red_unstaffed"), ["tests/main-red-abc"]);
    assert_eq!(d.count("in_progress_orphaned"), 0);
    assert_eq!(slugs(&d, "review_unreviewed"), ["tests/nightly-red-sim"]);
}

#[test]
fn a_red_staffed_through_a_subtask_or_reserved_is_named_with_its_reserver_until_claimed() {
    let c = Case::new();
    c.red("tests/main-red-abc");
    c.task_with(
        c.sub("tests/main-red-abc/fix", "tests/main-red-abc"),
        "ready",
    );
    let who = c.worker();
    c.f.store
        .reserve("tests/main-red-abc", &who, RESERVE_SECONDS)
        .unwrap();
    c.f.advance(UNSTAFFED_SECONDS);
    c.f.store.touch(&who).unwrap();
    assert!(c.digest().lines().contains(&format!(
        "  red_unstaffed p0 tests/main-red-abc — Red tests/main-red-abc   (reserved {who})"
    )));
    c.f.store.claim("tests/main-red-abc/fix", &who).unwrap();
    assert_eq!(c.digest().count("red_unstaffed"), 0);
}

#[test]
fn a_task_only_named_like_a_red_is_ready_work() {
    let c = Case::new();
    c.task_with(c.priority("tests/main-red-old", 0), "ready");
    c.f.advance(UNSTAFFED_SECONDS * 6.0);
    let d = c.digest();
    assert_eq!(
        (slugs(&d, "red_unstaffed"), slugs(&d, "ready_unstaffed")),
        (vec![], vec!["tests/main-red-old".into()])
    );
}
