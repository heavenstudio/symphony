//! The board's ticks on a thread of their own, beside the document refreshes: a tick due while a
//! refresh is held mid-render is published before that refresh ends, a refresh that read the board
//! before a tick places no older task counts than the tick sent, and the ticks' thread ends when the
//! hub stops, or when its project goes, the document loop with it; a run that begins after its hub
//! stopped or went starts neither.

use super::Hub;
use super::read_paths_tests::{PNG, fifo, holds, observe, writer};
use crate::backstop::Backstop;
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository,
};
use notify::Watcher as _;
use serde_json::Value;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};
use std::thread::JoinHandle;
use std::time::Duration;

/// The wall-clock backstop of a wait here on its event: only a stall reaches it (RULES.md
/// `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// A board holding `tools/held` in progress, its executor `worker`.
fn held_in_progress() -> Fixture {
    Fixture::new(
        Seed::default()
            .session("worker")
            .task("tools/held", "in_progress")
            .role("tools/held", "worker", "executor"),
    )
}

/// A hub started as the server starts one on `root`, reading the board of `fixture` on a store of
/// its own through `connect`'s reach, once its first board read is through.
fn started(root: &Path, fixture: &Fixture, connect: impl Fn() + Send + 'static) -> Arc<Hub> {
    let mut store = Some(fixture.other());
    let hub = Hub::start_watching::<notify::NullWatcher>(
        root,
        &root.join("build/client"),
        false,
        false,
        crate::backstop::PERIOD,
        move |_| {
            connect();
            store.take().ok_or_else(|| "one store in this test".into())
        },
    )
    .unwrap();
    hub.start_initial_board();
    test_time::until_true("the first board read", BACKSTOP, || {
        in_review(&hub) == Some(0)
    })
    .unwrap();
    hub
}

/// The tasks in review as main's page holds the board's counts, none while it holds none.
fn in_review(hub: &Hub) -> Option<u32> {
    hub.pages.read().unwrap()[""]
        .board
        .counts
        .tasks
        .as_ref()
        .map(|counts| counts.in_review)
}

/// The tasks in review each `counts` change on main's page said, in the order they were sent.
fn in_review_sent(hub: &Hub) -> Vec<u64> {
    hub.pages.read().unwrap()[""]
        .history
        .iter()
        .flat_map(|event| {
            let event: Value = serde_json::from_str(&event.json).unwrap();
            event["changes"].as_array().unwrap().clone()
        })
        .filter(|change| change["region"] == "counts")
        .filter_map(|change| change["data"]["tasks"]["in_review"].as_u64())
        .collect()
}

/// `sent` says one task in review, and never none again after it first did.
fn never_back(sent: &[u64]) {
    let first = sent
        .iter()
        .position(|count| *count == 1)
        .unwrap_or_else(|| panic!("no counts said the task in review: {sent:?}"));
    assert!(
        sent[first..].iter().all(|count| *count == 1),
        "counts older than the tick's were sent after them: {sent:?}"
    );
}

/// Main's `docs/PAUSED.md` followed with its first words, then edited to show the picture
/// `docs/held.png`, a FIFO a render waits in until the test writes it: that picture's path.
fn paused_on_a_picture(tree: &Tree, root: &Path, hub: &Hub) -> std::path::PathBuf {
    tree.write("main/docs/PAUSED.md", "# Paused\n\nThe first words.\n");
    assert!(holds(
        &hub.document("", "docs/PAUSED.md").unwrap(),
        "The first words."
    ));
    let picture = root.join("docs/held.png");
    // Made before the text that shows it, so every render of that text waits in it.
    fifo(&picture);
    tree.write("main/docs/PAUSED.md", "# Paused\n\n![Held](held.png)\n");
    picture
}

/// Let every read of the FIFO `picture` end, until `done` says so.
fn release(picture: &Path, what: &str, done: impl Fn() -> bool) {
    test_time::until_true(what, BACKSTOP, || {
        if let Some(mut opened) = writer(picture) {
            let _ = opened.write_all(&PNG);
        }
        done()
    })
    .unwrap();
}

