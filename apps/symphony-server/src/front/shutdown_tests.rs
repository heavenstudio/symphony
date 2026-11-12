//! Hold a real SQLite flush at a pipe event, without sleeps or a production-only test seam.

use super::*;
use crate::testing::{Tree, board, entered, fifo, repository};
use std::io::Write;
use symphony_board::sys::Command;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEST: &str =
    "front::shutdown_tests::stop_reaps_an_in_flight_ask_flush_before_repository_cleanup";
const CHILD: &str = "SF_ASK_FLUSH_STOP_CHILD";

struct Release(std::fs::File);

impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.write_all(b"finish\n");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stop_reaps_an_in_flight_ask_flush_before_repository_cleanup() {
    if std::env::var_os(CHILD).is_none() {
        let tree = Tree::new();
        let root = repository(&tree);
        let control = tree.path("control");
        std::fs::create_dir(&control).unwrap();
        let real_sqlite = crate::sys::which("sqlite3").unwrap();
        let wrapper = control.join("sqlite3");
        // Only the journal SELECT is paused, after the ask's HTTP write has finished. The child
        // execs the actual SQLite executable; BEGIN IMMEDIATE opens its database before the event.
        // Open the release reader before announcing entry, so even an immediate writer close
        // cannot discard the release byte before the shell has a reader.
        crate::fixture_script::write(
            &wrapper,
            r##"#!/bin/bash
script=
while IFS= read -r line || [ -n "$line" ]; do script="$script$line"$'\n'; done
case "$script" in
  *'SELECT id, text FROM callbacks ORDER BY id;'*)
    if [ -f "$SF_FLUSH_CONTROL/armed" ]; then
      exec "$SF_REAL_SQLITE" "$@" <<SQL
.timeout 30000
BEGIN IMMEDIATE;
.shell exec 3<'$SF_FLUSH_CONTROL/release'; echo '$$' > '$SF_FLUSH_CONTROL/entered'; read -r finish <&3
$script
COMMIT;
SQL
    fi
    ;;
esac
exec "$SF_REAL_SQLITE" "$@" <<< "$script"
"##,
            false,
        );
        let client = board::client();
        symphony_board::Store::open(symphony_board::rpc::Client::new(
            client.addr(),
            "sf",
            client.database(),
        ))
        .unwrap();
        let path = std::env::join_paths(
            std::iter::once(control.clone())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let done = symphony_board::sys::output(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, &root)
                .env("SF_REAL_SQLITE", real_sqlite)
                .env("SF_FLUSH_CONTROL", control)
                .env("ASKS_DB", root.join("asks.db"))
                .env("TASKS_SERVER", client.addr())
                .env("TASKS_DATABASE", client.database())
                .env("PATH", path)
                .env_remove("TASKS_SESSION")
                .env_remove("SF_SYMPHONY_UI_DIST"),
        )
        .unwrap();
        assert!(
            done.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&done.stdout),
            String::from_utf8_lossy(&done.stderr)
        );
        assert!(String::from_utf8_lossy(&done.stdout).contains("1 passed"));
        assert!(
            !root.exists(),
            "no worker recreated the repository after stop"
        );
        return;
    }

    let root = PathBuf::from(std::env::var_os(CHILD).unwrap());
    let control = PathBuf::from(std::env::var_os("SF_FLUSH_CONTROL").unwrap());
    let entry = fifo(&control.join("entered"));
    let release = Release(fifo(&control.join("release")));
    // Journal draining belongs to a shared hub; its home is private to this fixture.
    let home = control.join("shared-home");
    std::fs::create_dir(&home).unwrap();
    let shared = Shared::new(
        i64::from(std::process::id()),
        1,
        &root,
        Some(&home),
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let (first, second) = tokio::join!(crate::api::warm(&shared), crate::api::warm(&shared));
    let hub = first.unwrap();
    assert!(
        Arc::ptr_eq(&hub, &second.unwrap()),
        "first requests share one worker"
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let serving = shared.clone();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            router(serving.clone()).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .with_graceful_shutdown(async move { serving.until_stopped().await })
        .await
        .unwrap();
    });
    let body = r#"{"path":"docs/TOPIC.md","question":"Stop with a flush in flight"}"#;
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    stream.write_all(format!("POST /api/ask HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
    let mut response = String::new();
    tokio::time::timeout(
        Duration::from_secs(15),
        stream.read_to_string(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(
        response.contains("\"id\":"),
        "the ask was saved: {response}"
    );
    std::fs::write(control.join("armed"), "").unwrap();
    // A changed stamp guarantees another journal read even if the tick just cached it empty.
    std::fs::File::open(root.join("asks.db"))
        .unwrap()
        .set_modified(std::time::SystemTime::now() + Duration::from_secs(1))
        .unwrap();
    let pid = tokio::task::spawn_blocking(move || entered(&entry))
        .await
        .unwrap();
    assert!(sys::pid_alive(pid), "the flush child is actually in flight");
    let mut stopped = shared.stopped.subscribe();
    let (send, receive) = std::sync::mpsc::channel();
    let stopping = shared.clone();
    let stop = std::thread::spawn(move || {
        stopping.stop();
        send.send(sys::pid_alive(pid)).unwrap();
    });
    tokio::time::timeout(
        Duration::from_secs(30),
        stopped.wait_for(|stopped| *stopped),
    )
    .await
    .unwrap()
    .unwrap();
    // The event above proves stop has entered. A bounded receive checks it cannot return while
    // SQLite is held; this timeout is a negative assertion, never the flush's synchronization.
    let early = tokio::task::spawn_blocking(move || {
        let early = receive.recv_timeout(Duration::from_millis(100));
        (early, receive)
    })
    .await
    .unwrap();
    assert!(
        matches!(early.0, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
        "stop returned with a child alive: {:?}",
        early.0
    );
    drop(release);
    let child_alive = early.1.recv_timeout(Duration::from_secs(30)).unwrap();
    stop.join().unwrap();
    assert!(
        !child_alive,
        "stop returned before reaping its SQLite child"
    );
    assert!(!sys::pid_alive(pid));
    // This isolated server process has no other owned children. ECHILD also rejects a zombie.
    let mut status = 0;
    // SAFETY: waitpid writes to this owned status value; WNOHANG never blocks.
    assert_eq!(unsafe { libc::waitpid(-1, &mut status, libc::WNOHANG) }, -1);
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    assert!(*hub.draining.borrow());
    shared.stop(); // repeated stop must preserve the completed join
    assert!(
        crate::api::warm(&shared).await.is_err(),
        "stop cannot start another hub"
    );
    server.await.unwrap();
    if let Some(saves) = shared.saves.get() {
        saves.close();
    }
    std::fs::remove_dir_all(&root).unwrap();
    assert!(
        !root.exists(),
        "repository cleanup follows the completed stop"
    );
}

#[tokio::test]
async fn stopping_a_cold_server_prevents_its_first_hub_start() {
    let tree = Tree::new();
    let shared = Shared::new(
        1,
        1,
        &tree.root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    shared.stop();
    assert_eq!(
        crate::api::warm(&shared).await.err().as_deref(),
        Some("the Symphony server is stopping")
    );
    assert!(shared.api.get().is_none());
}

#[test]
fn background_shutdown_wakes_long_timers_and_remains_sticky() {
    let tree = Tree::new();
    let shared = Shared::new(
        1,
        1,
        &tree.root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let (entered, ready) = std::sync::mpsc::channel();
    let (finished, done) = std::sync::mpsc::channel();
    let waiting = shared.clone();
    let worker = std::thread::spawn(move || {
        entered.send(()).unwrap();
        assert!(waiting.wait(Duration::from_secs(300)));
        finished.send(()).unwrap();
    });
    ready.recv_timeout(Duration::from_secs(30)).unwrap();
    let background = Background {
        shared: shared.clone(),
        workers: vec![worker],
    };
    let shutdown = std::thread::spawn(move || drop(background));
    done.recv_timeout(Duration::from_secs(30)).unwrap();
    shutdown.join().unwrap();
    // The same timer called after stop must return promptly as well as observe the sticky signal.
    let (finished, done) = std::sync::mpsc::channel();
    let sticky = std::thread::spawn(move || {
        finished
            .send(shared.wait(Duration::from_secs(300)))
            .unwrap();
    });
    assert!(done.recv_timeout(Duration::from_secs(30)).unwrap());
    sticky.join().unwrap();
}

/// Stop waits for the hub's tick in flight. Run from a signal, that wait must not hold a runtime
/// worker: on a runtime with only one, a task still runs while stop is held.
#[test]
fn a_signalled_stop_leaves_the_runtime_worker_free_while_it_waits() {
    let tree = Tree::new();
    let shared = Shared::new(
        1,
        1,
        &tree.root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    // Stop sets its flag first, then waits on this lock, as it does behind a hub's tick.
    let tick = shared.lifecycle.lock().unwrap();
    let (signal, signalled) = tokio::sync::oneshot::channel::<()>();
    let signals = {
        let _runtime = runtime.enter();
        stop_when(&shared, async move {
            let _ = signalled.await;
        })
    };
    signal.send(()).unwrap();
    assert!(
        shared.wait(Duration::from_secs(30)),
        "the signal did not start a stop"
    );
    let (answered, answer) = std::sync::mpsc::channel();
    runtime.spawn(async move { answered.send(()).unwrap() });
    // Only a worker that stop does not hold can send this answer before the tick ends.
    let free = answer.recv_timeout(Duration::from_secs(30));
    drop(tick);
    runtime.block_on(signals).unwrap();
    assert!(
        free.is_ok(),
        "stop held the runtime's only worker: {free:?}"
    );
    assert!(shared.stopping());
}

/// [`Stages`] that send each stage the stop names, in order, to the receiver.
fn told() -> (Stages, std::sync::mpsc::Receiver<String>) {
    let (send, said) = std::sync::mpsc::channel();
    let stages: Stages = Arc::new(move |stage: &str| {
        let _ = send.send(stage.to_owned());
    });
    (stages, said)
}

/// A started server keeps its threads, and waits for each as it returns, naming each wait, its
/// saves' too, before it begins.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_started_server_keeps_every_worker_it_owns_until_it_returns() {
    let tree = Tree::new();
    let root = tree.path("root");
    let home = tree.path("home");
    std::fs::create_dir(&home).unwrap();
    // The shared installation's sweep runs this launcher, which answers at once.
    let launcher = tree.path("root/tools/rs-bin.sh");
    std::fs::create_dir_all(launcher.parent().unwrap()).unwrap();
    crate::fixture_script::write(&launcher, "#!/bin/sh\nexit 0\n", false);
    let mut started = start(
        Serve {
            home: Some(home),
            root: root.clone(),
            port: 0,
            hosts: vec!["127.0.0.1".into()],
            install: None,
        },
        false,
    )
    .await
    .unwrap();
    let (stages, said) = told();
    started.stages = stages;
    started
        .shared
        .saves
        .get_or_init(|| symphony::save::Saves::start(&root));
    // A server with a home is the one server, which stops its other projects first.
    let mut waits: Vec<String> = [
        "the other projects",
        "the open requests",
        "the save in flight",
        "the verdict filing",
        "the data refresh",
    ]
    .map(String::from)
    .into();
    waits.extend(started.background.workers.iter().map(|worker| {
        let name = worker.thread().name().unwrap_or("review background");
        format!("the {name} worker")
    }));
    let owned: std::collections::BTreeSet<String> = started
        .background
        .workers
        .iter()
        .filter_map(|worker| worker.thread().name().map(String::from))
        .collect();
    for name in [TAILNET_WORKER, SWEEP_WORKER, RELOAD_WORKER] {
        assert!(
            owned.contains(name),
            "the {name} worker is not kept for the join: {owned:?}"
        );
    }
    assert!(port_file(&root).is_file());
    assert_eq!(started.serve_until_stopped(std::future::ready(())).await, 0);
    assert!(
        !port_file(&root).exists(),
        "the server returned without clearing its port record"
    );
    assert_eq!(said.try_iter().collect::<Vec<_>>(), waits);
}

/// A generation with no worker of its own, saying its stop's waits through `stages`.
fn generation(shared: &Arc<Shared>, stages: Stages) -> Started {
    Started {
        serve: Serve {
            home: None,
            root: shared.root.clone(),
            port: 0,
            hosts: Vec::new(),
            install: None,
        },
        shared: shared.clone(),
        projects: None,
        background: Background {
            shared: shared.clone(),
            workers: Vec::new(),
        },
        serving: Vec::new(),
        record: false,
        stages,
        drain: REPLACED_DRAIN,
    }
}

/// A replaced generation waits for what it accepted, but not for ever: a request whose peer stopped
/// reading (here a serving task that never ends) holds it for its drain and no longer, the stop
/// naming that it dropped it. An unreplaced one's grace is unchanged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_replaced_generation_ends_once_a_request_that_never_finishes_outlasts_its_drain() {
    let shared = Shared::new(
        303,
        1,
        Path::new("/replaced-drain-fixture"),
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    shared.replaced.store(true, Ordering::SeqCst);
    let (stages, said) = told();
    let mut started = generation(&shared, stages);
    started.drain = Duration::from_millis(300);
    let (finished, finishing) = tokio::sync::oneshot::channel::<()>();
    started.serving = vec![
        // A request that finishes within the drain is waited for.
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = finished.send(());
        }),
        // One whose peer never reads its answer never does.
        tokio::spawn(std::future::pending()),
    ];
    let began = Instant::now();
    let status = tokio::time::timeout(
        Duration::from_secs(30),
        started.serve_until_stopped(std::future::ready(())),
    )
    .await
    .expect("a replaced generation held by a request that never ends never returned");
    assert_eq!(status, 0);
    assert!(
        began.elapsed() >= Duration::from_millis(300),
        "{:?}",
        began.elapsed()
    );
    assert!(
        finishing.await.is_ok(),
        "the request within the drain was not waited for"
    );
    assert_eq!(
        said.try_iter().collect::<Vec<_>>(),
        [
            "the open requests",
            "the open requests outlasted 0.3 s: dropping what is still open",
            "the data refresh"
        ]
    );
}

/// A generation can reload into the next while the one before it still drains, and both write
/// to the one log they inherited: each stage line names the generation that stops, so the held
/// generation's wait reads as its own while the next one stops beside it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn overlapping_drains_each_name_their_own_generation_in_the_shared_log() {
    let (send, mut log) = tokio::sync::mpsc::unbounded_channel();
    let into_log = |send: tokio::sync::mpsc::UnboundedSender<String>| {
        move |line: String| {
            let _ = send.send(line);
        }
    };
    let held = Shared::new(
        101,
        1,
        Path::new("/overlapping-drain-fixture-101"),
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    // Generation 101 was replaced, and drains a request that ends when the test releases it.
    held.replaced.store(true, Ordering::SeqCst);
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let mut draining = generation(&held, stages_of(101, into_log(send.clone())));
    draining.serving = vec![tokio::spawn(async move {
        let _ = released.await;
    })];
    let first = tokio::spawn(draining.serve_until_stopped(std::future::ready(())));
    // Generation 101 is held at its open request; 202 stops all the way beside it.
    let held_at = tokio::time::timeout(Duration::from_secs(30), log.recv())
        .await
        .expect("the held generation never named its wait")
        .unwrap();
    let next = Shared::new(
        202,
        1,
        Path::new("/overlapping-drain-fixture-202"),
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let status = generation(&next, stages_of(202, into_log(send)))
        .serve_until_stopped(std::future::ready(()))
        .await;
    assert_eq!(status, 0);
    release.send(()).unwrap();
    let status = tokio::time::timeout(Duration::from_secs(30), first)
        .await
        .expect("the held generation never returned after its request ended")
        .unwrap();
    assert_eq!(status, 0);
    let mut lines = vec![held_at];
    while let Ok(line) = log.try_recv() {
        lines.push(line);
    }
    assert_eq!(
        lines,
        [
            "· Symphony server pid 101 stopping: the open requests",
            "· Symphony server pid 202 stopping: the open requests",
            "· Symphony server pid 202 stopping: the data refresh",
            "· Symphony server pid 101 stopping: the data refresh",
        ]
    );
}
