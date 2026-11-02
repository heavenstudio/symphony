//! Bounded file streams with compression boundaries independent of filesystem read timing.

use axum::body::Body;
use tokio::io::{AsyncRead, AsyncReadExt};

const CHUNK_BYTES: usize = 256 * 1024;

pub(crate) fn stream(reader: impl AsyncRead + Unpin + Send + 'static, length: u64) -> Body {
    Body::from_stream(futures_util::stream::try_unfold(
        (reader, length),
        |(mut reader, remaining)| async move {
            // The gzip encoder flushes on Pending. Yield at every frame boundary, including
            // EOF, so a ready read and one waiting on the filesystem produce the same bytes.
            tokio::task::yield_now().await;
            if remaining == 0 {
                return Ok(None);
            }
            let size = remaining.min(CHUNK_BYTES as u64) as usize;
            let mut chunk = vec![0; size];
            // Coalesce short reads into fixed frames; truncation is an error, and appends do
            // not extend the representation beyond its advertised Content-Length.
            reader.read_exact(&mut chunk).await?;
            Ok::<_, std::io::Error>(Some((chunk, (reader, remaining - size as u64))))
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use std::io::Cursor;

    #[tokio::test]
    async fn frames_stop_at_the_advertised_length_including_empty_and_partial_files() {
        for length in [0, 23, CHUNK_BYTES, CHUNK_BYTES * 2 + 17] {
            let bytes: Vec<_> = (0..length + 31).map(|i| (i % 251) as u8).collect();
            let mut frames = stream(Cursor::new(bytes.clone()), length as u64).into_data_stream();
            let mut received = Vec::new();
            let mut sizes = Vec::new();
            while let Some(frame) = frames.next().await {
                let frame = frame.unwrap();
                sizes.push(frame.len());
                received.extend_from_slice(&frame);
            }
            assert_eq!(received, bytes[..length]);
            assert_eq!(
                sizes,
                bytes[..length]
                    .chunks(CHUNK_BYTES)
                    .map(<[u8]>::len)
                    .collect::<Vec<_>>()
            );
        }
    }

    #[tokio::test]
    async fn truncated_files_report_an_error_instead_of_a_short_success_body() {
        let body = stream(Cursor::new(b"short"), 10);
        let error = axum::body::to_bytes(body, usize::MAX).await.unwrap_err();
        assert!(error.to_string().contains("early eof"), "{error}");
    }
}
