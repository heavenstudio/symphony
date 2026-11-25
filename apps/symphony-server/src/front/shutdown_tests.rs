//! Hold a real SQLite flush at a pipe event, without sleeps or a production-only test seam.

use super::*;
use crate::testing::{Tree, board, entered, fifo, joined, repository};
use std::io::Write;
use symphony_board::sys::Command;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

const TEST: &str =
    "front::shutdown_tests::stop_reaps_an_in_flight_ask_flush_before_repository_cleanup";
const CHILD: &str = "SYMPHONY_ASK_FLUSH_STOP_CHILD";

/// The wall-clock backstop of every wait here: each waits on its event, and only a stall reaches
/// this (RULES.md `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// What a wait that ran out says: a stall, and the machine's load then.
fn stalled(what: &str) -> String {
    format!(
        "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
        test_time::Load::now()
    )
}

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
  *'SELECT id, text, ask FROM callbacks ORDER BY id;'*)
    if [ -f "$SYMPHONY_FLUSH_CONTROL/armed" ]; then
      exec "$SYMPHONY_REAL_SQLITE" "$@" <<SQL
.timeout 30000
BEGIN IMMEDIATE;
.shell exec 3<'$SYMPHONY_FLUSH_CONTROL/release'; echo '$$' > '$SYMPHONY_FLUSH_CONTROL/entered'; read -r finish <&3
$script
COMMIT;
SQL
    fi
    ;;
esac
exec "$SYMPHONY_REAL_SQLITE" "$@" <<< "$script"
"##,
            false,
        );
        let client = board::database();
        // The board's command line makes the board; the Symphony, a reader, never does.
        crate::testing::store_on(&client);
        let path = std::env::join_paths(
            std::iter::once(control.clone())
                .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap())),
        )
        .unwrap();
        let done = symphony_board::sys::output(
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST, "--nocapture"])
                .env(CHILD, &root)
                .env("SYMPHONY_REAL_SQLITE", real_sqlite)
                .env("SYMPHONY_FLUSH_CONTROL", control)
                .env("ASKS_DB", root.join("asks.db"))
                .env("TASKS_SERVER", client.addr())
                .env("TASKS_DATABASE", client.database())
                .env("PATH", path)
                .env_remove("TASKS_SESSION")
                .env_remove("SYMPHONY_UI_DIST"),
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
    let control = PathBuf::from(std::env::var_os("SYMPHONY_FLUSH_CONTROL").unwrap());
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
        crate::testing::open_gate(),
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
    tokio::time::timeout(BACKSTOP, stream.read_to_string(&mut response))
        .await
        .unwrap_or_else(|_| panic!("{}", stalled("the ask's answer")))
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
    let (send, receive) = crate::test_channel::channel();
    let stopping = shared.clone();
    let stop = std::thread::spawn(move || {
        stopping.stop();
        send.send(sys::pid_alive(pid)).unwrap();
    });
    tokio::time::timeout(BACKSTOP, stopped.wait_for(|stopped| *stopped))
        .await
        .unwrap_or_else(|_| panic!("{}", stalled("the stop to begin")))
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
    let child_alive = early
        .1
        .recv_timeout(BACKSTOP)
        .unwrap_or_else(|_| panic!("{}", stalled("the stop to return once SQLite is released")));
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
    let shared = Shared::new(1, 1, &tree.root, None, crate::testing::open_gate());
    shared.stop();
    assert_eq!(
        crate::api::warm(&shared).await.err().as_deref(),
        Some("the Symphony server is stopping")
    );
    assert!(shared.api.get().is_none());
}

/// A server whose stop the test calls itself.
fn idle_shared(tree: &Tree) -> Arc<Shared> {
    Shared::new(1, 1, &tree.root, None, crate::testing::open_gate())
}

