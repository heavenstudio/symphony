//! The board's health as the hub records it: a board tick whose dispatch fails warns of it, on the
//! pages' server notice too, a minute of good passes clears it, with no flicker on a board failing
//! every other tick, a tick that cannot reach the board is a failed
//! pass, the one server's record says the same, and `/api/health` answers it from the hub's memory,
//! with no board read.

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
use symphony_board::board_health::{CLEARS_AFTER, RECENT, Recorder, Report};
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

/// The hub's clock, as the test moves it.
#[derive(Clone)]
struct Clock(Arc<Mutex<f64>>);

impl Clock {
    fn at(time: f64) -> Clock {
        Clock(Arc::new(Mutex::new(time)))
    }

    fn now(&self) -> f64 {
        *self.0.lock().unwrap()
    }

    fn pass(&self, seconds: f64) {
        *self.0.lock().unwrap() += seconds;
    }
}

fn hub(root: &Path, file: &Path, clock: &Clock) -> Hub {
    let mut hub = Hub::new(root, &root.join("build/client"));
    hub.health = Recorder::writing(file.to_path_buf());
    let clock = clock.clone();
    hub.clock = Box::new(move || clock.now());
    hub
}

#[test]
fn a_failed_dispatch_pass_warns_until_a_minute_of_good_passes_and_the_record_says_the_same() {
    let tree = Tree::new();
    let root = repository(&tree);
    let file = tree.path("sf-tasks/health.json");
    let fixture = Fixture::new(Seed::default().task("tools/clock", "ready"));
    let relay = Relay::to(fixture.store.client().addr());
    let store = through(&relay, &fixture);
    let clock = Clock::at(now());
    let hub = hub(&root, &file, &clock);

    hub.board_read(&store).unwrap();
    let ok = hub.health.report(clock.now());
    assert_eq!(
        (ok.dispatch.ok, ok.dispatch.failed, ok.warnings.len()),
        (1, 0, 0)
    );
    assert!(ok.queries.count > 0, "the tick's requests are counted");
    assert_eq!(Report::read(&file).unwrap().unwrap().dispatch.ok, 1);
    assert_eq!(*hub.server_notice.borrow(), "", "a well board: no notice");
    assert_eq!(hub.health_notice(), "");

    clock.pass(1.0);
    relay.cut();
    assert!(
        hub.board_read(&store).is_err(),
        "a board that cannot be reached fails the tick"
    );
    let failed = hub.health.report(clock.now());
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
    // Every page of the project hears it on the server's notice, and the picker says it too.
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert_eq!(hub.health_notice(), DISPATCH_FAILING);

    clock.pass(1.0);
    relay.mend();
    hub.board_read(&store).unwrap();
    let recovering = hub.health.report(clock.now());
    assert_eq!((recovering.dispatch.ok, recovering.dispatch.failed), (2, 1));
    assert!(
        recovering.warnings.len() == 1
            && recovering.warnings[0].ends_with(&format!(
                "(1 of 3 passes in the last hour failed), ok again for 0 s: {error}"
            )),
        "one good pass does not clear it: {:?}",
        recovering.warnings
    );
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
    assert_eq!(hub.health_notice(), DISPATCH_FAILING);

    clock.pass(CLEARS_AFTER);
    hub.board_read(&store).unwrap();
    let cleared = hub.health.report(clock.now());
    assert_eq!(
        (
            cleared.dispatch.ok,
            cleared.dispatch.failed,
            cleared.warnings
        ),
        (3, 1, Vec::<String>::new()),
        "a minute of good passes clears the warning; the hour still counts the failure"
    );
    let recorded = Report::read(&file).unwrap().unwrap();
    assert_eq!(
        (recorded.dispatch.ok, recorded.warnings),
        (3, Vec::new()),
        "the dispatch stopped failing: the record says so at once"
    );
    assert_eq!(*hub.server_notice.borrow(), "", "and the pages hear it");
    assert_eq!(hub.health_notice(), "");
}

const DISPATCH_FAILING: &str = "The task board is unwell: its callback dispatch is failing, so \
                                agents may not hear from it. `just tasks status` says why.";

