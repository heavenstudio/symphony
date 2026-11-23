//! A task made on either database: what it is made with (its parent, links, the tasks it waits on,
//! its reservation and its marks), the line that says so, and the refusals before anything is
//! written; a body and a line of a megabyte, as each was given.

use crate::common::{Fixture, Seed, bounded, refused};
use serde_json::json;
use std::time::Duration;
use symphony_board::{NewTask, Task};

fn board() -> Fixture {
    Fixture::new(
        Seed::default()
            .task("tools/base", "ready")
            .task("tools/done", "done"),
    )
}

/// What a test reads of a task: everything but its times and number, which each database gives
/// its own.
fn shape(task: &Task) -> serde_json::Value {
    json!({
        "slug": task.slug, "title": task.title, "body": task.body, "status": task.status,
        "priority": task.priority, "parent": task.parent, "created_by": task.created_by,
        "links": task.links, "depends": task.depends, "instruction": task.instruction,
        "fields": task.fields,
        "roles": task.roles.iter().map(|role| [&role.handle, &role.role]).collect::<Vec<_>>(),
    })
}

#[test]
fn an_owner_task_acknowledges_its_commit_before_optional_orchestration() {
    let f = board();
    let orchestrator = f.session("orchestrator");
    let sender = f.session("worker");
    f.store
        .take_role("tools/base", &orchestrator, "orchestrator")
        .unwrap();
    f.advance(symphony_board::health::LIVE_SECONDS + 1.0);
    f.store.touch(&orchestrator).unwrap();
    let listener = crate::callbacks::Listener::new(f.store.database().database());
    f.store
        .callback(
            &orchestrator,
            Some(&format!("uds:{}", listener.path.display())),
            false,
            false,
        )
        .unwrap();
    listener.drain(&f, &sender, &orchestrator);
    let fast = f.other().dispatching_by(Box::new(|| {
        panic!("acknowledgement must not wait for delivery")
    }));
    let new = NewTask::new("tools/queued-owner", "Queued owner task", "owner");
    let log = fast.database().queries();
    let before = log.within(&(i64::MIN..=i64::MAX));
    let made = fast.persist_owner_task(&new, "queued-receipt").unwrap();
    let after = log.within(&(i64::MIN..=i64::MAX));
    assert_eq!(
        after.0.count - before.0.count,
        1,
        "acknowledge with only the creation transaction"
    );
    assert_eq!(made.fields["_symphony"]["owner_save"], "queued-receipt");
    assert!(made.roles.is_empty(), "optional give has not run");
    assert_eq!(
        f.store.owner_saved("queued-receipt").unwrap(),
        ["tools/queued-owner"]
    );
    assert_eq!(f.store.thread(&new.slug).unwrap().len(), 1);
    assert!(matches!(
        f.store.persist_owner_task(&new, "other-receipt"),
        Err(symphony_board::Error::Refused(_))
    ));
    let resumed = f.other();
    let board = resumed.board().unwrap();
    assert!(
        resumed.give_if_due(&board),
        "a later process recovers the give"
    );
    let task = f.store.get(&new.slug).unwrap();
    assert!(
        task.roles
            .iter()
            .any(|r| r.handle == orchestrator && r.role == "orchestrator")
    );
    assert!(
        !f.store.give_if_due(&f.store.board().unwrap()),
        "delivery is not repeated"
    );
    assert_eq!(f.store.thread(&new.slug).unwrap().len(), 2);
    resumed.dispatch_callbacks().unwrap();
    let lines = listener.drain(&f, &sender, &orchestrator);
    assert_eq!(
        lines
            .iter()
            .filter(|line| line.starts_with("GIVEN ") && line.contains(&new.slug))
            .count(),
        1,
        "{lines:?}"
    );
    let immediate = f
        .store
        .create_owner_task(
            &NewTask::new("tools/immediate-owner", "Immediate owner task", "owner"),
            "immediate-receipt",
        )
        .unwrap();
    assert!(
        immediate
            .roles
            .iter()
            .any(|r| r.handle == orchestrator && r.role == "orchestrator"),
        "the existing public method still gives before returning"
    );
}

