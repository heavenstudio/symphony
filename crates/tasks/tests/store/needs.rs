//! A task's needs on the real store ([`symphony_board::stamps`]): written by its assessor, a role
//! in name only, which the write refuses to a session that executed the task or work under it,
//! orchestrates or orchestrated it, or filed it; checked against the catalogue; each change a line
//! on the thread; and the board's own field, out of `set_field`'s reach.

use super::common::{Backend, Fixture, Seed, refused};
use serde_json::json;
use symphony_board::NewTask;
use symphony_board::stamps::{Catalogue, Needs};

const WHY: &str = "a store query and its test, no page or wire";
const TIP: &str = "1a2b3c4d5e6f7a8b9c0d1a2b3c4d5e6f7a8b9c0d";

/// The needs `by` names, read at `tip`, for `why`.
fn needs(by: &str, stamps: &[&str], tip: Option<&str>, why: &str) -> Needs {
    Needs {
        stamps: stamps.iter().map(|s| s.to_string()).collect(),
        tip: tip.map(str::to_string),
        by: by.into(),
        why: why.into(),
        import: None,
    }
}

/// `slug` filed by `filer`, orchestrated by `orch` and claimed by `exe`.
fn worked(f: &Fixture, slug: &str, filer: &str, orch: &str, exe: &str) {
    f.task_with(NewTask::new(slug, "A change", filer));
    f.store.take_role(slug, orch, "orchestrator").unwrap();
    f.store.claim(slug, exe).unwrap();
}

