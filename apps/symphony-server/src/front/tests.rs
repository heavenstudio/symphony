//! The server's gate (loopback names, the tailnet, its own route's Host), its owned workers'
//! join, and the orphan census's status and stop.

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
    let gate = Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now));
    let home = Path::new("/private-preview-fixture");
    let shared = Shared::new(1, 1, home, Some(home), gate);
    // Stop is sticky, so a started worker exits before any wait or external command.
    shared.stop();
    let private = session_maintenance(&shared, None, Stamp::now);
    let installed = session_maintenance(&shared, Some(home), Stamp::now);
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
fn leaving_the_server_scope_joins_its_owned_workers() {
    let shared = Shared::new(
        1,
        1,
        Path::new("/background-scope-fixture"),
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    std::thread::scope(|scope| {
        let (entered, stopping) = std::sync::mpsc::channel();
        let (release, finish) = std::sync::mpsc::channel();
        let (finished, completed) = std::sync::mpsc::channel();
        let (returned, stopped) = std::sync::mpsc::channel();
        let working = shared.clone();
        let worker = std::thread::spawn(move || {
            working.wait(Duration::from_secs(60));
            entered.send(working.stopping()).unwrap();
            // Closing release also lets an early assertion unwind without stranding a worker.
            let _ = finish.recv();
            finished.send(()).unwrap();
        });
        let background = Background {
            shared,
            workers: vec![worker],
            census: None,
        };
        scope.spawn(move || {
            drop(background);
            returned.send(()).unwrap();
        });
        assert!(stopping.recv_timeout(Duration::from_secs(30)).unwrap());
        let early = stopped.recv_timeout(Duration::from_millis(100));
        release.send(()).unwrap();
        if early.is_err() {
            stopped.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        completed.recv_timeout(Duration::from_secs(30)).unwrap();
        assert!(
            matches!(early, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "the server scope returned before its worker finished: {early:?}"
        );
    });
}

#[tokio::test]
async fn the_status_is_uncounted_until_the_first_census_ends_counted_or_not() {
    // None is a census that could not read the process table: it ends the wait all the same.
    for count in [Some(3), None] {
        let shared = Shared::new(
            1,
            1,
            Path::new("/census-fixture"),
            None,
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        // Each round answers what this test sends, and waits until it does.
        let (send, rounds) = tokio::sync::mpsc::unbounded_channel::<Option<usize>>();
        let rounds = Arc::new(tokio::sync::Mutex::new(rounds));
        let background = Background {
            shared: shared.clone(),
            workers: vec![],
            census: Some(census_worker(shared.clone(), move || {
                let rounds = rounds.clone();
                async move { rounds.lock().await.recv().await.flatten() }
            })),
        };
        let held = shared.status();
        assert!(held.uncounted, "counted before its census ended: {held:?}");
        assert!(held.census_pending, "the starter needs the pending event");
        assert_eq!(held.orphans, 0);
        send.send(count).unwrap();
        let began = Instant::now();
        let counted = loop {
            let status = shared.status();
            if !status.uncounted {
                break status;
            }
            assert!(
                began.elapsed() < Duration::from_secs(15),
                "{count:?}: the first census never ended"
            );
            tokio::task::yield_now().await;
        };
        assert_eq!(counted.orphans, count.unwrap_or(0), "{count:?}");
        assert!(!counted.census_pending);
        drop(send);
        drop(background);
    }
}

#[tokio::test]
async fn a_server_stopped_before_the_census_starts_launches_no_scan() {
    let shared = Shared::new(
        1,
        1,
        Path::new("/stopped-census-fixture"),
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    shared.stop();
    census_worker(shared.clone(), || async {
        panic!("a stopped server must not launch a census")
    })
    .await
    .unwrap();
    let status = shared.status();
    assert!(status.uncounted);
    assert!(!status.census_pending);
}

#[tokio::test]
async fn stopping_a_pending_census_cancels_its_scan_without_a_result() {
    let shared = Shared::new(
        1,
        1,
        Path::new("/cancelled-census-fixture"),
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let entered = Arc::new(tokio::sync::Notify::new());
    let started = entered.clone();
    let census = census_worker(shared.clone(), move || {
        let started = started.clone();
        async move {
            started.notify_one();
            std::future::pending().await
        }
    });
    entered.notified().await;
    assert!(shared.status().census_pending);
    shared.stop();
    census.await.unwrap();
    let status = shared.status();
    assert!(status.uncounted, "the scan returned no result");
    assert!(
        !status.census_pending,
        "the stop must cancel the pending scan"
    );
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
