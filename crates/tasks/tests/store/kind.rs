//! A task's kind on the real store ([`symphony_board::kind`]): written with the task where it is
//! filed, `red` for a red's filing that names none, given or changed on an open task by
//! `set_kind` with a line naming both, and out of a plugin field's reach.

use super::common::{Backend, Fixture, Seed, refused};
use serde_json::json;
use symphony_board::NewTask;
use symphony_board::red::{FIELDS, FILER};

/// The refusal of `kind`, which is none of the list.
fn unknown(kind: &str) -> String {
    format!("a task's kind is one of feature, fix, red, tests or docs, not '{kind}'")
}

on_both! {
    fn a_task_filed_with_a_kind_keeps_it_and_one_filed_without_has_none(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let worker = f.session("worker");
        let filed = f.task_with(NewTask {
            kind: Some("tests".into()),
            ..NewTask::new("tests/flaky-wait", "A flaky wait", &worker)
        });
        assert_eq!(filed.kind(), Some("tests"));
        assert_eq!(f.store.get("tests/flaky-wait").unwrap().kind(), Some("tests"));
        assert_eq!(filed.fields[FIELDS]["kind"], json!("tests"));
        // The owner's filing too, and one that names none has none.
        let owners = f.task_with(NewTask {
            kind: Some("docs".into()),
            ..NewTask::new("docs/guide", "A guide", "owner")
        });
        assert_eq!(owners.kind(), Some("docs"));
        assert_eq!(f.task("app/plain").kind(), None);
        assert!(!f.store.get("app/plain").unwrap().fields.contains_key(FIELDS));
    }

    fn an_unknown_kind_is_refused_naming_the_list_and_files_nothing(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let before = f.rows();
        for kind in ["bug", "Feature", "", "tests "] {
            let filing = NewTask {
                kind: Some(kind.into()),
                ..NewTask::new("app/odd", "Odd", "owner")
            };
            assert_eq!(refused(f.store.create(&filing)), unknown(kind), "{kind:?}");
            assert_eq!(f.rows(), before, "{kind:?}");
        }
    }

    fn a_reds_filing_is_red_unless_it_names_a_kind_of_its_own(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let filer = f.session(FILER);
        let red = |slug: &str, kind: Option<&str>| NewTask {
            red: Some("release".into()),
            kind: kind.map(str::to_string),
            ..NewTask::new(slug, "A red", &filer)
        };
        assert_eq!(f.task_with(red("tests/release-red-lint", None)).kind(), Some("red"));
        assert_eq!(
            f.task_with(red("tests/release-red-docs", Some("fix"))).kind(),
            Some("fix")
        );
        // A red filed by name alone, with no mark, is given none.
        assert_eq!(f.task("tests/release-red-unmarked").kind(), None);
    }

    fn an_open_task_is_given_a_kind_and_changed_with_a_line_naming_both(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let orchestrator = f.session("orchestrator");
        f.task("inbox/owner-ask");
        let version = |f: &Fixture| {
            let row = f.store.raw().row("task", "inbox/owner-ask");
            row.field("version").unwrap().as_i64().unwrap()
        };
        let filed = version(&f);
        let lines = f.said("inbox/owner-ask").len();
        f.advance(1.0);
        let given = f
            .store
            .set_kind("inbox/owner-ask", "feature", &orchestrator)
            .unwrap();
        assert_eq!(given.kind(), Some("feature"));
        assert_eq!(version(&f), filed + 1, "a write of the task moves its version");
        let expected = given;
        let said = f.said("inbox/owner-ask");
        assert_eq!(said.len(), lines + 1);
        assert_eq!(
            said.last().unwrap(),
            &format!("{orchestrator} gave it the kind feature")
        );
        // The same kind again writes nothing and says nothing.
        let same = f.store.set_kind("inbox/owner-ask", "feature", &orchestrator).unwrap();
        assert_eq!(same, expected);
        assert_eq!(f.said("inbox/owner-ask").len(), lines + 1);
        assert_eq!(version(&f), filed + 1);
        // Another replaces it, said with the one it replaced; the owner may change it too.
        let fixed = f.store.set_kind("inbox/owner-ask", "fix", "owner").unwrap();
        assert_eq!((fixed.kind(), version(&f)), (Some("fix"), filed + 2));
        assert_eq!(
            f.said("inbox/owner-ask").last().unwrap(),
            "owner changed its kind feature → fix"
        );
        let thread = f.store.thread("inbox/owner-ask").unwrap();
        let line = thread.last().unwrap();
        assert_eq!((line.sender.as_str(), line.kind.as_str()), ("owner", "system"));
    }

    fn a_closed_missing_or_unknown_kind_is_refused_and_changes_nothing(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let worker = f.session("worker");
        f.task_with(NewTask {
            kind: Some("docs".into()),
            ..NewTask::new("docs/old", "Old", "owner")
        });
        f.store.move_to("docs/old", "cancelled", "owner").unwrap();
        f.task("app/open");
        let before = f.rows();
        assert_eq!(
            refused(f.store.set_kind("docs/old", "fix", &worker)),
            "docs/old is cancelled: only an open task is given a kind"
        );
        assert_eq!(
            refused(f.store.set_kind("app/gone", "fix", &worker)),
            "no task 'app/gone'"
        );
        assert_eq!(
            refused(f.store.set_kind("app/open", "chore", &worker)),
            unknown("chore")
        );
        let ghost = f.no_session("ghost-owl");
        assert_eq!(refused(f.store.set_kind("app/open", "fix", "ghost-owl")), ghost);
        assert_eq!(f.rows(), before);
        assert_eq!(f.store.get("docs/old").unwrap().kind(), Some("docs"));
    }

    fn no_plugin_field_write_reaches_the_kind(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let worker = f.session("worker");
        f.task("app/open");
        let why = refused(f.store.set_field("app/open", FIELDS, "kind", &json!("fix"), &worker));
        assert!(why.ends_with("its kind where it is filed or by tasks kind"), "{why}");
        assert_eq!(f.store.get("app/open").unwrap().kind(), None);
    }
}
