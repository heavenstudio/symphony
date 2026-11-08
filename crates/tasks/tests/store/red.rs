//! A red's mark on the real store: only a red's filer, a session of kind `filer`, sets it, where it
//! files a red or afterwards on a red it filed before it marked reds ([`symphony_board::red`]).

use super::common::{Fixture, refused};
use serde_json::json;
use symphony_board::NewTask;
use symphony_board::red::{FIELDS, FILER, MARK};

/// What any session but a filer is told when it marks a task red.
fn unfiled(actor: &str) -> String {
    format!(
        "only a red's filer marks a task red, a session of kind filer as the tools that file reds register; '{actor}' is not one"
    )
}

#[test]
fn only_a_reds_filer_marks_a_task_red_where_it_is_filed() {
    let f = Fixture::empty();
    let (filer, worker) = (f.session(FILER), f.session("worker"));
    let orchestrator = f.session("orchestrator");
    let escape = |by: &str| NewTask {
        red: Some("main".into()),
        ..NewTask::new("work/feature-escape", "Escape the cap", by)
    };
    for by in [worker.as_str(), orchestrator.as_str(), "owner"] {
        assert_eq!(refused(f.store.create(&escape(by))), unfiled(by), "{by}");
        assert!(
            f.store.get("work/feature-escape").is_err(),
            "{by}'s refused create files nothing"
        );
    }
    let filed = f.store.create(&escape(&filer)).unwrap();
    assert!(filed.red(), "{filed:?}");
}

#[test]
fn no_session_marks_a_task_red_through_the_boards_own_fields() {
    let f = Fixture::empty();
    let filer = f.session(FILER);
    f.task("work/feature");
    for by in [filer.as_str(), "owner"] {
        assert_eq!(
            refused(
                f.store
                    .set_field("work/feature", FIELDS, MARK, &json!("main"), by)
            ),
            "fields._board are the board's own: a red is marked by its filer (mark_red)",
            "{by}"
        );
    }
    assert!(!f.store.get("work/feature").unwrap().red());
}

#[test]
fn a_red_filed_before_the_mark_is_marked_most_urgent_by_its_filer_once() {
    let f = Fixture::empty();
    let (filer, worker) = (f.session(FILER), f.session("worker"));
    f.task_with(NewTask {
        priority: 1,
        ..NewTask::new("tests/nightly-red-sim", "Nightly red: sim", &filer)
    });
    assert_eq!(
        refused(
            f.store
                .mark_red("tests/nightly-red-sim", "nightly", &worker)
        ),
        unfiled(&worker)
    );
    assert_eq!(
        refused(f.store.mark_red("tests/nightly-red-sim", " ", &filer)),
        "a red task names what is red, such as main"
    );
    let marked = (f.store.mark_red("tests/nightly-red-sim", "nightly", &filer)).unwrap();
    assert_eq!((marked.red(), marked.priority), (true, 0), "{marked:?}");
    let said = f.said("tests/nightly-red-sim");
    assert_eq!(
        said.last().map(String::as_str),
        Some(
            format!("{filer} marked it red (nightly) and most urgent, as a red is filed").as_str()
        )
    );
    // Marked and most urgent already: left as it is, its thread unchanged.
    (f.store.mark_red("tests/nightly-red-sim", "nightly", &filer)).unwrap();
    assert_eq!(f.said("tests/nightly-red-sim"), said);
}

#[test]
fn a_closed_task_is_no_red_to_mark() {
    let f = Fixture::empty();
    let filer = f.session(FILER);
    f.task("tests/nightly-red-old");
    f.store
        .move_to("tests/nightly-red-old", "cancelled", "owner")
        .unwrap();
    assert_eq!(
        refused(f.store.mark_red("tests/nightly-red-old", "nightly", &filer)),
        "tests/nightly-red-old is cancelled: only an open task is marked red"
    );
    assert!(!f.store.get("tests/nightly-red-old").unwrap().red());
}
