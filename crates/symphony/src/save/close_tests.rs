//! A close is a barrier: it returns only once the save in flight is done, and a document save after
//! it is refused. With no verdict committed, there are no publications to wait for.

use super::*;
use crate::test_tree::Tree;
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn a_close_waits_for_the_save_in_flight_and_names_what_it_waits_on() {
    let saves = Arc::new(Saves::start(Path::new("/nowhere")));
    let in_flight = saves.writes.lock().unwrap();
    let (stage, stages) = mpsc::channel();
    let closing = {
        let saves = saves.clone();
        std::thread::spawn(move || {
            saves.close_saying(|named| stage.send(named.to_owned()).unwrap())
        })
    };
    assert_eq!(stages.recv().unwrap(), "the save in flight");
    // A negative assertion, never the synchronization: the close may not end while the save runs.
    #[expect(
        clippy::disallowed_methods,
        reason = "no-timed-waits baseline: a test holds for a length of time to show the close waits for the save"
    )]
    std::thread::sleep(Duration::from_millis(200));
    assert!(!closing.is_finished(), "a close returned during a save");
    drop(in_flight);
    closing.join().unwrap();
    assert!(saves.closed.load(Ordering::Acquire));
    assert_eq!(
        stages.try_iter().collect::<Vec<_>>(),
        Vec::<String>::new(),
        "no verdict committed: no publications to wait for"
    );
}

#[test]
fn a_document_save_after_a_close_is_refused_and_writes_nothing() {
    let tree = Tree::new();
    tree.write("docs/A.md", "# A\n\nText.\n");
    let saves = Saves::start(&tree.root);
    saves.close();
    let failure = saves
        .post(
            "",
            "/api/comment",
            &json!({"path": "docs/A.md", "kind": "dislike", "title": "Too long", "body": "", "after_line": 3}),
        )
        .unwrap_err();
    assert_eq!(
        (failure.status, failure.error.as_str()),
        (503, "document saves are closing")
    );
    assert_eq!(tree.read("docs/A.md"), "# A\n\nText.\n");
}

#[test]
fn the_owners_workflow_operation_names_its_task_and_carries_a_whole_request() {
    let tree = Tree::new();
    let saves = Saves::start(&tree.root);
    let refused = |payload: Value| {
        let failure = saves.post("", WORKFLOW_APPLY, &payload).unwrap_err();
        (failure.status, failure.error)
    };
    assert_eq!(
        refused(json!({"request": {}})),
        (400, "slug is required".to_string())
    );
    let (status, error) = refused(json!({"slug": "sim/a", "request": {"task": "sim/a"}}));
    assert_eq!(status, 400);
    assert!(
        error.starts_with("request: not a workflow operation: "),
        "{error}"
    );
    let request = json!({
        "project": "sim", "task": "sim/b", "edge": "claim",
        "expected": {"workflow": {"format": 1, "digest": "d", "source": {"commit": "c", "project_blob": "b"}},
            "activation_generation": 1, "task_revision": 1, "at": {"phase": "build", "state": "available"}},
        "input_revisions": {}, "idempotency_key": "k", "inputs": {},
    });
    assert_eq!(
        refused(json!({"slug": "sim/a", "request": request})),
        (
            400,
            "request: its task is sim/b, not the slug sim/a".to_string()
        )
    );
    // A guarded edge's attestations are the authority's signed statements, read whole.
    let mut ours = request.clone();
    ours["task"] = "sim/a".into();
    let (status, error) = refused(json!({"slug": "sim/a", "request": ours,
        "attestations": [{"payload": "{}"}]}));
    assert_eq!(status, 400);
    assert!(
        error.starts_with("attestations: not signed statements: "),
        "{error}"
    );
}
