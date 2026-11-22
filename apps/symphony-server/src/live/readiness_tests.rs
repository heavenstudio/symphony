//! An older writer's scratch board, an already-serving host, and external migration. The real
//! JSON routes recover on the existing retry clock, with no restart and no reader migration.

use super::Hub;
use crate::front::{Shared, router};
use crate::testing::{Tree, board::surreal, repository};
use std::io::{Read, Write};
use std::net::TcpStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use symphony_board::{Opening, Schema, Store, cli::Env};

struct Board(symphony_board::server::Home);

impl Drop for Board {
    fn drop(&mut self) {
        let _ = self.0.stop();
    }
}

fn get(port: u16, path: &str) -> (u16, serde_json::Value) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(test_time::backstop(Duration::from_secs(30))))
        .unwrap();
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n"
    )
    .unwrap();
    let mut answer = String::new();
    stream.read_to_string(&mut answer).unwrap();
    let (headers, body) = answer.split_once("\r\n\r\n").unwrap();
    let status = headers
        .lines()
        .next()
        .unwrap()
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    (
        status,
        serde_json::from_str(body).unwrap_or_else(|_| serde_json::Value::String(body.into())),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn board_json_recovers_after_external_migration_at_the_existing_retry_deadline() {
    let tree = Tree::new();
    let root = repository(&tree);
    let _board = Board(symphony_board::server::Home::in_common(
        &root.join(".git"),
        Some(&surreal()),
    ));
    let env = Env {
        vars: [("SYMPHONY_SURREAL".into(), surreal().display().to_string())].into(),
        cwd: root.clone(),
    };
    let reached = symphony_board::cli::board(&env).unwrap();
    // An installed writer two versions behind the already-running reader.
    let before = Schema::this_build().before().before();
    let writer = Store::open_as(reached.client, &Opening::write().at(before.clone())).unwrap();
    writer
        .create(&symphony_board::NewTask::new(
            "tools/one",
            "Kept through migration",
            "owner",
        ))
        .unwrap();
    let mut hub = Hub::new(&root, &root.join("build/client"));
    let clock = Arc::new(Mutex::new(Instant::now()));
    let held = clock.clone();
    hub.retry_clock = Box::new(move || *held.lock().unwrap());
    let page_env = env.clone();
    hub.connect_reader =
        Box::new(move |root| symphony::taskview::connect_in(root, page_env.clone()));
    hub.refresh_page("", &root).unwrap();
    let hub = Arc::new(hub);
    let mut board = None;
    let mut connect = |root: &std::path::Path| symphony::taskview::connect_in(root, env.clone());
    // Fail often enough to reach the 60s cap without a wall-clock wait.
    for _ in 0..8 {
        hub.board_tick(&mut board, &mut connect);
        *clock.lock().unwrap() += Duration::from_secs(60);
    }
    assert!(board.is_none());
    assert_eq!(writer.board_version().unwrap(), Some(before.version()));

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .unwrap();
    let port = listener.local_addr().unwrap().port();
    let shared = Shared::new(7, port, &root, None, crate::testing::open_gate());
    assert!(shared.api.set(hub.clone()).is_ok());
    let app = router(shared.clone());
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let url = "/api/task-board?counts=1&work=0";
    hub.board_tick(&mut board, &mut connect);
    assert_eq!(
        get(port, "/tasks").0,
        200,
        "the HTML shell is independently reachable"
    );
    let (status, refusal) = get(port, url);
    assert_eq!(status, 503, "{refusal}");
    assert!(
        refusal["error"]
            .as_str()
            .unwrap()
            .contains("behind this build"),
        "{refusal}"
    );
    // A page reader has its own retry, and must not move the board either.
    assert!(hub.reader().is_err());
    assert_eq!(writer.board_version().unwrap(), Some(before.version()));

    // Only the compatible external writer migrates. Its identity replaces its old writer line,
    // exactly as linking that writer then running `tasks migrate` does.
    let mut out = Vec::new();
    let mut err = Vec::new();
    assert_eq!(
        symphony_board::cli::main(
            &["migrate".into()],
            &env,
            &mut std::io::empty(),
            &mut out,
            &mut err
        ),
        0,
        "{}",
        String::from_utf8_lossy(&err)
    );
    assert_eq!(
        writer.board_version().unwrap(),
        Some(Schema::this_build().version())
    );
    *clock.lock().unwrap() += Duration::from_secs(59);
    hub.board_tick(&mut board, &mut connect);
    assert_eq!(
        get(port, url),
        (503, refusal.clone()),
        "the worker retains its refusal until due"
    );
    assert!(
        hub.reader().is_ok(),
        "the page reader's independent deadline passed"
    );
    *clock.lock().unwrap() += Duration::from_secs(1);
    hub.board_tick(&mut board, &mut connect);
    let (status, body) = get(port, url);
    assert_eq!(status, 200, "{body}");
    let listing: symphony_api::TaskListing = serde_json::from_value(body).unwrap();
    assert_eq!(listing.counts, [("ready".into(), 1)]);
    assert_eq!(hub.task_snapshot().unwrap().tasks[0].slug, "tools/one");
    assert_eq!(
        writer.board_version().unwrap(),
        Some(Schema::this_build().version())
    );
    stop.send(()).unwrap();
    server.await.unwrap();
    shared.stop();
    hub.stop();
}