#[test]
fn the_notice_names_what_is_failing_and_nothing_while_the_board_is_well() {
    assert_eq!(super::health::notice([false, false]), "");
    assert_eq!(super::health::notice([true, false]), DISPATCH_FAILING);
    assert_eq!(
        super::health::notice([false, true]),
        "The task board is unwell: its idle sweep is failing. `just tasks status` says why."
    );
    assert_eq!(
        super::health::notice([true, true]),
        "The task board is unwell: its callback dispatch and its idle sweep are failing, so \
         agents may not hear from it. `just tasks status` says why."
    );
}

#[test]
fn the_notice_does_not_flicker_on_a_board_failing_every_other_tick() {
    let tree = Tree::new();
    let root = repository(&tree);
    let clock = Clock::at(now());
    let hub = hub(&root, &tree.path("health.json"), &clock);
    let mut notices = hub.server_notice.subscribe();
    let mut told = Vec::new();
    // A tick a second, as the hub's worker makes them: failing every other one for two minutes,
    // then well.
    for second in 0..240 {
        let outcome = if second < 120 && second % 2 == 0 {
            Err("the board did not answer")
        } else {
            Ok(())
        };
        hub.health.dispatched(hub.now(), outcome);
        hub.tell_health(hub.now());
        if notices.has_changed().unwrap() {
            told.push((second, notices.borrow_and_update().clone()));
        }
        clock.pass(1.0);
    }
    assert_eq!(
        told,
        [(0, DISPATCH_FAILING.to_string()), (119 + 60, String::new())],
        "told once as it starts failing, once a minute after its passes are good again"
    );
}

#[test]
fn a_failed_update_and_the_boards_health_share_the_notice_and_each_clears_alone() {
    let tree = Tree::new();
    let root = repository(&tree);
    let at = now();
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(at));
    let mut notices = hub.server_notice.subscribe();

    // A tick that tells no change sends nothing.
    hub.tell_health(at);
    assert!(
        !notices.has_changed().unwrap(),
        "a well board tells nothing"
    );

    hub.health.dispatched(at, Err("the board did not answer"));
    hub.tell_health(at);
    assert_eq!(*notices.borrow_and_update(), DISPATCH_FAILING);
    hub.health
        .dispatched(at + 1.0, Err("the board did not answer"));
    hub.tell_health(at + 1.0);
    assert!(
        !notices.has_changed().unwrap(),
        "still failing: the pages are not told again"
    );

    hub.tell_update("Symphony server update failed.".into());
    assert_eq!(
        *notices.borrow_and_update(),
        format!("Symphony server update failed. · {DISPATCH_FAILING}")
    );

    hub.health.dispatched(at + 2.0, Ok(()));
    hub.tell_health(at + 2.0);
    assert!(
        !notices.has_changed().unwrap(),
        "one good pass: still failing"
    );
    let later = at + 2.0 + CLEARS_AFTER;
    hub.health.dispatched(later, Ok(()));
    hub.tell_health(later);
    assert_eq!(
        *notices.borrow_and_update(),
        "Symphony server update failed.",
        "the board's health clears, the update's failure stays"
    );

    hub.health.swept(later + 1.0, Err("exit status: 3"));
    hub.tell_health(later + 1.0);
    hub.tell_update(String::new());
    assert_eq!(
        *notices.borrow_and_update(),
        super::health::notice([false, true]),
        "the update's failure clears, the board's health stays"
    );

    // A failed sweep with no sweep since is said for ten minutes, then the tick that sees it
    // older clears it.
    hub.tell_health(later + 1.0 + RECENT + 1.0);
    assert_eq!(*notices.borrow_and_update(), "");
}

#[test]
fn a_tick_that_cannot_reach_the_board_is_a_failed_pass() {
    let tree = Tree::new();
    let root = repository(&tree);
    let hub = hub(&root, &tree.path("health.json"), &Clock::at(now()));
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
    assert_eq!(*hub.server_notice.borrow(), DISPATCH_FAILING);
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