#[test]
fn a_tick_due_while_a_refresh_is_held_mid_render_is_published_before_that_refresh_ends() {
    let tree = Tree::new();
    let root = repository(&tree);
    let fixture = held_in_progress();
    let hub = started(&root, &fixture, || {});
    let picture = paused_on_a_picture(&tree, &root, &hub);
    // The document loop's next look refreshes main's documents, as after a lost event: its render of
    // the edited document waits in its read of the picture.
    hub.dirty.lock().unwrap().insert(root.clone());
    hub.rescan.store(true, Ordering::SeqCst);
    let mut opened = test_time::until("the refresh to read the picture", BACKSTOP, || {
        writer(&picture)
    })
    .unwrap();

    // Meanwhile the task goes to review, which the next tick reads.
    fixture
        .store
        .move_to("tools/held", "review", "worker")
        .unwrap();
    let published = test_time::until_true("the tick to publish the move", BACKSTOP, || {
        in_review(&hub) == Some(1)
    });
    let copy_then = hub.pages.read().unwrap()[""].documents["docs/PAUSED.md"].clone();
    // The picture's bytes let the refresh end, the tick come or not.
    opened.write_all(&PNG).unwrap();
    drop(opened);
    release(&picture, "the refresh to place its copy", || {
        holds(
            &hub.pages.read().unwrap()[""].documents["docs/PAUSED.md"],
            "held.png",
        )
    });
    hub.stop();

    published.unwrap();
    assert!(
        holds(&copy_then, "The first words."),
        "the refresh had placed its copy when the tick was published"
    );
    assert_eq!(in_review(&hub), Some(1));
    never_back(&in_review_sent(&hub));
}

#[test]
fn a_refresh_that_read_the_board_before_a_tick_places_no_older_task_counts_than_the_tick_sent() {
    let tree = Tree::new();
    let root = repository(&tree);
    let fixture = held_in_progress();
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    observe(&hub, &fixture.store, &[]);
    assert_eq!(in_review(&hub), Some(0));
    let picture = paused_on_a_picture(&tree, &root, &hub);

    let tick_said = std::thread::scope(|scope| {
        let refreshing =
            scope.spawn(|| hub.refresh_docs(Some(&BTreeSet::from([root.join("docs/PAUSED.md")]))));
        let mut opened = test_time::until("the refresh to read the picture", BACKSTOP, || {
            writer(&picture)
        })
        .unwrap();
        // While it renders, the task goes to review and a tick sends the counts that say so.
        fixture
            .store
            .move_to("tools/held", "review", "worker")
            .unwrap();
        observe(&hub, &fixture.store, &[]);
        let tick_said = in_review(&hub);
        opened.write_all(&PNG).unwrap();
        drop(opened);
        release(&picture, "the refresh to end", || refreshing.is_finished());
        refreshing.join().unwrap().unwrap();
        tick_said
    });

    assert_eq!(tick_said, Some(1));
    assert!(
        holds(
            &hub.pages.read().unwrap()[""].documents["docs/PAUSED.md"],
            "held.png"
        ),
        "the refresh placed its copy"
    );
    assert_eq!(
        in_review(&hub),
        Some(1),
        "the page went back to the counts the refresh read before the tick"
    );
    never_back(&in_review_sent(&hub));
}

#[test]
fn the_ticks_thread_ends_when_the_hub_stops() {
    let tree = Tree::new();
    let root = repository(&tree);
    let fixture = held_in_progress();
    // Held by the reach the ticks' thread owns, and let go with it.
    let owner = Arc::new(());
    let owned = Arc::clone(&owner);
    let hub = started(&root, &fixture, move || {
        let _ = Arc::strong_count(&owned);
    });
    assert_eq!(Arc::strong_count(&owner), 2);

    // The ticks wait for their next second, which the stop ends.
    let stopping = Arc::clone(&hub);
    let stop = std::thread::spawn(move || stopping.stop());
    test_time::until_true("the stop to end", BACKSTOP, || stop.is_finished()).unwrap();
    stop.join().unwrap();
    assert!(
        hub.worker.lock().unwrap().is_none(),
        "the stop joined the worker"
    );
    assert_eq!(
        Arc::strong_count(&owner),
        1,
        "the ticks' thread, which owns the reach, still runs"
    );
}

