//! Hold the first board I/O while exercising the published hub and its real read routes.

use super::Hub;
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
use std::time::{Duration, Instant};
use tower::ServiceExt;

struct Held {
    hub: Arc<Hub>,
    release: Option<mpsc::Sender<()>>,
    _tree: Arc<Tree>,
}

impl Held {
    async fn new(succeeds: bool) -> Self {
        let tree = Arc::new(Tree::new());
        let root = repository(&tree);
        let mut store = succeeds
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
                crate::backstop::PERIOD,
                move |_| {
                    if let Some(entered) = entered.take() {
                        let _ = entered.send(());
                    }
                    if let Some(waiting) = waiting.take() {
                        waiting.recv().map_err(|e| e.to_string())?;
                    }
                    store
                        .take()
                        .ok_or_else(|| "initial board fixture failed".into())
                },
            )
        });
        let hub = tokio::time::timeout(Duration::from_secs(30), starting)
            .await
            .expect("hub publication waited on the held board I/O")
            .unwrap()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(30), entering)
            .await
            .expect("initial board I/O did not start")
            .unwrap();
        Self {
            hub,
            release: Some(release),
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
}

impl Drop for Held {
    fn drop(&mut self) {
        self.release.take();
        self.hub.stop();
    }
}

fn request(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header("Host", "127.0.0.1:1")
        .body(Body::empty())
        .unwrap()
}

#[tokio::test]
async fn status_is_ready_while_data_reads_wait_for_the_initial_board_result() {
    for succeeds in [true, false] {
        let mut held = Held::new(succeeds).await;
        let app = router(held.shared()).layer(MockConnectInfo(
            "127.0.0.1:2".parse::<SocketAddr>().unwrap(),
        ));
        let status = app.clone().oneshot(request("/api/front")).await.unwrap();
        assert_eq!(status.status(), StatusCode::OK);
        let status: symphony_api::Front =
            serde_json::from_slice(&to_bytes(status.into_body(), 1024 * 1024).await.unwrap())
                .unwrap();
        assert!(status.ready, "the document hub is published: {status:?}");

        let mut read = Box::pin(app.oneshot(request("/api/tasks")));
        tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Poll::Ready(response) =
                    poll_fn(|cx| Poll::Ready(read.as_mut().poll(cx))).await
                {
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
        .await
        .expect("request did not reach the initial-board barrier");
        held.release();
        let response = tokio::time::timeout(Duration::from_secs(30), read)
            .await
            .unwrap()
            .unwrap();
        let status = response.status();
        let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
        if succeeds {
            assert_eq!(status, StatusCode::OK);
            let tasks: symphony_api::Tasks = serde_json::from_slice(&body).unwrap();
            assert_eq!(tasks.tasks.len(), 1);
            assert_eq!(tasks.tasks[0].slug, "tests/initial-board");
        } else {
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert!(
                String::from_utf8_lossy(&body).contains("initial board fixture failed"),
                "{body:?}"
            );
        }
    }
}

#[tokio::test]
async fn stopping_releases_initial_board_waiters_and_joins_the_owned_read() {
    let mut held = Held::new(false).await;
    let mut waiting = Box::pin(held.hub.wait_initial_board());
    assert!(
        poll_fn(|cx| Poll::Ready(waiting.as_mut().poll(cx)))
            .await
            .is_pending()
    );
    let stopping = held.hub.clone();
    let stopped = tokio::task::spawn_blocking(move || stopping.stop());
    let result = tokio::time::timeout(Duration::from_secs(30), waiting)
        .await
        .unwrap();
    assert_eq!(result.unwrap_err(), "the Symphony server is stopping");
    assert!(!stopped.is_finished(), "the held read was not joined");
    held.release();
    tokio::time::timeout(Duration::from_secs(30), stopped)
        .await
        .unwrap()
        .unwrap();
    assert!(held.hub.worker.lock().unwrap().is_none());
}