#[test]
fn a_task_is_made_with_its_links_waits_reservation_and_line() {
    let f = board();
    let worker = f.session("worker");
    let made = f
        .store
        .create(&NewTask {
            body: "The brief.".into(),
            priority: 1,
            parent: Some("tools/base".into()),
            links: vec!["docs/TASKS.md".into(), "docs/CHECKS.md".into()],
            depends: vec!["tools/done".into(), "tools/base".into()],
            assignee: Some(worker.clone()),
            ..NewTask::new("tools/base/child", "A child", "owner")
        })
        .unwrap();
    assert_eq!(made, f.store.get("tools/base/child").unwrap());
    let mut shown = shape(&made);
    shown["links"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(ToString::to_string);
    shown["depends"]
        .as_array_mut()
        .unwrap()
        .sort_by_key(ToString::to_string);
    assert_eq!(
        shown,
        json!({
            "slug": "tools/base/child", "title": "A child", "body": "The brief.",
            "status": "ready", "priority": 1, "parent": "tools/base", "created_by": "owner",
            "links": ["docs/CHECKS.md", "docs/TASKS.md"],
            "depends": ["tools/base", "tools/done"], "instruction": false, "fields": {},
            "roles": [[worker.as_str(), "reserved"]],
        })
    );
    let thread = f.store.thread("tools/base/child").unwrap();
    assert_eq!(thread.len(), 1, "{thread:?}");
    assert_eq!(thread[0].sender, "owner");
    assert_eq!(made.id, f.store.get("tools/done").unwrap().id + 1);
}

#[test]
fn a_red_and_an_owners_receipt_are_marks_on_the_task() {
    let f = board();
    let filer = f.session(symphony_board::red::FILER);
    let red = f
        .store
        .create(&NewTask {
            red: Some("main".into()),
            ..NewTask::new("tests/main-red-1", "A red", &filer)
        })
        .unwrap();
    assert_eq!(red.fields["_board"]["red"], json!("main"));
    f.store
        .create_owner_task(&NewTask::new("tools/saved", "Saved", "owner"), "receipt-1")
        .unwrap();
    let receipt = f.store.raw().row("task", "tools/saved");
    assert_eq!(
        receipt.field("fields._symphony.owner_save").unwrap(),
        json!("receipt-1")
    );
}

#[test]
fn a_task_is_refused_before_anything_is_written() {
    let f = board();
    let worker = f.session("worker");
    let marker = f.store.marker().unwrap();
    let taken = refused(
        f.store
            .create(&NewTask::new("tools/base", "Again", "owner")),
    );
    let red = refused(f.store.create(&NewTask {
        red: Some("main".into()),
        ..NewTask::new("tests/main-red-2", "A red", &worker)
    }));
    let waits = refused(f.store.create(&NewTask {
        depends: vec!["tools/none".into()],
        ..NewTask::new("tools/waiting", "Waits", "owner")
    }));
    assert_eq!(taken, "task tools/base already exists");
    assert_eq!(
        red,
        format!(
            "only a red's filer marks a task red, a session of kind filer as the tools that \
                 file reds register; '{worker}' is not one"
        )
    );
    assert_eq!(waits, "no task 'tools/none'");
    assert_eq!(f.store.marker().unwrap(), marker, "nothing was written");
    assert_eq!(f.store.tasks(None).unwrap().len(), 2);
}

#[test]
fn a_long_body_and_a_long_line_are_written_and_read_back_whole() {
    let f = board();
    let what = format!("a task and a line of {LONG:?} bytes each, on {backend:?}");
    bounded(&what, Duration::from_secs(10), move || {
        for (at, bytes) in LONG.into_iter().enumerate() {
            let slug = format!("tools/long-{at}");
            let made = f
                .store
                .create(&NewTask {
                    body: words(bytes),
                    ..NewTask::new(&slug, "A long brief", "owner")
                })
                .unwrap();
            assert_eq!(made.body, words(bytes));
            f.store.say(&slug, "owner", &words(bytes), None).unwrap();
            assert_eq!(f.store.get(&slug).unwrap().body, words(bytes));
            assert_eq!(f.said(&slug).last(), Some(&words(bytes)));
        }
    });
}

/// The lengths of text, in bytes, a board takes and gives back whole: past the 16 KiB from which
/// a PostgreSQL request whose answer came while it was still being sent once hung, and a megabyte.
const LONG: [usize; 2] = [64 << 10, 1 << 20];

/// Words, `bytes` of them, as a brief is written.
fn words(bytes: usize) -> String {
    let mut text: String = "the brief says a word "
        .chars()
        .cycle()
        .take(bytes - 1)
        .collect();
    text.push('.');
    text
}
