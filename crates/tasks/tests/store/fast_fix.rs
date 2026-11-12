//! A fast fix on the real store: filed with the risk that calls for no reviewer, and approved by its
//! orchestrator, which did not execute it ([`symphony_board::fast_fix`]); every other task by a
//! fresh reviewer.

use super::common::{Fixture, refused};
use serde_json::json;
use symphony_board::NewTask;

const WHY: &str = "a one-line regex fix, its regression test posted";

/// `slug` filed (a fast fix when `why` says why), orchestrated by `orch`, executed by `exe` and in
/// review.
fn in_review(f: &Fixture, slug: &str, why: Option<&str>, orch: &str, exe: &str) {
    f.task_with(NewTask {
        fast_fix: why.map(str::to_string),
        ..NewTask::new(slug, "A fix", orch)
    });
    f.store.take_role(slug, orch, "orchestrator").unwrap();
    f.store.claim(slug, exe).unwrap();
    f.store.move_to(slug, "review", exe).unwrap();
}

#[test]
fn a_fast_fix_is_filed_with_its_risk_and_never_marked_otherwise() {
    let f = Fixture::empty();
    let orch = f.session("orchestrator");
    let fast = f.task_with(NewTask {
        fast_fix: Some(format!("  {WHY} ")),
        ..NewTask::new("fix/regex", "A fix", &orch)
    });
    assert_eq!(fast.fast_fix(), Some(WHY));
    assert_eq!(f.store.get("fix/regex").unwrap().fast_fix(), Some(WHY));
    assert_eq!(f.task("fix/other").fast_fix(), None);
    for blank in ["", "  "] {
        assert_eq!(
            refused(f.store.create(&NewTask {
                fast_fix: Some(blank.into()),
                ..NewTask::new("fix/blank", "A fix", &orch)
            })),
            "a fast fix records why it needs no reviewer: its risk, in words"
        );
    }
    assert!(
        f.store.get("fix/blank").is_err(),
        "a refused filing files nothing"
    );
    // The mark is the board's own: no session sets it, or takes it off, afterwards.
    for (slug, value) in [("fix/other", json!(WHY)), ("fix/regex", json!(null))] {
        assert!(
            refused(f.store.set_field(slug, "_board", "fast_fix", &value, &orch))
                .starts_with("fields._board are the board's own"),
            "{slug}"
        );
    }
    assert_eq!(f.store.get("fix/other").unwrap().fast_fix(), None);
    assert_eq!(f.store.get("fix/regex").unwrap().fast_fix(), Some(WHY));
}

#[test]
fn its_orchestrator_gives_a_fast_fix_its_verdict_and_no_other_tasks() {
    let f = Fixture::empty();
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    in_review(&f, "fix/regex", Some(WHY), &orch, &exe);
    in_review(&f, "fix/plain", None, &orch, &exe);
    assert_eq!(
        refused(f.store.verdict("fix/plain", &orch, "approve", "")),
        format!(
            "{orch} is not a reviewer of fix/plain: it orchestrated fix/plain, and the verdict is \
             a fresh reviewer's, one that did not"
        )
    );
    let stranger = f.session("orchestrator");
    assert_eq!(
        refused(f.store.verdict("fix/regex", &stranger, "approve", "")),
        format!("{stranger} is neither a reviewer of fix/regex nor its orchestrator")
    );
    let task = f
        .store
        .verdict(
            "fix/regex",
            &orch,
            "approve",
            "4b5e7b0a: its test is red before, green after",
        )
        .unwrap();
    assert_eq!(task.status, "done");
    let last = f.store.thread("fix/regex").unwrap().pop().unwrap();
    assert_eq!(
        (last.sender, last.kind, last.text),
        (
            orch,
            "verdict".to_string(),
            "approve: 4b5e7b0a: its test is red before, green after".to_string()
        )
    );
    assert_eq!(f.store.get("fix/plain").unwrap().status, "review");
}

#[test]
fn an_orchestrator_that_executed_a_fast_fix_gives_it_no_verdict() {
    let f = Fixture::empty();
    let both = f.session("orchestrator");
    in_review(&f, "fix/regex", Some(WHY), &both, &both);
    for verdict in ["approve", "changes"] {
        assert_eq!(
            refused(f.store.verdict("fix/regex", &both, verdict, "")),
            format!(
                "{both} executed fix/regex: its verdict is a fresh reviewer's, one that did not \
                 write or execute it"
            )
        );
    }
    assert_eq!(f.store.get("fix/regex").unwrap().status, "review");
}
