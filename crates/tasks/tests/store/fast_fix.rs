//! Legacy marks are inert: only a fresh reviewer gives a verdict, and completed history stays.

use super::common::{Fixture, Seed, refused};
use serde_json::json;
use symphony_board::NewTask;

#[test]
fn a_legacy_mark_gives_no_requester_a_reviewers_role_or_verdict() {
    let f = Fixture::new(Seed::default());
    let (filer, former) = (f.session("worker"), f.session("orchestrator"));
    let (orch, exe) = (f.session("orchestrator"), f.session("worker"));
    let slug = "fix/legacy";
    f.task_with(NewTask::new(slug, "A fix", &filer));
    f.store
        .raw()
        .row("task", slug)
        .set(json!({"fields._board.fast_fix": "one line"}))
        .unwrap();
    f.store.take_role(slug, &former, "orchestrator").unwrap();
    f.store.hand_over(slug, &former, &orch).unwrap();
    f.store.claim(slug, &exe).unwrap();
    f.store.move_to(slug, "review", &exe).unwrap();
    for (who, did) in [
        (&orch, "orchestrated"),
        (&former, "orchestrated"),
        (&filer, "filed"),
    ] {
        assert!(
            refused(f.store.take_role(slug, who, "reviewer"))
                .contains(&format!("{who} {did} {slug}"))
        );
        for verdict in ["approve", "changes"] {
            assert!(
                refused(f.store.verdict(slug, who, verdict, "mine"))
                    .contains(&format!("{who} {did} {slug}"))
            );
        }
    }
    assert!(
        refused(f.store.verdict(slug, &exe, "approve", "mine")).contains("executed fix/legacy")
    );
    assert_eq!(f.store.get(slug).unwrap().status, "review");
    let rev = f.session("reviewer");
    f.store.take_role(slug, &rev, "reviewer").unwrap();
    assert_eq!(
        f.store
            .verdict(slug, &rev, "approve", "4b5e7b0a: good")
            .unwrap()
            .status,
        "done"
    );
}

#[test]
fn completed_legacy_tasks_keep_their_marks_verdicts_and_status() {
    let f = Fixture::new(Seed::default());
    let orch = f.session("orchestrator");
    let slug = "fix/completed";
    f.task_with(NewTask::new(slug, "A completed fix", &orch));
    f.store
        .raw()
        .row("task", slug)
        .set(json!({
            "fields._board.fast_fix": "a red is fixed as a bug fix is: its old risk",
            "status": "done"
        }))
        .unwrap();
    f.store
        .landed(slug, &orch, "approve: 4b5e7b0a: good", true, &["done"])
        .unwrap();
    let before = f.store.get(slug).unwrap();
    let thread = f.store.thread(slug).unwrap();
    assert_eq!(before.status, "done");
    assert_eq!(
        before.fields["_board"]["fast_fix"],
        "a red is fixed as a bug fix is: its old risk"
    );
    assert!(
        thread
            .iter()
            .any(|m| m.kind == "verdict" && m.sender == orch)
    );
    f.store.board().unwrap();
    assert_eq!(f.store.get(slug).unwrap(), before);
    assert_eq!(f.store.thread(slug).unwrap(), thread);
}