/// The stop wakes a loop that gave its waker before it, by message alone (the wait has no length),
/// and one that gives its waker after the stop is woken as it gives it, so no loop starting late
/// waits for a stop gone by.
#[test]
fn the_stop_wakes_each_loop_by_message_and_one_that_asks_after_it_at_once() {
    let tree = Tree::new();
    let shared = idle_shared(&tree);
    let (entered, ready) = crate::test_channel::channel();
    let (finished, done) = crate::test_channel::channel();
    let waiting = shared.clone();
    let worker = std::thread::spawn(move || {
        let wakes = Wakes::new();
        waiting.wake_at_stop(wakes.waker());
        entered.send(()).unwrap();
        // No length: only the stop's message ends this wait.
        assert!(wakes.wait(None));
        assert!(waiting.stopping());
        finished.send(()).unwrap();
    });
    ready
        .recv_timeout(BACKSTOP)
        .unwrap_or_else(|_| panic!("{}", stalled("the worker to start")));
    let background = Background {
        shared: shared.clone(),
        workers: vec![worker],
    };
    let shutdown = std::thread::spawn(move || drop(background));
    done.recv_timeout(BACKSTOP)
        .unwrap_or_else(|_| panic!("{}", stalled("shutdown to wake the waiting loop")));
    shutdown.join().unwrap();
    // A waker given after the stop: woken as it is given, its wait over before it begins.
    let late = Wakes::new();
    shared.wake_at_stop(late.waker());
    assert!(
        late.wait(Some(Duration::ZERO)),
        "a waker given after the stop is woken at once"
    );
}

/// A stop pipe reads its end at the stop, and one taken after the stop at once: what waits on a
/// descriptor beside a process's end ([`sys::until_ends`]) is ended by the stop alone.
#[test]
fn a_stop_pipe_reads_its_end_at_the_stop_and_one_taken_after_it_at_once() {
    use std::io::Read;
    let tree = Tree::new();
    let shared = idle_shared(&tree);
    let mut before = shared.stop_pipe().unwrap();
    let (read, ended) = crate::test_channel::channel();
    let reader = std::thread::spawn(move || {
        let mut rest = Vec::new();
        read.send(before.read_to_end(&mut rest).map(|_| rest))
            .unwrap();
    });
    // Nothing is written: the read ends only at its writer's going, which the stop is.
    assert!(
        ended.try_recv().is_err(),
        "a stop pipe reads nothing before the stop"
    );
    shared.stop();
    let rest = ended
        .recv_timeout(BACKSTOP)
        .unwrap_or_else(|_| panic!("{}", stalled("the stop to end the pipe's read")))
        .unwrap();
    assert!(rest.is_empty(), "the stop writes nothing: it ends the pipe");
    reader.join().unwrap();
    let mut after = shared.stop_pipe().unwrap();
    let mut rest = Vec::new();
    assert_eq!(after.read_to_end(&mut rest).unwrap(), 0, "ended as taken");
}

/// A hand-off asked for wakes each loop that waits for one, and one that gives its waker while a
/// hand-off waits is woken as it gives it; the flag stays for the reload worker to take.
#[test]
fn a_hand_off_asked_for_wakes_its_loops_and_one_that_asks_after_it() {
    let tree = Tree::new();
    let shared = idle_shared(&tree);
    let before = Wakes::new();
    shared.wake_at_handover(before.waker());
    assert!(!before.wait(Some(Duration::ZERO)), "no hand-off asked yet");
    shared.ask_handover();
    assert!(before.wait(Some(Duration::ZERO)), "the request wakes it");
    let after = Wakes::new();
    shared.wake_at_handover(after.waker());
    assert!(
        after.wait(Some(Duration::ZERO)),
        "a request waiting wakes it as it is given"
    );
    assert!(
        shared.handover.load(Ordering::SeqCst),
        "the flag waits for its taker"
    );
}

