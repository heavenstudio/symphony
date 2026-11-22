//! What a page's feed hears of a board that went: whether it is slow (it took the read and answered
//! late) or unreachable, said again only when it turns from one to the other, whether a board read
//! or the worker's reconnect found it so.

use super::startup_tests::SLOW;
use super::{Event, Hub};
use crate::testing::{
    Tree,
    board::{Fixture, Seed},
    repository,
};
use serde_json::{Value, json};
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// A board's failure on a server that refused the connection.
const REFUSED: &str =
    "the board's database: cannot connect to 127.0.0.1:1: Connection refused (os error 61)";

/// What a page hears of a board that took the request and answered too late.
fn slow_board(error: &str) -> Value {
    json!({"available":false,"slow":true,"error":error})
}

/// What a page hears of a board it cannot reach.
fn unreachable_board(error: &str) -> Value {
    json!({"available":false,"slow":false,"error":error})
}

/// The main page's `tasks-status` and `counts` events published after version `seen`, which moves
/// past them: each status's data, and for counts whether the task counts are gone.
fn board_said(hub: &Hub, seen: &mut u64) -> Vec<Value> {
    let pages = hub.pages.read().unwrap();
    let after = *seen;
    let unseen: Vec<&Event> = pages[""]
        .history
        .iter()
        .filter(|e| e.version > after)
        .collect();
    let mut said = Vec::new();
    for event in unseen {
        *seen = event.version;
        let event: Value = serde_json::from_str(&event.json).unwrap();
        let changes = event["changes"].as_array().unwrap();
        for change in changes {
            match change["region"].as_str().unwrap() {
                "tasks-status" => said.push(change["data"].clone()),
                "counts" => said.push(json!({"counts gone": change["data"]["tasks"].is_null()})),
                _ => {}
            }
        }
    }
    said
}

/// A gone board is said slow or unreachable, said again only when it turns from one to the other,
/// and its counts go once, when it goes.
#[test]
fn a_gone_board_tells_pages_whether_it_is_slow_and_again_only_when_that_turns() {
    let tree = Tree::new();
    let root = repository(&tree);
    let fixture = Fixture::new(Seed::default().task("tools/clock", "ready"));
    let hub = Hub::new(&root, &root.join("build/client"));
    hub.refresh_page("", &root).unwrap();
    let back = [json!({"available":true}), json!({"counts gone":false})];
    let nothing: [Value; 0] = [];
    let mut seen = 0;
    let mut heard = |step: &dyn Fn()| {
        step();
        board_said(&hub, &mut seen)
    };

    assert_eq!(heard(&|| hub.board_read(&fixture.store).unwrap()), back);
    assert_eq!(
        heard(&|| hub.board_error(REFUSED.into())),
        [unreachable_board(REFUSED), json!({"counts gone":true})]
    );
    assert_eq!(
        heard(&|| hub.board_error(format!("{REFUSED}, again"))),
        nothing
    );
    assert_eq!(
        heard(&|| hub.board_error(SLOW.into())),
        [slow_board(SLOW)],
        "an unreachable board turned slow"
    );
    assert_eq!(heard(&|| hub.board_error(SLOW.into())), nothing);
    assert_eq!(
        heard(&|| hub.board_error(REFUSED.into())),
        [unreachable_board(REFUSED)],
        "a slow board turned unreachable"
    );
    assert_eq!(heard(&|| hub.board_read(&fixture.store).unwrap()), back);
    assert_eq!(
        heard(&|| hub.board_error(SLOW.into())),
        [slow_board(SLOW), json!({"counts gone":true})],
        "an answering board turned slow"
    );
}

/// A store, or why the worker's connection to the board failed.
type Connection = Result<symphony_board::Store, String>;

/// A started hub whose worker connects to the board through the test: each connection says it was
/// asked for, then waits for the one the test gives.
struct Worker {
    hub: Arc<Hub>,
    asked: mpsc::Receiver<()>,
    give: Option<mpsc::Sender<Connection>>,
    _tree: Tree,
}

impl Worker {
    /// A hub whose first board read has asked for its connection.
    fn connecting() -> Self {
        let tree = Tree::new();
        let root = repository(&tree);
        let (ask, asked) = mpsc::channel();
        let (give, given) = mpsc::channel();
        let connect = move |_: &Path| -> Connection {
            ask.send(()).map_err(|e| e.to_string())?;
            given
                .recv()
                .map_err(|_| "the test gives no more connections".to_string())?
        };
        // A private hub leaves the chat journal to the shared server.
        let drain_chat = false;
        let hub = Hub::start_watching::<notify::NullWatcher>(
            &root,
            &root.join("build/client"),
            false,
            drain_chat,
            crate::backstop::PERIOD,
            connect,
        )
        .unwrap();
        hub.start_initial_board();
        let worker = Self {
            hub,
            asked,
            give: Some(give),
            _tree: tree,
        };
        worker.asked_again("the first board read never connected");
        worker
    }

    /// Give the waiting connection `connection`, and return once the worker asks for the next: by
    /// then it has told the pages what `connection` meant.
    fn gives(&self, connection: Connection) {
        self.give.as_ref().unwrap().send(connection).unwrap();
        self.asked_again("the worker never connected again after a failed connection");
    }

    /// Only a stall reaches the wait's backstop, and its failure says the load average.
    fn asked_again(&self, why: &str) {
        let backstop = test_time::backstop(Duration::from_secs(30));
        let asked = self.asked.recv_timeout(backstop);
        assert_eq!(
            asked,
            Ok(()),
            "{why}: stalled: the wait's wall-clock backstop of {backstop:?} ran out; {}",
            test_time::Load::now()
        );
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        // A waiting connection fails at once without its sender, so the worker reaches its stop.
        self.give.take();
        self.hub.stop();
    }
}

/// The worker's reconnect after a failed read tells the pages what it found, as the read does: a
/// slow board that then refuses the connection is unreachable, and back.
#[test]
fn the_workers_failed_reconnect_tells_pages_whether_the_board_turned_slow_or_unreachable() {
    let worker = Worker::connecting();
    let mut seen = 0;

    worker.gives(Err(SLOW.into()));
    assert_eq!(
        board_said(&worker.hub, &mut seen),
        [slow_board(SLOW)],
        "the first board read was answered late"
    );
    worker.gives(Err(REFUSED.into()));
    assert_eq!(
        board_said(&worker.hub, &mut seen),
        [unreachable_board(REFUSED)],
        "the slow board refused the reconnect"
    );
    assert_eq!(worker.hub.task_snapshot().unwrap_err(), REFUSED);
    worker.gives(Err(SLOW.into()));
    assert_eq!(
        board_said(&worker.hub, &mut seen),
        [slow_board(SLOW)],
        "the unreachable board answered the reconnect late"
    );
    assert_eq!(worker.hub.task_snapshot().unwrap_err(), SLOW);
}