/// The documents' loop runs on a period no test reaches ([`crate::backstop::NO_PERIOD`]) and a
/// watcher that says nothing, so it ends only as the ticks' end wakes it.
#[test]
fn a_project_removed_under_its_hub_ends_its_ticks_and_its_document_loop_with_no_stop() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Hub::start_watching::<notify::NullWatcher>(
        &root,
        &root.join("build/client"),
        false,
        false,
        crate::backstop::NO_PERIOD,
        |_: &Path| Err("no board in this test".to_string()),
    )
    .unwrap();
    hub.start_initial_board();
    test_time::until_true("the first board read", BACKSTOP, || {
        hub.has_board().is_some()
    })
    .unwrap();

    std::fs::remove_dir_all(&root).unwrap();
    test_time::until_true("both loops to end", BACKSTOP, || {
        hub.worker
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
    })
    .unwrap();
    assert_eq!(hub.tasks_read().err(), Some(hub.gone()));
    hub.stop();
}

/// A reconnect panic after the initial read ends the ticks and wakes the documents' loop even
/// while unwinding. With no watcher events and a period no test reaches, the enclosing worker
/// finishes and surfaces the panic without a separate stop.
#[test]
fn a_panicking_tick_wakes_the_documents_loop_and_ends_its_worker_without_a_stop() {
    let tree = Tree::new();
    let root = repository(&tree);
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let called = Arc::clone(&calls);
    let hub = Hub::start_watching::<notify::NullWatcher>(
        &root,
        &root.join("build/client"),
        false,
        false,
        crate::backstop::NO_PERIOD,
        move |_: &Path| {
            if called.fetch_add(1, Ordering::SeqCst) == 0 {
                Err("no board in this test".to_string())
            } else {
                panic!("the tick's reconnect panicked");
            }
        },
    )
    .unwrap();
    hub.start_initial_board();

    test_time::until_true("the tick panic to end both loops", BACKSTOP, || {
        hub.worker
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(JoinHandle::is_finished)
    })
    .unwrap();
    let worker = hub.worker.lock().unwrap().take().unwrap();
    assert!(worker.join().is_err(), "the worker surfaces the tick panic");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(*hub.initial_board.borrow(), "the initial read ended first");
    assert!(!*hub.draining.borrow(), "no stop ended the documents' wait");
}

/// The hub's loops run on `hub`, on a thread of their own, with no board yet, so a first tick would
/// reach for one: whether one did, once the run returned.
fn reached_after_run(hub: Weak<Hub>) -> bool {
    let reached = Arc::new(AtomicBool::new(false));
    let reach = Arc::clone(&reached);
    let watcher = notify::NullWatcher::new(|_| {}, notify::Config::default()).unwrap();
    let running = std::thread::spawn(move || {
        super::run(
            hub,
            watcher,
            BTreeSet::new(),
            Backstop::new(crate::backstop::PERIOD),
            None,
            move |_: &Path| {
                reach.store(true, Ordering::SeqCst);
                Err("no board in this test".to_string())
            },
        );
    });
    test_time::until_true("the run to return", BACKSTOP, || running.is_finished()).unwrap();
    running.join().unwrap();
    reached.load(Ordering::SeqCst)
}

#[test]
fn a_run_whose_hub_stopped_or_went_before_it_began_starts_neither_loop() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = Arc::new(Hub::new(&root, &root.join("build/client")));
    // The stop came while the first board read was under way, before the loops began.
    hub.stop();
    assert!(
        !reached_after_run(Arc::downgrade(&hub)),
        "a tick reached for the board after the stop"
    );
    assert!(
        hub.ticks.lock().unwrap().is_none(),
        "the run kept a stop signal no stop will take"
    );

    let gone = Arc::downgrade(&hub);
    drop(hub);
    assert!(
        !reached_after_run(gone),
        "a tick reached for the board of a hub gone"
    );
}