/// A server follows a process that is not its child to its end ([`end_when`]): that end stops the
/// server; the server's own stop ends the follow and leaves the process be; and a pid it cannot
/// follow is said, the server left to its stop alone.
#[test]
fn a_server_ends_with_the_process_it_follows_and_its_own_stop_ends_the_follow() {
    let tree = Tree::new();
    // A process that lives until the case kills it: it sleeps a day.
    let sleeper = || {
        std::process::Command::new("/bin/sleep")
            .arg("86400")
            .spawn()
            .unwrap()
    };

    let shared = idle_shared(&tree);
    let mut followed = sleeper();
    let pid = i64::from(followed.id());
    let follow = end_when(
        &shared,
        "follow test",
        "the process it follows ended",
        pid,
        || false,
    );
    followed.kill().unwrap();
    joined(follow, "the follow to see its process end", BACKSTOP);
    assert!(shared.stopping(), "the process's end stops the server");
    followed.wait().unwrap();

    let shared = idle_shared(&tree);
    let mut living = sleeper();
    let pid = i64::from(living.id());
    let follow = end_when(
        &shared,
        "follow test",
        "the process it follows ended",
        pid,
        || false,
    );
    shared.stop();
    joined(follow, "the stop to end the follow", BACKSTOP);
    assert!(living.try_wait().unwrap().is_none(), "the process goes on");
    living.kill().unwrap();
    living.wait().unwrap();

    let shared = idle_shared(&tree);
    let follow = end_when(&shared, "follow test", "no process", 0, || false);
    joined(follow, "a follow of no process to give up", BACKSTOP);
    assert!(!shared.stopping(), "only its stop ends the server now");
}

/// Stop waits for the hub's tick in flight. Run from a signal, that wait must not hold a runtime
/// worker: on a runtime with only one, a task still runs while stop is held.
#[test]
fn a_signalled_stop_leaves_the_runtime_worker_free_while_it_waits() {
    let tree = Tree::new();
    let shared = Shared::new(1, 1, &tree.root, None, crate::testing::open_gate());
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
    let wakes = Wakes::new();
    shared.wake_at_stop(wakes.waker());
    signal.send(()).unwrap();
    // The stop the signal starts wakes it; BACKSTOP is its bound alone.
    let stopped = wakes.wait(Some(BACKSTOP));
    assert!(stopped, "{}", stalled("the signal to start a stop"));
    let (answered, answer) = crate::test_channel::channel();
    runtime.spawn(async move { answered.send(()).unwrap() });
    // Only a worker that stop does not hold can send this answer before the tick ends.
    let free = answer.recv_timeout(BACKSTOP);
    drop(tick);
    runtime.block_on(signals).unwrap();
    assert!(
        free.is_ok(),
        "stop held the runtime's only worker: {free:?}: {}",
        stalled("a task on the runtime while stop waits")
    );
    assert!(shared.stopping());
}

