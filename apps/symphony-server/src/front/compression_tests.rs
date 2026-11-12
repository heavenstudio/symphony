use super::*;
use axum::body::to_bytes;
use std::pin::Pin;
use std::sync::atomic::AtomicUsize;
use std::task::{Context, Poll};
use tokio::io::{AsyncRead, ReadBuf};
use tower::ServiceExt;

/// Filesystem reads can be short, immediately ready, or wait for a blocking worker.
struct Reads {
    bytes: Vec<u8>,
    position: usize,
    fragment: usize,
    delayed: bool,
    waited: bool,
    waits: Arc<AtomicUsize>,
}

impl AsyncRead for Reads {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.delayed && !self.waited {
            self.waited = true;
            self.waits.fetch_add(1, Ordering::SeqCst);
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        self.waited = false;
        let size = buffer
            .remaining()
            .min(self.fragment)
            .min(self.bytes.len() - self.position);
        buffer.put_slice(&self.bytes[self.position..self.position + size]);
        self.position += size;
        Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn gzip_get_bytes_and_head_length_do_not_depend_on_file_read_scheduling() {
    // The SVG fixture, an exact frame, and multiple frames with a partial tail.
    for length in [6600, 256 * 1024, 2 * 256 * 1024 + 6600] {
        let bytes: Vec<_> = b"<svg>large fixture</svg>"
            .iter()
            .copied()
            .cycle()
            .take(length)
            .collect();
        let mut expected = None;
        for (fragment, delayed) in [(length, false), (173, false), (173, true)] {
            let waits = Arc::new(AtomicUsize::new(0));
            let observed = waits.clone();
            let bytes = bytes.clone();
            let app = Router::new()
                .fallback(move || {
                    let reader = Reads {
                        bytes: bytes.clone(),
                        position: 0,
                        fragment,
                        delayed,
                        waited: false,
                        waits: waits.clone(),
                    };
                    async move {
                        (
                            [
                                (header::CONTENT_TYPE, "image/svg+xml".to_string()),
                                (header::CONTENT_LENGTH, length.to_string()),
                            ],
                            crate::file_body::stream(reader, length as u64),
                        )
                    }
                })
                .layer(tower_http::compression::CompressionLayer::new())
                .layer(middleware::from_fn(head));
            for method in [Method::GET, Method::HEAD] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::builder()
                            .method(method.clone())
                            .header(header::ACCEPT_ENCODING, "br;q=1, gzip;q=0.8")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(response.headers()[header::CONTENT_ENCODING], "gzip");
                let (parts, body) = response.into_parts();
                let body = to_bytes(body, usize::MAX).await.unwrap();
                if method == Method::GET {
                    if let Some(expected) = &expected {
                        assert_eq!(&body, expected, "{length}: {fragment}, delayed={delayed}");
                    } else {
                        expected = Some(body);
                    }
                } else {
                    assert!(body.is_empty());
                    assert_eq!(
                        parts.headers[header::CONTENT_LENGTH],
                        expected.as_ref().unwrap().len().to_string(),
                        "{length}: {fragment}, delayed={delayed}"
                    );
                }
            }
            assert_eq!(observed.load(Ordering::SeqCst) > 0, delayed);
        }
    }
}
