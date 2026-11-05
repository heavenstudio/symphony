use super::*;
use axum::body::Body;
use tower::ServiceExt;

#[tokio::test]
async fn head_keeps_empty_responses_and_event_streams_bodyless() {
    for (status, content_type, stream, length) in [
        (StatusCode::CONTINUE, "text/plain", false, None),
        (StatusCode::NOT_MODIFIED, "text/plain", false, None),
        (StatusCode::NO_CONTENT, "text/plain", false, None),
        (StatusCode::OK, "text/plain", false, Some("0")),
        (StatusCode::OK, "text/event-stream", true, None),
    ] {
        let app = Router::new()
            .fallback(move || async move {
                let body = if stream {
                    Body::from_stream(futures_util::stream::poll_fn(
                        |_| -> std::task::Poll<Option<Result<Vec<u8>, std::io::Error>>> {
                            panic!("HEAD must not poll the event stream")
                        },
                    ))
                } else {
                    Body::empty()
                };
                (status, [(header::CONTENT_TYPE, content_type)], body)
            })
            .layer(tower_http::compression::CompressionLayer::new())
            .layer(middleware::from_fn(head));
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::HEAD)
                    .header(header::ACCEPT_ENCODING, "gzip")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_LENGTH)
                .map(|v| v.to_str().unwrap()),
            length,
            "{status}: {content_type}"
        );
        assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
        assert!(
            axum::body::to_bytes(response.into_body(), 0)
                .await
                .unwrap()
                .is_empty()
        );
    }
}

#[tokio::test]
async fn head_reports_a_body_encoding_failure_without_success_headers() {
    let app = Router::new()
        .fallback(|| async {
            Body::from_stream(futures_util::stream::once(async {
                Err::<Vec<u8>, _>(std::io::Error::other("download failed"))
            }))
        })
        .layer(tower_http::compression::CompressionLayer::new())
        .layer(middleware::from_fn(head));
    let response = app
        .oneshot(
            Request::builder()
                .method(Method::HEAD)
                .header(header::ACCEPT_ENCODING, "gzip")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(!response.headers().contains_key(header::CONTENT_ENCODING));
    assert_eq!(response.headers()[header::CONTENT_LENGTH], "0");
    assert!(
        axum::body::to_bytes(response.into_body(), 0)
            .await
            .unwrap()
            .is_empty()
    );
}