/// [`Stages`] that send each stage the stop names, in order, to the receiver.
fn told() -> (Stages, std::sync::mpsc::Receiver<String>) {
    let (send, said) = crate::test_channel::channel();
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
    // The shared installation's sweep runs this build of `tasks`, which answers at once.
    let launcher = tree.path("root/.target/bin/tasks");
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
        hand_off_asked(),
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
    for name in [
        TAILNET_WORKER,
        SWEEP_WORKER,
        RELOAD_WORKER,
        crate::projects::REGISTRY_WORKER,
    ] {
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

/// The one server sweeps every project its machine folder registers, its starter's by a worker of
/// its own and each other's by its state's, each project's `tasks reap` run in its checkout and
/// recorded in its own board's health; its stop joins every one of them. The checkouts are folders
/// of a temporary tree, each `tasks` a stand-in, the machine folder a temporary one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_one_server_sweeps_every_project_it_serves_each_in_its_own_record() {
    let tree = Tree::new();
    for id in ["alpha", "beta"] {
        tree.write(&format!("{id}/docs/README.md"), "# Docs\n");
        tree.write(
            &format!("{id}/symphony.toml"),
            &format!("[project]\nname = \"{id}\"\nid = \"{id}\"\n"),
        );
        let tasks = tree.path(&format!("{id}/.target/bin/tasks"));
        std::fs::create_dir_all(tasks.parent().unwrap()).unwrap();
        crate::fixture_script::write(&tasks, "#!/bin/sh\nprintf '%s\\n' \"$1\" >> calls\n", false);
    }
    let home = tree.path("home");
    let mut registry = symphony::machine::Registry::default();
    for id in ["alpha", "beta"] {
        registry
            .add(symphony::machine::Entry::of(&tree.path(id)).unwrap())
            .unwrap();
    }
    registry.write(&home).unwrap();
    let started = start(
        Serve {
            home: Some(home),
            root: tree.path("alpha").canonicalize().unwrap(),
            port: 0,
            hosts: vec!["127.0.0.1".into()],
            install: None,
        },
        false,
        hand_off_asked(),
    )
    .await
    .unwrap();
    let served = started.projects.as_ref().unwrap().served();
    let [alpha, beta] = ["alpha", "beta"].map(|id| {
        served
            .iter()
            .find(|served| served.entry.id == id)
            .unwrap()
            .shared
            .clone()
    });
    assert!(Arc::ptr_eq(&alpha, &started.shared));
    assert_eq!(beta.maintaining(), 1, "its sweep");
    let calls =
        |id: &str| std::fs::read_to_string(tree.path(&format!("{id}/calls"))).unwrap_or_default();
    let looked = [alpha.clone(), beta.clone()];
    let (alpha_calls, beta_calls) = (tree.path("alpha/calls"), tree.path("beta/calls"));
    tokio::task::spawn_blocking(move || {
        test_time::until_true(
            "each project swept, each in its own record",
            test_time::backstop(Duration::from_secs(20)),
            || {
                looked.iter().all(|shared| {
                    shared
                        .api
                        .get()
                        .is_some_and(|hub| hub.health.report(hub.now()).sweep.ok > 0)
                }) && [&alpha_calls, &beta_calls].iter().all(|calls| {
                    let calls = std::fs::read_to_string(calls).unwrap_or_default();
                    calls.lines().any(|call| call == "reap")
                })
            },
        )
    })
    .await
    .unwrap()
    .unwrap();
    for id in ["alpha", "beta"] {
        assert!(
            calls(id).lines().all(|call| call == "reap"),
            "{id}: {}",
            calls(id)
        );
    }
    assert_eq!(started.serve_until_stopped(std::future::ready(())).await, 0);
    assert!(beta.stopping());
    assert_eq!(beta.maintaining(), 0, "the stop joined beta's maintenance");
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
/// naming that it dropped it. An unreplaced one's grace is unchanged. The drain is a promise of
/// elapsed time, read here on the runtime's paused clock, which moves only when every task waits
/// on a timer, so the machine's load cannot carry the request past its drain.
#[tokio::test(start_paused = true)]
async fn a_replaced_generation_ends_once_a_request_that_never_finishes_outlasts_its_drain() {
    let shared = Shared::new(
        303,
        1,
        Path::new("/replaced-drain-fixture"),
        None,
        crate::testing::open_gate(),
    );
    shared.replaced.store(true, Ordering::SeqCst);
    let (stages, said) = told();
    let mut started = generation(&shared, stages);
    started.drain = Duration::from_millis(300);
    let (finished, finishing) = tokio::sync::oneshot::channel::<()>();
    started.serving = vec![
        // A request that finishes within the drain is waited for.
        tokio::spawn(async move {
            #[expect(
                clippy::disallowed_methods,
                reason = "no-timed-waits baseline: a test's request made slow by a length of time, not held until its test releases it"
            )]
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = finished.send(());
        }),
        // One whose peer never reads its answer never does.
        tokio::spawn(std::future::pending()),
    ];
    let began = tokio::time::Instant::now();
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
        crate::testing::open_gate(),
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
    let held_at = tokio::time::timeout(BACKSTOP, log.recv())
        .await
        .unwrap_or_else(|_| panic!("{}", stalled("the held generation to name its wait")))
        .unwrap();
    let next = Shared::new(
        202,
        1,
        Path::new("/overlapping-drain-fixture-202"),
        None,
        crate::testing::open_gate(),
    );
    let status = generation(&next, stages_of(202, into_log(send)))
        .serve_until_stopped(std::future::ready(()))
        .await;
    assert_eq!(status, 0);
    release.send(()).unwrap();
    let status = tokio::time::timeout(BACKSTOP, first)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{}",
                stalled("the held generation to return after its request ended")
            )
        })
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
