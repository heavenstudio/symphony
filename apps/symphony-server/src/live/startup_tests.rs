//! Hold the first board I/O while exercising the published hub and its real read routes.

use super::Hub;
use super::startup::PANICKED;
use crate::front::{Shared, router};
use crate::gate::Gate;
use crate::testing::{Tree, board::Fixture, board::Seed, repository};
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{Request, StatusCode};
use std::future::{Future, poll_fn};
use std::net::SocketAddr;
use std::sync::{Arc, mpsc};
use std::task::Poll;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tokio::sync::oneshot::error::TryRecvError;
use tower::ServiceExt;

/// How the held first board read ends once released.
#[derive(Clone, Copy, Debug)]
enum Board {
    /// It opens a seeded board holding one task.
    Opens,
    /// It returns an error, as an unavailable board does.
    Fails,
    /// It panics, as a defect in connecting, reading or probing would.
    Panics,
    /// It returns [`SLOW`], as a board that took a read and answered it late, twice, does.
    Slow,
    /// It returns what a project with no board says: no store, and no SurrealDB here to serve one.
    Absent,
}

/// The wall-clock backstop of a wait here on its event: only a stall reaches it (RULES.md
/// `tests/no-test-fails-on-load`).
const BACKSTOP: Duration = test_time::backstop(Duration::from_secs(30));

/// `work` to its end, or a failure that says `what` stalled and the load average then.
async fn within<T>(what: &str, work: impl Future<Output = T>) -> T {
    tokio::time::timeout(BACKSTOP, work)
        .await
        .unwrap_or_else(|_| {
            panic!(
                "{what}: stalled: the wait's wall-clock backstop of {BACKSTOP:?} ran out; {}",
                test_time::Load::now()
            )
        })
}

/// A board read's failure on a server that took the request and gave no answer within the wait.
pub(super) const SLOW: &str = "the board's database: the server at 127.0.0.1:1 is slow: no answer within 3s: read failed on a freshly opened socket after 0 completed requests: Resource temporarily unavailable (os error 35); the request only reads, so it went once more, on a new connection: read failed on a freshly opened socket after 0 completed requests: Resource temporarily unavailable (os error 35); it was not sent a third time";

struct Held {
    hub: Arc<Hub>,
    release: Option<mpsc::Sender<()>>,
    entering: Option<tokio::sync::oneshot::Receiver<()>>,
    _tree: Arc<Tree>,
}

impl Held {
    /// A published hub whose first board read has been activated and is held inside its I/O.
    async fn entered(board: Board) -> Self {
        let mut held = Self::published(board).await;
        held.hub.start_initial_board();
        within(
            "the initial board I/O to start",
            held.entering.take().unwrap(),
        )
        .await
        .unwrap();
        held
    }

    /// A published hub whose first board read has not been activated.
    async fn published(board: Board) -> Self {
        let tree = Arc::new(Tree::new());
        let root = repository(&tree);
        let mut store = matches!(board, Board::Opens)
            .then(|| Fixture::new(Seed::default().task("tests/initial-board", "ready")).store);
        let (entered, entering) = tokio::sync::oneshot::channel();
        let (release, waiting) = mpsc::channel();
        let mut entered = Some(entered);
        let mut waiting = Some(waiting);
        let kept_tree = tree.clone();
        let starting = tokio::task::spawn_blocking(move || {
            let _kept_tree = kept_tree;
            Hub::start_watching::<notify::NullWatcher>(
                &root,
                &root.join("build/client"),
                false,
                false,
                crate::backstop::PERIOD,
                move |_| {
                    if let Some(entered) = entered.take() {
                        let _ = entered.send(());
                    }
                    if let Some(waiting) = waiting.take() {
                        waiting.recv().map_err(|e| e.to_string())?;
                    }
                    if matches!(board, Board::Panics) {
                        panic!("the held first board read panicked");
                    }
                    if matches!(board, Board::Slow) {
                        return Err(SLOW.into());
                    }
                    if matches!(board, Board::Absent) {
                        return Err(symphony_board::server::ServerError::NoSurreal(
                            "/nowhere/surreal".into(),
                        )
                        .to_string());
                    }
                    store
                        .take()
                        .ok_or_else(|| "initial board fixture failed".into())
                },
            )
        });
        // A publication that waited on the held board I/O never ends: the read is released only
        // once this returns.
        let hub = within("the hub's publication beside the held board I/O", starting)
            .await
            .unwrap()
            .unwrap();
        Self {
            hub,
            release: Some(release),
            entering: Some(entering),
            _tree: tree,
        }
    }

    fn release(&mut self) {
        self.release.take().unwrap().send(()).unwrap();
    }

    fn shared(&self) -> Arc<Shared> {
        let shared = Shared::new(
            1,
            1,
            &self.hub.root,
            None,
            Gate::new(Arc::new(|_| Ok(None)), Arc::new(Instant::now)),
        );
        assert!(shared.api.set(self.hub.clone()).is_ok());
        shared
    }

    /// Whether the owned worker has ended, or `None` once it has been joined.
    fn worker_finished(&self) -> Option<bool> {
        self.hub
            .worker
            .lock()
            .unwrap()
            .as_ref()
            .map(JoinHandle::is_finished)
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release.take();
        self.hub.start_initial_board();
        self.hub.stop();
    }
}

