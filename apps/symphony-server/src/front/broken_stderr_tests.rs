//! A stop whose stderr is broken still closes every save and joins every worker: each line it
//! says on the way is a write whose failure is ignored, never a panic that ends the stop early.

use super::*;
use crate::testing::{Tree, repository};
use symphony_board::sys::{self, Stdio};

const TEST: &str = "front::broken_stderr_tests::a_stop_with_a_broken_stderr_closes_every_save_and_joins_every_worker";
const CHILD: &str = "SYMPHONY_BROKEN_STDERR_STOP_CHILD";
const DONE: &str = "broken-stderr stop: saves closed, every worker joined";

/// The stop runs in a child of its own whose stderr is a pipe with no reader from before it
/// starts, so every write to it fails (`EPIPE`), and the rest of this test run keeps its stderr.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stop_with_a_broken_stderr_closes_every_save_and_joins_every_worker() {
    if std::env::var_os(CHILD).is_some() {
        return stop_with_stderr_broken().await;
    }
    let (reader, writer) = sys::pipe().unwrap();
    drop(reader);
    let child = sys::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST, "--nocapture"])
        .env(CHILD, "1")
        .env_remove("TASKS_SESSION")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(writer)
        .spawn()
        .unwrap();
    let done = sys::wait_output(child).unwrap();
    let said = String::from_utf8_lossy(&done.stdout);
    assert!(done.status.success(), "{said}");
    assert!(said.contains(DONE), "{said}");
}

/// The child's side: a worker that panicked and a worker held until the test releases it, each
/// of which the stop names on its broken stderr before the next.
async fn stop_with_stderr_broken() {
    assert!(
        std::io::Write::write_all(&mut std::io::stderr(), b"probe\n").is_err(),
        "this child's stderr still has a reader"
    );
    let tree = Tree::new();
    let root = repository(&tree);
    let shared = Shared::new(
        i64::from(std::process::id()),
        0,
        &root,
        None,
        crate::testing::open_gate(),
    );
    shared
        .saves
        .get_or_init(|| symphony::save::Saves::start(&root));
    let panicking = worker("panicking probe", || panic!("the worker probe panics"));
    let (release, released) = std::sync::mpsc::channel::<()>();
    let joined = Arc::new(AtomicBool::new(false));
    let held = {
        let joined = joined.clone();
        worker("held probe", move || {
            let _ = released.recv();
            joined.store(true, Ordering::SeqCst);
        })
    };
    let started = Started {
        serve: Serve {
            home: None,
            root,
            port: 0,
            hosts: Vec::new(),
            install: None,
        },
        shared: shared.clone(),
        projects: None,
        background: Background {
            shared: shared.clone(),
            workers: vec![panicking, held],
        },
        serving: Vec::new(),
        record: false,
        stages: stages_on_stderr(shared.pid),
        drain: REPLACED_DRAIN,
    };
    let mut serving = tokio::spawn(started.serve_until_stopped(std::future::ready(())));
    // This bounded wait is a negative assertion, never the synchronization: the stop must still
    // be joining the held worker, past every line said before it.
    let early = tokio::time::timeout(Duration::from_millis(200), &mut serving).await;
    assert!(
        early.is_err(),
        "the stop ended before its last join: {early:?}"
    );
    release.send(()).unwrap();
    // Only a stall reaches this backstop, and its failure says the load average.
    let backstop = test_time::backstop(Duration::from_secs(30));
    let status = tokio::time::timeout(backstop, serving)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "the stop to return after the held worker ended: stalled: the wait's wall-clock \
                 backstop of {backstop:?} ran out; {}",
                test_time::Load::now()
            )
        })
        .expect("the stop panicked");
    assert_eq!(status, 0);
    assert!(
        joined.load(Ordering::SeqCst),
        "the stop returned before its held worker ended"
    );
    let saves = shared.saves.get().unwrap();
    let payload =
        serde_json::json!({"path": "docs/TOPIC.md", "slug": "absent-card", "state": "approved"});
    let refused = saves.post("", "/api/decision", &payload).err().unwrap();
    assert_eq!(
        (refused.status, refused.error.as_str()),
        (503, "document saves are closing")
    );
    println!("{DONE}");
}
