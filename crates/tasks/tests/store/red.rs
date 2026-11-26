//! A red's mark on the real store: only a red's filer, a session of kind `filer`, sets it, and only
//! on a task of a red series, where it files a red or afterwards on a red it filed before it marked
//! reds ([`symphony_board::red`]).

use super::common::{Fixture, Seed, refused};
use serde_json::json;
use symphony_board::NewTask;
use symphony_board::red::{FIELDS, FILER, MARK};

/// What any session but a filer is told when it marks a task red.
fn unfiled(actor: &str) -> String {
    format!(
        "only a red's filer marks a task red, a session of kind filer as the tools that file reds register; '{actor}' is not one"
    )
}

/// What a mark on `slug`, of no red series, is told, whoever marks it.
fn unseries(slug: &str) -> String {
    format!(
        "{slug} is no red series' own task: a red is marked only on a task of the series reds are filed in, tests/main-red-*, tests/release-red-* and tests/nightly-red-*, never on one named under it"
    )
}

/// The red the tests mark, filed before its filer marked reds.
const RED: &str = "tests/nightly-red-sim";

/// The lines on `slug`'s thread saying `filer` marked it.
fn marks(f: &Fixture, slug: &str, filer: &str) -> usize {
    let line = format!("{filer} marked it red (nightly) and most urgent, as a red is filed");
    f.said(slug).iter().filter(|said| **said == line).count()
}

#[test]
fn only_a_reds_filer_marks_a_task_of_a_red_series_where_it_is_filed() {
    let f = Fixture::new(Seed::default());
    let (filer, worker) = (f.session(FILER), f.session("worker"));
    let orchestrator = f.session("orchestrator");
    let filing = |slug: &str, by: &str| NewTask {
        red: Some("main".into()),
        ..NewTask::new(slug, "A red", by)
    };
    for by in [
        worker.as_str(),
        orchestrator.as_str(),
        "owner",
        filer.as_str(),
    ] {
        let escape = filing("work/feature-escape", by);
        assert_eq!(
            refused(f.store.create(&escape)),
            unseries("work/feature-escape"),
            "{by}"
        );
    }
    for by in [worker.as_str(), orchestrator.as_str(), "owner"] {
        let red = filing("tests/main-red-3e0c", by);
        assert_eq!(refused(f.store.create(&red)), unfiled(by), "{by}");
    }
    for slug in ["work/feature-escape", "tests/main-red-3e0c"] {
        assert!(
            f.store.get(slug).is_err(),
            "a refused create files nothing: {slug}"
        );
    }
    let filed = (f.store.create(&filing("tests/main-red-3e0c", &filer))).unwrap();
    assert!(filed.red(), "{filed:?}");
}

#[test]
fn a_filer_marks_no_feature_red_after_it_is_filed() {
    let f = Fixture::new(Seed::default());
    let filer = f.session(FILER);
    let before = f.task("work/feature");
    assert_eq!(
        refused(f.store.mark_red("work/feature", "main", &filer)),
        unseries("work/feature")
    );
    let feature = f.store.get("work/feature").unwrap();
    assert_eq!(
        (feature.red(), feature.priority),
        (false, before.priority),
        "{feature:?}"
    );
    let said = f.said("work/feature");
    assert!(
        said.iter().all(|line| !line.contains("marked it red")),
        "{said:?}"
    );
}

#[test]
fn no_session_marks_a_task_red_through_the_boards_own_fields() {
    let f = Fixture::new(Seed::default());
    let filer = f.session(FILER);
    f.task("work/feature");
    for by in [filer.as_str(), "owner"] {
        assert_eq!(
            refused(
                f.store
                    .set_field("work/feature", FIELDS, MARK, &json!("main"), by)
            ),
            "fields._board are the board's own: a red is marked by its filer (mark_red), a fast fix where an older board filed it, a landing's record by record_waits, a task's needs by its assessor (tasks needs), its gate decisions by its orchestrator (tasks gate), its kind where it is filed or by tasks kind",
            "{by}"
        );
    }
    assert!(!f.store.get("work/feature").unwrap().red());
}

#[test]
fn a_red_filed_before_the_mark_is_marked_most_urgent_by_its_filer_once() {
    let f = Fixture::new(Seed::default());
    let (filer, worker) = (f.session(FILER), f.session("worker"));
    f.task_with(NewTask {
        priority: 1,
        ..NewTask::new(RED, "Nightly red: sim", &filer)
    });
    assert_eq!(
        refused(f.store.mark_red(RED, "nightly", &worker)),
        unfiled(&worker)
    );
    assert_eq!(
        refused(f.store.mark_red(RED, " ", &filer)),
        "a red task names what is red, such as main"
    );
    let marked = (f.store.mark_red(RED, "nightly", &filer)).unwrap();
    assert_eq!((marked.red(), marked.priority), (true, 0), "{marked:?}");
    let said = f.said(RED);
    assert_eq!(marks(&f, RED, &filer), 1, "{said:?}");
    assert!(
        said.last()
            .is_some_and(|last| last.contains("marked it red")),
        "{said:?}"
    );
    // Marked and most urgent already: left as it is, its thread unchanged.
    (f.store.mark_red(RED, "nightly", &filer)).unwrap();
    assert_eq!(f.said(RED), said);
}

#[test]
fn two_marks_of_one_red_at_once_post_one_marking_line() {
    let f = Fixture::new(Seed::default());
    let filer = f.session(FILER);
    f.task_with(NewTask {
        priority: 1,
        ..NewTask::new(RED, "Nightly red: sim", &filer)
    });
    let (other, by) = (f.other(), filer.clone());
    // The second mark lands after the first has begun, before it writes.
    let late = f.late(move || {
        other.mark_red(RED, "nightly", &by).unwrap();
    });
    let marked = late.mark_red(RED, "nightly", &filer).unwrap();
    assert_eq!((marked.red(), marked.priority), (true, 0), "{marked:?}");
    assert_eq!(marks(&f, RED, &filer), 1, "{:?}", f.said(RED));
}

#[test]
fn a_closed_or_missing_task_is_no_red_to_mark() {
    let f = Fixture::new(Seed::default());
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
    assert_eq!(
        refused(
            f.store
                .mark_red("tests/nightly-red-gone", "nightly", &filer)
        ),
        "no task 'tests/nightly-red-gone'"
    );
}

#[test]
fn a_task_named_under_a_closed_red_is_ordinary_work_no_filer_marks() {
    let f = Fixture::new(Seed::default());
    let filer = f.session(FILER);
    let old = "tests/main-red-old";
    let filed = NewTask {
        red: Some("main".into()),
        ..NewTask::new(old, "Main red", &filer)
    };
    f.store.create(&filed).unwrap();
    f.store.move_to(old, "cancelled", "owner").unwrap();
    let child = format!("{old}/feature");
    f.task(&child);
    assert_eq!(
        refused(f.store.mark_red(&child, "main", &filer)),
        unseries(&child)
    );
    let unmarked = f.store.get(&child).unwrap();
    assert!(!unmarked.red(), "{unmarked:?}");
    let fix = format!("{old}/fix");
    let under = NewTask {
        red: Some("main".into()),
        ..NewTask::new(&fix, "A fix", &filer)
    };
    assert_eq!(refused(f.store.create(&under)), unseries(&fix));
    let unfiled = f.store.get(&fix);
    assert!(
        unfiled.is_err(),
        "a refused create files nothing: {unfiled:?}"
    );
}
