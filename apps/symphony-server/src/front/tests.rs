//! The server's gate (loopback names, the tailnet, its own route's Host) and its owned workers'
//! join.

use super::*;
use axum::body::Body;
use axum::extract::connect_info::MockConnectInfo;
use tower::ServiceExt;

/// A confirmed tailnet peer, the one address the gate's `tailscale whois` knows.
const PEER: &str = "100.101.102.103";

#[test]
fn loopback_hosts_accept_exact_names_and_numeric_ports() {
    for name in ["localhost", "LOCALHOST", "127.0.0.1", "[::1]"] {
        for suffix in ["", ":0", ":1", ":80", ":65535", ":00080"] {
            let host = format!("{name}{suffix}");
            assert!(names_loopback(&host), "{host:?}");
        }
    }
}

#[test]
fn loopback_hosts_refuse_foreign_names_and_incomplete_authorities() {
    for host in [
        "",
        "foreign.example",
        "localhost.evil.example",
        "127.0.0.1.evil.example",
        "localhost.",
        "127.1",
        "127.0.0.2",
        "::1",
        "[::1",
        "[::2]",
        "[0:0:0:0:0:0:0:1]",
        "[localhost]",
        "[127.0.0.1]",
        "[[::1]]",
        "http://localhost",
        "user@localhost",
        " localhost",
        "\tlocalhost",
        "ｌocalhost",
    ] {
        assert!(!names_loopback(host), "{host:?}");
    }
    for name in ["localhost", "127.0.0.1", "[::1]"] {
        for suffix in [
            ".evil.example",
            "]",
            " ",
            "\0",
            "/path",
            "?query",
            "#fragment",
            "@evil.example",
            ":",
            ":+1",
            ":-1",
            ": 80",
            ":80 ",
            ":80:90",
            ":65536",
            ":999999999999999999999999",
            ":0x50",
            ":８０",
            ":80/path",
            ":80?query",
            ":80#fragment",
            ":80.evil.example",
            ":80@evil.example",
        ] {
            let host = format!("{name}{suffix}");
            assert!(!names_loopback(&host), "{host:?}");
        }
    }
}

#[test]
fn idle_session_maintenance_starts_only_for_the_shared_installation() {
    let gate = crate::testing::open_gate();
    let home = Path::new("/private-preview-fixture");
    let shared = Shared::new(1, 1, home, Some(home), gate);
    // Stop is sticky, so a started worker exits before any wait or external command.
    shared.stop();
    let private = session_maintenance(&shared, None, MachineClock);
    let installed = session_maintenance(&shared, Some(home), MachineClock);
    let private_started = private.is_some();
    let installed_started = installed.is_some();
    for worker in private.into_iter().chain(installed) {
        worker.join().unwrap();
    }
    assert!(
        !private_started,
        "a private preview must not start a reaper"
    );
    assert!(
        installed_started,
        "the shared installation must start its reaper"
    );
}

#[test]
fn a_server_of_its_own_sweeps_where_its_project_asks_in_its_main_checkout_on_the_folders_board() {
    let none = |_: &str| false;
    let tree = crate::testing::Tree::new();
    // The project `lantern`: its main checkout and a linked worktree, whose files ask nothing.
    let (main, linked) = crate::testing::layouts::repository(&tree.root);
    let bare = tree.path("bare");
    std::fs::create_dir_all(&bare).unwrap();
    assert!(!sweeps_of_its_own(&bare, none), "no project file");
    assert_eq!(own_sweep_line(&bare, false), None, "no project, no line");
    assert!(!sweeps_of_its_own(&main, none), "a file that does not ask");
    let unasked = own_sweep_line(&main, false).unwrap();
    assert!(
        unasked.contains("runs no idle-session sweep") && unasked.contains("own_sweep = true"),
        "{unasked}"
    );
    // A preview's, where the key would change nothing, says nothing.
    assert_eq!(own_sweep_line(&linked, false), None);
    let asks = "[project]\nid = \"lantern\"\n\n[board]\nown_sweep = true\n";
    for checkout in [&main, &linked] {
        std::fs::write(checkout.join(symphony_board::project::FILE), asks).unwrap();
    }
    assert!(sweeps_of_its_own(&main, none), "the project asks");
    assert!(
        own_sweep_line(&main, true)
            .unwrap()
            .contains("runs the idle-session sweep"),
        "the line of a server that sweeps"
    );
    // A linked worktree runs branch code, a preview's: it never sweeps, and says why.
    assert!(!sweeps_of_its_own(&linked, none), "a linked worktree");
    let refused = own_sweep_line(&linked, false).unwrap();
    assert!(
        refused.contains("though [board] own_sweep asks") && refused.contains("linked worktree"),
        "{refused}"
    );
    // A board its environment names is not the folder's: a copy's terminals are another board's.
    for other in OTHER_BOARD {
        assert!(!sweeps_of_its_own(&main, |name| name == other), "{other}");
    }
    // Asked, a server of its own in the main checkout gets the worker the one server gets, and
    // one in a linked worktree none. Stop is sticky, so a started worker exits before any wait.
    let started = |root: &Path| {
        let shared = Shared::new(1, 1, root, None, crate::testing::open_gate());
        shared.stop();
        let worker = session_maintenance_where(&shared, None, MachineClock, none);
        let started = worker.is_some();
        for worker in worker {
            worker.join().unwrap();
        }
        started
    };
    assert!(
        started(&main),
        "a server of its own whose project asks starts its reaper"
    );
    assert!(!started(&linked), "a linked worktree's starts none");
}