on_both! {
    fn a_fresh_session_writes_the_needs_and_the_thread_says_them_and_what_they_were(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let (filer, orch, exe) = (
            f.session("worker"),
            f.session("orchestrator"),
            f.session("worker"),
        );
        worked(&f, "tools/query", &filer, &orch, &exe);
        let own = Catalogue::symphony();
        let assessor = f.session("worker");
        assert_eq!(f.store.get("tools/query").unwrap().needs(), None);
        let task = f
            .store
            .set_needs(
                "tools/query",
                &own,
                &needs(
                    &assessor,
                    &["review", "tests"],
                    Some(TIP),
                    &format!("  {WHY} "),
                ),
            )
            .unwrap();
        let written = needs(&assessor, &["tests", "review"], Some(TIP), WHY);
        assert_eq!(
            task.needs(),
            Some(written.clone()),
            "in the catalogue's order, trimmed"
        );
        assert_eq!(f.store.get("tools/query").unwrap().needs(), Some(written));
        let last = f.store.thread("tools/query").unwrap().pop().unwrap();
        assert_eq!(
            (last.sender.as_str(), last.kind.as_str(), last.text.as_str()),
            (
                assessor.as_str(),
                "system",
                format!("needs tests, review at 1a2b3c4 (was review): {WHY}; by {assessor}").as_str()
            ),
            "with no assessment before, what it needed was the catalogue's defaults"
        );
        // Assessed again, from another session, waiving everything: none, with a reason.
        let second = f.session("reviewer");
        let task = f
            .store
            .set_needs(
                "tools/query",
                &own,
                &needs(&second, &["none"], None, "documents only after all"),
            )
            .unwrap();
        assert_eq!(
            task.needs(),
            Some(needs(&second, &[], None, "documents only after all"))
        );
        assert_eq!(
            f.said("tools/query").last().unwrap(),
            &format!(
                "needs none from the brief (was tests, review): documents only after all; by {second}"
            )
        );
        // Nothing else of the task moved.
        assert_eq!(task.status, "in_progress");
        assert!(task.holders("executor").contains(&exe.as_str()));
    }

    fn an_empty_reason_or_a_stamp_the_catalogue_lacks_is_refused_and_writes_nothing(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        f.task("tools/query");
        let assessor = f.session("worker");
        let own = Catalogue::symphony();
        let project = Catalogue::parse("[stamps.tests]\n[stamps.review]\ndefault = true\n").unwrap();
        let before = f.said("tools/query");
        for why in ["", "   "] {
            assert_eq!(
                refused(f.store.set_needs(
                    "tools/query",
                    &own,
                    &needs(&assessor, &["review"], None, why)
                )),
                "an assessment says why (--why WHY): one line for each stamp it left out or added"
            );
        }
        assert_eq!(
            refused(f.store.set_needs(
                "tools/query",
                &project,
                &needs(&assessor, &["tests", "pictures"], None, WHY)
            )),
            "the catalogue has no stamp \"pictures\": its stamps are tests, review (`just tasks \
             stamps` lists them)"
        );
        assert_eq!(
            refused(
                f.store
                    .set_needs("tools/query", &own, &needs(&assessor, &["lint"], None, WHY))
            ),
            "the catalogue has no stamp \"lint\": its stamps are tests, review, pictures, owner \
             (`just tasks stamps` lists them)"
        );
        assert_eq!(
            refused(f.store.set_needs(
                "tools/query",
                &own,
                &needs(&assessor, &["none", "review"], None, WHY)
            )),
            "none is no stamp: name it alone for a task that needs none, or name the stamps it needs"
        );
        // An import is named whole, and at the head read.
        let import = |tip: Option<&str>, import: &str| Needs {
            import: Some(import.into()),
            ..needs(&assessor, &["review"], tip, WHY)
        };
        let upstream = "9f8e7d6c5b4a39281706f5e4d3c2b1a098765432";
        assert_eq!(
            refused(f.store.set_needs("tools/query", &own, &import(None, upstream))),
            "an import is named at the head the assessment read (--tip TIP): it binds that head \
             to the upstream commit it imports"
        );
        for short in ["9f8e7d6", "9f8e7d6c5b4a39281706f5e4d3c2b1a09876543z"] {
            assert_eq!(
                refused(f.store.set_needs("tools/query", &own, &import(Some(TIP), short))),
                format!("an import names its upstream commit whole, not {short:?}")
            );
        }
        assert_eq!(f.store.get("tools/query").unwrap().needs(), None);
        assert_eq!(f.said("tools/query"), before, "no line either");
        // Named whole at a tip, it is kept and said, and a blank one is none.
        f.store
            .set_needs("tools/query", &own, &import(Some(TIP), &format!(" {upstream} ")))
            .unwrap();
        let written = f.store.get("tools/query").unwrap().needs().unwrap();
        assert_eq!(written.import.as_deref(), Some(upstream));
        assert_eq!(
            f.said("tools/query").last().unwrap(),
            &format!("needs review at 1a2b3c4, an import of {upstream} (was review): {WHY}; by {assessor}")
        );
        f.store
            .set_needs("tools/query", &own, &import(None, "  "))
            .unwrap();
        assert_eq!(f.store.get("tools/query").unwrap().needs().unwrap().import, None);
        // The project's own catalogue: what it needed before is its defaults.
        f.store
            .set_needs(
                "tools/query",
                &project,
                &needs(&assessor, &["tests"], Some(TIP), WHY),
            )
            .unwrap();
        assert_eq!(
            f.said("tools/query").last().unwrap(),
            &format!("needs tests at 1a2b3c4 (was review): {WHY}; by {assessor}")
        );
    }

    fn the_tasks_executor_orchestrators_and_filer_write_no_needs(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let (filer, former, orch) = (
            f.session("worker"),
            f.session("orchestrator"),
            f.session("orchestrator"),
        );
        let (exe, gone, slice) = (
            f.session("worker"),
            f.session("worker"),
            f.session("worker"),
        );
        f.task_with(NewTask::new("tools/query", "A change", &filer));
        f.store
            .take_role("tools/query", &former, "orchestrator")
            .unwrap();
        f.store.hand_over("tools/query", &former, &orch).unwrap();
        // One executor let it go, another holds it; a third executed a slice of its work.
        f.store.claim("tools/query", &gone).unwrap();
        f.store.release("tools/query", &gone, None).unwrap();
        f.store.claim("tools/query", &exe).unwrap();
        f.task_with(NewTask {
            parent: Some("tools/query".into()),
            ..NewTask::new("tools/query/slice", "A slice", &orch)
        });
        f.store.claim("tools/query/slice", &slice).unwrap();
        let own = Catalogue::symphony();
        let tail = "its needs are its assessor's, a session that never executed, orchestrated or \
                    filed it, which its orchestrator starts as a session of its own";
        for (who, did) in [
            (&exe, "executed"),
            (&gone, "executed"),
            (&slice, "executed"),
            (&orch, "orchestrated"),
            (&former, "orchestrated"),
            (&filer, "filed"),
        ] {
            assert_eq!(
                refused(f.store.set_needs(
                    "tools/query",
                    &own,
                    &needs(who, &["none"], Some(TIP), "it asks for a waiver")
                )),
                format!("{who} {did} tools/query: {tail}"),
                "{did}"
            );
        }
        assert_eq!(f.store.get("tools/query").unwrap().needs(), None);
        // An instruction subtask's executor executed none of its work: a reviewer that claimed
        // SLUG/review, say, may assess it.
        let review = f.session("reviewer");
        f.task_with(NewTask {
            parent: Some("tools/query".into()),
            instruction: true,
            ..NewTask::new("tools/query/review", "Review it", &orch)
        });
        f.store.claim("tools/query/review", &review).unwrap();
        f.store
            .set_needs(
                "tools/query",
                &own,
                &needs(&review, &["tests", "review"], Some(TIP), WHY),
            )
            .unwrap();
        // A session the board does not know is refused as any write's actor is.
        assert_eq!(
            refused(f.store.set_needs(
                "tools/query",
                &own,
                &needs("nobody", &["review"], None, WHY)
            )),
            f.no_session("nobody")
        );
    }

    fn a_task_approved_and_not_landed_is_assessed_and_one_landed_or_closed_is_not(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let (filer, orch, exe) = (
            f.session("worker"),
            f.session("orchestrator"),
            f.session("worker"),
        );
        worked(&f, "tools/query", &filer, &orch, &exe);
        let own = Catalogue::symphony();
        let (assessor, reviewer) = (f.session("worker"), f.session("reviewer"));
        let store = &f.store;
        // A slice closed by its executor is done with no land of its own to make.
        f.task_with(NewTask {
            parent: Some("tools/query".into()),
            ..NewTask::new("tools/query/slice", "A slice", &filer)
        });
        store.claim("tools/query/slice", &exe).unwrap();
        store.done("tools/query/slice", &exe, "integrated").unwrap();
        assert_eq!(
            refused(store.set_needs(
                "tools/query/slice",
                &own,
                &needs(&assessor, &["review"], None, WHY)
            )),
            "tools/query/slice is done: only an open task, or one approved and not landed, is \
             assessed"
        );
        // Approved: it reads done, and its land is still to make, so its assessor still writes,
        // the task left where the approve put it.
        store.move_to("tools/query", "review", &exe).unwrap();
        store.take_role("tools/query", &reviewer, "reviewer").unwrap();
        store
            .verdict("tools/query", &reviewer, "approve", "reads well")
            .unwrap();
        let task = store
            .set_needs(
                "tools/query",
                &own,
                &needs(&assessor, &["tests"], Some(TIP), WHY),
            )
            .unwrap();
        assert_eq!(task.status, "done");
        assert!(symphony_board::orchestration::unlanded(&task).is_some());
        assert_eq!(
            task.needs(),
            Some(needs(&assessor, &["tests"], Some(TIP), WHY))
        );
        // Still by a fresh session alone.
        assert!(
            refused(store.set_needs(
                "tools/query",
                &own,
                &needs(&exe, &["tests"], Some(TIP), WHY)
            ))
            .starts_with(&format!("{exe} executed tools/query"))
        );
        // Landed, it is assessed no more.
        store
            .landed("tools/query", &orch, "landed tools/query on main", false, &["done"])
            .unwrap();
        assert_eq!(
            refused(store.set_needs(
                "tools/query",
                &own,
                &needs(&assessor, &["review"], Some(TIP), WHY)
            )),
            "tools/query is done: only an open task, or one approved and not landed, is assessed"
        );
        assert_eq!(
            store.get("tools/query").unwrap().needs(),
            Some(needs(&assessor, &["tests"], Some(TIP), WHY))
        );
    }

    fn a_closed_or_missing_task_takes_no_needs_and_no_session_sets_the_field_by_hand(backend: Backend) {
        let f = Fixture::on(backend, Seed::default());
        let assessor = f.session("worker");
        let own = Catalogue::symphony();
        let orch = f.session("orchestrator");
        f.task("tools/gone");
        f.store
            .take_role("tools/gone", &orch, "orchestrator")
            .unwrap();
        f.store.move_to("tools/gone", "cancelled", &orch).unwrap();
        assert_eq!(
            refused(f.store.set_needs(
                "tools/gone",
                &own,
                &needs(&assessor, &["review"], None, WHY)
            )),
            "tools/gone is cancelled: only an open task, or one approved and not landed, is assessed"
        );
        assert_eq!(
            refused(f.store.set_needs(
                "tools/none",
                &own,
                &needs(&assessor, &["review"], None, WHY)
            )),
            "no task 'tools/none'"
        );
        f.task("tools/query");
        assert!(
            refused(
                f.store
                    .set_field("tools/query", "_board", "needs", &json!("none"), &assessor)
            )
            .contains("a task's needs by its assessor (tasks needs)")
        );
        assert_eq!(f.store.get("tools/query").unwrap().needs(), None);
    }
}
