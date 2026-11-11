//! The board's health as the hub records it: a board tick whose dispatch fails warns of it, a good
//! pass clears it, a tick that cannot reach the board is a failed pass, the one server's record says
//! the same, and `/api/health` answers it from the hub's memory, with no board read.

use super::Hub;
use crate::front::Shared;
use crate::gate::Gate;
use crate::testing::{
    Tree,
    board::{Fixture, HOST, NOW, Seed},
    repository,
};
use axum::http::{Request, StatusCode};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::board_health::{Recorder, Report};
use symphony_board::rpc::Client;
use symphony_board::sweep::trigger::now;

/// A loopback relay in front of a board's server that can be cut: cut, it ends every connection it
/// carries and each one made to it, until it is mended.
struct Relay {
    addr: String,
    /// Whether it carries, and the ends of each connection it carries.
    state: Arc<Mutex<(bool, Vec<TcpStream>)>>,
}

impl Relay {
    fn to(server: &str) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let state = Arc::new(Mutex::new((true, Vec::new())));
        let held = Arc::clone(&state);
        let server = server.to_string();
        std::thread::spawn(move || {
            for client in listener.incoming() {
                let Ok(client) = client else { return };
                // Decided and kept under the lock a cut takes, so none it misses is carried.
                let mut state = held.lock().unwrap();
                let upstream = TcpStream::connect(&server);
                let (true, Ok(upstream)) = (state.0, upstream) else {
                    let _ = client.shutdown(Shutdown::Both);
                    continue;
                };
                for (mut from, mut to) in [
                    (client.try_clone().unwrap(), upstream.try_clone().unwrap()),
                    (upstream.try_clone().unwrap(), client.try_clone().unwrap()),
                ] {
                    std::thread::spawn(move || {
                        let _ = std::io::copy(&mut from, &mut to);
                        let _ = to.shutdown(Shutdown::Write);
                    });
                }
                state.1.extend([client, upstream]);
            }
        });
        Relay { addr, state }
    }

    fn cut(&self) {
        let mut state = self.state.lock().unwrap();
        state.0 = false;
        for stream in state.1.drain(..) {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }

    fn mend(&self) {
        self.state.lock().unwrap().0 = true;
    }
}

/// A store on `fixture`'s board through `relay`.
fn through(relay: &Relay, fixture: &Fixture) -> symphony_board::Store {
    let client = Client::new(&relay.addr, "sf", fixture.store.client().database())
        .waiting(Duration::from_secs(10));
    symphony_board::Store::with(client, Box::new(|| NOW), Box::new(|_| false), HOST).unwrap()
}

fn hub(root: &Path, file: &Path) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    hub.health = Recorder::writing(file.to_path_buf());
    hub
}

#[test]
fn a_failed_dispatch_pass_warns_until_a_good_pass_and_the_record_says_the_same() {
    let tree = Tree::new();
    let root = repository(&tree);
    let file = tree.path("sf-tasks/health.json");
    let fixture = Fixture::new(Seed::default().task("tools/clock", "ready"));
    let relay = Relay::to(fixture.store.client().addr());
    let store = through(&relay, &fixture);
    let hub = hub(&root, &file);

    hub.board_read(&store).unwrap();
    let ok = hub.health.report(now());
    assert_eq!(
        (ok.dispatch.ok, ok.dispatch.failed, ok.warnings.len()),
        (1, 0, 0)
    );
    assert!(ok.queries.count > 0, "the tick's requests are counted");
    assert_eq!(Report::read(&file).unwrap().unwrap().dispatch.ok, 1);

    relay.cut();
    assert!(
        hub.board_read(&store).is_err(),
        "a board that cannot be reached fails the tick"
    );
    let failed = hub.health.report(now());
    assert_eq!((failed.dispatch.ok, failed.dispatch.failed), (1, 1));
    assert_eq!(failed.warnings.len(), 1, "{:?}", failed.warnings);
    let error = failed.dispatch.error.clone().unwrap();
    assert!(
        failed.warnings[0].starts_with("callback dispatch failing for ")
            && failed.warnings[0]
                .ends_with(&format!("(1 of 2 passes in the last hour failed): {error}")),
        "{:?}",
        failed.warnings
    );
    assert!(failed.queries.failed > 0, "{:?}", failed.queries);
    let slowest = failed.queries.slowest.unwrap();
    assert!(
        slowest.source.starts_with("crates/tasks/src/"),
        "the board's request is named by its line: {}",
        slowest.source
    );
    let recorded = Report::read(&file).unwrap().unwrap();
    assert_eq!(
        (recorded.dispatch.failed, recorded.warnings.len()),
        (1, 1),
        "the dispatch started failing: the record says so at once"
    );

    relay.mend();
    hub.board_read(&store).unwrap();
    let cleared = hub.health.report(now());
    assert_eq!(
        (
            cleared.dispatch.ok,
            cleared.dispatch.failed,
            cleared.warnings
        ),
        (2, 1, Vec::<String>::new()),
        "a good pass clears the warning; the hour still counts the failure"
    );
    let recorded = Report::read(&file).unwrap().unwrap();
    assert_eq!(
        (recorded.dispatch.ok, recorded.warnings),
        (2, Vec::new()),
        "the dispatch stopped failing: the record says so at once"
    );
}

#[test]
fn a_tick_that_cannot_reach_the_board_is_a_failed_pass() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = hub(&root, &tree.path("health.json"));
    assert!(
        hub.reconnect(&mut |_: &Path| Err("connection refused".to_string()))
            .is_none()
    );
    let report = hub.health.report(now());
    assert_eq!(
        (report.dispatch.failed, report.dispatch.error.as_deref()),
        (
            1,
            Some("no pass, the board could not be reached: connection refused")
        )
    );
    assert_eq!(report.warnings.len(), 1);
}

#[tokio::test]
async fn the_health_route_answers_from_the_hubs_memory() {
    let tree = Tree::new();
    let root = repository(&tree);
    let shared = Shared::new(
        1,
        1234,
        &root,
        None,
        Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
    );
    let ask = |shared: Arc<Shared>| async move {
        let request = Request::builder()
            .uri("/api/health")
            .header("Host", "localhost:1234")
            .body(())
            .unwrap();
        let response = crate::api::answer(&shared, request).await.unwrap();
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        (status, body)
    };
    let (status, _) = ask(shared.clone()).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "no hub yet");

    // A hub whose board never answered: the route reads no board and waits on no first read.
    let hub = Arc::new(Hub::new(&root, &root.join("build/client")));
    hub.health
        .dispatched(now(), Err("the board did not answer"));
    assert!(shared.api.set(hub.clone()).is_ok());
    let (status, body) = ask(shared).await;
    assert_eq!(status, StatusCode::OK);
    let report: Report = serde_json::from_slice(&body).unwrap();
    assert_eq!(
        (report.dispatch.failed, report.dispatch.error.as_deref()),
        (1, Some("the board did not answer"))
    );
    assert_eq!(report.warnings, hub.health.report(report.at).warnings);
}