#[test]
fn leaving_the_server_scope_joins_its_owned_workers() {
    let shared = Shared::new(
        1,
        1,
        Path::new("/background-scope-fixture"),
        None,
        crate::testing::open_gate(),
    );
    // Each wait on an event here; only a stall reaches this backstop.
    const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));
    let stalled = |what: &str| {
        format!(
            "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
            test_time::Load::now()
        )
    };
    std::thread::scope(|scope| {
        let (entered, stopping) = std::sync::mpsc::channel();
        let (release, finish) = std::sync::mpsc::channel();
        let (finished, completed) = std::sync::mpsc::channel();
        let (returned, stopped) = std::sync::mpsc::channel();
        let working = shared.clone();
        let worker = std::thread::spawn(move || {
            // Only the stop wakes it.
            working.wait_for_stop();
            entered.send(working.stopping()).unwrap();
            // Closing release also lets an early assertion unwind without stranding a worker.
            let _ = finish.recv();
            finished.send(()).unwrap();
        });
        let background = Background {
            shared,
            workers: vec![worker],
        };
        scope.spawn(move || {
            drop(background);
            returned.send(()).unwrap();
        });
        assert!(
            stopping
                .recv_timeout(BACKSTOP)
                .unwrap_or_else(|_| panic!("{}", stalled("the stop to wake the worker")))
        );
        // A negative assertion, never the synchronization: a load can only make it wait longer.
        let early = stopped.recv_timeout(Duration::from_millis(100));
        release.send(()).unwrap();
        if early.is_err() {
            stopped
                .recv_timeout(BACKSTOP)
                .unwrap_or_else(|_| panic!("{}", stalled("the server scope to return")));
        }
        completed
            .recv_timeout(BACKSTOP)
            .unwrap_or_else(|_| panic!("{}", stalled("the worker to finish")));
        assert!(
            matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "the server scope returned before its worker finished: {early:?}"
        );
    });
}

/// The status this server's routes give a GET of `path` from `client` with `host`, behind a gate
/// whose `tailscale whois` knows [`PEER`] alone.
async fn status_from(client: &str, path: &str, host: Option<&str>) -> StatusCode {
    let gate = Gate::new(
        Arc::new(|ip: &str| {
            Ok::<_, String>((ip == PEER).then(|| r#"{"Node": {"Name": "phone"}}"#.to_string()))
        }),
        Arc::new(Instant::now),
    );
    let shared = Shared::new(1, 1, Path::new("/nowhere"), None, gate);
    let app = router(shared).layer(MockConnectInfo(client.parse::<SocketAddr>().unwrap()));
    let mut request = Request::builder().uri(path);
    if let Some(host) = host {
        request = request.header(header::HOST, host);
    }
    let request = request.body(Body::empty()).unwrap();
    app.oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn a_lan_address_or_a_tailnet_stranger_is_refused_before_any_route() {
    let strangers = [
        "192.168.1.20:5000",
        "10.0.0.2:5000",
        "100.64.0.9:5000",
        "[fd7a:115c:a1e0::9]:5000",
        "[::ffff:192.168.1.20]:5000",
    ];
    for client in strangers {
        for path in ["/api/front", "/docs/A.md"] {
            let status = status_from(client, path, Some("127.0.0.1:1")).await;
            assert_eq!(status, StatusCode::FORBIDDEN, "{client} {path}");
        }
    }
}

#[tokio::test]
async fn this_machine_and_a_confirmed_tailnet_peer_are_let_in() {
    for client in [
        "127.0.0.1:5000",
        "[::1]:5000",
        "[::ffff:127.0.0.1]:5000",
        "100.101.102.103:5000",
    ] {
        let status = status_from(client, "/api/front", Some("127.0.0.1:1")).await;
        assert_eq!(status, StatusCode::OK, "{client}");
    }
}

/// `/api/front`, the server's own route, is for the starters on this machine, which ask it by
/// loopback's name: a page another site rebinds to loopback names that site instead, and reads
/// nothing. The app's routes are judged by the app's own Host check, after this server.
#[tokio::test]
async fn the_servers_own_route_answers_only_a_host_naming_loopback() {
    for host in ["127.0.0.1:1", "localhost:1", "[::1]:1", "LOCALHOST"] {
        let status = status_from("127.0.0.1:5000", "/api/front", Some(host)).await;
        assert_eq!(status, StatusCode::OK, "{host}");
    }
    for host in [
        Some("evil.example:1"),
        Some("127.0.0.1.evil.example"),
        Some(""),
        None,
    ] {
        let status = status_from("127.0.0.1:5000", "/api/front", host).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{host:?}");
    }
}
