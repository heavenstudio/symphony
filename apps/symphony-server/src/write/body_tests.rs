//! Bounded JSON collection and the real router's gate-before-body order, on an owned checkout.

use super::*;
use axum::body::Bytes;
use axum::extract::connect_info::MockConnectInfo;
use futures_util::stream;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

async fn refused(response: Response, status: StatusCode, message: &str) {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let bytes = axum::body::to_bytes(response.into_body(), 4096)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        body["error"].as_str().unwrap().starts_with(message),
        "{body}"
    );
}

async fn body_refused(body: Body, status: StatusCode, message: &str) {
    let (code, why) = json_body(body).await.unwrap_err();
    refused(error(code, why), status, message).await;
}

#[tokio::test]
async fn empty_small_and_exactly_capped_json_keep_their_values() {
    for (body, expected) in [
        (Vec::new(), json!({})),
        (b"null".to_vec(), Value::Null),
        (b"{\"text\":\"saved\"}".to_vec(), json!({"text":"saved"})),
        (
            {
                let mut body = b"{}".to_vec();
                body.resize(JSON_BYTES, b' ');
                body
            },
            json!({}),
        ),
    ] {
        assert_eq!(json_body(Body::from(body)).await.unwrap(), expected);
    }
}

#[tokio::test]
async fn a_single_body_over_the_cap_is_a_413_even_when_it_is_valid_json() {
    let mut body = b"{}".to_vec();
    body.resize(JSON_BYTES + 1, b' ');
    body_refused(
        Body::from(body),
        StatusCode::PAYLOAD_TOO_LARGE,
        "JSON saves are at most 2 MiB",
    )
    .await;
}

#[tokio::test]
async fn a_stream_without_a_length_is_stopped_at_the_first_chunk_past_the_cap() {
    let polls = Arc::new(AtomicUsize::new(0));
    let seen = polls.clone();
    let body = Body::from_stream(stream::poll_fn(move |_| {
        let bytes = match seen.fetch_add(1, Ordering::SeqCst) {
            0 => Bytes::from(vec![b' '; JSON_BYTES]),
            1 => Bytes::from_static(b" "),
            _ => panic!("an oversized JSON body must not be polled again"),
        };
        std::task::Poll::Ready(Some(Ok::<_, std::io::Error>(bytes)))
    }));
    body_refused(
        body,
        StatusCode::PAYLOAD_TOO_LARGE,
        "JSON saves are at most 2 MiB",
    )
    .await;
    assert_eq!(polls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn malformed_json_and_transport_errors_stay_bad_requests() {
    let broken = Body::from_stream(stream::iter([Err::<Bytes, _>(std::io::Error::other(
        "fixture body broke off",
    ))]));
    for body in [Body::from("{"), broken] {
        body_refused(body, StatusCode::BAD_REQUEST, "bad JSON:").await;
    }
}

fn app(root: &std::path::Path) -> (Arc<Shared>, axum::Router) {
    let shared = Shared::new(1, 8123, root, None, crate::testing::open_gate());
    let app = crate::front::router(shared.clone()).layer(MockConnectInfo(
        "127.0.0.1:5000".parse::<SocketAddr>().unwrap(),
    ));
    (shared, app)
}

#[tokio::test]
async fn the_router_refuses_oversized_json_before_opening_a_save_store() {
    let tree = crate::testing::Tree::checkout();
    let (shared, app) = app(&tree.root);
    let response = app
        .oneshot(
            Request::post("/api/comment")
                .header(header::HOST, "localhost:8123")
                .body(Body::from(vec![b' '; JSON_BYTES + 1]))
                .unwrap(),
        )
        .await
        .unwrap();
    refused(
        response,
        StatusCode::PAYLOAD_TOO_LARGE,
        "JSON saves are at most 2 MiB",
    )
    .await;
    assert!(shared.saves.get().is_none());
    assert_eq!(tree.names(""), [".git"]);
}

#[tokio::test]
async fn refused_hosts_and_origins_never_poll_a_json_body() {
    let tree = crate::testing::Tree::checkout();
    let (shared, app) = app(&tree.root);
    for (host, origin) in [
        ("foreign.example", "http://foreign.example"),
        ("localhost:8123", "http://foreign.example"),
        ("localhost:8123", "null"),
    ] {
        let body = Body::from_stream(stream::poll_fn(
            |_| -> std::task::Poll<Option<Result<Bytes, std::io::Error>>> {
                panic!("a refused write must not poll its body")
            },
        ));
        let response = app
            .clone()
            .oneshot(
                Request::post("/api/comment")
                    .header(header::HOST, host)
                    .header(header::ORIGIN, origin)
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap();
        refused(response, StatusCode::FORBIDDEN, "Forbidden:").await;
    }
    assert!(shared.saves.get().is_none());
    assert_eq!(tree.names(""), [".git"]);
}
