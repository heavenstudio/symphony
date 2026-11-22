//! A JSON read's validator: a weak `ETag` naming its body, and `304` with no body for a request
//! that already holds that body (docs/engineering/analysis/client-cache.md, "How freshness is checked").
//!
//! The answer keeps `Cache-Control: no-store`, so the browser holds no copy of its own: the
//! client keeps the body and its tag and sends the tag back itself.

use axum::body::Body;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::Response;
use sha2::Digest;

/// The validator of a JSON body, quoted: the first 16 hex digits of its SHA-256, as a section's
/// `hash` is. It names the bytes the server built, before any compression, so the header sends
/// it weak (`W/`).
pub(super) fn checksum(body: &[u8]) -> String {
    let digest = sha2::Sha256::digest(body);
    let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
    format!("\"{hex}\"")
}

/// The validator a read names its content by, where its body holds more than its content: the
/// route puts it in the response's extensions, and [`validated`] tags the response with it in
/// place of its body's checksum.
#[derive(Clone, Debug)]
pub(super) struct Content(pub(super) String);

/// A document read as it is sent: its validator, its body, and the feed's cursor it was read at.
#[derive(Debug)]
pub(super) struct Tagged {
    pub(super) tag: String,
    pub(super) body: Vec<u8>,
    pub(super) cursor: Option<u64>,
}

/// The validator of a document read, leaving out its `version`: the feed's cursor at the read,
/// which moves with every change on the board, an agent's health included. So a document answers
/// `304` until its own content changes, and the cursor goes in a header of its own
/// ([`symphony_api::VERSION_HEADER`]), which a page drawing its kept copy on a `304` opens the
/// feed from (docs/engineering/analysis/client-cache.md, "A return visit, step by step"). `value` (a document,
/// its outline, sections' bodies, or a folder, whose intro is a document read at the cursor too) is
/// sent as it came, serialized once: the validator is the checksum of those bytes with the cursor
/// `null`, and the cursor is written into them after.
pub(super) fn without_cursor(value: &mut serde_json::Value) -> Tagged {
    fn cursor(value: &mut serde_json::Value) -> Option<&mut serde_json::Value> {
        if value.get("document").is_some() {
            value.get_mut("document")?.get_mut("version")
        } else if value.get("intro").is_some() {
            value.get_mut("intro")?.get_mut("version")
        } else {
            value.get_mut("version")
        }
    }
    let read = cursor(value).map(std::mem::take);
    let mut body = serde_json::to_vec(value).expect("a JSON value serializes");
    let tag = checksum(&body);
    let Some(read) = read else {
        return Tagged {
            tag,
            body,
            cursor: None,
        };
    };
    let number = read.as_u64();
    let written = serde_json::to_vec(&read).expect("a JSON value serializes");
    if let Some(slot) = cursor(value) {
        *slot = read;
    }
    // The cursor's slot is the one `"version":null` of the body, unless the content holds another
    // (a card named `version` with no thread, say): then the body is written again, whole.
    const SLOT: &[u8] = b"\"version\":null";
    let mut found = body
        .windows(SLOT.len())
        .enumerate()
        .filter(|(_, at)| *at == SLOT);
    match (found.next(), found.next()) {
        (Some((at, _)), None) => {
            let null = at + SLOT.len() - b"null".len();
            body.splice(null..null + b"null".len(), written);
        }
        _ => body = serde_json::to_vec(value).expect("a JSON value serializes"),
    }
    Tagged {
        tag,
        body,
        cursor: number,
    }
}

/// The validator of `document`'s outline as `/api/document?view=outline` sends it: what
/// [`without_cursor`] makes of that answer, so a check answered before its body is made names the
/// body as the answer that made it did.
pub(super) fn outline_tag(document: symphony_api::Document) -> String {
    let mut value = serde_json::json!(symphony::docview::outline(document));
    without_cursor(&mut value).tag
}

/// The validator of a task's timeline, leaving out its `now`: the server's clock at the read,
/// which the client moves on by its own clock while the rest stays the same. The page reads the
/// timeline each minute, and is answered `304` until the task's history or its gates change.
/// `value` is left as it came.
pub(super) fn without_now(value: &mut serde_json::Value) -> String {
    let now = value.get_mut("now").map(std::mem::take);
    let tag = checksum(&serde_json::to_vec(value).expect("a JSON value serializes"));
    if let (Some(now), Some(slot)) = (now, value.get_mut("now")) {
        *slot = now;
    }
    tag
}

/// Whether `response` is a JSON read: a `200` the server built as JSON and answered `no-store`
/// with no validator of its own. A file the client or media routes serve carries its own, an
/// error is never a read to keep, and the feed's stream is not JSON.
pub(super) fn is_json_read(response: &Response) -> bool {
    let headers = response.headers();
    response.status() == StatusCode::OK
        && !headers.contains_key(header::ETAG)
        && headers
            .get(header::CACHE_CONTROL)
            .is_some_and(|value| value == "no-store")
        && headers
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| {
                value
                    .split(';')
                    .next()
                    .is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json"))
            })
}

/// `response`, given its validator when it is a JSON read; anything else as it came. The
/// validator is the route's [`Content`] when it names one, else its body's checksum: a JSON
/// read's body is whole in memory already, so collecting it to hash waits on nothing. `asked` is
/// the request's `If-None-Match` values: one that names the validator, by weak comparison, is
/// answered `304` with no body, keeping the validator and `no-store`, and naming what the `200`
/// varies by: the router's compression gzips the body for a request that takes it, and says so
/// with `Vary: accept-encoding`, which it leaves off an empty answer.
pub(super) async fn validated(asked: &[HeaderValue], response: Response) -> Response {
    if !is_json_read(&response) {
        return response;
    }
    let (mut parts, body) = response.into_parts();
    let (tag, body) = match parts.extensions.remove::<Content>() {
        Some(Content(tag)) => (tag, body),
        None => match axum::body::to_bytes(body, usize::MAX).await {
            Ok(bytes) => (checksum(&bytes), Body::from(bytes)),
            Err(why) => return super::error(StatusCode::INTERNAL_SERVER_ERROR, why.to_string()),
        },
    };
    parts.headers.insert(
        header::ETAG,
        HeaderValue::from_str(&format!("W/{tag}")).expect("a quoted hex digest is a header"),
    );
    if super::names_etag(asked, &tag) {
        parts.status = StatusCode::NOT_MODIFIED;
        parts.headers.remove(header::CONTENT_TYPE);
        parts.headers.remove(header::CONTENT_LENGTH);
        let varies = parts.headers.get_all(header::VARY).iter().any(|value| {
            value.to_str().is_ok_and(|names| {
                names
                    .split(',')
                    .any(|name| name.trim().eq_ignore_ascii_case("accept-encoding"))
            })
        });
        if !varies {
            parts
                .headers
                .append(header::VARY, HeaderValue::from_static("accept-encoding"));
        }
        return Response::from_parts(parts, Body::empty());
    }
    Response::from_parts(parts, body)
}

#[cfg(test)]
mod tests;