#[tokio::test]
async fn a_published_hub_stops_and_joins_without_opening_its_unactivated_board() {
    let mut held = Held::published(Board::Fails).await;
    assert_eq!(held.worker_finished(), Some(false));
    assert_eq!(
        held.entering.as_mut().unwrap().try_recv(),
        Err(TryRecvError::Empty)
    );
    held.release();
    let stopping = held.hub.clone();
    let stopped = tokio::task::spawn_blocking(move || stopping.stop());
    within("the stop", stopped).await.unwrap();
    assert_eq!(
        held.entering.as_mut().unwrap().try_recv(),
        Err(TryRecvError::Closed)
    );
    assert_eq!(held.worker_finished(), None);
}

fn request(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("Host", "127.0.0.1:1")
        .body(Body::empty())
        .unwrap()
}

/// `/api/tasks` asked while the held first read is inside its I/O, answered once it is released.
async fn tasks_read_across_the_held_board(held: &mut Held) -> (StatusCode, Vec<u8>) {
    let app = router(held.shared()).layer(MockConnectInfo(
        "127.0.0.1:2".parse::<SocketAddr>().unwrap(),
    ));
    let status = app.clone().oneshot(request("/api/front")).await.unwrap();
    assert_eq!(status.status(), StatusCode::OK);
    let status: symphony_api::Front =
        serde_json::from_slice(&to_bytes(status.into_body(), 1024 * 1024).await.unwrap()).unwrap();
    assert!(status.ready, "the document hub is published: {status:?}");

    let mut read = Box::pin(app.oneshot(request("/api/tasks")));
    within("the request to reach the initial-board barrier", async {
        loop {
            if let Poll::Ready(response) = poll_fn(|cx| Poll::Ready(read.as_mut().poll(cx))).await {
                panic!(
                    "data answered before its initial board result: {}",
                    response.unwrap().status()
                );
            }
            if held.hub.initial_board.receiver_count() > 0 {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await;
    held.release();
    let response = within("the read's answer once the first board read ended", read)
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (status, body.to_vec())
}

#[tokio::test]
async fn status_is_ready_while_data_reads_wait_for_the_initial_board_result() {
    let mut held = Held::entered(Board::Opens).await;
    let (status, body) = tasks_read_across_the_held_board(&mut held).await;
    assert_eq!(status, StatusCode::OK);
    let tasks: symphony_api::Tasks = serde_json::from_slice(&body).unwrap();
    assert_eq!(tasks.tasks.len(), 1);
    assert_eq!(tasks.tasks[0].slug, "tests/initial-board");

    for (board, error) in [
        (Board::Fails, "initial board fixture failed"),
        (Board::Panics, PANICKED),
    ] {
        let mut held = Held::entered(board).await;
        let (status, body) = tasks_read_across_the_held_board(&mut held).await;
        assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE, "{board:?}");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains(error), "{board:?}: {body}");
        assert!(body.contains("the task board cannot be reached"), "{body}");
    }

    let mut held = Held::entered(Board::Slow).await;
    let (status, body) = tasks_read_across_the_held_board(&mut held).await;
    assert_eq!(status, StatusCode::GATEWAY_TIMEOUT);
    let body = String::from_utf8_lossy(&body);
    assert!(
        body.contains(&format!("the task board is slow: {SLOW}")),
        "{body}"
    );
    assert!(!body.contains("cannot be reached"), "{body}");
}

#[tokio::test]
async fn stopping_releases_initial_board_waiters_and_joins_the_owned_read() {
    let mut held = Held::entered(Board::Fails).await;
    let mut waiting = Box::pin(held.hub.wait_initial_board());
    assert_eq!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx))).await,
        Poll::Pending
    );
    let stopping = held.hub.clone();
    let mut stopped = tokio::task::spawn_blocking(move || stopping.stop());
    let result = within("the stop to release the initial board's waiter", waiting).await;
    assert_eq!(result, Err("the Symphony server is stopping".into()));
    assert_eq!(
        poll_fn(|cx| Poll::Ready(std::pin::Pin::new(&mut stopped).poll(cx).map(|r| r.is_ok())))
            .await,
        Poll::Pending,
        "the held read was not joined"
    );
    held.release();
    within("the stop to join the released read", stopped)
        .await
        .unwrap();
    assert_eq!(held.worker_finished(), None);
}

/// Whether the project has a board is not known until the hub's first board read is done: then a
/// board read, or one that cannot be reached now (unwell, but there), is one, and a project with no
/// store and no SurrealDB to serve one has none. The server's maintenance of the project waits on it.
#[tokio::test]
async fn a_hub_knows_whether_its_project_has_a_board_once_its_first_read_is_done() {
    for (board, has) in [
        (Board::Opens, true),
        (Board::Fails, true),
        (Board::Slow, true),
        (Board::Absent, false),
    ] {
        let mut held = Held::entered(board).await;
        assert_eq!(held.hub.has_board(), None, "{board:?}: its read is held");
        held.release();
        // The read ends on the hub's own thread, which needs nothing of this test's runtime.
        let known = test_time::until(
            format!("{board:?}: the first board read done"),
            BACKSTOP,
            || held.hub.has_board(),
        )
        .unwrap();
        assert_eq!(known, has, "{board:?}");
    }
}
